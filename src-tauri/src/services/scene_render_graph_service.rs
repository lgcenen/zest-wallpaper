use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use serde_json::Value;

use crate::{
    models::{
        EvaluatedSceneObject, SceneAnimationLayer, SceneAssetKind, SceneRuntimeDocument,
        SceneVisualEffect, SceneVisualLayer,
    },
    services::{
        scene_mdl_service::parse_scene_mdl_file,
        scene_render_planner_service::{
            parse_scene_color, parse_visual_blend_mode, SceneRenderBlendMode, SceneRenderColor,
            SceneRenderQuad, SceneRenderSourceKind,
        },
        scene_resource_service::{
            SceneResourceLookup, SceneResourceResolver, SceneResourceRootKind,
            SceneShaderSourceKind,
        },
        scene_shader_material_service::{
            inspect_scene_material_summary, inspect_scene_shader_source_with_effect_context,
            load_scene_effect_plan, load_scene_material_plan,
            load_scene_material_plan_with_effect_package_root, phase10b_blocked_effect_reason,
            phase10b_effect_contract_for_kind, phase10b_supported_effect_contract_for_shader_ref,
            shader_ref_requires_phase10_graph_semantics, SceneEffectBinding, SceneEffectPlan,
            SceneMaterialPassPlan, SceneMaterialSummary, SceneMaterialUniformValue,
            ScenePhase10bBindingSemantic, ScenePhase10bEffectContract, SceneResolvedMaterialPlan,
            SceneShaderProgramKind,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SceneGraphIssueSeverity {
    Warning,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SceneGraphIssueCode {
    MissingVisualSource,
    MissingMaterial,
    MissingPuppet,
    InvalidPuppet,
    InvalidMaterial,
    MissingTextureBinding,
    InvalidEffect,
    GraphTargetMissing,
    GraphInputMissing,
    GraphCycleOrOrderInvalid,
    GraphCopybackgroundUnavailable,
    GraphMaskTargetMissing,
    GraphConstructionIncomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneGraphIssue {
    pub severity: SceneGraphIssueSeverity,
    pub code: SceneGraphIssueCode,
    pub diagnostic_code: Option<&'static str>,
    pub message: String,
    pub object_id: Option<u32>,
    pub object_name: Option<String>,
    pub resource_path: Option<String>,
    pub detail: Option<String>,
    pub resource_lookup: Option<SceneResourceLookup>,
    pub resource_present_but_unsupported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePhase10EffectNode {
    pub effect_path: PathBuf,
    pub passes: Vec<ScenePhase10EffectPassNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenePhase10InputSource {
    LocalCurrentVisual,
    PreviousPass,
    Background,
    CopiedBackground,
    NamedTarget(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePhase10InputBinding {
    pub slot: usize,
    pub source: ScenePhase10InputSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePhase10EffectPassNode {
    pub index: usize,
    pub bindings: Vec<SceneEffectBinding>,
    pub target_name: Option<String>,
    pub copy_background: bool,
    pub input_bindings: Vec<ScenePhase10InputBinding>,
    pub constants: BTreeMap<String, SceneMaterialUniformValue>,
    pub texture_overrides: Vec<Option<PathBuf>>,
    pub material_passes: Vec<SceneMaterialPassPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenePhase10VisualPlan {
    pub object_id: u32,
    pub object_name: String,
    pub quad: SceneRenderQuad,
    pub base_color: SceneRenderColor,
    pub base_source_kind: Option<SceneRenderSourceKind>,
    pub authored_size: [f64; 2],
    pub world_position: [f64; 3],
    pub world_scale: [f64; 3],
    pub world_angles: [f64; 3],
    pub blend_mode: SceneRenderBlendMode,
    pub base_texture_path: Option<PathBuf>,
    pub material: SceneResolvedMaterialPlan,
    pub puppet_path: Option<PathBuf>,
    pub animation_layers: Vec<SceneAnimationLayer>,
    pub effect_chain: Vec<ScenePhase10EffectNode>,
    pub submesh_count: usize,
    pub mask_binding_count: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScenePhase10GraphPlan {
    pub visuals: Vec<ScenePhase10VisualPlan>,
    pub consumed_visual_ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenePhase10GraphReport {
    pub graph: ScenePhase10GraphPlan,
    pub issues: Vec<SceneGraphIssue>,
}

impl ScenePhase10GraphReport {
    pub fn is_blocked(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == SceneGraphIssueSeverity::Fatal)
    }

    pub fn warnings(&self) -> Vec<SceneGraphIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == SceneGraphIssueSeverity::Warning)
            .cloned()
            .collect()
    }

    pub fn fatal_errors(&self) -> Vec<SceneGraphIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == SceneGraphIssueSeverity::Fatal)
            .cloned()
            .collect()
    }
}

pub fn build_scene_phase10_graph(
    scene: &SceneRuntimeDocument,
    resolver: &SceneResourceResolver,
) -> ScenePhase10GraphReport {
    let source_layers = scene
        .source
        .visual_layers
        .iter()
        .map(|layer| (layer.id, layer))
        .collect::<BTreeMap<_, _>>();
    let mut issues = Vec::new();
    let mut visuals = Vec::new();
    let mut consumed = BTreeSet::new();

    for object_id in &scene.evaluated.render_list {
        let Some(EvaluatedSceneObject::Visual {
            base,
            asset_kind,
            asset_path,
            blend_mode,
            color,
            brightness,
            animation_layers,
            angles,
            ..
        }) = scene.evaluated.objects.get(object_id)
        else {
            continue;
        };
        if !base.visible || base.opacity <= 0.001 {
            continue;
        }
        let Some(source) = source_layers.get(object_id).copied() else {
            issues.push(SceneGraphIssue {
                severity: SceneGraphIssueSeverity::Warning,
                code: SceneGraphIssueCode::MissingVisualSource,
                diagnostic_code: None,
                message: format!(
                    "\"{}\" has evaluated visual state but no matching source visual layer for phase-10 graph planning.",
                    base.name
                ),
                object_id: Some(base.id),
                object_name: Some(base.name.clone()),
                resource_path: None,
                detail: None,
                resource_lookup: None,
                resource_present_but_unsupported: false,
            });
            continue;
        };

        let baseline_visual_output =
            source_has_baseline_visual_output(source, asset_kind.clone(), asset_path.as_deref());
        let material_summary = source
            .material_path
            .as_deref()
            .map(|material_path| inspect_scene_material_summary(resolver, material_path));
        let (source_effect_chain, source_effect_issues) = build_source_effect_chain(
            source,
            resolver,
            base.id,
            &base.name,
            baseline_visual_output,
        );
        issues.extend(source_effect_issues);

        let requires_phase10_from_source = source_requires_phase10_from_visual_source(
            source,
            asset_kind.clone(),
            asset_path.as_deref(),
            material_summary
                .as_ref()
                .and_then(|result| result.as_ref().ok()),
        ) || !source_effect_chain.is_empty();

        if let Some(Err(error)) = material_summary.as_ref() {
            if !requires_phase10_from_source && baseline_visual_output {
                continue;
            }
            issues.push(SceneGraphIssue {
                severity: SceneGraphIssueSeverity::Fatal,
                code: SceneGraphIssueCode::InvalidMaterial,
                diagnostic_code: None,
                message: format!(
                    "\"{}\" could not build a phase-10 material plan.",
                    base.name
                ),
                object_id: Some(base.id),
                object_name: Some(base.name.clone()),
                resource_path: source.material_path.clone(),
                detail: Some(error.clone()),
                resource_lookup: None,
                resource_present_but_unsupported: false,
            });
            continue;
        }

        if !requires_phase10_from_source {
            continue;
        }
        let Some(quad) = base
            .transform
            .render_bounds
            .or(source.render_bounds)
            .filter(|bounds| bounds[2] > 0.0 && bounds[3] > 0.0)
            .map(|[left, top, width, height]| SceneRenderQuad {
                left,
                top,
                width,
                height,
                rotation: base.transform.rotation,
                opacity: base.opacity,
                flip_x: base.transform.scale[0].is_sign_negative(),
                flip_y: base.transform.scale[1].is_sign_negative(),
            })
        else {
            continue;
        };
        let authored_size = source
            .size
            .filter(|size| size[0].abs() > 0.001 && size[1].abs() > 0.001)
            .unwrap_or([quad.width.abs().max(0.001), quad.height.abs().max(0.001)]);

        let material = if let Some(material_path) = source.material_path.as_deref() {
            match load_scene_material_plan(resolver, material_path) {
                Ok(material) => material,
                Err(error) => {
                    issues.push(SceneGraphIssue {
                        severity: SceneGraphIssueSeverity::Fatal,
                        code: SceneGraphIssueCode::InvalidMaterial,
                        diagnostic_code: None,
                        message: format!(
                            "\"{}\" could not build a phase-10 material plan.",
                            base.name
                        ),
                        object_id: Some(base.id),
                        object_name: Some(base.name.clone()),
                        resource_path: Some(material_path.to_string()),
                        detail: Some(error),
                        resource_lookup: None,
                        resource_present_but_unsupported: false,
                    });
                    continue;
                }
            }
        } else if !source_effect_chain.is_empty() {
            effect_only_material_plan(base.id)
        } else {
            issues.push(SceneGraphIssue {
                severity: SceneGraphIssueSeverity::Fatal,
                code: SceneGraphIssueCode::MissingMaterial,
                diagnostic_code: None,
                message: format!(
                    "\"{}\" enters the phase-10 native Scene graph without a material path.",
                    base.name
                ),
                object_id: Some(base.id),
                object_name: Some(base.name.clone()),
                resource_path: None,
                detail: Some(
                    "Phase-10 material and effect planning requires material_path for every deep visual."
                        .to_string(),
                ),
                resource_lookup: None,
                resource_present_but_unsupported: false,
            });
            continue;
        };
        for pass in &material.passes {
            for texture in &pass.textures {
                let Some(texture_name) = texture.texture_name.as_ref() else {
                    continue;
                };
                if texture.resolved_path.is_none() {
                    issues.push(SceneGraphIssue {
                        severity: SceneGraphIssueSeverity::Warning,
                        code: SceneGraphIssueCode::MissingTextureBinding,
                        diagnostic_code: None,
                        message: format!(
                            "\"{}\" is missing material texture {} for phase-10 graph execution.",
                            base.name, texture_name
                        ),
                        object_id: Some(base.id),
                        object_name: Some(base.name.clone()),
                        resource_path: Some(texture_name.clone()),
                        detail: Some(format!(
                            "slot {} has no resolved texture candidate",
                            texture.slot_name
                        )),
                        resource_lookup: None,
                        resource_present_but_unsupported: false,
                    });
                }
            }
        }
        if source_effect_chain.is_empty()
            && !source_requires_phase10_graph(source.puppet_path.is_some(), &material)
        {
            continue;
        }

        let mut puppet_path = None;
        let mut submesh_count = 0;
        let mut mask_binding_count = 0;
        if let Some(raw_puppet_path) = source.puppet_path.as_deref() {
            let Some(resolved_puppet_path) = resolver.resolve_relative_path(raw_puppet_path) else {
                issues.push(SceneGraphIssue {
                    severity: SceneGraphIssueSeverity::Fatal,
                    code: SceneGraphIssueCode::MissingPuppet,
                    diagnostic_code: None,
                    message: format!(
                        "\"{}\" references puppet {} but the file is missing.",
                        base.name, raw_puppet_path
                    ),
                    object_id: Some(base.id),
                    object_name: Some(base.name.clone()),
                    resource_path: Some(raw_puppet_path.to_string()),
                    detail: None,
                    resource_lookup: None,
                    resource_present_but_unsupported: false,
                });
                continue;
            };
            match parse_scene_mdl_file(&resolved_puppet_path) {
                Ok(document) => {
                    submesh_count = document.submeshes.len();
                    mask_binding_count = document.mask_bindings.len();
                }
                Err(error) => {
                    issues.push(SceneGraphIssue {
                        severity: SceneGraphIssueSeverity::Fatal,
                        code: SceneGraphIssueCode::InvalidPuppet,
                        diagnostic_code: None,
                        message: format!(
                            "\"{}\" could not parse puppet data for phase-10 native rendering.",
                            base.name
                        ),
                        object_id: Some(base.id),
                        object_name: Some(base.name.clone()),
                        resource_path: Some(resolved_puppet_path.display().to_string()),
                        detail: Some(error),
                        resource_lookup: None,
                        resource_present_but_unsupported: false,
                    });
                    continue;
                }
            }
            puppet_path = Some(resolved_puppet_path);
        }

        let Some(material_effect_chain) =
            build_material_effect_chain(&material, resolver, base.id, &base.name, &mut issues)
        else {
            continue;
        };
        let mut effect_chain = material_effect_chain;
        effect_chain.extend(source_effect_chain);

        let mut world_angles = angles.unwrap_or([0.0, 0.0, 0.0]);
        world_angles[2] = base.transform.rotation;

        visuals.push(ScenePhase10VisualPlan {
            object_id: base.id,
            object_name: base.name.clone(),
            quad,
            base_color: visual_base_color(color.as_deref(), *brightness, base.opacity),
            base_source_kind: match asset_kind {
                SceneAssetKind::Image => Some(SceneRenderSourceKind::Image),
                SceneAssetKind::Video => Some(SceneRenderSourceKind::Video),
                SceneAssetKind::System | SceneAssetKind::Unsupported => None,
            },
            authored_size,
            world_position: base.transform.position,
            world_scale: base.transform.scale,
            world_angles,
            blend_mode: parse_visual_blend_mode(blend_mode.as_deref(), source.color_blend_mode),
            base_texture_path: asset_path.as_deref().and_then(|path| {
                resolver
                    .resolve_relative_path(path)
                    .or_else(|| Some(PathBuf::from(path)))
            }),
            material,
            puppet_path,
            animation_layers: animation_layers.clone(),
            effect_chain,
            submesh_count,
            mask_binding_count,
        });
        consumed.insert(base.id);
    }

    ScenePhase10GraphReport {
        graph: ScenePhase10GraphPlan {
            visuals,
            consumed_visual_ids: consumed.into_iter().collect(),
        },
        issues,
    }
}

fn source_requires_phase10_graph(has_puppet: bool, material: &SceneResolvedMaterialPlan) -> bool {
    has_puppet
        || !material.material_effects.is_empty()
        || material.passes.len() > 1
        || material
            .passes
            .iter()
            .any(material_pass_requires_phase10_graph)
}

fn material_pass_requires_phase10_graph(pass: &SceneMaterialPassPlan) -> bool {
    pass.program.kind != SceneShaderProgramKind::Sprite
        || pass.textures.len() > 1
        || !pass.combos.is_empty()
        || !pass.effect_paths.is_empty()
}

fn effect_only_material_plan(object_id: u32) -> SceneResolvedMaterialPlan {
    SceneResolvedMaterialPlan {
        material_path: PathBuf::from(format!("<effect-only:{object_id}>")),
        passes: Vec::new(),
        material_effects: Vec::new(),
    }
}

fn source_requires_phase10_from_visual_source(
    source: &SceneVisualLayer,
    asset_kind: SceneAssetKind,
    asset_path: Option<&str>,
    material_summary: Option<&SceneMaterialSummary>,
) -> bool {
    source.puppet_path.is_some()
        || !source_has_baseline_visual_output(source, asset_kind, asset_path)
        || source.texture_names.len() > 1
        || source
            .shader_path
            .as_deref()
            .map(shader_ref_requires_phase10_graph_semantics)
            .unwrap_or(false)
        || material_summary
            .map(|summary| summary.requires_phase10_graph)
            .unwrap_or(false)
}

fn source_has_baseline_visual_output(
    source: &SceneVisualLayer,
    asset_kind: SceneAssetKind,
    asset_path: Option<&str>,
) -> bool {
    matches!(
        asset_kind,
        SceneAssetKind::Image | SceneAssetKind::Video | SceneAssetKind::System
    ) && asset_path
        .map(|path| !path.trim().is_empty())
        .unwrap_or(false)
        && !source
            .shader_path
            .as_deref()
            .map(shader_ref_requires_phase10_graph_semantics)
            .unwrap_or(false)
}

fn visual_base_color(
    color: Option<&str>,
    brightness: Option<f64>,
    opacity: f64,
) -> SceneRenderColor {
    let brightness = brightness.unwrap_or(1.0).clamp(0.0, 4.0);
    let mut base = parse_scene_color(color, opacity);
    if (brightness - 1.0).abs() > f64::EPSILON {
        base.red = ((base.red as f64) * brightness).round().clamp(0.0, 255.0) as u8;
        base.green = ((base.green as f64) * brightness).round().clamp(0.0, 255.0) as u8;
        base.blue = ((base.blue as f64) * brightness).round().clamp(0.0, 255.0) as u8;
    }
    base
}

fn build_material_effect_chain(
    material: &SceneResolvedMaterialPlan,
    resolver: &SceneResourceResolver,
    object_id: u32,
    object_name: &str,
    issues: &mut Vec<SceneGraphIssue>,
) -> Option<Vec<ScenePhase10EffectNode>> {
    let mut effect_chain = Vec::new();
    for effect in &material.material_effects {
        let Some(node) = build_effect_node(
            effect,
            resolver,
            object_id,
            object_name,
            SceneGraphIssueSeverity::Fatal,
            None,
            !effect_chain.is_empty(),
            issues,
        ) else {
            return None;
        };
        effect_chain.push(node);
    }
    Some(effect_chain)
}

fn build_source_effect_chain(
    source: &SceneVisualLayer,
    resolver: &SceneResourceResolver,
    object_id: u32,
    object_name: &str,
    baseline_visual_output: bool,
) -> (Vec<ScenePhase10EffectNode>, Vec<SceneGraphIssue>) {
    let mut chain = Vec::new();
    let mut issues = Vec::new();
    let severity = if baseline_visual_output {
        SceneGraphIssueSeverity::Warning
    } else {
        SceneGraphIssueSeverity::Fatal
    };

    for effect in source
        .effect_instances
        .iter()
        .filter(|effect| effect.visible)
    {
        let effect_plan = match load_scene_effect_plan(resolver, &effect.effect_path) {
            Ok(effect_plan) => effect_plan,
            Err(error) => {
                issues.push(SceneGraphIssue {
                    severity,
                    code: SceneGraphIssueCode::InvalidEffect,
                    diagnostic_code: None,
                    message: format!(
                        "\"{}\" could not resolve object effect {} for phase-10 native rendering.",
                        object_name, effect.effect_path
                    ),
                    object_id: Some(object_id),
                    object_name: Some(object_name.to_string()),
                    resource_path: Some(effect.effect_path.clone()),
                    detail: Some(error),
                    resource_lookup: Some(resolver.inspect_relative_path(&effect.effect_path)),
                    resource_present_but_unsupported: false,
                });
                continue;
            }
        };

        if let Some(node) = build_effect_node(
            &effect_plan,
            resolver,
            object_id,
            object_name,
            severity,
            Some(effect),
            !chain.is_empty(),
            &mut issues,
        ) {
            chain.push(node);
        }
    }

    (chain, issues)
}

fn build_effect_node(
    effect: &SceneEffectPlan,
    resolver: &SceneResourceResolver,
    object_id: u32,
    object_name: &str,
    severity: SceneGraphIssueSeverity,
    runtime_effect: Option<&SceneVisualEffect>,
    chain_has_previous_output: bool,
    issues: &mut Vec<SceneGraphIssue>,
) -> Option<ScenePhase10EffectNode> {
    let mut resolved_passes = Vec::new();

    for dependency_lookup in &effect.dependency_lookups {
        if dependency_lookup.matched_path.is_none() {
            issues.push(unresolved_effect_dependency_issue(
                object_id,
                object_name,
                severity,
                effect,
                dependency_lookup.clone(),
            ));
            return None;
        }
    }

    for pass in &effect.passes {
        let Some(effect_material_path) = pass.material_path.as_deref() else {
            continue;
        };
        if pass
            .material_lookup
            .as_ref()
            .and_then(|lookup| lookup.matched_path.as_ref())
            .is_none()
        {
            issues.push(unresolved_effect_material_issue(
                object_id,
                object_name,
                severity,
                effect,
                effect_material_path,
                pass.material_lookup.clone(),
            ));
            return None;
        }
        match load_scene_material_plan_with_effect_package_root(
            resolver,
            effect_material_path,
            &effect.effect_package_root,
        ) {
            Ok(effect_material) => {
                let runtime_pass = runtime_effect.and_then(|effect| effect.passes.get(pass.index));
                if let Err(error) =
                    validate_phase10_effect_contract(effect, pass, &effect_material, runtime_pass)
                {
                    let graph_blocker = effect_pass_requires_phase10d_graph(effect, pass);
                    let code = if graph_blocker {
                        phase10d_error_code(&error)
                    } else {
                        SceneGraphIssueCode::InvalidEffect
                    };
                    let diagnostic_code = if graph_blocker {
                        Some(phase10_graph_issue_diagnostic_code(code))
                    } else {
                        Some("effect-unsupported")
                    };
                    issues.push(SceneGraphIssue {
                        severity,
                        code,
                        diagnostic_code,
                        message: format!(
                            "\"{}\" resolved effect material {}, but {} does not support that authored effect contract.",
                            object_name,
                            effect_material_path,
                            if graph_blocker {
                                "phase-10d graph input scope"
                            } else {
                                "phase-10b"
                            }
                        ),
                        object_id: Some(object_id),
                        object_name: Some(object_name.to_string()),
                        resource_path: Some(effect_material_path.to_string()),
                        detail: Some(error),
                        resource_lookup: pass.material_lookup.clone(),
                        resource_present_but_unsupported: true,
                    });
                    return None;
                }
                resolved_passes.push(ScenePhase10EffectPassNode {
                    index: pass.index,
                    bindings: pass.bindings.clone(),
                    target_name: pass.target_name.clone(),
                    copy_background: effect.copy_background || pass.copy_background,
                    input_bindings: phase10_effect_input_bindings(
                        effect,
                        pass,
                        chain_has_previous_output,
                    ),
                    constants: runtime_pass
                        .map(|runtime_pass| {
                            crate::services::scene_shader_material_service::parse_uniform_map_from_constants(
                                &runtime_pass.constants,
                            )
                        })
                        .unwrap_or_default(),
                    texture_overrides: runtime_pass
                        .map(|runtime_pass| {
                            runtime_pass
                                .textures
                                .iter()
                                .map(|texture| {
                                    texture.as_deref().and_then(|texture| {
                                        resolve_effect_runtime_texture_override(
                                            resolver,
                                            effect,
                                            effect_material_path,
                                            pass,
                                            texture,
                                        )
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    material_passes: effect_material.passes,
                });
            }
            Err(error) => {
                issues.push(effect_material_plan_failure_issue(
                    resolver,
                    object_id,
                    object_name,
                    severity,
                    effect,
                    pass,
                    error,
                ));
                return None;
            }
        }
    }

    Some(ScenePhase10EffectNode {
        effect_path: effect.effect_path.clone(),
        passes: resolved_passes,
    })
}

fn resolve_effect_runtime_texture_override(
    resolver: &SceneResourceResolver,
    effect: &SceneEffectPlan,
    effect_material_path: &str,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    texture: &str,
) -> Option<PathBuf> {
    let material_file_path = effect_pass
        .material_lookup
        .as_ref()
        .and_then(|lookup| lookup.matched_path.as_deref());
    resolver
        .resolve_texture_candidates_with_local_root(
            Some(effect_material_path),
            material_file_path,
            texture,
            SceneResourceRootKind::EffectPackage,
            &effect.effect_package_root,
        )
        .into_iter()
        .next()
        .or_else(|| resolver.resolve_relative_path(texture))
}

fn validate_phase10_effect_contract(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    effect_material: &SceneResolvedMaterialPlan,
    runtime_pass: Option<&crate::models::SceneVisualEffectPass>,
) -> Result<&'static ScenePhase10bEffectContract, String> {
    if effect_pass_requires_phase10d_graph(effect, effect_pass) {
        validate_phase10d_graph_contract(effect, effect_pass)?;
        validate_phase10_effect_material_contract(
            effect,
            effect_pass,
            effect_material,
            runtime_pass,
            "phase-10d",
            true,
        )
    } else {
        validate_phase10b_graph_contract(effect, effect_pass)?;
        validate_phase10_effect_material_contract(
            effect,
            effect_pass,
            effect_material,
            runtime_pass,
            "phase-10b",
            false,
        )
    }
}

fn validate_phase10b_graph_contract(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
) -> Result<(), String> {
    if !effect.fbo_names.is_empty() {
        return Err(format!(
            "effect declares named render targets {:?}; phase-10b only supports explicit single-pass compat families and leaves named FBO/pass-order execution to phase-10d.",
            effect.fbo_names
        ));
    }
    if effect.copy_background || effect_pass.copy_background {
        return Err(
            "effect declares copybackground; phase-10d render-graph support is required for copied background routing."
                .to_string(),
        );
    }
    if effect.passes.len() != 1 {
        return Err(format!(
            "effect declares {} passes; phase-10b only supports a single authored effect pass per compat family.",
            effect.passes.len()
        ));
    }
    if let Some(target_name) = effect_pass.target_name.as_deref() {
        return Err(format!(
            "effect pass targets {target_name}; phase-10d render-graph support is required for named render targets and previous-texture chaining."
        ));
    }
    Ok(())
}

fn validate_phase10d_graph_contract(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
) -> Result<(), String> {
    if let Some(target_name) = effect_pass.target_name.as_deref() {
        if !effect
            .fbo_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(target_name))
        {
            return Err(format!(
                "phase-10d target lifecycle blocker: effect pass targets {target_name}, but the target is not declared in fbos {:?}.",
                effect.fbo_names
            ));
        }
    }

    for binding in &effect_pass.bindings {
        let Some(ScenePhase10InputSource::NamedTarget(target_name)) = effect_binding_input_source(
            effect,
            effect_pass,
            binding,
            ScenePhase10InputSource::LocalCurrentVisual,
        ) else {
            continue;
        };
        if !effect
            .fbo_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&target_name))
        {
            return Err(format!(
                "phase-10d input scope blocker: binding {} at g_Texture{} references named target {target_name}, but the effect declares fbos {:?}.",
                binding.name, binding.index, effect.fbo_names
            ));
        }
    }

    validate_phase10d_pass_target_ordering(effect)?;

    Ok(())
}

fn validate_phase10d_pass_target_ordering(effect: &SceneEffectPlan) -> Result<(), String> {
    let mut written_targets: BTreeSet<String> = BTreeSet::new();

    for pass in &effect.passes {
        let target_name = pass.target_name.as_deref().unwrap_or("");

        for binding in &pass.bindings {
            let Some(ScenePhase10InputSource::NamedTarget(target_ref)) = effect_binding_input_source(
                effect,
                pass,
                binding,
                ScenePhase10InputSource::LocalCurrentVisual,
            ) else {
                continue;
            };
            let normalized_ref = normalized_contract_key(&target_ref);
            if !written_targets.contains(&normalized_ref) {
                return Err(format!(
                    "phase-10d graph-cycle-or-order-invalid: pass {} binds named target {target_ref} at g_Texture{}, but no earlier pass writes to that target. Currently written targets: {:?}.",
                    pass.index,
                    binding.index,
                    written_targets
                ));
            }
        }

        if !target_name.is_empty() {
            written_targets.insert(normalized_contract_key(target_name));
        }
    }

    Ok(())
}

fn phase10d_error_code(error: &str) -> SceneGraphIssueCode {
    if error.contains("target lifecycle blocker") {
        SceneGraphIssueCode::GraphTargetMissing
    } else if error.contains("input scope blocker") {
        SceneGraphIssueCode::GraphInputMissing
    } else if error.contains("graph-cycle-or-order-invalid") {
        SceneGraphIssueCode::GraphCycleOrOrderInvalid
    } else if error.contains("graph/source scope blocker") {
        SceneGraphIssueCode::GraphConstructionIncomplete
    } else if error.contains("copybackground") {
        SceneGraphIssueCode::GraphCopybackgroundUnavailable
    } else if error.contains("mask") && error.contains("target") {
        SceneGraphIssueCode::GraphMaskTargetMissing
    } else {
        SceneGraphIssueCode::InvalidEffect
    }
}

fn phase10_graph_issue_diagnostic_code(code: SceneGraphIssueCode) -> &'static str {
    match code {
        SceneGraphIssueCode::GraphTargetMissing => "graph-target-missing",
        SceneGraphIssueCode::GraphInputMissing => "graph-input-missing",
        SceneGraphIssueCode::GraphCycleOrOrderInvalid => "graph-cycle-or-order-invalid",
        SceneGraphIssueCode::GraphCopybackgroundUnavailable => "graph-copybackground-unavailable",
        SceneGraphIssueCode::GraphMaskTargetMissing => "graph-mask-target-missing",
        SceneGraphIssueCode::GraphConstructionIncomplete => "graph-construction-incomplete",
        SceneGraphIssueCode::InvalidEffect => "effect-graph-scope-blocked",
        _ => "effect-graph-scope-blocked",
    }
}

fn validate_phase10_effect_material_contract(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    effect_material: &SceneResolvedMaterialPlan,
    runtime_pass: Option<&crate::models::SceneVisualEffectPass>,
    phase_label: &'static str,
    allow_graph_input_bindings: bool,
) -> Result<&'static ScenePhase10bEffectContract, String> {
    if effect_material.passes.len() != 1 {
        return Err(format!(
            "effect material {} expands to {} passes; {phase_label} currently supports one material pass per graph pass.",
            effect_material.material_path.display(),
            effect_material.passes.len()
        ));
    }
    if !effect_material.material_effects.is_empty() {
        return Err(format!(
            "effect material {} declares nested effects; {phase_label} does not recurse authored effect stacks inside compat families.",
            effect_material.material_path.display()
        ));
    }

    let material_pass = &effect_material.passes[0];
    if let Some(reason) = phase10b_blocked_effect_reason(&material_pass.shader_ref) {
        return Err(reason.to_string());
    }
    let Some(contract) =
        phase10b_supported_effect_contract_for_shader_ref(&material_pass.shader_ref)
    else {
        if phase_label == "phase-10d" {
            return Err(format!(
                "phase-10d graph/source scope blocker: shader {} is not one of the executable compat families for input scope, target lifecycle, or previous-background routing.",
                material_pass.shader_ref
            ));
        }
        return Err(format!(
            "shader {} is not one of the explicit phase-10b single-pass compat families.",
            material_pass.shader_ref
        ));
    };
    if effect_material.passes.iter().any(|pass| {
        phase10b_supported_effect_contract_for_shader_ref(&pass.shader_ref)
            .map(|pass_contract| pass_contract.kind != contract.kind)
            .unwrap_or(true)
    }) {
        return Err(format!(
            "effect material {} mixes authored shader families; {phase_label} compat requires one explicit family per effect material.",
            effect_material.material_path.display()
        ));
    }
    if !material_pass.effect_paths.is_empty() {
        return Err(format!(
            "effect material {} declares nested material effects; {phase_label} compat does not execute authored effect-in-effect chains.",
            effect_material.material_path.display()
        ));
    }

    for (combo_name, combo_value) in &material_pass.combos {
        if !contract
            .supported_combo_defaults
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(combo_name))
        {
            return Err(format!(
                "{} combo {} falls outside the explicit {phase_label} {} contract.",
                contract.family, combo_name, contract.family
            ));
        }
        if !phase10b_combo_value_supported(contract, combo_name, *combo_value) {
            return Err(format!(
                "{} combo {}={} falls outside the supported {phase_label} {} contract.",
                contract.family, combo_name, combo_value, contract.family
            ));
        }
    }

    let material_slots = material_pass
        .textures
        .iter()
        .filter(|binding| binding.texture_name.is_some() && binding.resolved_path.is_some())
        .map(|binding| binding.slot_index)
        .collect::<BTreeSet<_>>();
    let runtime_override_slots = runtime_pass
        .map(|runtime_pass| {
            runtime_pass
                .textures
                .iter()
                .enumerate()
                .filter(|(_, texture)| texture.is_some())
                .map(|(slot, _)| slot)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let binding_slots = effect_pass
        .bindings
        .iter()
        .map(|binding| binding.index)
        .collect::<BTreeSet<_>>();

    for slot in material_slots
        .iter()
        .chain(runtime_override_slots.iter())
        .chain(binding_slots.iter())
    {
        if !contract.supported_texture_slots.contains(slot) {
            return Err(format!(
                "{} uses g_Texture{} but the explicit {phase_label} {} contract only supports slots {:?}.",
                contract.family, slot, contract.family, contract.supported_texture_slots
            ));
        }
    }
    for required_slot in contract.required_texture_slots {
        if *required_slot == 0 {
            continue;
        }
        let present = material_slots.contains(required_slot)
            || runtime_override_slots.contains(required_slot)
            || binding_slots.contains(required_slot);
        if !present {
            return Err(format!(
                "{} requires g_Texture{} according to the {phase_label} contract, but no authored material texture, runtime override, or binding provides it.",
                contract.family, required_slot
            ));
        }
    }
    if material_slots.contains(&0) || runtime_override_slots.contains(&0) {
        return Err(format!(
            "{} reserves g_Texture0 for graph input; {phase_label} does not support authored textures or runtime overrides replacing that slot.",
            contract.family
        ));
    }

    for binding in &effect_pass.bindings {
        if allow_graph_input_bindings
            && effect_binding_input_source(
                effect,
                effect_pass,
                binding,
                ScenePhase10InputSource::LocalCurrentVisual,
            )
            .is_some()
        {
            continue;
        }
        let Some(slot_contract) = contract
            .runtime_binding_layout
            .iter()
            .find(|slot_contract| slot_contract.slot == binding.index)
        else {
            return Err(format!(
                "{} bind.index {} is outside the explicit {phase_label} {} layout.",
                contract.family, binding.index, contract.family
            ));
        };
        if !binding_name_matches_semantic(&binding.name, slot_contract.semantic) {
            return Err(format!(
                "{} binding {} at g_Texture{} does not match the explicit {phase_label} {} binding layout.",
                contract.family, binding.name, binding.index, contract.family
            ));
        }
    }
    for (combo_name, combo_value) in &material_pass.combos {
        let Some(required_slot) = phase10b_combo_required_texture_slot(contract, combo_name) else {
            continue;
        };
        if *combo_value == 0 {
            continue;
        }
        let present = material_slots.contains(&required_slot)
            || runtime_override_slots.contains(&required_slot);
        if !present {
            return Err(format!(
                "{} combo {}={} requires g_Texture{}, but that authored slot is not populated by the material or runtime override.",
                contract.family, combo_name, combo_value, required_slot
            ));
        }
    }

    for uniform_name in material_pass.uniforms.keys().chain(
        runtime_pass
            .into_iter()
            .flat_map(|runtime_pass| runtime_pass.constants.keys()),
    ) {
        let normalized = normalized_contract_key(uniform_name);
        if normalized.is_empty() {
            continue;
        }
        if !contract.supported_uniforms.contains(&normalized.as_str()) {
            return Err(format!(
                "{} uniform {} falls outside the explicit {phase_label} {} contract.",
                contract.family, uniform_name, contract.family
            ));
        }
    }

    let Some(material_contract) = phase10b_effect_contract_for_kind(contract.kind) else {
        return Err(format!(
            "{} is not available as a {phase_label} compat family.",
            contract.family
        ));
    };
    Ok(material_contract)
}

fn effect_pass_requires_phase10d_graph(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
) -> bool {
    !effect.fbo_names.is_empty()
        || effect.passes.len() != 1
        || effect.copy_background
        || effect_pass.copy_background
        || effect_pass.target_name.is_some()
}

fn phase10_effect_input_bindings(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    chain_has_previous_output: bool,
) -> Vec<ScenePhase10InputBinding> {
    let mut bindings = BTreeMap::new();
    let default_input_source =
        phase10_effect_default_input_source(effect, effect_pass, chain_has_previous_output);
    bindings.insert(0, default_input_source.clone());

    for binding in &effect_pass.bindings {
        let Some(source) =
            effect_binding_input_source(effect, effect_pass, binding, default_input_source.clone())
        else {
            continue;
        };
        bindings.insert(binding.index, source);
    }

    bindings
        .into_iter()
        .map(|(slot, source)| ScenePhase10InputBinding { slot, source })
        .collect()
}

fn phase10_effect_default_input_source(
    effect: &SceneEffectPlan,
    effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    chain_has_previous_output: bool,
) -> ScenePhase10InputSource {
    if effect.copy_background || effect_pass.copy_background {
        ScenePhase10InputSource::CopiedBackground
    } else if chain_has_previous_output || effect_pass.index > 0 {
        ScenePhase10InputSource::PreviousPass
    } else {
        ScenePhase10InputSource::LocalCurrentVisual
    }
}

fn effect_binding_input_source(
    effect: &SceneEffectPlan,
    _effect_pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    binding: &SceneEffectBinding,
    default_input_source: ScenePhase10InputSource,
) -> Option<ScenePhase10InputSource> {
    let normalized = normalized_contract_key(&binding.name);
    if normalized.is_empty() {
        return None;
    }

    if let Some(target_name) = effect.fbo_names.iter().find(|target_name| {
        normalized == normalized_contract_key(target_name)
            || normalized.contains(&normalized_contract_key(target_name))
    }) {
        return Some(ScenePhase10InputSource::NamedTarget(target_name.clone()));
    }

    if normalized.contains("copybackground") || normalized.contains("copiedbackground") {
        return Some(ScenePhase10InputSource::CopiedBackground);
    }
    if normalized == "background" || normalized.ends_with("background") {
        return Some(ScenePhase10InputSource::Background);
    }
    if normalized.contains("previous") {
        return Some(ScenePhase10InputSource::PreviousPass);
    }
    if normalized.contains("input")
        || normalized.contains("source")
        || normalized.contains("main")
        || normalized.contains("base")
    {
        return Some(default_input_source);
    }

    None
}

fn phase10b_combo_value_supported(
    contract: &ScenePhase10bEffectContract,
    combo_name: &str,
    combo_value: i32,
) -> bool {
    let normalized = normalized_contract_key(combo_name);
    match contract.family {
        "pulse" => match normalized.as_str() {
            "blendmode" => matches!(combo_value, 0 | 2 | 7 | 9 | 30 | 31 | 32),
            "audioprocessing" => combo_value == 0,
            "mask" | "pulsealpha" | "pulsecolor" => matches!(combo_value, 0 | 1),
            _ => false,
        },
        "shake" => match normalized.as_str() {
            "direction" => matches!(combo_value, 0 | 1 | 2),
            "audioprocessing" => combo_value == 0,
            "mask" | "noise" | "timeoffset" => matches!(combo_value, 0 | 1),
            _ => false,
        },
        "waterripple" => match normalized.as_str() {
            "mask" => matches!(combo_value, 0 | 1),
            "perspective" | "specular" => combo_value == 0,
            _ => false,
        },
        "waterwaves" => match normalized.as_str() {
            "mask" | "timeoffset" => matches!(combo_value, 0 | 1),
            "perspective" | "dualwaves" => combo_value == 0,
            _ => false,
        },
        "tint" => match normalized.as_str() {
            "blendmode" => matches!(combo_value, 0 | 2 | 7 | 9 | 30 | 31 | 32),
            "mask" => matches!(combo_value, 0 | 1),
            _ => false,
        },
        "scroll" => false,
        _ => false,
    }
}

fn phase10b_combo_required_texture_slot(
    contract: &ScenePhase10bEffectContract,
    combo_name: &str,
) -> Option<usize> {
    let normalized = normalized_contract_key(combo_name);
    match contract.family {
        "pulse" if normalized == "mask" => Some(2),
        "shake" if normalized == "mask" => Some(3),
        "shake" if normalized == "timeoffset" => Some(2),
        "waterripple" if normalized == "mask" => Some(1),
        "waterwaves" if normalized == "mask" => Some(1),
        "waterwaves" if normalized == "timeoffset" => Some(2),
        "tint" if normalized == "mask" => Some(1),
        _ => None,
    }
}

fn normalized_contract_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn binding_name_matches_semantic(
    binding_name: &str,
    semantic: ScenePhase10bBindingSemantic,
) -> bool {
    let normalized = normalized_contract_key(binding_name);
    match semantic {
        ScenePhase10bBindingSemantic::PreviousInput => {
            normalized.contains("previous")
                || normalized.contains("input")
                || normalized.contains("source")
                || normalized.contains("main")
                || normalized.contains("base")
        }
        ScenePhase10bBindingSemantic::NoiseTexture => normalized.contains("noise"),
        ScenePhase10bBindingSemantic::FlowMap => {
            normalized.contains("flow") || normalized.contains("direction")
        }
        ScenePhase10bBindingSemantic::TimeOffset => {
            normalized.contains("timeoffset") || normalized == "time" || normalized == "offset"
        }
        ScenePhase10bBindingSemantic::OpacityMask => {
            normalized.contains("mask") || normalized.contains("opacity")
        }
        ScenePhase10bBindingSemantic::NormalMap => {
            normalized.contains("normal")
                || normalized.contains("ripple")
                || normalized.contains("distort")
        }
    }
}

fn unresolved_effect_dependency_issue(
    object_id: u32,
    object_name: &str,
    severity: SceneGraphIssueSeverity,
    effect: &SceneEffectPlan,
    lookup: SceneResourceLookup,
) -> SceneGraphIssue {
    SceneGraphIssue {
        severity,
        code: SceneGraphIssueCode::InvalidEffect,
        diagnostic_code: Some("effect-dependency-reference-unresolved"),
        message: format!(
            "\"{}\" could not resolve effect dependency {} for phase-10 native rendering.",
            object_name, lookup.authored_reference
        ),
        object_id: Some(object_id),
        object_name: Some(object_name.to_string()),
        resource_path: Some(lookup.authored_reference.clone()),
        detail: Some(format!(
            "dependency was attempted from effect package {} before Scene roots",
            effect.effect_package_root.display()
        )),
        resource_lookup: Some(lookup),
        resource_present_but_unsupported: false,
    }
}

fn unresolved_effect_material_issue(
    object_id: u32,
    object_name: &str,
    severity: SceneGraphIssueSeverity,
    effect: &SceneEffectPlan,
    effect_material_path: &str,
    material_lookup: Option<SceneResourceLookup>,
) -> SceneGraphIssue {
    SceneGraphIssue {
        severity,
        code: SceneGraphIssueCode::InvalidEffect,
        diagnostic_code: Some("effect-material-reference-unresolved"),
        message: format!(
            "\"{}\" could not resolve effect material {} for phase-10 native rendering.",
            object_name, effect_material_path
        ),
        object_id: Some(object_id),
        object_name: Some(object_name.to_string()),
        resource_path: Some(effect_material_path.to_string()),
        detail: Some(format!(
            "material was attempted from effect package {} before Scene roots",
            effect.effect_package_root.display()
        )),
        resource_lookup: material_lookup,
        resource_present_but_unsupported: false,
    }
}

fn effect_material_plan_failure_issue(
    resolver: &SceneResourceResolver,
    object_id: u32,
    object_name: &str,
    severity: SceneGraphIssueSeverity,
    effect: &SceneEffectPlan,
    pass: &crate::services::scene_shader_material_service::SceneEffectPassPlan,
    error: String,
) -> SceneGraphIssue {
    let effect_material_path = pass.material_path.as_deref().unwrap_or_default();
    let Some(material_lookup) = pass.material_lookup.as_ref() else {
        return unresolved_effect_material_issue(
            object_id,
            object_name,
            severity,
            effect,
            effect_material_path,
            None,
        );
    };
    let Some(material_json_path) = material_lookup.matched_path.as_deref() else {
        return unresolved_effect_material_issue(
            object_id,
            object_name,
            severity,
            effect,
            effect_material_path,
            Some(material_lookup.clone()),
        );
    };

    let Some(material_json) = read_json(material_json_path) else {
        return SceneGraphIssue {
            severity,
            code: SceneGraphIssueCode::InvalidEffect,
            diagnostic_code: Some("effect-material-invalid"),
            message: format!(
                "\"{}\" could not parse effect material {} for phase-10 native rendering.",
                object_name, effect_material_path
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            resource_path: Some(effect_material_path.to_string()),
            detail: Some(error),
            resource_lookup: Some(material_lookup.clone()),
            resource_present_but_unsupported: false,
        };
    };

    for shader_ref in material_shader_references(&material_json) {
        let shader_lookup = inspect_scene_shader_source_with_effect_context(
            resolver,
            &shader_ref,
            Some(&effect.effect_package_root),
        );
        match shader_lookup.kind {
            SceneShaderSourceKind::AuthoredSourceSet => {
                let material_resolved_path = material_lookup
                    .matched_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| effect_material_path.to_string());
                return SceneGraphIssue {
                    severity,
                    code: SceneGraphIssueCode::InvalidEffect,
                    diagnostic_code: Some("effect-unsupported"),
                    message: format!(
                        "\"{}\" resolved effect material {}, but phase-10b does not support authored shader source pair {} yet.",
                        object_name, effect_material_path, shader_ref
                    ),
                    object_id: Some(object_id),
                    object_name: Some(object_name.to_string()),
                    resource_path: Some(shader_ref.clone()),
                    detail: Some(format!(
                        "effect material {effect_material_path} resolved at {material_resolved_path}. {error}"
                    )),
                    resource_lookup: Some(shader_lookup.lookup),
                    resource_present_but_unsupported: true,
                };
            }
            SceneShaderSourceKind::Missing => {
                let material_resolved_path = material_lookup
                    .matched_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| effect_material_path.to_string());
                return SceneGraphIssue {
                    severity,
                    code: SceneGraphIssueCode::InvalidEffect,
                    diagnostic_code: Some("shader-reference-unresolved"),
                    message: format!(
                        "\"{}\" resolved effect material {}, but could not resolve authored shader {} for phase-10 native rendering.",
                        object_name, effect_material_path, shader_ref
                    ),
                    object_id: Some(object_id),
                    object_name: Some(object_name.to_string()),
                    resource_path: Some(shader_ref.clone()),
                    detail: Some(format!(
                        "effect material {effect_material_path} resolved at {material_resolved_path} before authored shader lookup failed."
                    )),
                    resource_lookup: Some(shader_lookup.lookup),
                    resource_present_but_unsupported: false,
                };
            }
            SceneShaderSourceKind::Metal => {}
        }
    }

    let diagnostic_code = if error
        .contains("does not map to a supported phase-10 compatibility program")
        || error.contains("phase-10b does not support that authored shader family")
        || error.contains(
            "phase-10b does not support that authored shader family as single-pass compat",
        )
        || error.contains(
            "phase-10b does not support that authored shader family as explicit single-pass compat",
        )
        || error.contains("requires phase-10d")
    {
        Some("effect-unsupported")
    } else {
        Some("effect-invalid")
    };
    let message = if diagnostic_code == Some("effect-unsupported") {
        format!(
            "\"{}\" resolved effect material {}, but phase-10b does not support that authored effect shader contract.",
            object_name, effect_material_path
        )
    } else {
        format!(
            "\"{}\" could not build effect material {} for phase-10 native rendering.",
            object_name, effect_material_path
        )
    };

    SceneGraphIssue {
        severity,
        code: SceneGraphIssueCode::InvalidEffect,
        diagnostic_code,
        message,
        object_id: Some(object_id),
        object_name: Some(object_name.to_string()),
        resource_path: Some(effect_material_path.to_string()),
        detail: Some(error),
        resource_lookup: Some(material_lookup.clone()),
        resource_present_but_unsupported: diagnostic_code == Some("effect-unsupported"),
    }
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn material_shader_references(material_json: &Value) -> Vec<String> {
    material_pass_values(material_json)
        .into_iter()
        .filter_map(|pass| {
            pass.get("shader")
                .and_then(Value::as_str)
                .or_else(|| material_json.get("shader").and_then(Value::as_str))
        })
        .map(ToString::to_string)
        .collect()
}

fn material_pass_values(material_json: &Value) -> Vec<&Value> {
    if let Some(passes) = material_json.get("passes").and_then(Value::as_array) {
        return passes.iter().collect();
    }
    if material_json
        .get("shader")
        .and_then(Value::as_str)
        .map(|shader| !shader.trim().is_empty())
        .unwrap_or(false)
        || material_json
            .get("textures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
    {
        return vec![material_json];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::{
        models::{SceneManifest, WallpaperRecord, WallpaperType},
        services::{runtime_document_service, scene_resource_service::SceneResourceResolver},
    };

    use super::build_scene_phase10_graph;

    fn write(path: &std::path::Path, body: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(path, body).expect("fixture");
    }

    fn scene_record(managed_path: &std::path::Path) -> WallpaperRecord {
        WallpaperRecord {
            id: "scene-model".to_string(),
            title: "Scene Model".to_string(),
            wallpaper_type: WallpaperType::Scene,
            source_path: managed_path.display().to_string(),
            managed_path: managed_path.display().to_string(),
            preview_path: None,
            entry_path: None,
            last_snapshot_path: None,
            property_schema: vec![],
            property_sections: vec![],
            scene_cache: None,
            scene_manifest: Some(SceneManifest::default()),
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec![],
        }
    }

    fn synthetic_mdl_fixture() -> Vec<u8> {
        fn push_cstring(bytes: &mut Vec<u8>, value: &str) {
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
        }
        fn push_u16(bytes: &mut Vec<u8>, value: u16) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn push_u32(bytes: &mut Vec<u8>, value: u32) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn push_i32(bytes: &mut Vec<u8>, value: i32) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn push_f32(bytes: &mut Vec<u8>, value: f32) {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        fn push_identity(bytes: &mut Vec<u8>) {
            for column in 0..4 {
                for row in 0..4 {
                    push_f32(bytes, if column == row { 1.0 } else { 0.0 });
                }
            }
        }

        let mut bytes = Vec::new();
        push_cstring(&mut bytes, "MDLV0023");
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_cstring(&mut bytes, "materials/hero.material");
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 0x0180_0009);
        push_u32(&mut bytes, 52 * 3);
        for (position, uv) in [
            ([0.0, 0.0, 0.0], [0.0, 0.0]),
            ([64.0, 0.0, 0.0], [1.0, 0.0]),
            ([0.0, 64.0, 0.0], [0.0, 1.0]),
        ] {
            for value in position {
                push_f32(&mut bytes, value);
            }
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_f32(&mut bytes, 1.0);
            push_f32(&mut bytes, 0.0);
            push_f32(&mut bytes, 0.0);
            push_f32(&mut bytes, 0.0);
            push_f32(&mut bytes, uv[0]);
            push_f32(&mut bytes, uv[1]);
        }
        push_u32(&mut bytes, 6);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 2);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 16);
        push_cstring(&mut bytes, "masks/eye_mask");
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        push_cstring(&mut bytes, "MDLS0004");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_cstring(&mut bytes, "root");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, u32::MAX);
        push_u32(&mut bytes, 64);
        push_identity(&mut bytes);
        push_cstring(&mut bytes, "{}");
        push_u16(&mut bytes, 0);
        bytes
    }

    #[test]
    fn phase10_graph_resolves_materials_builtins_and_puppet_assets() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-model.metal"),
            b"fragment float4 compat_model_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":1,
                  "name":"PuppetHero",
                  "image":"models/hero.model.json",
                  "origin":"512 384 0",
                  "size":"320 320",
                  "visible": true
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/hero.model.json"),
            br#"{
              "width":320,
              "height":320,
              "material":"materials/hero.material",
              "puppet":"models/hero.mdl"
            }"#,
        );
        write(
            &extracted.join("materials/hero.material"),
            br#"{"passes":[{"shader":"modelimage","textures":["textures/hero"]}]}"#,
        );
        write(&extracted.join("textures/hero.tex"), b"fake-tex");
        write(&extracted.join("models/hero.mdl"), &synthetic_mdl_fixture());

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].submesh_count, 1);
        assert!(report.graph.visuals[0].puppet_path.is_some());
        assert_eq!(report.graph.consumed_visual_ids, vec![1]);
    }

    #[test]
    fn phase10_graph_flattens_effect_material_passes_into_visual_chain() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            b"fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &builtin.join("assets/materials/compat/tint.material"),
            br#"{"passes":[{"shader":"effects/tint"}]}"#,
        );
        write(
            &builtin.join("assets/effects/compat/tint.effect"),
            br#"{"version":1,"passes":[{"material":"assets/materials/compat/tint.material","bind":[{"name":"input","index":0}]}]}"#,
        );
        write(
            &builtin.join("assets/effects/compat/shaders/effects/tint.vert"),
            b"void main() {}",
        );
        write(
            &builtin.join("assets/effects/compat/shaders/effects/tint.frag"),
            b"void main() {}",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":1,
                  "name":"EffectHero",
                  "image":"models/hero.model.json",
                  "origin":"256 256 0",
                  "size":"256 256",
                  "visible": true
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/hero.model.json"),
            br#"{"width":256,"height":256,"material":"materials/hero.material"}"#,
        );
        write(
            &extracted.join("materials/hero.material"),
            br#"{"effects":["assets/effects/compat/tint.effect"],"passes":[{"shader":"genericimage4","textures":["textures/hero"]}]}"#,
        );
        write(&extracted.join("textures/hero.tex"), b"fake-tex");

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain[0].passes.len(), 1);
        assert_eq!(
            report.graph.visuals[0].effect_chain[0].passes[0]
                .bindings
                .len(),
            1
        );
        assert_eq!(
            report.graph.visuals[0].effect_chain[0].passes[0].bindings[0].name,
            "input"
        );
    }

    #[test]
    fn phase10_graph_leaves_simple_sprite_materials_on_baseline() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            b"fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":1,
                  "name":"BaselineHero",
                  "image":"models/hero.model.json",
                  "origin":"256 256 0",
                  "size":"256 256",
                  "visible": true
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/hero.model.json"),
            br#"{"width":256,"height":256,"material":"materials/hero.material"}"#,
        );
        write(
            &extracted.join("materials/hero.material"),
            br#"{"passes":[{"shader":"genericimage4","textures":["textures/hero"]}]}"#,
        );
        write(&extracted.join("textures/hero.tex"), b"fake-tex");
        write(&managed.join("decoded/textures/hero.png"), b"decoded");

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert!(report.graph.visuals.is_empty());
        assert!(report.graph.consumed_visual_ids.is_empty());
    }

    #[test]
    fn phase10_graph_leaves_solid_layer_with_missing_material_on_baseline() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":13,
                  "name":"Background",
                  "image":"models/solid.model.json",
                  "origin":"960 540 0",
                  "size":"2920 2000",
                  "color":"0.17 0.40 0.80"
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/solid.model.json"),
            br#"{"solidlayer":true,"material":"materials/missing.material"}"#,
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert!(report.graph.visuals.is_empty());
        assert!(report.graph.consumed_visual_ids.is_empty());
        assert!(report
            .issues
            .iter()
            .all(|issue| issue.code != super::SceneGraphIssueCode::InvalidMaterial));
    }

    #[test]
    fn phase10_graph_prefers_evaluated_world_bounds_for_parented_puppets() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            b"fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "general": { "orthogonalprojection": { "width": 1000, "height": 1000 } },
              "objects":[
                {
                  "id":1,
                  "name":"Parent",
                  "image":"models/parent.model.json",
                  "origin":"400 300 0",
                  "size":"200 100",
                  "scale":"2 2 1",
                  "visible": true
                },
                {
                  "id":2,
                  "parent":1,
                  "name":"ChildPuppet",
                  "image":"models/child.model.json",
                  "origin":"50 25 0",
                  "size":"100 50",
                  "visible": true
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/parent.model.json"),
            br#"{"width":200,"height":100,"material":"materials/parent.material"}"#,
        );
        write(
            &extracted.join("models/child.model.json"),
            br#"{"width":100,"height":50,"material":"materials/child.material","puppet":"models/child.mdl"}"#,
        );
        write(
            &extracted.join("materials/parent.material"),
            br#"{"passes":[{"shader":"genericimage4","textures":["textures/parent"]}]}"#,
        );
        write(
            &extracted.join("materials/child.material"),
            br#"{"passes":[{"shader":"genericimage4","textures":["textures/child"]}]}"#,
        );
        write(&extracted.join("textures/parent.tex"), b"fake-tex");
        write(&extracted.join("textures/child.tex"), b"fake-tex");
        write(
            &extracted.join("models/child.mdl"),
            &synthetic_mdl_fixture(),
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].object_id, 2);
        assert_eq!(report.graph.visuals[0].quad.left, 400.0);
        assert_eq!(report.graph.visuals[0].quad.top, 600.0);
        assert_eq!(report.graph.visuals[0].quad.width, 200.0);
        assert_eq!(report.graph.visuals[0].quad.height, 100.0);
        assert_eq!(report.graph.visuals[0].authored_size, [100.0, 50.0]);
    }

    #[test]
    fn phase10_graph_supports_effect_only_solid_layer_without_material_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            b"fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &builtin.join("assets/materials/compat/tint.material"),
            br#"{"passes":[{"shader":"effects/tint"}]}"#,
        );
        write(
            &builtin.join("assets/effects/compat/tint.effect"),
            br#"{"version":1,"passes":[{"material":"assets/materials/compat/tint.material","bind":[{"name":"input","index":0}]}]}"#,
        );
        write(
            &builtin.join("assets/effects/compat/shaders/effects/tint.vert"),
            b"void main() {}",
        );
        write(
            &builtin.join("assets/effects/compat/shaders/effects/tint.frag"),
            b"void main() {}",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"Sky",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "scale":"4 2 1",
                  "color":"0.1 0.2 0.3",
                  "effects":[
                    {
                      "file":"assets/effects/compat/tint.effect",
                      "visible":true,
                      "passes":[{"textures":[null]}]
                    }
                  ]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain.len(), 1);
        assert!(report
            .issues
            .iter()
            .all(|issue| issue.code != super::SceneGraphIssueCode::MissingMaterial));
    }

    #[test]
    fn phase10_graph_keeps_unsupported_object_effect_on_baseline_when_base_visual_exists() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"Sky",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "scale":"4 2 1",
                  "color":"0.1 0.2 0.3",
                  "effects":[
                    {
                      "file":"effects/custom/tint/effect.json",
                      "visible":true,
                      "passes":[{"textures":[null,"masks/tint-mask"]}]
                    }
                  ]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/custom/tint/effect.json"),
            br#"{"passes":[{"material":"materials/custom/tint.material"}]}"#,
        );
        write(
            &extracted.join("materials/custom/tint.material"),
            br#"{"passes":[{"shader":"effects/custom/tint"}]}"#,
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert!(report.graph.visuals.is_empty());
        assert!(report.issues.iter().any(|issue| {
            issue.code == super::SceneGraphIssueCode::InvalidEffect
                && issue.severity == super::SceneGraphIssueSeverity::Warning
        }));
        assert!(report
            .issues
            .iter()
            .all(|issue| issue.code != super::SceneGraphIssueCode::MissingMaterial));
    }

    #[test]
    fn phase10_graph_supports_compat_authored_effect_shader_families() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"Summer",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/pulse/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/pulse/effect.json"),
            br#"{"passes":[{"material":"materials/effects/pulse.json"}]}"#,
        );
        write(
            &extracted.join("effects/pulse/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.frag"),
            b"void main() {}",
        );
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert!(report
            .issues
            .iter()
            .all(|issue| issue.diagnostic_code != Some("effect-unsupported")));
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain[0].passes.len(), 1);
    }

    #[test]
    fn phase10d_graph_distinguishes_input_sources_named_targets_and_pass_chain() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"GraphScoped",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "color":"0.1 0.2 0.3",
                  "effects":[
                    {"file":"effects/copy-target/effect.json","visible":true},
                    {"file":"effects/local-chain/effect.json","visible":true}
                  ]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/copy-target/effect.json"),
            br#"{
              "copybackground": true,
              "fbos":[{"name":"scratch"}],
              "passes":[
                {"target":"scratch","material":"materials/effects/tint.json","bind":[{"name":"copybackground","index":0},{"name":"background","index":1}]},
                {"material":"materials/effects/tint.json","bind":[{"name":"scratch","index":0}]},
                {"material":"materials/effects/tint.json","bind":[{"name":"previous","index":0}]}
              ]
            }"#,
        );
        write(
            &extracted.join("effects/copy-target/materials/effects/tint.json"),
            br#"{"passes":[{"shader":"effects/tint"}]}"#,
        );
        write(
            &extracted.join("effects/copy-target/shaders/effects/tint.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/copy-target/shaders/effects/tint.frag"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/local-chain/effect.json"),
            br#"{
              "passes":[
                {"material":"materials/effects/tint.json"},
                {"material":"materials/effects/tint.json","bind":[{"name":"previous","index":0}]}
              ]
            }"#,
        );
        write(
            &extracted.join("effects/local-chain/materials/effects/tint.json"),
            br#"{"passes":[{"shader":"effects/tint"}]}"#,
        );
        write(
            &extracted.join("effects/local-chain/shaders/effects/tint.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/local-chain/shaders/effects/tint.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        let visual = &report.graph.visuals[0];
        let copy_target = &visual.effect_chain[0];
        assert_eq!(copy_target.passes.len(), 3);
        assert_eq!(
            copy_target.passes[0].target_name.as_deref(),
            Some("scratch")
        );
        assert!(copy_target.passes[0].copy_background);
        assert_eq!(
            copy_target.passes[0].input_bindings[0].source,
            super::ScenePhase10InputSource::CopiedBackground
        );
        assert_eq!(
            copy_target.passes[0].input_bindings[1].source,
            super::ScenePhase10InputSource::Background
        );
        assert_eq!(
            copy_target.passes[1].input_bindings[0].source,
            super::ScenePhase10InputSource::NamedTarget("scratch".to_string())
        );
        assert_eq!(
            copy_target.passes[2].input_bindings[0].source,
            super::ScenePhase10InputSource::PreviousPass
        );

        let local_chain = &visual.effect_chain[1];
        assert_eq!(
            local_chain.passes[0].input_bindings[0].source,
            super::ScenePhase10InputSource::PreviousPass
        );
        assert_eq!(
            local_chain.passes[1].input_bindings[0].source,
            super::ScenePhase10InputSource::PreviousPass
        );
    }

    #[test]
    fn phase10d_graph_resolves_runtime_effect_texture_overrides_from_material_candidates() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let mask_path = extracted.join("materials/masks/waterwaves_mask.tex");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":67,
                  "name":"MaskedWaterwaves",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "color":"0.1 0.2 0.3",
                  "effects":[
                    {
                      "file":"effects/waterwaves/effect.json",
                      "visible":true,
                      "passes":[
                        {"textures":[null,"masks/waterwaves_mask"]}
                      ]
                    }
                  ]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(&mask_path, b"synthetic mask placeholder");
        write(
            &extracted.join("effects/waterwaves/effect.json"),
            br#"{
              "passes":[{"material":"materials/effects/waterwaves.json"}],
              "dependencies":[
                "materials/effects/waterwaves.json",
                "shaders/effects/waterwaves.vert",
                "shaders/effects/waterwaves.frag"
              ]
            }"#,
        );
        write(
            &extracted.join("effects/waterwaves/materials/effects/waterwaves.json"),
            br#"{"passes":[{"shader":"effects/waterwaves"}]}"#,
        );
        write(
            &extracted.join("effects/waterwaves/shaders/effects/waterwaves.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/waterwaves/shaders/effects/waterwaves.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        let texture_overrides =
            &report.graph.visuals[0].effect_chain[0].passes[0].texture_overrides;
        assert_eq!(texture_overrides.get(0), Some(&None));
        assert_eq!(
            texture_overrides.get(1).and_then(|path| path.as_deref()),
            Some(mask_path.as_path())
        );
    }

    #[test]
    fn phase10d_graph_chains_runtime_masked_waterripple_before_later_effects() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let mask_path = extracted.join("materials/masks/ripple_mask.tex");
        let normal_path = extracted.join("materials/effects/waterripplenormal.tex");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":81,
                  "name":"ChainedRipple",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "color":"0.1 0.2 0.3",
                  "effects":[
                    {
                      "file":"effects/waterripple/effect.json",
                      "visible":true,
                      "passes":[
                        {"textures":[null,"masks/ripple_mask","effects/waterripplenormal"]}
                      ]
                    },
                    {"file":"effects/pulse/effect.json","visible":true}
                  ]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(&mask_path, b"synthetic mask placeholder");
        write(&normal_path, b"synthetic normal placeholder");
        write(
            &extracted.join("effects/waterripple/effect.json"),
            br#"{
              "passes":[{"material":"materials/effects/waterripple.json"}],
              "dependencies":[
                "materials/effects/waterripple.json",
                "materials/effects/waterripplenormal.png",
                "shaders/effects/waterripple.vert",
                "shaders/effects/waterripple.frag"
              ]
            }"#,
        );
        write(
            &extracted.join("materials/effects/waterripple.json"),
            br#"{"passes":[{"shader":"effects/waterripple","textures":[null,null,"effects/waterripplenormal"]}]}"#,
        );
        write(
            &extracted.join("shaders/effects/waterripple.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("shaders/effects/waterripple.frag"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/pulse/effect.json"),
            br#"{"passes":[{"material":"materials/effects/pulse.json"}]}"#,
        );
        write(
            &extracted.join("effects/pulse/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        let visual = &report.graph.visuals[0];
        assert_eq!(visual.effect_chain.len(), 2);

        let ripple_pass = &visual.effect_chain[0].passes[0];
        assert_eq!(
            ripple_pass.input_bindings[0].source,
            super::ScenePhase10InputSource::LocalCurrentVisual
        );
        assert_eq!(
            ripple_pass
                .texture_overrides
                .get(1)
                .and_then(|path| path.as_deref()),
            Some(mask_path.as_path())
        );
        assert_eq!(
            ripple_pass
                .texture_overrides
                .get(2)
                .and_then(|path| path.as_deref()),
            Some(normal_path.as_path())
        );

        let later_pass = &visual.effect_chain[1].passes[0];
        assert_eq!(
            later_pass.input_bindings[0].source,
            super::ScenePhase10InputSource::PreviousPass
        );
    }

    #[test]
    fn phase10d_graph_reports_target_lifecycle_blocker_diagnostic() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":91,
                  "name":"BlockedGraph",
                  "image":"models/deep.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/bad-target/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/deep.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/bad-target/effect.json"),
            br#"{"passes":[{"target":"missing","material":"materials/effects/tint.json"}]}"#,
        );
        write(
            &extracted.join("effects/bad-target/materials/effects/tint.json"),
            br#"{"passes":[{"shader":"effects/tint"}]}"#,
        );
        write(
            &extracted.join("effects/bad-target/shaders/effects/tint.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/bad-target/shaders/effects/tint.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(report.is_blocked());
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.code == super::SceneGraphIssueCode::GraphTargetMissing)
            .expect("graph target missing");
        assert_eq!(issue.severity, super::SceneGraphIssueSeverity::Fatal);
        assert_eq!(
            issue.diagnostic_code,
            Some("graph-target-missing")
        );
        assert!(issue
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("target lifecycle blocker"));
    }

    #[test]
    fn phase10_graph_reports_unknown_authored_effect_shader_as_unsupported_not_unresolved() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"Summer",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/mystery/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/mystery/effect.json"),
            br#"{"passes":[{"material":"materials/effects/mystery.json"}]}"#,
        );
        write(
            &extracted.join("effects/mystery/materials/effects/mystery.json"),
            br#"{"passes":[{"shader":"effects/mystery"}]}"#,
        );
        write(
            &extracted.join("effects/mystery/shaders/effects/mystery.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/mystery/shaders/effects/mystery.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        let issue = report
            .issues
            .iter()
            .find(|issue| issue.diagnostic_code == Some("effect-unsupported"))
            .expect("unsupported effect issue");
        assert_eq!(issue.severity, super::SceneGraphIssueSeverity::Warning);
        assert!(issue
            .message
            .contains("resolved effect material materials/effects/mystery.json"));
        assert!(issue.detail.as_deref().unwrap_or_default().contains(
            "does not support that authored shader family as explicit single-pass compat"
        ));
        assert!(!issue.message.contains("could not resolve effect material"));
        assert!(issue.resource_present_but_unsupported);
        assert_eq!(
            issue
                .resource_lookup
                .as_ref()
                .and_then(|lookup| lookup.matched_path.as_ref()),
            Some(&extracted.join("effects/mystery/shaders/effects/mystery.vert"))
        );
    }

    #[test]
    fn phase10_graph_marks_blur_and_shine_as_phase10d_blockers() {
        for family in ["blur", "shine"] {
            let temp = tempdir().expect("temp dir");
            let managed = temp.path().join("managed");
            let extracted = managed.join("extracted");
            let builtin = temp.path().join("builtin");

            write(
                &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
                b"fragment float4 phase10_effect_fragment() { return float4(1); }",
            );
            write(
                &extracted.join("scene.json"),
                format!(
                    r#"{{
                      "objects":[
                        {{
                          "id":191,
                          "name":"Blocked",
                          "image":"models/util/solidlayer.json",
                          "origin":"960 540 0",
                          "size":"256 256",
                          "effects":[{{"file":"effects/{family}/effect.json","visible":true}}]
                        }}
                      ]
                    }}"#
                )
                .as_bytes(),
            );
            write(
                &extracted.join("models/util/solidlayer.json"),
                br#"{"solidlayer":true}"#,
            );
            write(
                &extracted.join(format!("effects/{family}/effect.json")),
                br#"{
                  "fbos":[{"name":"scratch"}],
                  "passes":[{"target":"scratch","material":"materials/effects/blocker.json"}]
                }"#,
            );
            write(
                &extracted.join(format!("effects/{family}/materials/effects/blocker.json")),
                format!(r#"{{"passes":[{{"shader":"effects/{family}"}}]}}"#).as_bytes(),
            );
            write(
                &extracted.join(format!("effects/{family}/shaders/effects/{family}.vert")),
                b"void main() {}",
            );
            write(
                &extracted.join(format!("effects/{family}/shaders/effects/{family}.frag")),
                b"void main() {}",
            );

            let mut record = scene_record(&managed);
            record.scene_manifest = Some(
                crate::scene::parse_scene_manifest(
                    &extracted.join("scene.json"),
                    &extracted,
                    &BTreeMap::new(),
                )
                .expect("manifest"),
            );
            let runtime = runtime_document_service::runtime_record(&record);
            let scene = match &runtime.runtime {
                crate::models::WallpaperRuntime::Scene { scene } => scene,
                _ => panic!("expected scene runtime"),
            };
            let resolver =
                SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
            let report = build_scene_phase10_graph(scene, &resolver);

            let issue = report
                .issues
                .iter()
                .find(|issue| issue.diagnostic_code == Some("effect-unsupported"))
                .expect("unsupported effect issue");
            assert!(issue.resource_present_but_unsupported);
            assert!(
                issue
                    .detail
                    .as_deref()
                    .unwrap_or_default()
                    .contains("phase-10d"),
                "detail should point at phase-10d capability boundary: {:?}",
                issue.detail
            );
        }
    }

    #[test]
    fn phase10_graph_rejects_combo_that_requires_missing_authored_slot() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"Tint",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/tint/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/tint/effect.json"),
            br#"{"passes":[{"material":"materials/effects/tint.json"}]}"#,
        );
        write(
            &extracted.join("effects/tint/materials/effects/tint.json"),
            br#"{"passes":[{"shader":"effects/tint","combos":{"MASK":1}}]}"#,
        );
        write(
            &extracted.join("effects/tint/shaders/effects/tint.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/tint/shaders/effects/tint.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        let issue = report
            .issues
            .iter()
            .find(|issue| issue.diagnostic_code == Some("effect-unsupported"))
            .expect("unsupported effect issue");
        assert!(issue.resource_present_but_unsupported);
        assert!(issue
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("requires g_Texture1"));
    }

    #[test]
    fn phase10_graph_rejects_supported_family_use_case_that_exceeds_phase10b_local_uv_contract() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"WaterRipple",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/waterripple/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/waterripple/effect.json"),
            br#"{"passes":[{"material":"materials/effects/waterripple.json"}]}"#,
        );
        write(
            &extracted.join("effects/waterripple/materials/effects/waterripple.json"),
            br#"{"passes":[{"shader":"effects/waterripple","combos":{"PERSPECTIVE":1},"textures":[null,null,"textures/ripple.png"]}]}"#,
        );
        write(
            &extracted.join("effects/waterripple/shaders/effects/waterripple.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/waterripple/shaders/effects/waterripple.frag"),
            b"void main() {}",
        );
        write(&extracted.join("textures/ripple.png"), b"png");

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        let issue = report
            .issues
            .iter()
            .find(|issue| issue.diagnostic_code == Some("effect-unsupported"))
            .expect("unsupported effect issue");
        assert!(issue.resource_present_but_unsupported);
        assert!(issue
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("combo PERSPECTIVE=1"));
    }

    #[test]
    fn phase10_graph_accepts_effect_dependency_resolved_from_authored_shader_pair() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            b"fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":191,
                  "name":"Pulse",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/pulse/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/pulse/effect.json"),
            br#"{
              "dependencies":["effects/pulse-dependency"],
              "passes":[{"material":"materials/effects/pulse.json"}]
            }"#,
        );
        write(
            &extracted.join("effects/pulse/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.frag"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse-dependency.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse-dependency.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain.len(), 1);
        assert!(report.issues.iter().all(|issue| {
            issue.diagnostic_code != Some("effect-dependency-reference-unresolved")
        }));
    }

    #[test]
    fn phase10d_graph_reports_input_scope_blocker_diagnostic() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":92,
                  "name":"InputScopeFail",
                  "image":"models/inputscope.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/bad-input/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/inputscope.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/bad-input/effect.json"),
            br#"{
              "fbos":[{"name":"scratch"}],
              "passes":[{"material":"materials/effects/pulse.json","bind":[{"name":"nonexistent","index":2}]}]
            }"#,
        );
        write(
            &extracted.join("effects/bad-input/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/bad-input/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/bad-input/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(report.is_blocked());
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.code == super::SceneGraphIssueCode::InvalidEffect
                && issue.diagnostic_code == Some("effect-graph-scope-blocked")
                && matches!(issue.severity, super::SceneGraphIssueSeverity::Fatal))
            .expect("fatal graph-scope-blocked issue");
        assert!(issue
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("binding nonexistent"));
    }

    #[test]
    fn phase10d_graph_reports_cycle_order_invalid_for_unwritten_named_target() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":93,
                  "name":"CycleOrder",
                  "image":"models/cycleorder.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/order-cycle/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/cycleorder.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/order-cycle/effect.json"),
            br#"{
              "fbos":[{"name":"scratch"}],
              "passes":[
                {"material":"materials/effects/pulse.json","bind":[{"name":"scratch","index":1}]},
                {"material":"materials/effects/pulse.json","target":"scratch"}
              ]
            }"#,
        );
        write(
            &extracted.join("effects/order-cycle/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/order-cycle/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/order-cycle/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(report.is_blocked());
        let issue = report
            .issues
            .iter()
            .find(|issue| {
                issue.code == super::SceneGraphIssueCode::GraphCycleOrOrderInvalid
            })
            .expect("graph cycle or order invalid");
        assert_eq!(issue.severity, super::SceneGraphIssueSeverity::Fatal);
        assert_eq!(
            issue.diagnostic_code,
            Some("graph-cycle-or-order-invalid")
        );
        assert!(issue
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("graph-cycle-or-order-invalid"));
    }

    #[test]
    fn phase10d_graph_reports_construction_incomplete_for_unsupported_shader_in_graph_scope() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":94,
                  "name":"GraphIncomplete",
                  "image":"models/graphincomplete.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/graph-incomplete/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/graphincomplete.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/graph-incomplete/effect.json"),
            br#"{
              "fbos":[{"name":"scratch"}],
              "passes":[{"target":"scratch","material":"materials/effects/pulse.json"}]
            }"#,
        );
        write(
            &extracted.join("effects/graph-incomplete/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/graph-incomplete/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/graph-incomplete/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert!(report.graph.visuals[0].effect_chain.len() >= 1);
        assert!(report.issues.iter().all(|issue| {
            issue.code != super::SceneGraphIssueCode::GraphCycleOrOrderInvalid
        }));
    }

    #[test]
    fn phase10d_graph_accepts_valid_named_target_write_then_read_order() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":95,
                  "name":"ValidChain",
                  "image":"models/util/solidlayer.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/valid-chain/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/util/solidlayer.json"),
            br#"{"solidlayer":true}"#,
        );
        write(
            &extracted.join("effects/valid-chain/effect.json"),
            br#"{
              "fbos":[{"name":"scratch"}],
              "passes":[
                {"target":"scratch","material":"materials/effects/pulse.json"},
                {"bind":[{"name":"scratch","index":1}],"material":"materials/effects/pulse.json"}
              ]
            }"#,
        );
        write(
            &extracted.join("effects/valid-chain/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/valid-chain/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/valid-chain/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert_eq!(report.graph.visuals[0].effect_chain.len(), 1);
        assert!(report.issues.iter().all(|issue| {
            issue.code != super::SceneGraphIssueCode::GraphCycleOrOrderInvalid
        }));
        assert!(report.issues.iter().all(|issue| {
            issue.code != super::SceneGraphIssueCode::GraphTargetMissing
        }));
    }

    #[test]
    fn phase10d_graph_reports_target_missing_diagnostic_code_as_string_in_diagnostic_entry() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":96,
                  "name":"MissingTarget",
                  "image":"models/missingtarget.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/diag-target/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/missingtarget.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/diag-target/effect.json"),
            br#"{
              "passes":[{"target":"ghost","material":"materials/effects/pulse.json"}]
            }"#,
        );
        write(
            &extracted.join("effects/diag-target/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/diag-target/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/diag-target/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(report.is_blocked());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == super::SceneGraphIssueCode::GraphTargetMissing));
    }

    #[test]
    fn phase10d_graph_unsupported_diagnostic_uses_graph_construction_code_not_generic_invalid() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":97,
                  "name":"GraphOnly",
                  "image":"models/graphonly.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/graph-only/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/graphonly.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/graph-only/effect.json"),
            br#"{
              "fbos":[{"name":"blur_out"}],
              "passes":[{"target":"blur_out","material":"materials/effects/blur.json"}]
            }"#,
        );
        write(
            &extracted.join("effects/graph-only/materials/effects/blur.json"),
            br#"{"passes":[{"shader":"effects/blur"}]}"#,
        );
        write(
            &extracted.join("effects/graph-only/shaders/effects/blur.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/graph-only/shaders/effects/blur.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(report.is_blocked());
        assert!(report
            .issues
            .iter()
            .any(|issue| {
                issue.diagnostic_code == Some("effect-unsupported")
                    && issue.resource_present_but_unsupported
            }));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == super::SceneGraphIssueCode::InvalidEffect
                || issue.code == super::SceneGraphIssueCode::GraphConstructionIncomplete));
    }

    #[test]
    fn phase10d_graph_copybackground_effect_enters_phase10d_scope_with_copied_background_source() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");

        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            b"fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("scene.json"),
            br#"{
              "objects":[
                {
                  "id":98,
                  "name":"CopyBG",
                  "image":"models/copybg.model.json",
                  "origin":"960 540 0",
                  "size":"256 256",
                  "effects":[{"file":"effects/copybg/effect.json","visible":true}]
                }
              ]
            }"#,
        );
        write(
            &extracted.join("models/copybg.model.json"),
            br#"{"width":256,"height":256}"#,
        );
        write(
            &extracted.join("effects/copybg/effect.json"),
            br#"{
              "copybackground": true,
              "passes":[{"material":"materials/effects/pulse.json","bind":[{"name":"copybackground","index":1}]}]
            }"#,
        );
        write(
            &extracted.join("effects/copybg/materials/effects/pulse.json"),
            br#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &extracted.join("effects/copybg/shaders/effects/pulse.vert"),
            b"void main() {}",
        );
        write(
            &extracted.join("effects/copybg/shaders/effects/pulse.frag"),
            b"void main() {}",
        );

        let mut record = scene_record(&managed);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted.join("scene.json"),
                &extracted,
                &BTreeMap::new(),
            )
            .expect("manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let scene = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => panic!("expected scene runtime"),
        };
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let report = build_scene_phase10_graph(scene, &resolver);

        assert!(!report.is_blocked());
        assert_eq!(report.graph.visuals.len(), 1);
        assert!(report.graph.visuals[0].effect_chain.len() >= 1);
        let effect_node = &report.graph.visuals[0].effect_chain[0];
        assert!(effect_node.passes.iter().any(|pass| pass.copy_background));
        assert!(effect_node.passes.iter().any(|pass| {
            pass.input_bindings.iter().any(|binding| {
                matches!(
                    binding.source,
                    super::ScenePhase10InputSource::CopiedBackground
                )
            })
        }));
    }
}
