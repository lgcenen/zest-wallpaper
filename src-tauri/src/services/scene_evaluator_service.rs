use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Local, Utc};
use serde_json::Value;

use crate::models::{
    EvaluatedAudioState, EvaluatedSceneCamera, EvaluatedSceneObject, EvaluatedSceneObjectBase,
    EvaluatedSceneTransform, EvaluatedTextLayout, EvaluatedTextState, EvaluatedTextStyle,
    SceneAxisBindings, SceneBinding, SceneCamera, SceneManifest, SceneNowPlayingSnapshot,
    SceneParticleKind, SceneRenderNodeKind, SceneTextBehavior, SceneTextLayer, SceneVisualLayer,
    WallpaperProperty,
};

use super::{scene_text_behavior_service, scene_text_script_runtime_service};

pub fn evaluate_scene_runtime_document_with_runtime_key(
    runtime_owner_key: Option<&str>,
    source: SceneManifest,
    properties: &BTreeMap<String, Value>,
    persisted_properties: &BTreeMap<String, Value>,
    property_definitions: &BTreeMap<String, WallpaperProperty>,
    now_playing: Option<&SceneNowPlayingSnapshot>,
    now: DateTime<Utc>,
) -> crate::models::SceneRuntimeDocument {
    let evaluated = evaluate_scene_with_runtime_key(
        runtime_owner_key,
        &source,
        properties,
        persisted_properties,
        property_definitions,
        now_playing,
        now,
    );
    crate::models::SceneRuntimeDocument {
        runtime_owner_key: runtime_owner_key.map(ToString::to_string),
        source,
        evaluated,
        now_playing: now_playing.cloned().unwrap_or_default(),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn evaluate_scene(
    source: &SceneManifest,
    properties: &BTreeMap<String, Value>,
    persisted_properties: &BTreeMap<String, Value>,
    property_definitions: &BTreeMap<String, WallpaperProperty>,
    now_playing: Option<&SceneNowPlayingSnapshot>,
    now: DateTime<Utc>,
) -> crate::models::SceneEvaluatedDocument {
    evaluate_scene_with_runtime_key(
        None,
        source,
        properties,
        persisted_properties,
        property_definitions,
        now_playing,
        now,
    )
}

fn evaluate_scene_with_runtime_key(
    runtime_owner_key: Option<&str>,
    source: &SceneManifest,
    properties: &BTreeMap<String, Value>,
    persisted_properties: &BTreeMap<String, Value>,
    property_definitions: &BTreeMap<String, WallpaperProperty>,
    now_playing: Option<&SceneNowPlayingSnapshot>,
    now: DateTime<Utc>,
) -> crate::models::SceneEvaluatedDocument {
    let canvas_width = source.canvas_width.unwrap_or(3840.0).max(1.0);
    let canvas_height = source.canvas_height.unwrap_or(2160.0).max(1.0);
    let camera = evaluate_camera(&source.camera, properties);
    let local_now = now.with_timezone(&Local);
    let mut objects = BTreeMap::new();
    let active_script_layer_ids = source
        .text_layers
        .iter()
        .filter(|layer| layer.behavior == SceneTextBehavior::Script)
        .map(|layer| layer.id)
        .collect::<BTreeSet<_>>();

    for node in &source.nodes {
        let visible = source_object_visible(
            source,
            Some(node.id),
            properties,
            persisted_properties,
            property_definitions,
        );
        let transform = resolve_transform(
            source,
            node.parent_id,
            node.position,
            node.position_bindings.as_ref(),
            node.scale,
            None,
            node.rotation.unwrap_or(0.0),
            None,
            None,
            None,
            canvas_width,
            canvas_height,
            properties,
        );
        objects.insert(
            node.id,
            EvaluatedSceneObject::Container {
                base: EvaluatedSceneObjectBase {
                    id: node.id,
                    name: node.name.clone(),
                    parent_id: node.parent_id,
                    dependencies: node.dependencies.clone(),
                    visible,
                    alignment: None,
                    opacity: 1.0,
                    transform,
                },
            },
        );
    }

    for layer in &source.visual_layers {
        let visible = source_object_visible(
            source,
            Some(layer.id),
            properties,
            persisted_properties,
            property_definitions,
        );
        let resolved_opacity = resolve_bound_number(layer.opacity_binding.as_deref(), properties)
            .unwrap_or(layer.opacity.unwrap_or(1.0))
            .clamp(0.0, 1.0);
        let resolved_color = resolve_bound_string(layer.color_binding.as_deref(), properties)
            .or_else(|| layer.color.clone());
        let resolved_brightness =
            resolve_bound_number(layer.brightness_binding.as_deref(), properties)
                .or(layer.brightness);
        let resolved_parallax_depth = resolve_bound_vec2(
            layer.parallax_depth_binding.as_deref(),
            layer.parallax_depth,
            properties,
        );
        let transform = resolve_transform(
            source,
            layer.parent_id,
            layer.position,
            layer.position_bindings.as_ref(),
            layer.scale,
            layer.scale_binding.as_deref(),
            layer.rotation.unwrap_or(0.0),
            layer.size,
            layer.render_bounds,
            layer.alignment.as_deref(),
            canvas_width,
            canvas_height,
            properties,
        );
        let background_candidate = layer.primary_visual_candidate(source)
            && full_bleed_bounds(&transform, canvas_width, canvas_height, layer.fullscreen);
        objects.insert(
            layer.id,
            EvaluatedSceneObject::Visual {
                base: EvaluatedSceneObjectBase {
                    id: layer.id,
                    name: layer.name.clone(),
                    parent_id: layer.parent_id,
                    dependencies: layer.dependencies.clone(),
                    visible,
                    alignment: layer.alignment.clone(),
                    opacity: resolved_opacity,
                    transform,
                },
                asset_kind: layer.asset_kind.clone(),
                asset_path: layer.asset_path.clone(),
                system_texture_key: layer.system_texture_key.clone(),
                texture_names: layer.texture_names.clone(),
                blend_mode: layer.blend_mode.clone(),
                color: resolved_color,
                brightness: resolved_brightness,
                color_blend_mode: layer.color_blend_mode,
                parallax_depth: resolved_parallax_depth,
                angles: layer.angles,
                fullscreen: layer.fullscreen,
                autosize: layer.autosize,
                solid_layer: layer.solid_layer,
                passthrough: layer.passthrough,
                no_padding: layer.no_padding,
                puppet_path: layer.puppet_path.clone(),
                animation_layers: layer.animation_layers.clone(),
                primary: source.primary_visual.as_ref().map(|primary| primary.id) == Some(layer.id),
                background_candidate,
            },
        );
    }

    for layer in &source.text_layers {
        let visible = source_object_visible(
            source,
            Some(layer.id),
            properties,
            persisted_properties,
            property_definitions,
        );
        let resolved_color = resolve_bound_string(layer.color_binding.as_deref(), properties)
            .or_else(|| layer.color.clone())
            .or_else(|| Some("1 1 1".to_string()));
        let resolved_alpha = resolve_bound_number(layer.alpha_binding.as_deref(), properties)
            .unwrap_or(layer.alpha.unwrap_or(1.0));
        let resolved_point_size =
            resolve_bound_number(layer.point_size_binding.as_deref(), properties)
                .unwrap_or(layer.point_size.unwrap_or(24.0));
        let resolved_text = resolve_text(
            runtime_owner_key,
            layer,
            properties,
            &local_now,
            now_playing,
        );
        let measured_size = layer.size.or_else(|| {
            estimate_text_size(&resolved_text, resolved_point_size, layer.behavior.clone())
        });
        let transform_alignment = text_transform_alignment(
            layer.alignment.as_deref(),
            layer.anchor.as_deref(),
            layer.horizontal_align.as_deref(),
            layer.vertical_align.as_deref(),
        );
        let transform = resolve_transform(
            source,
            layer.parent_id,
            layer.position,
            layer.position_bindings.as_ref(),
            layer.scale,
            layer.scale_binding.as_deref(),
            layer.rotation.unwrap_or(0.0),
            measured_size,
            layer.render_bounds,
            transform_alignment.as_deref(),
            canvas_width,
            canvas_height,
            properties,
        );
        let text_layout =
            evaluate_text_layout(layer, &resolved_text, resolved_point_size, &transform);

        objects.insert(
            layer.id,
            EvaluatedSceneObject::Text {
                base: EvaluatedSceneObjectBase {
                    id: layer.id,
                    name: layer.name.clone(),
                    parent_id: layer.parent_id,
                    dependencies: layer.dependencies.clone(),
                    visible,
                    alignment: layer.alignment.clone(),
                    opacity: resolved_alpha.clamp(0.0, 1.0),
                    transform: transform.clone(),
                },
                behavior: layer.behavior.clone(),
                text: EvaluatedTextState {
                    value: resolved_text,
                    style: EvaluatedTextStyle {
                        color: resolved_color,
                        alpha: resolved_alpha.clamp(0.0, 1.0),
                        point_size: normalize_scene_point_size(resolved_point_size),
                        font_path: layer.font_path.clone(),
                        effect_paths: layer.effect_paths.clone(),
                        horizontal_align: layer.horizontal_align.clone(),
                        vertical_align: layer.vertical_align.clone(),
                        padding: layer.padding,
                        max_rows: layer.max_rows,
                        max_width: layer.max_width,
                        limit_width: layer.limit_width,
                        limit_use_ellipsis: layer.limit_use_ellipsis,
                        block_align: layer.block_align,
                    },
                    layout: text_layout,
                    dynamic_input_generation:
                        scene_text_behavior_service::text_dynamic_input_generation(
                            layer,
                            now_playing,
                        ),
                },
            },
        );
    }

    for layer in &source.audio_layers {
        let visible = source_object_visible(
            source,
            Some(layer.id),
            properties,
            persisted_properties,
            property_definitions,
        );
        let opacity = layer.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        let transform = resolve_transform(
            source,
            layer.parent_id,
            layer.position,
            layer.position_bindings.as_ref(),
            layer.scale,
            layer.scale_binding.as_deref(),
            layer.rotation.or(layer.angle).unwrap_or(0.0),
            layer.size,
            layer.render_bounds,
            layer.alignment.as_deref(),
            canvas_width,
            canvas_height,
            properties,
        );
        objects.insert(
            layer.id,
            EvaluatedSceneObject::Audio {
                base: EvaluatedSceneObjectBase {
                    id: layer.id,
                    name: layer.name.clone(),
                    parent_id: layer.parent_id,
                    dependencies: layer.dependencies.clone(),
                    visible,
                    alignment: layer.alignment.clone(),
                    opacity,
                    transform,
                },
                audio: EvaluatedAudioState {
                    bar_count: layer.bar_count,
                    color: layer.color.clone(),
                    bar_spacing: layer.bar_spacing,
                    bar_bounds: layer.bar_bounds,
                    minimum_height: layer.minimum_height,
                    radius: layer.radius,
                    volume_factor: layer.volume_factor,
                    opacity: layer.opacity,
                },
            },
        );
    }

    for layer in &source.particle_layers {
        let visible = source_object_visible(
            source,
            Some(layer.id),
            properties,
            persisted_properties,
            property_definitions,
        );
        let resolved_color = resolve_bound_string(layer.color_binding.as_deref(), properties)
            .or_else(|| layer.color.clone());
        let resolved_size =
            resolve_bound_number(layer.size_binding.as_deref(), properties).unwrap_or(layer.size);
        let transform = resolve_transform(
            source,
            layer.parent_id,
            layer.position,
            layer.position_bindings.as_ref(),
            layer.scale,
            layer.scale_binding.as_deref(),
            layer.rotation.unwrap_or(0.0),
            None,
            None,
            layer_visibility_alignment(source, layer.parent_id),
            canvas_width,
            canvas_height,
            properties,
        );
        objects.insert(
            layer.id,
            EvaluatedSceneObject::Particle {
                base: EvaluatedSceneObjectBase {
                    id: layer.id,
                    name: layer.name.clone(),
                    parent_id: layer.parent_id,
                    dependencies: layer.dependencies.clone(),
                    visible,
                    alignment: None,
                    opacity: 1.0,
                    transform,
                },
                particle_path: layer.particle_path.clone(),
                particle_kind: layer.kind.clone(),
                color: resolved_color,
                size: resolved_size,
                emission_rate: layer.emission_rate,
            },
        );
    }

    let legacy_particle_layer_ids = source
        .particle_layers
        .iter()
        .map(|layer| layer.id)
        .collect::<BTreeSet<_>>();
    for runtime in &source.particle_runtimes {
        if legacy_particle_layer_ids.contains(&runtime.object_id) {
            continue;
        }

        let node = source
            .nodes
            .iter()
            .find(|node| node.id == runtime.object_id);
        let parent_id = node.and_then(|node| node.parent_id);
        let visible = source_object_visible(
            source,
            Some(runtime.object_id),
            properties,
            persisted_properties,
            property_definitions,
        );
        let transform = resolve_transform(
            source,
            parent_id,
            [0.0, 0.0, 0.0],
            None,
            [1.0, 1.0, 1.0],
            None,
            0.0,
            None,
            None,
            layer_visibility_alignment(source, parent_id),
            canvas_width,
            canvas_height,
            properties,
        );
        objects.insert(
            runtime.object_id,
            EvaluatedSceneObject::Particle {
                base: EvaluatedSceneObjectBase {
                    id: runtime.object_id,
                    name: node
                        .map(|node| node.name.clone())
                        .unwrap_or_else(|| runtime.object_name.clone()),
                    parent_id,
                    dependencies: node
                        .map(|node| node.dependencies.clone())
                        .unwrap_or_default(),
                    visible,
                    alignment: None,
                    opacity: 1.0,
                    transform,
                },
                particle_path: runtime.particle_path.clone(),
                particle_kind: runtime
                    .adapter
                    .draw_kind
                    .clone()
                    .unwrap_or(SceneParticleKind::PetalTrail),
                color: runtime.instance_override.color.clone(),
                size: runtime.instance_override.size.unwrap_or(1.0).max(0.1),
                emission_rate: runtime
                    .system
                    .emitters
                    .first()
                    .and_then(|emitter| emitter.rate)
                    .unwrap_or(0.0),
            },
        );
    }

    for track in &source.sound_tracks {
        let volume = resolve_bound_number(track.volume_binding.as_deref(), properties)
            .unwrap_or(track.volume);
        objects.insert(
            track.id,
            EvaluatedSceneObject::Sound {
                base: EvaluatedSceneObjectBase {
                    id: track.id,
                    name: track.name.clone(),
                    parent_id: None,
                    dependencies: Vec::new(),
                    visible: true,
                    alignment: None,
                    opacity: 1.0,
                    transform: EvaluatedSceneTransform {
                        position: [0.0, 0.0, 0.0],
                        scale: [1.0, 1.0, 1.0],
                        rotation: 0.0,
                        render_bounds: None,
                    },
                },
                asset_path: track.asset_path.clone(),
                looped: track.looped,
                volume: volume.clamp(0.0, 1.0),
            },
        );
    }

    let mut render_list = Vec::new();
    for render in &source.render_graph {
        if render.kind == SceneRenderNodeKind::Unsupported {
            continue;
        }
        if let Some(object) = objects.get(&render.id) {
            if object_visible(object) {
                render_list.push(render.id);
            }
        }
    }
    for id in source
        .visual_layers
        .iter()
        .map(|layer| layer.id)
        .chain(source.text_layers.iter().map(|layer| layer.id))
        .chain(source.audio_layers.iter().map(|layer| layer.id))
        .chain(source.particle_layers.iter().map(|layer| layer.id))
        .chain(
            source
                .particle_runtimes
                .iter()
                .map(|runtime| runtime.object_id),
        )
    {
        if !render_list.contains(&id) {
            if let Some(object) = objects.get(&id) {
                if object_visible(object) {
                    render_list.push(id);
                }
            }
        }
    }

    scene_text_script_runtime_service::retain_scene_text_script_runtime_owner_layers(
        runtime_owner_key,
        &active_script_layer_ids,
    );

    crate::models::SceneEvaluatedDocument {
        canvas_width,
        canvas_height,
        clear_color: source.clear_color.clone(),
        camera,
        parallax: source.parallax.clone(),
        objects,
        render_list,
        evaluated_at: now,
    }
}

pub fn property_values_from_schema(properties: &[WallpaperProperty]) -> BTreeMap<String, Value> {
    properties
        .iter()
        .map(|property| (property.key.clone(), property.value.clone()))
        .collect()
}

pub fn property_definitions_from_schema(
    properties: &[WallpaperProperty],
) -> BTreeMap<String, WallpaperProperty> {
    properties
        .iter()
        .map(|property| (property.key.clone(), property.clone()))
        .collect()
}

pub fn object_visible(object: &EvaluatedSceneObject) -> bool {
    match object {
        EvaluatedSceneObject::Container { base }
        | EvaluatedSceneObject::Visual { base, .. }
        | EvaluatedSceneObject::Text { base, .. }
        | EvaluatedSceneObject::Audio { base, .. }
        | EvaluatedSceneObject::Particle { base, .. }
        | EvaluatedSceneObject::Sound { base, .. } => base.visible,
    }
}

fn evaluate_camera(
    camera: &SceneCamera,
    properties: &BTreeMap<String, Value>,
) -> EvaluatedSceneCamera {
    EvaluatedSceneCamera {
        zoom: resolve_bound_number(camera.zoom_binding.as_deref(), properties)
            .unwrap_or(camera.zoom)
            .max(0.01),
        center: camera.center,
        camera_shake: camera
            .camera_shake_binding
            .as_deref()
            .and_then(|key| properties.get(key))
            .map(truthy)
            .unwrap_or(camera.camera_shake),
        camera_shake_amplitude: camera.camera_shake_amplitude,
        camera_shake_speed: camera.camera_shake_speed,
        parallax_mouse_influence: resolve_bound_number(
            camera.parallax_mouse_influence_binding.as_deref(),
            properties,
        )
        .unwrap_or(camera.parallax_mouse_influence),
    }
}

fn source_object_visible(
    source: &SceneManifest,
    object_id: Option<u32>,
    properties: &BTreeMap<String, Value>,
    persisted_properties: &BTreeMap<String, Value>,
    property_definitions: &BTreeMap<String, WallpaperProperty>,
) -> bool {
    let Some(object_id) = object_id else {
        return true;
    };
    let mut trail = BTreeSet::new();
    evaluate_source_visibility(
        source,
        object_id,
        properties,
        persisted_properties,
        property_definitions,
        &mut trail,
    )
}

fn evaluate_source_visibility(
    source: &SceneManifest,
    object_id: u32,
    properties: &BTreeMap<String, Value>,
    persisted_properties: &BTreeMap<String, Value>,
    property_definitions: &BTreeMap<String, WallpaperProperty>,
    trail: &mut BTreeSet<u32>,
) -> bool {
    if !trail.insert(object_id) {
        return true;
    }
    let Some((parent_id, visible, binding)) = source_visibility_state(source, object_id) else {
        return true;
    };
    if !binding_visible(
        visible,
        binding,
        properties,
        persisted_properties,
        property_definitions,
    ) {
        return false;
    }
    let Some(parent_id) = parent_id else {
        return true;
    };
    evaluate_source_visibility(
        source,
        parent_id,
        properties,
        persisted_properties,
        property_definitions,
        trail,
    )
}

fn source_visibility_state<'a>(
    source: &'a SceneManifest,
    object_id: u32,
) -> Option<(Option<u32>, bool, Option<&'a SceneBinding>)> {
    if let Some(node) = source.nodes.iter().find(|node| node.id == object_id) {
        return Some((
            node.parent_id,
            node.visible,
            node.visibility_binding.as_ref(),
        ));
    }
    if let Some(layer) = source
        .visual_layers
        .iter()
        .find(|layer| layer.id == object_id)
    {
        return Some((
            layer.parent_id,
            layer.visible,
            layer.visibility_binding.as_ref(),
        ));
    }
    if let Some(layer) = source
        .text_layers
        .iter()
        .find(|layer| layer.id == object_id)
    {
        return Some((
            layer.parent_id,
            layer.visible,
            layer.visibility_binding.as_ref(),
        ));
    }
    if let Some(layer) = source
        .audio_layers
        .iter()
        .find(|layer| layer.id == object_id)
    {
        return Some((
            layer.parent_id,
            layer.visible,
            layer.visibility_binding.as_ref(),
        ));
    }
    if let Some(layer) = source
        .particle_layers
        .iter()
        .find(|layer| layer.id == object_id)
    {
        return Some((
            layer.parent_id,
            layer.visible,
            layer.visibility_binding.as_ref(),
        ));
    }
    None
}

fn binding_visible(
    visible: bool,
    binding: Option<&SceneBinding>,
    properties: &BTreeMap<String, Value>,
    _persisted_properties: &BTreeMap<String, Value>,
    property_definitions: &BTreeMap<String, WallpaperProperty>,
) -> bool {
    let Some(binding) = binding else {
        return visible;
    };
    let Some(current) = properties.get(&binding.property_key) else {
        return visible;
    };

    if let Some(condition) = binding.condition.as_deref() {
        if looks_like_expression(condition) {
            if let Some(evaluated) = evaluate_property_condition(condition, properties) {
                return evaluated;
            }
        }
        return values_equal(current, &Value::String(condition.trim().to_string()));
    }

    if let Some(semantic) = combo_option_visibility(
        property_definitions.get(&binding.property_key),
        current,
        visible,
    ) {
        return semantic;
    }
    truthy(current)
}

fn resolve_transform(
    source: &SceneManifest,
    parent_id: Option<u32>,
    position: [f64; 3],
    position_bindings: Option<&SceneAxisBindings>,
    scale: [f64; 3],
    scale_binding: Option<&str>,
    rotation: f64,
    size: Option<[f64; 2]>,
    fallback_bounds: Option<[f64; 4]>,
    alignment: Option<&str>,
    canvas_width: f64,
    canvas_height: f64,
    properties: &BTreeMap<String, Value>,
) -> EvaluatedSceneTransform {
    let (parent_position, parent_scale, parent_rotation) = resolve_parent_transform(
        source,
        parent_id,
        properties,
        canvas_width,
        canvas_height,
        &mut BTreeSet::new(),
    );
    let resolved_position = resolve_bound_position(
        position,
        position_bindings,
        properties,
        canvas_width,
        canvas_height,
    );
    let resolved_scale = resolve_bound_scale(scale, scale_binding, properties);
    let local_x = resolved_position[0] * parent_scale[0];
    let local_y = resolved_position[1] * parent_scale[1];
    let cos = parent_rotation.cos();
    let sin = parent_rotation.sin();
    let world_position = [
        parent_position[0] + local_x * cos - local_y * sin,
        parent_position[1] + local_x * sin + local_y * cos,
        parent_position[2] + resolved_position[2] * parent_scale[2],
    ];
    let world_scale = [
        resolved_scale[0] * parent_scale[0],
        resolved_scale[1] * parent_scale[1],
        resolved_scale[2] * parent_scale[2],
    ];
    let world_rotation = rotation + parent_rotation;
    let render_bounds = size
        .map(|size| {
            compute_render_bounds(world_position, size, world_scale, alignment, canvas_height)
        })
        .or(fallback_bounds);

    EvaluatedSceneTransform {
        position: world_position,
        scale: world_scale,
        rotation: world_rotation,
        render_bounds,
    }
}

fn resolve_parent_transform(
    source: &SceneManifest,
    node_id: Option<u32>,
    properties: &BTreeMap<String, Value>,
    canvas_width: f64,
    canvas_height: f64,
    trail: &mut BTreeSet<u32>,
) -> ([f64; 3], [f64; 3], f64) {
    let Some(node_id) = node_id else {
        return ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 0.0);
    };
    if !trail.insert(node_id) {
        return ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 0.0);
    }
    let Some(node) = source.nodes.iter().find(|node| node.id == node_id) else {
        return ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], 0.0);
    };
    let (parent_position, parent_scale, parent_rotation) = resolve_parent_transform(
        source,
        node.parent_id,
        properties,
        canvas_width,
        canvas_height,
        trail,
    );
    let resolved_position = resolve_bound_position(
        node.position,
        node.position_bindings.as_ref(),
        properties,
        canvas_width,
        canvas_height,
    );
    let local_x = resolved_position[0] * parent_scale[0];
    let local_y = resolved_position[1] * parent_scale[1];
    let cos = parent_rotation.cos();
    let sin = parent_rotation.sin();
    let world_position = [
        parent_position[0] + local_x * cos - local_y * sin,
        parent_position[1] + local_x * sin + local_y * cos,
        parent_position[2] + resolved_position[2] * parent_scale[2],
    ];
    let world_scale = [
        node.scale[0] * parent_scale[0],
        node.scale[1] * parent_scale[1],
        node.scale[2] * parent_scale[2],
    ];
    let world_rotation = node.rotation.unwrap_or(0.0) + parent_rotation;
    (world_position, world_scale, world_rotation)
}

fn resolve_bound_position(
    position: [f64; 3],
    position_bindings: Option<&SceneAxisBindings>,
    properties: &BTreeMap<String, Value>,
    canvas_width: f64,
    canvas_height: f64,
) -> [f64; 3] {
    let mut resolved = position;
    if let Some(bindings) = position_bindings {
        if let Some(binding) = bindings.x.as_deref() {
            if let Some(value) = resolve_bound_number(Some(binding), properties) {
                resolved[0] = value * canvas_width;
            }
        }
        if let Some(binding) = bindings.y.as_deref() {
            if let Some(value) = resolve_bound_number(Some(binding), properties) {
                resolved[1] = value * canvas_height;
            }
        }
    }
    resolved
}

fn resolve_bound_scale(
    scale: [f64; 3],
    scale_binding: Option<&str>,
    properties: &BTreeMap<String, Value>,
) -> [f64; 3] {
    let Some(binding) = scale_binding else {
        return scale;
    };
    let Some(value) = properties.get(binding) else {
        return scale;
    };
    value_to_vec3(value).unwrap_or(scale)
}

fn compute_render_bounds(
    position: [f64; 3],
    size: [f64; 2],
    scale: [f64; 3],
    alignment: Option<&str>,
    canvas_height: f64,
) -> [f64; 4] {
    let width = size[0].abs() * scale[0].abs().max(0.001);
    let height = size[1].abs() * scale[1].abs().max(0.001);
    let alignment = alignment.unwrap_or_default().to_ascii_lowercase();
    let anchor_x = if alignment.contains("left") {
        0.0
    } else if alignment.contains("right") {
        1.0
    } else {
        0.5
    };
    let anchor_y = if alignment.contains("bottom") {
        0.0
    } else if alignment.contains("top") {
        1.0
    } else {
        0.5
    };
    let left = position[0] - width * anchor_x;
    let bottom = position[1] - height * anchor_y;
    [left, canvas_height - (bottom + height), width, height]
}

fn full_bleed_bounds(
    transform: &EvaluatedSceneTransform,
    canvas_width: f64,
    canvas_height: f64,
    fullscreen: bool,
) -> bool {
    if fullscreen {
        return true;
    }
    let Some([_, _, width, height]) = transform.render_bounds else {
        return false;
    };
    let coverage = (width * height) / (canvas_width * canvas_height);
    (width >= canvas_width * 0.95 && height >= canvas_height * 0.95)
        || (coverage >= 0.82 && width >= canvas_width * 0.72 && height >= canvas_height * 0.72)
}

fn resolve_text(
    runtime_owner_key: Option<&str>,
    layer: &SceneTextLayer,
    properties: &BTreeMap<String, Value>,
    now: &DateTime<Local>,
    now_playing: Option<&SceneNowPlayingSnapshot>,
) -> String {
    if let Some(binding) = layer.text_binding.as_deref() {
        if let Some(value) = properties.get(binding) {
            if let Some(text) = string_from_value(value) {
                if !text.trim().is_empty() {
                    return text;
                }
            }
        }
    }

    if layer.behavior == SceneTextBehavior::Script {
        return scene_text_script_runtime_service::evaluate_scripted_text_layer(
            runtime_owner_key,
            layer,
            properties,
            now,
        )
        .ok()
        .flatten()
        .unwrap_or_else(|| scene_text_behavior_service::fallback_text(layer));
    }

    scene_text_behavior_service::evaluate_text_behavior(layer, now, now_playing).value
}

fn evaluate_text_layout(
    layer: &SceneTextLayer,
    text: &str,
    point_size: f64,
    transform: &EvaluatedSceneTransform,
) -> EvaluatedTextLayout {
    let world_scale = transform.scale;
    let layout_scale_factor = world_scale[0].abs().max(world_scale[1].abs()).max(0.001);
    let base_point_size = normalize_scene_point_size(point_size);
    let scaled_padding = text_layout_padding(layer) * layout_scale_factor;
    let render_bounds = transform.render_bounds;
    let estimated_content_size = estimate_text_size(text, base_point_size, layer.behavior.clone())
        .unwrap_or([base_point_size, base_point_size]);
    let max_width = layer
        .max_width
        .filter(|value| *value > 0.0)
        .filter(|_| render_bounds.is_none())
        .map(|value| value * layout_scale_factor);
    let fit_scale = render_bounds
        .map(|bounds| {
            let inner_width = (bounds[2] - scaled_padding * 2.0).max(1.0);
            let inner_height = (bounds[3] - scaled_padding * 2.0).max(1.0);
            let available_width = max_width
                .map(|value| value.min(inner_width))
                .unwrap_or(inner_width);
            let width_scale = available_width / estimated_content_size[0].max(1.0);
            let height_scale = inner_height / estimated_content_size[1].max(1.0);
            match text_layout_fit_mode(layer) {
                TextLayoutFitMode::Width => width_scale.min(height_scale.max(width_scale)),
                TextLayoutFitMode::Height => {
                    if layer.limit_width == Some(true) || layer.limit_use_ellipsis == Some(true) {
                        height_scale.min(width_scale)
                    } else {
                        height_scale
                    }
                }
                TextLayoutFitMode::Contain => width_scale.min(height_scale),
            }
            .clamp(0.001, 64.0)
        })
        .unwrap_or(1.0);
    let scaled_point_size = base_point_size * fit_scale;
    let fitted_content_size = estimate_text_size(text, scaled_point_size, layer.behavior.clone())
        .unwrap_or([scaled_point_size, scaled_point_size]);
    let content_bounds = render_bounds.map(|bounds| {
        let inner_width = (bounds[2] - scaled_padding * 2.0).max(1.0);
        let inner_height = (bounds[3] - scaled_padding * 2.0).max(1.0);
        let max_width = max_width.unwrap_or(inner_width);
        let content_width = fitted_content_size[0].max(1.0);
        let content_width = if render_bounds.is_some() {
            content_width
        } else {
            content_width.min(inner_width).min(max_width)
        };
        let content_height = if render_bounds.is_some() {
            fitted_content_size[1].max(1.0)
        } else {
            fitted_content_size[1].min(inner_height).max(1.0)
        };
        let horizontal = layer.horizontal_align.as_deref().unwrap_or("center");
        let vertical = layer.vertical_align.as_deref().unwrap_or("center");
        let offset_x = alignment_offset(inner_width, content_width, horizontal);
        let offset_y = alignment_offset(inner_height, content_height, vertical);
        [
            bounds[0] + scaled_padding + offset_x,
            bounds[1] + scaled_padding + offset_y,
            content_width,
            content_height,
        ]
    });

    EvaluatedTextLayout {
        size: layer.size.or(Some(estimated_content_size)),
        render_bounds,
        content_bounds,
        scaled_point_size,
        scaled_padding,
        world_scale,
    }
}

fn text_layout_padding(layer: &SceneTextLayer) -> f64 {
    if layer.block_align == Some(false) {
        0.0
    } else {
        layer.padding.unwrap_or(0.0).max(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextLayoutFitMode {
    Width,
    Height,
    Contain,
}

fn text_layout_fit_mode(layer: &SceneTextLayer) -> TextLayoutFitMode {
    if layer.size.is_none() && layer.render_bounds.is_none() {
        return TextLayoutFitMode::Contain;
    }
    if matches!(
        layer.behavior,
        SceneTextBehavior::Static | SceneTextBehavior::Script
    ) {
        TextLayoutFitMode::Width
    } else {
        TextLayoutFitMode::Height
    }
}

fn text_transform_alignment(
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

fn alignment_offset(container: f64, content: f64, alignment: &str) -> f64 {
    let free = container - content;
    let lower = alignment.to_ascii_lowercase();
    if lower.contains("left") || lower.contains("top") {
        0.0
    } else if lower.contains("right") || lower.contains("bottom") {
        free
    } else {
        free * 0.5
    }
}

fn estimate_text_size(
    text: &str,
    point_size: f64,
    behavior: SceneTextBehavior,
) -> Option<[f64; 2]> {
    let safe_text = if text.trim().is_empty() { " " } else { text };
    let normalized_point_size = normalize_scene_point_size(point_size);
    let lines = safe_text.lines().collect::<Vec<_>>();
    let line_height = if behavior == SceneTextBehavior::Clock {
        0.88
    } else if behavior == SceneTextBehavior::Weekday {
        0.84
    } else {
        0.94
    };
    let max_units = lines
        .iter()
        .map(|line| line.chars().map(glyph_width_units).sum::<f64>())
        .fold(1.0, f64::max);
    Some([
        max_units * normalized_point_size + normalized_point_size * 0.44,
        (lines.len().max(1) as f64) * normalized_point_size * line_height
            + normalized_point_size * 0.34,
    ])
}

fn normalize_scene_point_size(point_size: f64) -> f64 {
    if !point_size.is_finite() {
        return 24.0;
    }
    point_size.max(6.0)
}

fn glyph_width_units(character: char) -> f64 {
    match character {
        ' ' => 0.34,
        '|' | 'i' | 'l' | 'I' => 0.38,
        '1' | '!' | ':' | ';' | ',' | '.' => 0.42,
        'W' | 'M' | '@' | '#' | '%' | '&' => 1.04,
        '\t' => 1.6,
        _ if character.is_ascii_uppercase() => 0.74,
        _ if character.is_ascii_digit() => 0.66,
        _ if character.is_ascii_lowercase() => 0.62,
        _ => 1.0,
    }
}

fn resolve_bound_number(
    binding_key: Option<&str>,
    properties: &BTreeMap<String, Value>,
) -> Option<f64> {
    binding_key
        .and_then(|key| properties.get(key))
        .and_then(value_to_f64)
}

fn resolve_bound_string(
    binding_key: Option<&str>,
    properties: &BTreeMap<String, Value>,
) -> Option<String> {
    binding_key
        .and_then(|key| properties.get(key))
        .and_then(string_from_value)
}

fn resolve_bound_vec2(
    binding_key: Option<&str>,
    fallback: Option<[f64; 2]>,
    properties: &BTreeMap<String, Value>,
) -> Option<[f64; 2]> {
    binding_key
        .and_then(|key| properties.get(key))
        .and_then(value_to_vec2)
        .or(fallback)
}

fn value_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn value_to_vec3(value: &Value) -> Option<[f64; 3]> {
    match value {
        Value::String(text) => {
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
            if numbers.len() >= 2 {
                Some([
                    numbers[0],
                    numbers[1],
                    numbers.get(2).copied().unwrap_or(1.0),
                ])
            } else if numbers.len() == 1 {
                Some([numbers[0], numbers[0], numbers[0]])
            } else {
                None
            }
        }
        _ => value_to_f64(value).map(|scalar| [scalar, scalar, scalar]),
    }
}

fn value_to_vec2(value: &Value) -> Option<[f64; 2]> {
    match value {
        Value::String(text) => {
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
            if numbers.len() >= 2 {
                Some([numbers[0], numbers[1]])
            } else if numbers.len() == 1 {
                Some([numbers[0], numbers[0]])
            } else {
                None
            }
        }
        _ => value_to_f64(value).map(|scalar| [scalar, scalar]),
    }
}

fn string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(if *flag { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

fn normalize_comparable(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            let lowered = trimmed.to_ascii_lowercase();
            if matches!(lowered.as_str(), "true" | "on") {
                Value::Bool(true)
            } else if matches!(lowered.as_str(), "false" | "off") {
                Value::Bool(false)
            } else if let Ok(number) = trimmed.parse::<f64>() {
                serde_json::Number::from_f64(number)
                    .map(Value::Number)
                    .unwrap_or_else(|| Value::String(trimmed.to_string()))
            } else {
                Value::String(trimmed.to_string())
            }
        }
        _ => value.clone(),
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    normalize_comparable(left) == normalize_comparable(right)
}

fn truthy(value: &Value) -> bool {
    match normalize_comparable(value) {
        Value::Bool(flag) => flag,
        Value::Number(number) => number.as_f64().map(|value| value != 0.0).unwrap_or(false),
        Value::String(text) => {
            !text.is_empty() && !matches!(text.to_ascii_lowercase().as_str(), "0" | "none")
        }
        Value::Null => false,
        _ => true,
    }
}

fn combo_option_visibility(
    property: Option<&WallpaperProperty>,
    current: &Value,
    visible: bool,
) -> Option<bool> {
    let property = property?;
    if property.kind != crate::models::PropertyKind::Combo || property.options.is_empty() {
        return None;
    }
    let selected = property
        .options
        .iter()
        .find(|option| values_equal(current, &Value::String(option.value.clone())))?;
    let label = selected.label.trim().to_ascii_lowercase();
    let negative_tokens = [
        "hide", "close", "off", "disable", "hidden", "隐藏", "关闭", "禁用",
    ];
    let positive_tokens = [
        "show", "display", "open", "enable", "visible", "显示", "开启", "启用",
    ];
    if negative_tokens.iter().any(|token| label.contains(token)) {
        return Some(false);
    }
    if positive_tokens.iter().any(|token| label.contains(token)) {
        return Some(true);
    }
    Some(visible)
}

fn looks_like_expression(condition: &str) -> bool {
    let compact = condition.trim();
    compact.contains("&&")
        || compact.contains("||")
        || compact.contains("==")
        || compact.contains("!=")
        || compact.contains(">=")
        || compact.contains("<=")
        || compact.contains('>')
        || compact.contains('<')
        || compact.contains('!')
        || compact.contains('(')
        || compact.contains(')')
        || compact.contains(".value")
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConditionTokenType {
    Identifier,
    Number,
    String,
    Boolean,
    Operator,
    Paren,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConditionToken {
    token_type: ConditionTokenType,
    value: String,
}

fn tokenize_condition(input: &str) -> Option<Vec<ConditionToken>> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let chars = input.as_bytes();
    while index < chars.len() {
        let char = chars[index] as char;
        if char.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let two = input.get(index..index + 2).unwrap_or_default();
        if ["&&", "||", "==", "!=", ">=", "<="].contains(&two) {
            tokens.push(ConditionToken {
                token_type: ConditionTokenType::Operator,
                value: two.to_string(),
            });
            index += 2;
            continue;
        }
        if matches!(char, '(' | ')') {
            tokens.push(ConditionToken {
                token_type: ConditionTokenType::Paren,
                value: char.to_string(),
            });
            index += 1;
            continue;
        }
        if matches!(char, '!' | '>' | '<') {
            tokens.push(ConditionToken {
                token_type: ConditionTokenType::Operator,
                value: char.to_string(),
            });
            index += 1;
            continue;
        }
        if matches!(char, '\'' | '"') {
            let quote = char;
            let mut cursor = index + 1;
            let mut value = String::new();
            while cursor < chars.len() && (chars[cursor] as char) != quote {
                value.push(chars[cursor] as char);
                cursor += 1;
            }
            if cursor >= chars.len() {
                return None;
            }
            tokens.push(ConditionToken {
                token_type: ConditionTokenType::String,
                value,
            });
            index = cursor + 1;
            continue;
        }
        if char.is_ascii_digit() || char == '-' {
            let remainder = &input[index..];
            if let Some(matched) = remainder
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '.'))
                .collect::<String>()
                .strip_suffix('-')
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    let value = remainder
                        .chars()
                        .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '.'))
                        .collect::<String>();
                    (!value.is_empty()).then_some(value)
                })
            {
                tokens.push(ConditionToken {
                    token_type: ConditionTokenType::Number,
                    value: matched.clone(),
                });
                index += matched.len();
                continue;
            }
        }
        if char.is_ascii_alphabetic() || char == '_' {
            let value = input[index..]
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.'))
                .collect::<String>();
            let token_type = if matches!(value.as_str(), "true" | "false") {
                ConditionTokenType::Boolean
            } else {
                ConditionTokenType::Identifier
            };
            tokens.push(ConditionToken {
                token_type,
                value: value.clone(),
            });
            index += value.len();
            continue;
        }
        return None;
    }
    Some(tokens)
}

fn evaluate_property_condition(
    expression: &str,
    properties: &BTreeMap<String, Value>,
) -> Option<bool> {
    let tokens = tokenize_condition(expression)?;
    let mut index = 0usize;

    fn parse_primary(
        tokens: &[ConditionToken],
        index: &mut usize,
        properties: &BTreeMap<String, Value>,
    ) -> Option<Value> {
        let token = tokens.get(*index)?;
        if token.token_type == ConditionTokenType::Operator && token.value == "!" {
            *index += 1;
            let value = parse_primary(tokens, index, properties)?;
            return Some(Value::Bool(!truthy(&value)));
        }
        if token.token_type == ConditionTokenType::Paren && token.value == "(" {
            *index += 1;
            let value = parse_or(tokens, index, properties)?;
            let closing = tokens.get(*index)?;
            if closing.token_type != ConditionTokenType::Paren || closing.value != ")" {
                return None;
            }
            *index += 1;
            return Some(Value::Bool(truthy(&value)));
        }
        *index += 1;
        match token.token_type {
            ConditionTokenType::Boolean => Some(Value::Bool(token.value == "true")),
            ConditionTokenType::Number => token
                .value
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number),
            ConditionTokenType::String => Some(Value::String(token.value.clone())),
            ConditionTokenType::Identifier => {
                let key = token.value.strip_suffix(".value").unwrap_or(&token.value);
                Some(properties.get(key).cloned().unwrap_or(Value::Null))
            }
            _ => None,
        }
    }

    fn compare_numbers(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
        let left = value_to_f64(left)?;
        let right = value_to_f64(right)?;
        left.partial_cmp(&right)
    }

    fn parse_comparison(
        tokens: &[ConditionToken],
        index: &mut usize,
        properties: &BTreeMap<String, Value>,
    ) -> Option<Value> {
        let mut left = parse_primary(tokens, index, properties)?;
        while let Some(token) = tokens.get(*index) {
            if token.token_type != ConditionTokenType::Operator
                || !matches!(token.value.as_str(), "==" | "!=" | ">" | "<" | ">=" | "<=")
            {
                break;
            }
            let operator = token.value.clone();
            *index += 1;
            let right = parse_primary(tokens, index, properties)?;
            let result = match operator.as_str() {
                "==" => values_equal(&left, &right),
                "!=" => !values_equal(&left, &right),
                ">" => compare_numbers(&left, &right)
                    .map(|ordering| ordering.is_gt())
                    .unwrap_or(false),
                "<" => compare_numbers(&left, &right)
                    .map(|ordering| ordering.is_lt())
                    .unwrap_or(false),
                ">=" => compare_numbers(&left, &right)
                    .map(|ordering| ordering.is_ge())
                    .unwrap_or(false),
                "<=" => compare_numbers(&left, &right)
                    .map(|ordering| ordering.is_le())
                    .unwrap_or(false),
                _ => false,
            };
            left = Value::Bool(result);
        }
        Some(left)
    }

    fn parse_and(
        tokens: &[ConditionToken],
        index: &mut usize,
        properties: &BTreeMap<String, Value>,
    ) -> Option<Value> {
        let mut left = parse_comparison(tokens, index, properties)?;
        while let Some(token) = tokens.get(*index) {
            if token.token_type != ConditionTokenType::Operator || token.value != "&&" {
                break;
            }
            *index += 1;
            let right = parse_comparison(tokens, index, properties)?;
            left = Value::Bool(truthy(&left) && truthy(&right));
        }
        Some(left)
    }

    fn parse_or(
        tokens: &[ConditionToken],
        index: &mut usize,
        properties: &BTreeMap<String, Value>,
    ) -> Option<Value> {
        let mut left = parse_and(tokens, index, properties)?;
        while let Some(token) = tokens.get(*index) {
            if token.token_type != ConditionTokenType::Operator || token.value != "||" {
                break;
            }
            *index += 1;
            let right = parse_and(tokens, index, properties)?;
            left = Value::Bool(truthy(&left) || truthy(&right));
        }
        Some(left)
    }

    let result = parse_or(&tokens, &mut index, properties)?;
    if index != tokens.len() {
        return None;
    }
    Some(truthy(&result))
}

fn layer_visibility_alignment(
    _source: &SceneManifest,
    _parent_id: Option<u32>,
) -> Option<&'static str> {
    None
}

trait PrimaryVisualCandidate {
    fn primary_visual_candidate(&self, source: &SceneManifest) -> bool;
}

impl PrimaryVisualCandidate for SceneVisualLayer {
    fn primary_visual_candidate(&self, source: &SceneManifest) -> bool {
        source
            .primary_visual
            .as_ref()
            .map(|layer| layer.id == self.id)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        PropertyKind, PropertyPresentation, SceneAudioLayer, SceneBinding, SceneManifest,
        SceneNodeState, SceneNowPlayingAvailability, SceneNowPlayingSnapshot, SceneNowPlayingState,
        SceneParticleRendererFamily, SceneParticleRendererRuntime, SceneParticleRuntime,
        SceneParticleRuntimeAdapter, SceneParticleScheduleMode, SceneParticleSystemRuntime,
        SceneRenderNode, SceneRenderNodeKind, SceneRuntimeDocument, SceneTextLayer,
        WallpaperOption, WallpaperProperty,
    };
    use crate::services::scene_render_planner_service::build_scene_render_plan;
    use crate::services::scene_text_script_runtime_service::clear_scene_text_script_runtime_cache;
    use chrono::TimeZone;
    use serde_json::json;

    fn property_definitions() -> BTreeMap<String, WallpaperProperty> {
        BTreeMap::from([(
            "mode".to_string(),
            WallpaperProperty {
                key: "mode".to_string(),
                label: "Mode".to_string(),
                markup: None,
                kind: PropertyKind::Combo,
                value: Value::String("0".to_string()),
                default_value: Value::String("0".to_string()),
                min: None,
                max: None,
                step: None,
                condition: None,
                order: None,
                presentation: PropertyPresentation::Control,
                options: vec![
                    WallpaperOption {
                        label: "隐藏".to_string(),
                        value: "0".to_string(),
                    },
                    WallpaperOption {
                        label: "显示".to_string(),
                        value: "1".to_string(),
                    },
                ],
            },
        )])
    }

    fn sample_day_period_layer(content: &str, script_text: Option<&str>) -> SceneTextLayer {
        SceneTextLayer {
            id: 11,
            name: "DayPeriod".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: None,
            vertical_align: None,
            content: content.to_string(),
            behavior: SceneTextBehavior::DayPeriod,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: None,
            use_24h_format: None,
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [0.0, 0.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: None,
            render_bounds: None,
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: None,
            alpha_binding: None,
            point_size: None,
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: script_text.map(str::to_string),
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        }
    }

    fn sample_script_layer(content: &str, script_text: &str) -> SceneTextLayer {
        SceneTextLayer {
            id: 19,
            name: "Greeting".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: Some("center".to_string()),
            vertical_align: Some("center".to_string()),
            content: content.to_string(),
            behavior: SceneTextBehavior::Script,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: None,
            use_24h_format: None,
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [0.0, 0.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: None,
            render_bounds: Some([0.0, 0.0, 480.0, 120.0]),
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: Some(1.0),
            alpha_binding: None,
            point_size: Some(24.0),
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: Some(script_text.to_string()),
            script_refresh_interval_millis: None,
            padding: Some(0.0),
            max_rows: Some(2),
            max_width: Some(480.0),
            limit_width: Some(false),
            limit_use_ellipsis: Some(false),
            block_align: Some(false),
        }
    }

    fn neighboring_marker_text_layer(
        id: u32,
        horizontal_align: &str,
        origin: [f64; 3],
        size: [f64; 2],
        content: &str,
    ) -> SceneTextLayer {
        SceneTextLayer {
            id,
            name: format!("Caption {id}"),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: Some("none".to_string()),
            horizontal_align: Some(horizontal_align.to_string()),
            vertical_align: Some("center".to_string()),
            content: content.to_string(),
            behavior: SceneTextBehavior::Static,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: None,
            use_24h_format: None,
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: origin,
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: Some(size),
            render_bounds: None,
            parallax_depth: None,
            color: Some("1 1 1".to_string()),
            color_binding: None,
            alpha: Some(1.0),
            alpha_binding: None,
            point_size: Some(6.0),
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: None,
            script_refresh_interval_millis: None,
            padding: Some(32.0),
            max_rows: Some(1),
            max_width: Some(500.0),
            limit_width: Some(false),
            limit_use_ellipsis: Some(false),
            block_align: Some(false),
        }
    }

    fn sample_audio_layer(id: u32, origin: [f64; 3], size: [f64; 2]) -> SceneAudioLayer {
        SceneAudioLayer {
            id,
            name: format!("Spectrum {id}"),
            dependencies: vec![],
            parent_id: None,
            alignment: Some("center".to_string()),
            visible: true,
            visibility_binding: None,
            position: origin,
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: Some(size),
            render_bounds: None,
            angle: None,
            bar_count: 32,
            color: Some("1 1 1".to_string()),
            bar_spacing: None,
            bar_bounds: None,
            minimum_height: None,
            radius: None,
            volume_factor: None,
            opacity: Some(1.0),
        }
    }

    #[test]
    fn evaluates_relational_and_unary_condition_expressions() {
        let properties = BTreeMap::from([
            ("value".to_string(), json!(3)),
            ("enabled".to_string(), json!(true)),
        ]);
        assert_eq!(
            evaluate_property_condition("value >= 2 && !disabled", &properties),
            Some(true)
        );
        assert_eq!(
            evaluate_property_condition("value < 2 || enabled == false", &properties),
            Some(false)
        );
    }

    #[test]
    fn combo_visibility_uses_option_labels() {
        let properties = BTreeMap::from([("mode".to_string(), Value::String("0".to_string()))]);
        let defs = property_definitions();
        assert!(!binding_visible(
            true,
            Some(&SceneBinding {
                property_key: "mode".to_string(),
                condition: None,
            }),
            &properties,
            &properties,
            &defs,
        ));
    }

    #[test]
    fn direct_bool_visibility_binding_uses_current_truthiness() {
        let defs = property_definitions();

        let hidden_properties = BTreeMap::from([("prompt".to_string(), Value::Bool(false))]);
        assert!(!binding_visible(
            true,
            Some(&SceneBinding {
                property_key: "prompt".to_string(),
                condition: None,
            }),
            &hidden_properties,
            &hidden_properties,
            &defs,
        ));

        let shown_properties = BTreeMap::from([("prompt".to_string(), Value::Bool(true))]);
        assert!(binding_visible(
            true,
            Some(&SceneBinding {
                property_key: "prompt".to_string(),
                condition: None,
            }),
            &shown_properties,
            &shown_properties,
            &defs,
        ));
    }

    #[test]
    fn direct_bool_visibility_binding_does_not_invert_false_authored_value() {
        let defs = property_definitions();
        let properties = BTreeMap::from([("chineseTime".to_string(), Value::Bool(false))]);
        assert!(!binding_visible(
            false,
            Some(&SceneBinding {
                property_key: "chineseTime".to_string(),
                condition: None,
            }),
            &properties,
            &properties,
            &defs,
        ));
    }

    #[test]
    fn formats_media_title_with_fallback() {
        let layer = SceneTextLayer {
            id: 1,
            name: "Title".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: None,
            vertical_align: None,
            content: "Fallback".to_string(),
            behavior: SceneTextBehavior::MediaTitle,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: None,
            use_24h_format: None,
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [0.0, 0.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: None,
            render_bounds: None,
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: None,
            alpha_binding: None,
            point_size: None,
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: None,
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        };
        let now = Utc::now().with_timezone(&Local);
        let now_playing = SceneNowPlayingSnapshot {
            availability: SceneNowPlayingAvailability::Available,
            state: SceneNowPlayingState::Ready,
            title: Some("Now Playing".to_string()),
            artist: None,
            album: None,
            source: None,
            generation: 4,
            updated_at: Utc::now(),
            refresh_interval_millis: 1500,
            diagnostics: Vec::new(),
        };
        assert_eq!(
            resolve_text(None, &layer, &BTreeMap::new(), &now, Some(&now_playing)),
            "Now Playing"
        );
        assert_eq!(
            resolve_text(None, &layer, &BTreeMap::new(), &now, None),
            "Fallback"
        );
    }

    #[test]
    fn media_title_tracks_provider_generation_as_dynamic_input() {
        let layer = SceneTextLayer {
            id: 1,
            name: "Title".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: None,
            vertical_align: None,
            content: "Fallback".to_string(),
            behavior: SceneTextBehavior::MediaTitle,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: None,
            use_24h_format: None,
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [0.0, 0.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: None,
            render_bounds: Some([0.0, 0.0, 300.0, 80.0]),
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: None,
            alpha_binding: None,
            point_size: None,
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: None,
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        };
        let now_playing = SceneNowPlayingSnapshot {
            availability: SceneNowPlayingAvailability::Available,
            state: SceneNowPlayingState::PlayingWithoutTitle,
            generation: 9,
            refresh_interval_millis: 1500,
            updated_at: Utc::now(),
            title: None,
            artist: None,
            album: None,
            source: Some("Music".to_string()),
            diagnostics: Vec::new(),
        };
        let scene = SceneManifest {
            text_layers: vec![layer],
            ..SceneManifest::default()
        };
        let evaluated = evaluate_scene(
            &scene,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            Some(&now_playing),
            Utc::now(),
        );
        let object = evaluated.objects.get(&1).expect("text object");
        let EvaluatedSceneObject::Text { text, .. } = object else {
            panic!("expected text object");
        };

        assert_eq!(text.value, "Fallback");
        assert_eq!(text.dynamic_input_generation, Some(9));
    }

    #[test]
    fn day_period_prefers_authored_script_thresholds_for_english_layers() {
        let layer = sample_day_period_layer(
            "Before dawn",
            Some(
                "'use strict'; let shichenY = {1:'Before dawn',2:'At night',3:'Morning',4:'Morning',5:'Noon',6:'Afternoon',7:'Evening',8:'Night'}; export function update(value) { let hour = new Date().getHours(); let timeTag = hour< 2 ?'lingchen':hour< 6 ?'yejian':hour< 8 ?'zaochen':hour< 11 ?'shangwu':hour < 13 ?'zhongwu':hour< 17 ?'xiawu':hour< 20 ?'bangwan':'wanshang'; switch (timeTag) { case 'lingchen' : value = shichenY[1] ;break; case 'yejian' : value = shichenY[2] ;break; case 'zaochen' : value = shichenY[3] ;break; case 'shangwu' : value = shichenY[4] ;break; case 'zhongwu' : value = shichenY[5] ;break; case 'xiawu' : value = shichenY[6] ;break; case 'bangwan' : value = shichenY[7] ;break; case 'wanshang' : value = shichenY[8] ;break; } return value; }",
            ),
        );
        let now = Local
            .with_ymd_and_hms(2026, 4, 13, 3, 0, 0)
            .single()
            .expect("local time");

        assert_eq!(
            resolve_text(None, &layer, &BTreeMap::new(), &now, None),
            "At night"
        );
    }

    #[test]
    fn day_period_prefers_authored_script_thresholds_for_chinese_layers() {
        let layer = sample_day_period_layer(
            "凌晨",
            Some(
                "'use strict'; let shichenC = {1:'凌晨',2:'夜间',3:'早晨',4:'上午',5:'中午',6:'下午',7:'傍晚',8:'晚上'}; export function update(value) { let hour = new Date().getHours(); let timeTag = hour< 2 ?'lingchen':hour< 6 ?'yejian':hour< 8 ?'zaochen':hour< 11 ?'shangwu':hour < 13 ?'zhongwu':hour< 17 ?'xiawu':hour< 20 ?'bangwan':'wanshang'; switch (timeTag) { case 'lingchen' : value = shichenC[1] ;break; case 'yejian' : value = shichenC[2] ;break; case 'zaochen' : value = shichenC[3] ;break; case 'shangwu' : value = shichenC[4] ;break; case 'zhongwu' : value = shichenC[5] ;break; case 'xiawu' : value = shichenC[6] ;break; case 'bangwan' : value = shichenC[7] ;break; case 'wanshang' : value = shichenC[8] ;break; } return value; }",
            ),
        );
        let now = Local
            .with_ymd_and_hms(2026, 4, 13, 3, 0, 0)
            .single()
            .expect("local time");

        assert_eq!(
            resolve_text(None, &layer, &BTreeMap::new(), &now, None),
            "夜间"
        );
    }

    #[test]
    fn day_period_default_mapping_matches_shared_phase_08_ranges() {
        let layer = sample_day_period_layer("Before dawn", None);
        let now = Local
            .with_ymd_and_hms(2026, 4, 13, 3, 0, 0)
            .single()
            .expect("local time");

        assert_eq!(
            resolve_text(None, &layer, &BTreeMap::new(), &now, None),
            "At night"
        );
    }

    #[test]
    fn scene_visibility_respects_hidden_parent_binding() {
        let source = SceneManifest {
            nodes: vec![SceneNodeState {
                id: 10,
                name: "Parent".to_string(),
                dependencies: vec![],
                parent_id: None,
                visible: true,
                visibility_binding: Some(SceneBinding {
                    property_key: "mode".to_string(),
                    condition: Some("0".to_string()),
                }),
                position: [0.0, 0.0, 0.0],
                position_bindings: None,
                scale: [1.0, 1.0, 1.0],
                angles: None,
                rotation: None,
            }],
            ..SceneManifest::default()
        };
        let properties = BTreeMap::from([("mode".to_string(), Value::String("1".to_string()))]);
        assert!(!source_object_visible(
            &source,
            Some(10),
            &properties,
            &properties,
            &property_definitions(),
        ));
    }

    #[test]
    fn render_list_falls_back_to_visible_visual_layers_when_render_graph_is_incomplete() {
        let source = SceneManifest {
            visual_layers: vec![SceneVisualLayer {
                id: 42,
                name: "Background".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: Some("center".to_string()),
                visible: true,
                visibility_binding: None,
                position: [1920.0, 1080.0, 0.0],
                position_bindings: None,
                scale: [1.0, 1.0, 1.0],
                scale_binding: None,
                angles: None,
                size: Some([3840.0, 2160.0]),
                intrinsic_size: Some([3840.0, 2160.0]),
                parallax_depth: None,
                parallax_depth_binding: None,
                rotation: None,
                opacity: Some(1.0),
                opacity_binding: None,
                color: None,
                color_binding: None,
                brightness: None,
                brightness_binding: None,
                color_blend_mode: None,
                render_bounds: Some([0.0, 0.0, 3840.0, 2160.0]),
                fullscreen: true,
                autosize: false,
                solid_layer: false,
                passthrough: false,
                no_padding: false,
                model_width: None,
                model_height: None,
                puppet_path: None,
                animation_layers: vec![],
                effect_instances: vec![],
                model_path: Some("/tmp/background.png".to_string()),
                material_path: None,
                shader_path: None,
                texture_names: vec![],
                asset_kind: crate::models::SceneAssetKind::Image,
                asset_path: Some("/tmp/background.png".to_string()),
                system_texture_key: None,
                blend_mode: None,
            }],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        assert_eq!(evaluated.render_list, vec![42]);
        assert!(matches!(
            evaluated.objects.get(&42),
            Some(EvaluatedSceneObject::Visual { .. })
        ));
    }

    #[test]
    fn render_list_includes_first_class_particle_runtime_without_legacy_layer() {
        let source = SceneManifest {
            nodes: vec![SceneNodeState {
                id: 49,
                name: "Sprite Runtime".to_string(),
                dependencies: vec![],
                parent_id: None,
                visible: true,
                visibility_binding: Some(SceneBinding {
                    property_key: "mode".to_string(),
                    condition: Some("1".to_string()),
                }),
                position: [1311.0, 2.0, 0.0],
                position_bindings: None,
                scale: [1.0, 1.0, 1.0],
                angles: None,
                rotation: None,
            }],
            particle_runtimes: vec![SceneParticleRuntime {
                object_id: 49,
                object_name: "Sprite Runtime".to_string(),
                particle_path: "particles/sprite.json".to_string(),
                object_origin: [1311.0, 2.0, 0.0],
                system: SceneParticleSystemRuntime {
                    renderers: vec![SceneParticleRendererRuntime {
                        family: SceneParticleRendererFamily::Sprite,
                        name: Some("sprite".to_string()),
                        ..SceneParticleRendererRuntime::default()
                    }],
                    ..SceneParticleSystemRuntime::default()
                },
                adapter: SceneParticleRuntimeAdapter {
                    supported: true,
                    draw_kind: None,
                    schedule_mode: SceneParticleScheduleMode::Autonomous,
                    reason: None,
                },
                ..SceneParticleRuntime::default()
            }],
            ..SceneManifest::default()
        };
        let properties = BTreeMap::from([("mode".to_string(), Value::String("1".to_string()))]);

        let evaluated = evaluate_scene(
            &source,
            &properties,
            &properties,
            &property_definitions(),
            None,
            Utc::now(),
        );

        assert_eq!(evaluated.render_list, vec![49]);
        let Some(EvaluatedSceneObject::Particle { base, .. }) = evaluated.objects.get(&49) else {
            panic!("first-class particle runtime should evaluate as a renderable particle object");
        };
        assert!(base.visible);
        assert_eq!(base.transform.position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn evaluates_visual_position_and_scale_bindings_into_transform() {
        let source = SceneManifest {
            canvas_width: Some(200.0),
            canvas_height: Some(100.0),
            visual_layers: vec![SceneVisualLayer {
                id: 7,
                name: "Bound Visual".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: Some("center".to_string()),
                visible: true,
                visibility_binding: None,
                position: [50.0, 50.0, 0.0],
                position_bindings: Some(SceneAxisBindings {
                    x: Some("visualX".to_string()),
                    y: Some("visualY".to_string()),
                }),
                scale: [1.0, 1.0, 1.0],
                scale_binding: Some("visualScale".to_string()),
                angles: None,
                size: Some([20.0, 10.0]),
                intrinsic_size: None,
                parallax_depth: None,
                parallax_depth_binding: None,
                rotation: None,
                opacity: Some(1.0),
                opacity_binding: None,
                color: None,
                color_binding: None,
                brightness: None,
                brightness_binding: None,
                color_blend_mode: None,
                render_bounds: None,
                fullscreen: false,
                autosize: false,
                solid_layer: false,
                passthrough: false,
                no_padding: false,
                model_width: None,
                model_height: None,
                puppet_path: None,
                animation_layers: vec![],
                effect_instances: vec![],
                model_path: None,
                material_path: None,
                shader_path: None,
                texture_names: vec![],
                asset_kind: crate::models::SceneAssetKind::Image,
                asset_path: Some("/tmp/bound.png".to_string()),
                system_texture_key: None,
                blend_mode: None,
            }],
            render_graph: vec![SceneRenderNode {
                id: 7,
                name: "Bound Visual".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Sprite,
                visible: true,
                asset_path: Some("/tmp/bound.png".to_string()),
                material_path: None,
            }],
            ..SceneManifest::default()
        };

        let properties = BTreeMap::from([
            ("visualX".to_string(), json!(0.5)),
            ("visualY".to_string(), json!(0.25)),
            ("visualScale".to_string(), json!(2.0)),
        ]);

        let evaluated = evaluate_scene(
            &source,
            &properties,
            &properties,
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&7) {
            Some(EvaluatedSceneObject::Visual { base, .. }) => {
                assert_eq!(base.transform.position, [100.0, 25.0, 0.0]);
                assert_eq!(base.transform.scale, [2.0, 2.0, 2.0]);
                assert_eq!(base.transform.render_bounds, Some([80.0, 65.0, 40.0, 20.0]));
            }
            other => panic!("expected visual object, got {other:?}"),
        }
    }

    #[test]
    fn text_scale_binding_affects_render_bounds_and_content_box() {
        let mut text = neighboring_marker_text_layer(
            17,
            "left",
            [200.0, 150.0, 0.0],
            [100.0, 40.0],
            "Scaled text",
        );
        text.alignment = Some("center".to_string());
        text.horizontal_align = Some("left".to_string());
        text.vertical_align = Some("top".to_string());
        text.scale_binding = Some("textScale".to_string());
        text.padding = Some(10.0);
        text.block_align = Some(true);
        text.point_size = Some(20.0);
        let source = SceneManifest {
            canvas_width: Some(400.0),
            canvas_height: Some(300.0),
            text_layers: vec![text],
            ..SceneManifest::default()
        };
        let properties = BTreeMap::from([("textScale".to_string(), json!("2 3 1"))]);

        let evaluated = evaluate_scene(
            &source,
            &properties,
            &properties,
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&17) {
            Some(EvaluatedSceneObject::Text { base, text, .. }) => {
                assert_eq!(base.transform.scale, [2.0, 3.0, 1.0]);
                assert_eq!(
                    base.transform.render_bounds,
                    Some([100.0, 90.0, 200.0, 120.0])
                );
                assert_eq!(text.layout.world_scale, [2.0, 3.0, 1.0]);
                assert_eq!(text.layout.scaled_padding, 30.0);
                let content = text.layout.content_bounds.expect("content bounds");
                assert_eq!(content[0], 130.0);
                assert_eq!(content[1], 120.0);
            }
            other => panic!("expected text object, got {other:?}"),
        }
    }

    #[test]
    fn blockalign_false_text_uses_authored_box_without_padding_inset() {
        let mut padded =
            neighboring_marker_text_layer(118, "left", [100.0, 150.0, 0.0], [300.0, 60.0], "READY");
        padded.padding = Some(32.0);
        padded.block_align = Some(true);
        padded.point_size = Some(12.0);

        let mut unpadded = padded.clone();
        unpadded.id = 119;
        unpadded.name = "Caption 119".to_string();
        unpadded.position = [100.0, 70.0, 0.0];
        unpadded.block_align = Some(false);

        let source = SceneManifest {
            canvas_width: Some(500.0),
            canvas_height: Some(300.0),
            text_layers: vec![padded, unpadded],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );
        let Some(EvaluatedSceneObject::Text {
            text: padded_text, ..
        }) = evaluated.objects.get(&118)
        else {
            panic!("expected padded text object");
        };
        let Some(EvaluatedSceneObject::Text {
            text: unpadded_text,
            ..
        }) = evaluated.objects.get(&119)
        else {
            panic!("expected unpadded text object");
        };

        let [padded_x, _, _, _] = padded_text.layout.content_bounds.expect("content bounds");
        let [unpadded_x, _, _, _] = unpadded_text.layout.content_bounds.expect("content bounds");

        assert_eq!(padded_text.layout.scaled_padding, 32.0);
        assert_eq!(unpadded_text.layout.scaled_padding, 0.0);
        assert!((padded_x - 132.0).abs() < 0.001);
        assert!((unpadded_x - 100.0).abs() < 0.001);
        assert!(unpadded_text.layout.scaled_point_size > padded_text.layout.scaled_point_size);
    }

    #[test]
    fn rotated_text_uses_authored_rotation_in_transform_and_quad() {
        let mut text = neighboring_marker_text_layer(
            18,
            "center",
            [200.0, 150.0, 0.0],
            [120.0, 60.0],
            "Rotated",
        );
        text.angles = Some([0.0, 0.0, 0.625]);
        text.rotation = Some(0.625);
        let source = SceneManifest {
            canvas_width: Some(400.0),
            canvas_height: Some(300.0),
            text_layers: vec![text],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );
        let runtime = SceneRuntimeDocument {
            runtime_owner_key: None,
            source,
            evaluated,
            now_playing: Default::default(),
        };
        let report = build_scene_render_plan(&runtime);

        match runtime.evaluated.objects.get(&18) {
            Some(EvaluatedSceneObject::Text { base, .. }) => {
                assert_eq!(base.transform.rotation, 0.625);
            }
            other => panic!("expected text object, got {other:?}"),
        }
        assert!(!report.is_blocked());
        assert_eq!(report.plan.texts.len(), 1);
        assert_eq!(report.plan.texts[0].quad.rotation, 0.625);
    }

    #[test]
    fn text_and_audio_transform_bindings_compose_with_parent() {
        let mut text =
            neighboring_marker_text_layer(51, "left", [10.0, 5.0, 0.0], [20.0, 10.0], "Child");
        text.parent_id = Some(50);
        text.alignment = Some("left-bottom".to_string());
        text.scale_binding = Some("textScale".to_string());
        let mut audio = sample_audio_layer(52, [15.0, 6.0, 0.0], [30.0, 10.0]);
        audio.parent_id = Some(50);
        audio.scale_binding = Some("audioScale".to_string());
        let source = SceneManifest {
            canvas_width: Some(300.0),
            canvas_height: Some(200.0),
            nodes: vec![SceneNodeState {
                id: 50,
                name: "Parent".to_string(),
                dependencies: vec![],
                parent_id: None,
                visible: true,
                visibility_binding: None,
                position: [40.0, 30.0, 0.0],
                position_bindings: None,
                scale: [2.0, 3.0, 1.0],
                angles: None,
                rotation: None,
            }],
            text_layers: vec![text],
            audio_layers: vec![audio],
            ..SceneManifest::default()
        };
        let properties = BTreeMap::from([
            ("textScale".to_string(), json!("1.5 2 1")),
            ("audioScale".to_string(), json!(2.0)),
        ]);

        let evaluated = evaluate_scene(
            &source,
            &properties,
            &properties,
            &BTreeMap::new(),
            None,
            Utc::now(),
        );
        let runtime = SceneRuntimeDocument {
            runtime_owner_key: None,
            source,
            evaluated,
            now_playing: Default::default(),
        };
        let report = build_scene_render_plan(&runtime);

        match runtime.evaluated.objects.get(&51) {
            Some(EvaluatedSceneObject::Text { base, .. }) => {
                assert_eq!(base.transform.position, [60.0, 45.0, 0.0]);
                assert_eq!(base.transform.scale, [3.0, 6.0, 1.0]);
                assert_eq!(base.transform.render_bounds, Some([60.0, 95.0, 60.0, 60.0]));
            }
            other => panic!("expected text object, got {other:?}"),
        }
        match runtime.evaluated.objects.get(&52) {
            Some(EvaluatedSceneObject::Audio { base, .. }) => {
                assert_eq!(base.transform.position, [70.0, 48.0, 0.0]);
                assert_eq!(base.transform.scale, [4.0, 6.0, 2.0]);
                assert_eq!(
                    base.transform.render_bounds,
                    Some([10.0, 122.0, 120.0, 60.0])
                );
            }
            other => panic!("expected audio object, got {other:?}"),
        }
        assert_eq!(report.plan.audios.len(), 1);
        assert_eq!(report.plan.audios[0].quad.left, 10.0);
        assert_eq!(report.plan.audios[0].quad.top, 122.0);
        assert_eq!(report.plan.audios[0].quad.width, 120.0);
        assert_eq!(report.plan.audios[0].quad.height, 60.0);
    }

    #[test]
    fn evaluates_visual_runtime_metadata_from_bound_source_fields() {
        let source = SceneManifest {
            canvas_width: Some(200.0),
            canvas_height: Some(100.0),
            visual_layers: vec![SceneVisualLayer {
                id: 11,
                name: "Metadata Visual".to_string(),
                dependencies: vec![4, 8],
                parent_id: None,
                alignment: Some("center".to_string()),
                visible: true,
                visibility_binding: None,
                position: [100.0, 50.0, 0.0],
                position_bindings: None,
                scale: [1.0, 1.0, 1.0],
                scale_binding: None,
                angles: Some([1.0, 2.0, 3.0]),
                size: Some([50.0, 25.0]),
                intrinsic_size: None,
                parallax_depth: Some([0.3, 0.4]),
                parallax_depth_binding: Some("depth".to_string()),
                rotation: Some(3.0),
                opacity: Some(1.0),
                opacity_binding: Some("alpha".to_string()),
                color: Some("1 1 1".to_string()),
                color_binding: Some("tint".to_string()),
                brightness: Some(1.0),
                brightness_binding: Some("brightness".to_string()),
                color_blend_mode: Some(2),
                render_bounds: None,
                fullscreen: false,
                autosize: true,
                solid_layer: false,
                passthrough: true,
                no_padding: true,
                model_width: Some(50.0),
                model_height: Some(25.0),
                puppet_path: Some("models/hero_puppet.mdl".to_string()),
                animation_layers: vec![crate::models::SceneAnimationLayer {
                    id: 90,
                    rate: 1.2,
                    visible: true,
                    visibility_binding: None,
                    blend: "normal".to_string(),
                    animation: "idle".to_string(),
                }],
                effect_instances: vec![],
                model_path: None,
                material_path: None,
                shader_path: None,
                texture_names: vec![],
                asset_kind: crate::models::SceneAssetKind::Image,
                asset_path: Some("/tmp/meta.png".to_string()),
                system_texture_key: None,
                blend_mode: None,
            }],
            ..SceneManifest::default()
        };

        let properties = BTreeMap::from([
            ("alpha".to_string(), json!(0.45)),
            ("tint".to_string(), json!("0.1 0.2 0.3")),
            ("brightness".to_string(), json!(1.7)),
            ("depth".to_string(), json!("0.9 0.1")),
        ]);

        let evaluated = evaluate_scene(
            &source,
            &properties,
            &properties,
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&11) {
            Some(EvaluatedSceneObject::Visual {
                base,
                color,
                brightness,
                color_blend_mode,
                parallax_depth,
                angles,
                autosize,
                passthrough,
                no_padding,
                puppet_path,
                animation_layers,
                ..
            }) => {
                assert_eq!(base.dependencies, vec![4, 8]);
                assert!((base.opacity - 0.45).abs() < f64::EPSILON);
                assert_eq!(color.as_deref(), Some("0.1 0.2 0.3"));
                assert_eq!(*brightness, Some(1.7));
                assert_eq!(*color_blend_mode, Some(2));
                assert_eq!(*parallax_depth, Some([0.9, 0.1]));
                assert_eq!(*angles, Some([1.0, 2.0, 3.0]));
                assert!(*autosize);
                assert!(*passthrough);
                assert!(*no_padding);
                assert_eq!(puppet_path.as_deref(), Some("models/hero_puppet.mdl"));
                assert_eq!(animation_layers.len(), 1);
                assert_eq!(animation_layers[0].id, 90);
            }
            other => panic!("expected visual object, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_scene_builds_camera_and_text_runtime_from_current_inputs() {
        let mut source = SceneManifest::default();
        source.camera.zoom_binding = Some("zoom".to_string());
        source.camera.center = [-120.0, -45.0];
        source.text_layers = vec![SceneTextLayer {
            id: 7,
            name: "Clock".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: Some("center".to_string()),
            anchor: None,
            horizontal_align: Some("center".to_string()),
            vertical_align: Some("center".to_string()),
            content: "Clock".to_string(),
            behavior: SceneTextBehavior::Clock,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: Some(true),
            use_24h_format: Some(true),
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [1920.0, 1080.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: Some([400.0, 120.0]),
            render_bounds: Some([1720.0, 1020.0, 400.0, 120.0]),
            parallax_depth: None,
            color: Some("1 1 1".to_string()),
            color_binding: None,
            alpha: Some(1.0),
            alpha_binding: None,
            point_size: Some(80.0),
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: None,
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        }];
        source.render_graph = vec![SceneRenderNode {
            id: 7,
            name: "Clock".to_string(),
            parent_id: None,
            kind: SceneRenderNodeKind::Text,
            visible: true,
            asset_path: None,
            material_path: None,
        }];

        let properties = BTreeMap::from([("zoom".to_string(), json!(1.35))]);
        let now = Utc.with_ymd_and_hms(2026, 4, 4, 8, 9, 10).unwrap();
        let evaluated = evaluate_scene(
            &source,
            &properties,
            &properties,
            &BTreeMap::new(),
            None,
            now,
        );
        let expected_clock = now.with_timezone(&Local).format("%H:%M:%S").to_string();

        assert_eq!(evaluated.camera.zoom, 1.35);
        assert_eq!(evaluated.camera.center, [-120.0, -45.0]);
        assert_eq!(evaluated.render_list, vec![7]);
        let object = evaluated.objects.get(&7).expect("clock text object");
        match object {
            EvaluatedSceneObject::Text { text, .. } => {
                assert_eq!(text.value, expected_clock);
                let [render_x, render_y, render_w, render_h] =
                    text.layout.render_bounds.expect("render bounds");
                let [content_x, content_y, content_w, content_h] =
                    text.layout.content_bounds.expect("content bounds");
                assert_eq!(
                    [render_x, render_y, render_w, render_h],
                    [1720.0, 1020.0, 400.0, 120.0]
                );
                assert!(text.layout.scaled_point_size > 80.0);
                assert!(content_w >= render_w * 0.9);
                assert!(content_h >= render_h * 0.8);
                assert!(((content_x + content_w * 0.5) - (render_x + render_w * 0.5)).abs() < 1.0);
                assert!(((content_y + content_h * 0.5) - (render_y + render_h * 0.5)).abs() < 1.0);
                assert_eq!(text.layout.scaled_padding, 0.0);
                assert_eq!(text.layout.world_scale, [1.0, 1.0, 1.0]);
            }
            other => panic!("expected text object, got {other:?}"),
        }
    }

    #[test]
    fn evaluates_scaled_text_layout_from_world_scale_and_authored_size() {
        let source = SceneManifest {
            canvas_width: Some(3840.0),
            canvas_height: Some(2160.0),
            text_layers: vec![SceneTextLayer {
                id: 99,
                name: "Date".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: Some("center".to_string()),
                anchor: None,
                horizontal_align: Some("center".to_string()),
                vertical_align: Some("center".to_string()),
                content: "<Date>".to_string(),
                behavior: SceneTextBehavior::Date,
                delimiter: Some("/".to_string()),
                month_format: Some("2".to_string()),
                day_format: Some("1".to_string()),
                show_day: Some(false),
                align_vertical: Some(false),
                use_delimiter: Some(true),
                show_seconds: None,
                use_24h_format: None,
                visible: true,
                visibility_binding: None,
                text_binding: None,
                position: [1315.24268, 1419.55127, 0.0],
                position_bindings: None,
                scale: [0.18, 0.18, 0.44841],
                scale_binding: None,
                angles: None,
                rotation: None,
                size: Some([1971.0, 671.0]),
                render_bounds: None,
                parallax_depth: None,
                color: Some("1 1 1".to_string()),
                color_binding: None,
                alpha: Some(1.0),
                alpha_binding: None,
                point_size: Some(140.0),
                point_size_binding: None,
                font_reference: None,
                font_path: None,
                effect_paths: vec![],
                script_text: None,
                script_refresh_interval_millis: None,
                padding: Some(32.0),
                max_rows: Some(1),
                max_width: Some(500.0),
                limit_width: None,
                limit_use_ellipsis: None,
                block_align: None,
            }],
            render_graph: vec![SceneRenderNode {
                id: 99,
                name: "Date".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Text,
                visible: true,
                asset_path: None,
                material_path: None,
            }],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&99) {
            Some(EvaluatedSceneObject::Text { text, base, .. }) => {
                let [render_x, render_y, render_w, render_h] =
                    base.transform.render_bounds.expect("render bounds");
                assert_eq!(
                    [render_x, render_y, render_w, render_h],
                    [1137.85268, 680.0587300000002, 354.78, 120.78]
                );
                assert!(text.layout.scaled_point_size >= 6.0);
                assert!((text.layout.scaled_padding - 5.76).abs() < 0.001);
                assert_eq!(text.layout.world_scale, [0.18, 0.18, 0.44841]);
                let [x, y, w, h] = text.layout.content_bounds.expect("content bounds");
                assert!(w > 0.0);
                assert!(h > 0.0);
                assert!(h >= render_h * 0.8);
                assert!(((x + w * 0.5) - (render_x + render_w * 0.5)).abs() < 1.0);
                assert!(((y + h * 0.5) - (render_y + render_h * 0.5)).abs() < 1.0);
            }
            other => panic!("expected text object, got {other:?}"),
        }
    }

    #[test]
    fn text_layout_fits_natural_text_size_into_evaluated_render_box() {
        let source = SceneManifest {
            canvas_width: Some(3840.0),
            canvas_height: Some(2160.0),
            text_layers: vec![SceneTextLayer {
                id: 301,
                name: "Clock".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: Some("center".to_string()),
                anchor: None,
                horizontal_align: Some("left".to_string()),
                vertical_align: Some("center".to_string()),
                content: "00:00:00".to_string(),
                behavior: SceneTextBehavior::Clock,
                delimiter: None,
                month_format: None,
                day_format: None,
                show_day: None,
                align_vertical: None,
                use_delimiter: None,
                show_seconds: Some(true),
                use_24h_format: Some(true),
                visible: true,
                visibility_binding: None,
                text_binding: None,
                position: [2201.59473, 1538.3949, 0.0],
                position_bindings: None,
                scale: [0.97861, 0.97861, 0.97861],
                scale_binding: None,
                angles: None,
                rotation: None,
                size: Some([479.0, 220.0]),
                render_bounds: Some([1962.09473, 511.6051, 479.0, 220.0]),
                parallax_depth: None,
                color: Some("1 1 1".to_string()),
                color_binding: None,
                alpha: Some(1.0),
                alpha_binding: None,
                point_size: Some(32.0),
                point_size_binding: None,
                font_reference: None,
                font_path: None,
                effect_paths: vec![],
                script_text: None,
                script_refresh_interval_millis: None,
                padding: Some(32.0),
                max_rows: Some(1),
                max_width: Some(500.0),
                limit_width: Some(false),
                limit_use_ellipsis: Some(false),
                block_align: Some(false),
            }],
            render_graph: vec![SceneRenderNode {
                id: 301,
                name: "Clock".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Text,
                visible: true,
                asset_path: None,
                material_path: None,
            }],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&301) {
            Some(EvaluatedSceneObject::Text { text, .. }) => {
                let [content_x, content_y, content_w, content_h] =
                    text.layout.content_bounds.expect("content bounds");
                let [render_x, render_y, render_w, render_h] =
                    text.layout.render_bounds.expect("render bounds");
                assert!(text.layout.scaled_point_size > 100.0);
                assert!(content_w >= render_w * 0.95);
                assert!(content_h >= render_h * 0.65);
                assert!(content_x <= render_x + 40.0);
                assert!(((content_y + content_h * 0.5) - (render_y + render_h * 0.5)).abs() < 1.0);
            }
            other => panic!("expected text object, got {other:?}"),
        }
    }

    #[test]
    fn dynamic_text_layout_ignores_max_width_when_render_box_is_already_evaluated() {
        let source = SceneManifest {
            canvas_width: Some(3840.0),
            canvas_height: Some(2160.0),
            text_layers: vec![SceneTextLayer {
                id: 401,
                name: "Clock".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: Some("center".to_string()),
                anchor: None,
                horizontal_align: Some("center".to_string()),
                vertical_align: Some("center".to_string()),
                content: "00:00".to_string(),
                behavior: SceneTextBehavior::Clock,
                delimiter: None,
                month_format: None,
                day_format: None,
                show_day: None,
                align_vertical: None,
                use_delimiter: None,
                show_seconds: Some(false),
                use_24h_format: Some(true),
                visible: true,
                visibility_binding: None,
                text_binding: None,
                position: [2408.30518, 971.39758, 0.0],
                position_bindings: None,
                scale: [0.28038, 0.28038, 0.28038],
                scale_binding: None,
                angles: None,
                rotation: None,
                size: Some([2118.0, 1047.0]),
                render_bounds: Some([2309.0, 919.0, 196.0, 97.0]),
                parallax_depth: None,
                color: Some("1 1 1".to_string()),
                color_binding: None,
                alpha: Some(1.0),
                alpha_binding: None,
                point_size: Some(202.26199),
                point_size_binding: None,
                font_reference: None,
                font_path: None,
                effect_paths: vec![],
                script_text: Some("clock-script".to_string()),
                script_refresh_interval_millis: None,
                padding: Some(32.0),
                max_rows: Some(1),
                max_width: Some(500.0),
                limit_width: Some(false),
                limit_use_ellipsis: Some(false),
                block_align: Some(false),
            }],
            render_graph: vec![SceneRenderNode {
                id: 401,
                name: "Clock".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Text,
                visible: true,
                asset_path: None,
                material_path: None,
            }],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&401) {
            Some(EvaluatedSceneObject::Text { text, .. }) => {
                let [render_x, _, render_w, _] = text.layout.render_bounds.expect("render bounds");
                let [content_x, _, content_w, _] =
                    text.layout.content_bounds.expect("content bounds");
                assert!(text.layout.scaled_point_size > 70.0);
                assert!(content_w > render_w);
                assert!(content_x < render_x);
            }
            other => panic!("expected text object, got {other:?}"),
        }
    }

    #[test]
    fn static_text_layout_uses_render_box_width_without_local_scale_reshrink() {
        let source = SceneManifest {
            canvas_width: Some(3840.0),
            canvas_height: Some(2160.0),
            text_layers: vec![SceneTextLayer {
                id: 402,
                name: "Custom".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: Some("center".to_string()),
                anchor: None,
                horizontal_align: Some("left".to_string()),
                vertical_align: Some("center".to_string()),
                content: "Bilibili/抖音 夜莺Night".to_string(),
                behavior: SceneTextBehavior::Static,
                delimiter: None,
                month_format: None,
                day_format: None,
                show_day: None,
                align_vertical: None,
                use_delimiter: None,
                show_seconds: None,
                use_24h_format: None,
                visible: true,
                visibility_binding: None,
                text_binding: Some("newproperty55".to_string()),
                position: [1771.77002, 863.14062, 0.0],
                position_bindings: None,
                scale: [0.3684, 0.3684, 0.614],
                scale_binding: None,
                angles: None,
                rotation: None,
                size: Some([2439.0, 346.0]),
                render_bounds: Some([1530.0, 830.0, 483.0, 68.0]),
                parallax_depth: None,
                color: Some("1 1 1".to_string()),
                color_binding: None,
                alpha: Some(1.0),
                alpha_binding: None,
                point_size: Some(54.411999),
                point_size_binding: None,
                font_reference: None,
                font_path: None,
                effect_paths: vec![],
                script_text: None,
                script_refresh_interval_millis: None,
                padding: Some(32.0),
                max_rows: Some(1),
                max_width: Some(500.0),
                limit_width: Some(false),
                limit_use_ellipsis: Some(false),
                block_align: Some(false),
            }],
            render_graph: vec![SceneRenderNode {
                id: 402,
                name: "Custom".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Text,
                visible: true,
                asset_path: None,
                material_path: None,
            }],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&402) {
            Some(EvaluatedSceneObject::Text { text, .. }) => {
                let [render_x, _, render_w, _] = text.layout.render_bounds.expect("render bounds");
                let [content_x, _, content_w, _] =
                    text.layout.content_bounds.expect("content bounds");
                assert!(text.layout.scaled_point_size > 35.0);
                assert!(content_w >= render_w * 0.9);
                assert!((content_x - render_x).abs() < 16.0);
            }
            other => panic!("expected text object, got {other:?}"),
        }
    }

    #[test]
    fn text_anchor_none_keeps_content_box_to_right_of_neighboring_marker() {
        let neighboring_marker_right = 1115.0;
        let source = SceneManifest {
            canvas_width: Some(1920.0),
            canvas_height: Some(1080.0),
            text_layers: vec![
                neighboring_marker_text_layer(
                    501,
                    "left",
                    [1140.0, 441.0, 0.0],
                    [275.0, 30.0],
                    "Short caption",
                ),
                neighboring_marker_text_layer(
                    502,
                    "right",
                    [1620.0, 401.0, 0.0],
                    [475.0, 30.0],
                    "Longer caption beside marker",
                ),
            ],
            render_graph: vec![
                SceneRenderNode {
                    id: 501,
                    name: "Caption 501".to_string(),
                    parent_id: None,
                    kind: SceneRenderNodeKind::Text,
                    visible: true,
                    asset_path: None,
                    material_path: None,
                },
                SceneRenderNode {
                    id: 502,
                    name: "Caption 502".to_string(),
                    parent_id: None,
                    kind: SceneRenderNodeKind::Text,
                    visible: true,
                    asset_path: None,
                    material_path: None,
                },
            ],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene(
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        for id in [501, 502] {
            match evaluated.objects.get(&id) {
                Some(EvaluatedSceneObject::Text { text, base, .. }) => {
                    let [render_x, _, _, _] = base.transform.render_bounds.expect("render bounds");
                    let [content_x, _, _, _] = text.layout.content_bounds.expect("content bounds");
                    assert!(render_x >= neighboring_marker_right);
                    assert!(content_x >= neighboring_marker_right);
                    assert!(content_x >= render_x);
                }
                other => panic!("expected text object {id}, got {other:?}"),
            }
        }
    }

    #[test]
    fn centered_script_text_keeps_authored_box_position_and_width_fit() {
        clear_scene_text_script_runtime_cache();
        let source = SceneManifest {
            canvas_width: Some(1920.0),
            canvas_height: Some(1080.0),
            text_layers: vec![SceneTextLayer {
                id: 601,
                name: "Centered Script Caption".to_string(),
                dependencies: vec![],
                parent_id: None,
                alignment: None,
                anchor: Some("none".to_string()),
                horizontal_align: Some("center".to_string()),
                vertical_align: Some("center".to_string()),
                content: "Placeholder Text\n<Name>".to_string(),
                behavior: SceneTextBehavior::Script,
                delimiter: None,
                month_format: None,
                day_format: None,
                show_day: None,
                align_vertical: None,
                use_delimiter: None,
                show_seconds: None,
                use_24h_format: None,
                visible: true,
                visibility_binding: None,
                text_binding: None,
                position: [960.0, 180.0, 0.0],
                position_bindings: None,
                scale: [1.0, 1.0, 1.0],
                scale_binding: None,
                angles: None,
                rotation: None,
                size: Some([360.0, 84.0]),
                render_bounds: None,
                parallax_depth: None,
                color: Some("1 1 1".to_string()),
                color_binding: None,
                alpha: Some(1.0),
                alpha_binding: None,
                point_size: Some(7.0),
                point_size_binding: None,
                font_reference: None,
                font_path: None,
                effect_paths: vec![],
                script_text: Some(
                    "export function update() { thisLayer.text = 'Have a nice evening, USER!'; }"
                        .to_string(),
                ),
                script_refresh_interval_millis: None,
                padding: Some(0.0),
                max_rows: None,
                max_width: None,
                limit_width: Some(false),
                limit_use_ellipsis: Some(false),
                block_align: Some(false),
            }],
            render_graph: vec![SceneRenderNode {
                id: 601,
                name: "Centered Script Caption".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Text,
                visible: true,
                asset_path: None,
                material_path: None,
            }],
            ..SceneManifest::default()
        };

        let evaluated = evaluate_scene_with_runtime_key(
            Some("centered-script-width-fit"),
            &source,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            Utc::now(),
        );

        match evaluated.objects.get(&601) {
            Some(EvaluatedSceneObject::Text { text, base, .. }) => {
                let [render_x, render_y, render_w, render_h] =
                    base.transform.render_bounds.expect("render bounds");
                assert_eq!(
                    [render_x, render_y, render_w, render_h],
                    [780.0, 858.0, 360.0, 84.0]
                );
                assert_eq!(text.value, "Have a nice evening, USER!");
                assert!(text.layout.scaled_point_size < 40.0);
                let [content_x, _, content_w, _] =
                    text.layout.content_bounds.expect("content bounds");
                assert!(content_w <= render_w);
                assert!(((content_x + content_w * 0.5) - (render_x + render_w * 0.5)).abs() < 1.0);
            }
            other => panic!("expected text object 601, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_scene_executes_scripted_greeting_text_layers() {
        clear_scene_text_script_runtime_cache();
        let source = SceneManifest {
            canvas_width: Some(1920.0),
            canvas_height: Some(1080.0),
            text_layers: vec![sample_script_layer(
                "Greetings Placeholder Text\n<Name>",
                "'use strict';\n\nlet eveningGreetings = [\n  'Good evening, $!',\n  'Hi $,\\nhow has your day been?'\n];\n\nvar lastString;\nvar lastTimeTag;\nvar stringIndex = 0;\nexport function update() {\n  let hour = new Date().getHours();\n  let timeTag = hour < 18 ? 'afternoon' : 'evening';\n  if (lastTimeTag != timeTag) {\n    lastTimeTag = timeTag;\n    stringIndex = 0;\n  }\n  let newString = eveningGreetings[stringIndex];\n  if (newString != lastString) {\n    lastString = newString;\n    thisLayer.text = newString.replace('$', engine.userProperties.name);\n  }\n}\n\nexport function applyUserProperties() {\n  lastString = '';\n}\n",
            )],
            render_graph: vec![SceneRenderNode {
                id: 19,
                name: "Greeting".to_string(),
                parent_id: None,
                kind: SceneRenderNodeKind::Text,
                visible: true,
                asset_path: None,
                material_path: None,
            }],
            ..SceneManifest::default()
        };
        let persisted = BTreeMap::new();
        let definitions = BTreeMap::new();
        let now = Local
            .with_ymd_and_hms(2026, 4, 18, 19, 0, 0)
            .unwrap()
            .with_timezone(&Utc);

        let alice = evaluate_scene_with_runtime_key(
            Some("summer-feeling"),
            &source,
            &BTreeMap::from([(String::from("name"), json!("Alice"))]),
            &persisted,
            &definitions,
            None,
            now,
        );
        let bob = evaluate_scene_with_runtime_key(
            Some("summer-feeling"),
            &source,
            &BTreeMap::from([(String::from("name"), json!("Bob"))]),
            &persisted,
            &definitions,
            None,
            now,
        );

        let alice_text = match alice.objects.get(&19) {
            Some(EvaluatedSceneObject::Text { text, .. }) => text.value.clone(),
            other => panic!("expected greeting text object, got {other:?}"),
        };
        let bob_text = match bob.objects.get(&19) {
            Some(EvaluatedSceneObject::Text { text, .. }) => text.value.clone(),
            other => panic!("expected greeting text object, got {other:?}"),
        };

        assert_eq!(alice_text, "Good evening, Alice!");
        assert_eq!(bob_text, "Good evening, Bob!");
    }
}
