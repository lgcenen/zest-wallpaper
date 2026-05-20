use super::*;

#[cfg(target_os = "macos")]
pub(crate) fn phase10_texture_cache_key(path: &Path) -> String {
    format!("phase10:texture:{}", path.display())
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_shader_variant_key(
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
    blend_mode: SceneRenderBlendMode,
) -> String {
    let mut key = format!(
        "phase10:shader:{:?}:{}:{:?}",
        program.kind,
        program.metal_source_path.display(),
        blend_mode
    );
    for (name, value) in merged_shader_defines(program, defines) {
        key.push('|');
        key.push_str(&name);
        key.push('=');
        key.push_str(&value.to_string());
    }
    key
}

#[cfg(target_os = "macos")]
fn phase10_base_passes(visual: &ScenePhase10VisualPlan) -> &[SceneMaterialPassPlan] {
    &visual.material.passes
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_visual_requires_offscreen_chain(visual: &ScenePhase10VisualPlan) -> bool {
    if visual.puppet_path.is_some() {
        return phase10_visual_pass_chain(visual).len() > 1;
    }
    !phase10_base_passes(visual).is_empty() || !visual.effect_chain.is_empty()
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_visual_first_pass_is_mask_alpha(
    visual: &ScenePhase10VisualPlan,
) -> bool {
    phase10_base_passes(visual)
        .first()
        .map(|pass| pass.program.kind == SceneShaderProgramKind::MaskAlpha)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_mask_apply_shader_program() -> SceneShaderProgram {
    use crate::services::scene_shader_material_service::SceneShaderProgramKind;

    SceneShaderProgram {
        key: "clippingmaskimage4-apply".to_string(),
        kind: SceneShaderProgramKind::MaskApply,
        metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-mask-apply.metal"),
        vertex_entry: "compat_mask_apply_vertex",
        fragment_entry: "compat_mask_apply_fragment",
        variant_defines: BTreeMap::new(),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_render_target_size(visual: &ScenePhase10VisualPlan) -> (usize, usize) {
    (
        visual.quad.width.abs().max(1.0).ceil() as usize,
        visual.quad.height.abs().max(1.0).ceil() as usize,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_visual_needs_background_snapshot(
    visual: &ScenePhase10VisualPlan,
) -> bool {
    visual.effect_chain.iter().any(|effect| {
        effect.passes.iter().any(|pass| {
            pass.copy_background
                || pass.input_bindings.iter().any(|binding| {
                    matches!(
                        binding.source,
                        ScenePhase10InputSource::Background
                            | ScenePhase10InputSource::CopiedBackground
                    )
                })
        })
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_resolved_pass_target_name<'a>(
    resolved_pass: &'a Phase10ResolvedPass<'_>,
) -> Option<&'a str> {
    match resolved_pass.context {
        Phase10PassContext::Effect(effect_pass) => effect_pass.target_name.as_deref(),
        Phase10PassContext::Base => None,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_local_background_projection(
    visual: &ScenePhase10VisualPlan,
    width: usize,
    height: usize,
) -> SceneProjection {
    SceneProjection {
        scene_origin_x: -visual.quad.left,
        scene_origin_y: -visual.quad.top,
        scene_canvas_height: visual.quad.height.abs().max(1.0),
        camera_scale: 1.0,
        view_width: width.max(1) as f64,
        view_height: height.max(1) as f64,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_puppet_offscreen_projection(
    visual: &ScenePhase10VisualPlan,
    canvas_height: f64,
    width: usize,
    height: usize,
) -> SceneProjection {
    SceneProjection {
        scene_origin_x: -visual.quad.left,
        scene_origin_y: -visual.quad.top,
        scene_canvas_height: canvas_height,
        camera_scale: 1.0,
        view_width: width.max(1) as f64,
        view_height: height.max(1) as f64,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_fullscreen_vertices(tint: SceneRenderColor) -> [SceneVertex; 6] {
    let color = color_to_shader(tint);
    [
        SceneVertex {
            position: [-1.0, -1.0],
            uv: [0.0, 1.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [1.0, -1.0],
            uv: [1.0, 1.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [-1.0, 1.0],
            uv: [0.0, 0.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [-1.0, 1.0],
            uv: [0.0, 0.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [1.0, -1.0],
            uv: [1.0, 1.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [1.0, 1.0],
            uv: [1.0, 0.0],
            color,
            opacity: 1.0,
        },
    ]
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_solid_texture_key(
    color: SceneRenderColor,
    width: usize,
    height: usize,
) -> String {
    format!(
        "phase10:solid:{:02x}{:02x}{:02x}{:02x}:{width}x{height}",
        color.red, color.green, color.blue, color.alpha,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_output_texture_key(object_id: u32, width: usize, height: usize) -> String {
    format!("phase10:output:{object_id}:{width}x{height}")
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_scratch_texture_key(width: usize, height: usize, slot: usize) -> String {
    format!("phase10:scratch:{width}x{height}:{slot}")
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_named_target_texture_key(
    object_id: u32,
    target_name: &str,
    width: usize,
    height: usize,
) -> String {
    format!("phase10:named:{object_id}:{target_name}:{width}x{height}")
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_background_texture_key(
    object_id: u32,
    width: usize,
    height: usize,
) -> String {
    format!("phase10:background:{object_id}:{width}x{height}")
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase10EffectTextureSource {
    GraphInput(ScenePhase10InputSource),
    MaterialSlot(usize),
    OverrideSlot(usize),
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_effect_texture_slot_plan(
    material_textures: &[SceneMaterialTextureBinding],
    effect_pass: &ScenePhase10EffectPassNode,
) -> BTreeMap<usize, Phase10EffectTextureSource> {
    let mut slots = BTreeMap::new();
    for binding in &effect_pass.input_bindings {
        slots.insert(
            binding.slot,
            Phase10EffectTextureSource::GraphInput(binding.source.clone()),
        );
    }
    for binding in material_textures {
        if binding.texture_name.is_some() && binding.resolved_path.is_some() {
            slots.insert(
                binding.slot_index,
                Phase10EffectTextureSource::MaterialSlot(binding.slot_index),
            );
        }
    }
    for (slot, texture) in effect_pass.texture_overrides.iter().enumerate() {
        if texture.is_some() {
            slots.insert(slot, Phase10EffectTextureSource::OverrideSlot(slot));
        }
    }
    slots
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_pass_shader_defines(
    resolved_pass: &Phase10ResolvedPass<'_>,
) -> BTreeMap<String, i32> {
    let Some(effect_kind) = phase10_effect_family_from_program(&resolved_pass.pass.program) else {
        return resolved_pass.pass.combos.clone();
    };
    let Some(contract) = phase10b_effect_contract_for_kind(effect_kind) else {
        return resolved_pass.pass.combos.clone();
    };
    let mut defines = contract
        .supported_combo_defaults
        .iter()
        .map(|(name, value)| ((*name).to_string(), *value))
        .collect::<BTreeMap<_, _>>();
    for (name, value) in &resolved_pass.pass.combos {
        defines.insert(name.clone(), *value);
    }
    if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
        let slot_plan = phase10_effect_texture_slot_plan(&resolved_pass.pass.textures, effect_pass);
        for slot_contract in contract.runtime_binding_layout {
            if !slot_plan.contains_key(&slot_contract.slot) {
                continue;
            }
            match slot_contract.semantic {
                crate::services::scene_shader_material_service::ScenePhase10bBindingSemantic::OpacityMask => {
                    if !resolved_pass
                        .pass
                        .combos
                        .keys()
                        .any(|name| name.eq_ignore_ascii_case("MASK"))
                    {
                        defines.insert("MASK".to_string(), 1);
                    }
                }
                crate::services::scene_shader_material_service::ScenePhase10bBindingSemantic::TimeOffset => {
                    if !resolved_pass
                        .pass
                        .combos
                        .keys()
                        .any(|name| name.eq_ignore_ascii_case("TIMEOFFSET"))
                    {
                        defines.insert("TIMEOFFSET".to_string(), 1);
                    }
                }
                _ => {}
            }
        }
    }
    defines
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_effect_uniform_values(
    resolved_pass: &Phase10ResolvedPass<'_>,
) -> BTreeMap<String, SceneMaterialUniformValue> {
    let mut values = BTreeMap::new();
    for (name, value) in &resolved_pass.pass.uniforms {
        values.insert(normalized_effect_uniform_name(name), value.clone());
    }
    if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
        for (name, value) in &effect_pass.constants {
            values.insert(normalized_effect_uniform_name(name), value.clone());
        }
    }
    values
}

#[cfg(target_os = "macos")]
fn normalized_effect_uniform_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_uniform_float(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: f32,
) -> f32 {
    aliases
        .iter()
        .find_map(|alias| values.get(*alias))
        .and_then(SceneMaterialUniformValue::as_float)
        .unwrap_or(default)
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_uniform_vec2(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: [f32; 2],
) -> [f32; 2] {
    for alias in aliases {
        let Some(value) = values.get(*alias) else {
            continue;
        };
        if let Some(vector) = value.as_float2() {
            return vector;
        }
        if let Some(number) = value.as_float() {
            return [number, number];
        }
    }
    default
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_uniform_color(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: [f32; 4],
) -> [f32; 4] {
    for alias in aliases {
        let Some(value) = values.get(*alias) else {
            continue;
        };
        if let Some(color) = value.as_float4() {
            return color;
        }
        if let Some(color) = value.as_float3() {
            return [color[0], color[1], color[2], 1.0];
        }
    }
    default
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_uniform_vec4(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: [f32; 4],
) -> [f32; 4] {
    for alias in aliases {
        let Some(value) = values.get(*alias) else {
            continue;
        };
        if let Some(vector) = value.as_float4() {
            return vector;
        }
        if let Some(vector) = value.as_float3() {
            return [vector[0], vector[1], vector[2], default[3]];
        }
        if let Some(vector) = value.as_float2() {
            return [vector[0], vector[1], default[2], default[3]];
        }
        if let Some(number) = value.as_float() {
            return [number, number, default[2], default[3]];
        }
    }
    default
}

#[cfg(target_os = "macos")]
fn phase10_optional_uniform_float(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
) -> Option<f32> {
    aliases
        .iter()
        .find_map(|alias| values.get(*alias))
        .and_then(SceneMaterialUniformValue::as_float)
}

#[cfg(target_os = "macos")]
fn phase10_optional_uniform_vec2(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
) -> Option<[f32; 2]> {
    for alias in aliases {
        let Some(value) = values.get(*alias) else {
            continue;
        };
        if let Some(vector) = value.as_float2() {
            return Some(vector);
        }
        if let Some(number) = value.as_float() {
            return Some([number, number]);
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_perspective_corner_uniforms(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
) -> ([f32; 4], [f32; 4]) {
    let point0 = phase10_uniform_vec2(values, &["point0"], [0.0, 0.0]);
    let point1 = phase10_uniform_vec2(values, &["point1"], [1.0, 0.0]);
    let point2 = phase10_uniform_vec2(values, &["point2"], [1.0, 1.0]);
    let point3 = phase10_uniform_vec2(values, &["point3"], [0.0, 1.0]);
    (
        [point0[0], point0[1], point1[0], point1[1]],
        [point2[0], point2[1], point3[0], point3[1]],
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_skew_controls(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
) -> [f32; 4] {
    let direct_anchor = phase10_optional_uniform_vec2(
        values,
        &["anchor", "uieditorpropertiesanchor"],
    );
    let direct_skew = phase10_optional_uniform_vec2(values, &["skew", "uieditorpropertiesskew"]);
    let direct_skew_x =
        phase10_optional_uniform_float(values, &["skewx", "uieditorpropertiesskewx"]);
    let direct_skew_y =
        phase10_optional_uniform_float(values, &["skewy", "uieditorpropertiesskewy"]);
    let top = phase10_optional_uniform_float(values, &["top", "uieditorpropertiestop"]);
    let bottom =
        phase10_optional_uniform_float(values, &["bottom", "uieditorpropertiesbottom"]);
    let left = phase10_optional_uniform_float(values, &["left", "uieditorpropertiesleft"]);
    let right = phase10_optional_uniform_float(values, &["right", "uieditorpropertiesright"]);

    let mut anchor = direct_anchor.unwrap_or([0.5, 0.5]);
    let mut skew_x = direct_skew.map(|vector| vector[0]).unwrap_or(0.0);
    let mut skew_y = direct_skew.map(|vector| vector[1]).unwrap_or(0.0);

    if let Some(value) = direct_skew_x {
        skew_x = value;
    } else if let (Some(top), Some(bottom)) = (top, bottom) {
        skew_x = bottom - top;
        if direct_anchor.is_none() && skew_x.abs() >= 1e-6 {
            anchor[1] = (-top / skew_x).clamp(0.0, 1.0);
        }
    } else if let Some(top) = top {
        skew_x = -top * 2.0;
    } else if let Some(bottom) = bottom {
        skew_x = bottom * 2.0;
    }

    if let Some(value) = direct_skew_y {
        skew_y = value;
    } else if let (Some(left), Some(right)) = (left, right) {
        skew_y = right - left;
        if direct_anchor.is_none() && skew_y.abs() >= 1e-6 {
            anchor[0] = (-left / skew_y).clamp(0.0, 1.0);
        }
    } else if let Some(left) = left {
        skew_y = -left * 2.0;
    } else if let Some(right) = right {
        skew_y = right * 2.0;
    }

    [skew_x, skew_y, anchor[0], anchor[1]]
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_spin_controls(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
) -> (f32, f32, [f32; 4]) {
    let angle = phase10_uniform_float(values, &["amount", "angle"], 0.0);
    let speed = phase10_uniform_float(values, &["speed"], 1.0);
    let mut center_and_mask =
        phase10_uniform_vec4(values, &["center", "spincenter"], [0.5, 0.5, 0.1, 0.002]);
    center_and_mask[2] = phase10_uniform_float(values, &["size"], center_and_mask[2]);
    center_and_mask[3] = phase10_uniform_float(values, &["feather"], center_and_mask[3]);
    (angle, speed, center_and_mask)
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_transform_controls(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
) -> ([f32; 4], [f32; 4], f32) {
    let offset = phase10_optional_uniform_vec2(
        values,
        &[
            "offset",
            "translate",
            "translation",
            "uieditorpropertiesoffset",
            "uieditorpropertiestranslate",
            "uieditorpropertiestranslation",
        ],
    )
    .unwrap_or([0.0, 0.0]);
    let scale = phase10_optional_uniform_vec2(values, &["scale", "uieditorpropertiesscale"])
        .unwrap_or([1.0, 1.0]);
    let anchor = phase10_optional_uniform_vec2(
        values,
        &[
            "anchor",
            "pivot",
            "center",
            "uieditorpropertiesanchor",
            "uieditorpropertiespivot",
            "uieditorpropertiescenter",
        ],
    )
    .unwrap_or([0.5, 0.5]);
    let angle = phase10_uniform_float(
        values,
        &[
            "rotation",
            "angle",
            "uieditorpropertiesrotation",
            "uieditorpropertiesangle",
        ],
        0.0,
    );
    (
        [offset[0], offset[1], scale[0], scale[1]],
        [anchor[0], anchor[1], 0.0, 0.0],
        angle,
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn rotate2d(vector: [f32; 2], angle: f32) -> [f32; 2] {
    let sine = angle.sin();
    let cosine = angle.cos();
    [
        vector[0] * cosine - vector[1] * sine,
        vector[0] * sine + vector[1] * cosine,
    ]
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_alpha_prefill_required(blend_mode: SceneRenderBlendMode) -> bool {
    !matches!(blend_mode, SceneRenderBlendMode::Normal)
}

#[cfg(target_os = "macos")]
pub(crate) fn phase10_effect_family_from_program(
    program: &SceneShaderProgram,
) -> Option<SceneCompatEffectKind> {
    match program.kind {
        SceneShaderProgramKind::EffectCompat(kind) => Some(kind),
        _ => None,
    }
}
