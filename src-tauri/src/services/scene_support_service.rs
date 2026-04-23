use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use tauri::AppHandle;

use crate::{
    models::{SceneAssetKind, SceneRuntimeDocument, WallpaperRecord, WallpaperType},
    services::{
        diagnostic_service,
        scene_diagnostics::{
            SceneDiagnosticDetail, SceneDiagnosticDomain, SceneDiagnosticEntry,
            SceneDiagnosticResourceDetail, SceneDiagnosticSeverity,
        },
        scene_render_graph_service::{
            build_scene_phase10_graph, SceneGraphIssue, SceneGraphIssueCode,
            SceneGraphIssueSeverity,
        },
        scene_render_planner_service::{
            build_scene_render_plan_with_resolver, SceneRenderIssue, SceneRenderIssueCode,
            SceneRenderIssueSeverity,
        },
        scene_resource_service::{
            builtin_scene_assets_root_for_app, font_reference_looks_like_path, SceneResourceLookup,
            SceneResourceResolver, SceneResourceRoot, SceneResourceRootKind, SceneShaderSourceKind,
        },
        scene_runtime_settings_service,
        scene_shader_material_service::{
            inspect_scene_effect_dependency, inspect_scene_shader_source_with_effect_context,
        },
    },
};

const DIAGNOSTIC_SUBSYSTEM: &str = "scene-support";
const APPLY_BLOCKED_CODE: &str = "apply-blocked";
const APPLY_WARNING_CODE: &str = "apply-warning";

#[derive(Debug, Clone, Copy)]
struct EffectPackageContext<'a> {
    root: &'a Path,
}

pub type SceneSupportSeverity = SceneDiagnosticSeverity;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SceneSupportErrorCode {
    MissingSceneJson,
    MissingResource,
    UnsupportedVisualAsset,
    MissingRenderBounds,
    NoRenderableVisuals,
    MissingVisualSource,
    MissingMaterial,
    MissingPuppet,
    InvalidPuppet,
    InvalidMaterial,
    MissingTextureBinding,
    InvalidEffect,
    TextFontReferenceUnresolved,
    TextEffectReferenceUnresolved,
    TextEffectUnsupported,
    AudioRenderBoundsMissing,
    SoundAssetReferenceUnresolved,
    ParticleResourceReferenceUnresolved,
    ParticleResourceUnsupported,
    VideoTextureSourceMissing,
    InputSnapshotUnavailable,
    AudioInputUnavailable,
}

pub type SceneSupportError = SceneDiagnosticEntry;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneSupportReport {
    pub wallpaper_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_json_path: Option<String>,
    pub resource_roots: Vec<SceneResourceRoot>,
    pub errors: Vec<SceneSupportError>,
    pub warnings: Vec<SceneSupportError>,
}

impl SceneSupportReport {
    pub fn is_supported(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    pub fn apply_error_message(&self) -> String {
        if self.errors.is_empty() {
            return "Scene native apply is supported.".to_string();
        }

        let preview = self
            .errors
            .iter()
            .take(4)
            .map(|error| error.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let remaining = self.errors.len().saturating_sub(4);
        let suffix = if remaining == 0 {
            String::new()
        } else {
            format!("; and {remaining} more blocking issue(s)")
        };

        format!("Scene native apply is blocked in the native Scene runtime: {preview}{suffix}")
    }

    pub fn warning_summary(&self) -> Option<String> {
        if self.warnings.is_empty() {
            return None;
        }

        let preview = self
            .warnings
            .iter()
            .take(4)
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let remaining = self.warnings.len().saturating_sub(4);
        let suffix = if remaining == 0 {
            String::new()
        } else {
            format!("; and {remaining} more warning(s)")
        };

        Some(format!(
            "Scene native apply entered the native Scene runtime with warnings: {preview}{suffix}"
        ))
    }

    pub fn diagnostic_detail(&self) -> Option<String> {
        serde_json::to_string_pretty(self).ok()
    }
}

fn missing_scene_json() -> SceneSupportError {
    SceneDiagnosticEntry::fatal(
        "scene-json-missing",
        "Scene resource graph is missing scene.json.",
    )
    .with_resource_path(Some("scene.json"))
    .with_detail(SceneDiagnosticDetail::resource(
        SceneDiagnosticDomain::Scene,
        SceneDiagnosticResourceDetail {
            authored_reference: "scene.json".to_string(),
            attempted_roots: Vec::new(),
            attempted_candidates: vec!["scene.json".to_string()],
            matched_root_kind: None,
            matched_path: None,
            external_assets_available: false,
            builtin_assets_available: false,
            reference_resolved: false,
            present_but_unsupported: false,
            family_candidates: Vec::new(),
        },
    ))
}

pub fn clear_scene_support_diagnostics(app: &AppHandle) {
    let _ = diagnostic_service::clear_subsystem(app, DIAGNOSTIC_SUBSYSTEM);
}

pub fn analyze_scene_support_for_app(
    app: &AppHandle,
    record: &WallpaperRecord,
    runtime_scene: Option<&SceneRuntimeDocument>,
) -> SceneSupportReport {
    analyze_scene_support_with_resolver(
        record,
        SceneResourceResolver::for_managed_root_with_asset_roots(
            &record.managed_path,
            builtin_scene_assets_root_for_app(app),
            scene_runtime_settings_service::external_assets_root_for_app(app),
        ),
        runtime_scene,
    )
}

#[cfg(test)]
pub fn analyze_scene_support_with_builtin_root(
    record: &WallpaperRecord,
    builtin_assets_root: impl AsRef<Path>,
    runtime_scene: Option<&SceneRuntimeDocument>,
) -> SceneSupportReport {
    analyze_scene_support_with_resolver(
        record,
        SceneResourceResolver::for_managed_root_with_builtin_root(
            &record.managed_path,
            builtin_assets_root,
        ),
        runtime_scene,
    )
}

fn analyze_scene_support_with_resolver(
    record: &WallpaperRecord,
    resolver: SceneResourceResolver,
    runtime_scene: Option<&SceneRuntimeDocument>,
) -> SceneSupportReport {
    let mut report = SceneSupportReport {
        wallpaper_id: record.id.clone(),
        title: record.title.clone(),
        scene_json_path: resolver
            .scene_json_path()
            .map(|path| path.display().to_string()),
        resource_roots: resolver.resource_roots(),
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
        return report;
    }

    let Some(scene_json_path) = resolver.scene_json_path() else {
        report.errors.push(missing_scene_json());
        return report;
    };

    let scene_json = match read_json(&scene_json_path) {
        Some(scene_json) => scene_json,
        None => {
            report.errors.push(
                SceneDiagnosticEntry::fatal(
                    "scene-json-invalid",
                    "Scene JSON could not be parsed for native Scene diagnostics.",
                )
                .with_resource_path(Some(scene_json_path.display().to_string()))
                .with_detail(
                    SceneDiagnosticDetail::resource(
                        SceneDiagnosticDomain::Scene,
                        SceneDiagnosticResourceDetail::from_lookup(
                            &resolver.inspect_relative_path("scene.json"),
                        ),
                    )
                    .with_note("Scene JSON could not be parsed."),
                ),
            );
            return report;
        }
    };

    let objects = scene_json
        .get("objects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for object in &objects {
        analyze_object(object, &resolver, &mut report.warnings);
    }

    if let Some(runtime_scene) = runtime_scene {
        let render_plan = build_scene_render_plan_with_resolver(runtime_scene, Some(&resolver));
        for issue in render_plan.issues {
            let support_error = support_error_from_render_issue(issue, runtime_scene);
            match support_error.severity {
                SceneSupportSeverity::Fatal => push_unique_issue(&mut report.errors, support_error),
                SceneSupportSeverity::Warning => {
                    push_unique_issue(&mut report.warnings, support_error)
                }
            }
        }

        let graph_report = build_scene_phase10_graph(runtime_scene, &resolver);
        for issue in graph_report.issues {
            let support_error = support_error_from_graph_issue(issue);
            match support_error.severity {
                SceneSupportSeverity::Fatal => push_unique_issue(&mut report.errors, support_error),
                SceneSupportSeverity::Warning => {
                    push_unique_issue(&mut report.warnings, support_error)
                }
            }
        }
    }

    report.errors.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then(left.message.cmp(&right.message))
    });
    report.warnings.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then(left.message.cmp(&right.message))
    });
    report
}

pub fn ensure_scene_supported_for_apply(
    app: &AppHandle,
    record: &WallpaperRecord,
    runtime_scene: Option<&SceneRuntimeDocument>,
) -> Result<(), String> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
        clear_scene_support_diagnostics(app);
        return Ok(());
    }

    let report = analyze_scene_support_for_app(app, record, runtime_scene);
    if report.is_supported() {
        let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, APPLY_BLOCKED_CODE);
        if report.has_warnings() {
            let summary = report.warning_summary().unwrap_or_else(|| {
                "Scene native apply entered the native Scene runtime with warnings.".to_string()
            });
            let _ = diagnostic_service::record_warning(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                APPLY_WARNING_CODE,
                summary,
                report.diagnostic_detail(),
            );
        } else {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, APPLY_WARNING_CODE);
        }
        return Ok(());
    }

    let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, APPLY_WARNING_CODE);
    let _ = diagnostic_service::record_error(
        app,
        DIAGNOSTIC_SUBSYSTEM,
        APPLY_BLOCKED_CODE,
        "Scene native apply was blocked by support diagnostics.",
        report.diagnostic_detail(),
    );
    Err(report.apply_error_message())
}

fn support_error_from_render_issue(
    issue: SceneRenderIssue,
    runtime_scene: &SceneRuntimeDocument,
) -> SceneSupportError {
    let severity = match issue.severity {
        SceneRenderIssueSeverity::Warning => SceneDiagnosticSeverity::Warning,
        SceneRenderIssueSeverity::Fatal => SceneDiagnosticSeverity::Fatal,
    };
    let object_kind = issue.object_kind.as_deref().unwrap_or("scene");
    let (domain, code) = match issue.code {
        SceneRenderIssueCode::UnsupportedVisualAsset => {
            (SceneDiagnosticDomain::Visual, "visual-asset-unsupported")
        }
        SceneRenderIssueCode::MissingAssetPath | SceneRenderIssueCode::MissingAssetFile => {
            match object_kind {
                "sound" => (
                    SceneDiagnosticDomain::Sound,
                    "sound-asset-reference-unresolved",
                ),
                "visual" if render_issue_targets_video_texture(runtime_scene, issue.object_id) => (
                    SceneDiagnosticDomain::VideoTexture,
                    "video-texture-source-missing",
                ),
                "visual" => (
                    SceneDiagnosticDomain::Visual,
                    "visual-asset-reference-unresolved",
                ),
                _ => (
                    SceneDiagnosticDomain::Scene,
                    "resource-reference-unresolved",
                ),
            }
        }
        SceneRenderIssueCode::MissingRenderBounds => match object_kind {
            "text" => (SceneDiagnosticDomain::Text, "text-render-bounds-missing"),
            "audio" => (SceneDiagnosticDomain::Audio, "audio-render-bounds-missing"),
            "visual" => (
                SceneDiagnosticDomain::Visual,
                "visual-render-bounds-missing",
            ),
            _ => (SceneDiagnosticDomain::Scene, "render-bounds-missing"),
        },
        SceneRenderIssueCode::NoRenderableVisuals => {
            (SceneDiagnosticDomain::Scene, "scene-output-empty")
        }
    };

    let entry = SceneDiagnosticEntry::new(severity, code, issue.message)
        .with_object(issue.object_id, issue.object_name, issue.object_kind)
        .with_resource_path(issue.resource_path);
    if let Some(detail) = issue.detail {
        return entry.with_detail(SceneDiagnosticDetail::capability(domain, detail));
    }
    entry
}

fn support_error_from_graph_issue(issue: SceneGraphIssue) -> SceneSupportError {
    let severity = match issue.severity {
        SceneGraphIssueSeverity::Warning => SceneDiagnosticSeverity::Warning,
        SceneGraphIssueSeverity::Fatal => SceneDiagnosticSeverity::Fatal,
    };
    let (default_code, domain) = match issue.code {
        SceneGraphIssueCode::MissingVisualSource => {
            ("visual-source-missing", SceneDiagnosticDomain::Visual)
        }
        SceneGraphIssueCode::MissingMaterial => (
            "material-reference-unresolved",
            SceneDiagnosticDomain::Visual,
        ),
        SceneGraphIssueCode::MissingPuppet => {
            ("model-reference-unresolved", SceneDiagnosticDomain::Visual)
        }
        SceneGraphIssueCode::InvalidPuppet => ("model-invalid", SceneDiagnosticDomain::Visual),
        SceneGraphIssueCode::InvalidMaterial => ("material-invalid", SceneDiagnosticDomain::Visual),
        SceneGraphIssueCode::MissingTextureBinding => (
            "material-texture-binding-missing",
            SceneDiagnosticDomain::Visual,
        ),
        SceneGraphIssueCode::InvalidEffect => ("effect-invalid", SceneDiagnosticDomain::Visual),
    };
    let code = issue.diagnostic_code.unwrap_or(default_code);

    let entry = SceneDiagnosticEntry::new(severity, code, issue.message)
        .with_object(issue.object_id, issue.object_name, Some("visual"))
        .with_resource_path(issue.resource_path);
    if let Some(lookup) = issue.resource_lookup.as_ref() {
        let resource_detail = if issue.resource_present_but_unsupported {
            SceneDiagnosticResourceDetail::from_lookup(lookup).mark_present_but_unsupported()
        } else {
            SceneDiagnosticResourceDetail::from_lookup(lookup)
        };
        let detail = if let Some(reason) = issue.detail {
            SceneDiagnosticDetail::resource(domain, resource_detail).with_note(reason)
        } else {
            SceneDiagnosticDetail::resource(domain, resource_detail)
        };
        return entry.with_detail(detail);
    }
    if let Some(detail) = issue.detail {
        return entry.with_detail(SceneDiagnosticDetail::capability(domain, detail));
    }
    entry
}

fn analyze_object(
    object: &Value,
    resolver: &SceneResourceResolver,
    warnings: &mut Vec<SceneSupportError>,
) {
    let object_id = object
        .get("id")
        .and_then(Value::as_u64)
        .map(|value| value as u32);
    let object_name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());

    let image_path = object.get("image").and_then(Value::as_str);
    let particle_path = object.get("particle").and_then(Value::as_str);
    let sound_paths = object
        .get("sound")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let is_text = object.get("text").is_some()
        || object.get("font").is_some()
        || object.get("pointsize").is_some();

    if let Some(image_path) = image_path {
        analyze_visual_object(
            object,
            resolver,
            warnings,
            object_id,
            object_name,
            image_path,
        );
    }

    if is_text {
        if let Some(font_reference) = object
            .get("font")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            require_text_font_resource(warnings, resolver, object_id, object_name, font_reference);
        }
        analyze_text_effect_paths(
            warnings,
            resolver,
            object_id,
            object_name,
            effect_paths(object),
        );
    }

    if let Some(particle_path) = particle_path {
        analyze_particle_resource(warnings, resolver, object_id, object_name, particle_path);
    }

    for sound_path in sound_paths {
        analyze_sound_resource(warnings, resolver, object_id, object_name, sound_path);
    }

    if !is_text && image_path.is_none() {
        analyze_generic_effect_paths(
            warnings,
            resolver,
            object_id,
            object_name,
            effect_paths(object),
        );
    }
}

fn analyze_visual_object(
    object: &Value,
    resolver: &SceneResourceResolver,
    warnings: &mut Vec<SceneSupportError>,
    object_id: Option<u32>,
    object_name: Option<&str>,
    image_path: &str,
) {
    let lower_image_path = image_path.to_ascii_lowercase();
    let is_solid_layer = lower_image_path.contains("solidlayer");
    let model_lookup = resolver.inspect_relative_path(image_path);
    let model_json_path = model_lookup.matched_path.clone();

    if model_json_path.is_none() && !is_solid_layer {
        push_unique_issue(
            warnings,
            unresolved_resource_warning(
                "visual-model-reference-unresolved",
                SceneDiagnosticDomain::Visual,
                object_id,
                object_name,
                Some("visual"),
                image_path,
                &model_lookup,
                "Visual model JSON could not be resolved from Scene search roots.",
            ),
        );
        return;
    }

    let model_json = model_json_path.as_deref().and_then(read_json);
    if model_json_path.is_some() && model_json.is_none() {
        push_unique_issue(
            warnings,
            SceneDiagnosticEntry::warning(
                "visual-model-invalid",
                format!(
                    "{} could not parse visual model JSON {}.",
                    object_name
                        .map(quoted)
                        .unwrap_or_else(|| "Scene".to_string()),
                    image_path
                ),
            )
            .with_object(object_id, object_name, Some("visual"))
            .with_resource_path(Some(image_path))
            .with_detail(
                SceneDiagnosticDetail::resource(
                    SceneDiagnosticDomain::Visual,
                    SceneDiagnosticResourceDetail::from_lookup(&model_lookup),
                )
                .with_note("Visual model JSON could not be parsed."),
            ),
        );
        return;
    }

    if let Some(puppet_path) = model_json
        .as_ref()
        .and_then(|value| value.get("puppet"))
        .and_then(Value::as_str)
    {
        require_resource(
            warnings,
            resolver,
            SceneDiagnosticDomain::Visual,
            "model-reference-unresolved",
            object_id,
            object_name,
            Some("visual"),
            puppet_path,
            "Puppet resource could not be resolved from Scene search roots.",
        );
    }

    let Some(material_path) = model_json
        .as_ref()
        .and_then(|value| value.get("material"))
        .and_then(Value::as_str)
    else {
        analyze_generic_effect_paths(
            warnings,
            resolver,
            object_id,
            object_name,
            effect_paths(object),
        );
        return;
    };

    let Some(material_json_path) = resolver.resolve_relative_path(material_path) else {
        push_unique_issue(
            warnings,
            unresolved_resource_warning(
                "material-reference-unresolved",
                SceneDiagnosticDomain::Visual,
                object_id,
                object_name,
                Some("visual"),
                material_path,
                &resolver.inspect_relative_path(material_path),
                "Material JSON could not be resolved from Scene search roots.",
            ),
        );
        analyze_generic_effect_paths(
            warnings,
            resolver,
            object_id,
            object_name,
            effect_paths(object),
        );
        return;
    };

    let material_json = match read_json(&material_json_path) {
        Some(material_json) => material_json,
        None => {
            push_unique_issue(
                warnings,
                SceneDiagnosticEntry::warning(
                    "material-invalid",
                    format!(
                        "{} could not parse material JSON {}.",
                        object_name
                            .map(quoted)
                            .unwrap_or_else(|| "Scene".to_string()),
                        material_path
                    ),
                )
                .with_object(object_id, object_name, Some("visual"))
                .with_resource_path(Some(material_path))
                .with_detail(
                    SceneDiagnosticDetail::resource(
                        SceneDiagnosticDomain::Visual,
                        SceneDiagnosticResourceDetail::from_lookup(
                            &resolver.inspect_relative_path(material_path),
                        ),
                    )
                    .with_note("Material JSON could not be parsed."),
                ),
            );
            return;
        }
    };

    let mut visited_effects = BTreeSet::new();
    analyze_material_resources(
        warnings,
        resolver,
        object_id,
        object_name,
        material_path,
        &material_json_path,
        &material_json,
        None,
        &mut visited_effects,
    );
    analyze_generic_effect_paths_with_context(
        warnings,
        resolver,
        object_id,
        object_name,
        effect_paths(object),
        None,
        &mut visited_effects,
    );
}

fn analyze_generic_effect_paths(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    effect_paths: Vec<&str>,
) {
    let mut visited_effects = BTreeSet::new();
    analyze_generic_effect_paths_with_context(
        warnings,
        resolver,
        object_id,
        object_name,
        effect_paths,
        None,
        &mut visited_effects,
    );
}

fn analyze_generic_effect_paths_with_context(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    effect_paths: Vec<&str>,
    context: Option<EffectPackageContext<'_>>,
    visited_effects: &mut BTreeSet<PathBuf>,
) {
    for effect_path in effect_paths {
        let lookup = inspect_with_effect_context(resolver, effect_path, context);
        if let Some(effect_json_path) = lookup.matched_path.as_deref() {
            analyze_effect_package(
                warnings,
                resolver,
                object_id,
                object_name,
                effect_path,
                effect_json_path,
                &lookup,
                visited_effects,
            );
        } else {
            push_unique_issue(
                warnings,
                unresolved_resource_warning(
                    "resource-reference-unresolved",
                    SceneDiagnosticDomain::Visual,
                    object_id,
                    object_name,
                    Some("visual"),
                    effect_path,
                    &lookup,
                    "Effect resource could not be resolved from Scene search roots.",
                ),
            );
        }
    }
}

fn analyze_effect_package(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    effect_path: &str,
    effect_json_path: &Path,
    effect_lookup: &SceneResourceLookup,
    visited_effects: &mut BTreeSet<PathBuf>,
) {
    if !visited_effects.insert(effect_json_path.to_path_buf()) {
        return;
    }

    let Some(effect_json) = read_json(effect_json_path) else {
        push_unique_issue(
            warnings,
            SceneDiagnosticEntry::warning(
                "effect-invalid",
                format!(
                    "{} could not parse effect package {}.",
                    object_name
                        .map(quoted)
                        .unwrap_or_else(|| "Scene".to_string()),
                    effect_path
                ),
            )
            .with_object(object_id, object_name, Some("visual"))
            .with_resource_path(Some(effect_path))
            .with_detail(
                SceneDiagnosticDetail::resource(
                    SceneDiagnosticDomain::Visual,
                    SceneDiagnosticResourceDetail::from_lookup(effect_lookup),
                )
                .with_note("Effect package JSON could not be parsed."),
            ),
        );
        return;
    };

    let effect_package_root = effect_json_path.parent().unwrap_or(effect_json_path);
    let context = Some(EffectPackageContext {
        root: effect_package_root,
    });

    for dependency in effect_json
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let lookup = if let Some(context) = context {
            inspect_scene_effect_dependency(resolver, dependency, context.root)
        } else {
            resolver.inspect_relative_path(dependency)
        };
        if lookup.matched_path.is_none() {
            push_unique_issue(
                warnings,
                unresolved_resource_warning(
                    "effect-dependency-reference-unresolved",
                    SceneDiagnosticDomain::Visual,
                    object_id,
                    object_name,
                    Some("visual"),
                    dependency,
                    &lookup,
                    "Effect dependency could not be resolved from the effect package before Scene search roots.",
                ),
            );
        }
    }

    for pass in effect_json
        .get("passes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(material_path) = pass.get("material").and_then(Value::as_str) else {
            continue;
        };
        let material_lookup = inspect_with_effect_context(resolver, material_path, context);
        let Some(material_json_path) = material_lookup.matched_path.as_deref() else {
            push_unique_issue(
                warnings,
                unresolved_resource_warning(
                    "effect-material-reference-unresolved",
                    SceneDiagnosticDomain::Visual,
                    object_id,
                    object_name,
                    Some("visual"),
                    material_path,
                    &material_lookup,
                    "Effect pass material could not be resolved from the effect package before Scene search roots.",
                ),
            );
            continue;
        };
        let Some(material_json) = read_json(material_json_path) else {
            push_unique_issue(
                warnings,
                SceneDiagnosticEntry::warning(
                    "effect-material-invalid",
                    format!(
                        "{} could not parse effect material {}.",
                        object_name
                            .map(quoted)
                            .unwrap_or_else(|| "Scene".to_string()),
                        material_path
                    ),
                )
                .with_object(object_id, object_name, Some("visual"))
                .with_resource_path(Some(material_path))
                .with_detail(
                    SceneDiagnosticDetail::resource(
                        SceneDiagnosticDomain::Visual,
                        SceneDiagnosticResourceDetail::from_lookup(&material_lookup),
                    )
                    .with_note("Effect pass material JSON could not be parsed."),
                ),
            );
            continue;
        };
        analyze_material_resources(
            warnings,
            resolver,
            object_id,
            object_name,
            material_path,
            material_json_path,
            &material_json,
            context,
            visited_effects,
        );
    }
}

fn analyze_material_resources(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    material_path: &str,
    material_json_path: &Path,
    material_json: &Value,
    context: Option<EffectPackageContext<'_>>,
    visited_effects: &mut BTreeSet<PathBuf>,
) {
    for pass in material_pass_values(material_json) {
        if let Some(shader_path) = pass
            .get("shader")
            .and_then(Value::as_str)
            .or_else(|| material_json.get("shader").and_then(Value::as_str))
        {
            if looks_like_scene_resource_path(shader_path) {
                let lookup = inspect_scene_shader_source_with_effect_context(
                    resolver,
                    shader_path,
                    context.map(|context| context.root),
                );
                if lookup.kind == SceneShaderSourceKind::Missing {
                    push_unique_issue(
                        warnings,
                        unresolved_resource_warning(
                            "shader-reference-unresolved",
                            SceneDiagnosticDomain::Visual,
                            object_id,
                            object_name,
                            Some("visual"),
                            shader_path,
                            &lookup.lookup,
                            "Shader resource could not be resolved from the active material/effect package roots.",
                        ),
                    );
                }
            }
        }

        if let Some(textures) = pass
            .get("textures")
            .and_then(Value::as_array)
            .or_else(|| material_json.get("textures").and_then(Value::as_array))
        {
            for texture_name in textures.iter().filter_map(Value::as_str) {
                let texture_lookup = inspect_texture_with_effect_context(
                    resolver,
                    Some(material_path),
                    Some(material_json_path),
                    texture_name,
                    context,
                );
                if texture_lookup.matched_path.is_none() {
                    push_unique_issue(
                        warnings,
                        SceneDiagnosticEntry::warning(
                            "material-texture-reference-unresolved",
                            format!(
                                "{} is missing material texture {}.",
                                object_name
                                    .map(quoted)
                                    .unwrap_or_else(|| "Scene".to_string()),
                                texture_name
                            ),
                        )
                        .with_object(object_id, object_name, Some("visual"))
                        .with_resource_path(Some(texture_name))
                        .with_detail(
                            SceneDiagnosticDetail::resource(
                                SceneDiagnosticDomain::Visual,
                                SceneDiagnosticResourceDetail::from_lookup(&texture_lookup),
                            )
                            .with_note(
                                "Material texture is missing from the active material/effect package roots.",
                            ),
                        ),
                    );
                }
            }
        }

        analyze_generic_effect_paths_with_context(
            warnings,
            resolver,
            object_id,
            object_name,
            effect_paths(pass),
            context,
            visited_effects,
        );
    }

    analyze_generic_effect_paths_with_context(
        warnings,
        resolver,
        object_id,
        object_name,
        effect_paths(material_json),
        context,
        visited_effects,
    );
}

fn inspect_with_effect_context(
    resolver: &SceneResourceResolver,
    resource_path: &str,
    context: Option<EffectPackageContext<'_>>,
) -> SceneResourceLookup {
    if let Some(context) = context {
        resolver.inspect_relative_path_with_local_root(
            resource_path,
            SceneResourceRootKind::EffectPackage,
            context.root,
        )
    } else {
        resolver.inspect_relative_path(resource_path)
    }
}

fn inspect_texture_with_effect_context(
    resolver: &SceneResourceResolver,
    material_path: Option<&str>,
    material_json_path: Option<&Path>,
    texture_name: &str,
    context: Option<EffectPackageContext<'_>>,
) -> SceneResourceLookup {
    if let Some(context) = context {
        resolver
            .inspect_texture_candidates_with_local_root(
                material_path,
                material_json_path,
                texture_name,
                SceneResourceRootKind::EffectPackage,
                context.root,
            )
            .lookup
    } else {
        resolver
            .inspect_texture_candidates(material_path, material_json_path, texture_name)
            .lookup
    }
}

fn material_pass_values(json: &Value) -> Vec<&Value> {
    if let Some(passes) = json.get("passes").and_then(Value::as_array) {
        return passes.iter().collect();
    }
    if json
        .get("shader")
        .and_then(Value::as_str)
        .map(|shader| !shader.trim().is_empty())
        .unwrap_or(false)
        || json
            .get("textures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
    {
        return vec![json];
    }
    Vec::new()
}

fn looks_like_scene_resource_path(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.contains('/')
        || lower.contains('\\')
        || lower.ends_with(".metal")
        || lower.ends_with(".json")
}

fn require_resource(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    domain: SceneDiagnosticDomain,
    code: &str,
    object_id: Option<u32>,
    object_name: Option<&str>,
    object_kind: Option<&str>,
    resource_path: &str,
    reason: &str,
) {
    let lookup = resolver.inspect_relative_path(resource_path);
    if lookup.matched_path.is_some() {
        return;
    }

    push_unique_issue(
        warnings,
        unresolved_resource_warning(
            code,
            domain,
            object_id,
            object_name,
            object_kind,
            resource_path,
            &lookup,
            reason,
        ),
    );
}

fn require_text_font_resource(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    font_reference: &str,
) {
    let resolved = resolver.inspect_text_font(font_reference);
    if resolved.lookup.matched_path.is_some() || !font_reference_looks_like_path(font_reference) {
        return;
    }

    push_unique_issue(
        warnings,
        SceneDiagnosticEntry::warning(
            "text-font-reference-unresolved",
            format!(
                "{} could not resolve text font {} for native Scene text rendering.",
                object_name
                    .map(quoted)
                    .unwrap_or_else(|| "Scene".to_string()),
                font_reference
            ),
        )
        .with_object(object_id, object_name, Some("text"))
        .with_resource_path(Some(font_reference))
        .with_detail(
            SceneDiagnosticDetail::resource(
                SceneDiagnosticDomain::Text,
                SceneDiagnosticResourceDetail::from_text_font_lookup(&resolved),
            )
            .with_note("Text font could not be resolved from Scene font roots or builtin assets."),
        ),
    );
}

fn analyze_text_effect_paths(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    effect_paths: Vec<&str>,
) {
    for effect_path in effect_paths {
        if effect_path.to_ascii_lowercase().contains("blur") {
            continue;
        }
        let lookup = resolver.inspect_relative_path(effect_path);
        let object_label = object_name
            .map(quoted)
            .unwrap_or_else(|| "Scene".to_string());
        let detail = SceneDiagnosticDetail::resource(
            SceneDiagnosticDomain::Text,
            if lookup.matched_path.is_some() {
                SceneDiagnosticResourceDetail::from_lookup(&lookup).mark_present_but_unsupported()
            } else {
                SceneDiagnosticResourceDetail::from_lookup(&lookup)
            },
        )
        .with_note(
            "Phase-09 text keeps blur support in the raster path; deeper effect semantics stay outside the current native text baseline.",
        );
        let warning = if lookup.matched_path.is_some() {
            SceneDiagnosticEntry::warning(
                "text-effect-unsupported",
                format!(
                    "{object_label} references text effect {effect_path}, but the current native text path does not execute that effect."
                ),
            )
        } else {
            SceneDiagnosticEntry::warning(
                "text-effect-reference-unresolved",
                format!("{object_label} could not resolve text effect {effect_path}."),
            )
        };
        push_unique_issue(
            warnings,
            warning
                .with_object(object_id, object_name, Some("text"))
                .with_resource_path(Some(effect_path))
                .with_detail(detail),
        );
    }
}

fn analyze_particle_resource(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    particle_path: &str,
) {
    let lookup = resolver.inspect_relative_path(particle_path);
    let object_label = object_name
        .map(quoted)
        .unwrap_or_else(|| "Scene".to_string());
    let detail = SceneDiagnosticDetail::resource(
        SceneDiagnosticDomain::Particle,
        if lookup.matched_path.is_some() {
            SceneDiagnosticResourceDetail::from_lookup(&lookup).mark_present_but_unsupported()
        } else {
            SceneDiagnosticResourceDetail::from_lookup(&lookup)
        },
    )
    .with_note(
        "Phase-09 particle rendering consumes evaluated kind/size/emission data, but does not execute authored particle JSON semantics as a first-class runtime module yet.",
    );
    let warning = if lookup.matched_path.is_some() {
        SceneDiagnosticEntry::warning(
            "particle-resource-unsupported",
            format!(
                "{object_label} resolves particle resource {particle_path}, but the current native particle path does not execute authored particle system JSON semantics."
            ),
        )
    } else {
        SceneDiagnosticEntry::warning(
            "particle-resource-reference-unresolved",
            format!("{object_label} could not resolve particle resource {particle_path}."),
        )
    };
    push_unique_issue(
        warnings,
        warning
            .with_object(object_id, object_name, Some("particle"))
            .with_resource_path(Some(particle_path))
            .with_detail(detail),
    );
}

fn analyze_sound_resource(
    warnings: &mut Vec<SceneSupportError>,
    resolver: &SceneResourceResolver,
    object_id: Option<u32>,
    object_name: Option<&str>,
    sound_path: &str,
) {
    require_resource(
        warnings,
        resolver,
        SceneDiagnosticDomain::Sound,
        "sound-asset-reference-unresolved",
        object_id,
        object_name,
        Some("sound"),
        sound_path,
        "Sound asset could not be resolved from Scene search roots.",
    );
}

fn unresolved_resource_warning(
    code: &str,
    domain: SceneDiagnosticDomain,
    object_id: Option<u32>,
    object_name: Option<&str>,
    object_kind: Option<&str>,
    resource_path: &str,
    lookup: &SceneResourceLookup,
    reason: &str,
) -> SceneSupportError {
    SceneDiagnosticEntry::warning(
        code,
        format!(
            "{} could not resolve resource {}.",
            object_name
                .map(quoted)
                .unwrap_or_else(|| "Scene".to_string()),
            resource_path
        ),
    )
    .with_object(object_id, object_name, object_kind)
    .with_resource_path(Some(resource_path))
    .with_detail(
        SceneDiagnosticDetail::resource(domain, SceneDiagnosticResourceDetail::from_lookup(lookup))
            .with_note(reason),
    )
}

fn render_issue_targets_video_texture(
    runtime_scene: &SceneRuntimeDocument,
    object_id: Option<u32>,
) -> bool {
    let Some(object_id) = object_id else {
        return false;
    };
    matches!(
        runtime_scene.evaluated.objects.get(&object_id),
        Some(crate::models::EvaluatedSceneObject::Visual {
            asset_kind: SceneAssetKind::Video,
            ..
        })
    )
}

fn effect_paths(value: &Value) -> Vec<&str> {
    value
        .get("effects")
        .and_then(Value::as_array)
        .map(|effects| {
            effects
                .iter()
                .filter_map(|effect| {
                    effect
                        .get("file")
                        .and_then(Value::as_str)
                        .or_else(|| effect.as_str())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn push_unique_issue(errors: &mut Vec<SceneSupportError>, candidate: SceneSupportError) {
    if errors.iter().any(|existing| existing == &candidate) {
        return;
    }
    errors.push(candidate);
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::{
        models::{SceneManifest, WallpaperRecord, WallpaperType},
        services::{
            runtime_document_service,
            scene_resource_service::{SceneResourceResolver, SceneResourceRootKind},
        },
    };

    use super::{analyze_scene_support_with_builtin_root, analyze_scene_support_with_resolver};

    fn write_video_tex_fixture(path: &Path) {
        let payload = b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TEXV0005\0");
        bytes.extend_from_slice(b"TEXI0001\0");
        bytes.extend_from_slice(&(0_u32).to_le_bytes());
        bytes.extend_from_slice(&(32_u32).to_le_bytes());
        bytes.extend_from_slice(&(1920_u32).to_le_bytes());
        bytes.extend_from_slice(&(1080_u32).to_le_bytes());
        bytes.extend_from_slice(&(1920_u32).to_le_bytes());
        bytes.extend_from_slice(&(1080_u32).to_le_bytes());
        bytes.extend_from_slice(&(0_u32).to_le_bytes());
        bytes.extend_from_slice(b"TEXB0004\0");
        bytes.extend_from_slice(&(1_u32).to_le_bytes());
        bytes.extend_from_slice(&(u32::MAX).to_le_bytes());
        bytes.extend_from_slice(&(1_u32).to_le_bytes());
        bytes.extend_from_slice(&(1_u32).to_le_bytes());
        bytes.extend_from_slice(&(1920_u32).to_le_bytes());
        bytes.extend_from_slice(&(1080_u32).to_le_bytes());
        bytes.extend_from_slice(&(0_u32).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(payload);
        fs::write(path, bytes).expect("video tex fixture");
    }

    fn scene_record(managed_path: &Path) -> WallpaperRecord {
        WallpaperRecord {
            id: "scene-demo".to_string(),
            title: "Scene Demo".to_string(),
            wallpaper_type: WallpaperType::Scene,
            source_path: managed_path.display().to_string(),
            managed_path: managed_path.display().to_string(),
            preview_path: None,
            entry_path: None,
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

    #[test]
    fn empty_scene_is_blocked_when_phase_08_has_no_renderable_visuals() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(&extracted_root).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(extracted_root.join("scene.json"), r#"{"objects":[]}"#).expect("scene json");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        assert!(!report.is_supported());
        assert!(report
            .errors
            .iter()
            .any(|error| error.code == "scene-output-empty"));
    }

    #[test]
    fn phase_10_support_report_accepts_runtime_text_without_raw_font_warning() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        let builtin_font = builtin_root.join("assets").join("fonts").join("clock.ttf");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(builtin_font.parent().expect("builtin font dir"))
            .expect("builtin font dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(&builtin_font, b"font").expect("builtin font");

        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 1,
                  "name": "Hero",
                  "image": "models/solidlayer.json",
                  "size": "256 128",
                  "color": "1 0.5 0"
                },
                {
                  "id": 2,
                  "name": "Clock",
                  "text": "12:34",
                  "font": "fonts/clock.ttf"
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("solidlayer.json"),
            r#"{"solidlayer":true,"width":256,"height":128}"#,
        )
        .expect("solidlayer model");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        assert!(report.is_supported());
        assert!(!report.has_warnings());
        assert_eq!(report.errors.len(), 0);
    }

    #[test]
    fn support_report_accepts_font_family_without_missing_resource_warning() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(&extracted_root).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 2,
                  "name": "Clock",
                  "text": "12:34",
                  "font": "DIN Alternate"
                }
              ]
            }"#,
        )
        .expect("scene json");

        let record = scene_record(&managed_root);
        let report = analyze_scene_support_with_builtin_root(&record, &builtin_root, None);

        assert!(report.is_supported());
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn support_report_accepts_builtin_font_asset_when_runtime_scene_is_missing() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        let builtin_font = builtin_root.join("assets").join("fonts").join("clock.ttf");

        fs::create_dir_all(&extracted_root).expect("extracted dir");
        fs::create_dir_all(builtin_font.parent().expect("builtin font parent"))
            .expect("builtin font dir");
        fs::write(&builtin_font, b"font").expect("builtin font");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 2,
                  "name": "Clock",
                  "text": "12:34",
                  "font": "fonts/clock.ttf"
                }
              ]
            }"#,
        )
        .expect("scene json");

        let record = scene_record(&managed_root);
        let report = analyze_scene_support_with_builtin_root(&record, &builtin_root, None);

        assert!(report.is_supported());
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn phase_08_support_report_accepts_sound_only_scene() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        let sounds_root = extracted_root.join("sounds");

        fs::create_dir_all(&sounds_root).expect("sounds dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 7,
                  "name": "Ambient",
                  "sound": ["sounds/ambient.m4a"],
                  "playbackmode": "loop",
                  "volume": 0.7
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(sounds_root.join("ambient.m4a"), b"fake-audio").expect("sound fixture");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        assert!(report.is_supported());
        assert_eq!(report.errors.len(), 0);
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.code == "scene-output-empty"));
    }

    #[test]
    fn support_report_keeps_material_resource_failures_as_phase_10_warnings() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(extracted_root.join("materials")).expect("materials dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{"objects":[{"id":1,"name":"Hero","image":"models/hero.json"}]}"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("hero.json"),
            r#"{"material":"materials/hero.material"}"#,
        )
        .expect("hero model");
        fs::write(
            extracted_root.join("materials").join("hero.material"),
            r#"{"passes":[{"shader":"shaders/hero.frag","textures":["textures/hero"]}]}"#,
        )
        .expect("hero material");

        let report = analyze_scene_support_with_builtin_root(
            &scene_record(&managed_root),
            &builtin_root,
            None,
        );

        assert!(report.is_supported());
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "material-texture-reference-unresolved"));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.code == "material-reference-unresolved"));
    }

    #[test]
    fn phase_08_support_report_accepts_video_visual_without_poster_semantics() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(extracted_root.join("materials")).expect("materials dir");
        fs::create_dir_all(managed_root.join("decoded").join("materials")).expect("decoded dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::create_dir_all(builtin_root.join("assets").join("shaders").join("compat"))
            .expect("builtin shaders");
        fs::write(
            builtin_root
                .join("assets")
                .join("shaders")
                .join("compat")
                .join("scene-sprite.metal"),
            b"#include <metal_stdlib>\nusing namespace metal;\nfragment float4 compat_sprite_fragment() { return float4(1); }",
        )
        .expect("builtin sprite shader");

        fs::write(
            extracted_root.join("scene.json"),
            r#"{"objects":[{"id":1,"name":"Loop","image":"models/loop.json","size":"1920 1080"}]}"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("loop.json"),
            r#"{"material":"materials/loop.json"}"#,
        )
        .expect("loop model");
        fs::write(
            extracted_root.join("materials").join("loop.json"),
            r#"{"passes":[{"shader":"genericimage4","textures":["loop"]}]}"#,
        )
        .expect("loop material");
        write_video_tex_fixture(&extracted_root.join("materials").join("loop.tex"));

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        assert!(report.is_supported());
        assert_eq!(report.errors.len(), 0);
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.message.contains("poster")));
    }

    #[test]
    fn phase_10_support_report_accepts_solid_layer_with_missing_material_when_baseline_exists() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
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
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("solid.model.json"),
            r#"{"solidlayer":true,"material":"materials/missing.material"}"#,
        )
        .expect("solid model");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        assert!(report.is_supported());
        assert!(report.errors.is_empty());
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.code == "material-invalid"));
    }

    #[test]
    fn phase_09i_support_report_adds_text_diagnostics_with_structured_resource_detail() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(&extracted_root).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 2,
                  "name": "Clock",
                  "text": "12:34",
                  "font": "fonts/missing-clock.ttf",
                  "effects": ["effects/glow.effect"]
                }
              ]
            }"#,
        )
        .expect("scene json");

        let report = analyze_scene_support_with_builtin_root(
            &scene_record(&managed_root),
            &builtin_root,
            None,
        );

        let font_warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "text-font-reference-unresolved")
            .expect("font warning");
        let font_detail = font_warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("font resource detail");
        assert_eq!(font_detail.authored_reference, "fonts/missing-clock.ttf");
        assert!(!font_detail.reference_resolved);
        assert!(!font_detail.attempted_candidates.is_empty());

        let effect_warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "text-effect-reference-unresolved")
            .expect("text effect warning");
        let effect_detail = effect_warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("text effect resource detail");
        assert_eq!(effect_detail.authored_reference, "effects/glow.effect");
        assert!(!effect_detail.external_assets_available);
    }

    #[test]
    fn phase_09i_support_report_distinguishes_sound_and_particle_resource_warnings() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(&extracted_root).expect("extracted dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 7,
                  "name": "Ambient",
                  "sound": ["sounds/missing.m4a"]
                },
                {
                  "id": 8,
                  "name": "Trail",
                  "particle": "particles/petal.json"
                }
              ]
            }"#,
        )
        .expect("scene json");

        let report = analyze_scene_support_with_builtin_root(
            &scene_record(&managed_root),
            &builtin_root,
            None,
        );

        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "sound-asset-reference-unresolved"));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.code == "particle-resource-reference-unresolved"));
    }

    #[test]
    fn phase_10a_support_report_reports_unresolved_effect_dependency_with_package_detail() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(extracted_root.join("effects/pulse")).expect("effect dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 10,
                  "name": "Pulse Layer",
                  "image": "models/solidlayer.json",
                  "effects": [{"file":"effects/pulse/effect.json"}]
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("solidlayer.json"),
            r#"{"solidlayer":true}"#,
        )
        .expect("solid layer");
        fs::write(
            extracted_root.join("effects/pulse/effect.json"),
            r#"{"dependencies":["shaders/effects/missing-pulse.metal"],"passes":[]}"#,
        )
        .expect("effect json");

        let report = analyze_scene_support_with_builtin_root(
            &scene_record(&managed_root),
            &builtin_root,
            None,
        );

        let warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "effect-dependency-reference-unresolved")
            .expect("effect dependency warning");
        let detail = warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("resource detail");
        assert_eq!(
            detail.authored_reference,
            "shaders/effects/missing-pulse.metal"
        );
        assert_eq!(
            detail.attempted_roots.first().map(|root| root.kind),
            Some(SceneResourceRootKind::EffectPackage)
        );
        assert!(detail
            .attempted_candidates
            .first()
            .expect("first candidate")
            .contains("effects/pulse/shaders/effects/missing-pulse.metal"));
        assert!(!detail.reference_resolved);
    }

    #[test]
    fn phase_10b_support_report_resolves_effect_texture_dependencies_with_tex_sidecars() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        let effect_root = extracted_root.join("effects/water");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(effect_root.join("materials/effects")).expect("effect materials dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 10,
                  "name": "Water Layer",
                  "image": "models/solidlayer.json",
                  "effects": [{"file":"effects/water/effect.json"}]
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("solidlayer.json"),
            r#"{"solidlayer":true}"#,
        )
        .expect("solid layer");
        fs::write(
            effect_root.join("effect.json"),
            r#"{
              "dependencies": [
                "materials/effects/water-normal.png",
                "materials/effects/water-normal.tex-json"
              ],
              "passes": []
            }"#,
        )
        .expect("effect json");
        fs::write(
            effect_root.join("materials/effects/water-normal.tex"),
            b"tex",
        )
        .expect("texture sidecar");

        let report = analyze_scene_support_with_builtin_root(
            &scene_record(&managed_root),
            &builtin_root,
            None,
        );

        assert!(
            report
                .warnings
                .iter()
                .all(|warning| warning.code != "effect-dependency-reference-unresolved"),
            "effect texture dependencies with authored image/tex-json names should resolve through .tex sidecars"
        );
    }

    #[test]
    fn phase_10b_support_report_marks_unknown_effect_shader_pair_as_unsupported_with_lookup() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models/util")).expect("models dir");
        fs::create_dir_all(extracted_root.join("effects/mystery/shaders/effects"))
            .expect("effect shader dir");
        fs::create_dir_all(extracted_root.join("effects/mystery/materials/effects"))
            .expect("effect material dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 10,
                  "name": "Summer",
                  "image": "models/util/solidlayer.json",
                  "origin": "960 540 0",
                  "size": "256 256",
                  "effects": [{"file":"effects/mystery/effect.json","visible":true}]
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models/util").join("solidlayer.json"),
            r#"{"solidlayer":true}"#,
        )
        .expect("solid layer");
        fs::write(
            extracted_root.join("effects/mystery/effect.json"),
            r#"{"passes":[{"material":"materials/effects/mystery.json"}]}"#,
        )
        .expect("effect json");
        fs::write(
            extracted_root.join("effects/mystery/materials/effects/mystery.json"),
            r#"{"passes":[{"shader":"effects/mystery"}]}"#,
        )
        .expect("effect material");
        fs::write(
            extracted_root.join("effects/mystery/shaders/effects/mystery.vert"),
            b"void main() {}",
        )
        .expect("effect vert");
        fs::write(
            extracted_root.join("effects/mystery/shaders/effects/mystery.frag"),
            b"void main() {}",
        )
        .expect("effect frag");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        let warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "effect-unsupported")
            .expect("effect unsupported warning");
        assert!(warning
            .message
            .contains("resolved effect material materials/effects/mystery.json"));
        assert!(warning
            .message
            .contains("does not support authored shader source pair effects/mystery yet"));
        assert!(!warning
            .message
            .contains("could not resolve effect material"));

        let detail = warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("effect resource detail");
        assert_eq!(detail.authored_reference, "effects/mystery");
        assert!(detail.reference_resolved);
        assert!(detail.present_but_unsupported);
        assert_eq!(
            detail.matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );
        assert!(detail
            .matched_path
            .as_deref()
            .expect("matched path")
            .contains("effects/mystery/shaders/effects/mystery.vert"));
        assert!(detail
            .attempted_candidates
            .iter()
            .any(|candidate| candidate.contains("effects/mystery/shaders/effects/mystery.vert")));
    }

    #[test]
    fn phase_10a_support_report_keeps_unresolved_effect_material_wording_when_missing() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models/util")).expect("models dir");
        fs::create_dir_all(extracted_root.join("effects/pulse")).expect("effect dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 11,
                  "name": "Summer",
                  "image": "models/util/solidlayer.json",
                  "origin": "960 540 0",
                  "size": "256 256",
                  "effects": [{"file":"effects/pulse/effect.json","visible":true}]
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models/util").join("solidlayer.json"),
            r#"{"solidlayer":true}"#,
        )
        .expect("solid layer");
        fs::write(
            extracted_root.join("effects/pulse/effect.json"),
            r#"{"passes":[{"material":"materials/effects/pulse.json"}]}"#,
        )
        .expect("effect json");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_builtin_root(&record, &builtin_root, Some(scene))
            }
            _ => panic!("expected scene runtime"),
        };

        let warning = report
            .warnings
            .iter()
            .find(|warning| {
                warning.code == "effect-material-reference-unresolved"
                    && warning
                        .message
                        .contains("could not resolve effect material materials/effects/pulse.json")
            })
            .expect("effect material unresolved warning");
        let detail = warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("effect material resource detail");
        assert_eq!(detail.authored_reference, "materials/effects/pulse.json");
        assert!(!detail.reference_resolved);
        assert_eq!(
            detail.attempted_roots.first().map(|root| root.kind),
            Some(SceneResourceRootKind::EffectPackage)
        );
    }

    #[test]
    fn phase_09i_support_report_marks_resolved_particle_resource_as_present_but_unsupported() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("particles")).expect("particles dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 8,
                  "name": "Trail",
                  "particle": "particles/petal.json"
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("particles").join("petal.json"),
            r#"{"emitters":[]}"#,
        )
        .expect("particle json");

        let report = analyze_scene_support_with_builtin_root(
            &scene_record(&managed_root),
            &builtin_root,
            None,
        );

        let particle_warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "particle-resource-unsupported")
            .expect("particle unsupported warning");
        let particle_detail = particle_warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("particle resource detail");
        assert!(particle_detail.reference_resolved);
        assert!(particle_detail.present_but_unsupported);
    }

    #[test]
    fn phase_10a_support_report_does_not_flag_existing_authored_shader_pairs_as_unresolved() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("effects/pulse")).expect("effect dir");
        fs::create_dir_all(external_root.join("effects/pulse/materials/effects"))
            .expect("external material dir");
        fs::create_dir_all(external_root.join("effects/pulse/shaders/effects"))
            .expect("external shader dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 10,
                  "name": "Pulse Layer",
                  "image": "models/solidlayer.json",
                  "effects": [{"file":"effects/pulse/effect.json"}]
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("effects/pulse/effect.json"),
            r#"{"passes":[{"material":"materials/effects/pulse.json"}]}"#,
        )
        .expect("effect json");
        fs::write(
            external_root.join("effects/pulse/materials/effects/pulse.json"),
            r#"{"passes":[{"shader":"effects/pulse"}]}"#,
        )
        .expect("effect material");
        fs::write(
            external_root.join("effects/pulse/shaders/effects/pulse.vert"),
            "void main() {}",
        )
        .expect("effect vert");
        fs::write(
            external_root.join("effects/pulse/shaders/effects/pulse.frag"),
            "void main() {}",
        )
        .expect("effect frag");

        let report = analyze_scene_support_with_resolver(
            &scene_record(&managed_root),
            SceneResourceResolver::for_managed_root_with_asset_roots(
                &managed_root,
                &builtin_root,
                Some(external_root),
            ),
            None,
        );

        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.code == "effect-material-reference-unresolved"));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.code == "shader-reference-unresolved"));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.code == "effect-unsupported"));
    }

    #[test]
    fn phase_10b_support_report_preserves_external_assets_provenance_for_unsupported_effect_shader_pair(
    ) {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let external_root = temp.path().join("external-assets");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models/util")).expect("models dir");
        fs::create_dir_all(extracted_root.join("effects/mystery")).expect("effect dir");
        fs::create_dir_all(external_root.join("effects/mystery/materials/effects"))
            .expect("external material dir");
        fs::create_dir_all(external_root.join("effects/mystery/shaders/effects"))
            .expect("external shader dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 10,
                  "name": "Pulse Layer",
                  "image": "models/util/solidlayer.json",
                  "effects": [{"file":"effects/mystery/effect.json"}]
                }
              ]
            }"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models/util/solidlayer.json"),
            r#"{"solidlayer":true}"#,
        )
        .expect("solid layer");
        fs::write(
            extracted_root.join("effects/mystery/effect.json"),
            r#"{"passes":[{"material":"materials/effects/mystery.json"}]}"#,
        )
        .expect("effect json");
        fs::write(
            external_root.join("effects/mystery/materials/effects/mystery.json"),
            r#"{"passes":[{"shader":"effects/mystery"}]}"#,
        )
        .expect("effect material");
        fs::write(
            external_root.join("effects/mystery/shaders/effects/mystery.vert"),
            "void main() {}",
        )
        .expect("effect vert");
        fs::write(
            external_root.join("effects/mystery/shaders/effects/mystery.frag"),
            "void main() {}",
        )
        .expect("effect frag");

        let mut record = scene_record(&managed_root);
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );
        let runtime = runtime_document_service::runtime_record(&record);
        let report = match &runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                analyze_scene_support_with_resolver(
                    &record,
                    SceneResourceResolver::for_managed_root_with_asset_roots(
                        &managed_root,
                        &builtin_root,
                        Some(external_root),
                    ),
                    Some(scene),
                )
            }
            _ => panic!("expected scene runtime"),
        };

        let warning = report
            .warnings
            .iter()
            .find(|warning| warning.code == "effect-unsupported")
            .expect("effect unsupported warning");
        let detail = warning
            .detail
            .as_ref()
            .and_then(|detail| detail.resource.as_ref())
            .expect("effect resource detail");
        assert_eq!(
            detail.matched_root_kind,
            Some(SceneResourceRootKind::ExternalAssets)
        );
    }
}
