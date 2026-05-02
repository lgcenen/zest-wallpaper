use std::collections::BTreeMap;

use serde_json::Value;

use crate::models::{
    SceneParticleChildKind, SceneParticleChildRuntime, SceneParticleControlPointOverride,
    SceneParticleControlPointRuntime, SceneParticleDiagnosticKind, SceneParticleEmitterRuntime,
    SceneParticleInstanceOverride, SceneParticleKind, SceneParticleRendererFamily,
    SceneParticleRendererRuntime, SceneParticleRuntime, SceneParticleRuntimeAdapter,
    SceneParticleRuntimeDiagnostic, SceneParticleScheduleMode, SceneParticleStageRuntime,
    SceneParticleSystemRuntime,
};

pub fn build_scene_particle_runtime(
    object_id: u32,
    object_name: String,
    particle_path: String,
    particle_json: Option<&Value>,
    object_origin: [f64; 3],
    object_scale: [f64; 3],
    object_angles: Option<[f64; 3]>,
    instance_override: SceneParticleInstanceOverride,
) -> SceneParticleRuntime {
    let system = particle_json.map(parse_particle_system).unwrap_or_default();
    let (mut adapter, diagnostics) = classify_particle_runtime(particle_json.is_some(), &system);
    if adapter.supported && particle_runtime_uses_input_control_points(&system, &instance_override)
    {
        adapter.schedule_mode = SceneParticleScheduleMode::InputDriven;
    }

    SceneParticleRuntime {
        object_id,
        object_name,
        particle_path,
        object_origin,
        object_scale,
        object_angles,
        system,
        instance_override,
        adapter,
        diagnostics,
    }
}

pub fn scene_particle_runtime_uses_input_control_points(runtime: &SceneParticleRuntime) -> bool {
    particle_runtime_uses_input_control_points(&runtime.system, &runtime.instance_override)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn build_authored_particle_runtime_for_resource(
    particle_path: String,
    particle_json: &Value,
) -> SceneParticleRuntime {
    build_scene_particle_runtime(
        0,
        "Particle Resource".to_string(),
        particle_path,
        Some(particle_json),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        None,
        SceneParticleInstanceOverride::default(),
    )
}

pub fn scene_particle_control_point_override(
    key: String,
    value: Option<[f64; 3]>,
    binding: Option<String>,
) -> SceneParticleControlPointOverride {
    SceneParticleControlPointOverride {
        key,
        value,
        binding,
    }
}

fn parse_particle_system(root: &Value) -> SceneParticleSystemRuntime {
    SceneParticleSystemRuntime {
        max_count: u32_field(root, "maxcount").or_else(|| u32_field(root, "maxCount")),
        start_time: f64_field(root, "starttime").or_else(|| f64_field(root, "startTime")),
        flags: string_list_field(root, "flags"),
        material: string_field(root, "material"),
        emitters: particle_entries(root, &["emitter", "emitters"])
            .into_iter()
            .map(parse_emitter)
            .collect(),
        renderers: particle_entries(root, &["renderer", "renderers"])
            .into_iter()
            .map(parse_renderer)
            .collect(),
        control_points: particle_entries(root, &["controlpoint", "controlpoints"])
            .into_iter()
            .map(parse_control_point)
            .collect(),
        children: particle_entries(root, &["children", "child"])
            .into_iter()
            .map(parse_child)
            .collect(),
        initializer_names: stage_names(root, &["initializer", "initializers"]),
        operator_names: stage_names(root, &["operator", "operators"]),
        initializers: particle_entries(root, &["initializer", "initializers"])
            .into_iter()
            .filter_map(parse_stage)
            .collect(),
        operators: particle_entries(root, &["operator", "operators"])
            .into_iter()
            .filter_map(parse_stage)
            .collect(),
        sequence_multiplier: f64_field(root, "sequencemultiplier")
            .or_else(|| f64_field(root, "sequenceMultiplier")),
    }
}

fn parse_emitter(value: &Value) -> SceneParticleEmitterRuntime {
    let control_point =
        u32_field(value, "controlpoint").or_else(|| u32_field(value, "controlPoint"));
    let lock_to_pointer = bool_field(value, "locktopointer").unwrap_or(false)
        || bool_field(value, "lockToPointer").unwrap_or(false);
    let schedule_mode = if control_point.is_some() || lock_to_pointer {
        SceneParticleScheduleMode::InputDriven
    } else {
        SceneParticleScheduleMode::Autonomous
    };

    SceneParticleEmitterRuntime {
        name: string_field(value, "name"),
        rate: f64_field(value, "rate"),
        origin: vector3_field(value, "origin"),
        distance_min: f64_field(value, "distancemin").or_else(|| f64_field(value, "distanceMin")),
        distance_max: f64_field(value, "distancemax").or_else(|| f64_field(value, "distanceMax")),
        directions: vector3_list_field(value, "directions")
            .or_else(|| vector3_field(value, "directions").map(|direction| vec![direction]))
            .or_else(|| vector3_field(value, "direction").map(|direction| vec![direction]))
            .unwrap_or_default(),
        sign: f64_field(value, "sign"),
        speed_min: f64_field(value, "speedmin").or_else(|| f64_field(value, "speedMin")),
        speed_max: f64_field(value, "speedmax").or_else(|| f64_field(value, "speedMax")),
        control_point,
        instantaneous: bool_field(value, "instantaneous").unwrap_or(false),
        schedule_mode,
    }
}

fn parse_renderer(value: &Value) -> SceneParticleRendererRuntime {
    let name = string_field(value, "name");
    let family = particle_renderer_family(name.as_deref());
    SceneParticleRendererRuntime {
        name,
        family,
        length: f64_field(value, "length"),
        max_length: f64_field(value, "maxlength").or_else(|| f64_field(value, "maxLength")),
        min_length: f64_field(value, "minlength").or_else(|| f64_field(value, "minLength")),
        subdivision: u32_field(value, "subdivision"),
        segments: u32_field(value, "segments"),
        axis: string_field(value, "axis"),
        orientation: string_field(value, "orientation"),
        uv_scale: vector2_field(value, "uvscale").or_else(|| vector2_field(value, "uvScale")),
        uv_scrolling: vector2_field(value, "uvscrolling")
            .or_else(|| vector2_field(value, "uvScrolling")),
        uv_smoothing: f64_field(value, "uvsmoothing").or_else(|| f64_field(value, "uvSmoothing")),
        fade_alpha: f64_field(value, "fadealpha").or_else(|| f64_field(value, "fadeAlpha")),
    }
}

fn parse_control_point(value: &Value) -> SceneParticleControlPointRuntime {
    let flags = string_list_field(value, "flags");
    let lock_to_pointer = bool_field(value, "locktopointer")
        .or_else(|| bool_field(value, "lockToPointer"))
        .unwrap_or(false)
        || control_point_flags_lock_to_pointer(&flags);

    SceneParticleControlPointRuntime {
        id: u32_field(value, "id"),
        flags,
        offset: vector3_field(value, "offset"),
        parent_control_point: u32_field(value, "parentcontrolpoint")
            .or_else(|| u32_field(value, "parentControlPoint")),
        lock_to_pointer,
    }
}

fn parse_child(value: &Value) -> SceneParticleChildRuntime {
    let name = string_field(value, "name");
    SceneParticleChildRuntime {
        child_type: particle_child_kind(string_field(value, "type").as_deref(), name.as_deref()),
        name,
        origin: vector3_field(value, "origin"),
        scale: vector3_field(value, "scale"),
        angles: vector3_field(value, "angles"),
        probability: f64_field(value, "probability"),
        max_count: u32_field(value, "maxcount").or_else(|| u32_field(value, "maxCount")),
        control_point_start_index: u32_field(value, "controlpointstartindex")
            .or_else(|| u32_field(value, "controlPointStartIndex")),
    }
}

fn parse_stage(value: &Value) -> Option<SceneParticleStageRuntime> {
    let name = string_field(value, "name")
        .or_else(|| string_field(value, "type"))
        .or_else(|| value.as_str().map(ToString::to_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let fields = value
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    Some(SceneParticleStageRuntime { name, fields })
}

fn classify_particle_runtime(
    has_particle_json: bool,
    system: &SceneParticleSystemRuntime,
) -> (
    SceneParticleRuntimeAdapter,
    Vec<SceneParticleRuntimeDiagnostic>,
) {
    let mut diagnostics = Vec::new();
    if !has_particle_json {
        diagnostics.push(runtime_diagnostic(
            "particle-runtime-json-missing",
            "Particle object references a resource that was not available to the parser.",
        ));
        return (
            unsupported_adapter("particle resource is unresolved"),
            diagnostics,
        );
    }

    if system.emitters.len() != 1 {
        diagnostics.push(runtime_diagnostic(
            "particle-emitter-count-unsupported",
            "Phase-09e supports exactly one emitter in the narrow adapter.",
        ));
    }
    if system.renderers.len() != 1 {
        diagnostics.push(runtime_diagnostic(
            "particle-renderer-count-unsupported",
            "Phase-09e supports exactly one renderer in the narrow adapter.",
        ));
    }

    let renderer = system.renderers.first();
    let renderer_family = renderer.map(|renderer| renderer.family);
    let draw_kind = renderer.and_then(|renderer| particle_draw_kind(renderer.family));
    match renderer_family {
        Some(SceneParticleRendererFamily::Sprite) => {}
        Some(SceneParticleRendererFamily::Unsupported) | None => {
            diagnostics.push(runtime_diagnostic(
                "particle-renderer-family-unsupported",
                "Renderer family is outside the phase-09e trail adapter whitelist.",
            ))
        }
        Some(SceneParticleRendererFamily::SpriteTrail)
        | Some(SceneParticleRendererFamily::Rope)
        | Some(SceneParticleRendererFamily::RopeTrail) => {}
    }

    if renderer_family == Some(SceneParticleRendererFamily::Sprite)
        || renderer_family == Some(SceneParticleRendererFamily::SpriteTrail)
    {
        diagnostics.extend(sprite_particle_runtime_diagnostics(system));
    } else if !system.children.is_empty() {
        let unsupported_child = system
            .children
            .iter()
            .find(|child| {
                matches!(
                    child.child_type,
                    SceneParticleChildKind::EventSpawn | SceneParticleChildKind::Unsupported
                )
            })
            .map(|child| match child.child_type {
                SceneParticleChildKind::EventSpawn => "eventspawn",
                SceneParticleChildKind::Unsupported => "unsupported child",
                SceneParticleChildKind::Static => "static",
                SceneParticleChildKind::EventFollow => "eventfollow",
                SceneParticleChildKind::EventDeath => "eventdeath",
            });
        diagnostics.push(runtime_diagnostic(
            "particle-child-hierarchy-deferred",
            match unsupported_child {
                Some(kind) => {
                    format!("Particle child hierarchy includes {kind}; phase-09e preserves it but does not map it to the narrow adapter.")
                }
                None => "Particle child hierarchy is preserved for the first-class runtime and kept out of the narrow adapter.".to_string(),
            },
        ));
    }

    if !matches!(
        renderer_family,
        Some(SceneParticleRendererFamily::Sprite | SceneParticleRendererFamily::SpriteTrail)
    ) {
        for stage_name in system
            .initializer_names
            .iter()
            .chain(system.operator_names.iter())
        {
            if !particle_stage_name_is_adapter_safe(stage_name) {
                diagnostics.push(runtime_diagnostic(
                    "particle-stage-unsupported",
                    format!(
                        "Particle initializer/operator {stage_name:?} is outside the phase-09e adapter whitelist."
                    ),
                ));
            }
        }
    }

    if diagnostics.is_empty() {
        let emitter = system.emitters.first();
        let schedule_mode = emitter
            .map(|emitter| emitter.schedule_mode)
            .unwrap_or(SceneParticleScheduleMode::InputDriven);

        let is_sprite_family = matches!(
            renderer_family,
            Some(
                SceneParticleRendererFamily::Sprite | SceneParticleRendererFamily::SpriteTrail
            )
        );

        if !is_sprite_family {
            if let Some(renderer) = renderer {
                if renderer.min_length.is_some() {
                    diagnostics.push(semi_adapted_diagnostic(
                        "particle-renderer-field-semi-adapted",
                        "Renderer minLength is parsed but not consumed in the trail adapter.",
                    ));
                }
                if renderer.segments.is_some() {
                    diagnostics.push(semi_adapted_diagnostic(
                        "particle-renderer-field-semi-adapted",
                        "Renderer segments is parsed but not consumed in the trail adapter.",
                    ));
                }
                if renderer.axis.is_some() {
                    diagnostics.push(semi_adapted_diagnostic(
                        "particle-renderer-field-semi-adapted",
                        "Renderer axis is parsed but not consumed in the trail adapter.",
                    ));
                }
                if renderer.orientation.is_some() {
                    diagnostics.push(semi_adapted_diagnostic(
                        "particle-renderer-field-semi-adapted",
                        "Renderer orientation is parsed but not consumed in the trail adapter.",
                    ));
                }
                if renderer.uv_smoothing.is_some() {
                    diagnostics.push(semi_adapted_diagnostic(
                        "particle-renderer-field-semi-adapted",
                        "Renderer uvSmoothing is parsed but not consumed in the trail adapter.",
                    ));
                }
            }
        }

        if is_sprite_family {
            if let Some(emitter) = emitter {
                if emitter.control_point.is_some() {
                    diagnostics.push(semi_adapted_diagnostic(
                        "particle-emitter-field-semi-adapted",
                        "Emitter controlPoint is parsed but not consumed in the sprite runtime.",
                    ));
                }
            }
            diagnostics.extend(sprite_stage_semi_adapted_diagnostics(system));
        }

        (
            SceneParticleRuntimeAdapter {
                supported: true,
                draw_kind,
                schedule_mode,
                reason: None,
            },
            diagnostics,
        )
    } else {
        (
            unsupported_adapter(
                diagnostics
                    .first()
                    .map(|diagnostic| diagnostic.message.as_str())
                    .unwrap_or("particle runtime is outside the phase-09e adapter whitelist"),
            ),
            diagnostics,
        )
    }
}

fn sprite_particle_runtime_diagnostics(
    system: &SceneParticleSystemRuntime,
) -> Vec<SceneParticleRuntimeDiagnostic> {
    let mut diagnostics = Vec::new();

    for child in &system.children {
        if matches!(
            child.child_type,
            SceneParticleChildKind::EventSpawn | SceneParticleChildKind::Unsupported
        ) {
            diagnostics.push(runtime_diagnostic(
                "particle-child-unsupported",
                match child.child_type {
                    SceneParticleChildKind::EventSpawn => {
                        "Sprite particle child type eventspawn is outside phase-09e2."
                    }
                    SceneParticleChildKind::Unsupported => {
                        "Sprite particle child type is not recognized by phase-09e2."
                    }
                    SceneParticleChildKind::Static
                    | SceneParticleChildKind::EventFollow
                    | SceneParticleChildKind::EventDeath => unreachable!(),
                },
            ));
        }
    }

    for stage_name in system
        .initializer_names
        .iter()
        .chain(system.operator_names.iter())
    {
        if sprite_stage_name_is_unsupported(stage_name) {
            diagnostics.push(runtime_diagnostic(
                "particle-stage-unsupported",
                format!(
                    "Sprite particle initializer/operator {stage_name:?} is outside the phase-09e2 whitelist."
                ),
            ));
        }
    }

    diagnostics
}

fn sprite_stage_semi_adapted_diagnostics(
    system: &SceneParticleSystemRuntime,
) -> Vec<SceneParticleRuntimeDiagnostic> {
    let mut diagnostics = Vec::new();
    for stage_name in system
        .initializer_names
        .iter()
        .chain(system.operator_names.iter())
    {
        let lower = stage_name.to_ascii_lowercase();
        let compact: String = lower
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if compact.contains("colorchange") || compact.contains("colourchange") {
            diagnostics.push(semi_adapted_diagnostic(
                "particle-stage-semi-adapted",
                format!(
                    "Sprite particle stage {stage_name:?} is whitelisted but not yet consumed by the runtime."
                ),
            ));
        }
        if compact.contains("controlpointattract") {
            diagnostics.push(semi_adapted_diagnostic(
                "particle-stage-semi-adapted",
                format!(
                    "Sprite particle stage {stage_name:?} is whitelisted but not yet consumed by the runtime."
                ),
            ));
        }
    }
    diagnostics
}

fn particle_runtime_uses_input_control_points(
    system: &SceneParticleSystemRuntime,
    instance_override: &SceneParticleInstanceOverride,
) -> bool {
    !instance_override.control_points.is_empty()
        || system
            .emitters
            .iter()
            .any(|emitter| emitter.schedule_mode == SceneParticleScheduleMode::InputDriven)
        || system.control_points.iter().any(|control_point| {
            control_point.lock_to_pointer
                || control_point_flags_lock_to_pointer(&control_point.flags)
        })
}

fn control_point_flags_lock_to_pointer(flags: &[String]) -> bool {
    flags.iter().any(|flag| {
        let normalized = flag.trim().to_ascii_lowercase();
        normalized == "locktopointer"
            || normalized == "lock_to_pointer"
            || normalized == "pointer"
            || normalized
                .parse::<u32>()
                .map(|bits| (bits & 1) != 0)
                .unwrap_or(false)
    })
}

fn unsupported_adapter(reason: impl Into<String>) -> SceneParticleRuntimeAdapter {
    SceneParticleRuntimeAdapter {
        supported: false,
        draw_kind: None,
        schedule_mode: SceneParticleScheduleMode::InputDriven,
        reason: Some(reason.into()),
    }
}

fn particle_draw_kind(family: SceneParticleRendererFamily) -> Option<SceneParticleKind> {
    match family {
        SceneParticleRendererFamily::Rope | SceneParticleRendererFamily::RopeTrail => {
            Some(SceneParticleKind::LineTrail)
        }
        SceneParticleRendererFamily::SpriteTrail
        | SceneParticleRendererFamily::Sprite
        | SceneParticleRendererFamily::Unsupported => None,
    }
}

fn particle_renderer_family(name: Option<&str>) -> SceneParticleRendererFamily {
    match name
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("sprite") => SceneParticleRendererFamily::Sprite,
        Some("spritetrail") => SceneParticleRendererFamily::SpriteTrail,
        Some("rope") => SceneParticleRendererFamily::Rope,
        Some("ropetrail") => SceneParticleRendererFamily::RopeTrail,
        _ => SceneParticleRendererFamily::Unsupported,
    }
}

fn particle_child_kind(value: Option<&str>, child_name: Option<&str>) -> SceneParticleChildKind {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("static") => SceneParticleChildKind::Static,
        Some("eventfollow") => SceneParticleChildKind::EventFollow,
        Some("eventdeath") => SceneParticleChildKind::EventDeath,
        Some("eventspawn") => SceneParticleChildKind::EventSpawn,
        None if child_name.is_some() => SceneParticleChildKind::Static,
        _ => SceneParticleChildKind::Unsupported,
    }
}

fn particle_stage_name_is_adapter_safe(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let unsupported_tokens = [
        "collision",
        "collide",
        "mask",
        "remap",
        "boid",
        "flock",
        "vortex",
        "turbulence",
        "sequence",
        "event",
        "inherit",
        "model",
        "bounds",
    ];
    if unsupported_tokens.iter().any(|token| lower.contains(token)) {
        return false;
    }

    let adapter_safe_tokens = [
        "lifetime", "life", "size", "color", "colour", "alpha", "fade", "velocity", "speed",
        "movement", "move", "position", "rotation", "spin", "random", "gravity",
    ];
    adapter_safe_tokens
        .iter()
        .any(|token| lower.contains(token))
}

fn sprite_stage_name_is_unsupported(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let compact = lower
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    if compact == "oscillateposition"
        || compact.ends_with("oscillateposition")
        || compact == "positionoscillate"
        || compact.ends_with("positionoscillate")
    {
        return false;
    }
    if compact == "oscillatealpha"
        || compact.ends_with("oscillatealpha")
        || compact == "alphaoscillate"
        || compact.ends_with("alphaoscillate")
    {
        return false;
    }
    if compact == "oscillatesize"
        || compact.ends_with("oscillatesize")
        || compact == "sizeoscillate"
        || compact.ends_with("sizeoscillate")
    {
        return false;
    }
    if compact == "colorchange"
        || compact.ends_with("colorchange")
        || compact == "colourchange"
        || compact.ends_with("colourchange")
    {
        return false;
    }
    if compact == "controlpointattract"
        || compact.ends_with("controlpointattract")
        || compact == "attractcontrolpoint"
        || compact.ends_with("attractcontrolpoint")
    {
        return false;
    }

    let unsupported_tokens = [
        "collision",
        "collide",
        "model",
        "mask",
        "boid",
        "flock",
        "vortex",
        "remap",
        "inherit",
        "layerimage",
        "maintain",
        "oscillate",
    ];
    unsupported_tokens.iter().any(|token| lower.contains(token))
}

fn runtime_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
) -> SceneParticleRuntimeDiagnostic {
    SceneParticleRuntimeDiagnostic {
        code: code.into(),
        message: message.into(),
        diagnostic_kind: SceneParticleDiagnosticKind::Blocking,
    }
}

fn semi_adapted_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
) -> SceneParticleRuntimeDiagnostic {
    SceneParticleRuntimeDiagnostic {
        code: code.into(),
        message: message.into(),
        diagnostic_kind: SceneParticleDiagnosticKind::SemiAdapted,
    }
}

fn particle_entries<'a>(root: &'a Value, keys: &[&str]) -> Vec<&'a Value> {
    for key in keys {
        let Some(value) = root.get(*key) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            return items.iter().collect();
        }
        if value.is_object() {
            return vec![value];
        }
    }
    Vec::new()
}

fn stage_names(root: &Value, keys: &[&str]) -> Vec<String> {
    particle_entries(root, keys)
        .into_iter()
        .filter_map(|value| {
            string_field(value, "name")
                .or_else(|| string_field(value, "type"))
                .or_else(|| value.as_str().map(ToString::to_string))
        })
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(as_string)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn f64_field(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(as_f64)
}

fn u32_field(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.round() as u32)
}

fn bool_field(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(as_bool)
}

fn string_list_field(value: &Value, key: &str) -> Vec<String> {
    let Some(raw) = value.get(key) else {
        return Vec::new();
    };
    if let Some(items) = raw.as_array() {
        return items
            .iter()
            .filter_map(as_string)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
    }
    as_string(raw)
        .map(|value| {
            value
                .split(|character: char| character.is_whitespace() || character == ',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn vector3_list_field(value: &Value, key: &str) -> Option<Vec<[f64; 3]>> {
    let raw = value.get(key)?;
    let items = raw.as_array()?;
    let vectors = items
        .iter()
        .filter_map(|item| parse_vector::<3>(item))
        .collect::<Vec<_>>();
    (!vectors.is_empty()).then_some(vectors)
}

fn vector2_field(value: &Value, key: &str) -> Option<[f64; 2]> {
    value.get(key).and_then(parse_vector::<2>)
}

fn vector3_field(value: &Value, key: &str) -> Option<[f64; 3]> {
    value.get(key).and_then(parse_vector::<3>)
}

fn parse_vector<const N: usize>(value: &Value) -> Option<[f64; N]> {
    if let Some(values) = value.as_array() {
        let mut result = [0.0; N];
        for (index, slot) in result.iter_mut().enumerate() {
            *slot = values.get(index).and_then(as_f64).unwrap_or(0.0);
        }
        return Some(result);
    }

    let text = as_string(value)?;
    let values = text
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter_map(|part| {
            let part = part.trim();
            (!part.is_empty())
                .then(|| part.parse::<f64>().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    if values.len() < N.saturating_sub(1).max(1) {
        return None;
    }
    let mut result = [0.0; N];
    for (index, slot) in result.iter_mut().enumerate() {
        *slot = values.get(index).copied().unwrap_or(0.0);
    }
    Some(result)
}

fn as_string(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_f64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Some(value.to_string());
    }
    None
}

fn as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn as_bool(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        let text = value.as_str()?.trim().to_ascii_lowercase();
        match text.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::models::{
        SceneParticleChildKind, SceneParticleInstanceOverride, SceneParticleRendererFamily,
        SceneParticleScheduleMode,
    };

    use super::{
        build_authored_particle_runtime_for_resource, build_scene_particle_runtime,
        scene_particle_runtime_uses_input_control_points,
    };

    #[test]
    fn parses_first_class_particle_hierarchy_and_adapter_family() {
        let particle = json!({
            "maxcount": 128,
            "starttime": 0.25,
            "material": "materials/particle.json",
            "emitter": [{
                "name": "root",
                "rate": 48,
                "origin": "10 20 0",
                "speedmin": 2,
                "speedmax": 5
            }],
            "renderer": [{
                "name": "spritetrail",
                "length": 8,
                "maxlength": 32,
                "subdivision": 4
            }],
            "controlpoint": [{
                "id": 0,
                "offset": "1 2 0"
            }]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/simple.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert_eq!(
            runtime.system.renderers[0].family,
            SceneParticleRendererFamily::SpriteTrail
        );
        assert_eq!(
            runtime.system.emitters[0].schedule_mode,
            SceneParticleScheduleMode::Autonomous
        );
        assert_eq!(runtime.system.max_count, Some(128));
        assert_eq!(
            runtime.system.control_points[0].offset,
            Some([1.0, 2.0, 0.0])
        );
    }

    #[test]
    fn preserves_static_follow_and_death_children_without_mapping_to_adapter() {
        let particle = json!({
            "emitter": [{"name": "root", "rate": 16}],
            "renderer": [{"name": "rope"}],
            "children": [
                {"name": "attached", "type": "static", "origin": "1 0 0"},
                {"name": "follow", "type": "eventfollow", "scale": "0.5 0.5 1"},
                {"name": "death", "type": "eventdeath", "probability": 0.4}
            ]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/children.json".to_string(),
            &particle,
        );

        assert!(!runtime.adapter.supported);
        assert_eq!(runtime.system.children.len(), 3);
        assert_eq!(
            runtime.system.children[0].child_type,
            SceneParticleChildKind::Static
        );
        assert_eq!(
            runtime.system.children[1].child_type,
            SceneParticleChildKind::EventFollow
        );
        assert_eq!(
            runtime.system.children[2].child_type,
            SceneParticleChildKind::EventDeath
        );
        assert!(runtime
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "particle-child-hierarchy-deferred"));
    }

    #[test]
    fn rejects_complex_operator_families_for_the_narrow_adapter() {
        let particle = json!({
            "emitter": [{"name": "root", "rate": 16}],
            "renderer": [{"name": "ropetrail"}],
            "operator": [{"name": "turbulence force"}]
        });

        let runtime = build_scene_particle_runtime(
            9,
            "Complex".to_string(),
            "particles/complex.json".to_string(),
            Some(&particle),
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            None,
            SceneParticleInstanceOverride::default(),
        );

        assert!(!runtime.adapter.supported);
        assert!(runtime
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "particle-stage-unsupported"));
    }

    #[test]
    fn numeric_control_point_flags_mark_supported_trails_as_input_driven() {
        let particle = json!({
            "emitter": [{"name": "root", "rate": 32}],
            "renderer": [{"name": "rope"}],
            "controlpoint": [
                {"id": 0, "flags": 1},
                {"id": 1, "flags": 0}
            ],
            "initializer": [{"name": "lifetimerandom"}],
            "operator": [{"name": "movement"}]
        });

        let runtime = build_scene_particle_runtime(
            12,
            "Input Trail".to_string(),
            "particles/input-trail.json".to_string(),
            Some(&particle),
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            None,
            SceneParticleInstanceOverride::default(),
        );

        assert!(runtime.adapter.supported);
        assert!(scene_particle_runtime_uses_input_control_points(&runtime));
        assert!(runtime.system.control_points[0].lock_to_pointer);
        assert_eq!(
            runtime.adapter.schedule_mode,
            SceneParticleScheduleMode::InputDriven
        );
    }

    #[test]
    fn ordinary_sprite_particles_are_first_class_without_trail_draw_kind() {
        let particle = json!({
            "maxcount": 64,
            "material": "materials/genericparticle.json",
            "emitter": [{
                "name": "sphererandom",
                "rate": 20,
                "origin": "4 8 0",
                "distancemin": 2,
                "distancemax": 16,
                "directions": ["0 1 0"],
                "speedmin": 3,
                "speedmax": 9
            }],
            "renderer": [{"name": "sprite"}],
            "initializer": [
                {"name": "lifetimerandom", "min": 0.4, "max": 1.2},
                {"name": "sizerandom", "min": 12, "max": 24},
                {"name": "colorrandom", "min": "255 160 80", "max": "255 240 180"}
            ],
            "operator": [
                {"name": "movement"},
                {"name": "alphafade"},
                {"name": "turbulentvelocityrandom", "strength": 4},
                {"name": "angularvelocityrandom", "min": -30, "max": 30}
            ],
            "children": [
                {"name": "particles/static.json", "type": "static"},
                {"name": "particles/follow.json", "type": "eventfollow"},
                {"name": "particles/death.json", "type": "eventdeath"}
            ]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/sprite.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert_eq!(runtime.adapter.draw_kind, None);
        assert_eq!(
            runtime.system.renderers[0].family,
            SceneParticleRendererFamily::Sprite
        );
        assert_eq!(runtime.system.initializers.len(), 3);
        assert_eq!(
            runtime.system.initializers[1]
                .fields
                .get("min")
                .and_then(|value| value.as_f64()),
            Some(12.0)
        );
        assert_eq!(runtime.system.children.len(), 3);
    }

    #[test]
    fn sprite_runtime_accepts_position_oscillation_without_admitting_alpha_oscillation() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{"name": "sphererandom", "rate": 20}],
            "renderer": [{"name": "sprite"}],
            "operator": [
                {
                    "name": "oscillateposition",
                    "frequencymin": 0.8,
                    "frequencymax": 1.0,
                    "phasemin": 0.0,
                    "phasemax": 1.0,
                    "scalemin": 20,
                    "scalemax": 35,
                    "mask": "1 0.5 0"
                }
            ]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/oscillating-sprite.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert!(runtime.diagnostics.is_empty());

        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{"name": "sphererandom", "rate": 20}],
            "renderer": [{"name": "sprite"}],
            "operator": [{"name": "oscillatealpha"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/alpha-oscillating-sprite.json".to_string(),
            &particle,
        );

        assert!(
            runtime.adapter.supported,
            "oscillatealpha should be accepted in the sprite runtime"
        );
        assert!(
            !runtime
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "particle-stage-unsupported"),
            "oscillatealpha should not produce unsupported stage diagnostic"
        );
    }

    #[test]
    fn sprite_child_without_authored_type_defaults_to_static_child_system() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{
                "name": "sphererandom",
                "rate": 5,
                "directions": "1 0.1 1"
            }],
            "renderer": [{"name": "sprite"}],
            "children": [{"name": "particles/secondary.json"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/sprite.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert_eq!(
            runtime.system.children[0].child_type,
            SceneParticleChildKind::Static
        );
        assert_eq!(runtime.system.emitters[0].directions, vec![[1.0, 0.1, 1.0]]);
    }

    #[test]
    fn sprite_runtime_rejects_eventspawn_and_complex_stage_families() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{"name": "sphererandom", "rate": 20}],
            "renderer": [{"name": "sprite"}],
            "operator": [{"name": "vortex"}],
            "children": [{"name": "particles/spawn.json", "type": "eventspawn"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/complex-sprite.json".to_string(),
            &particle,
        );

        assert!(!runtime.adapter.supported);
        assert!(runtime
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "particle-child-unsupported"));
        assert!(runtime
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "particle-stage-unsupported"));
    }

    #[test]
    fn trail_adapter_produces_semi_adapted_diagnostics_for_unconsumed_renderer_fields() {
        let particle = json!({
            "emitter": [{"name": "root", "rate": 16}],
            "renderer": [{
                "name": "rope",
                "minlength": 1.5,
                "axis": "x",
                "orientation": "billboard",
                "segments": 32,
                "uvsmoothing": 0.5
            }]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/renderer-fields.json".to_string(),
            &particle,
        );

        assert!(
            runtime.adapter.supported,
            "adapter should stay supported with semi-adapted diagnostics"
        );
        let codes: Vec<&str> = runtime
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            codes.contains(&"particle-renderer-field-semi-adapted"),
            "should produce semi-adapted diagnostics for minlength/axis/orientation/segments/uvsmoothing"
        );
        let all_semi_adapted = runtime
            .diagnostics
            .iter()
            .all(|d| d.diagnostic_kind == crate::models::SceneParticleDiagnosticKind::SemiAdapted);
        assert!(
            all_semi_adapted,
            "renderer field diagnostics should be SemiAdapted kind"
        );
    }

    #[test]
    fn clean_renderer_without_unconsumed_fields_produces_no_semi_adapted_diagnostics() {
        let particle = json!({
            "emitter": [{"name": "root", "rate": 16}],
            "renderer": [{"name": "rope", "length": 8, "maxlength": 32, "subdivision": 4}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/clean-renderer.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        let has_semi = runtime
            .diagnostics
            .iter()
            .any(|d| d.diagnostic_kind == crate::models::SceneParticleDiagnosticKind::SemiAdapted);
        assert!(
            !has_semi,
            "clean renderer should not produce semi-adapted diagnostics"
        );
    }

    #[test]
    fn sprite_runtime_produces_semi_adapted_diagnostic_for_unconsumed_emitter_controlpoint() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{
                "name": "sphererandom",
                "rate": 20,
                "controlpoint": 0
            }],
            "renderer": [{"name": "sprite"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/sprite-controlpoint.json".to_string(),
            &particle,
        );

        assert!(
            runtime.adapter.supported,
            "sprite adapter should stay supported"
        );
        assert!(runtime.diagnostics.iter().any(|d| {
            d.code == "particle-emitter-field-semi-adapted"
                && d.diagnostic_kind == crate::models::SceneParticleDiagnosticKind::SemiAdapted
        }));
    }

    #[test]
    fn sprite_runtime_allows_sequence_operator_without_blocking_adapter() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{"name": "sphererandom", "rate": 20}],
            "renderer": [{"name": "sprite"}],
            "operator": [{"name": "sequence"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/sprite-sequence.json".to_string(),
            &particle,
        );

        assert!(
            runtime.adapter.supported,
            "sprite adapter should remain supported when sequence is authored"
        );
        assert!(
            !runtime
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "particle-stage-unsupported"),
            "sequence should no longer block the sprite adapter"
        );
    }

    #[test]
    fn spritetrail_has_no_trail_draw_kind_and_is_sprite_family() {
        let particle = json!({
            "emitter": [{"name": "root", "rate": 24}],
            "renderer": [{"name": "spritetrail", "length": 12}],
            "material": "materials/genericparticle.json"
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/spritetrail.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert!(
            runtime.adapter.draw_kind.is_none(),
            "SpriteTrail should not produce a narrow trail draw_kind"
        );
    }

    #[test]
    fn colorchange_produces_semi_adapted_diagnostic_not_blocking() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{"name": "sphererandom", "rate": 20}],
            "renderer": [{"name": "sprite"}],
            "operator": [{"name": "colorchange"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/colorchange.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert!(
            !runtime
                .diagnostics
                .iter()
                .any(|d| d.code == "particle-stage-unsupported"),
            "colorchange should not block the sprite adapter"
        );
        assert!(runtime
            .diagnostics
            .iter()
            .any(|d| d.code == "particle-stage-semi-adapted"
                && d.diagnostic_kind == crate::models::SceneParticleDiagnosticKind::SemiAdapted));
    }

    #[test]
    fn controlpointattract_produces_semi_adapted_diagnostic_not_blocking() {
        let particle = json!({
            "material": "materials/genericparticle.json",
            "emitter": [{"name": "sphererandom", "rate": 20}],
            "renderer": [{"name": "sprite"}],
            "operator": [{"name": "controlpointattract"}]
        });

        let runtime = build_authored_particle_runtime_for_resource(
            "particles/controlpointattract.json".to_string(),
            &particle,
        );

        assert!(runtime.adapter.supported);
        assert!(
            !runtime
                .diagnostics
                .iter()
                .any(|d| d.code == "particle-stage-unsupported"),
            "controlpointattract should not block the sprite adapter"
        );
        assert!(runtime
            .diagnostics
            .iter()
            .any(|d| d.code == "particle-stage-semi-adapted"
                && d.diagnostic_kind == crate::models::SceneParticleDiagnosticKind::SemiAdapted));
    }
}
