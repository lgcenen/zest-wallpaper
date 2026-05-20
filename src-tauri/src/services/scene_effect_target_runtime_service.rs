use super::*;

#[cfg(target_os = "macos")]
pub(super) struct Phase10RenderTargetStores {
    output_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    scratch_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    named_target_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    background_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
}

#[cfg(target_os = "macos")]
impl Default for Phase10RenderTargetStores {
    fn default() -> Self {
        Self {
            output_textures: BTreeMap::new(),
            scratch_textures: BTreeMap::new(),
            named_target_textures: BTreeMap::new(),
            background_textures: BTreeMap::new(),
        }
    }
}

#[cfg(target_os = "macos")]
impl Phase10RenderTargetStores {
    pub(super) fn clear(&mut self) {
        self.output_textures.clear();
        self.scratch_textures.clear();
        self.named_target_textures.clear();
        self.background_textures.clear();
    }

    pub(super) fn retain_required(
        &mut self,
        required_output_keys: &BTreeSet<String>,
        required_scratch_keys: &BTreeSet<String>,
        required_named_target_keys: &BTreeSet<String>,
        required_background_keys: &BTreeSet<String>,
    ) {
        self.output_textures
            .retain(|key, _| required_output_keys.contains(key));
        self.scratch_textures
            .retain(|key, _| required_scratch_keys.contains(key));
        self.named_target_textures
            .retain(|key, _| required_named_target_keys.contains(key));
        self.background_textures
            .retain(|key, _| required_background_keys.contains(key));
    }

    pub(super) fn ensure_output_target(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            device,
            &mut self.output_textures,
            key,
            width,
            height,
        )
    }

    pub(super) fn ensure_scratch_target(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            device,
            &mut self.scratch_textures,
            key,
            width,
            height,
        )
    }

    pub(super) fn ensure_named_target(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            device,
            &mut self.named_target_textures,
            key,
            width,
            height,
        )
    }

    pub(super) fn ensure_background_target(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            device,
            &mut self.background_textures,
            key,
            width,
            height,
        )
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(super) struct Phase10TextureHandle {
    pub(super) texture: Retained<ProtocolObject<dyn MTLTexture>>,
    pub(super) metrics: Phase10TextureMetrics,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Phase10TextureMetrics {
    pub(super) texture_size: [f32; 2],
    pub(super) content_size: [f32; 2],
}

#[cfg(target_os = "macos")]
impl Default for Phase10TextureMetrics {
    fn default() -> Self {
        Self {
            texture_size: [1.0, 1.0],
            content_size: [1.0, 1.0],
        }
    }
}

#[cfg(target_os = "macos")]
impl Phase10TextureMetrics {
    pub(super) fn resolution(self) -> [f32; 4] {
        [
            self.texture_size[0].max(1.0),
            self.texture_size[1].max(1.0),
            self.content_size[0].max(1.0),
            self.content_size[1].max(1.0),
        ]
    }

    pub(super) fn texel_size(self) -> [f32; 2] {
        [
            1.0 / self.texture_size[0].max(1.0),
            1.0 / self.texture_size[1].max(1.0),
        ]
    }
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_texture_metrics_from_size(
    width: usize,
    height: usize,
) -> Phase10TextureMetrics {
    Phase10TextureMetrics {
        texture_size: [width.max(1) as f32, height.max(1) as f32],
        content_size: [width.max(1) as f32, height.max(1) as f32],
    }
}

#[cfg(target_os = "macos")]
pub(super) fn phase10_texture_metrics_from_texture(
    texture: &ProtocolObject<dyn MTLTexture>,
) -> Phase10TextureMetrics {
    phase10_texture_metrics_from_size(texture.width(), texture.height())
}

#[cfg(target_os = "macos")]
fn ensure_phase10_render_target_in_store(
    device: &ProtocolObject<dyn MTLDevice>,
    store: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    key: &str,
    width: usize,
    height: usize,
) -> Option<Phase10TextureHandle> {
    if let Some(texture) = store.get(key) {
        return Some(Phase10TextureHandle {
            texture: texture.clone(),
            metrics: phase10_texture_metrics_from_size(width, height),
        });
    }
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm,
            width.max(1),
            height.max(1),
            false,
        )
    };
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
    descriptor.setStorageMode(MTLStorageMode::Private);
    let texture = device.newTextureWithDescriptor(&descriptor)?;
    store.insert(key.to_string(), texture.clone());
    Some(Phase10TextureHandle {
        texture,
        metrics: phase10_texture_metrics_from_size(width, height),
    })
}
