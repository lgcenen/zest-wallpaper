use super::*;
use super::scene_effect_runtime_service::{
    phase10_effect_texture_slot_plan, phase10_render_target_size, phase10_solid_texture_key,
    phase10_texture_cache_key, Phase10EffectTextureSource, Phase10PassContext,
    Phase10ResolvedPass,
};
use super::scene_effect_target_runtime_service::{
    phase10_texture_metrics_from_size, phase10_texture_metrics_from_texture, Phase10TextureHandle,
};
use super::scene_metal_renderer::NativeSceneVideoSource;
#[cfg(target_os = "macos")]
use crate::services::scene_resource_service;
#[cfg(target_os = "macos")]
use crate::services::scene_text_raster_service::rasterize_text_texture;

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(super) struct Phase10PassTextures {
    pub(super) slots: Vec<Option<Phase10TextureHandle>>,
}

#[cfg(target_os = "macos")]
pub(super) struct Phase10PassInputScope<'a> {
    pub(super) local_current: Option<&'a Phase10TextureHandle>,
    pub(super) previous_pass: Option<&'a Phase10TextureHandle>,
    pub(super) background: Option<&'a Phase10TextureHandle>,
    pub(super) copied_background: Option<&'a Phase10TextureHandle>,
    pub(super) named_targets: &'a BTreeMap<String, Phase10TextureHandle>,
}

#[cfg(target_os = "macos")]
impl Phase10PassInputScope<'_> {
    pub(super) fn texture_for(
        &self,
        source: &ScenePhase10InputSource,
    ) -> Option<Phase10TextureHandle> {
        match source {
            ScenePhase10InputSource::LocalCurrentVisual => self.local_current.cloned(),
            ScenePhase10InputSource::PreviousPass => self.previous_pass.cloned(),
            ScenePhase10InputSource::Background => self.background.cloned(),
            ScenePhase10InputSource::CopiedBackground => self.copied_background.cloned(),
            ScenePhase10InputSource::NamedTarget(target_name) => {
                self.named_targets.get(target_name).cloned()
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_texel_size(texture: Option<&Phase10TextureHandle>) -> [f32; 2] {
    let Some(texture) = texture else {
        return [1.0, 1.0];
    };
    texture.metrics.texel_size()
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_optional_texel_size(texture: Option<&Phase10TextureHandle>) -> [f32; 2] {
    let Some(texture) = texture else {
        return [0.0, 0.0];
    };
    texture.metrics.texel_size()
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_texture_resolution(texture: Option<&Phase10TextureHandle>) -> [f32; 4] {
    let Some(texture) = texture else {
        return [1.0, 1.0, 1.0, 1.0];
    };
    texture.metrics.resolution()
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_optional_texture_resolution(
    texture: Option<&Phase10TextureHandle>,
) -> [f32; 4] {
    let Some(texture) = texture else {
        return [1.0, 1.0, 0.0, 0.0];
    };
    texture.metrics.resolution()
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_base_texture_for_visual(
    visual: &ScenePhase10VisualPlan,
    texture_cache: &BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &BTreeMap<String, Phase10TextureMetrics>,
    video_sources: &mut BTreeMap<u32, NativeSceneVideoSource>,
    video_texture_cache: &CVMetalTextureCache,
    paused: bool,
) -> Result<Option<Phase10TextureHandle>, String> {
    Ok(match visual.base_source_kind {
        Some(SceneRenderSourceKind::Image) => visual
            .base_texture_path
            .as_deref()
            .and_then(|path| phase10_texture_for_path(path, texture_cache, texture_resolution_cache)),
        Some(SceneRenderSourceKind::Video) => {
            let source = match video_sources.get_mut(&visual.object_id) {
                Some(source) => source,
                None => return Ok(None),
            };
            match source.current_texture(video_texture_cache, paused)? {
                Some(texture) => Some(Phase10TextureHandle {
                    metrics: phase10_texture_metrics_from_texture(texture.as_ref()),
                    texture,
                }),
                None => None,
            }
        }
        None => {
            let (width, height) = phase10_render_target_size(visual);
            texture_cache
                .get(&phase10_solid_texture_key(visual.base_color, width, height))
                .cloned()
                .map(|texture| Phase10TextureHandle {
                    texture,
                    metrics: *texture_resolution_cache
                        .get(&phase10_solid_texture_key(visual.base_color, width, height))
                        .unwrap_or(&phase10_texture_metrics_from_size(width, height)),
                })
        }
    })
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_visual_texture_loaded(
    device: &ProtocolObject<dyn MTLDevice>,
    item: &SceneRenderVisualItem,
    key: &str,
    texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &mut BTreeMap<String, Phase10TextureMetrics>,
) -> Result<(), String> {
    if texture_cache.contains_key(key) {
        return Ok(());
    }

    let image = scene_resource_service::load_scene_texture_image(&item.texture_path).map_err(
        |error| format!("unable to decode texture {}: {error}", item.texture_path.display()),
    )?;
    let metrics = phase10_texture_metrics_from_size(image.width() as usize, image.height() as usize);
    let texture = load_texture(device, image)
        .map_err(|error| format!("unable to upload texture {}: {error}", item.texture_path.display()))?;
    texture_resolution_cache.insert(key.to_string(), metrics);
    texture_cache.insert(key.to_string(), texture);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_text_texture_loaded(
    device: &ProtocolObject<dyn MTLDevice>,
    item: &SceneRenderTextItem,
    key: &str,
    text_texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
) -> Result<Vec<NativeSceneWarning>, String> {
    if text_texture_cache.contains_key(key) {
        return Ok(Vec::new());
    }

    let rasterized = rasterize_text_texture(
        item,
        |item, path| NativeSceneWarning::unsupported_text_effect(&item.object_name, path),
        NativeSceneWarning::text_font_fallback,
    )?;
    let texture = load_texture(device, rasterized.image)
        .map_err(|error| format!("unable to upload text texture {}: {error}", item.object_name))?;
    text_texture_cache.insert(key.to_string(), texture);
    Ok(rasterized.warnings)
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_procedural_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    key: &str,
    image: DynamicImage,
    required_keys: &mut BTreeSet<String>,
    texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &mut BTreeMap<String, Phase10TextureMetrics>,
) -> Result<(), String> {
    required_keys.insert(key.to_string());
    if texture_cache.contains_key(key) {
        return Ok(());
    }

    let texture = load_texture(device, image)
        .map_err(|error| format!("unable to upload procedural texture {key}: {error}"))?;
    texture_resolution_cache.insert(
        key.to_string(),
        phase10_texture_metrics_from_texture(texture.as_ref()),
    );
    texture_cache.insert(key.to_string(), texture);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_phase10_base_texture_loaded(
    device: &ProtocolObject<dyn MTLDevice>,
    visual: &ScenePhase10VisualPlan,
    required_keys: &mut BTreeSet<String>,
    texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &mut BTreeMap<String, Phase10TextureMetrics>,
    video_sources: &BTreeMap<u32, NativeSceneVideoSource>,
) -> Result<bool, String> {
    match visual.base_source_kind {
        Some(SceneRenderSourceKind::Image) => {
            let Some(base_texture_path) = visual.base_texture_path.as_ref() else {
                return Ok(false);
            };
            phase10_ensure_texture_loaded(
                device,
                base_texture_path,
                required_keys,
                texture_cache,
                texture_resolution_cache,
            )?;
            Ok(true)
        }
        Some(SceneRenderSourceKind::Video) => Ok(video_sources.contains_key(&visual.object_id)),
        None => {
            if visual.base_color.alpha == 0 {
                return Ok(false);
            }
            let (width, height) = phase10_render_target_size(visual);
            ensure_phase10_solid_texture(
                device,
                visual.base_color,
                width,
                height,
                required_keys,
                texture_cache,
                texture_resolution_cache,
            )?;
            Ok(true)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_phase10_pass_textures_loaded(
    device: &ProtocolObject<dyn MTLDevice>,
    resolved_pass: &Phase10ResolvedPass<'_>,
    required_keys: &mut BTreeSet<String>,
    texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &mut BTreeMap<String, Phase10TextureMetrics>,
) -> Result<bool, String> {
    let mut pass_ready = false;
    for texture_path in resolved_pass
        .pass
        .textures
        .iter()
        .filter_map(|binding| binding.resolved_path.as_ref())
    {
        phase10_ensure_texture_loaded(
            device,
            texture_path,
            required_keys,
            texture_cache,
            texture_resolution_cache,
        )?;
        pass_ready = true;
    }
    if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
        for texture_path in effect_pass.texture_overrides.iter().flatten() {
            phase10_ensure_texture_loaded(
                device,
                texture_path,
                required_keys,
                texture_cache,
                texture_resolution_cache,
            )?;
            pass_ready = true;
        }
    }
    Ok(pass_ready)
}

#[cfg(target_os = "macos")]
fn ensure_phase10_solid_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    color: SceneRenderColor,
    width: usize,
    height: usize,
    required_keys: &mut BTreeSet<String>,
    texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &mut BTreeMap<String, Phase10TextureMetrics>,
) -> Result<(), String> {
    let key = phase10_solid_texture_key(color, width, height);
    required_keys.insert(key.clone());
    if texture_cache.contains_key(&key) {
        return Ok(());
    }

    let texture = load_texture(device, build_solid_texture_image(color, width, height))
        .map_err(|error| format!("unable to upload phase-10 solid texture {key}: {error}"))?;
    texture_resolution_cache.insert(
        key.clone(),
        phase10_texture_metrics_from_size(width, height),
    );
    texture_cache.insert(key, texture);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_ensure_texture_loaded(
    device: &ProtocolObject<dyn MTLDevice>,
    path: &Path,
    required_keys: &mut BTreeSet<String>,
    texture_cache: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &mut BTreeMap<String, Phase10TextureMetrics>,
) -> Result<(), String> {
    let key = phase10_texture_cache_key(path);
    required_keys.insert(key.clone());
    if texture_cache.contains_key(&key) {
        return Ok(());
    }

    let decoded = load_phase10_texture_source(path)?;
    let texture = load_texture(device, decoded.image)
        .map_err(|error| format!("unable to upload phase-10 texture {}: {error}", path.display()))?;
    texture_resolution_cache.insert(key.clone(), decoded.metrics);
    texture_cache.insert(key, texture);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_texture_for_path(
    path: &Path,
    texture_cache: &BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &BTreeMap<String, Phase10TextureMetrics>,
) -> Option<Phase10TextureHandle> {
    let key = phase10_texture_cache_key(path);
    let texture = texture_cache.get(&key)?.clone();
    let metrics = texture_resolution_cache
        .get(&key)
        .copied()
        .unwrap_or_else(|| phase10_texture_metrics_from_texture(texture.as_ref()));
    Some(Phase10TextureHandle { texture, metrics })
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_pass_textures_for(
    visual: &ScenePhase10VisualPlan,
    resolved_pass: &Phase10ResolvedPass<'_>,
    input_scope: &Phase10PassInputScope<'_>,
    texture_cache: &BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: &BTreeMap<String, Phase10TextureMetrics>,
) -> Phase10PassTextures {
    let mut slots = BTreeMap::<usize, Phase10TextureHandle>::new();

    match resolved_pass.context {
        Phase10PassContext::Base => {
            for binding in &resolved_pass.pass.textures {
                let Some(texture) = binding
                    .resolved_path
                    .as_deref()
                    .and_then(|path| phase10_texture_for_path(path, texture_cache, texture_resolution_cache))
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
                        .and_then(|path| {
                            phase10_texture_for_path(path, texture_cache, texture_resolution_cache)
                        }),
                    Phase10EffectTextureSource::OverrideSlot(override_slot) => effect_pass
                        .texture_overrides
                        .get(override_slot)
                        .and_then(|path| path.as_deref())
                        .and_then(|path| {
                            phase10_texture_for_path(path, texture_cache, texture_resolution_cache)
                        }),
                };
                if let Some(texture) = texture {
                    slots.insert(slot, texture);
                }
            }
        }
    }

    if slots.is_empty() && visual.base_source_kind.is_none() && visual.base_color.alpha > 0 {
        let (width, height) = phase10_render_target_size(visual);
        if let Some(texture) = texture_cache
            .get(&phase10_solid_texture_key(visual.base_color, width, height))
            .cloned()
        {
            slots.insert(
                0,
                Phase10TextureHandle {
                    metrics: *texture_resolution_cache
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
