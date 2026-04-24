use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::services::{
    scene_render_planner_service::SceneRenderBlendMode,
    scene_resource_service::{
        SceneResourceLookup, SceneResourceResolver, SceneResourceRootKind, SceneShaderSourceKind,
        SceneShaderSourceLookup,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneShaderProgramKind {
    Sprite,
    Model,
    MaskAlpha,
    MaskApply,
    Copy,
    EffectCompat(SceneCompatEffectKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SceneCompatEffectKind {
    Pulse,
    Shake,
    WaterRipple,
    WaterWaves,
    Blur,
    Tint,
    Scroll,
    Shine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenePhase10bBindingSemantic {
    PreviousInput,
    NoiseTexture,
    FlowMap,
    TimeOffset,
    OpacityMask,
    NormalMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePhase10bTextureSlotContract {
    pub slot: usize,
    pub semantic: ScenePhase10bBindingSemantic,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePhase10bEffectContract {
    pub kind: SceneCompatEffectKind,
    pub family: &'static str,
    pub required_texture_slots: &'static [usize],
    pub supported_texture_slots: &'static [usize],
    pub supported_combo_defaults: &'static [(&'static str, i32)],
    pub supported_uniforms: &'static [&'static str],
    pub runtime_binding_layout: &'static [ScenePhase10bTextureSlotContract],
}

const PULSE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::NoiseTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        required: false,
    },
];

const SHAKE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::FlowMap,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 3,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        required: false,
    },
];

const WATERRIPPLE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::NormalMap,
        required: true,
    },
];

const WATERWAVES_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        required: false,
    },
];

const TINT_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        required: false,
    },
];

const SCROLL_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] =
    &[ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        required: true,
    }];

const PULSE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Pulse,
    family: "pulse",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("AUDIOPROCESSING", 0),
        ("BLENDMODE", 9),
        ("MASK", 0),
        ("PULSEALPHA", 0),
        ("PULSECOLOR", 1),
    ],
    supported_uniforms: &[
        "amount",
        "bounds",
        "noiseamount",
        "noisespeed",
        "phase",
        "power",
        "speed",
        "tinthigh",
        "tintlow",
    ],
    runtime_binding_layout: PULSE_TEXTURE_SLOTS,
};

const SHAKE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Shake,
    family: "shake",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 2, 3],
    supported_combo_defaults: &[
        ("AUDIOPROCESSING", 0),
        ("DIRECTION", 0),
        ("MASK", 0),
        ("NOISE", 0),
        ("TIMEOFFSET", 0),
    ],
    supported_uniforms: &["bounds", "friction", "speed", "strength"],
    runtime_binding_layout: SHAKE_TEXTURE_SLOTS,
};

const WATERRIPPLE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::WaterRipple,
    family: "waterripple",
    required_texture_slots: &[0, 2],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("MASK", 0), ("PERSPECTIVE", 0), ("SPECULAR", 0)],
    supported_uniforms: &[
        "animationspeed",
        "ratio",
        "ripplestrength",
        "scale",
        "scrolldirection",
        "scrollspeed",
    ],
    runtime_binding_layout: WATERRIPPLE_TEXTURE_SLOTS,
};

const WATERWAVES_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::WaterWaves,
    family: "waterwaves",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("DUALWAVES", 0),
        ("MASK", 0),
        ("PERSPECTIVE", 0),
        ("TIMEOFFSET", 0),
    ],
    supported_uniforms: &["direction", "exponent", "scale", "speed", "strength"],
    runtime_binding_layout: WATERWAVES_TEXTURE_SLOTS,
};

const TINT_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Tint,
    family: "tint",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("BLENDMODE", 30), ("MASK", 0)],
    supported_uniforms: &["alpha", "color"],
    runtime_binding_layout: TINT_TEXTURE_SLOTS,
};

const SCROLL_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Scroll,
    family: "scroll",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[],
    supported_uniforms: &["repeat", "speedx", "speedy"],
    runtime_binding_layout: SCROLL_TEXTURE_SLOTS,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneShaderProgram {
    pub key: String,
    pub kind: SceneShaderProgramKind,
    pub metal_source_path: PathBuf,
    pub vertex_entry: &'static str,
    pub fragment_entry: &'static str,
    pub variant_defines: BTreeMap<String, i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneMaterialUniformValue {
    Float(u32),
    Float2([u32; 2]),
    Float3([u32; 3]),
    Float4([u32; 4]),
}

impl SceneMaterialUniformValue {
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(f32::from_bits(*value)),
            _ => None,
        }
    }

    pub fn as_float2(&self) -> Option<[f32; 2]> {
        match self {
            Self::Float2(value) => Some([f32::from_bits(value[0]), f32::from_bits(value[1])]),
            _ => None,
        }
    }

    pub fn as_float3(&self) -> Option<[f32; 3]> {
        match self {
            Self::Float3(value) => Some([
                f32::from_bits(value[0]),
                f32::from_bits(value[1]),
                f32::from_bits(value[2]),
            ]),
            _ => None,
        }
    }

    pub fn as_float4(&self) -> Option<[f32; 4]> {
        match self {
            Self::Float4(value) => Some([
                f32::from_bits(value[0]),
                f32::from_bits(value[1]),
                f32::from_bits(value[2]),
                f32::from_bits(value[3]),
            ]),
            _ => None,
        }
    }

    fn from_floats(values: &[f32]) -> Option<Self> {
        match values {
            [x] => Some(Self::Float(x.to_bits())),
            [x, y] => Some(Self::Float2([x.to_bits(), y.to_bits()])),
            [x, y, z] => Some(Self::Float3([x.to_bits(), y.to_bits(), z.to_bits()])),
            [x, y, z, w] => Some(Self::Float4([
                x.to_bits(),
                y.to_bits(),
                z.to_bits(),
                w.to_bits(),
            ])),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterialTextureBinding {
    pub slot_index: usize,
    pub slot_name: String,
    pub texture_name: Option<String>,
    pub resolved_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterialPassPlan {
    pub index: usize,
    pub shader_ref: String,
    pub program: SceneShaderProgram,
    pub blend_mode: SceneRenderBlendMode,
    pub combos: BTreeMap<String, i32>,
    pub uniforms: BTreeMap<String, SceneMaterialUniformValue>,
    pub textures: Vec<SceneMaterialTextureBinding>,
    pub effect_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEffectBinding {
    pub index: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEffectPassPlan {
    pub index: usize,
    pub material_path: Option<String>,
    pub material_lookup: Option<SceneResourceLookup>,
    pub target_name: Option<String>,
    pub bindings: Vec<SceneEffectBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEffectPlan {
    pub effect_path: PathBuf,
    pub effect_package_root: PathBuf,
    pub version: Option<i64>,
    pub fbo_names: Vec<String>,
    pub shader_dependencies: Vec<String>,
    pub dependency_lookups: Vec<SceneResourceLookup>,
    pub passes: Vec<SceneEffectPassPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneResolvedMaterialPlan {
    pub material_path: PathBuf,
    pub passes: Vec<SceneMaterialPassPlan>,
    pub material_effects: Vec<SceneEffectPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterialSummary {
    pub material_path: PathBuf,
    pub pass_count: usize,
    pub max_texture_count: usize,
    pub has_combos: bool,
    pub has_effects: bool,
    pub requires_phase10_graph: bool,
}

pub fn load_scene_material_plan(
    resolver: &SceneResourceResolver,
    material_path: &str,
) -> Result<SceneResolvedMaterialPlan, String> {
    let lookup = resolver.inspect_relative_path(material_path);
    let resolved_path = lookup.matched_path.clone().ok_or_else(|| {
        format!("material {material_path} could not be resolved from Scene roots")
    })?;
    load_scene_material_plan_from_resolved_path(resolver, material_path, &resolved_path, None)
}

pub fn load_scene_material_plan_with_effect_package_root(
    resolver: &SceneResourceResolver,
    material_path: &str,
    effect_package_root: &Path,
) -> Result<SceneResolvedMaterialPlan, String> {
    let lookup = resolver.inspect_relative_path_with_local_root(
        material_path,
        SceneResourceRootKind::EffectPackage,
        effect_package_root,
    );
    let resolved_path = lookup.matched_path.clone().ok_or_else(|| {
        format!(
            "material {material_path} could not be resolved from effect package {} or Scene roots",
            effect_package_root.display()
        )
    })?;
    load_scene_material_plan_from_resolved_path(
        resolver,
        material_path,
        &resolved_path,
        Some(effect_package_root),
    )
}

fn load_scene_material_plan_from_resolved_path(
    resolver: &SceneResourceResolver,
    material_path: &str,
    resolved_path: &Path,
    effect_package_root: Option<&Path>,
) -> Result<SceneResolvedMaterialPlan, String> {
    let json = read_json(resolved_path)?;
    let passes = material_pass_values(&json);
    if passes.is_empty() {
        return Err(format!(
            "material {} has no render passes",
            resolved_path.display()
        ));
    }

    let mut compiled_passes = Vec::with_capacity(passes.len());
    for (index, pass) in passes.iter().enumerate() {
        let shader_ref = pass
            .get("shader")
            .and_then(Value::as_str)
            .or_else(|| json.get("shader").and_then(Value::as_str))
            .ok_or_else(|| {
                format!(
                    "material {} pass {} does not declare a shader",
                    resolved_path.display(),
                    index
                )
            })?
            .to_string();
        let combos = parse_combo_map(pass.get("combos").or_else(|| json.get("combos")));
        let uniforms = parse_material_uniform_map(pass, &json);
        let program = resolve_shader_program_with_context(
            resolver,
            &shader_ref,
            &combos,
            effect_package_root,
        )?;
        let texture_names =
            parse_texture_list(pass.get("textures").or_else(|| json.get("textures")));
        let textures = texture_names
            .into_iter()
            .enumerate()
            .map(|(slot, name)| SceneMaterialTextureBinding {
                slot_index: slot,
                slot_name: format!("g_Texture{slot}"),
                resolved_path: name.as_deref().and_then(|name| {
                    resolve_texture_candidates_for_material(
                        resolver,
                        material_path,
                        resolved_path,
                        effect_package_root,
                        name,
                    )
                    .into_iter()
                    .next()
                }),
                texture_name: name,
            })
            .collect::<Vec<_>>();
        compiled_passes.push(SceneMaterialPassPlan {
            index,
            shader_ref,
            program,
            blend_mode: parse_material_blend_mode(
                pass.get("blending")
                    .or_else(|| json.get("blending"))
                    .and_then(Value::as_str),
            ),
            combos,
            uniforms,
            textures,
            effect_paths: effect_paths(pass)
                .into_iter()
                .chain(effect_paths(&json))
                .collect(),
        });
    }

    let material_effects = effect_paths(&json)
        .into_iter()
        .map(|path| {
            if let Some(effect_package_root) = effect_package_root {
                load_scene_effect_plan_with_local_root(resolver, &path, effect_package_root)
            } else {
                load_scene_effect_plan(resolver, &path)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SceneResolvedMaterialPlan {
        material_path: resolved_path.to_path_buf(),
        passes: compiled_passes,
        material_effects,
    })
}

pub fn inspect_scene_material_summary(
    resolver: &SceneResourceResolver,
    material_path: &str,
) -> Result<SceneMaterialSummary, String> {
    let resolved_path = resolver
        .resolve_relative_path(material_path)
        .ok_or_else(|| {
            format!("material {material_path} could not be resolved from Scene roots")
        })?;
    let json = read_json(&resolved_path)?;
    let passes = material_pass_values(&json);
    if passes.is_empty() {
        return Err(format!(
            "material {} has no render passes",
            resolved_path.display()
        ));
    }

    let mut max_texture_count = 0;
    let mut has_combos = false;
    let mut requires_phase10_graph = false;
    for pass in &passes {
        let textures = parse_texture_list(pass.get("textures").or_else(|| json.get("textures")));
        max_texture_count = max_texture_count.max(textures.len());
        let combos = parse_combo_map(pass.get("combos").or_else(|| json.get("combos")));
        has_combos |= !combos.is_empty();
        let shader_ref = pass
            .get("shader")
            .and_then(Value::as_str)
            .or_else(|| json.get("shader").and_then(Value::as_str));
        requires_phase10_graph |= shader_ref
            .map(shader_ref_requires_phase10_graph_semantics)
            .unwrap_or(false);
    }

    let has_effects = !effect_paths(&json).is_empty();
    requires_phase10_graph |=
        passes.len() > 1 || has_combos || has_effects || max_texture_count > 1;

    Ok(SceneMaterialSummary {
        material_path: resolved_path,
        pass_count: passes.len(),
        max_texture_count,
        has_combos,
        has_effects,
        requires_phase10_graph,
    })
}

pub fn load_scene_effect_plan(
    resolver: &SceneResourceResolver,
    effect_path: &str,
) -> Result<SceneEffectPlan, String> {
    let lookup = resolver.inspect_relative_path(effect_path);
    let resolved_path = lookup
        .matched_path
        .clone()
        .ok_or_else(|| format!("effect {effect_path} could not be resolved from Scene roots"))?;
    load_scene_effect_plan_from_resolved_path(resolver, effect_path, &resolved_path)
}

fn load_scene_effect_plan_with_local_root(
    resolver: &SceneResourceResolver,
    effect_path: &str,
    local_root: &Path,
) -> Result<SceneEffectPlan, String> {
    let lookup = resolver.inspect_relative_path_with_local_root(
        effect_path,
        SceneResourceRootKind::EffectPackage,
        local_root,
    );
    let resolved_path = lookup.matched_path.clone().ok_or_else(|| {
        format!(
            "effect {effect_path} could not be resolved from effect package {} or Scene roots",
            local_root.display()
        )
    })?;
    load_scene_effect_plan_from_resolved_path(resolver, effect_path, &resolved_path)
}

fn load_scene_effect_plan_from_resolved_path(
    resolver: &SceneResourceResolver,
    _authored_effect_path: &str,
    resolved_path: &Path,
) -> Result<SceneEffectPlan, String> {
    let json = read_json(resolved_path)?;
    let effect_package_root = resolved_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(PathBuf::new);
    let passes = json
        .get("passes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let shader_dependencies = json
        .get("dependencies")
        .and_then(Value::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dependency_lookups = shader_dependencies
        .iter()
        .map(|dependency| {
            inspect_scene_effect_dependency(resolver, dependency, &effect_package_root)
        })
        .collect::<Vec<_>>();

    Ok(SceneEffectPlan {
        effect_path: resolved_path.to_path_buf(),
        effect_package_root: effect_package_root.clone(),
        version: json.get("version").and_then(Value::as_i64),
        fbo_names: json
            .get("fbos")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(Value::as_str))
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        shader_dependencies,
        dependency_lookups,
        passes: passes
            .iter()
            .enumerate()
            .map(|(index, pass)| SceneEffectPassPlan {
                index,
                material_path: pass
                    .get("material")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                material_lookup: pass
                    .get("material")
                    .and_then(Value::as_str)
                    .map(|material| {
                        resolver.inspect_relative_path_with_local_root(
                            material,
                            SceneResourceRootKind::EffectPackage,
                            &effect_package_root,
                        )
                    }),
                bindings: pass
                    .get("bind")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| {
                                Some(SceneEffectBinding {
                                    index: entry.get("index")?.as_u64()? as usize,
                                    name: entry.get("name")?.as_str()?.to_string(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                target_name: pass
                    .get("target")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            })
            .collect(),
    })
}

pub fn inspect_scene_effect_dependency(
    resolver: &SceneResourceResolver,
    dependency: &str,
    effect_package_root: &Path,
) -> SceneResourceLookup {
    if effect_dependency_is_texture(dependency) {
        return resolver
            .inspect_texture_candidates_with_local_root(
                None,
                None,
                dependency,
                SceneResourceRootKind::EffectPackage,
                effect_package_root,
            )
            .lookup;
    }
    if effect_dependency_is_shader(dependency) {
        return inspect_scene_shader_source_with_effect_context(
            resolver,
            dependency,
            Some(effect_package_root),
        )
        .lookup;
    }
    resolver.inspect_relative_path_with_local_root(
        dependency,
        SceneResourceRootKind::EffectPackage,
        effect_package_root,
    )
}

fn effect_dependency_is_shader(dependency: &str) -> bool {
    let lower = dependency.to_ascii_lowercase();
    lower.starts_with("shaders/")
        || lower.contains("/shaders/")
        || lower.ends_with(".vert")
        || lower.ends_with(".frag")
        || lower.ends_with(".metal")
        || (Path::new(&lower).extension().is_none()
            && !lower.starts_with("materials/")
            && !lower.starts_with("textures/")
            && !lower.starts_with("preview/"))
}

fn effect_dependency_is_texture(dependency: &str) -> bool {
    let lower = dependency.to_ascii_lowercase();
    lower.ends_with(".tex")
        || lower.ends_with(".tex-json")
        || lower.ends_with(".tex.json")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".tga")
        || lower.ends_with(".bmp")
}

pub fn load_shader_program_source(
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
) -> Result<String, String> {
    let source = fs::read_to_string(&program.metal_source_path).map_err(|error| {
        format!(
            "unable to read shader source {}: {error}",
            program.metal_source_path.display()
        )
    })?;
    Ok(preprocess_scene_shader_source(
        &source,
        &merged_shader_defines(program, defines),
    ))
}

pub fn preprocess_scene_shader_source(source: &str, defines: &BTreeMap<String, i32>) -> String {
    let mut normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.contains('；') {
        normalized = normalized.replace('；', ";");
    }

    let stripped = normalized
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("#include") || trimmed.starts_with("#require") {
                format!("// {trimmed}")
            } else if line.contains("[COMBO]")
                || (line.contains("uniform sampler2D") && line.contains('{'))
            {
                line.split('{').next().unwrap_or("").trim_end().to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if defines.is_empty() {
        stripped
    } else {
        let prefix = defines
            .iter()
            .map(|(key, value)| format!("#define {key} {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{prefix}\n{stripped}")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_shader_program(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
) -> Result<SceneShaderProgram, String> {
    resolve_shader_program_with_context(resolver, shader_ref, combos, None)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_shader_program_with_effect_package_root(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
    effect_package_root: &Path,
) -> Result<SceneShaderProgram, String> {
    resolve_shader_program_with_context(resolver, shader_ref, combos, Some(effect_package_root))
}

pub fn inspect_scene_shader_source_with_effect_context(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    effect_package_root: Option<&Path>,
) -> SceneShaderSourceLookup {
    if let Some(effect_package_root) = effect_package_root {
        resolver.inspect_shader_source_with_local_root(
            shader_ref,
            SceneResourceRootKind::EffectPackage,
            effect_package_root,
        )
    } else {
        resolver.inspect_shader_source(shader_ref)
    }
}

fn resolve_shader_program_with_context(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
    effect_package_root: Option<&Path>,
) -> Result<SceneShaderProgram, String> {
    let shader_ref = shader_ref.trim();
    if shader_ref.is_empty() {
        return Err("shader reference is empty".to_string());
    }

    if shader_ref.ends_with(".metal") || shader_ref.contains('/') || shader_ref.contains('\\') {
        let lookup = inspect_scene_shader_source_with_effect_context(
            resolver,
            shader_ref,
            effect_package_root,
        );
        return match lookup.kind {
            SceneShaderSourceKind::Metal => {
                let path = lookup.metal_source_path.ok_or_else(|| {
                    format!("shader {shader_ref} resolved without a metal source")
                })?;
                Ok(SceneShaderProgram {
                    key: path.display().to_string(),
                    kind: SceneShaderProgramKind::Sprite,
                    metal_source_path: path,
                    vertex_entry: "compat_sprite_vertex",
                    fragment_entry: "compat_sprite_fragment",
                    variant_defines: BTreeMap::new(),
                })
            }
            SceneShaderSourceKind::AuthoredSourceSet => {
                if let Some(program) =
                    resolve_supported_authored_effect_program(resolver, shader_ref, combos)?
                {
                    Ok(program)
                } else {
                    Err(unsupported_authored_shader_message(
                        shader_ref,
                        effect_package_root,
                        &lookup,
                    ))
                }
            }
            SceneShaderSourceKind::Missing => {
                Err(unresolved_shader_message(shader_ref, effect_package_root))
            }
        };
    }

    let lower = shader_ref.to_ascii_lowercase();
    let (kind, asset_path) = if lower.contains("clippingmaskimage4") {
        (
            SceneShaderProgramKind::MaskAlpha,
            "assets/shaders/compat/scene-mask-alpha.metal",
        )
    } else if combos.get("CLIPPINGTARGET").copied() == Some(1) {
        (
            SceneShaderProgramKind::MaskApply,
            "assets/shaders/compat/scene-mask-apply.metal",
        )
    } else if lower.contains("copy") {
        (
            SceneShaderProgramKind::Copy,
            "assets/shaders/compat/scene-copy.metal",
        )
    } else if lower.contains("model") || lower.contains("puppet") {
        (
            SceneShaderProgramKind::Model,
            "assets/shaders/compat/scene-model.metal",
        )
    } else if lower.contains("genericimage")
        || lower.contains("image")
        || lower.contains("sprite")
        || lower.contains("default")
        || lower.contains("textured")
    {
        (
            SceneShaderProgramKind::Sprite,
            "assets/shaders/compat/scene-sprite.metal",
        )
    } else {
        return Err(format!(
            "shader {shader_ref} does not map to a supported phase-10 compatibility program"
        ));
    };
    let metal_source_path = resolver
        .resolve_relative_path(asset_path)
        .ok_or_else(|| format!("built-in Scene shader asset {asset_path} could not be resolved"))?;
    Ok(SceneShaderProgram {
        key: format!("{shader_ref}:{:?}", kind),
        kind,
        metal_source_path,
        vertex_entry: shader_program_vertex_entry(kind),
        fragment_entry: shader_program_fragment_entry(kind),
        variant_defines: shader_program_base_defines(kind),
    })
}

pub fn merged_shader_defines(
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    let mut merged = program.variant_defines.clone();
    for (name, value) in defines {
        merged.insert(name.clone(), *value);
    }
    merged
}

fn unresolved_shader_message(shader_ref: &str, effect_package_root: Option<&Path>) -> String {
    if let Some(effect_package_root) = effect_package_root {
        format!(
            "shader {shader_ref} could not be resolved from effect package {} or Scene roots",
            effect_package_root.display()
        )
    } else {
        format!("shader {shader_ref} could not be resolved from Scene roots")
    }
}

fn unsupported_authored_shader_message(
    shader_ref: &str,
    effect_package_root: Option<&Path>,
    lookup: &SceneShaderSourceLookup,
) -> String {
    let sources = lookup
        .matched_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(reason) = phase10b_blocked_effect_reason(shader_ref) {
        if let Some(effect_package_root) = effect_package_root {
            return format!(
                "shader {shader_ref} resolved authored source assets ({sources}) from effect package {} or Scene roots, but phase-10b does not support that authored shader family as single-pass compat. {reason}",
                effect_package_root.display()
            );
        }
        return format!(
            "shader {shader_ref} resolved authored source assets ({sources}) from Scene roots, but phase-10b does not support that authored shader family as single-pass compat. {reason}"
        );
    }

    if let Some(effect_package_root) = effect_package_root {
        format!(
            "shader {shader_ref} resolved authored source assets ({sources}) from effect package {} or Scene roots, but phase-10b does not support that authored shader family as explicit single-pass compat",
            effect_package_root.display()
        )
    } else {
        format!(
            "shader {shader_ref} resolved authored source assets ({sources}) from Scene roots, but phase-10b does not support that authored shader family as explicit single-pass compat"
        )
    }
}

fn resolve_supported_authored_effect_program(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
) -> Result<Option<SceneShaderProgram>, String> {
    let Some(contract) = phase10b_supported_effect_contract_for_shader_ref(shader_ref) else {
        return Ok(None);
    };
    let asset_path = "assets/shaders/compat/scene-effect-compat.metal";
    let metal_source_path = resolver
        .resolve_relative_path(asset_path)
        .ok_or_else(|| format!("built-in Scene shader asset {asset_path} could not be resolved"))?;
    Ok(Some(SceneShaderProgram {
        key: format!("effect-compat:{:?}:{shader_ref}", contract.kind),
        kind: SceneShaderProgramKind::EffectCompat(contract.kind),
        metal_source_path,
        vertex_entry: "phase10_effect_vertex",
        fragment_entry: "phase10_effect_fragment",
        variant_defines: compat_effect_shader_defines(contract.kind, combos),
    }))
}

pub fn phase10b_supported_effect_contract_for_shader_ref(
    shader_ref: &str,
) -> Option<&'static ScenePhase10bEffectContract> {
    let normalized = normalized_shader_stem(shader_ref);
    match normalized.as_str() {
        "pulse" => Some(&PULSE_CONTRACT),
        "shake" => Some(&SHAKE_CONTRACT),
        "waterripple" => Some(&WATERRIPPLE_CONTRACT),
        "waterwaves" => Some(&WATERWAVES_CONTRACT),
        "tint" => Some(&TINT_CONTRACT),
        "scroll" => Some(&SCROLL_CONTRACT),
        _ => None,
    }
}

pub fn phase10b_effect_contract_for_kind(
    kind: SceneCompatEffectKind,
) -> Option<&'static ScenePhase10bEffectContract> {
    match kind {
        SceneCompatEffectKind::Pulse => Some(&PULSE_CONTRACT),
        SceneCompatEffectKind::Shake => Some(&SHAKE_CONTRACT),
        SceneCompatEffectKind::WaterRipple => Some(&WATERRIPPLE_CONTRACT),
        SceneCompatEffectKind::WaterWaves => Some(&WATERWAVES_CONTRACT),
        SceneCompatEffectKind::Tint => Some(&TINT_CONTRACT),
        SceneCompatEffectKind::Scroll => Some(&SCROLL_CONTRACT),
        SceneCompatEffectKind::Blur | SceneCompatEffectKind::Shine => None,
    }
}

pub fn phase10b_blocked_effect_reason(shader_ref: &str) -> Option<&'static str> {
    match normalized_shader_stem(shader_ref).as_str() {
        "blur" => Some(
            "blur requires phase-10d named render targets, multi-pass order, previous-texture chaining, and copy-background lifecycle support.",
        ),
        "shine" => Some(
            "shine requires phase-10d named render targets, multi-pass order, previous-texture chaining, and copy-background lifecycle support.",
        ),
        _ => None,
    }
}

fn normalized_shader_stem(shader_ref: &str) -> String {
    let candidate = Path::new(shader_ref)
        .file_stem()
        .or_else(|| Path::new(shader_ref).file_name())
        .and_then(|value| value.to_str())
        .unwrap_or(shader_ref);
    candidate
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn shader_program_vertex_entry(kind: SceneShaderProgramKind) -> &'static str {
    match kind {
        SceneShaderProgramKind::Sprite => "compat_sprite_vertex",
        SceneShaderProgramKind::Model => "compat_model_vertex",
        SceneShaderProgramKind::MaskAlpha => "compat_mask_vertex",
        SceneShaderProgramKind::MaskApply => "compat_mask_apply_vertex",
        SceneShaderProgramKind::Copy => "compat_copy_vertex",
        SceneShaderProgramKind::EffectCompat(_) => "phase10_effect_vertex",
    }
}

fn shader_program_fragment_entry(kind: SceneShaderProgramKind) -> &'static str {
    match kind {
        SceneShaderProgramKind::Sprite => "compat_sprite_fragment",
        SceneShaderProgramKind::Model => "compat_model_fragment",
        SceneShaderProgramKind::MaskAlpha => "compat_mask_alpha_fragment",
        SceneShaderProgramKind::MaskApply => "compat_mask_apply_fragment",
        SceneShaderProgramKind::Copy => "compat_copy_fragment",
        SceneShaderProgramKind::EffectCompat(_) => "phase10_effect_fragment",
    }
}

fn shader_program_base_defines(kind: SceneShaderProgramKind) -> BTreeMap<String, i32> {
    match kind {
        SceneShaderProgramKind::EffectCompat(kind) => {
            compat_effect_shader_defines(kind, &BTreeMap::new())
        }
        _ => BTreeMap::new(),
    }
}

fn compat_effect_shader_defines(
    kind: SceneCompatEffectKind,
    combos: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    let mut defines = BTreeMap::new();
    match kind {
        SceneCompatEffectKind::Pulse => {
            defines.insert("PHASE10_EFFECT_PULSE".to_string(), 1);
        }
        SceneCompatEffectKind::Shake => {
            defines.insert("PHASE10_EFFECT_SHAKE".to_string(), 1);
        }
        SceneCompatEffectKind::WaterRipple => {
            defines.insert("PHASE10_EFFECT_WATERRIPPLE".to_string(), 1);
        }
        SceneCompatEffectKind::WaterWaves => {
            defines.insert("PHASE10_EFFECT_WATERWAVES".to_string(), 1);
        }
        SceneCompatEffectKind::Tint => {
            defines.insert("PHASE10_EFFECT_TINT".to_string(), 1);
        }
        SceneCompatEffectKind::Scroll => {
            defines.insert("PHASE10_EFFECT_SCROLL".to_string(), 1);
        }
        SceneCompatEffectKind::Blur | SceneCompatEffectKind::Shine => {}
    }
    if let Some(contract) = phase10b_effect_contract_for_kind(kind) {
        for (name, value) in contract.supported_combo_defaults {
            defines.insert((*name).to_string(), *value);
        }
    }
    for (name, value) in combos {
        defines.insert(name.clone(), *value);
    }
    defines
}

fn resolve_texture_candidates_for_material(
    resolver: &SceneResourceResolver,
    material_path: &str,
    resolved_material_path: &Path,
    effect_package_root: Option<&Path>,
    texture_name: &str,
) -> Vec<PathBuf> {
    if let Some(effect_package_root) = effect_package_root {
        resolver.resolve_texture_candidates_with_local_root(
            Some(material_path),
            Some(resolved_material_path),
            texture_name,
            SceneResourceRootKind::EffectPackage,
            effect_package_root,
        )
    } else {
        resolver
            .inspect_texture_candidates(
                Some(material_path),
                Some(resolved_material_path),
                texture_name,
            )
            .matched_paths
    }
}

pub fn shader_ref_requires_phase10_graph_semantics(shader_ref: &str) -> bool {
    let shader_ref = shader_ref.trim();
    if shader_ref.is_empty() {
        return false;
    }

    let lower = shader_ref.to_ascii_lowercase();
    if lower.contains("clippingmaskimage4")
        || lower.contains("copy")
        || lower.contains("model")
        || lower.contains("puppet")
    {
        return true;
    }
    if lower.contains("genericimage")
        || lower.contains("image")
        || lower.contains("sprite")
        || lower.contains("default")
        || lower.contains("textured")
    {
        return false;
    }

    true
}

pub fn parse_combo_map(value: Option<&Value>) -> BTreeMap<String, i32> {
    let mut combos = BTreeMap::new();
    let Some(value) = value.and_then(Value::as_object) else {
        return combos;
    };
    for (key, value) in value {
        let parsed = value
            .as_i64()
            .map(|value| value as i32)
            .or_else(|| value.as_bool().map(|value| if value { 1 } else { 0 }))
            .or_else(|| value.as_str().and_then(|text| text.parse::<i32>().ok()));
        if let Some(parsed) = parsed {
            combos.insert(key.to_string(), parsed);
        }
    }
    combos
}

pub fn parse_uniform_map_from_constants(
    constants: &BTreeMap<String, Value>,
) -> BTreeMap<String, SceneMaterialUniformValue> {
    constants
        .iter()
        .filter_map(|(name, value)| parse_uniform_value(value).map(|parsed| (name.clone(), parsed)))
        .collect()
}

fn parse_texture_list(value: Option<&Value>) -> Vec<Option<String>> {
    value
        .and_then(Value::as_array)
        .map(|textures| {
            textures
                .iter()
                .map(|texture| texture.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_material_uniform_map(
    pass: &Value,
    material_json: &Value,
) -> BTreeMap<String, SceneMaterialUniformValue> {
    let mut uniforms = BTreeMap::new();
    merge_uniform_scope(&mut uniforms, material_json);
    merge_uniform_scope(&mut uniforms, pass);
    uniforms
}

fn merge_uniform_scope(target: &mut BTreeMap<String, SceneMaterialUniformValue>, scope: &Value) {
    for key in [
        "defaults",
        "defaultvalues",
        "uniforms",
        "constants",
        "constantshadervalues",
    ] {
        if let Some(entries) = scope.get(key).and_then(Value::as_object) {
            for (name, value) in entries {
                if let Some(parsed) = parse_uniform_value(value) {
                    target.insert(name.clone(), parsed);
                }
            }
        }
    }

    let Some(entries) = scope.as_object() else {
        return;
    };
    for (name, value) in entries {
        if is_reserved_material_field(name) {
            continue;
        }
        if let Some(parsed) = parse_uniform_value(value) {
            target.insert(name.clone(), parsed);
        }
    }
}

fn is_reserved_material_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "shader"
            | "textures"
            | "usertextures"
            | "passes"
            | "effects"
            | "file"
            | "bind"
            | "combos"
            | "blending"
            | "constants"
            | "constantshadervalues"
            | "uniforms"
            | "defaults"
            | "defaultvalues"
            | "target"
            | "targetscale"
            | "targetformat"
    )
}

fn parse_uniform_value(value: &Value) -> Option<SceneMaterialUniformValue> {
    if let Some(number) = value.as_f64() {
        return SceneMaterialUniformValue::from_floats(&[number as f32]);
    }
    if let Some(boolean) = value.as_bool() {
        return SceneMaterialUniformValue::from_floats(&[if boolean { 1.0 } else { 0.0 }]);
    }
    if let Some(text) = value.as_str() {
        let parts = text
            .split(|character: char| character == ' ' || character == ',' || character == '\t')
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse::<f32>().ok())
            .collect::<Vec<_>>();
        return SceneMaterialUniformValue::from_floats(&parts);
    }
    if let Some(array) = value.as_array() {
        let parts = array
            .iter()
            .filter_map(|item| {
                item.as_f64()
                    .map(|value| value as f32)
                    .or_else(|| item.as_bool().map(|value| if value { 1.0 } else { 0.0 }))
            })
            .collect::<Vec<_>>();
        return SceneMaterialUniformValue::from_floats(&parts);
    }
    if let Some(value) = value.get("value") {
        return parse_uniform_value(value);
    }
    None
}

fn material_pass_values(json: &Value) -> Vec<Value> {
    if let Some(passes) = json.get("passes").and_then(Value::as_array) {
        return passes.clone();
    }
    if material_declares_inline_pass(json) {
        return vec![json.clone()];
    }
    Vec::new()
}

fn material_declares_inline_pass(json: &Value) -> bool {
    json.get("shader")
        .and_then(Value::as_str)
        .map(|shader| !shader.trim().is_empty())
        .unwrap_or(false)
        || json
            .get("textures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || json
            .get("usertextures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || json.get("blending").and_then(Value::as_str).is_some()
        || json
            .get("combos")
            .and_then(Value::as_object)
            .map(|combos| !combos.is_empty())
            .unwrap_or(false)
}

fn parse_material_blend_mode(value: Option<&str>) -> SceneRenderBlendMode {
    match value.unwrap_or_default().to_ascii_lowercase().as_str() {
        "additive" | "add" => SceneRenderBlendMode::Additive,
        "multiply" | "mul" => SceneRenderBlendMode::Multiply,
        _ => SceneRenderBlendMode::Normal,
    }
}

fn effect_paths(value: &Value) -> Vec<String> {
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
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("unable to parse {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use tempfile::tempdir;

    use crate::services::scene_resource_service::{SceneResourceResolver, SceneResourceRootKind};

    use super::{
        inspect_scene_material_summary, load_scene_effect_plan, load_scene_material_plan,
        load_scene_material_plan_with_effect_package_root, preprocess_scene_shader_source,
        resolve_shader_program, resolve_shader_program_with_effect_package_root,
        shader_ref_requires_phase10_graph_semantics, SceneCompatEffectKind, SceneShaderProgramKind,
    };

    fn write(path: &std::path::Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(path, body).expect("write fixture");
    }

    #[test]
    fn material_plan_resolves_builtin_compat_program_and_textures() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("materials/hero.material"),
            r#"{
              "passes":[
                {
                  "shader":"genericimage4",
                  "textures":["textures/hero"],
                  "combos":{"NORMALMAP":true}
                }
              ]
            }"#,
        );
        write(&extracted.join("textures/hero.tex"), "fake");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan =
            load_scene_material_plan(&resolver, "materials/hero.material").expect("material plan");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(plan.passes[0].program.kind, SceneShaderProgramKind::Sprite);
        assert_eq!(plan.passes[0].textures[0].slot_name, "g_Texture0");
        assert!(plan.passes[0].textures[0].resolved_path.is_some());
        assert_eq!(plan.passes[0].combos.get("NORMALMAP"), Some(&1));
    }

    #[test]
    fn effect_plan_reads_pass_bindings_and_dependencies() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("effects/blur.effect"),
            r#"{
              "version": 2,
              "dependencies": ["shaders/fx/blur.frag", "shaders/fx/blur.vert"],
              "passes": [
                {
                  "material": "materials/fx.material",
                  "bind": [{"name":"input","index":0}]
                }
              ]
            }"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_effect_plan(&resolver, "effects/blur.effect").expect("effect plan");

        assert_eq!(plan.version, Some(2));
        assert_eq!(plan.shader_dependencies.len(), 2);
        assert_eq!(plan.passes[0].bindings[0].name, "input");
        assert_eq!(
            plan.passes[0].material_path.as_deref(),
            Some("materials/fx.material")
        );
    }

    #[test]
    fn effect_plan_resolves_pass_material_relative_to_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("effects/waterripple/effect.json"),
            r#"{"passes":[{"material":"materials/effects/waterripple.json","bind":[{"name":"input","index":0}]}]}"#,
        );
        write(
            &extracted.join("effects/waterripple/materials/effects/waterripple.json"),
            r#"{"passes":[{"shader":"genericimage4","textures":["textures/ripple"]}]}"#,
        );
        write(
            &extracted.join("effects/waterripple/textures/ripple.tex"),
            "fake",
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan =
            load_scene_effect_plan(&resolver, "effects/waterripple/effect.json").expect("effect");
        let material_lookup = plan.passes[0]
            .material_lookup
            .as_ref()
            .expect("material lookup");
        assert_eq!(
            material_lookup.matched_path.as_deref(),
            Some(
                extracted
                    .join("effects/waterripple/materials/effects/waterripple.json")
                    .as_path()
            )
        );
        assert_eq!(
            material_lookup.matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );

        let material = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/waterripple.json",
            &plan.effect_package_root,
        )
        .expect("effect material");
        assert_eq!(material.passes.len(), 1);
        assert!(material.passes[0].textures[0].resolved_path.is_some());
    }

    #[test]
    fn effect_plan_resolves_texture_dependencies_with_tex_sidecar_candidates() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let effect_root = extracted.join("effects/waterripple");
        write(
            &effect_root.join("effect.json"),
            r#"{
              "dependencies": [
                "materials/effects/waterripple.json",
                "materials/effects/waterripplenormal.png",
                "materials/effects/waterripplenormal.tex-json",
                "shaders/effects/waterripple.frag",
                "shaders/effects/waterripple.vert"
              ],
              "passes":[{"material":"materials/effects/waterripple.json"}]
            }"#,
        );
        write(
            &effect_root.join("materials/effects/waterripple.json"),
            r#"{"passes":[{"shader":"genericimage4","textures":["effects/waterripplenormal"]}]}"#,
        );
        write(
            &effect_root.join("materials/effects/waterripplenormal.tex"),
            "tex",
        );
        write(
            &effect_root.join("shaders/effects/waterripple.frag"),
            "frag",
        );
        write(
            &effect_root.join("shaders/effects/waterripple.vert"),
            "vert",
        );

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let plan =
            load_scene_effect_plan(&resolver, "effects/waterripple/effect.json").expect("effect");

        assert!(plan
            .dependency_lookups
            .iter()
            .all(|lookup| lookup.matched_path.is_some()));
        assert!(plan
            .dependency_lookups
            .iter()
            .filter(|lookup| lookup.authored_reference.ends_with(".png"))
            .any(|lookup| lookup
                .matched_path
                .as_ref()
                .is_some_and(|path| path.ends_with("waterripplenormal.tex"))));
        assert!(plan
            .dependency_lookups
            .iter()
            .filter(|lookup| lookup.authored_reference.ends_with(".tex-json"))
            .any(|lookup| lookup
                .matched_path
                .as_ref()
                .is_some_and(|path| path.ends_with("waterripplenormal.tex"))));
    }

    #[test]
    fn effect_plan_resolves_shader_dependencies_and_programs_from_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("effects/pulse/effect.json"),
            r#"{
              "dependencies":["shaders/effects/pulse.metal"],
              "passes":[{"material":"materials/effects/pulse.json"}]
            }"#,
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.metal"),
            "fragment float4 pulse_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("effects/pulse/materials/effects/pulse.json"),
            r#"{"passes":[{"shader":"shaders/effects/pulse.metal"}]}"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_effect_plan(&resolver, "effects/pulse/effect.json").expect("effect");
        assert_eq!(
            plan.dependency_lookups[0].matched_path.as_deref(),
            Some(
                extracted
                    .join("effects/pulse/shaders/effects/pulse.metal")
                    .as_path()
            )
        );
        assert_eq!(
            plan.dependency_lookups[0].matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );

        let program = resolve_shader_program_with_effect_package_root(
            &resolver,
            "shaders/effects/pulse.metal",
            &BTreeMap::new(),
            &plan.effect_package_root,
        )
        .expect("effect shader");
        assert_eq!(
            program.metal_source_path,
            extracted.join("effects/pulse/shaders/effects/pulse.metal")
        );

        let material = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/pulse.json",
            &plan.effect_package_root,
        )
        .expect("effect material");
        assert_eq!(
            material.passes[0].program.metal_source_path,
            extracted.join("effects/pulse/shaders/effects/pulse.metal")
        );
    }

    #[test]
    fn effect_plan_resolves_package_materials_from_external_assets_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &external.join("effects/shake/effect.json"),
            r#"{"passes":[{"material":"materials/effects/shake.json"}]}"#,
        );
        write(
            &external.join("effects/shake/materials/effects/shake.json"),
            r#"{"passes":[{"shader":"genericimage4"}]}"#,
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        let plan = load_scene_effect_plan(&resolver, "effects/shake/effect.json").expect("effect");
        assert_eq!(plan.effect_path, external.join("effects/shake/effect.json"));
        assert_eq!(
            plan.passes[0]
                .material_lookup
                .as_ref()
                .and_then(|lookup| lookup.matched_root_kind),
            Some(SceneResourceRootKind::ExternalAssets)
        );
        let material = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/shake.json",
            &plan.effect_package_root,
        )
        .expect("effect material");
        assert_eq!(
            material.material_path,
            external.join("effects/shake/materials/effects/shake.json")
        );
    }

    #[test]
    fn effect_plan_resolves_authored_shader_pair_dependencies_from_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("effects/pulse/effect.json"),
            r#"{
              "dependencies":["effects/pulse-dependency"],
              "passes":[]
            }"#,
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse-dependency.vert"),
            "void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse-dependency.frag"),
            "void main() {}",
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_effect_plan(&resolver, "effects/pulse/effect.json").expect("effect");
        assert_eq!(plan.shader_dependencies, vec!["effects/pulse-dependency"]);
        assert_eq!(
            plan.dependency_lookups[0].matched_path.as_deref(),
            Some(
                extracted
                    .join("effects/pulse/shaders/effects/pulse-dependency.vert")
                    .as_path()
            )
        );
        assert_eq!(
            plan.dependency_lookups[0].matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );
    }

    #[test]
    fn supported_authored_effect_shader_pairs_map_to_phase_10b_compat_program() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &external.join("effects/pulse/materials/effects/pulse.json"),
            r#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &external.join("effects/pulse/shaders/effects/pulse.vert"),
            "void main() {}",
        );
        write(
            &external.join("effects/pulse/shaders/effects/pulse.frag"),
            "void main() {}",
        );
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            "fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        let plan = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/pulse.json",
            &extracted.join("effects/pulse"),
        )
        .expect("pulse compat effect material");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(
            plan.passes[0].program.kind,
            SceneShaderProgramKind::EffectCompat(SceneCompatEffectKind::Pulse)
        );
        assert_eq!(plan.passes[0].program.vertex_entry, "phase10_effect_vertex");
        assert_eq!(
            plan.passes[0].program.fragment_entry,
            "phase10_effect_fragment"
        );
    }

    #[test]
    fn material_plan_preserves_sparse_authored_texture_slot_ordinals() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("materials/layer.material"),
            r#"{"passes":[{"shader":"genericimage4","textures":[null,null,"textures/mask.png"]}]}"#,
        );
        write(&extracted.join("textures/mask.png"), "png");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan =
            load_scene_material_plan(&resolver, "materials/layer.material").expect("material");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(plan.passes[0].textures.len(), 3);
        assert_eq!(plan.passes[0].textures[0].slot_index, 0);
        assert_eq!(plan.passes[0].textures[0].texture_name, None);
        assert_eq!(plan.passes[0].textures[1].slot_index, 1);
        assert_eq!(plan.passes[0].textures[1].texture_name, None);
        assert_eq!(plan.passes[0].textures[2].slot_index, 2);
        assert_eq!(
            plan.passes[0].textures[2].texture_name.as_deref(),
            Some("textures/mask.png")
        );
    }

    #[test]
    fn phase10b_supported_families_expose_explicit_contract_defaults_and_slots() {
        let families = [
            (SceneCompatEffectKind::Shake, vec![0, 1, 2, 3]),
            (SceneCompatEffectKind::Pulse, vec![0, 1, 2]),
            (SceneCompatEffectKind::WaterRipple, vec![0, 1, 2]),
            (SceneCompatEffectKind::WaterWaves, vec![0, 1, 2]),
            (SceneCompatEffectKind::Tint, vec![0, 1]),
            (SceneCompatEffectKind::Scroll, vec![0]),
        ];

        for (kind, supported_slots) in families {
            let contract =
                super::phase10b_effect_contract_for_kind(kind).expect("phase-10b contract");
            assert_eq!(contract.kind, kind);
            assert_eq!(contract.supported_texture_slots, supported_slots.as_slice());
            assert_eq!(
                contract
                    .runtime_binding_layout
                    .iter()
                    .map(|slot| slot.slot)
                    .collect::<Vec<_>>(),
                supported_slots
            );
        }

        let pulse = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Pulse)
            .expect("pulse contract");
        assert!(pulse.supported_combo_defaults.contains(&("BLENDMODE", 9)));
        assert!(pulse.supported_uniforms.contains(&"noiseamount"));

        let scroll = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Scroll)
            .expect("scroll contract");
        assert_eq!(scroll.supported_combo_defaults, &[]);
        assert!(scroll.supported_uniforms.contains(&"repeat"));
    }

    #[test]
    fn phase10b_blur_and_shine_report_phase10d_blockers() {
        let blur_reason =
            super::phase10b_blocked_effect_reason("effects/blur").expect("blur blocker");
        let shine_reason =
            super::phase10b_blocked_effect_reason("effects/shine").expect("shine blocker");

        assert!(blur_reason.contains("phase-10d"));
        assert!(blur_reason.contains("named render targets"));
        assert!(shine_reason.contains("phase-10d"));
        assert!(shine_reason.contains("copy-background lifecycle"));
    }

    #[test]
    fn tint_compat_effect_uses_authored_blendmode_default_without_overriding_combos() {
        let defaults =
            super::compat_effect_shader_defines(SceneCompatEffectKind::Tint, &BTreeMap::new());
        assert_eq!(defaults.get("BLENDMODE"), Some(&30));

        let explicit = super::compat_effect_shader_defines(
            SceneCompatEffectKind::Tint,
            &BTreeMap::from([("BLENDMODE".to_string(), 2)]),
        );
        assert_eq!(explicit.get("BLENDMODE"), Some(&2));
    }

    #[test]
    fn unknown_authored_effect_shader_pairs_remain_phase_10b_unsupported_not_missing() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &external.join("effects/mystery/materials/effects/mystery.json"),
            r#"{"passes":[{"shader":"effects/mystery"}]}"#,
        );
        write(
            &external.join("effects/mystery/shaders/effects/mystery.vert"),
            "void main() {}",
        );
        write(
            &external.join("effects/mystery/shaders/effects/mystery.frag"),
            "void main() {}",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        let error = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/mystery.json",
            &extracted.join("effects/mystery"),
        )
        .expect_err("unknown authored effect shader should remain unsupported in phase-10b");

        assert!(error.contains("resolved authored source assets"));
        assert!(error.contains(
            "phase-10b does not support that authored shader family as explicit single-pass compat"
        ));
        assert!(!error.contains("could not be resolved"));
    }

    #[test]
    fn shader_preprocessor_comments_include_and_injects_defines() {
        let mut defines = BTreeMap::new();
        defines.insert("CLIPPINGTARGET".to_string(), 1);
        let source = r#"#include "lib/common.glsl"
uniform sampler2D g_Texture0 {"label":"base"}
float main()；"#;

        let processed = preprocess_scene_shader_source(source, &defines);

        assert!(processed.contains("#define CLIPPINGTARGET 1"));
        assert!(processed.contains("// #include \"lib/common.glsl\""));
        assert!(!processed.contains("{\"label\":\"base\"}"));
        assert!(!processed.contains('；'));
    }

    #[test]
    fn resolve_shader_program_maps_clipping_combo_to_mask_apply() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-mask-apply.metal"),
            "fragment float4 mask_apply_fragment() { return float4(1); }",
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let program = resolve_shader_program(
            &resolver,
            "genericimage4",
            &BTreeMap::from([("CLIPPINGTARGET".to_string(), 1)]),
        )
        .expect("program");

        assert_eq!(program.kind, SceneShaderProgramKind::MaskApply);
    }

    #[test]
    fn material_plan_accepts_inline_single_pass_shape() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("materials/inline.material"),
            r#"{
              "shader":"genericimage4",
              "textures":["textures/hero"]
            }"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_material_plan(&resolver, "materials/inline.material")
            .expect("inline material plan");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(plan.passes[0].program.kind, SceneShaderProgramKind::Sprite);
        assert_eq!(plan.passes[0].textures.len(), 1);
    }

    #[test]
    fn material_summary_marks_simple_sprite_inline_material_as_baseline() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("materials/inline.material"),
            r#"{
              "shader":"genericimage4",
              "textures":["textures/hero"]
            }"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let summary = inspect_scene_material_summary(&resolver, "materials/inline.material")
            .expect("material summary");

        assert_eq!(summary.pass_count, 1);
        assert_eq!(summary.max_texture_count, 1);
        assert!(!summary.requires_phase10_graph);
    }

    #[test]
    fn shader_semantics_keep_genericimage_on_baseline_and_model_on_phase10() {
        assert!(!shader_ref_requires_phase10_graph_semantics(
            "genericimage4"
        ));
        assert!(shader_ref_requires_phase10_graph_semantics("modelimage"));
        assert!(shader_ref_requires_phase10_graph_semantics(
            "shaders/custom.frag"
        ));
    }
}
