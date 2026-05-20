use super::*;

use crate::services::scene_resource_service;
#[cfg(target_os = "macos")]
use crate::services::scene_text_raster_service::rasterize_text_texture;

#[cfg(target_os = "macos")]
pub(super) struct NativeSceneMetalRenderer {
    app: AppHandle,
    command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    video_texture_cache: CFRetained<CVMetalTextureCache>,
    pipelines: NativeScenePipelineStates,
    scene_key: Option<String>,
    plan: Option<SceneRenderPlan>,
    phase10_graph: ScenePhase10GraphPlan,
    texture_cache: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: BTreeMap<String, Phase10TextureMetrics>,
    text_texture_cache: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_output_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_scratch_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_named_target_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_background_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    video_sources: BTreeMap<u32, NativeSceneVideoSource>,
    mdl_cache: BTreeMap<PathBuf, SceneMdlDocument>,
    compiled_shader_variants:
        BTreeMap<String, Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
    started_at: Instant,
    last_frame_at: Instant,
    animation_time_seconds: f64,
    audio_coordinator: SceneAudioCoordinator,
    input_coordinator: SceneInputCoordinator,
    particle_signature: Option<u64>,
    particle_scheduler: SceneParticleScheduler,
    rope_particle_scheduler: SceneRopeParticleScheduler,
    sprite_particle_scheduler: SceneSpriteParticleScheduler,
    paused: bool,
}

#[cfg(target_os = "macos")]
pub(super) struct NativeScenePipelineStates {
    pub(super) normal: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub(super) additive: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub(super) multiply: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub(super) struct NativeSceneVideoSource {
    object_id: u32,
    asset_path: PathBuf,
    paused: bool,
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    current_cv_texture: Option<CFRetained<CVMetalTexture>>,
    current_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct SceneVertex {
    pub(super) position: [f32; 2],
    pub(super) uv: [f32; 2],
    pub(super) color: [f32; 4],
    pub(super) opacity: f32,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(super) struct SceneQuadPrimitive {
    pub(super) left: f64,
    pub(super) top: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) rotation: f64,
    pub(super) opacity: f64,
    pub(super) flip_x: bool,
    pub(super) flip_y: bool,
    pub(super) uv_rect: [f32; 4],
    pub(super) color: SceneRenderColor,
    pub(super) transform_origin_x: f64,
    pub(super) transform_origin_y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(super) struct SceneProjection {
    pub(super) scene_origin_x: f64,
    pub(super) scene_origin_y: f64,
    pub(super) scene_canvas_height: f64,
    pub(super) camera_scale: f64,
    pub(super) view_width: f64,
    pub(super) view_height: f64,
}

#[cfg(target_os = "macos")]
impl NativeSceneMetalRenderer {
    pub(super) fn new(
        app: AppHandle,
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    ) -> Result<Self, String> {
        Ok(Self {
            app,
            command_queue,
            device: device.clone(),
            video_texture_cache: create_scene_video_texture_cache(device.as_ref())?,
            pipelines: build_scene_pipeline_states(&device)?,
            scene_key: None,
            plan: None,
            phase10_graph: ScenePhase10GraphPlan::default(),
            texture_cache: BTreeMap::new(),
            texture_resolution_cache: BTreeMap::new(),
            text_texture_cache: BTreeMap::new(),
            phase10_output_textures: BTreeMap::new(),
            phase10_scratch_textures: BTreeMap::new(),
            phase10_named_target_textures: BTreeMap::new(),
            phase10_background_textures: BTreeMap::new(),

            video_sources: BTreeMap::new(),
            mdl_cache: BTreeMap::new(),
            compiled_shader_variants: BTreeMap::new(),
            started_at: Instant::now(),
            last_frame_at: Instant::now(),
            animation_time_seconds: 0.0,
            audio_coordinator: SceneAudioCoordinator::default(),
            input_coordinator: SceneInputCoordinator::default(),
            particle_signature: None,
            particle_scheduler: SceneParticleScheduler::default(),
            rope_particle_scheduler: SceneRopeParticleScheduler::default(),
            sprite_particle_scheduler: SceneSpriteParticleScheduler::default(),
            paused: false,
        })
    }

    pub(super) fn apply_scene(
        &mut self,
        scene_key: &str,
        mut plan: SceneRenderPlan,
        phase10_graph: ScenePhase10GraphPlan,
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let mut warnings = Vec::new();
        let mut required_keys = BTreeSet::new();
        let mut required_text_keys = BTreeSet::new();
        let mut retained_visuals = Vec::new();
        let mut retained_texts = Vec::new();
        let phase10_consumed_ids = phase10_graph
            .consumed_visual_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let next_particle_signature = particle_plan_signature(&plan);

        if self.scene_key.as_deref() != Some(scene_key) {
            self.clear_video_sources();
        }
        warnings.extend(self.sync_video_sources(&plan.visuals, paused));

        for item in &plan.visuals {
            if should_retain_visual_in_draw_plan(item, &phase10_consumed_ids, false) {
                retained_visuals.push(item.clone());
                continue;
            }
            match item.source_kind {
                SceneRenderSourceKind::Image => {
                    let key = visual_texture_cache_key(item);
                    match self.ensure_visual_texture_loaded(item, &key) {
                        Ok(()) => {
                            required_keys.insert(key);
                            if should_retain_visual_in_draw_plan(item, &phase10_consumed_ids, true)
                            {
                                retained_visuals.push(item.clone());
                            }
                        }
                        Err(error) => {
                            warnings
                                .push(NativeSceneWarning::texture_load(&item.texture_path, error));
                        }
                    }
                }
                SceneRenderSourceKind::Video => {
                    retained_visuals.push(item.clone());
                }
            }
        }

        for item in &plan.texts {
            let key = text_texture_cache_key(item);
            match self.ensure_text_texture_loaded(item, &key) {
                Ok(text_warnings) => {
                    required_text_keys.insert(key);
                    retained_texts.push(item.clone());
                    warnings.extend(text_warnings);
                }
                Err(error) => warnings.push(NativeSceneWarning {
                    code: "text-raster-failed".to_string(),
                    message: format!(
                        "Scene text {} could not be rasterized for Metal.",
                        item.object_name
                    ),
                    detail: Some(SceneDiagnosticDetail::runtime(
                        SceneDiagnosticDomain::Text,
                        "text-raster",
                        error,
                    )),
                }),
            }
        }

        for item in &plan.sprite_particles {
            for texture_path in sprite_particle_texture_paths(item) {
                match self.ensure_phase10_texture_loaded(&texture_path, &mut required_keys) {
                    Ok(()) => {}
                    Err(error) => warnings.push(NativeSceneWarning::texture_load(
                        &texture_path,
                        format!("Sprite particle texture could not be prepared: {error}"),
                    )),
                }
            }
        }
        for item in &plan.rope_particles {
            let Some(texture_path) = item.texture_path.as_ref() else {
                continue;
            };
            if texture_path == Path::new("__rope-white__") {
                continue;
            }
            match self.ensure_phase10_texture_loaded(texture_path, &mut required_keys) {
                Ok(()) => {}
                Err(error) => warnings.push(NativeSceneWarning::texture_load(
                    texture_path,
                    format!("Rope particle texture could not be prepared: {error}"),
                )),
            }
        }

        let (prepared_phase10_graph, phase10_warnings) =
            self.prepare_phase10_graph(&phase10_graph, &mut required_keys)?;
        warnings.extend(phase10_warnings);

        if !plan.audios.is_empty() || !plan.particles.is_empty() || !plan.rope_particles.is_empty()
        {
            self.ensure_procedural_texture(
                WHITE_TEXTURE_KEY,
                build_white_texture_image(),
                &mut required_keys,
            )?;
        }
        if plan.particles.iter().any(|item| {
            matches!(
                item.particle_kind,
                crate::models::SceneParticleKind::PetalTrail
            )
        }) {
            self.ensure_procedural_texture(
                PETAL_TEXTURE_KEY,
                build_petal_texture_image(),
                &mut required_keys,
            )?;
        }

        self.texture_cache
            .retain(|key, _| required_keys.contains(key));
        self.texture_resolution_cache
            .retain(|key, _| required_keys.contains(key));
        self.text_texture_cache
            .retain(|key, _| required_text_keys.contains(key));
        let active_audio_counts = plan
            .audios
            .iter()
            .map(|item| item.bar_count)
            .collect::<BTreeSet<_>>();
        self.audio_coordinator.retain_counts(&active_audio_counts);

        plan.visuals = retained_visuals;
        plan.texts = retained_texts;
        self.phase10_graph = prepared_phase10_graph;
        self.paused = paused;

        if scene_debug_layout_enabled() {
            eprintln!(
                "[scene-layout] scene_key={scene_key} visuals={} texts={} audios={}",
                plan.visuals.len(),
                plan.texts.len(),
                plan.audios.len()
            );
            for item in plan.texts.iter().take(12) {
                eprintln!(
                    "[scene-layout][text] id={} name={} behavior={:?} quad=({:.1},{:.1},{:.1},{:.1}) content=({:.1},{:.1},{:.1},{:.1}) point={:.1} text={:?}",
                    item.object_id,
                    item.object_name,
                    item.behavior,
                    item.quad.left,
                    item.quad.top,
                    item.quad.width,
                    item.quad.height,
                    item.content_left,
                    item.content_top,
                    item.content_width,
                    item.content_height,
                    item.point_size,
                    item.text
                );
            }
            for item in plan.audios.iter().take(6) {
                eprintln!(
                    "[scene-layout][audio] id={} name={} quad=({:.1},{:.1},{:.1},{:.1}) rot={:.3} bars={} gap={:.2} width={:.2} top={:.2} height={:.2}",
                    item.object_id,
                    item.object_name,
                    item.quad.left,
                    item.quad.top,
                    item.quad.width,
                    item.quad.height,
                    item.quad.rotation,
                    item.bar_count,
                    item.gap,
                    item.bar_width,
                    item.drawable_top,
                    item.drawable_height
                );
            }
        }

        if !plan.has_renderable_output() && self.phase10_graph.visuals.is_empty() {
            let preview = warnings
                .iter()
                .take(4)
                .map(|warning| warning.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let suffix = if preview.is_empty() {
                String::new()
            } else {
                format!(": {preview}")
            };
            return Err(format!(
                "Scene native renderer has no draw-ready output after texture preparation{suffix}"
            ));
        }

        if self.scene_key.as_deref() != Some(scene_key)
            || self.particle_signature != next_particle_signature
        {
            self.particle_scheduler.reset();
            self.rope_particle_scheduler.reset();
            self.sprite_particle_scheduler.reset();
        }
        if self.scene_key.as_deref() != Some(scene_key) {
            self.audio_coordinator.reset();
            self.input_coordinator.reset();
            self.animation_time_seconds = 0.0;
        }

        warnings.extend(self.runtime_dependency_warnings(&plan));
        self.scene_key = Some(scene_key.to_string());
        self.particle_signature = next_particle_signature;
        self.plan = Some(plan);
        Ok(warnings)
    }

    pub(super) fn apply_dynamic_text_update(
        &mut self,
        texts: Vec<SceneRenderTextItem>,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let mut retained_texts = Vec::new();
        let mut required_text_keys = BTreeSet::new();
        let mut warnings = Vec::new();

        if self.plan.is_none() {
            return Err("native Scene renderer has no active plan for dynamic text update".into());
        }

        for item in texts {
            let key = text_texture_cache_key(&item);
            match self.ensure_text_texture_loaded(&item, &key) {
                Ok(text_warnings) => {
                    required_text_keys.insert(key);
                    retained_texts.push(item);
                    warnings.extend(text_warnings);
                }
                Err(error) => warnings.push(NativeSceneWarning {
                    code: "text-raster-failed".to_string(),
                    message: format!(
                        "Scene text {} could not be rasterized for Metal.",
                        item.object_name
                    ),
                    detail: Some(SceneDiagnosticDetail::runtime(
                        SceneDiagnosticDomain::Text,
                        "text-raster",
                        error,
                    )),
                }),
            }
        }

        self.text_texture_cache
            .retain(|key, _| required_text_keys.contains(key));
        if let Some(plan) = self.plan.as_mut() {
            plan.texts = retained_texts;
        }

        Ok(warnings)
    }

    fn runtime_dependency_warnings(&self, plan: &SceneRenderPlan) -> Vec<NativeSceneWarning> {
        runtime_dependency_warnings_for_plan(
            plan,
            shared_audio_capture_unavailable(&self.app),
            input_service::input_snapshot_initialized(&self.app),
            shared_input_snapshot_unavailable(&self.app),
        )
    }

    pub(super) fn clear_scene(&mut self) {
        self.scene_key = None;
        self.plan = None;
        self.phase10_graph = ScenePhase10GraphPlan::default();
        self.texture_cache.clear();
        self.texture_resolution_cache.clear();
        self.text_texture_cache.clear();
        self.phase10_output_textures.clear();
        self.phase10_scratch_textures.clear();
        self.clear_video_sources();
        self.mdl_cache.clear();
        self.compiled_shader_variants.clear();
        self.particle_signature = None;
        self.audio_coordinator.reset();
        self.input_coordinator.reset();
        self.particle_scheduler.reset();
        self.rope_particle_scheduler.reset();
        self.sprite_particle_scheduler.reset();
        self.last_frame_at = Instant::now();
        self.animation_time_seconds = 0.0;
        self.paused = false;
    }

    pub(super) fn draw(&mut self, view: &MTKView) {
        let Some(current_drawable) = view.currentDrawable() else {
            return;
        };
        let Some(pass_descriptor) = view.currentRenderPassDescriptor() else {
            return;
        };
        let Some(command_buffer) = self.command_queue.commandBuffer() else {
            return;
        };

        let plan = self.plan.clone();
        if let Some(plan) = plan.as_ref() {
            self.draw_plan(view, &command_buffer, &pass_descriptor, plan);
        }
        command_buffer.presentDrawable(ProtocolObject::from_ref(&*current_drawable));
        command_buffer.commit();
    }

    pub(super) fn metal_device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        self.device.clone()
    }

    fn draw_plan(
        &mut self,
        view: &MTKView,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        pass_descriptor: &MTLRenderPassDescriptor,
        plan: &SceneRenderPlan,
    ) {
        let delta_seconds = self.frame_delta_seconds();
        if !self.paused {
            self.animation_time_seconds += delta_seconds;
        }
        let input_frame = self.update_input_frame(view, plan, delta_seconds);
        let projection = scene_projection(view, plan, input_frame.camera_offset);
        let shared_audio_snapshot = audio_input_service::current_audio_snapshot(&self.app).ok();
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        let phase10_graph = self.phase10_graph.clone();
        let phase10_outputs = self.render_phase10_outputs(
            command_buffer,
            plan,
            &phase10_graph,
            self.animation_time_seconds,
        );
        let phase10_visuals = phase10_graph
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
        let audio_items = plan
            .audios
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let particle_items = plan
            .particles
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let rope_particle_items = plan
            .rope_particles
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let sprite_particle_items = plan
            .sprite_particles
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let mut drawn_phase10_ids = BTreeSet::new();
        let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(pass_descriptor)
        else {
            return;
        };
        let white_texture = self.texture_cache.get(WHITE_TEXTURE_KEY).cloned();
        let petal_texture = self.texture_cache.get(PETAL_TEXTURE_KEY).cloned();
        let now_ms_f64 = now_ms as f64;

        if white_texture.is_some() && !plan.particles.is_empty() {
            self.advance_particle_items(&plan.particles, input_frame.response, now_ms_f64);
        }
        if !plan.rope_particles.is_empty() {
            self.advance_rope_particle_items(
                &plan.rope_particles,
                input_frame.response,
                now_ms_f64,
            );
        }
        if !plan.sprite_particles.is_empty() {
            if !self.paused {
                self.sprite_particle_scheduler
                    .advance_with_cursor(
                        &plan.sprite_particles,
                        input_frame.response.cursor(),
                        now_ms_f64,
                    );
            } else {
                self.sprite_particle_scheduler.pause();
            }
        }

        for draw_item in &plan.draw_order {
            match draw_item.kind {
                SceneRenderDrawKind::Visual => {
                    if let Some(visual) = phase10_visuals.get(&draw_item.object_id) {
                        self.draw_phase10_visual(
                            &encoder,
                            &projection,
                            visual,
                            phase10_outputs.get(&draw_item.object_id),
                            self.animation_time_seconds,
                        );
                        drawn_phase10_ids.insert(draw_item.object_id);
                        continue;
                    }
                    let Some(item) = visual_items.get(&draw_item.object_id) else {
                        continue;
                    };
                    let texture = match item.source_kind {
                        SceneRenderSourceKind::Image => self
                            .texture_cache
                            .get(&visual_texture_cache_key(item))
                            .cloned(),
                        SceneRenderSourceKind::Video => self.video_texture_for_item(item),
                    };
                    let Some(texture) = texture.as_ref() else {
                        continue;
                    };
                    let quad = quad_primitive_from_render_quad(item, SceneRenderColor::default());
                    self.draw_quad(
                        &encoder,
                        texture.as_ref(),
                        item.blend_mode,
                        &projection,
                        quad,
                    );
                }
                SceneRenderDrawKind::Text => {
                    let Some(item) = text_items.get(&draw_item.object_id) else {
                        continue;
                    };
                    let key = text_texture_cache_key(item);
                    let Some(texture) = self.text_texture_cache.get(&key) else {
                        continue;
                    };
                    let quad = SceneQuadPrimitive {
                        left: item.quad.left,
                        top: item.quad.top,
                        width: item.quad.width,
                        height: item.quad.height,
                        rotation: item.quad.rotation,
                        opacity: item.quad.opacity,
                        flip_x: item.quad.flip_x,
                        flip_y: item.quad.flip_y,
                        uv_rect: [0.0, 0.0, 1.0, 1.0],
                        color: SceneRenderColor::default(),
                        transform_origin_x: item.quad.left + item.quad.width / 2.0,
                        transform_origin_y: item.quad.top + item.quad.height / 2.0,
                    };
                    self.draw_quad(
                        &encoder,
                        texture.as_ref(),
                        SceneRenderBlendMode::Normal,
                        &projection,
                        quad,
                    );
                }
                SceneRenderDrawKind::Audio => {
                    let (Some(item), Some(white_texture)) = (
                        audio_items.get(&draw_item.object_id),
                        white_texture.as_ref(),
                    ) else {
                        continue;
                    };
                    self.draw_audio_item(
                        &encoder,
                        white_texture.as_ref(),
                        &projection,
                        item,
                        shared_audio_snapshot.as_ref(),
                        now_ms,
                    );
                }
                SceneRenderDrawKind::Particle => {
                    let (Some(item), Some(white_texture)) = (
                        particle_items.get(&draw_item.object_id),
                        white_texture.as_ref(),
                    ) else {
                        continue;
                    };
                    self.draw_particle_item(
                        &encoder,
                        white_texture.as_ref(),
                        petal_texture.as_ref().map(|texture| texture.as_ref()),
                        &projection,
                        plan,
                        item,
                        now_ms_f64,
                    );
                }
                SceneRenderDrawKind::RopeParticle => {
                    let (Some(item), Some(white_texture)) = (
                        rope_particle_items.get(&draw_item.object_id),
                        white_texture.as_ref(),
                    ) else {
                        continue;
                    };
                    self.draw_rope_particle_item(
                        &encoder,
                        white_texture.as_ref(),
                        &projection,
                        plan.canvas_height,
                        item,
                        now_ms_f64,
                    );
                }
                SceneRenderDrawKind::SpriteParticle => {
                    let Some(item) = sprite_particle_items.get(&draw_item.object_id) else {
                        continue;
                    };
                    self.draw_sprite_particle_item(&encoder, &projection, plan, item, now_ms_f64);
                }
                SceneRenderDrawKind::Sound => {}
            }
        }

        for visual in &phase10_graph.visuals {
            if drawn_phase10_ids.insert(visual.object_id) {
                self.draw_phase10_visual(
                    &encoder,
                    &projection,
                    visual,
                    phase10_outputs.get(&visual.object_id),
                    self.animation_time_seconds,
                );
            }
        }

        encoder.endEncoding();
    }

    fn render_phase10_outputs(
        &mut self,
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
                        let background_snapshot =
                            if phase10_visual_needs_background_snapshot(visual) {
                                self.render_phase10_background_snapshot(
                                    command_buffer,
                                    visual,
                                    &previous_layers,
                                    &mut required_background_keys,
                                )
                            } else {
                                None
                            };
                        if let Some(texture) = self.render_phase10_output_for_visual(
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

            if let Some(layer) =
                self.phase10_background_layer_for_draw_item(&draw_item, &visual_items, &text_items)
            {
                previous_layers.push(layer);
            }
        }

        for visual in &graph.visuals {
            if rendered_ids.contains(&visual.object_id)
                || !phase10_visual_requires_offscreen_chain(visual)
            {
                continue;
            }
            let background_snapshot = if phase10_visual_needs_background_snapshot(visual) {
                self.render_phase10_background_snapshot(
                    command_buffer,
                    visual,
                    &previous_layers,
                    &mut required_background_keys,
                )
            } else {
                None
            };
            if let Some(texture) = self.render_phase10_output_for_visual(
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

        self.phase10_output_textures
            .retain(|key, _| required_output_keys.contains(key));
        self.phase10_scratch_textures
            .retain(|key, _| required_scratch_keys.contains(key));
        self.phase10_named_target_textures
            .retain(|key, _| required_named_target_keys.contains(key));
        self.phase10_background_textures
            .retain(|key, _| required_background_keys.contains(key));
        outputs
    }

    fn render_phase10_output_for_visual(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        canvas_height: f64,
        visual: &ScenePhase10VisualPlan,
        background_snapshot: Option<&Phase10TextureHandle>,
        elapsed_seconds: f64,
        required_output_keys: &mut BTreeSet<String>,
        required_scratch_keys: &mut BTreeSet<String>,
        required_named_target_keys: &mut BTreeSet<String>,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let base_texture = self.phase10_base_texture_for_visual(visual)?;
        let mut previous_texture = base_texture.clone();
        let mut named_targets = BTreeMap::<String, Phase10TextureHandle>::new();
        let passes = phase10_visual_pass_chain(visual);
        if passes.is_empty() {
            return None;
        }

        let (width, height) = phase10_render_target_size(visual);
        let output_key = phase10_output_texture_key(visual.object_id, width, height);
        required_output_keys.insert(output_key.clone());
        let output_texture = self.ensure_phase10_output_target(&output_key, width, height)?;

        if visual.puppet_path.is_some() {
            return self.render_phase10_puppet_output_for_visual(
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
            return self.render_phase10_mask_chain(
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
                let key =
                    phase10_named_target_texture_key(visual.object_id, target_name, width, height);
                required_named_target_keys.insert(key.clone());
                self.ensure_phase10_named_target(&key, width, height)?
            } else if is_last {
                output_texture.clone()
            } else {
                let scratch_key = phase10_scratch_texture_key(width, height, index % 2);
                required_scratch_keys.insert(scratch_key.clone());
                self.ensure_phase10_scratch_target(&scratch_key, width, height)?
            };

            let input_scope = Phase10PassInputScope {
                local_current: Some(&base_texture),
                previous_pass: Some(&previous_texture),
                background: background_snapshot,
                copied_background: background_snapshot,
                named_targets: &named_targets,
            };
            let pass_textures = self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
            let uniforms = self.phase10_effect_uniforms_for_pass(
                resolved_pass,
                &pass_textures,
                width,
                height,
                elapsed_seconds,
            );
            let shader_defines = phase10_pass_shader_defines(resolved_pass);
            if !self.encode_phase10_pass(
                command_buffer,
                &target.texture,
                resolved_pass.pass,
                &shader_defines,
                &pass_textures,
                &uniforms,
                input_scope
                    .previous_pass
                    .map(|texture| texture.texture.clone()),
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

    fn render_phase10_puppet_output_for_visual(
        &mut self,
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
        use crate::services::scene_mdl_service::SceneMdlMeshFrame;

        let puppet_path = visual.puppet_path.as_ref()?;
        let document = self.mdl_cache.get(puppet_path)?;
        let mesh_frame =
            evaluate_scene_mdl_mesh(document, &visual.animation_layers, elapsed_seconds)?;
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
                let key =
                    phase10_named_target_texture_key(visual.object_id, target_name, width, height);
                required_named_target_keys.insert(key.clone());
                self.ensure_phase10_named_target(&key, width, height)?
            } else if is_last {
                output_texture.clone()
            } else {
                let scratch_key = phase10_scratch_texture_key(width, height, index % 2);
                required_scratch_keys.insert(scratch_key.clone());
                self.ensure_phase10_scratch_target(&scratch_key, width, height)?
            };

            let input_scope = Phase10PassInputScope {
                local_current: Some(base_texture),
                previous_pass: Some(&previous_texture),
                background: None,
                copied_background: None,
                named_targets: &named_targets,
            };
            let pass_textures = self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
            if pass_textures
                .slots
                .first()
                .and_then(|slot| slot.as_ref())
                .is_none()
            {
                return None;
            }
            let uniforms = self.phase10_effect_uniforms_for_pass(
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
            let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor)
            else {
                return None;
            };

            if phase10_alpha_prefill_required(resolved_pass.pass.blend_mode) {
                if let Some(previous_texture) = input_scope.previous_pass.map(|t| t.texture.clone())
                {
                    self.draw_phase10_fullscreen_texture(
                        &encoder,
                        previous_texture,
                        SceneRenderColor::default(),
                        SceneRenderBlendMode::Normal,
                    );
                }
            }

            for sub_frame in &submesh_frames {
                self.draw_phase10_mesh(
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

    fn render_phase10_mask_chain(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        visual: &ScenePhase10VisualPlan,
        base_texture: &Phase10TextureHandle,
        resolved_pass: &Phase10ResolvedPass<'_>,
        width: usize,
        height: usize,
        required_scratch_keys: &mut BTreeSet<String>,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let output_key = phase10_output_texture_key(visual.object_id, width, height);
        let output_texture = self.ensure_phase10_output_target(&output_key, width, height)?;

        let scratch_key = phase10_scratch_texture_key(width, height, 0);
        required_scratch_keys.insert(scratch_key.clone());
        let scratch_texture = self.ensure_phase10_scratch_target(&scratch_key, width, height)?;

        let input_scope = Phase10PassInputScope {
            local_current: Some(base_texture),
            previous_pass: Some(base_texture),
            background: None,
            copied_background: None,
            named_targets: &BTreeMap::new(),
        };
        let mask_alpha_pass_textures =
            self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
        let mask_alpha_defines = phase10_pass_shader_defines(resolved_pass);
        let mask_alpha_uniforms = self.phase10_effect_uniforms_for_pass(
            resolved_pass,
            &mask_alpha_pass_textures,
            width,
            height,
            0.0,
        );

        if !self.encode_phase10_pass(
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
        let Some(mask_apply_pipeline) = self.compiled_shader_variants.get(&mask_apply_key) else {
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

        self.draw_phase10_fullscreen_pass(
            &encoder,
            mask_apply_pipeline.as_ref(),
            &mask_apply_pass_textures,
            &Phase10EffectUniforms::default(),
            visual.base_color,
        );
        encoder.endEncoding();

        Some(output_texture.texture)
    }

    fn phase10_background_layer_for_draw_item(
        &mut self,
        draw_item: &SceneRenderDrawItem,
        visual_items: &BTreeMap<u32, &SceneRenderVisualItem>,
        text_items: &BTreeMap<u32, &SceneRenderTextItem>,
    ) -> Option<Phase10BackgroundLayer> {
        match draw_item.kind {
            SceneRenderDrawKind::Visual => {
                let item = visual_items.get(&draw_item.object_id)?;
                let texture = match item.source_kind {
                    SceneRenderSourceKind::Image => self
                        .texture_cache
                        .get(&visual_texture_cache_key(item))
                        .cloned(),
                    SceneRenderSourceKind::Video => self.video_texture_for_item(item),
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
                let texture = self
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

    fn render_phase10_background_snapshot(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        visual: &ScenePhase10VisualPlan,
        layers: &[Phase10BackgroundLayer],
        required_background_keys: &mut BTreeSet<String>,
    ) -> Option<Phase10TextureHandle> {
        let (width, height) = phase10_render_target_size(visual);
        let key = phase10_background_texture_key(visual.object_id, width, height);
        required_background_keys.insert(key.clone());
        let target = self.ensure_phase10_background_target(&key, width, height)?;

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
            self.draw_quad(
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

    fn prepare_phase10_graph(
        &mut self,
        graph: &ScenePhase10GraphPlan,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(ScenePhase10GraphPlan, Vec<NativeSceneWarning>), String> {
        let mut warnings = Vec::new();
        let mut visuals = Vec::new();
        let required_models = graph
            .visuals
            .iter()
            .filter_map(|visual| visual.puppet_path.clone())
            .collect::<BTreeSet<_>>();
        let mut required_shader_variants = BTreeSet::new();
        let mut required_output_keys = BTreeSet::new();
        let mut required_scratch_keys = BTreeSet::new();
        let mut required_named_target_keys = BTreeSet::new();
        let mut required_background_keys = BTreeSet::new();

        self.mdl_cache
            .retain(|path, _| required_models.contains(path));

        for visual in &graph.visuals {
            if let Some(puppet_path) = visual.puppet_path.as_ref() {
                if !self.mdl_cache.contains_key(puppet_path) {
                    let document = parse_scene_mdl_file(puppet_path).map_err(|error| {
                        format!(
                            "phase-10 Scene puppet {} could not be loaded: {error}",
                            puppet_path.display()
                        )
                    })?;
                    self.mdl_cache.insert(puppet_path.clone(), document);
                }
            }

            let mut draw_ready = self.ensure_phase10_base_texture_loaded(visual, required_keys)?;
            let passes = phase10_visual_pass_chain(visual);
            if phase10_visual_requires_offscreen_chain(visual) {
                let (width, height) = phase10_render_target_size(visual);
                required_output_keys.insert(phase10_output_texture_key(
                    visual.object_id,
                    width,
                    height,
                ));
                if passes.len() > 1 {
                    required_scratch_keys.insert(phase10_scratch_texture_key(width, height, 0));
                    required_scratch_keys.insert(phase10_scratch_texture_key(width, height, 1));
                }
                if phase10_visual_needs_background_snapshot(visual) {
                    required_background_keys.insert(phase10_background_texture_key(
                        visual.object_id,
                        width,
                        height,
                    ));
                }
                for resolved_pass in &passes {
                    if let Some(target_name) = phase10_resolved_pass_target_name(resolved_pass) {
                        required_named_target_keys.insert(phase10_named_target_texture_key(
                            visual.object_id,
                            target_name,
                            width,
                            height,
                        ));
                    }
                }
            }
            if passes.is_empty() {
                if draw_ready {
                    visuals.push(visual.clone());
                } else {
                    warnings.push(NativeSceneWarning::phase10_draw(
                        &visual.object_name,
                        "No draw-ready material or base texture remained after phase-10 preparation."
                            .to_string(),
                    ));
                }
                continue;
            } else {
                let mut shader_failed = false;
                for resolved_pass in &passes {
                    let shader_defines = phase10_pass_shader_defines(resolved_pass);
                    let variant_key = phase10_shader_variant_key(
                        &resolved_pass.pass.program,
                        &shader_defines,
                        resolved_pass.pass.blend_mode,
                    );
                    required_shader_variants.insert(variant_key.clone());
                    if !self.compiled_shader_variants.contains_key(&variant_key) {
                        match self.compile_phase10_shader_variant(
                            &resolved_pass.pass.program,
                            &shader_defines,
                            resolved_pass.pass.blend_mode,
                        ) {
                            Ok(pipeline) => {
                                self.compiled_shader_variants.insert(variant_key, pipeline);
                            }
                            Err(error) => {
                                warnings.push(NativeSceneWarning::phase10_draw(
                                    &visual.object_name,
                                    error,
                                ));
                                shader_failed = true;
                                break;
                            }
                        }
                    }
                    match self.ensure_phase10_pass_textures_loaded(resolved_pass, required_keys) {
                        Ok(pass_ready) => draw_ready |= pass_ready,
                        Err(error) => warnings
                            .push(NativeSceneWarning::phase10_draw(&visual.object_name, error)),
                    }
                }
                if shader_failed {
                    continue;
                }
            }

            if phase10_visual_first_pass_is_mask_alpha(visual) {
                let mask_apply_program = phase10_mask_apply_shader_program();
                let mask_apply_key = phase10_shader_variant_key(
                    &mask_apply_program,
                    &BTreeMap::new(),
                    visual.blend_mode,
                );
                required_shader_variants.insert(mask_apply_key.clone());
                if !self.compiled_shader_variants.contains_key(&mask_apply_key) {
                    match self.compile_phase10_shader_variant(
                        &mask_apply_program,
                        &BTreeMap::new(),
                        visual.blend_mode,
                    ) {
                        Ok(pipeline) => {
                            self.compiled_shader_variants
                                .insert(mask_apply_key, pipeline);
                        }
                        Err(error) => {
                            warnings
                                .push(NativeSceneWarning::phase10_draw(&visual.object_name, error));
                            continue;
                        }
                    }
                }
            }

            if draw_ready {
                visuals.push(visual.clone());
            } else {
                warnings.push(NativeSceneWarning::phase10_draw(
                    &visual.object_name,
                    "No draw-ready material or base texture remained after phase-10 preparation."
                        .to_string(),
                ));
            }
        }

        self.compiled_shader_variants
            .retain(|key, _| required_shader_variants.contains(key));
        self.phase10_output_textures
            .retain(|key, _| required_output_keys.contains(key));
        self.phase10_scratch_textures
            .retain(|key, _| required_scratch_keys.contains(key));
        self.phase10_named_target_textures
            .retain(|key, _| required_named_target_keys.contains(key));
        self.phase10_background_textures
            .retain(|key, _| required_background_keys.contains(key));

        Ok((
            ScenePhase10GraphPlan {
                visuals,
                consumed_visual_ids: graph.consumed_visual_ids.clone(),
            },
            warnings,
        ))
    }

    fn ensure_phase10_base_texture_loaded(
        &mut self,
        visual: &ScenePhase10VisualPlan,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<bool, String> {
        match visual.base_source_kind {
            Some(SceneRenderSourceKind::Image) => {
                let Some(base_texture_path) = visual.base_texture_path.as_ref() else {
                    return Ok(false);
                };
                self.ensure_phase10_texture_loaded(base_texture_path, required_keys)?;
                Ok(true)
            }
            Some(SceneRenderSourceKind::Video) => {
                Ok(self.video_sources.contains_key(&visual.object_id))
            }
            None => {
                if visual.base_color.alpha == 0 {
                    return Ok(false);
                }
                let (width, height) = phase10_render_target_size(visual);
                self.ensure_phase10_solid_texture(visual.base_color, width, height, required_keys)?;
                Ok(true)
            }
        }
    }

    fn ensure_phase10_pass_textures_loaded(
        &mut self,
        resolved_pass: &Phase10ResolvedPass<'_>,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<bool, String> {
        let mut pass_ready = false;
        for texture_path in resolved_pass
            .pass
            .textures
            .iter()
            .filter_map(|binding| binding.resolved_path.as_ref())
        {
            self.ensure_phase10_texture_loaded(texture_path, required_keys)?;
            pass_ready = true;
        }
        if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
            for texture_path in effect_pass.texture_overrides.iter().flatten() {
                self.ensure_phase10_texture_loaded(texture_path, required_keys)?;
                pass_ready = true;
            }
        }
        Ok(pass_ready)
    }

    fn ensure_phase10_solid_texture(
        &mut self,
        color: SceneRenderColor,
        width: usize,
        height: usize,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        let key = phase10_solid_texture_key(color, width, height);
        required_keys.insert(key.clone());
        if self.texture_cache.contains_key(&key) {
            return Ok(());
        }
        let texture = load_texture(
            &self.device,
            build_solid_texture_image(color, width, height),
        )
        .map_err(|error| format!("unable to upload phase-10 solid texture {key}: {error}"))?;
        self.texture_resolution_cache.insert(
            key.clone(),
            phase10_texture_metrics_from_size(width, height),
        );
        self.texture_cache.insert(key, texture);
        Ok(())
    }

    fn ensure_phase10_texture_loaded(
        &mut self,
        path: &Path,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        let key = phase10_texture_cache_key(path);
        required_keys.insert(key.clone());
        if self.texture_cache.contains_key(&key) {
            return Ok(());
        }

        let decoded = load_phase10_texture_source(path)?;
        let texture = load_texture(&self.device, decoded.image).map_err(|error| {
            format!(
                "unable to upload phase-10 texture {}: {error}",
                path.display()
            )
        })?;
        self.texture_resolution_cache
            .insert(key.clone(), decoded.metrics);
        self.texture_cache.insert(key, texture);
        Ok(())
    }
    fn compile_phase10_shader_variant(
        &self,
        program: &SceneShaderProgram,
        defines: &BTreeMap<String, i32>,
        blend_mode: SceneRenderBlendMode,
    ) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, String> {
        let source = load_shader_program_source(program, defines)?;
        let source = NSString::from_str(source.as_str());
        let library = self
            .device
            .newLibraryWithSource_options_error(&source, None)
            .map_err(|error| {
                format!(
                    "failed to compile phase-10 Scene shader {}: {error:?}",
                    program.metal_source_path.display()
                )
            })?;
        let vertex_name = NSString::from_str(program.vertex_entry);
        let fragment_name = NSString::from_str(program.fragment_entry);
        let vertex_function = library.newFunctionWithName(&vertex_name).ok_or_else(|| {
            format!(
                "phase-10 shader {} is missing vertex entry {}",
                program.metal_source_path.display(),
                program.vertex_entry
            )
        })?;
        let fragment_function = library.newFunctionWithName(&fragment_name).ok_or_else(|| {
            format!(
                "phase-10 shader {} is missing fragment entry {}",
                program.metal_source_path.display(),
                program.fragment_entry
            )
        })?;
        build_pipeline_state(
            self.device.as_ref(),
            vertex_function.as_ref(),
            fragment_function.as_ref(),
            blend_mode,
        )
    }

    fn draw_phase10_visual(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        projection: &SceneProjection,
        visual: &ScenePhase10VisualPlan,
        rendered_output: Option<&Retained<ProtocolObject<dyn MTLTexture>>>,
        elapsed_seconds: f64,
    ) {
        if let Some(output_texture) = rendered_output {
            self.draw_quad(
                encoder,
                output_texture.as_ref(),
                visual.blend_mode,
                projection,
                SceneQuadPrimitive {
                    left: visual.quad.left,
                    top: visual.quad.top,
                    width: visual.quad.width,
                    height: visual.quad.height,
                    rotation: visual.quad.rotation,
                    opacity: visual.quad.opacity,
                    flip_x: visual.quad.flip_x,
                    flip_y: visual.quad.flip_y,
                    uv_rect: full_quad_uv_rect(),
                    color: visual
                        .base_source_kind
                        .map(|_| visual.base_color)
                        .unwrap_or_default(),
                    transform_origin_x: visual.quad.left + visual.quad.width / 2.0,
                    transform_origin_y: visual.quad.top + visual.quad.height / 2.0,
                },
            );
            return;
        }

        let passes = phase10_visual_pass_chain(visual);
        if passes.is_empty() {
            let Some(texture) = self.phase10_base_texture_for_visual(visual) else {
                return;
            };
            self.draw_quad(
                encoder,
                texture.texture.as_ref(),
                visual.blend_mode,
                projection,
                SceneQuadPrimitive {
                    left: visual.quad.left,
                    top: visual.quad.top,
                    width: visual.quad.width,
                    height: visual.quad.height,
                    rotation: visual.quad.rotation,
                    opacity: visual.quad.opacity,
                    flip_x: visual.quad.flip_x,
                    flip_y: visual.quad.flip_y,
                    uv_rect: full_quad_uv_rect(),
                    color: visual.base_color,
                    transform_origin_x: visual.quad.left + visual.quad.width / 2.0,
                    transform_origin_y: visual.quad.top + visual.quad.height / 2.0,
                },
            );
            return;
        }

        if let Some(puppet_path) = visual.puppet_path.as_ref() {
            let Some(document) = self.mdl_cache.get(puppet_path) else {
                return;
            };
            let Some(mesh_frame) =
                evaluate_scene_mdl_mesh(document, &visual.animation_layers, elapsed_seconds)
            else {
                return;
            };
            if mesh_frame.positions.is_empty() || mesh_frame.indices.len() < 3 {
                return;
            }
            let base_texture = self.phase10_base_texture_for_visual(visual);
            let named_targets = BTreeMap::<String, Phase10TextureHandle>::new();
            for resolved_pass in &passes {
                let input_scope = Phase10PassInputScope {
                    local_current: base_texture.as_ref(),
                    previous_pass: base_texture.as_ref(),
                    background: None,
                    copied_background: None,
                    named_targets: &named_targets,
                };
                let pass_textures =
                    self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
                if pass_textures
                    .slots
                    .first()
                    .and_then(|slot| slot.as_ref())
                    .is_none()
                {
                    continue;
                }
                let uniforms = self.phase10_effect_uniforms_for_pass(
                    resolved_pass,
                    &pass_textures,
                    phase10_render_target_size(visual).0,
                    phase10_render_target_size(visual).1,
                    elapsed_seconds,
                );
                let shader_defines = phase10_pass_shader_defines(resolved_pass);
                self.draw_phase10_mesh(
                    encoder,
                    projection,
                    visual,
                    &mesh_frame,
                    resolved_pass.pass,
                    &shader_defines,
                    &pass_textures,
                    &uniforms,
                );
            }
        } else {
            let Some(texture) = self.phase10_base_texture_for_visual(visual) else {
                return;
            };
            self.draw_quad(
                encoder,
                texture.texture.as_ref(),
                visual.blend_mode,
                projection,
                SceneQuadPrimitive {
                    left: visual.quad.left,
                    top: visual.quad.top,
                    width: visual.quad.width,
                    height: visual.quad.height,
                    rotation: visual.quad.rotation,
                    opacity: visual.quad.opacity,
                    flip_x: visual.quad.flip_x,
                    flip_y: visual.quad.flip_y,
                    uv_rect: full_quad_uv_rect(),
                    color: visual.base_color,
                    transform_origin_x: visual.quad.left + visual.quad.width / 2.0,
                    transform_origin_y: visual.quad.top + visual.quad.height / 2.0,
                },
            );
        }
    }

    fn phase10_base_texture_for_visual(
        &mut self,
        visual: &ScenePhase10VisualPlan,
    ) -> Option<Phase10TextureHandle> {
        match visual.base_source_kind {
            Some(SceneRenderSourceKind::Image) => visual
                .base_texture_path
                .as_deref()
                .and_then(|path| self.phase10_texture_for_path(path)),
            Some(SceneRenderSourceKind::Video) => {
                let source = self.video_sources.get_mut(&visual.object_id)?;
                match source.current_texture(&self.video_texture_cache, self.paused) {
                    Ok(texture) => texture.map(|texture| Phase10TextureHandle {
                        metrics: phase10_texture_metrics_from_texture(texture.as_ref()),
                        texture,
                    }),
                    Err(error) => {
                        let detail = video_texture_frame_warning(&visual.object_name, error);
                        let _ = diagnostic_service::record_warning(
                            &self.app,
                            DIAGNOSTIC_SUBSYSTEM,
                            &detail.code,
                            detail.message.clone(),
                            detail.detail_json(),
                        );
                        None
                    }
                }
            }
            None => {
                let (width, height) = phase10_render_target_size(visual);
                self.texture_cache
                    .get(&phase10_solid_texture_key(visual.base_color, width, height))
                    .cloned()
                    .map(|texture| Phase10TextureHandle {
                        texture,
                        metrics: *self
                            .texture_resolution_cache
                            .get(&phase10_solid_texture_key(visual.base_color, width, height))
                            .unwrap_or(&phase10_texture_metrics_from_size(width, height)),
                    })
            }
        }
    }

    fn phase10_texture_for_path(&self, path: &Path) -> Option<Phase10TextureHandle> {
        let key = phase10_texture_cache_key(path);
        let texture = self.texture_cache.get(&key)?.clone();
        let metrics = self
            .texture_resolution_cache
            .get(&key)
            .copied()
            .unwrap_or_else(|| phase10_texture_metrics_from_texture(texture.as_ref()));
        Some(Phase10TextureHandle { texture, metrics })
    }

    fn phase10_pass_textures_for(
        &self,
        visual: &ScenePhase10VisualPlan,
        resolved_pass: &Phase10ResolvedPass<'_>,
        input_scope: &Phase10PassInputScope<'_>,
    ) -> Phase10PassTextures {
        let mut slots = BTreeMap::<usize, Phase10TextureHandle>::new();

        match resolved_pass.context {
            Phase10PassContext::Base => {
                for binding in &resolved_pass.pass.textures {
                    let Some(texture) = binding
                        .resolved_path
                        .as_deref()
                        .and_then(|path| self.phase10_texture_for_path(path))
                    else {
                        continue;
                    };
                    slots.insert(binding.slot_index, texture);
                }
                if let Some(texture) = input_scope.local_current {
                    slots.entry(0).or_insert(texture.clone());
                }
            }
            Phase10PassContext::Effect(effect_pass) => {
                for (slot, source) in
                    phase10_effect_texture_slot_plan(&resolved_pass.pass.textures, effect_pass)
                {
                    let texture = match source {
                        Phase10EffectTextureSource::GraphInput(input_source) => {
                            input_scope.texture_for(&input_source)
                        }
                        Phase10EffectTextureSource::MaterialSlot(binding_slot) => resolved_pass
                            .pass
                            .textures
                            .iter()
                            .find(|binding| binding.slot_index == binding_slot)
                            .and_then(|binding| binding.resolved_path.as_deref())
                            .and_then(|path| self.phase10_texture_for_path(path)),
                        Phase10EffectTextureSource::OverrideSlot(override_slot) => effect_pass
                            .texture_overrides
                            .get(override_slot)
                            .and_then(|path| path.as_deref())
                            .and_then(|path| self.phase10_texture_for_path(path)),
                    };
                    if let Some(texture) = texture {
                        slots.insert(slot, texture);
                    }
                }
            }
        }

        if slots.is_empty() && visual.base_source_kind.is_none() && visual.base_color.alpha > 0 {
            let (width, height) = phase10_render_target_size(visual);
            if let Some(texture) = self
                .texture_cache
                .get(&phase10_solid_texture_key(visual.base_color, width, height))
                .cloned()
            {
                slots.insert(
                    0,
                    Phase10TextureHandle {
                        metrics: *self
                            .texture_resolution_cache
                            .get(&phase10_solid_texture_key(visual.base_color, width, height))
                            .unwrap_or(&phase10_texture_metrics_from_size(width, height)),
                        texture,
                    },
                );
            }
        }

        let max_slot = slots.keys().next_back().copied().unwrap_or(0);
        let mut ordered = vec![None; max_slot + 1];
        for (slot, texture) in slots {
            ordered[slot] = Some(texture);
        }

        Phase10PassTextures { slots: ordered }
    }

    fn phase10_effect_uniforms_for_pass(
        &self,
        resolved_pass: &Phase10ResolvedPass<'_>,
        pass_textures: &Phase10PassTextures,
        width: usize,
        height: usize,
        elapsed_seconds: f64,
    ) -> Phase10EffectUniforms {
        let effect_kind = phase10_effect_family_from_program(&resolved_pass.pass.program);
        let mut uniforms = Phase10EffectUniforms {
            color: [1.0, 1.0, 1.0, 1.0],
            user0: [0.0, 0.0, 0.0, 0.0],
            user1: [0.0, 0.0, 0.0, 0.0],
            primary_resolution: phase10_texture_resolution(
                pass_textures.slots.first().and_then(|slot| slot.as_ref()),
            ),
            slot1_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(1).and_then(|slot| slot.as_ref()),
            ),
            slot2_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(2).and_then(|slot| slot.as_ref()),
            ),
            slot3_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(3).and_then(|slot| slot.as_ref()),
            ),
            slot4_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(4).and_then(|slot| slot.as_ref()),
            ),
            slot5_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(5).and_then(|slot| slot.as_ref()),
            ),
            slot6_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(6).and_then(|slot| slot.as_ref()),
            ),
            slot7_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(7).and_then(|slot| slot.as_ref()),
            ),
            texel_size: phase10_texel_size(
                pass_textures.slots.first().and_then(|slot| slot.as_ref()),
            ),
            aux_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(1).and_then(|slot| slot.as_ref()),
            ),
            aux2_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(2).and_then(|slot| slot.as_ref()),
            ),
            aux3_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(3).and_then(|slot| slot.as_ref()),
            ),
            aux4_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(4).and_then(|slot| slot.as_ref()),
            ),
            aux5_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(5).and_then(|slot| slot.as_ref()),
            ),
            aux6_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(6).and_then(|slot| slot.as_ref()),
            ),
            aux7_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(7).and_then(|slot| slot.as_ref()),
            ),
            screen_size: [width.max(1) as f32, height.max(1) as f32],
            time: elapsed_seconds as f32,
            intensity: 1.0,
            speed: 1.0,
            radius: 1.0,
            angle: 0.0,
        };
        let uniform_values = phase10_effect_uniform_values(resolved_pass);

        match effect_kind {
            Some(SceneCompatEffectKind::Pulse) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["amount", "pulseamount"], 1.0);
                uniforms.speed =
                    phase10_uniform_float(&uniform_values, &["speed", "pulsespeed"], 3.0);
                uniforms.user0[0] =
                    phase10_uniform_float(&uniform_values, &["phase", "pulsephase"], 0.0);
                uniforms.user0[1] =
                    phase10_uniform_float(&uniform_values, &["power"], 1.0).max(0.001);
                let bounds = phase10_uniform_vec2(
                    &uniform_values,
                    &["bounds", "pulsethresholds"],
                    [0.0, 1.0],
                );
                uniforms.user0[2] = bounds[0];
                uniforms.user0[3] = bounds[1];
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["noisespeed"], 0.5).max(0.0);
                uniforms.angle = phase10_uniform_float(&uniform_values, &["noiseamount"], 0.0);
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["tintlow", "tintcolor1"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                let tint_high = phase10_uniform_color(
                    &uniform_values,
                    &["tinthigh", "tintcolor2"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                uniforms.user1 = [tint_high[0], tint_high[1], tint_high[2], 1.0];
            }
            Some(SceneCompatEffectKind::Shake) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["strength", "amp"], 0.1);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
                let bounds =
                    phase10_uniform_vec2(&uniform_values, &["bounds", "gbounds"], [0.0, 1.0]);
                let friction =
                    phase10_uniform_vec2(&uniform_values, &["friction", "gfriction"], [1.0, 1.0]);
                uniforms.user0[0] = bounds[0];
                uniforms.user0[1] = bounds[1];
                uniforms.user1[0] = friction[0];
                uniforms.user1[1] = friction[1];
            }
            Some(SceneCompatEffectKind::WaterRipple) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["ripplestrength", "strength"], 0.1);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["animationspeed"], 0.15);
                uniforms.radius = phase10_uniform_float(&uniform_values, &["scale"], 1.0);
                uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["scrollspeed"], 0.0);
                uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["ratio"], 1.0);
                uniforms.angle =
                    phase10_uniform_float(&uniform_values, &["scrolldirection", "direction"], 0.0);
            }
            Some(SceneCompatEffectKind::WaterWaves) => {
                uniforms.intensity = phase10_uniform_float(&uniform_values, &["strength"], 0.1);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 5.0);
                uniforms.angle = phase10_uniform_float(&uniform_values, &["direction"], 0.0);
                let direction = rotate2d([0.0, 1.0], uniforms.angle);
                uniforms.user0[0] = direction[0];
                uniforms.user0[1] = direction[1];
                uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["scale"], 200.0);
                uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["exponent"], 1.0);
            }
            Some(SceneCompatEffectKind::Tint) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["alpha", "blendalpha"], 1.0);
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["color", "tintcolor"],
                    [1.0, 0.0, 0.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::Scroll) => {
                uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["speedx"], 0.2);
                uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["speedy"], 0.2);
                let repeat =
                    phase10_uniform_vec2(&uniform_values, &["repeat", "scale"], [1.0, 1.0]);
                uniforms.user0[2] = repeat[0];
                uniforms.user0[3] = repeat[1];
            }
            Some(SceneCompatEffectKind::LightShafts) => {
                uniforms.intensity = phase10_uniform_float(
                    &uniform_values,
                    &["colorwintensity", "intensity"],
                    1.0,
                );
                uniforms.speed =
                    phase10_uniform_float(&uniform_values, &["rayspeed", "speed"], 0.2);
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["rayradius", "radius"], 0.5);
                uniforms.user0 =
                    phase10_uniform_vec4(&uniform_values, &["rayscale", "scale"], [0.5, 0.1, 0.0, 0.0]);
                let feather =
                    phase10_uniform_vec2(&uniform_values, &["rayfeather", "feather"], [0.05, 0.2]);
                uniforms.user0[2] = feather[0];
                uniforms.user0[3] = feather[1];
                uniforms.user1[0] = phase10_uniform_float(
                    &uniform_values,
                    &["raysmoothness", "smoothness"],
                    0.75,
                );
                uniforms.user1[1] = phase10_uniform_float(
                    &uniform_values,
                    &["noiseamount", "noise"],
                    0.33,
                );
                uniforms.user1[2] = phase10_uniform_float(
                    &uniform_values,
                    &["noisescale"],
                    1.0,
                );
                uniforms.user1[3] = phase10_uniform_float(
                    &uniform_values,
                    &["colorwexponent", "exponent"],
                    0.5,
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["colorastart", "colorstart"],
                    [1.0, 1.0, 1.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::FoliageSway) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["strength"], 33.34) / 100.0;
                uniforms.speed =
                    phase10_uniform_float(&uniform_values, &["speed"], 3.0);
                uniforms.user0[0] =
                    phase10_uniform_float(&uniform_values, &["phase"], 0.0);
                uniforms.user0[1] =
                    phase10_uniform_float(&uniform_values, &["power"], 1.0);
                uniforms.user0[2] = phase10_uniform_float(
                    &uniform_values,
                    &["mode"],
                    0.0,
                );
            }
            Some(SceneCompatEffectKind::Circle) => {}
            Some(SceneCompatEffectKind::Opacity) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["alpha", "useralpha"], 1.0);
            }
            Some(SceneCompatEffectKind::Transform) => {
                (uniforms.user0, uniforms.user1, uniforms.angle) =
                    phase10_transform_controls(&uniform_values);
            }
            Some(SceneCompatEffectKind::Skew) => {
                uniforms.user0 = phase10_skew_controls(&uniform_values);
            }
            Some(SceneCompatEffectKind::Perspective) => {
                (uniforms.user0, uniforms.user1) =
                    phase10_perspective_corner_uniforms(&uniform_values);
            }
            Some(SceneCompatEffectKind::Spin) => {
                (uniforms.angle, uniforms.speed, uniforms.user0) =
                    phase10_spin_controls(&uniform_values);
            }
            Some(SceneCompatEffectKind::Swing) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["amount"], 0.2);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["size"], 0.4);
                uniforms.angle =
                    phase10_uniform_float(&uniform_values, &["center"], 0.5);
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &["point0"],
                    [0.25, 0.5, 0.75, 0.5],
                );
                let point1 = phase10_uniform_vec2(&uniform_values, &["point1"], [0.75, 0.5]);
                uniforms.user0[2] = point1[0];
                uniforms.user0[3] = point1[1];
                uniforms.user1[0] =
                    phase10_uniform_float(&uniform_values, &["feather"], 0.01);
                uniforms.user1[1] =
                    phase10_uniform_float(&uniform_values, &["noisespeed"], 0.15);
                uniforms.user1[2] =
                    phase10_uniform_float(&uniform_values, &["noiseamount"], 0.2);
            }
            Some(SceneCompatEffectKind::Twirl) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["amount"], 0.2);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["size"], 0.5);
                uniforms.angle =
                    phase10_uniform_float(&uniform_values, &["angle"], 0.0);
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &["center"],
                    [0.5, 0.5, 1.0, 0.002],
                );
                uniforms.user0[2] =
                    phase10_uniform_float(&uniform_values, &["ratio"], 1.0);
                uniforms.user0[3] =
                    phase10_uniform_float(&uniform_values, &["feather"], 0.002);
                uniforms.user1[0] =
                    phase10_uniform_float(&uniform_values, &["noisespeed"], 0.15);
                uniforms.user1[1] =
                    phase10_uniform_float(&uniform_values, &["noiseamount"], 0.5);
            }
            Some(SceneCompatEffectKind::ChromaticAberration) => {
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &[
                        "center",
                        "uieditorpropertiescenter",
                        "ui_editor_properties_center",
                    ],
                    [0.5, 0.5, 0.5, 9.0],
                );
                uniforms.user0[2] = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "centerfalloff",
                        "uieditorpropertiescenterfalloff",
                        "ui_editor_properties_center_falloff",
                    ],
                    uniforms.user0[2],
                );
                uniforms.user0[3] = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "strength",
                        "uieditorpropertiesstrength",
                        "ui_editor_properties_strength",
                    ],
                    uniforms.user0[3],
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "direction",
                        "uieditorpropertiesdirection",
                        "ui_editor_properties_direction",
                    ],
                    1.5707964,
                );
            }
            Some(SceneCompatEffectKind::ColorKey) => {
                uniforms.intensity = phase10_uniform_float(&uniform_values, &["alpha"], 0.0);
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["fuzziness", "keyfuzz"],
                    0.0,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["tolerance", "keytolerance"],
                    0.1,
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["color", "keycolor"],
                    [1.0, 1.0, 1.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::FishEye) => {
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &["center"],
                    [0.5, 0.5, 1.0, 1.0],
                );
                uniforms.user0[2] =
                    phase10_uniform_float(&uniform_values, &["size"], uniforms.user0[2]);
                uniforms.user0[3] = phase10_uniform_float(
                    &uniform_values,
                    &["distortion", "scale"],
                    uniforms.user0[3],
                );
            }
            Some(SceneCompatEffectKind::EdgeDetection) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["alpha", "blendalpha"], 1.0);
                uniforms.speed = phase10_uniform_float(
                    &uniform_values,
                    &["brightness", "blendbrightness"],
                    1.0,
                );
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["detectthreshold", "detectionthreshold"],
                    0.5,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["detectmultiply", "detectionmultiply"],
                    1.0,
                );
                let color1 = phase10_uniform_color(
                    &uniform_values,
                    &["outlinecolor"],
                    [0.0, 0.0, 0.0, 1.0],
                );
                let color2 = phase10_uniform_color(
                    &uniform_values,
                    &["outlinebackground"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                uniforms.color = color1;
                uniforms.user0 = [color2[0], color2[1], color2[2], 1.0];
            }
            Some(SceneCompatEffectKind::Iris) => {
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &["scale"],
                    [20.0, 20.0, 0.0, 0.0],
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["color", "eyecolor"],
                    [1.0, 1.0, 1.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::CloudMotion) => {
                uniforms.intensity = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "amount",
                        "uieditorpropertiesamount",
                        "ui_editor_properties_amount",
                    ],
                    0.1,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "direction",
                        "uieditorpropertiesdirection",
                        "ui_editor_properties_direction",
                    ],
                    1.5707964,
                );
            }
            Some(SceneCompatEffectKind::Clouds) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["alpha", "cloudsalpha"], 1.0);
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["threshold", "cloudthreshold"],
                    0.0,
                );
                uniforms.user0[0] = phase10_uniform_float(
                    &uniform_values,
                    &["feather", "cloudfeather"],
                    0.5,
                );
                uniforms.user0[1] = phase10_uniform_float(
                    &uniform_values,
                    &["smoothness", "cloudlod"],
                    0.0,
                );
                let speed = phase10_uniform_vec4(
                    &uniform_values,
                    &["speed", "cloudspeeds"],
                    [0.01, 0.01, -0.02, -0.02],
                );
                let scale = phase10_uniform_vec4(
                    &uniform_values,
                    &["scale", "cloudscales"],
                    [1.3, 1.3, 0.5, 0.5],
                );
                uniforms.user1 = [speed[0], speed[1], scale[0], scale[2]];
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["colorstart", "color1"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                let color2 = phase10_uniform_color(
                    &uniform_values,
                    &["colorend", "color2"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                uniforms.user0[2] = color2[0];
                uniforms.user0[3] = color2[1];
            }
            Some(SceneCompatEffectKind::WaterFlow) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["strength", "flowamp"], 1.0);
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["phasescale", "flowphasescale"], 2.0);
            }
            Some(SceneCompatEffectKind::Nitro) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["multiply", "nitroalpha"], 1.0);
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["smoothness", "nitrolod"],
                    1.0,
                );
                let bounds =
                    phase10_uniform_vec2(&uniform_values, &["bounds", "nitroranges"], [0.3, 0.25]);
                uniforms.user0[0] = bounds[0];
                uniforms.user0[1] = bounds[1];
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["colorstart", "nitrocolor0"],
                    [0.0, 0.5, 1.0, 1.0],
                );
                let color1 = phase10_uniform_color(
                    &uniform_values,
                    &["colorend", "nitrocolor1"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                uniforms.user0[2] = color1[0];
                uniforms.user0[3] = color1[1];
                uniforms.user1[0] = color1[2];
            }
            Some(SceneCompatEffectKind::Blend) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["multiply"], 1.0);
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["alpha", "alphamultiply"], 1.0);
            }
            Some(SceneCompatEffectKind::DepthParallax) => {
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &["scale"],
                    [1.0, 1.0, 0.3, 0.0],
                );
                uniforms.user0[2] =
                    phase10_uniform_float(&uniform_values, &["center"], uniforms.user0[2]);
                uniforms.user0[3] =
                    phase10_uniform_float(&uniform_values, &["sens"], 1.0);
            }
            Some(SceneCompatEffectKind::Reflection) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["alpha", "reflectionalpha"], 1.0);
            }
            Some(SceneCompatEffectKind::Shimmer) => {
                uniforms.intensity = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "uieditorpropertiesamount",
                        "uieditorpropertiesbrightness",
                        "ui_editor_properties_brightness",
                    ],
                    1.0,
                );
                uniforms.speed = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesspeed", "ui_editor_properties_speed"],
                    1.0,
                );
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesdelay", "ui_editor_properties_delay"],
                    2.0,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesdirection", "ui_editor_properties_direction"],
                    1.5707964,
                );
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &[
                        "uieditorpropertiesgranularity",
                        "ui_editor_properties_granularity",
                    ],
                    [1.0, 1.0, 0.0, 0.05],
                );
                uniforms.user0[1] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertieswidth", "ui_editor_properties_width"],
                    uniforms.user0[1],
                );
                uniforms.user0[2] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesoffset", "ui_editor_properties_offset"],
                    uniforms.user0[2],
                );
                uniforms.user0[3] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiestimescale", "ui_editor_properties_timescale"],
                    uniforms.user0[3],
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["uieditorpropertiescolor", "ui_editor_properties_color"],
                    [1.0, 1.0, 1.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::FilmGrain) => {
                uniforms.intensity =
                    phase10_uniform_float(
                        &uniform_values,
                        &["strength", "uieditorpropertiesstrength", "noisealpha"],
                        1.0,
                    );
                uniforms.radius =
                    phase10_uniform_float(
                        &uniform_values,
                        &["exponent", "uieditorpropertiespower", "noisepower"],
                        0.5,
                    );
            }
            Some(SceneCompatEffectKind::Vhs) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["strength", "noisealpha"], 1.0);
                uniforms.speed = phase10_uniform_float(
                    &uniform_values,
                    &["distortionspeed"],
                    1.0,
                );
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["distortionstrength"],
                    1.0,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["distortionwidth"],
                    1.0,
                );
                uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["scale"], 0.3);
                uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["artifacts"], 1.0);
                uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["chromatic"], 0.3);
                uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["tracking"], 0.5);
            }
            Some(SceneCompatEffectKind::BlendGradient) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["multiply"], 1.0);
                uniforms.speed = phase10_uniform_float(
                    &uniform_values,
                    &["gradientscale"],
                    0.05,
                );
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["alpha", "alphamultiply"], 1.0);
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["edgebrightness"],
                    1.0,
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["edgecolor"],
                    [1.0, 0.75, 0.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::WaterCaustics) => {
                uniforms.intensity = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesbrightness"],
                    1.0,
                );
                uniforms.speed = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesspeed"],
                    1.0,
                );
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesgranularity"],
                    2.0,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesglow"],
                    0.5,
                );
                uniforms.user0[0] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesdistortion"],
                    1.0,
                );
                uniforms.user0[1] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertieschromaticaberration"],
                    1.0,
                );
                uniforms.user0[2] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiesblur"],
                    0.0,
                );
                uniforms.user0[3] = phase10_uniform_float(
                    &uniform_values,
                    &["uieditorpropertiestimeoffset"],
                    0.0,
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["uieditorpropertiescolorstart"],
                    [0.7, 0.9, 1.0, 1.0],
                );
                let color2 = phase10_uniform_color(
                    &uniform_values,
                    &["uieditorpropertiescolorend"],
                    [0.4, 0.6, 1.0, 1.0],
                );
                uniforms.user1[0] = color2[0];
                uniforms.user1[1] = color2[1];
                uniforms.user1[2] = color2[2];
            }
            Some(SceneCompatEffectKind::Fire) => {
                uniforms.intensity = phase10_uniform_float(
                    &uniform_values,
                    &["alpha", "uieditorpropertiesalpha"],
                    2.0,
                );
                uniforms.speed = phase10_uniform_float(
                    &uniform_values,
                    &["speed", "uieditorpropertiesspeed"],
                    1.0,
                );
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &["phasescale"],
                    1.0,
                );
                uniforms.angle = phase10_uniform_float(
                    &uniform_values,
                    &["distortion"],
                    1.0,
                );
                uniforms.user0 = phase10_uniform_vec4(
                    &uniform_values,
                    &["scale", "uieditorpropertiesscale"],
                    [2.0, 0.0, 0.0, 0.0],
                );
                uniforms.user0[1] = phase10_uniform_float(
                    &uniform_values,
                    &["threshold", "uieditorpropertiesthreshold"],
                    0.0,
                );
                uniforms.user0[2] = phase10_uniform_float(
                    &uniform_values,
                    &["feather", "uieditorpropertiesfeather"],
                    0.5,
                );
                uniforms.user0[3] = phase10_uniform_float(
                    &uniform_values,
                    &["smoothness", "uieditorpropertiessmoothness"],
                    0.0,
                );
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["colorstart", "uieditorpropertiescolorstart"],
                    [1.0, 0.25, 0.0, 1.0],
                );
                let color2 = phase10_uniform_color(
                    &uniform_values,
                    &["colorend", "uieditorpropertiescolorend"],
                    [1.0, 0.8, 0.0, 1.0],
                );
                uniforms.user1[0] = color2[0];
                uniforms.user1[1] = color2[1];
                uniforms.user1[2] = color2[2];
            }
            Some(SceneCompatEffectKind::XRay) => {
                uniforms.intensity =
                    phase10_uniform_float(
                        &uniform_values,
                        &["multiply", "uieditorpropertiesmultiply"],
                        1.0,
                    );
                uniforms.radius = phase10_uniform_float(
                    &uniform_values,
                    &[
                        "uieditorparticleelementexponent",
                        "ui_editor_particle_element_exponent",
                    ],
                    1.0,
                );
            }
            _ => {}
        }

        uniforms
    }

    fn encode_phase10_pass(
        &self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        target: &Retained<ProtocolObject<dyn MTLTexture>>,
        pass: &SceneMaterialPassPlan,
        shader_defines: &BTreeMap<String, i32>,
        pass_textures: &Phase10PassTextures,
        uniforms: &Phase10EffectUniforms,
        previous_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    ) -> bool {
        let variant_key =
            phase10_shader_variant_key(&pass.program, shader_defines, pass.blend_mode);
        let Some(pipeline) = self.compiled_shader_variants.get(&variant_key) else {
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
            if let Some(previous_texture) = previous_texture {
                self.draw_phase10_fullscreen_texture(
                    &encoder,
                    previous_texture,
                    SceneRenderColor::default(),
                    SceneRenderBlendMode::Normal,
                );
            }
        }

        self.draw_phase10_fullscreen_pass(
            &encoder,
            pipeline.as_ref(),
            pass_textures,
            uniforms,
            SceneRenderColor::default(),
        );
        encoder.endEncoding();
        true
    }

    fn ensure_phase10_output_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_output_textures,
            key,
            width,
            height,
        )
    }

    fn ensure_phase10_scratch_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_scratch_textures,
            key,
            width,
            height,
        )
    }

    fn ensure_phase10_named_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_named_target_textures,
            key,
            width,
            height,
        )
    }

    fn ensure_phase10_background_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_background_textures,
            key,
            width,
            height,
        )
    }

    fn draw_phase10_fullscreen_texture(
        &self,
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
        self.draw_phase10_fullscreen_pass(
            encoder,
            self.pipeline_for(blend_mode),
            &pass_textures,
            &Phase10EffectUniforms::default(),
            tint,
        );
    }

    fn draw_phase10_fullscreen_pass(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
        pass_textures: &Phase10PassTextures,
        uniforms: &Phase10EffectUniforms,
        tint: SceneRenderColor,
    ) {
        let vertices = phase10_fullscreen_vertices(tint);
        self.draw_phase10_vertices_with_pipeline(
            encoder,
            pipeline,
            pass_textures,
            uniforms,
            &vertices,
        );
    }

    fn draw_phase10_mesh(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        projection: &SceneProjection,
        visual: &ScenePhase10VisualPlan,
        mesh_frame: &crate::services::scene_mdl_service::SceneMdlMeshFrame,
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
        let variant_key =
            phase10_shader_variant_key(&pass.program, shader_defines, pass.blend_mode);
        let Some(pipeline) = self.compiled_shader_variants.get(&variant_key) else {
            return;
        };
        self.draw_phase10_vertices_with_pipeline(
            encoder,
            pipeline.as_ref(),
            pass_textures,
            uniforms,
            &vertices,
        );
    }

    fn draw_phase10_vertices_with_pipeline(
        &self,
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
                    let Some(vertex_buffer) = self.device.newBufferWithBytes_length_options(
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

    fn frame_delta_seconds(&mut self) -> f64 {
        let now = Instant::now();
        let delta_seconds = now.duration_since(self.last_frame_at).as_secs_f64();
        self.last_frame_at = now;
        delta_seconds.clamp(1.0 / 240.0, 0.25)
    }

    fn update_input_frame(
        &mut self,
        view: &MTKView,
        plan: &SceneRenderPlan,
        delta_seconds: f64,
    ) -> SceneInputCoordinatorFrame {
        let projection = scene_input_projection_for_view(&self.app, view, plan);
        self.input_coordinator.update(SceneInputCoordinatorUpdate {
            target: projection.target,
            camera: plan.camera.clone(),
            canvas_width: plan.canvas_width,
            canvas_height: plan.canvas_height,
            delta_seconds,
            scene_time_seconds: self.animation_time_seconds,
            projection_diagnostics: projection.diagnostics,
        })
    }

    fn pipeline_for(
        &self,
        blend_mode: SceneRenderBlendMode,
    ) -> &ProtocolObject<dyn MTLRenderPipelineState> {
        match blend_mode {
            SceneRenderBlendMode::Normal => self.pipelines.normal.as_ref(),
            SceneRenderBlendMode::Additive => self.pipelines.additive.as_ref(),
            SceneRenderBlendMode::Multiply => self.pipelines.multiply.as_ref(),
        }
    }

    fn ensure_visual_texture_loaded(
        &mut self,
        item: &SceneRenderVisualItem,
        key: &str,
    ) -> Result<(), String> {
        if self.texture_cache.contains_key(key) {
            return Ok(());
        }

        let image = scene_resource_service::load_scene_texture_image(&item.texture_path)
            .map_err(|error| {
                format!(
                    "unable to decode texture {}: {error}",
                    item.texture_path.display()
                )
            })?;
        let metrics =
            phase10_texture_metrics_from_size(image.width() as usize, image.height() as usize);
        let texture = load_texture(&self.device, image).map_err(|error| {
            format!(
                "unable to upload texture {}: {error}",
                item.texture_path.display()
            )
        })?;
        self.texture_resolution_cache
            .insert(key.to_string(), metrics);
        self.texture_cache.insert(key.to_string(), texture);
        Ok(())
    }

    fn ensure_text_texture_loaded(
        &mut self,
        item: &SceneRenderTextItem,
        key: &str,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        if self.text_texture_cache.contains_key(key) {
            return Ok(Vec::new());
        }

        let rasterized = rasterize_text_texture(
            item,
            |item, path| NativeSceneWarning::unsupported_text_effect(&item.object_name, path),
            NativeSceneWarning::text_font_fallback,
        )?;
        let texture = load_texture(&self.device, rasterized.image).map_err(|error| {
            format!(
                "unable to upload text texture {}: {error}",
                item.object_name
            )
        })?;
        self.text_texture_cache.insert(key.to_string(), texture);
        Ok(rasterized.warnings)
    }

    fn ensure_procedural_texture(
        &mut self,
        key: &str,
        image: DynamicImage,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        required_keys.insert(key.to_string());
        if self.texture_cache.contains_key(key) {
            return Ok(());
        }

        let texture = load_texture(&self.device, image)
            .map_err(|error| format!("unable to upload procedural texture {key}: {error}"))?;
        self.texture_resolution_cache.insert(
            key.to_string(),
            phase10_texture_metrics_from_texture(texture.as_ref()),
        );
        self.texture_cache.insert(key.to_string(), texture);
        Ok(())
    }

    fn sync_video_sources(
        &mut self,
        visuals: &[SceneRenderVisualItem],
        paused: bool,
    ) -> Vec<NativeSceneWarning> {
        let current_paths = self
            .video_sources
            .iter()
            .map(|(object_id, source)| (*object_id, source.state()))
            .collect::<BTreeMap<_, _>>();
        let desired = scene_video_texture_service::desired_video_texture_sources(visuals);
        let plan = scene_video_texture_service::plan_video_texture_source_sync(
            &current_paths,
            &desired,
            paused,
        );
        let mut warnings = Vec::new();

        for action in plan.actions {
            match action {
                SceneVideoTextureLifecycleAction::Remove { object_id, .. } => {
                    if let Some(mut source) = self.video_sources.remove(&object_id) {
                        source.stop();
                    }
                }
                SceneVideoTextureLifecycleAction::SetPaused { object_id, paused } => {
                    if let Some(source) = self.video_sources.get_mut(&object_id) {
                        source.set_paused(paused);
                    }
                }
                SceneVideoTextureLifecycleAction::Replace { source, paused, .. } => {
                    if let Some(existing) = self.video_sources.get_mut(&source.object_id) {
                        existing.stop();
                    }
                    match NativeSceneVideoSource::new(
                        source.object_id,
                        source.asset_path.clone(),
                        paused,
                    ) {
                        Ok(next_source) => {
                            self.video_sources.insert(source.object_id, next_source);
                        }
                        Err(error) => {
                            self.video_sources.remove(&source.object_id);
                            warnings.push(video_texture_source_warning(&source, error));
                        }
                    }
                }
                SceneVideoTextureLifecycleAction::Create { source, paused } => {
                    match NativeSceneVideoSource::new(
                        source.object_id,
                        source.asset_path.clone(),
                        paused,
                    ) {
                        Ok(next_source) => {
                            self.video_sources.insert(source.object_id, next_source);
                        }
                        Err(error) => warnings.push(video_texture_source_warning(&source, error)),
                    }
                }
            }
        }
        warnings
    }

    fn clear_video_sources(&mut self) {
        for (_, source) in self.video_sources.iter_mut() {
            source.stop();
        }
        self.video_sources.clear();
        self.video_texture_cache.flush(0);
    }

    fn video_texture_for_item(
        &mut self,
        item: &SceneRenderVisualItem,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let source = self.video_sources.get_mut(&item.object_id)?;
        match source.current_texture(&self.video_texture_cache, self.paused) {
            Ok(texture) => texture,
            Err(error) => {
                let detail = video_texture_frame_warning(&item.object_name, error);
                let _ = diagnostic_service::record_warning(
                    &self.app,
                    DIAGNOSTIC_SUBSYSTEM,
                    &detail.code,
                    detail.message.clone(),
                    detail.detail_json(),
                );
                None
            }
        }
    }

    fn draw_quad(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        texture: &ProtocolObject<dyn MTLTexture>,
        blend_mode: SceneRenderBlendMode,
        projection: &SceneProjection,
        quad: SceneQuadPrimitive,
    ) {
        let vertices = build_projected_quad_vertices(quad, projection);
        self.draw_vertices(encoder, texture, blend_mode, &vertices);
    }

    fn draw_vertices(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        texture: &ProtocolObject<dyn MTLTexture>,
        blend_mode: SceneRenderBlendMode,
        vertices: &[SceneVertex],
    ) {
        let byte_len = std::mem::size_of_val(vertices);
        if byte_len == 0 {
            return;
        }
        let Some(vertices_bytes) = NonNull::new(vertices.as_ptr() as *mut c_void) else {
            return;
        };
        encoder.setRenderPipelineState(self.pipeline_for(blend_mode));
        unsafe {
            match scene_vertex_upload_strategy(byte_len) {
                SceneVertexUploadStrategy::InlineBytes => {
                    encoder.setVertexBytes_length_atIndex(vertices_bytes, byte_len, 0);
                }
                SceneVertexUploadStrategy::SharedBuffer => {
                    let Some(vertex_buffer) = self.device.newBufferWithBytes_length_options(
                        vertices_bytes,
                        byte_len,
                        MTLResourceOptions::StorageModeShared,
                    ) else {
                        return;
                    };
                    encoder.setVertexBuffer_offset_atIndex(Some(vertex_buffer.as_ref()), 0, 0);
                }
            }
            encoder.setFragmentTexture_atIndex(Some(texture), 0);
            encoder.drawPrimitives_vertexStart_vertexCount(
                MTLPrimitiveType::Triangle,
                0,
                vertices.len(),
            );
        }
    }

    fn draw_audio_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        white_texture: &ProtocolObject<dyn MTLTexture>,
        projection: &SceneProjection,
        item: &SceneRenderAudioItem,
        shared_audio_snapshot: Option<&audio_input_service::AudioSnapshot>,
        now_ms: u64,
    ) {
        let sound_levels = scene_soundscape_audio_levels(&self.app, item.bar_count);
        let levels = self.audio_coordinator.levels_for_count(
            shared_audio_snapshot,
            sound_levels.as_deref(),
            item.bar_count,
            now_ms,
        );
        let origin_x = item.quad.left + item.quad.width / 2.0;
        let origin_y = item.quad.top + item.quad.height / 2.0;
        for index in 0..item.bar_count {
            let level = clamp_f64(
                levels.get(index).copied().unwrap_or_default() * item.volume_factor,
                0.0,
                1.0,
            );
            let bounded = item.normalized_lower_bound + (1.0 - item.normalized_lower_bound) * level;
            let scale_y = clamp_f64(bounded.max(item.min_scale), item.min_scale, 1.0);
            let bar_height = item.drawable_height * scale_y;
            let left = item.quad.left + index as f64 * (item.bar_width + item.gap);
            let top = item.quad.top + item.drawable_top + (item.drawable_height - bar_height);
            let quad = SceneQuadPrimitive {
                left,
                top,
                width: item.bar_width,
                height: bar_height.max(1.0),
                rotation: scene_audio_bar_rotation(item.quad.rotation),
                opacity: item.quad.opacity * clamp_f64(0.35 + scale_y * 0.65, 0.35, 1.0),
                flip_x: item.quad.flip_x,
                flip_y: item.quad.flip_y,
                uv_rect: full_quad_uv_rect(),
                color: item.color,
                transform_origin_x: origin_x,
                transform_origin_y: origin_y,
            };
            self.draw_quad(
                encoder,
                white_texture,
                SceneRenderBlendMode::Normal,
                projection,
                quad,
            );
        }
    }

    fn advance_particle_items(
        &mut self,
        items: &[SceneRenderParticleItem],
        input_response: SceneInputResponse,
        now_ms: f64,
    ) {
        let cursor = input_response
            .cursor()
            .map(|(x, y)| SceneParticleCursor { x, y });
        if !self.paused {
            self.particle_scheduler.advance(cursor, items, now_ms);
        } else {
            self.particle_scheduler.pause_cursor(cursor);
        }
    }

    fn advance_rope_particle_items(
        &mut self,
        items: &[SceneRenderRopeParticleItem],
        input_response: SceneInputResponse,
        now_ms: f64,
    ) {
        let cursor = input_response
            .cursor()
            .map(|(x, y)| SceneParticleCursor { x, y });
        if !self.paused {
            self.rope_particle_scheduler.advance(cursor, items, now_ms);
        } else {
            self.rope_particle_scheduler.pause_cursor(cursor);
        }
    }

    fn draw_particle_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        white_texture: &ProtocolObject<dyn MTLTexture>,
        petal_texture: Option<&ProtocolObject<dyn MTLTexture>>,
        projection: &SceneProjection,
        plan: &SceneRenderPlan,
        item: &SceneRenderParticleItem,
        now_ms: f64,
    ) {
        match item.particle_kind {
            crate::models::SceneParticleKind::LineTrail => {
                for segment in
                    self.particle_scheduler
                        .line_primitives(item, plan.canvas_height, now_ms)
                {
                    self.draw_quad(
                        encoder,
                        white_texture,
                        SceneRenderBlendMode::Additive,
                        projection,
                        quad_primitive_from_particle(segment),
                    );
                }
            }
            crate::models::SceneParticleKind::PetalTrail => {
                let Some(petal_texture) = petal_texture else {
                    return;
                };
                for petal in
                    self.particle_scheduler
                        .petal_primitives(item, plan.canvas_height, now_ms)
                {
                    self.draw_quad(
                        encoder,
                        petal_texture,
                        SceneRenderBlendMode::Normal,
                        projection,
                        quad_primitive_from_particle(petal),
                    );
                }
            }
        }
    }

    fn draw_rope_particle_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        white_texture: &ProtocolObject<dyn MTLTexture>,
        projection: &SceneProjection,
        canvas_height: f64,
        item: &SceneRenderRopeParticleItem,
        now_ms: f64,
    ) {
        let texture = item
            .texture_path
            .as_ref()
            .filter(|path| path.as_path() != Path::new("__rope-white__"))
            .and_then(|path| {
                let key = phase10_texture_cache_key(path);
                self.texture_cache.get(&key).cloned()
            });
        for segment in self.rope_particle_scheduler.primitives(item, now_ms) {
            self.draw_quad(
                encoder,
                texture
                    .as_ref()
                    .map(|texture| texture.as_ref())
                    .unwrap_or(white_texture),
                item.blend_mode,
                projection,
                quad_primitive_from_rope_particle(segment, canvas_height),
            );
        }
    }

    fn draw_sprite_particle_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        projection: &SceneProjection,
        plan: &SceneRenderPlan,
        item: &SceneRenderSpriteParticleItem,
        now_ms: f64,
    ) {
        for primitive in self
            .sprite_particle_scheduler
            .primitives(item, plan.canvas_height, now_ms)
        {
            let key = phase10_texture_cache_key(&primitive.texture_path);
            let Some(texture) = self.texture_cache.get(&key).cloned() else {
                continue;
            };
            self.draw_quad(
                encoder,
                texture.as_ref(),
                primitive.blend_mode,
                projection,
                quad_primitive_from_sprite_particle(primitive),
            );
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn scene_audio_bar_rotation(rotation: f64) -> f64 {
    -rotation
}

#[cfg(target_os = "macos")]
fn scene_debug_layout_enabled() -> bool {
    matches!(
        std::env::var("SCENE_DEBUG_LAYOUT").ok().as_deref(),
        Some("1" | "true" | "TRUE")
    )
}

#[cfg(target_os = "macos")]
fn visual_texture_cache_key(item: &SceneRenderVisualItem) -> String {
    let source_prefix = match item.source_kind {
        SceneRenderSourceKind::Image => "image",
        SceneRenderSourceKind::Video => "video",
    };
    format!("{source_prefix}:{}", item.texture_path.display())
}

#[cfg(target_os = "macos")]
pub(super) fn should_retain_visual_in_draw_plan(
    item: &SceneRenderVisualItem,
    phase10_consumed_ids: &BTreeSet<u32>,
    image_texture_loaded: bool,
) -> bool {
    phase10_consumed_ids.contains(&item.object_id)
        || matches!(item.source_kind, SceneRenderSourceKind::Video)
        || image_texture_loaded
}

#[cfg(all(target_os = "macos", test))]
pub(super) fn compile_scene_shader_program_pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
    blend_mode: SceneRenderBlendMode,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, String> {
    let source = load_shader_program_source(program, defines)?;
    let source = NSString::from_str(source.as_str());
    let library = device
        .newLibraryWithSource_options_error(&source, None)
        .map_err(|error| {
            format!(
                "failed to compile phase-10 Scene shader {}: {error:?}",
                program.metal_source_path.display()
            )
        })?;
    let vertex_name = NSString::from_str(program.vertex_entry);
    let fragment_name = NSString::from_str(program.fragment_entry);
    let vertex_function = library.newFunctionWithName(&vertex_name).ok_or_else(|| {
        format!(
            "phase-10 shader {} is missing vertex entry {}",
            program.metal_source_path.display(),
            program.vertex_entry
        )
    })?;
    let fragment_function = library.newFunctionWithName(&fragment_name).ok_or_else(|| {
        format!(
            "phase-10 shader {} is missing fragment entry {}",
            program.metal_source_path.display(),
            program.fragment_entry
        )
    })?;
    build_pipeline_state(
        device,
        vertex_function.as_ref(),
        fragment_function.as_ref(),
        blend_mode,
    )
}

#[cfg(target_os = "macos")]
fn phase10_texture_cache_key(path: &Path) -> String {
    format!("phase10:texture:{}", path.display())
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_shader_variant_key(
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
pub(super) fn phase10_visual_requires_offscreen_chain(visual: &ScenePhase10VisualPlan) -> bool {
    if visual.puppet_path.is_some() {
        return phase10_visual_pass_chain(visual).len() > 1;
    }
    !phase10_base_passes(visual).is_empty() || !visual.effect_chain.is_empty()
}

#[cfg(target_os = "macos")]
fn phase10_visual_first_pass_is_mask_alpha(visual: &ScenePhase10VisualPlan) -> bool {
    phase10_base_passes(visual)
        .first()
        .map(|pass| pass.program.kind == SceneShaderProgramKind::MaskAlpha)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn phase10_mask_apply_shader_program() -> SceneShaderProgram {
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
fn phase10_render_target_size(visual: &ScenePhase10VisualPlan) -> (usize, usize) {
    (
        visual.quad.width.abs().max(1.0).ceil() as usize,
        visual.quad.height.abs().max(1.0).ceil() as usize,
    )
}

#[cfg(target_os = "macos")]
fn phase10_visual_needs_background_snapshot(visual: &ScenePhase10VisualPlan) -> bool {
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
fn phase10_resolved_pass_target_name<'a>(
    resolved_pass: &'a Phase10ResolvedPass<'_>,
) -> Option<&'a str> {
    match resolved_pass.context {
        Phase10PassContext::Effect(effect_pass) => effect_pass.target_name.as_deref(),
        Phase10PassContext::Base => None,
    }
}

#[cfg(target_os = "macos")]
fn phase10_local_background_projection(
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
pub(super) fn phase10_puppet_offscreen_projection(
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
fn phase10_fullscreen_vertices(tint: SceneRenderColor) -> [SceneVertex; 6] {
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
fn phase10_solid_texture_key(color: SceneRenderColor, width: usize, height: usize) -> String {
    format!(
        "phase10:solid:{:02x}{:02x}{:02x}{:02x}:{width}x{height}",
        color.red, color.green, color.blue, color.alpha,
    )
}

#[cfg(target_os = "macos")]
fn phase10_output_texture_key(object_id: u32, width: usize, height: usize) -> String {
    format!("phase10:output:{object_id}:{width}x{height}")
}

#[cfg(target_os = "macos")]
fn phase10_scratch_texture_key(width: usize, height: usize, slot: usize) -> String {
    format!("phase10:scratch:{width}x{height}:{slot}")
}

#[cfg(target_os = "macos")]
fn phase10_named_target_texture_key(
    object_id: u32,
    target_name: &str,
    width: usize,
    height: usize,
) -> String {
    format!("phase10:named:{object_id}:{target_name}:{width}x{height}")
}

#[cfg(target_os = "macos")]
fn phase10_background_texture_key(object_id: u32, width: usize, height: usize) -> String {
    format!("phase10:background:{object_id}:{width}x{height}")
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Phase10EffectTextureSource {
    GraphInput(ScenePhase10InputSource),
    MaterialSlot(usize),
    OverrideSlot(usize),
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_effect_texture_slot_plan(
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
pub(super) fn phase10_pass_shader_defines(resolved_pass: &Phase10ResolvedPass<'_>) -> BTreeMap<String, i32> {
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
fn phase10_effect_uniform_values(
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
fn phase10_uniform_float(
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
fn phase10_uniform_vec2(
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
fn phase10_uniform_color(
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
fn phase10_uniform_vec4(
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
pub(super) fn phase10_perspective_corner_uniforms(
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
pub(super) fn phase10_skew_controls(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
) -> [f32; 4] {
    let direct_anchor = phase10_optional_uniform_vec2(
        values,
        &["anchor", "uieditorpropertiesanchor"],
    );
    let direct_skew =
        phase10_optional_uniform_vec2(values, &["skew", "uieditorpropertiesskew"]);
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
pub(super) fn phase10_spin_controls(
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
pub(super) fn phase10_transform_controls(
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
    ([offset[0], offset[1], scale[0], scale[1]], [anchor[0], anchor[1], 0.0, 0.0], angle)
}

#[cfg(target_os = "macos")]
fn rotate2d(vector: [f32; 2], angle: f32) -> [f32; 2] {
    let sine = angle.sin();
    let cosine = angle.cos();
    [
        vector[0] * cosine - vector[1] * sine,
        vector[0] * sine + vector[1] * cosine,
    ]
}

#[cfg(target_os = "macos")]
fn phase10_alpha_prefill_required(blend_mode: SceneRenderBlendMode) -> bool {
    !matches!(blend_mode, SceneRenderBlendMode::Normal)
}

#[cfg(target_os = "macos")]
fn phase10_effect_family_from_program(
    program: &SceneShaderProgram,
) -> Option<SceneCompatEffectKind> {
    match program.kind {
        SceneShaderProgramKind::EffectCompat(kind) => Some(kind),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
impl NativeSceneVideoSource {
    fn new(object_id: u32, asset_path: PathBuf, paused: bool) -> Result<Self, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Scene video source must be created on the main thread".to_string())?;
        let url = file_url_for_path(&asset_path)?;
        let item = unsafe { AVPlayerItem::playerItemWithURL(&url, mtm) };
        let output_settings = scene_video_output_settings();
        let output = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&output_settings),
            )
        };
        unsafe {
            item.addOutput(output.as_ref());
        }

        let player = unsafe { AVPlayer::playerWithPlayerItem(Some(item.as_ref()), mtm) };
        unsafe {
            player.setMuted(true);
            player.setVolume(0.0);
            player.setActionAtItemEnd(AVPlayerActionAtItemEnd::None);
            if paused {
                player.pause();
            } else {
                player.play();
            }
        }

        Ok(Self {
            object_id,
            asset_path,
            paused,
            player,
            item,
            output,
            current_cv_texture: None,
            current_texture: None,
        })
    }

    fn stop(&mut self) {
        unsafe {
            self.player.pause();
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
        self.current_texture = None;
        self.current_cv_texture = None;
    }

    fn state(&self) -> SceneVideoTextureSourceState {
        SceneVideoTextureSourceState {
            asset_path: self.asset_path.clone(),
            paused: self.paused,
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        unsafe {
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    fn current_texture(
        &mut self,
        texture_cache: &CVMetalTextureCache,
        paused: bool,
    ) -> Result<Option<Retained<ProtocolObject<dyn MTLTexture>>>, String> {
        self.set_paused(paused);
        self.ensure_looping(paused);

        match unsafe { self.item.status() } {
            AVPlayerItemStatus::Failed => {
                return Err(format!(
                    "AVPlayerItem failed for object {} at {}: {:?}",
                    self.object_id,
                    self.asset_path.display(),
                    unsafe { self.item.error() }
                ));
            }
            AVPlayerItemStatus::Unknown => {
                return Ok(self.current_texture.clone());
            }
            AVPlayerItemStatus::ReadyToPlay => {}
            _ => {}
        }

        let mut item_time = unsafe { self.output.itemTimeForHostTime(CACurrentMediaTime()) };
        if !cm_time_is_numeric(item_time) {
            item_time = unsafe { self.player.currentTime() };
        }

        let needs_frame = self.current_texture.is_none()
            || unsafe { self.output.hasNewPixelBufferForItemTime(item_time) };
        if needs_frame {
            let pixel_buffer = unsafe {
                self.output
                    .copyPixelBufferForItemTime_itemTimeForDisplay(item_time, std::ptr::null_mut())
            };
            if let Some(pixel_buffer) = pixel_buffer.as_ref() {
                self.update_current_texture(texture_cache, pixel_buffer)?;
            }
        }

        Ok(self.current_texture.clone())
    }

    fn ensure_looping(&self, paused: bool) {
        let duration_seconds = unsafe { self.item.duration().seconds() };
        let current_seconds = unsafe { self.player.currentTime().seconds() };
        if !duration_seconds.is_finite()
            || !current_seconds.is_finite()
            || duration_seconds <= 0.05
            || current_seconds < duration_seconds - 0.03
        {
            return;
        }

        let zero = unsafe { CMTime::with_seconds(0.0, 600) };
        unsafe {
            self.player.seekToTime(zero);
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    fn update_current_texture(
        &mut self,
        texture_cache: &CVMetalTextureCache,
        pixel_buffer: &CVPixelBuffer,
    ) -> Result<(), String> {
        let width = CVPixelBufferGetWidth(pixel_buffer);
        let height = CVPixelBufferGetHeight(pixel_buffer);
        if width == 0 || height == 0 {
            return Err(format!(
                "Scene video source {} produced an empty pixel buffer",
                self.asset_path.display()
            ));
        }

        let mut cv_texture_ptr = std::ptr::null_mut();
        let status = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                texture_cache,
                pixel_buffer,
                None,
                MTLPixelFormat::BGRA8Unorm,
                width,
                height,
                0,
                NonNull::from(&mut cv_texture_ptr),
            )
        };
        if status != kCVReturnSuccess {
            return Err(format!(
                "CVMetalTextureCacheCreateTextureFromImage failed for {} with status {status}",
                self.asset_path.display()
            ));
        }
        let cv_texture_ptr = NonNull::new(cv_texture_ptr).ok_or_else(|| {
            format!(
                "CVMetalTextureCacheCreateTextureFromImage returned null for {}",
                self.asset_path.display()
            )
        })?;
        let cv_texture = unsafe { CFRetained::from_raw(cv_texture_ptr) };
        let texture = CVMetalTextureGetTexture(cv_texture.as_ref()).ok_or_else(|| {
            format!(
                "CVMetalTextureGetTexture returned null for {}",
                self.asset_path.display()
            )
        })?;
        self.current_cv_texture = Some(cv_texture);
        self.current_texture = Some(texture);
        Ok(())
    }
}
