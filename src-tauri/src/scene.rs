use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use image::{DynamicImage, Rgba, RgbaImage};
use serde_json::Value;

use crate::{
    models::{
        SceneAnimationLayer, SceneAssetKind, SceneAudioLayer, SceneAudioSource, SceneAxisBindings,
        SceneBinding, SceneCamera, SceneLogicNode, SceneLogicNodeKind, SceneManifest,
        SceneMaterialPass, SceneNodeState, SceneParallax, SceneParticleInstanceOverride,
        SceneParticleKind, SceneParticleLayer, SceneParticleRuntime, SceneRenderNode,
        SceneRenderNodeKind, SceneSoundTrack, SceneTextBehavior, SceneTextLayer, SceneVisualEffect,
        SceneVisualEffectPass, SceneVisualLayer,
    },
    services::{
        asset_resolver::AssetResolver,
        scene_particle_runtime_service::{
            build_scene_particle_runtime, scene_particle_control_point_override,
        },
    },
    system_texture::resolve_system_texture,
    tex::{extract_tex_asset, ExtractedTextureAsset},
};

#[derive(Debug, Clone, Default)]
struct MaterialInfo {
    blend_mode: Option<String>,
    shader: Option<String>,
    texture_names: Vec<String>,
    system_texture_key: Option<String>,
}

pub fn parse_scene_manifest(
    scene_json_path: &Path,
    extracted_root: &Path,
    property_values: &BTreeMap<String, Value>,
) -> Result<SceneManifest> {
    let raw = fs::read_to_string(scene_json_path)
        .with_context(|| format!("Unable to read {}", scene_json_path.display()))?;
    let scene_json: Value = serde_json::from_str(&raw)
        .with_context(|| format!("Unable to parse {}", scene_json_path.display()))?;

    let general = scene_json.get("general");
    let canvas_width = general
        .and_then(|value| value.get("orthogonalprojection"))
        .and_then(|value| value.get("width"))
        .and_then(as_f64);
    let canvas_height = general
        .and_then(|value| value.get("orthogonalprojection"))
        .and_then(|value| value.get("height"))
        .and_then(as_f64);

    let scene_size = [
        canvas_width.unwrap_or(3840.0),
        canvas_height.unwrap_or(2160.0),
    ];
    let decoded_root =
        AssetResolver::for_managed_root(extracted_root.parent().unwrap_or(extracted_root))
            .decoded_root()
            .to_path_buf();

    let objects = scene_json
        .get("objects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut texture_cache: BTreeMap<PathBuf, Option<ExtractedTextureAsset>> = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut visual_layers = Vec::new();
    let mut text_layers = Vec::new();
    let mut audio_layers = Vec::new();
    let mut sound_tracks = Vec::new();
    let mut particle_layers = Vec::new();
    let mut particle_runtimes = Vec::new();

    for object in &objects {
        if let Some(node) = parse_scene_node(object, scene_size, property_values) {
            nodes.push(node);
        }

        if let Some(text_layer) =
            parse_text_layer(object, extracted_root, scene_size, property_values)
        {
            text_layers.push(text_layer);
        }

        if let Some(audio_layer) = parse_audio_layer(object, scene_size, property_values) {
            audio_layers.push(audio_layer);
        }

        if let Some(sound_track) = parse_sound_track(object, extracted_root, property_values) {
            sound_tracks.push(sound_track);
        }

        if let Some(particle_source) =
            parse_particle_source(object, extracted_root, scene_size, property_values)
        {
            if let Some(particle_layer) = particle_source.layer {
                particle_layers.push(particle_layer);
            }
            particle_runtimes.push(particle_source.runtime);
        }

        if let Some(visual_layer) = parse_visual_layer(
            object,
            extracted_root,
            &decoded_root,
            scene_size,
            property_values,
            &mut texture_cache,
        ) {
            visual_layers.push(visual_layer);
        }
    }

    let primary_visual = visual_layers.iter().cloned().max_by(|left, right| {
        visual_area(left)
            .partial_cmp(&visual_area(right))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let camera = parse_camera(scene_json.get("camera"), general, property_values);
    let logic_graph = build_logic_graph(
        &nodes,
        &visual_layers,
        &text_layers,
        &audio_layers,
        &particle_layers,
        &sound_tracks,
    );
    let render_graph = build_render_graph(
        &visual_layers,
        &text_layers,
        &audio_layers,
        &particle_layers,
        &sound_tracks,
    );
    let material_passes = build_material_passes(&visual_layers);
    let audio_sources = build_audio_sources(&audio_layers, &sound_tracks);

    Ok(SceneManifest {
        canvas_width,
        canvas_height,
        clear_color: general
            .and_then(|value| value.get("clearcolor"))
            .and_then(as_string),
        camera,
        parallax: SceneParallax {
            enabled: general
                .and_then(|value| value.get("cameraparallax"))
                .and_then(as_bool)
                .unwrap_or(false),
            amount: general
                .and_then(|value| value.get("cameraparallaxamount"))
                .and_then(as_f64),
            delay: general
                .and_then(|value| value.get("cameraparallaxdelay"))
                .and_then(as_f64),
        },
        nodes,
        primary_visual,
        visual_layers,
        text_layers,
        audio_layers,
        sound_tracks,
        particle_layers,
        particle_runtimes,
        logic_graph,
        render_graph,
        material_passes,
        audio_sources,
        object_count: objects.len(),
    })
}

fn as_value(value: &Value) -> &Value {
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
}

fn as_string(value: &Value) -> Option<String> {
    let resolved = as_value(value);
    match resolved {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn as_f64(value: &Value) -> Option<f64> {
    let resolved = as_value(value);
    resolved
        .as_f64()
        .or_else(|| resolved.as_i64().map(|number| number as f64))
        .or_else(|| resolved.as_u64().map(|number| number as f64))
        .or_else(|| resolved.as_str().and_then(|text| text.parse::<f64>().ok()))
}

fn as_bool(value: &Value) -> Option<bool> {
    let resolved = as_value(value);
    resolved
        .as_bool()
        .or_else(|| resolved.as_i64().map(|number| number != 0))
        .or_else(|| {
            resolved
                .as_str()
                .and_then(|text| match text.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "on" => Some(true),
                    "false" | "0" | "off" => Some(false),
                    _ => None,
                })
        })
}

fn parse_vector2(value: Option<&Value>) -> Option<[f64; 2]> {
    let text = value.and_then(as_string)?;
    let numbers = text
        .split(|char: char| char.is_whitespace() || char == ',')
        .filter_map(|segment| {
            if segment.is_empty() {
                None
            } else {
                segment.parse::<f64>().ok()
            }
        })
        .collect::<Vec<_>>();
    if numbers.len() < 2 {
        return None;
    }
    Some([numbers[0], numbers[1]])
}

fn parse_dependencies(value: Option<&Value>) -> Vec<u32> {
    value
        .and_then(Value::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .filter_map(|dependency| dependency.as_u64().map(|value| value as u32))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_vector3(value: Option<&Value>) -> Option<[f64; 3]> {
    let text = value.and_then(as_string)?;
    let numbers = text
        .split(|char: char| char.is_whitespace() || char == ',')
        .filter_map(|segment| {
            if segment.is_empty() {
                None
            } else {
                segment.parse::<f64>().ok()
            }
        })
        .collect::<Vec<_>>();
    if numbers.len() < 2 {
        return None;
    }
    Some([
        numbers[0],
        numbers[1],
        numbers.get(2).copied().unwrap_or(0.0),
    ])
}

fn normalize_text_font_reference(value: Option<&Value>) -> Option<String> {
    value
        .and_then(as_string)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn text_font_reference_looks_like_path(value: &str) -> bool {
    if value.contains('/') || value.contains('\\') {
        return true;
    }

    Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc" | "woff" | "woff2"
            )
        })
        .unwrap_or(false)
}

fn legacy_text_font_path(extracted_root: &Path, font_reference: Option<&str>) -> Option<String> {
    let font_reference = font_reference?.trim();
    if font_reference.is_empty() || !text_font_reference_looks_like_path(font_reference) {
        return None;
    }

    Some(extracted_root.join(font_reference).display().to_string())
}

fn parse_script_refresh_interval_millis(
    script_properties: Option<&serde_json::Map<String, Value>>,
) -> Option<u64> {
    let raw_value = script_properties.and_then(|script_properties| {
        script_properties
            .get("refreshInterval")
            .or_else(|| script_properties.get("refreshinterval"))
    })?;
    let interval = as_f64(raw_value)?.round();
    (interval.is_finite() && interval > 0.0).then_some(interval as u64)
}

fn parse_binding(value: Option<&Value>) -> Option<SceneBinding> {
    let value = value?.as_object()?;
    let user = value.get("user")?;
    if let Some(property_key) = user.as_str() {
        return Some(SceneBinding {
            property_key: property_key.to_string(),
            condition: None,
        });
    }

    let object = user.as_object()?;
    Some(SceneBinding {
        property_key: object.get("name")?.as_str()?.to_string(),
        condition: object
            .get("condition")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn direct_user_property_key(value: Option<&Value>) -> Option<String> {
    value?
        .as_object()?
        .get("user")?
        .as_str()
        .map(ToString::to_string)
}

fn resolve_string_with_binding(
    value: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> (Option<String>, Option<String>) {
    let binding = direct_user_property_key(value);
    let resolved = binding
        .as_deref()
        .and_then(|key| property_values.get(key))
        .and_then(as_string)
        .or_else(|| value.and_then(as_string));
    (resolved, binding)
}

fn resolve_f64_with_binding(
    value: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> (Option<f64>, Option<String>) {
    let binding = direct_user_property_key(value);
    let resolved = binding
        .as_deref()
        .and_then(|key| property_values.get(key))
        .and_then(as_f64)
        .or_else(|| value.and_then(as_f64));
    (resolved, binding)
}

fn resolve_bool_with_binding(
    value: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> (Option<bool>, Option<String>) {
    let binding = direct_user_property_key(value);
    let resolved = binding
        .as_deref()
        .and_then(|key| property_values.get(key))
        .and_then(as_bool)
        .or_else(|| value.and_then(as_bool));
    (resolved, binding)
}

fn resolve_vector3_with_binding(
    value: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> (Option<[f64; 3]>, Option<String>) {
    let binding = direct_user_property_key(value);
    let resolved = binding
        .as_deref()
        .and_then(|key| property_values.get(key))
        .and_then(|property| {
            parse_vector3(Some(property))
                .or_else(|| as_f64(property).map(|scalar| [scalar, scalar, scalar]))
        })
        .or_else(|| parse_vector3(value));
    (resolved, binding)
}

fn resolve_vector2_with_binding(
    value: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> (Option<[f64; 2]>, Option<String>) {
    let binding = direct_user_property_key(value);
    let resolved = binding
        .as_deref()
        .and_then(|key| property_values.get(key))
        .and_then(|property| {
            parse_vector2(Some(property))
                .or_else(|| as_f64(property).map(|scalar| [scalar, scalar]))
        })
        .or_else(|| parse_vector2(value));
    (resolved, binding)
}

fn parse_position_bindings(value: Option<&Value>) -> Option<SceneAxisBindings> {
    let object = value?.as_object()?;
    let script_properties = object.get("scriptproperties")?.as_object()?;
    let x = script_properties
        .get("x")
        .and_then(|value| value.get("user"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let y = script_properties
        .get("y")
        .and_then(|value| value.get("user"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    if x.is_none() && y.is_none() {
        None
    } else {
        Some(SceneAxisBindings { x, y })
    }
}

fn resolve_origin_value(
    value: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
    scene_size: [f64; 2],
) -> Option<[f64; 3]> {
    let direct_value = value.and_then(|value| parse_vector3(Some(value)));
    let object = match value?.as_object() {
        Some(object) => object,
        None => return direct_value,
    };
    let current_value = object
        .get("value")
        .and_then(|value| parse_vector3(Some(value)))
        .or(direct_value)
        .unwrap_or([0.0, 0.0, 0.0]);
    let script_properties = match object.get("scriptproperties").and_then(Value::as_object) {
        Some(script_properties) => script_properties,
        None => return Some(current_value),
    };
    let axis_value = |axis: &str, extent: f64| -> Option<f64> {
        let script_property = script_properties.get(axis)?;
        if let Some(binding) = script_property.get("user").and_then(Value::as_str) {
            if let Some(current) = property_values.get(binding).and_then(as_f64) {
                return Some(current * extent);
            }
        }
        script_property
            .get("value")
            .and_then(as_f64)
            .map(|current| current * extent)
    };

    let x = axis_value("x", scene_size[0]).unwrap_or(current_value[0]);
    let y = axis_value("y", scene_size[1]).unwrap_or(current_value[1]);
    let z = script_properties
        .get("z")
        .and_then(|value| value.get("value"))
        .and_then(as_f64)
        .unwrap_or(current_value[2]);
    Some([x, y, z])
}

fn parse_camera(
    scene_camera: Option<&Value>,
    general: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> SceneCamera {
    let general = general.unwrap_or(&Value::Null);
    let scene_camera = scene_camera.unwrap_or(&Value::Null);
    let (zoom, zoom_binding) = resolve_f64_with_binding(general.get("zoom"), property_values);
    let (camera_shake, camera_shake_binding) =
        resolve_bool_with_binding(general.get("camerashake"), property_values);
    let (parallax_mouse_influence, parallax_mouse_influence_binding) =
        resolve_f64_with_binding(general.get("cameraparallaxmouseinfluence"), property_values);

    SceneCamera {
        zoom: zoom.unwrap_or(1.0),
        center: parse_vector3(scene_camera.get("center"))
            .map(|center| [center[0], center[1]])
            .unwrap_or([0.0, 0.0]),
        camera_shake: camera_shake.unwrap_or(false),
        camera_shake_amplitude: general
            .get("camerashakeamplitude")
            .and_then(as_f64)
            .unwrap_or(1.0),
        camera_shake_speed: general
            .get("camerashakespeed")
            .and_then(as_f64)
            .unwrap_or(0.9),
        parallax_mouse_influence: parallax_mouse_influence.unwrap_or(0.3),
        zoom_binding,
        camera_shake_binding,
        parallax_mouse_influence_binding,
    }
}

fn parse_scene_node(
    object: &Value,
    scene_size: [f64; 2],
    property_values: &BTreeMap<String, Value>,
) -> Option<SceneNodeState> {
    Some(SceneNodeState {
        id: object.get("id").and_then(Value::as_u64)? as u32,
        name: object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("Scene Node")
            .to_string(),
        dependencies: parse_dependencies(object.get("dependencies")),
        parent_id: object
            .get("parent")
            .and_then(Value::as_u64)
            .map(|id| id as u32),
        visible: object.get("visible").and_then(as_bool).unwrap_or(true),
        visibility_binding: parse_binding(object.get("visible")),
        position: resolve_origin_value(object.get("origin"), property_values, scene_size)
            .unwrap_or([0.0, 0.0, 0.0]),
        position_bindings: parse_position_bindings(object.get("origin")),
        scale: resolve_vector3_with_binding(object.get("scale"), property_values)
            .0
            .unwrap_or([1.0, 1.0, 1.0]),
        angles: parse_vector3(object.get("angles")),
        rotation: parse_vector3(object.get("angles")).map(|angles| angles[2]),
    })
}

fn parse_visual_layer(
    object: &Value,
    extracted_root: &Path,
    decoded_root: &Path,
    scene_size: [f64; 2],
    property_values: &BTreeMap<String, Value>,
    texture_cache: &mut BTreeMap<PathBuf, Option<ExtractedTextureAsset>>,
) -> Option<SceneVisualLayer> {
    let image_path = object.get("image").and_then(as_string)?;
    let object_id = object.get("id").and_then(Value::as_u64)? as u32;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("Scene Layer")
        .to_string();
    let parent_id = object
        .get("parent")
        .and_then(Value::as_u64)
        .map(|id| id as u32);
    let dependencies = parse_dependencies(object.get("dependencies"));
    let alignment = object
        .get("alignment")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    let origin = resolve_origin_value(object.get("origin"), property_values, scene_size)
        .unwrap_or([0.0, 0.0, 0.0]);
    let position_bindings = parse_position_bindings(object.get("origin"));
    let (_, scale_binding) = resolve_vector3_with_binding(object.get("scale"), property_values);
    let scale = resolve_vector3_with_binding(object.get("scale"), property_values)
        .0
        .unwrap_or([1.0, 1.0, 1.0]);
    let angles = parse_vector3(object.get("angles"));
    let rotation = angles.map(|resolved| resolved[2]);
    let model_path = extracted_root.join(&image_path);
    let model_json = read_json(&model_path);
    let material_path = model_json
        .as_ref()
        .and_then(|value| value.get("material"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let material_json = material_path
        .as_deref()
        .and_then(|material| read_json(&extracted_root.join(material)));
    if image_path.to_ascii_lowercase().contains("solidlayer") && has_audio_effect(object) {
        return None;
    }
    let material_info = parse_material_info(material_json.as_ref());
    let resolved_asset = resolve_visual_asset(
        object_id,
        object,
        &image_path,
        extracted_root,
        decoded_root,
        model_json.as_ref(),
        material_path.as_deref(),
        &material_info,
        texture_cache,
    );
    let intrinsic_size = resolved_asset
        .as_ref()
        .map(|asset| [asset.width as f64, asset.height as f64]);
    let mut size = parse_vector2(object.get("size"))
        .or_else(|| {
            let width = model_json
                .as_ref()
                .and_then(|value| value.get("width"))
                .and_then(as_f64)?;
            let height = model_json
                .as_ref()
                .and_then(|value| value.get("height"))
                .and_then(as_f64)?;
            Some([width, height])
        })
        .or(intrinsic_size);
    let fullscreen = model_json
        .as_ref()
        .and_then(|value| value.get("fullscreen"))
        .and_then(as_bool)
        .unwrap_or(false);
    let autosize = model_json
        .as_ref()
        .and_then(|value| value.get("autosize"))
        .and_then(as_bool)
        .unwrap_or(false);
    let solidlayer = model_json
        .as_ref()
        .and_then(|value| value.get("solidlayer"))
        .and_then(as_bool)
        .unwrap_or(false)
        || image_path.to_ascii_lowercase().contains("solidlayer");
    let passthrough = model_json
        .as_ref()
        .and_then(|value| value.get("passthrough"))
        .and_then(as_bool)
        .unwrap_or(false);
    let no_padding = model_json
        .as_ref()
        .and_then(|value| value.get("nopadding"))
        .and_then(as_bool)
        .unwrap_or(false);
    let model_width = model_json
        .as_ref()
        .and_then(|value| value.get("width"))
        .and_then(as_f64);
    let model_height = model_json
        .as_ref()
        .and_then(|value| value.get("height"))
        .and_then(as_f64);
    let puppet_path = model_json
        .as_ref()
        .and_then(|value| value.get("puppet"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let animation_layers = parse_animation_layers(object.get("animationlayers"), property_values);
    let (parallax_depth, parallax_depth_binding) =
        resolve_vector2_with_binding(object.get("parallaxDepth"), property_values);
    let (opacity, opacity_binding) = resolve_f64_with_binding(object.get("alpha"), property_values);
    let (color, color_binding) = resolve_string_with_binding(object.get("color"), property_values);
    let (brightness, brightness_binding) =
        resolve_f64_with_binding(object.get("brightness"), property_values);
    let color_blend_mode = object.get("colorBlendMode").and_then(Value::as_i64);
    let effect_instances = parse_effect_instances(object.get("effects"));
    let mut position = origin;
    if fullscreen {
        size = Some(scene_size);
        position = [scene_size[0] / 2.0, scene_size[1] / 2.0, origin[2]];
    } else if (solidlayer || autosize) && size.is_none() {
        size = intrinsic_size;
    }
    if solidlayer && size.unwrap_or([0.0, 0.0]) == [0.0, 0.0] {
        size = Some(scene_size);
    }

    let render_bounds = size.map(|resolved_size| {
        compute_render_bounds(
            position,
            resolved_size,
            scale,
            alignment.as_deref(),
            scene_size,
        )
    });

    Some(SceneVisualLayer {
        id: object_id,
        name,
        dependencies,
        parent_id,
        alignment,
        visible: object.get("visible").and_then(as_bool).unwrap_or(true),
        visibility_binding: parse_binding(object.get("visible")),
        position,
        position_bindings,
        scale,
        scale_binding,
        angles,
        size,
        intrinsic_size,
        parallax_depth,
        parallax_depth_binding,
        rotation,
        opacity,
        opacity_binding,
        color,
        color_binding,
        brightness,
        brightness_binding,
        color_blend_mode,
        render_bounds,
        fullscreen,
        autosize,
        solid_layer: solidlayer,
        passthrough,
        no_padding,
        model_width,
        model_height,
        puppet_path,
        animation_layers,
        effect_instances,
        model_path: Some(image_path),
        material_path,
        shader_path: material_info.shader.clone(),
        texture_names: material_info.texture_names.clone(),
        asset_kind: resolved_asset
            .as_ref()
            .map(|asset| asset.kind.clone())
            .unwrap_or(SceneAssetKind::Unsupported),
        asset_path: resolved_asset
            .as_ref()
            .map(|asset| asset.output_path.display().to_string()),
        system_texture_key: material_info.system_texture_key,
        blend_mode: material_info.blend_mode,
    })
}

fn has_audio_effect(object: &Value) -> bool {
    object
        .get("effects")
        .and_then(Value::as_array)
        .map(|effects| {
            effects.iter().any(|effect| {
                effect
                    .get("file")
                    .and_then(Value::as_str)
                    .map(|path| path.to_ascii_lowercase().contains("audio"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn parse_text_layer(
    object: &Value,
    extracted_root: &Path,
    scene_size: [f64; 2],
    property_values: &BTreeMap<String, Value>,
) -> Option<SceneTextLayer> {
    let text = object.get("text");
    if text.is_none() && object.get("font").is_none() && object.get("pointsize").is_none() {
        return None;
    }
    let object_id = object.get("id").and_then(Value::as_u64)? as u32;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Text Layer")
        .to_string();
    let (content, text_binding) = resolve_string_with_binding(text, property_values);
    let content = content.unwrap_or_default();
    let script = text
        .and_then(Value::as_object)
        .and_then(|value| value.get("script"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let script_properties = text
        .and_then(Value::as_object)
        .and_then(|value| value.get("scriptproperties"))
        .and_then(Value::as_object);
    let font_reference = normalize_text_font_reference(object.get("font"));
    let behavior = detect_text_behavior(&name, &content, script);
    let (color, color_binding) = resolve_string_with_binding(object.get("color"), property_values);
    let (alpha, alpha_binding) = resolve_f64_with_binding(object.get("alpha"), property_values);
    let (point_size, point_size_binding) =
        resolve_f64_with_binding(object.get("pointsize"), property_values);
    let position = resolve_origin_value(object.get("origin"), property_values, scene_size)
        .unwrap_or([0.0, 0.0, 0.0]);
    let (resolved_scale, scale_binding) =
        resolve_vector3_with_binding(object.get("scale"), property_values);
    let scale = resolved_scale.unwrap_or([1.0, 1.0, 1.0]);
    let angles = parse_vector3(object.get("angles"));
    let rotation = angles.map(|resolved| resolved[2]);
    let explicit_size = parse_vector2(object.get("size"));
    let estimated_size = estimate_text_layer_size(
        &content,
        behavior.clone(),
        point_size.unwrap_or(84.0),
        script_properties,
        script,
    );
    let padding = object.get("padding").and_then(as_f64);
    let block_align = object.get("blockalign").and_then(as_bool);
    let max_width = object.get("maxwidth").and_then(as_f64);
    let size = normalize_text_layout_size(
        explicit_size,
        estimated_size,
        max_width,
        text_layout_padding_for_block(padding, block_align),
    );
    let alignment = object
        .get("alignment")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    let anchor = object
        .get("anchor")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    let horizontal_align = object
        .get("horizontalalign")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    let vertical_align = object
        .get("verticalalign")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    let bounds_alignment = text_bounds_alignment(
        alignment.as_deref(),
        anchor.as_deref(),
        horizontal_align.as_deref(),
        vertical_align.as_deref(),
    );
    let render_bounds = size.map(|resolved_size| {
        compute_render_bounds(
            position,
            resolved_size,
            scale,
            bounds_alignment.as_deref(),
            scene_size,
        )
    });

    Some(SceneTextLayer {
        id: object_id,
        name,
        dependencies: parse_dependencies(object.get("dependencies")),
        parent_id: object
            .get("parent")
            .and_then(Value::as_u64)
            .map(|id| id as u32),
        alignment,
        anchor,
        horizontal_align,
        vertical_align,
        content,
        behavior,
        delimiter: script_properties
            .and_then(|properties| {
                properties
                    .get("delimiter")
                    .or_else(|| properties.get("addDelimiter"))
            })
            .and_then(as_string),
        month_format: script_properties
            .and_then(|properties| properties.get("monthFormat"))
            .and_then(as_string),
        day_format: script_properties
            .and_then(|properties| properties.get("dayFormat"))
            .and_then(as_string),
        show_day: script_properties
            .and_then(|properties| properties.get("showDay"))
            .and_then(as_bool),
        align_vertical: script_properties
            .and_then(|properties| properties.get("alignVertical"))
            .and_then(as_bool),
        use_delimiter: script_properties
            .and_then(|properties| properties.get("useDelimiter"))
            .and_then(as_bool),
        show_seconds: script_properties
            .and_then(|properties| properties.get("showSeconds"))
            .and_then(as_bool),
        use_24h_format: script_properties
            .and_then(|properties| properties.get("use24hFormat"))
            .and_then(as_bool),
        visible: object.get("visible").and_then(as_bool).unwrap_or(true),
        visibility_binding: parse_binding(object.get("visible")),
        text_binding: text_binding.or_else(|| {
            text.and_then(|value| parse_binding(Some(value)).map(|binding| binding.property_key))
        }),
        position,
        position_bindings: parse_position_bindings(object.get("origin")),
        scale,
        scale_binding,
        angles,
        rotation,
        size,
        render_bounds,
        parallax_depth: parse_vector2(object.get("parallaxDepth")),
        color,
        color_binding,
        alpha,
        alpha_binding,
        point_size,
        point_size_binding,
        font_reference: font_reference.clone(),
        font_path: legacy_text_font_path(extracted_root, font_reference.as_deref()),
        effect_paths: object
            .get("effects")
            .and_then(Value::as_array)
            .map(|effects| {
                effects
                    .iter()
                    .filter_map(|effect| effect.get("file").and_then(Value::as_str))
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        script_text: (!script.trim().is_empty()).then(|| script.to_string()),
        script_refresh_interval_millis: parse_script_refresh_interval_millis(script_properties),
        padding,
        max_rows: object
            .get("maxrows")
            .and_then(as_f64)
            .map(|value| value.max(1.0) as u32),
        max_width,
        limit_width: object.get("limitwidth").and_then(as_bool),
        limit_use_ellipsis: object.get("limituseellipsis").and_then(as_bool),
        block_align,
    })
}

fn parse_audio_layer(
    object: &Value,
    scene_size: [f64; 2],
    property_values: &BTreeMap<String, Value>,
) -> Option<SceneAudioLayer> {
    let effects = object.get("effects")?.as_array()?;
    let audio_effect = effects.iter().find(|effect| {
        effect
            .get("file")
            .and_then(Value::as_str)
            .map(|path| path.to_ascii_lowercase().contains("audio"))
            .unwrap_or(false)
    })?;

    let pass = audio_effect
        .get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first());
    let constants = pass.and_then(|value| value.get("constantshadervalues"));
    let position = resolve_origin_value(object.get("origin"), property_values, scene_size)
        .unwrap_or([0.0, 0.0, 0.0]);
    let (resolved_scale, scale_binding) =
        resolve_vector3_with_binding(object.get("scale"), property_values);
    let scale = resolved_scale.unwrap_or([1.0, 1.0, 1.0]);
    let angles = parse_vector3(object.get("angles"));
    let rotation = angles.map(|resolved| resolved[2]);
    let size = parse_vector2(object.get("size")).or(Some([320.0, 132.0]));
    let render_bounds = size.map(|resolved_size| {
        compute_render_bounds(
            position,
            resolved_size,
            scale,
            object.get("alignment").and_then(Value::as_str),
            scene_size,
        )
    });

    Some(SceneAudioLayer {
        id: object.get("id").and_then(Value::as_u64)? as u32,
        name: object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("Audio Layer")
            .to_string(),
        dependencies: parse_dependencies(object.get("dependencies")),
        parent_id: object
            .get("parent")
            .and_then(Value::as_u64)
            .map(|id| id as u32),
        alignment: object
            .get("alignment")
            .and_then(Value::as_str)
            .map(|value| value.to_ascii_lowercase()),
        visible: object.get("visible").and_then(as_bool).unwrap_or(true),
        visibility_binding: parse_binding(object.get("visible")),
        position,
        position_bindings: parse_position_bindings(object.get("origin")),
        scale,
        scale_binding,
        angles,
        rotation,
        size,
        render_bounds,
        angle: rotation,
        bar_count: constants
            .and_then(|value| value.get("Bar Count").or_else(|| value.get("Bar Count ")))
            .and_then(as_f64)
            .map(|value| value.round().clamp(8.0, 96.0) as usize)
            .unwrap_or(32),
        color: constants
            .and_then(|value| value.get("Bar Color"))
            .and_then(as_string),
        bar_spacing: constants
            .and_then(|value| value.get("Bar Spacing"))
            .and_then(as_f64),
        bar_bounds: constants
            .and_then(|value| value.get("Lower/Upper Bar Bounds"))
            .and_then(|value| parse_vector2(Some(value))),
        minimum_height: constants
            .and_then(|value| value.get("Minimum Height (Will be multiplied by the bar width)"))
            .and_then(as_f64),
        radius: constants
            .and_then(|value| value.get("Radius"))
            .and_then(as_f64),
        volume_factor: constants
            .and_then(|value| value.get("Volume Factor"))
            .and_then(as_f64),
        opacity: constants
            .and_then(|value| value.get("ui_editor_properties_opacity"))
            .and_then(as_f64),
    })
}

fn parse_sound_track(
    object: &Value,
    extracted_root: &Path,
    property_values: &BTreeMap<String, Value>,
) -> Option<SceneSoundTrack> {
    let sound_path = object
        .get("sound")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find_map(Value::as_str))?;
    let asset_path = extracted_root.join(sound_path);
    if !asset_path.exists() {
        return None;
    }

    let (volume, volume_binding) = resolve_f64_with_binding(object.get("volume"), property_values);

    Some(SceneSoundTrack {
        id: object.get("id").and_then(Value::as_u64)? as u32,
        name: object
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("Scene Sound")
            .to_string(),
        asset_path: asset_path.display().to_string(),
        looped: object
            .get("playbackmode")
            .and_then(Value::as_str)
            .map(|value| value.eq_ignore_ascii_case("loop"))
            .unwrap_or(false),
        volume: volume.unwrap_or(1.0).clamp(0.0, 1.0),
        volume_binding,
    })
}

struct ParsedParticleSource {
    layer: Option<SceneParticleLayer>,
    runtime: SceneParticleRuntime,
}

fn parse_particle_source(
    object: &Value,
    extracted_root: &Path,
    scene_size: [f64; 2],
    property_values: &BTreeMap<String, Value>,
) -> Option<ParsedParticleSource> {
    let particle_path = object.get("particle").and_then(Value::as_str)?;
    let particle_json_path = extracted_root.join(particle_path);
    let particle_json = read_json(&particle_json_path);
    let object_id = object.get("id").and_then(Value::as_u64)? as u32;
    let object_name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("Particle Layer")
        .to_string();
    let parent_id = object
        .get("parent")
        .and_then(Value::as_u64)
        .map(|id| id as u32);
    let position = resolve_origin_value(object.get("origin"), property_values, scene_size)
        .unwrap_or([0.0, 0.0, 0.0]);
    let position_bindings = parse_position_bindings(object.get("origin"));
    let (scale, scale_binding) = resolve_vector3_with_binding(object.get("scale"), property_values);
    let scale = scale.unwrap_or([1.0, 1.0, 1.0]);
    let angles = parse_vector3(object.get("angles"));
    let rotation = angles.map(|resolved| resolved[2]);
    let instance_override = object.get("instanceoverride");
    let runtime_instance_override =
        parse_particle_instance_override(instance_override, property_values);
    let runtime = build_scene_particle_runtime(
        object_id,
        object_name.clone(),
        particle_path.to_string(),
        particle_json.as_ref(),
        position,
        scale,
        angles,
        runtime_instance_override.clone(),
    );

    let layer = runtime
        .adapter
        .supported
        .then(|| runtime.adapter.draw_kind.clone())
        .flatten()
        .map(|kind| {
            let fallback_size = scale[2].abs().max(match &kind {
                SceneParticleKind::LineTrail => 5.0,
                SceneParticleKind::PetalTrail => 1.5,
            });
            SceneParticleLayer {
                id: object_id,
                name: object_name,
                dependencies: parse_dependencies(object.get("dependencies")),
                parent_id,
                visible: object.get("visible").and_then(as_bool).unwrap_or(true),
                visibility_binding: parse_binding(object.get("visible")),
                position,
                position_bindings,
                scale,
                scale_binding,
                angles,
                rotation,
                kind: kind.clone(),
                particle_path: particle_path.to_string(),
                color: runtime_instance_override.color.clone(),
                color_binding: runtime_instance_override.color_binding.clone(),
                size: runtime_instance_override
                    .size
                    .unwrap_or(fallback_size)
                    .max(0.1),
                size_binding: runtime_instance_override.size_binding.clone(),
                emission_rate: particle_emission_rate(&runtime, &kind),
            }
        });

    Some(ParsedParticleSource { layer, runtime })
}

fn parse_particle_instance_override(
    instance_override: Option<&Value>,
    property_values: &BTreeMap<String, Value>,
) -> SceneParticleInstanceOverride {
    let (rate, rate_binding) = resolve_f64_with_binding(
        instance_override.and_then(|value| value.get("rate")),
        property_values,
    );
    let (size, size_binding) = resolve_f64_with_binding(
        instance_override.and_then(|value| value.get("size")),
        property_values,
    );
    let (speed, speed_binding) = resolve_f64_with_binding(
        instance_override.and_then(|value| value.get("speed")),
        property_values,
    );
    let (alpha, alpha_binding) = resolve_f64_with_binding(
        instance_override.and_then(|value| value.get("alpha")),
        property_values,
    );
    let (lifetime, lifetime_binding) = resolve_f64_with_binding(
        instance_override.and_then(|value| value.get("lifetime")),
        property_values,
    );
    let (count, count_binding) = resolve_f64_with_binding(
        instance_override.and_then(|value| value.get("count")),
        property_values,
    );
    let (color, color_binding) = resolve_string_with_binding(
        instance_override
            .and_then(|value| value.get("colorn"))
            .or_else(|| instance_override.and_then(|value| value.get("color"))),
        property_values,
    );
    let control_points = instance_override
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| key.to_ascii_lowercase().starts_with("controlpoint"))
                .map(|(key, value)| {
                    let binding = direct_user_property_key(Some(value));
                    let resolved = binding
                        .as_deref()
                        .and_then(|key| property_values.get(key))
                        .and_then(|value| {
                            parse_vector3(Some(value))
                                .or_else(|| as_f64(value).map(|scalar| [scalar, 0.0, 0.0]))
                        })
                        .or_else(|| {
                            parse_vector3(Some(value))
                                .or_else(|| as_f64(value).map(|scalar| [scalar, 0.0, 0.0]))
                        });
                    scene_particle_control_point_override(key.clone(), resolved, binding)
                })
                .collect()
        })
        .unwrap_or_default();

    SceneParticleInstanceOverride {
        rate,
        rate_binding,
        size,
        size_binding,
        speed,
        speed_binding,
        alpha,
        alpha_binding,
        lifetime,
        lifetime_binding,
        count: count.map(|value| value.max(0.0)),
        count_binding,
        color,
        color_binding,
        control_points,
    }
}

fn particle_emission_rate(runtime: &SceneParticleRuntime, kind: &SceneParticleKind) -> f64 {
    runtime
        .instance_override
        .rate
        .or_else(|| {
            runtime
                .system
                .emitters
                .first()
                .and_then(|emitter| emitter.rate)
        })
        .unwrap_or(match kind {
            SceneParticleKind::LineTrail => 32.0,
            SceneParticleKind::PetalTrail => 100.0,
        })
}

fn parse_animation_layers(
    value: Option<&Value>,
    _property_values: &BTreeMap<String, Value>,
) -> Vec<SceneAnimationLayer> {
    value
        .and_then(Value::as_array)
        .map(|layers| {
            layers
                .iter()
                .filter_map(|layer| {
                    Some(SceneAnimationLayer {
                        id: layer.get("id").and_then(Value::as_i64)?,
                        rate: layer.get("rate").and_then(as_f64).unwrap_or(0.0),
                        visible: layer.get("visible").and_then(as_bool).unwrap_or(false),
                        visibility_binding: parse_binding(layer.get("visible")).or_else(|| {
                            direct_user_property_key(layer.get("visible")).map(|property_key| {
                                SceneBinding {
                                    property_key,
                                    condition: None,
                                }
                            })
                        }),
                        blend: layer.get("blend").and_then(as_string).unwrap_or_default(),
                        animation: layer
                            .get("animation")
                            .and_then(as_string)
                            .unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_effect_instances(value: Option<&Value>) -> Vec<SceneVisualEffect> {
    value
        .and_then(Value::as_array)
        .map(|effects| {
            effects
                .iter()
                .filter_map(|effect| {
                    Some(SceneVisualEffect {
                        effect_path: effect.get("file").and_then(Value::as_str)?.to_string(),
                        visible: effect.get("visible").and_then(as_bool).unwrap_or(true),
                        visibility_binding: parse_binding(effect.get("visible")),
                        passes: effect
                            .get("passes")
                            .and_then(Value::as_array)
                            .map(|passes| {
                                passes
                                    .iter()
                                    .map(|pass| SceneVisualEffectPass {
                                        constants: pass
                                            .get("constantshadervalues")
                                            .and_then(Value::as_object)
                                            .map(|constants| {
                                                constants
                                                    .iter()
                                                    .map(|(key, value)| {
                                                        (key.clone(), value.clone())
                                                    })
                                                    .collect::<BTreeMap<_, _>>()
                                            })
                                            .unwrap_or_default(),
                                        textures: pass
                                            .get("textures")
                                            .and_then(Value::as_array)
                                            .map(|textures| {
                                                textures
                                                    .iter()
                                                    .map(|texture| {
                                                        texture.as_str().map(ToString::to_string)
                                                    })
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default(),
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_material_info(material_json: Option<&Value>) -> MaterialInfo {
    let Some(pass) = material_json.and_then(first_material_pass) else {
        return MaterialInfo::default();
    };

    let texture_names = pass
        .get("textures")
        .and_then(Value::as_array)
        .map(|textures| {
            textures
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let system_texture_key = pass
        .get("usertextures")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                let object = item.as_object()?;
                if object.get("type").and_then(Value::as_str)? == "system" {
                    object
                        .get("name")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                } else {
                    None
                }
            })
        });

    MaterialInfo {
        blend_mode: pass
            .get("blending")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        shader: pass
            .get("shader")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        texture_names,
        system_texture_key,
    }
}

fn first_material_pass(value: &Value) -> Option<&Value> {
    value
        .get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .or_else(|| material_declares_inline_pass(value).then_some(value))
}

fn material_declares_inline_pass(value: &Value) -> bool {
    value
        .get("shader")
        .and_then(Value::as_str)
        .map(|shader| !shader.trim().is_empty())
        .unwrap_or(false)
        || value
            .get("textures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || value
            .get("usertextures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || value.get("blending").and_then(Value::as_str).is_some()
}

fn resolve_visual_asset(
    object_id: u32,
    object: &Value,
    image_path: &str,
    extracted_root: &Path,
    decoded_root: &Path,
    model_json: Option<&Value>,
    material_path: Option<&str>,
    material_info: &MaterialInfo,
    texture_cache: &mut BTreeMap<PathBuf, Option<ExtractedTextureAsset>>,
) -> Option<ExtractedTextureAsset> {
    if let Some(system_texture_key) = material_info.system_texture_key.as_deref() {
        let provider_root = decoded_root.join("system");
        if let Ok(system_texture) = resolve_system_texture(system_texture_key, &provider_root) {
            return Some(ExtractedTextureAsset {
                kind: SceneAssetKind::System,
                output_path: system_texture.output_path,
                width: system_texture.width,
                height: system_texture.height,
            });
        }
    }

    if is_solid_layer(image_path, model_json) {
        return render_solid_layer_asset(object_id, object, decoded_root).ok();
    }

    let _ = &material_info.shader;
    let candidates =
        resolve_texture_candidates(extracted_root, material_path, &material_info.texture_names);
    for candidate in candidates {
        if let Some(existing) = texture_cache.get(&candidate) {
            if let Some(texture) = existing {
                return Some(texture.clone());
            }
            continue;
        }

        let relative = candidate.strip_prefix(extracted_root).ok()?.to_path_buf();
        let output_path = decoded_root.join(relative).with_extension("");
        let extracted = extract_tex_asset(&candidate, &output_path).ok();
        texture_cache.insert(candidate.clone(), extracted.clone());
        if let Some(texture) = extracted {
            return Some(texture);
        }
    }

    None
}

fn is_solid_layer(image_path: &str, model_json: Option<&Value>) -> bool {
    image_path.to_ascii_lowercase().contains("solidlayer")
        || model_json
            .and_then(|value| value.get("solidlayer"))
            .and_then(as_bool)
            .unwrap_or(false)
}

fn render_solid_layer_asset(
    object_id: u32,
    object: &Value,
    decoded_root: &Path,
) -> Result<ExtractedTextureAsset> {
    let output_path = decoded_root
        .join("generated")
        .join(format!("solid-{object_id}.png"));
    if !output_path.exists() {
        let size = parse_vector2(object.get("size")).unwrap_or([256.0, 256.0]);
        let width = size[0].round().clamp(8.0, 2048.0) as u32;
        let height = size[1].round().clamp(8.0, 2048.0) as u32;
        let color = parse_color_rgba(object.get("color")).unwrap_or([255, 255, 255, 255]);
        let mut image = RgbaImage::new(width, height);
        for pixel in image.pixels_mut() {
            *pixel = Rgba(color);
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        DynamicImage::ImageRgba8(image).save(&output_path)?;
    }

    let image = image::image_dimensions(&output_path)?;
    Ok(ExtractedTextureAsset {
        kind: SceneAssetKind::Image,
        output_path,
        width: image.0,
        height: image.1,
    })
}

fn parse_color_rgba(value: Option<&Value>) -> Option<[u8; 4]> {
    let text = value.and_then(as_string)?;
    let parts = text
        .split_whitespace()
        .filter_map(|segment| segment.parse::<f32>().ok())
        .collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    Some([
        (parts[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (parts[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (parts[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    ])
}

fn detect_text_behavior(name: &str, content: &str, script: &str) -> SceneTextBehavior {
    let lower_name = name.to_ascii_lowercase();
    let lower_content = content.to_ascii_lowercase();
    let lower_script = script.to_ascii_lowercase();

    if lower_name == "date"
        || lower_name.contains("date")
        || lower_content.contains("<date>")
        || lower_content == "date"
    {
        SceneTextBehavior::Date
    } else if lower_name.contains("d a y")
        || lower_name.contains("weekday")
        || lower_content.contains("<day>")
        || lower_content == "day"
    {
        SceneTextBehavior::Weekday
    } else if lower_name.contains("clock")
        || lower_content.contains("clock")
        || lower_script.contains("showseconds")
        || (lower_script.contains("gethours") && lower_script.contains("getminutes"))
    {
        SceneTextBehavior::Clock
    } else if name.contains("帧率")
        || lower_script.contains("fps")
        || lower_script.contains("refreshinterval")
    {
        SceneTextBehavior::Fps
    } else if lower_script.contains("mediapropertieschanged")
        || lower_script.contains("event.title")
    {
        SceneTextBehavior::MediaTitle
    } else if looks_like_day_period_text_behavior(name, &lower_script) {
        SceneTextBehavior::DayPeriod
    } else if script_requires_runtime(&lower_script) {
        SceneTextBehavior::Script
    } else {
        SceneTextBehavior::Static
    }
}

fn looks_like_day_period_text_behavior(name: &str, lower_script: &str) -> bool {
    let mentions_day_period = name.contains("早中晚")
        || lower_script.contains("morning")
        || lower_script.contains("afternoon")
        || lower_script.contains("evening")
        || lower_script.contains("night")
        || lower_script.contains("before dawn")
        || lower_script.contains("at night");

    mentions_day_period
        && !lower_script.contains("thislayer.")
        && !lower_script.contains("engine.userproperties")
        && !lower_script.contains("applyuserproperties")
        && !lower_script.contains("math.random")
}

fn script_requires_runtime(lower_script: &str) -> bool {
    lower_script.contains("export function update")
        || lower_script.contains("function update")
        || lower_script.contains("export function applyuserproperties")
        || lower_script.contains("function applyuserproperties")
        || lower_script.contains("thislayer.")
        || lower_script.contains("engine.userproperties")
}

fn estimate_text_layer_size(
    content: &str,
    behavior: SceneTextBehavior,
    point_size: f64,
    script_properties: Option<&serde_json::Map<String, Value>>,
    script: &str,
) -> Option<[f64; 2]> {
    let sample = sample_text_for_behavior(content, &behavior, script_properties, Some(script));
    let safe_point_size = normalize_scene_point_size(point_size);
    let lines = sample.lines().collect::<Vec<_>>();
    let line_count = lines.len().max(1) as f64;
    let max_units = lines
        .iter()
        .map(|line| line.chars().map(glyph_width_units).sum::<f64>())
        .fold(1.0, f64::max);
    let line_height = match behavior {
        SceneTextBehavior::Clock => 0.88,
        SceneTextBehavior::Weekday => 0.84,
        _ => 0.94,
    };

    Some([
        max_units * safe_point_size + safe_point_size * 0.44,
        line_count * safe_point_size * line_height + safe_point_size * 0.34,
    ])
}

fn normalize_scene_point_size(point_size: f64) -> f64 {
    if !point_size.is_finite() {
        return 24.0;
    }
    point_size.max(6.0)
}

fn normalize_text_layout_size(
    explicit_size: Option<[f64; 2]>,
    estimated_size: Option<[f64; 2]>,
    max_width: Option<f64>,
    padding: Option<f64>,
) -> Option<[f64; 2]> {
    if let Some([width, height]) = explicit_size {
        if width > 0.0 && height > 0.0 {
            return Some([width, height]);
        }
    }

    let estimated = estimated_size?;
    let pad = padding.unwrap_or(0.0).max(0.0);
    let width_limit = max_width.filter(|value| *value > 0.0);
    let estimated_width = (estimated[0] + pad * 2.0).max(24.0);
    let estimated_height = (estimated[1] + pad * 2.0).max(24.0);
    let width = width_limit
        .map(|limit| estimated_width.min(limit))
        .unwrap_or(estimated_width);
    Some([width.max(24.0), estimated_height.max(24.0)])
}

fn text_layout_padding_for_block(padding: Option<f64>, block_align: Option<bool>) -> Option<f64> {
    if block_align == Some(false) {
        None
    } else {
        padding
    }
}

fn text_bounds_alignment(
    alignment: Option<&str>,
    anchor: Option<&str>,
    horizontal_align: Option<&str>,
    vertical_align: Option<&str>,
) -> Option<String> {
    alignment
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| {
            let anchor = anchor?.trim().to_ascii_lowercase();
            if anchor.is_empty() {
                None
            } else if anchor == "none" {
                Some(text_alignment_from_axes(horizontal_align, vertical_align))
            } else {
                Some(anchor)
            }
        })
        .or_else(|| Some(text_alignment_from_axes(horizontal_align, vertical_align)))
}

fn text_alignment_from_axes(
    horizontal_align: Option<&str>,
    vertical_align: Option<&str>,
) -> String {
    let horizontal = match horizontal_align
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "left" => "left",
        "right" => "right",
        _ => "center",
    };
    let vertical = match vertical_align
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "top" => "top",
        "bottom" => "bottom",
        _ => "center",
    };
    format!("{horizontal}-{vertical}")
}

fn sample_text_for_behavior(
    content: &str,
    behavior: &SceneTextBehavior,
    script_properties: Option<&serde_json::Map<String, Value>>,
    script: Option<&str>,
) -> String {
    let trimmed = content.trim();
    match behavior {
        SceneTextBehavior::Clock => {
            let show_seconds = script_properties
                .and_then(|properties| properties.get("showSeconds"))
                .and_then(as_bool)
                .unwrap_or(false);
            if show_seconds {
                "22:14:33".to_string()
            } else {
                "22:14".to_string()
            }
        }
        SceneTextBehavior::Date => sample_calendar_text(content, script_properties, script, false),
        SceneTextBehavior::Weekday => {
            sample_calendar_text(content, script_properties, script, true)
        }
        SceneTextBehavior::DayPeriod => {
            if trimmed.is_empty() {
                "Evening".to_string()
            } else {
                trimmed.to_string()
            }
        }
        SceneTextBehavior::Script => {
            if trimmed.is_empty() {
                "Text".to_string()
            } else {
                trimmed.to_string()
            }
        }
        SceneTextBehavior::Fps => "60 FPS".to_string(),
        SceneTextBehavior::MediaTitle => {
            if trimmed.is_empty() {
                "Song Title".to_string()
            } else {
                trimmed.to_string()
            }
        }
        SceneTextBehavior::Static => {
            if trimmed.is_empty() {
                "Text".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

fn sample_calendar_text(
    content: &str,
    script_properties: Option<&serde_json::Map<String, Value>>,
    script: Option<&str>,
    weekday_only: bool,
) -> String {
    let script = script.unwrap_or_default();
    let lower_script = script.to_ascii_lowercase();
    let month_format = script_properties
        .and_then(|properties| properties.get("monthFormat"))
        .and_then(as_string)
        .unwrap_or_else(|| "2".to_string());
    let day_format = script_properties
        .and_then(|properties| properties.get("dayFormat"))
        .and_then(as_string)
        .unwrap_or_else(|| "1".to_string());
    let show_day = script_properties
        .and_then(|properties| properties.get("showDay"))
        .and_then(as_bool)
        .unwrap_or(weekday_only);
    let align_vertical = script_properties
        .and_then(|properties| properties.get("alignVertical"))
        .and_then(as_bool)
        .unwrap_or(weekday_only);
    let use_delimiter = script_properties
        .and_then(|properties| properties.get("useDelimiter"))
        .and_then(as_bool)
        .unwrap_or(!align_vertical);
    let delimiter = script_properties
        .and_then(|properties| {
            properties
                .get("delimiter")
                .or_else(|| properties.get("addDelimiter"))
        })
        .and_then(as_string)
        .unwrap_or_else(|| "/".to_string());
    let spaced_weekday = lower_script.contains("'s u n'")
        || lower_script.contains("'m o n'")
        || lower_script.contains("'s u n d a y'")
        || lower_script.contains("'m o n d a y'");
    let vertical_tokens = align_vertical
        && (content.contains('\n')
            || !use_delimiter
            || lower_script.contains("+ newline +")
            || lower_script.contains("+ nl +")
            || lower_script.contains("delimitervalue = [\n      '\\n\\n'")
            || lower_script.contains("delimitervalue = [\n  '\\n\\n'"));

    let weekday = if day_format == "2" { "MONDAY" } else { "MON" };
    let weekday_text = if align_vertical {
        weekday
            .chars()
            .map(|character| character.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    } else if spaced_weekday {
        weekday
            .chars()
            .map(|character| character.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        weekday.to_string()
    };

    if weekday_only && show_day {
        return weekday_text;
    }

    let day_text = if vertical_tokens {
        "3\n1".to_string()
    } else {
        "31".to_string()
    };
    let month_text = match month_format.as_str() {
        "1" if vertical_tokens => "0\n3".to_string(),
        "1" => "3".to_string(),
        "2" if vertical_tokens => "M\nA\nR".to_string(),
        "2" => "MAR".to_string(),
        "3" if vertical_tokens => "M\na\nr\nc\nh".to_string(),
        "3" => "March".to_string(),
        _ => "MAR".to_string(),
    };
    let year_text = if vertical_tokens {
        "2\n0\n2\n6".to_string()
    } else {
        "2026".to_string()
    };
    let separator = if use_delimiter {
        delimiter
    } else if vertical_tokens {
        "\n\n".to_string()
    } else {
        " ".to_string()
    };
    let date_text = format!("{day_text}{separator}{month_text}{separator}{year_text}");

    if show_day {
        if align_vertical {
            format!("{weekday_text}\n\n{date_text}")
        } else {
            format!("{weekday_text} {date_text}")
        }
    } else {
        date_text
    }
}

fn glyph_width_units(character: char) -> f64 {
    if character == ' ' {
        return 0.34;
    }
    if character.is_ascii_uppercase() || character.is_ascii_digit() {
        return 0.66;
    }
    if character.is_ascii_lowercase() {
        return 0.58;
    }
    if matches!(character, ':' | '.' | '/' | '-') {
        return 0.30;
    }
    if ('\u{4e00}'..='\u{9fff}').contains(&character)
        || ('\u{3040}'..='\u{30ff}').contains(&character)
        || ('\u{ac00}'..='\u{d7af}').contains(&character)
    {
        return 1.0;
    }
    0.5
}

fn compute_render_bounds(
    position: [f64; 3],
    size: [f64; 2],
    scale: [f64; 3],
    alignment: Option<&str>,
    scene_size: [f64; 2],
) -> [f64; 4] {
    let scaled_width = size[0] * scale[0].abs().max(0.0001);
    let scaled_height = size[1] * scale[1].abs().max(0.0001);
    let normalized = alignment.unwrap_or_default();
    let anchor_x = if normalized.contains("left") {
        0.0
    } else if normalized.contains("right") {
        1.0
    } else {
        0.5
    };
    let anchor_y = if normalized.contains("bottom") {
        0.0
    } else if normalized.contains("top") {
        1.0
    } else {
        0.5
    };
    let left = position[0] - scaled_width * anchor_x;
    let bottom = position[1] - scaled_height * anchor_y;
    let top = bottom + scaled_height;

    [left, scene_size[1] - top, scaled_width, scaled_height]
}

fn resolve_texture_candidates(
    extracted_root: &Path,
    material_path: Option<&str>,
    texture_names: &[String],
) -> Vec<PathBuf> {
    let material_dir = material_path
        .map(|path| extracted_root.join(path))
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let mut candidates = Vec::new();

    for texture_name in texture_names {
        let path = Path::new(texture_name);
        let has_relative_segments = path.components().count() > 1;
        let mut local_candidates = Vec::new();

        if path.extension().is_some() {
            local_candidates.push(extracted_root.join(path));
            if has_relative_segments {
                local_candidates.push(extracted_root.join("materials").join(path));
            }
            if let Some(material_dir) = material_dir.as_ref() {
                local_candidates.push(material_dir.join(path));
            }
        } else {
            if has_relative_segments {
                local_candidates.push(extracted_root.join(path).with_extension("tex"));
                local_candidates.push(
                    extracted_root
                        .join("materials")
                        .join(path)
                        .with_extension("tex"),
                );
            }
            if let Some(material_dir) = material_dir.as_ref() {
                local_candidates.push(material_dir.join(path).with_extension("tex"));
            }
            if !has_relative_segments {
                local_candidates.push(extracted_root.join(format!("{texture_name}.tex")));
            }
        }

        for candidate in local_candidates {
            if candidate.exists() && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    candidates
}

fn read_json(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn build_logic_graph(
    nodes: &[SceneNodeState],
    visual_layers: &[SceneVisualLayer],
    text_layers: &[SceneTextLayer],
    audio_layers: &[SceneAudioLayer],
    particle_layers: &[SceneParticleLayer],
    sound_tracks: &[SceneSoundTrack],
) -> Vec<SceneLogicNode> {
    let mut graph = nodes
        .iter()
        .map(|node| SceneLogicNode {
            id: node.id,
            name: node.name.clone(),
            parent_id: node.parent_id,
            kind: SceneLogicNodeKind::Container,
            visible: node.visible,
            condition: node
                .visibility_binding
                .as_ref()
                .and_then(|binding| binding.condition.clone()),
            bindings: node
                .visibility_binding
                .as_ref()
                .map(|binding| vec![binding.property_key.clone()])
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    graph.extend(visual_layers.iter().map(|layer| {
        SceneLogicNode {
            id: layer.id,
            name: layer.name.clone(),
            parent_id: layer.parent_id,
            kind: SceneLogicNodeKind::Visual,
            visible: layer.visible,
            condition: layer
                .visibility_binding
                .as_ref()
                .and_then(|binding| binding.condition.clone()),
            bindings: [
                layer
                    .visibility_binding
                    .as_ref()
                    .map(|binding| binding.property_key.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.x.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.y.clone()),
                layer.scale_binding.clone(),
                layer.parallax_depth_binding.clone(),
                layer.opacity_binding.clone(),
                layer.color_binding.clone(),
                layer.brightness_binding.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }));
    graph.extend(text_layers.iter().map(|layer| {
        SceneLogicNode {
            id: layer.id,
            name: layer.name.clone(),
            parent_id: layer.parent_id,
            kind: SceneLogicNodeKind::Text,
            visible: layer.visible,
            condition: layer
                .visibility_binding
                .as_ref()
                .and_then(|binding| binding.condition.clone()),
            bindings: [
                layer
                    .visibility_binding
                    .as_ref()
                    .map(|binding| binding.property_key.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.x.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.y.clone()),
                layer.scale_binding.clone(),
                layer.text_binding.clone(),
                layer.color_binding.clone(),
                layer.alpha_binding.clone(),
                layer.point_size_binding.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }));
    graph.extend(audio_layers.iter().map(|layer| {
        SceneLogicNode {
            id: layer.id,
            name: layer.name.clone(),
            parent_id: layer.parent_id,
            kind: SceneLogicNodeKind::Audio,
            visible: layer.visible,
            condition: layer
                .visibility_binding
                .as_ref()
                .and_then(|binding| binding.condition.clone()),
            bindings: [
                layer
                    .visibility_binding
                    .as_ref()
                    .map(|binding| binding.property_key.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.x.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.y.clone()),
                layer.scale_binding.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }));
    graph.extend(particle_layers.iter().map(|layer| {
        SceneLogicNode {
            id: layer.id,
            name: layer.name.clone(),
            parent_id: layer.parent_id,
            kind: SceneLogicNodeKind::Particle,
            visible: layer.visible,
            condition: layer
                .visibility_binding
                .as_ref()
                .and_then(|binding| binding.condition.clone()),
            bindings: [
                layer
                    .visibility_binding
                    .as_ref()
                    .map(|binding| binding.property_key.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.x.clone()),
                layer
                    .position_bindings
                    .as_ref()
                    .and_then(|bindings| bindings.y.clone()),
                layer.scale_binding.clone(),
                layer.color_binding.clone(),
                layer.size_binding.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }));
    graph.extend(sound_tracks.iter().map(|track| SceneLogicNode {
        id: track.id,
        name: track.name.clone(),
        parent_id: None,
        kind: SceneLogicNodeKind::Sound,
        visible: true,
        condition: None,
        bindings: track.volume_binding.clone().into_iter().collect(),
    }));

    graph
}

fn build_render_graph(
    visual_layers: &[SceneVisualLayer],
    text_layers: &[SceneTextLayer],
    audio_layers: &[SceneAudioLayer],
    particle_layers: &[SceneParticleLayer],
    sound_tracks: &[SceneSoundTrack],
) -> Vec<SceneRenderNode> {
    let mut graph = visual_layers
        .iter()
        .map(|layer| SceneRenderNode {
            id: layer.id,
            name: layer.name.clone(),
            parent_id: layer.parent_id,
            kind: match layer.asset_kind {
                SceneAssetKind::Image => SceneRenderNodeKind::Sprite,
                SceneAssetKind::Video => SceneRenderNodeKind::Video,
                SceneAssetKind::System => SceneRenderNodeKind::SystemTexture,
                SceneAssetKind::Unsupported => SceneRenderNodeKind::Unsupported,
            },
            visible: layer.visible,
            asset_path: layer.asset_path.clone(),
            material_path: layer.material_path.clone(),
        })
        .collect::<Vec<_>>();

    graph.extend(text_layers.iter().map(|layer| SceneRenderNode {
        id: layer.id,
        name: layer.name.clone(),
        parent_id: layer.parent_id,
        kind: SceneRenderNodeKind::Text,
        visible: layer.visible,
        asset_path: None,
        material_path: None,
    }));
    graph.extend(audio_layers.iter().map(|layer| SceneRenderNode {
        id: layer.id,
        name: layer.name.clone(),
        parent_id: layer.parent_id,
        kind: SceneRenderNodeKind::AudioReactive,
        visible: layer.visible,
        asset_path: None,
        material_path: None,
    }));
    graph.extend(particle_layers.iter().map(|layer| SceneRenderNode {
        id: layer.id,
        name: layer.name.clone(),
        parent_id: layer.parent_id,
        kind: SceneRenderNodeKind::Particle,
        visible: layer.visible,
        asset_path: Some(layer.particle_path.clone()),
        material_path: None,
    }));
    graph.extend(sound_tracks.iter().map(|track| SceneRenderNode {
        id: track.id,
        name: track.name.clone(),
        parent_id: None,
        kind: SceneRenderNodeKind::Sound,
        visible: true,
        asset_path: Some(track.asset_path.clone()),
        material_path: None,
    }));

    graph
}

fn build_material_passes(visual_layers: &[SceneVisualLayer]) -> Vec<SceneMaterialPass> {
    visual_layers
        .iter()
        .filter(|layer| {
            layer.material_path.is_some()
                || !layer.texture_names.is_empty()
                || layer.system_texture_key.is_some()
                || layer.blend_mode.is_some()
        })
        .map(|layer| SceneMaterialPass {
            owner_id: layer.id,
            material_path: layer.material_path.clone(),
            shader_path: layer.shader_path.clone(),
            blend_mode: layer.blend_mode.clone(),
            texture_names: layer.texture_names.clone(),
            system_texture_key: layer.system_texture_key.clone(),
        })
        .collect()
}

fn build_audio_sources(
    audio_layers: &[SceneAudioLayer],
    sound_tracks: &[SceneSoundTrack],
) -> Vec<SceneAudioSource> {
    let mut sources = audio_layers
        .iter()
        .map(|layer| SceneAudioSource {
            id: layer.id,
            name: layer.name.clone(),
            source_type: "system-audio-reactive".to_string(),
            asset_path: None,
            reactive: true,
        })
        .collect::<Vec<_>>();
    sources.extend(sound_tracks.iter().map(|track| SceneAudioSource {
        id: track.id,
        name: track.name.clone(),
        source_type: "wallpaper-bgm".to_string(),
        asset_path: Some(track.asset_path.clone()),
        reactive: false,
    }));
    sources
}

fn visual_area(layer: &SceneVisualLayer) -> f64 {
    let size = layer.size.unwrap_or([0.0, 0.0]);
    size[0].abs() * size[1].abs() * layer.scale[0].abs().max(0.1) * layer.scale[1].abs().max(0.1)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use uuid::Uuid;

    use super::{
        detect_text_behavior, estimate_text_layer_size, normalize_text_layout_size,
        parse_scene_manifest, resolve_texture_candidates, sample_text_for_behavior,
        text_layout_padding_for_block,
    };
    use serde_json::json;

    #[test]
    fn resolves_prefixed_and_plain_texture_names() {
        let extracted_root = std::env::temp_dir().join(format!("scene-texture-{}", Uuid::new_v4()));
        fs::create_dir_all(extracted_root.join("materials/workshop/3449579583"))
            .expect("texture root");
        fs::write(
            extracted_root.join("materials").join("伊蕾娜 提示框.tex"),
            b"prompt",
        )
        .expect("prompt tex");
        fs::write(
            extracted_root.join("materials/workshop/3449579583/placeholder.tex"),
            b"placeholder",
        )
        .expect("placeholder tex");

        let prompt_candidates = resolve_texture_candidates(
            &extracted_root,
            Some("materials/伊蕾娜 提示框.json"),
            &[String::from("伊蕾娜 提示框")],
        );
        assert!(prompt_candidates.iter().any(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some("伊蕾娜 提示框.tex")
        }));

        let placeholder_candidates = resolve_texture_candidates(
            &extracted_root,
            Some("materials/workshop/3449579583/placeholder.json"),
            &[String::from("workshop/3449579583/placeholder")],
        );
        assert!(placeholder_candidates
            .iter()
            .any(|path| path.ends_with("workshop/3449579583/placeholder.tex")));

        let _ = fs::remove_dir_all(&extracted_root);
    }

    #[test]
    fn parses_string_origin_into_visual_position() {
        let root = std::env::temp_dir().join(format!("scene-origin-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("models")).expect("temp scene models");
        fs::write(
            root.join("models").join("bg.json"),
            r#"{"width":200,"height":100}"#,
        )
        .expect("model json");
        fs::write(
            root.join("scene.json"),
            r#"{
              "camera": { "center": "-120 -45 -1", "eye": "-120 -45 0" },
              "general": { "orthogonalprojection": { "width": 1280, "height": 720 } },
              "objects": [
                {
                  "id": 1,
                  "name": "Background",
                  "image": "models/bg.json",
                  "origin": "640 360 0",
                  "scale": "1 1 1",
                  "size": "200 100"
                }
              ]
            }"#,
        )
        .expect("scene json");

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &BTreeMap::new())
            .expect("manifest");
        let visual = manifest
            .visual_layers
            .iter()
            .find(|layer| layer.id == 1)
            .expect("visual");
        assert_eq!(manifest.camera.center, [-120.0, -45.0]);
        assert_eq!(visual.position, [640.0, 360.0, 0.0]);
        assert_eq!(visual.render_bounds, Some([540.0, 310.0, 200.0, 100.0]));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_particle_runtime_and_adapter_from_authored_renderer_family() {
        let root = std::env::temp_dir().join(format!("scene-particle-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("particles")).expect("particle dir");
        fs::write(
            root.join("particles").join("source.json"),
            r#"{
              "maxcount": 64,
              "emitter": [{
                "name": "root",
                "rate": 36,
                "origin": "10 20 0",
                "speedmin": 2,
                "speedmax": 8
              }],
              "renderer": [{
                "name": "rope",
                "length": 12,
                "maxlength": 48,
                "subdivision": 3
              }],
              "controlpoint": [{"id": 0, "offset": "1 2 0"}]
            }"#,
        )
        .expect("particle json");
        fs::write(
            root.join("scene.json"),
            r#"{
              "general": { "orthogonalprojection": { "width": 1280, "height": 720 } },
              "objects": [{
                "id": 9,
                "name": "Authored Particle",
                "particle": "particles/source.json",
                "origin": "320 240 0",
                "scale": "1 1 2",
                "instanceoverride": {
                  "rate": 42,
                  "size": 3,
                  "lifetime": 1.4,
                  "count": 24,
                  "colorn": "0.1 0.2 0.8"
                }
              }]
            }"#,
        )
        .expect("scene json");

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &BTreeMap::new())
            .expect("manifest");

        assert_eq!(manifest.particle_layers.len(), 1);
        assert_eq!(
            manifest.particle_layers[0].kind,
            crate::models::SceneParticleKind::LineTrail
        );
        assert_eq!(manifest.particle_layers[0].position, [320.0, 240.0, 0.0]);
        assert_eq!(manifest.particle_layers[0].emission_rate, 42.0);
        assert_eq!(manifest.particle_runtimes.len(), 1);
        let runtime = &manifest.particle_runtimes[0];
        assert!(runtime.adapter.supported);
        assert_eq!(
            runtime.adapter.schedule_mode,
            crate::models::SceneParticleScheduleMode::Autonomous
        );
        assert_eq!(runtime.system.max_count, Some(64));
        assert_eq!(runtime.system.renderers[0].length, Some(12.0));
        assert_eq!(
            runtime.system.control_points[0].offset,
            Some([1.0, 2.0, 0.0])
        );
        assert_eq!(runtime.instance_override.count, Some(24.0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preserves_particle_child_hierarchy_without_polluting_legacy_layers() {
        let root = std::env::temp_dir().join(format!("scene-particle-child-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("particles")).expect("particle dir");
        fs::write(
            root.join("particles").join("source.json"),
            r#"{
              "emitter": [{"name": "root", "rate": 12}],
              "renderer": [{"name": "spritetrail"}],
              "children": [
                {"name": "attached", "type": "static", "origin": "2 0 0"},
                {"name": "follow", "type": "eventfollow", "scale": "0.5 0.5 1"},
                {"name": "death", "type": "eventdeath", "probability": 0.5}
              ]
            }"#,
        )
        .expect("particle json");
        fs::write(
            root.join("scene.json"),
            r#"{
              "objects": [{
                "id": 10,
                "name": "Child Particle",
                "particle": "particles/source.json"
              }]
            }"#,
        )
        .expect("scene json");

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &BTreeMap::new())
            .expect("manifest");

        assert!(manifest.particle_layers.is_empty());
        assert_eq!(manifest.particle_runtimes.len(), 1);
        assert_eq!(manifest.particle_runtimes[0].system.children.len(), 3);
        assert!(
            manifest.particle_runtimes[0].adapter.supported,
            "SpriteTrail with children should be supported in the first-class sprite path"
        );
        assert!(manifest.render_graph.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preserves_local_origin_for_parented_visual_objects() {
        let root = std::env::temp_dir().join(format!("scene-parent-origin-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("models")).expect("temp scene models");
        fs::write(
            root.join("models").join("parent.json"),
            r#"{"width":400,"height":200}"#,
        )
        .expect("parent model");
        fs::write(
            root.join("models").join("child.json"),
            r#"{"width":120,"height":80}"#,
        )
        .expect("child model");
        fs::write(
            root.join("scene.json"),
            r#"{
              "general": { "orthogonalprojection": { "width": 1280, "height": 720 } },
              "objects": [
                {
                  "id": 1,
                  "name": "Parent",
                  "image": "models/parent.json",
                  "origin": "640 360 0",
                  "scale": "2 2 1",
                  "size": "400 200"
                },
                {
                  "id": 2,
                  "name": "Child",
                  "parent": 1,
                  "image": "models/child.json",
                  "origin": "50 25 0",
                  "scale": "1 1 1",
                  "size": "120 80"
                }
              ]
            }"#,
        )
        .expect("scene json");

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &BTreeMap::new())
            .expect("manifest");
        let child = manifest
            .visual_layers
            .iter()
            .find(|layer| layer.id == 2)
            .expect("child visual");
        assert_eq!(child.parent_id, Some(1));
        assert_eq!(child.position, [50.0, 25.0, 0.0]);
        assert_eq!(child.scale, [1.0, 1.0, 1.0]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_visual_authored_metadata_and_dynamic_fields() {
        let root = std::env::temp_dir().join(format!("scene-visual-meta-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("models")).expect("temp scene models");
        fs::write(
            root.join("models").join("hero.json"),
            r#"{
              "width": 300,
              "height": 150,
              "autosize": true,
              "solidlayer": true,
              "passthrough": true,
              "nopadding": true,
              "puppet": "models/hero_puppet.mdl"
            }"#,
        )
        .expect("model json");
        fs::write(
            root.join("scene.json"),
            r#"{
              "general": { "orthogonalprojection": { "width": 1280, "height": 720 } },
              "objects": [
                {
                  "id": 7,
                  "name": "Hero",
                  "parent": 2,
                  "dependencies": [5, 6],
                  "image": "models/hero.json",
                  "visible": { "user": { "name": "mode", "condition": "hero" } },
                  "origin": {
                    "value": "100 200 0",
                    "scriptproperties": {
                      "x": { "user": "visualX" },
                      "y": { "value": 0.5 }
                    }
                  },
                  "scale": { "user": "visualScale" },
                  "alpha": { "user": "opacity" },
                  "color": { "user": "tint" },
                  "brightness": { "user": "brightness" },
                  "parallaxDepth": { "user": "depth" },
                  "colorBlendMode": 2,
                  "angles": "0 0 15",
                  "size": "300 150",
                  "animationlayers": [
                    {
                      "id": 9,
                      "rate": 1.25,
                      "visible": { "user": "animVisible" },
                      "blend": "normal",
                      "animation": "idle"
                    }
                  ],
                  "effects": [
                    {
                      "file": "effects/workshop/common/scroll/effect.json",
                      "visible": true,
                      "passes": [
                        {
                          "constantshadervalues": {
                            "speedx": 0.08,
                            "repeat": "1 1"
                          },
                          "textures": [null, "masks/cloud-mask"]
                        }
                      ]
                    }
                  ]
                }
              ]
            }"#,
        )
        .expect("scene json");

        let property_values = BTreeMap::from([
            ("visualX".to_string(), json!(0.25)),
            ("visualScale".to_string(), json!("2 3 1")),
        ]);

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &property_values)
            .expect("manifest");
        let visual = manifest
            .visual_layers
            .iter()
            .find(|layer| layer.id == 7)
            .expect("visual");

        assert_eq!(visual.dependencies, vec![5, 6]);
        assert_eq!(visual.parent_id, Some(2));
        assert_eq!(visual.position, [320.0, 360.0, 0.0]);
        assert_eq!(
            visual
                .position_bindings
                .as_ref()
                .and_then(|bindings| bindings.x.as_deref()),
            Some("visualX")
        );
        assert_eq!(visual.scale, [2.0, 3.0, 1.0]);
        assert_eq!(visual.scale_binding.as_deref(), Some("visualScale"));
        assert_eq!(
            visual
                .visibility_binding
                .as_ref()
                .map(|binding| binding.property_key.as_str()),
            Some("mode")
        );
        assert_eq!(
            visual
                .visibility_binding
                .as_ref()
                .and_then(|binding| binding.condition.as_deref()),
            Some("hero")
        );
        assert_eq!(visual.angles, Some([0.0, 0.0, 15.0]));
        assert_eq!(visual.opacity_binding.as_deref(), Some("opacity"));
        assert_eq!(visual.color_binding.as_deref(), Some("tint"));
        assert_eq!(visual.brightness_binding.as_deref(), Some("brightness"));
        assert_eq!(visual.parallax_depth_binding.as_deref(), Some("depth"));
        assert_eq!(visual.color_blend_mode, Some(2));
        assert!(visual.autosize);
        assert!(visual.solid_layer);
        assert!(visual.passthrough);
        assert!(visual.no_padding);
        assert_eq!(visual.model_width, Some(300.0));
        assert_eq!(visual.model_height, Some(150.0));
        assert_eq!(
            visual.puppet_path.as_deref(),
            Some("models/hero_puppet.mdl")
        );
        assert_eq!(visual.animation_layers.len(), 1);
        assert_eq!(visual.animation_layers[0].id, 9);
        assert_eq!(visual.animation_layers[0].rate, 1.25);
        assert_eq!(visual.animation_layers[0].blend, "normal");
        assert_eq!(visual.animation_layers[0].animation, "idle");
        assert_eq!(
            visual.animation_layers[0]
                .visibility_binding
                .as_ref()
                .map(|binding| binding.property_key.as_str()),
            Some("animVisible")
        );
        assert_eq!(visual.effect_instances.len(), 1);
        assert_eq!(
            visual.effect_instances[0].effect_path,
            "effects/workshop/common/scroll/effect.json"
        );
        assert!(visual.effect_instances[0].visible);
        assert_eq!(visual.effect_instances[0].passes.len(), 1);
        assert_eq!(
            visual.effect_instances[0].passes[0].constants.get("speedx"),
            Some(&json!(0.08))
        );
        assert_eq!(
            visual.effect_instances[0].passes[0].textures,
            vec![None, Some("masks/cloud-mask".to_string())]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_text_fields_from_generic_object_without_text_type() {
        let root = std::env::temp_dir().join(format!("scene-text-generic-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp scene root");
        fs::write(
            root.join("scene.json"),
            r#"{
              "general": { "orthogonalprojection": { "width": 1280, "height": 720 } },
              "objects": [
                {
                  "id": 11,
                  "name": "Date",
                  "font": "fonts/demo.ttf",
                  "pointsize": { "user": "fontScale", "value": 140 },
                  "origin": {
                    "value": "640 360 0",
                    "scriptproperties": {
                      "x": { "user": "x" },
                      "y": { "user": "y", "value": 0.5 }
                    }
                  },
                  "scale": { "user": "textScale", "value": "0.18 0.18 1" },
                  "angles": "0 0 0.75",
                  "size": "1971 671",
                  "anchor": "none",
                  "padding": 32,
                  "maxwidth": 500,
                  "maxrows": 1,
                  "horizontalalign": "left",
                  "verticalalign": "center",
                  "text": "<Date>",
                  "visible": { "user": { "name": "day", "condition": "3" }, "value": false }
                }
              ]
            }"#,
        )
        .expect("scene json");

        let property_values = BTreeMap::from([("textScale".to_string(), json!("0.2 0.25 1"))]);
        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &property_values)
            .expect("manifest");
        let text = manifest
            .text_layers
            .iter()
            .find(|layer| layer.id == 11)
            .expect("text layer");
        let expected_font_path = root.join("fonts/demo.ttf").display().to_string();
        assert_eq!(text.name, "Date");
        assert_eq!(text.scale, [0.2, 0.25, 1.0]);
        assert_eq!(text.scale_binding.as_deref(), Some("textScale"));
        assert_eq!(text.angles, Some([0.0, 0.0, 0.75]));
        assert_eq!(text.rotation, Some(0.75));
        assert_eq!(text.size, Some([1971.0, 671.0]));
        assert_eq!(text.anchor.as_deref(), Some("none"));
        assert_eq!(text.render_bounds.map(|bounds| bounds[0]), Some(640.0));
        assert_eq!(text.padding, Some(32.0));
        assert_eq!(text.max_width, Some(500.0));
        assert_eq!(text.point_size_binding.as_deref(), Some("fontScale"));
        assert_eq!(text.font_reference.as_deref(), Some("fonts/demo.ttf"));
        assert_eq!(text.font_path.as_deref(), Some(expected_font_path.as_str()));
        assert_eq!(
            text.position_bindings
                .as_ref()
                .and_then(|bindings| bindings.x.as_deref()),
            Some("x")
        );
        assert_eq!(
            text.position_bindings
                .as_ref()
                .and_then(|bindings| bindings.y.as_deref()),
            Some("y")
        );
        assert_eq!(
            text.visibility_binding
                .as_ref()
                .map(|binding| binding.property_key.as_str()),
            Some("day")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_audio_transform_bindings_and_rotation_from_authored_object() {
        let root = std::env::temp_dir().join(format!("scene-audio-transform-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp scene root");
        fs::write(
            root.join("scene.json"),
            r#"{
              "general": { "orthogonalprojection": { "width": 1280, "height": 720 } },
              "objects": [
                {
                  "id": 21,
                  "name": "Spectrum",
                  "origin": "320 180 0",
                  "scale": { "user": "audioScale", "value": "1 1 1" },
                  "angles": "0 0 0.4",
                  "size": "300 120",
                  "effects": [
                    {
                      "file": "effects/audio/effect.json",
                      "passes": [
                        {
                          "constantshadervalues": {
                            "Bar Count": 24
                          }
                        }
                      ]
                    }
                  ]
                }
              ]
            }"#,
        )
        .expect("scene json");
        let property_values = BTreeMap::from([("audioScale".to_string(), json!("1.5 2 1"))]);

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &property_values)
            .expect("manifest");
        let audio = manifest
            .audio_layers
            .iter()
            .find(|layer| layer.id == 21)
            .expect("audio layer");

        assert_eq!(audio.scale, [1.5, 2.0, 1.0]);
        assert_eq!(audio.scale_binding.as_deref(), Some("audioScale"));
        assert_eq!(audio.angles, Some([0.0, 0.0, 0.4]));
        assert_eq!(audio.rotation, Some(0.4));
        assert_eq!(audio.angle, Some(0.4));
        assert_eq!(audio.render_bounds, Some([95.0, 420.0, 450.0, 240.0]));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn text_anchor_none_uses_text_align_to_center_authored_box() {
        let root =
            std::env::temp_dir().join(format!("scene-text-anchor-center-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp scene root");
        fs::write(
            root.join("scene.json"),
            r#"{
              "general": { "orthogonalprojection": { "width": 1920, "height": 1080 } },
              "objects": [
                {
                  "id": 12,
                  "name": "Centered Caption",
                  "font": "systemfont_comicsans",
                  "pointsize": 7,
                  "origin": "960 180 0",
                  "size": "360 84",
                  "anchor": "none",
                  "horizontalalign": "center",
                  "verticalalign": "center",
                  "text": "Centered caption"
                }
              ]
            }"#,
        )
        .expect("scene json");

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &BTreeMap::new())
            .expect("manifest");
        let text = manifest
            .text_layers
            .iter()
            .find(|layer| layer.id == 12)
            .expect("text layer");
        assert_eq!(text.anchor.as_deref(), Some("none"));
        assert_eq!(text.horizontal_align.as_deref(), Some("center"));
        assert_eq!(text.vertical_align.as_deref(), Some("center"));
        assert_eq!(text.render_bounds, Some([780.0, 858.0, 360.0, 84.0]));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_text_font_family_and_script_refresh_interval() {
        let root = std::env::temp_dir().join(format!("scene-text-family-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp scene root");
        fs::write(
            root.join("scene.json"),
            r#"{
              "objects": [
                {
                  "id": 12,
                  "name": "Clock",
                  "font": "DIN Alternate",
                  "pointsize": 96,
                  "text": {
                    "value": "<Clock>",
                    "script": "return clockText;",
                    "scriptproperties": {
                      "showSeconds": true,
                      "refreshInterval": 1500
                    }
                  }
                }
              ]
            }"#,
        )
        .expect("scene json");

        let manifest = parse_scene_manifest(&root.join("scene.json"), &root, &BTreeMap::new())
            .expect("manifest");
        let text = manifest
            .text_layers
            .iter()
            .find(|layer| layer.id == 12)
            .expect("text layer");

        assert_eq!(text.font_reference.as_deref(), Some("DIN Alternate"));
        assert!(text.font_path.is_none());
        assert_eq!(text.script_refresh_interval_millis, Some(1500));
        assert_eq!(text.show_seconds, Some(true));
        assert_eq!(text.script_text.as_deref(), Some("return clockText;"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn estimates_multiline_text_layers_with_more_height() {
        let script_properties = json!({
            "alignVertical": true,
            "dayFormat": "2"
        });
        let vertical = estimate_text_layer_size(
            "",
            crate::models::SceneTextBehavior::Weekday,
            84.0,
            script_properties.as_object(),
            "",
        )
        .expect("vertical text size");
        let horizontal = estimate_text_layer_size(
            "MONDAY",
            crate::models::SceneTextBehavior::Static,
            84.0,
            None,
            "",
        )
        .expect("horizontal text size");
        assert!(vertical[1] > horizontal[1]);
    }

    #[test]
    fn keeps_authored_text_size_when_present() {
        let normalized = normalize_text_layout_size(
            Some([2118.0, 1047.0]),
            Some([292.0, 178.0]),
            Some(500.0),
            Some(32.0),
        )
        .expect("normalized text layout size");
        assert_eq!(normalized, [2118.0, 1047.0]);
    }

    #[test]
    fn uses_estimated_text_size_only_when_authored_size_is_missing() {
        let normalized =
            normalize_text_layout_size(None, Some([92.0, 116.0]), Some(500.0), Some(32.0))
                .expect("normalized text layout size");
        assert_eq!(normalized, [156.0, 180.0]);
    }

    #[test]
    fn blockalign_false_text_size_estimation_does_not_apply_padding() {
        let normalized = normalize_text_layout_size(
            None,
            Some([92.0, 116.0]),
            Some(500.0),
            text_layout_padding_for_block(Some(32.0), Some(false)),
        )
        .expect("normalized text layout size");

        assert_eq!(normalized, [92.0, 116.0]);
        assert_eq!(
            text_layout_padding_for_block(Some(32.0), Some(true)),
            Some(32.0)
        );
        assert_eq!(text_layout_padding_for_block(Some(32.0), None), Some(32.0));
    }

    #[test]
    fn samples_clock_text_with_seconds_when_enabled() {
        let script_properties = json!({
            "showSeconds": true
        });
        assert_eq!(
            sample_text_for_behavior(
                "",
                &crate::models::SceneTextBehavior::Clock,
                script_properties.as_object(),
                None,
            ),
            "22:14:33"
        );
    }

    #[test]
    fn samples_spaced_and_vertical_calendar_text() {
        let weekday_props = json!({
            "dayFormat": "2",
            "showDay": true,
            "alignVertical": false
        });
        let weekday_script = "return ['S U N D A Y','M O N D A Y'][1]";
        assert_eq!(
            sample_text_for_behavior(
                "DAY",
                &crate::models::SceneTextBehavior::Weekday,
                weekday_props.as_object(),
                Some(weekday_script),
            ),
            "M O N D A Y"
        );

        let date_props = json!({
            "monthFormat": "2",
            "showDay": false,
            "alignVertical": true,
            "useDelimiter": false
        });
        let date_script =
            "delimiterValue = ['\\n\\n']; months = ['M' + newLine + 'A' + newLine + 'R'];";
        assert_eq!(
            sample_text_for_behavior(
                "<Date>",
                &crate::models::SceneTextBehavior::Date,
                date_props.as_object(),
                Some(date_script),
            ),
            "3\n1\n\nM\nA\nR\n\n2\n0\n2\n6"
        );
    }

    #[test]
    fn detects_media_title_text_behavior() {
        assert_eq!(
            detect_text_behavior(
                "歌词",
                "Title",
                "export function mediaPropertiesChanged(event) { return event.title; }",
            ),
            crate::models::SceneTextBehavior::MediaTitle,
        );
    }

    #[test]
    fn detects_day_period_behavior_before_clock_for_time_of_day_scripts() {
        assert_eq!(
            detect_text_behavior(
                "早中晚英文",
                "Before dawn",
                "let shichenY = {1:'Before dawn',2:'At night',3:'Morning',4:'Morning',5:'Noon',6:'Afternoon',7:'Evening',8:'Night'}; let hour = new Date().getHours(); return value;",
            ),
            crate::models::SceneTextBehavior::DayPeriod,
        );
    }

    #[test]
    fn detects_script_behavior_for_user_property_greeting_scripts() {
        assert_eq!(
            detect_text_behavior(
                "Greeting",
                "Greetings Placeholder Text\n<Name>",
                "'use strict'; let eveningGreetings = ['Good evening, $!']; var lastString; export function update() { let newString = eveningGreetings[0]; if (newString != lastString) { lastString = newString; thisLayer.text = newString.replace('$', engine.userProperties.name); } } export function applyUserProperties() { lastString = ''; }",
            ),
            crate::models::SceneTextBehavior::Script,
        );
    }

    #[test]
    fn detects_date_behavior_before_weekday_for_calendar_scripts() {
        assert_eq!(
            detect_text_behavior(
                "Date",
                "<Date>",
                "export var scriptProperties = createScriptProperties().addCombo({ name: 'dayFormat' }).addCheckbox({ name: 'showDay' }).finish();",
            ),
            crate::models::SceneTextBehavior::Date,
        );
    }
}
