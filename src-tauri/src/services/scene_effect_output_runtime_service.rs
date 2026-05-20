use super::*;
use super::scene_effect_input_runtime_service::{Phase10PassInputScope, Phase10PassTextures};
use super::scene_effect_target_runtime_service::Phase10TextureHandle;
use crate::services::scene_mdl_service::SceneMdlMeshFrame;

#[cfg(target_os = "macos")]
pub(super) fn phase10_background_source_order(
    plan: &SceneRenderPlan,
    graph: &ScenePhase10GraphPlan,
) -> Vec<(SceneRenderDrawItem, Phase10BackgroundSourceKind)> {
    let phase10_visual_ids = graph
        .visuals
        .iter()
        .map(|visual| visual.object_id)
        .collect::<BTreeSet<_>>();
    let visual_ids = plan
        .visuals
        .iter()
        .map(|visual| visual.object_id)
        .collect::<BTreeSet<_>>();
    let text_ids = plan
        .texts
        .iter()
        .map(|text| text.object_id)
        .collect::<BTreeSet<_>>();

    plan.draw_order
        .iter()
        .filter_map(|draw_item| match draw_item.kind {
            SceneRenderDrawKind::Visual if phase10_visual_ids.contains(&draw_item.object_id) => {
                Some((*draw_item, Phase10BackgroundSourceKind::Phase10Visual))
            }
            SceneRenderDrawKind::Visual if visual_ids.contains(&draw_item.object_id) => {
                Some((*draw_item, Phase10BackgroundSourceKind::Visual))
            }
            SceneRenderDrawKind::Text if text_ids.contains(&draw_item.object_id) => {
                Some((*draw_item, Phase10BackgroundSourceKind::Text))
            }
            SceneRenderDrawKind::Audio
            | SceneRenderDrawKind::Particle
            | SceneRenderDrawKind::RopeParticle
            | SceneRenderDrawKind::SpriteParticle
            | SceneRenderDrawKind::Sound
            | SceneRenderDrawKind::Visual
            | SceneRenderDrawKind::Text => None,
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn render_phase10_outputs(
    renderer: &mut NativeSceneMetalRenderer,
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    plan: &SceneRenderPlan,
    graph: &ScenePhase10GraphPlan,
    elapsed_seconds: f64,
) -> BTreeMap<u32, Retained<ProtocolObject<dyn MTLTexture>>> {
    let mut outputs = BTreeMap::new();
    let mut required_output_keys = BTreeSet::new();
    let mut required_scratch_keys = BTreeSet::new();
    let mut required_named_target_keys = BTreeSet::new();
    let mut required_background_keys = BTreeSet::new();
    let phase10_visuals = graph
        .visuals
        .iter()
        .map(|visual| (visual.object_id, visual))
        .collect::<BTreeMap<_, _>>();
    let visual_items = plan
        .visuals
        .iter()
        .map(|item| (item.object_id, item))
        .collect::<BTreeMap<_, _>>();
    let text_items = plan
        .texts
        .iter()
        .map(|item| (item.object_id, item))
        .collect::<BTreeMap<_, _>>();
    let mut rendered_ids = BTreeSet::new();
    let mut previous_layers = Vec::<Phase10BackgroundLayer>::new();

    for (draw_item, source_kind) in phase10_background_source_order(plan, graph) {
        if source_kind == Phase10BackgroundSourceKind::Phase10Visual {
            if let Some(visual) = phase10_visuals.get(&draw_item.object_id) {
                if phase10_visual_requires_offscreen_chain(visual) {
                    let background_snapshot = if phase10_visual_needs_background_snapshot(visual) {
                        render_phase10_background_snapshot(
                            renderer,
                            command_buffer,
                            visual,
                            &previous_layers,
                            &mut required_background_keys,
                        )
                    } else {
                        None
                    };
                    if let Some(texture) = render_phase10_output_for_visual(
                        renderer,
                        command_buffer,
                        plan.canvas_height,
                        visual,
                        background_snapshot.as_ref(),
                        elapsed_seconds,
                        &mut required_output_keys,
                        &mut required_scratch_keys,
                        &mut required_named_target_keys,
                    ) {
                        previous_layers.push(Phase10BackgroundLayer {
                            quad: visual.quad,
                            blend_mode: visual.blend_mode,
                            texture: texture.clone(),
                            uv_rect: full_quad_uv_rect(),
                        });
                        outputs.insert(visual.object_id, texture);
                    }
                }
                rendered_ids.insert(visual.object_id);
                continue;
            }
        }

        if let Some(layer) = phase10_background_layer_for_draw_item(
            renderer,
            &draw_item,
            &visual_items,
            &text_items,
        ) {
            previous_layers.push(layer);
        }
    }

    for visual in &graph.visuals {
        if rendered_ids.contains(&visual.object_id) || !phase10_visual_requires_offscreen_chain(visual)
        {
            continue;
        }
        let background_snapshot = if phase10_visual_needs_background_snapshot(visual) {
            render_phase10_background_snapshot(
                renderer,
                command_buffer,
                visual,
                &previous_layers,
                &mut required_background_keys,
            )
        } else {
            None
        };
        if let Some(texture) = render_phase10_output_for_visual(
            renderer,
            command_buffer,
            plan.canvas_height,
            visual,
            background_snapshot.as_ref(),
            elapsed_seconds,
            &mut required_output_keys,
            &mut required_scratch_keys,
            &mut required_named_target_keys,
        ) {
            previous_layers.push(Phase10BackgroundLayer {
                quad: visual.quad,
                blend_mode: visual.blend_mode,
                texture: texture.clone(),
                uv_rect: full_quad_uv_rect(),
            });
            outputs.insert(visual.object_id, texture);
        }
    }

    renderer.phase10_targets.retain_required(
        &required_output_keys,
        &required_scratch_keys,
        &required_named_target_keys,
        &required_background_keys,
    );
    outputs
}

#[cfg(target_os = "macos")]
pub(super) fn render_phase10_output_for_visual(
    renderer: &mut NativeSceneMetalRenderer,
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    canvas_height: f64,
    visual: &ScenePhase10VisualPlan,
    background_snapshot: Option<&Phase10TextureHandle>,
    elapsed_seconds: f64,
    required_output_keys: &mut BTreeSet<String>,
    required_scratch_keys: &mut BTreeSet<String>,
    required_named_target_keys: &mut BTreeSet<String>,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let base_texture = renderer.phase10_base_texture_for_visual(visual)?;
    let mut previous_texture = base_texture.clone();
    let mut named_targets = BTreeMap::<String, Phase10TextureHandle>::new();
    let passes = phase10_visual_pass_chain(visual);
    if passes.is_empty() {
        return None;
    }

    let (width, height) = phase10_render_target_size(visual);
    let output_key = phase10_output_texture_key(visual.object_id, width, height);
    required_output_keys.insert(output_key.clone());
    let output_texture = renderer.ensure_phase10_output_target(&output_key, width, height)?;

    if visual.puppet_path.is_some() {
        return render_phase10_puppet_output_for_visual(
            renderer,
            command_buffer,
            canvas_height,
            visual,
            &base_texture,
            &passes,
            width,
            height,
            output_texture,
            elapsed_seconds,
            required_scratch_keys,
            required_named_target_keys,
        );
    }

    if phase10_visual_first_pass_is_mask_alpha(visual) && passes.len() == 1 {
        return render_phase10_mask_chain(
            renderer,
            command_buffer,
            visual,
            &base_texture,
            &passes[0],
            width,
            height,
            required_scratch_keys,
        );
    }

    for (index, resolved_pass) in passes.iter().enumerate() {
        let target_name = phase10_resolved_pass_target_name(resolved_pass);
        let is_last = index + 1 == passes.len();
        let target = if let Some(target_name) = target_name {
            let key = phase10_named_target_texture_key(visual.object_id, target_name, width, height);
            required_named_target_keys.insert(key.clone());
            renderer.ensure_phase10_named_target(&key, width, height)?
        } else if is_last {
            output_texture.clone()
        } else {
            let scratch_key = phase10_scratch_texture_key(width, height, index % 2);
            required_scratch_keys.insert(scratch_key.clone());
            renderer.ensure_phase10_scratch_target(&scratch_key, width, height)?
        };

        let input_scope = Phase10PassInputScope {
            local_current: Some(&base_texture),
            previous_pass: Some(&previous_texture),
            background: background_snapshot,
            copied_background: background_snapshot,
            named_targets: &named_targets,
        };
        let pass_textures = renderer.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
        let uniforms = renderer.phase10_effect_uniforms_for_pass(
            resolved_pass,
            &pass_textures,
            width,
            height,
            elapsed_seconds,
        );
        let shader_defines = phase10_pass_shader_defines(resolved_pass);
        if !renderer.encode_phase10_pass(
            command_buffer,
            &target.texture,
            resolved_pass.pass,
            &shader_defines,
            &pass_textures,
            &uniforms,
            input_scope.previous_pass.map(|texture| texture.texture.clone()),
        ) {
            return None;
        }

        previous_texture = target.clone();
        if let Some(target_name) = target_name {
            named_targets.insert(target_name.to_string(), target);
        }
    }

    Some(previous_texture.texture)
}

#[cfg(target_os = "macos")]
pub(super) fn render_phase10_puppet_output_for_visual(
    renderer: &mut NativeSceneMetalRenderer,
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    canvas_height: f64,
    visual: &ScenePhase10VisualPlan,
    base_texture: &Phase10TextureHandle,
    passes: &[Phase10ResolvedPass<'_>],
    width: usize,
    height: usize,
    output_texture: Phase10TextureHandle,
    elapsed_seconds: f64,
    required_scratch_keys: &mut BTreeSet<String>,
    required_named_target_keys: &mut BTreeSet<String>,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let puppet_path = visual.puppet_path.as_ref()?;
    let document = renderer.mdl_cache.get(puppet_path)?;
    let mesh_frame = evaluate_scene_mdl_mesh(document, &visual.animation_layers, elapsed_seconds)?;
    if mesh_frame.positions.is_empty() || mesh_frame.indices.len() < 3 {
        return None;
    }

    let submesh_frames: Vec<SceneMdlMeshFrame> = if visual.submeshes.is_empty() {
        vec![mesh_frame.clone()]
    } else {
        visual
            .submeshes
            .iter()
            .filter_map(|submesh| mesh_frame.extract_submesh(submesh))
            .collect()
    };
    if submesh_frames.is_empty() {
        return None;
    }

    let projection = phase10_puppet_offscreen_projection(visual, canvas_height, width, height);
    let mut previous_texture = base_texture.clone();
    let mut named_targets = BTreeMap::<String, Phase10TextureHandle>::new();

    for (index, resolved_pass) in passes.iter().enumerate() {
        let target_name = phase10_resolved_pass_target_name(resolved_pass);
        let is_last = index + 1 == passes.len();
        let target = if let Some(target_name) = target_name {
            let key = phase10_named_target_texture_key(visual.object_id, target_name, width, height);
            required_named_target_keys.insert(key.clone());
            renderer.ensure_phase10_named_target(&key, width, height)?
        } else if is_last {
            output_texture.clone()
        } else {
            let scratch_key = phase10_scratch_texture_key(width, height, index % 2);
            required_scratch_keys.insert(scratch_key.clone());
            renderer.ensure_phase10_scratch_target(&scratch_key, width, height)?
        };

        let input_scope = Phase10PassInputScope {
            local_current: Some(base_texture),
            previous_pass: Some(&previous_texture),
            background: None,
            copied_background: None,
            named_targets: &named_targets,
        };
        let pass_textures = renderer.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
        if pass_textures
            .slots
            .first()
            .and_then(|slot| slot.as_ref())
            .is_none()
        {
            return None;
        }
        let uniforms = renderer.phase10_effect_uniforms_for_pass(
            resolved_pass,
            &pass_textures,
            width,
            height,
            elapsed_seconds,
        );
        let shader_defines = phase10_pass_shader_defines(resolved_pass);

        let descriptor = MTLRenderPassDescriptor::new();
        unsafe {
            let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
            attachment.setTexture(Some(target.texture.as_ref()));
            attachment.setLoadAction(MTLLoadAction::Clear);
            attachment.setStoreAction(MTLStoreAction::Store);
            attachment.setClearColor(objc2_metal::MTLClearColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            });
        }
        let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor) else {
            return None;
        };

        if phase10_alpha_prefill_required(resolved_pass.pass.blend_mode) {
            if let Some(previous_texture) = input_scope.previous_pass.map(|t| t.texture.clone()) {
                renderer.draw_phase10_fullscreen_texture(
                    &encoder,
                    previous_texture,
                    SceneRenderColor::default(),
                    SceneRenderBlendMode::Normal,
                );
            }
        }

        for sub_frame in &submesh_frames {
            renderer.draw_phase10_mesh(
                &encoder,
                &projection,
                visual,
                sub_frame,
                resolved_pass.pass,
                &shader_defines,
                &pass_textures,
                &uniforms,
            );
        }
        encoder.endEncoding();

        previous_texture = target.clone();
        if let Some(target_name) = target_name {
            named_targets.insert(target_name.to_string(), target);
        }
    }

    Some(previous_texture.texture)
}

#[cfg(target_os = "macos")]
pub(super) fn render_phase10_mask_chain(
    renderer: &mut NativeSceneMetalRenderer,
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    visual: &ScenePhase10VisualPlan,
    base_texture: &Phase10TextureHandle,
    resolved_pass: &Phase10ResolvedPass<'_>,
    width: usize,
    height: usize,
    required_scratch_keys: &mut BTreeSet<String>,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let output_key = phase10_output_texture_key(visual.object_id, width, height);
    let output_texture = renderer.ensure_phase10_output_target(&output_key, width, height)?;

    let scratch_key = phase10_scratch_texture_key(width, height, 0);
    required_scratch_keys.insert(scratch_key.clone());
    let scratch_texture = renderer.ensure_phase10_scratch_target(&scratch_key, width, height)?;

    let input_scope = Phase10PassInputScope {
        local_current: Some(base_texture),
        previous_pass: Some(base_texture),
        background: None,
        copied_background: None,
        named_targets: &BTreeMap::new(),
    };
    let mask_alpha_pass_textures =
        renderer.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
    let mask_alpha_defines = phase10_pass_shader_defines(resolved_pass);
    let mask_alpha_uniforms = renderer.phase10_effect_uniforms_for_pass(
        resolved_pass,
        &mask_alpha_pass_textures,
        width,
        height,
        0.0,
    );

    if !renderer.encode_phase10_pass(
        command_buffer,
        &scratch_texture.texture,
        resolved_pass.pass,
        &mask_alpha_defines,
        &mask_alpha_pass_textures,
        &mask_alpha_uniforms,
        None,
    ) {
        return None;
    }

    let mask_apply_program = phase10_mask_apply_shader_program();
    let mask_apply_key =
        phase10_shader_variant_key(&mask_apply_program, &BTreeMap::new(), visual.blend_mode);
    let Some(mask_apply_pipeline) = renderer.compiled_shader_variants.get(&mask_apply_key) else {
        return None;
    };

    let mask_apply_pass_textures = Phase10PassTextures {
        slots: vec![
            Some(base_texture.clone()),
            Some(scratch_texture),
            None,
            None,
        ],
    };

    let descriptor = MTLRenderPassDescriptor::new();
    unsafe {
        let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
        attachment.setTexture(Some(output_texture.texture.as_ref()));
        attachment.setLoadAction(MTLLoadAction::Clear);
        attachment.setStoreAction(MTLStoreAction::Store);
        attachment.setClearColor(objc2_metal::MTLClearColor {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha: 0.0,
        });
    }
    let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor) else {
        return None;
    };

    renderer.draw_phase10_fullscreen_pass(
        &encoder,
        mask_apply_pipeline.as_ref(),
        &mask_apply_pass_textures,
        &Phase10EffectUniforms::default(),
        visual.base_color,
    );
    encoder.endEncoding();

    Some(output_texture.texture)
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_background_layer_for_draw_item(
    renderer: &mut NativeSceneMetalRenderer,
    draw_item: &SceneRenderDrawItem,
    visual_items: &BTreeMap<u32, &SceneRenderVisualItem>,
    text_items: &BTreeMap<u32, &SceneRenderTextItem>,
) -> Option<Phase10BackgroundLayer> {
    match draw_item.kind {
        SceneRenderDrawKind::Visual => {
            let item = visual_items.get(&draw_item.object_id)?;
            let texture = match item.source_kind {
                SceneRenderSourceKind::Image => renderer
                    .texture_cache
                    .get(&visual_texture_cache_key(item))
                    .cloned(),
                SceneRenderSourceKind::Video => renderer.video_texture_for_item(item),
            }?;
            Some(Phase10BackgroundLayer {
                quad: item.quad,
                blend_mode: item.blend_mode,
                texture,
                uv_rect: item.uv_rect,
            })
        }
        SceneRenderDrawKind::Text => {
            let item = text_items.get(&draw_item.object_id)?;
            let texture = renderer
                .text_texture_cache
                .get(&text_texture_cache_key(item))?
                .clone();
            Some(Phase10BackgroundLayer {
                quad: item.quad,
                blend_mode: SceneRenderBlendMode::Normal,
                texture,
                uv_rect: full_quad_uv_rect(),
            })
        }
        SceneRenderDrawKind::Audio
        | SceneRenderDrawKind::Particle
        | SceneRenderDrawKind::RopeParticle
        | SceneRenderDrawKind::SpriteParticle
        | SceneRenderDrawKind::Sound => None,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn render_phase10_background_snapshot(
    renderer: &mut NativeSceneMetalRenderer,
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    visual: &ScenePhase10VisualPlan,
    layers: &[Phase10BackgroundLayer],
    required_background_keys: &mut BTreeSet<String>,
) -> Option<Phase10TextureHandle> {
    let (width, height) = phase10_render_target_size(visual);
    let key = phase10_background_texture_key(visual.object_id, width, height);
    required_background_keys.insert(key.clone());
    let target = renderer.ensure_phase10_background_target(&key, width, height)?;

    let descriptor = MTLRenderPassDescriptor::new();
    unsafe {
        let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
        attachment.setTexture(Some(target.texture.as_ref()));
        attachment.setLoadAction(MTLLoadAction::Clear);
        attachment.setStoreAction(MTLStoreAction::Store);
        attachment.setClearColor(objc2_metal::MTLClearColor {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha: 0.0,
        });
    }
    let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor) else {
        return None;
    };
    let projection = phase10_local_background_projection(visual, width, height);
    for layer in layers {
        renderer.draw_quad(
            &encoder,
            layer.texture.as_ref(),
            layer.blend_mode,
            &projection,
            SceneQuadPrimitive {
                left: layer.quad.left,
                top: layer.quad.top,
                width: layer.quad.width,
                height: layer.quad.height,
                rotation: layer.quad.rotation,
                opacity: layer.quad.opacity,
                flip_x: layer.quad.flip_x,
                flip_y: layer.quad.flip_y,
                uv_rect: layer.uv_rect,
                color: SceneRenderColor::default(),
                transform_origin_x: layer.quad.left + layer.quad.width / 2.0,
                transform_origin_y: layer.quad.top + layer.quad.height / 2.0,
            },
        );
    }
    encoder.endEncoding();
    Some(target)
}
