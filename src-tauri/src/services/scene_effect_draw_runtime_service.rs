use super::*;
use crate::services::scene_mdl_service::SceneMdlMeshFrame;

#[cfg(target_os = "macos")]
pub(super) fn encode_phase10_pass(
    renderer: &NativeSceneMetalRenderer,
    command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
    target: &Retained<ProtocolObject<dyn MTLTexture>>,
    pass: &SceneMaterialPassPlan,
    shader_defines: &BTreeMap<String, i32>,
    pass_textures: &Phase10PassTextures,
    uniforms: &Phase10EffectUniforms,
    previous_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
) -> bool {
    let variant_key = phase10_shader_variant_key(&pass.program, shader_defines, pass.blend_mode);
    let Some(pipeline) = renderer.compiled_shader_variants.get(&variant_key) else {
        return false;
    };

    let descriptor = MTLRenderPassDescriptor::new();
    unsafe {
        let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
        attachment.setTexture(Some(target.as_ref()));
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
        return false;
    };

    if phase10_alpha_prefill_required(pass.blend_mode) {
        if let Some(texture) = previous_texture {
            draw_phase10_fullscreen_texture(
                renderer,
                &encoder,
                texture,
                SceneRenderColor::default(),
                SceneRenderBlendMode::Normal,
            );
        }
    }

    draw_phase10_fullscreen_pass(
        renderer,
        &encoder,
        pipeline.as_ref(),
        pass_textures,
        uniforms,
        SceneRenderColor::default(),
    );
    encoder.endEncoding();
    true
}

#[cfg(target_os = "macos")]
pub(super) fn draw_phase10_fullscreen_texture(
    renderer: &NativeSceneMetalRenderer,
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
    tint: SceneRenderColor,
    blend_mode: SceneRenderBlendMode,
) {
    let pass_textures = Phase10PassTextures {
        slots: vec![Some(Phase10TextureHandle {
            metrics: phase10_texture_metrics_from_texture(texture.as_ref()),
            texture,
        })],
    };
    let pipeline = renderer.pipeline_for(blend_mode);
    draw_phase10_fullscreen_pass(
        renderer,
        encoder,
        pipeline,
        &pass_textures,
        &Phase10EffectUniforms::default(),
        tint,
    );
}

#[cfg(target_os = "macos")]
pub(super) fn draw_phase10_fullscreen_pass(
    renderer: &NativeSceneMetalRenderer,
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
    pass_textures: &Phase10PassTextures,
    uniforms: &Phase10EffectUniforms,
    tint: SceneRenderColor,
) {
    let vertices = phase10_fullscreen_vertices(tint);
    draw_phase10_vertices_with_pipeline(renderer, encoder, pipeline, pass_textures, uniforms, &vertices);
}

#[cfg(target_os = "macos")]
pub(super) fn draw_phase10_mesh(
    renderer: &NativeSceneMetalRenderer,
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    projection: &SceneProjection,
    visual: &ScenePhase10VisualPlan,
    mesh_frame: &SceneMdlMeshFrame,
    pass: &SceneMaterialPassPlan,
    shader_defines: &BTreeMap<String, i32>,
    pass_textures: &Phase10PassTextures,
    uniforms: &Phase10EffectUniforms,
) {
    let Some(vertices) = build_projected_puppet_mesh_vertices(
        mesh_frame,
        visual.world_position,
        visual.world_scale,
        visual.world_angles,
        visual.quad.opacity,
        projection,
    ) else {
        return;
    };
    let variant_key = phase10_shader_variant_key(&pass.program, shader_defines, pass.blend_mode);
    let Some(pipeline) = renderer.compiled_shader_variants.get(&variant_key) else {
        return;
    };
    draw_phase10_vertices_with_pipeline(
        renderer,
        encoder,
        pipeline.as_ref(),
        pass_textures,
        uniforms,
        &vertices,
    );
}

#[cfg(target_os = "macos")]
pub(super) fn draw_phase10_vertices_with_pipeline(
    renderer: &NativeSceneMetalRenderer,
    encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
    pass_textures: &Phase10PassTextures,
    uniforms: &Phase10EffectUniforms,
    vertices: &[SceneVertex],
) {
    let byte_len = std::mem::size_of_val(vertices);
    if byte_len == 0 {
        return;
    }
    let Some(vertices_bytes) = NonNull::new(vertices.as_ptr() as *mut c_void) else {
        return;
    };
    let Some(uniform_bytes) =
        NonNull::new(uniforms as *const Phase10EffectUniforms as *mut c_void)
    else {
        return;
    };

    encoder.setRenderPipelineState(pipeline);
    unsafe {
        match scene_vertex_upload_strategy(byte_len) {
            SceneVertexUploadStrategy::InlineBytes => {
                encoder.setVertexBytes_length_atIndex(vertices_bytes, byte_len, 0);
            }
            SceneVertexUploadStrategy::SharedBuffer => {
                let Some(vertex_buffer) = renderer.device.newBufferWithBytes_length_options(
                    vertices_bytes,
                    byte_len,
                    MTLResourceOptions::StorageModeShared,
                ) else {
                    return;
                };
                encoder.setVertexBuffer_offset_atIndex(Some(vertex_buffer.as_ref()), 0, 0);
            }
        }
        encoder.setVertexBytes_length_atIndex(
            uniform_bytes,
            std::mem::size_of::<Phase10EffectUniforms>(),
            1,
        );
        encoder.setFragmentBytes_length_atIndex(
            uniform_bytes,
            std::mem::size_of::<Phase10EffectUniforms>(),
            0,
        );
        for slot in 0..4 {
            let texture = pass_textures
                .slots
                .get(slot)
                .and_then(|texture| texture.as_ref())
                .map(|texture| texture.texture.as_ref());
            encoder.setFragmentTexture_atIndex(texture, slot);
        }
        encoder.drawPrimitives_vertexStart_vertexCount(
            MTLPrimitiveType::Triangle,
            0,
            vertices.len(),
        );
    }
}
