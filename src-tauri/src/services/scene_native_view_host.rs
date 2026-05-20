use super::*;
use crate::services::player_host_service;

#[cfg(target_os = "macos")]
use std::cell::RefCell;

#[cfg(target_os = "macos")]
use objc2::{define_class, msg_send, runtime::NSObject, DefinedClass, MainThreadOnly};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use objc2_metal::MTLCreateSystemDefaultDevice;
#[cfg(target_os = "macos")]
use objc2_metal_kit::MTKViewDelegate;

pub(crate) struct NativeSceneViewHandle {
    #[cfg(target_os = "macos")]
    host: MainThreadBound<NativeSceneViewHost>,
}

impl NativeSceneViewHandle {
    pub(crate) fn create(app: &AppHandle, clear_color: SceneClearColor) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let host = run_on_main(move |mtm| {
                NativeSceneViewHost::create(app, clear_color, mtm)
                    .map(|host| MainThreadBound::new(host, mtm))
            })?;
            return Ok(Self { host });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, clear_color);
            Ok(Self {})
        }
    }

    pub(crate) fn sync(
        &self,
        app: &AppHandle,
        label: &str,
        spec: &SceneRendererSpec,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let label = label.to_string();
            let spec = spec.clone();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                player_host_service::with_player_host_container_view(
                    &app,
                    &label,
                    mtm,
                    |container| host.sync(container, &spec),
                )
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, label, spec);
            Ok(Vec::new())
        }
    }

    pub(crate) fn update_dynamic_text(
        &self,
        app: &AppHandle,
        label: &str,
        texts: &[SceneRenderTextItem],
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let label = label.to_string();
            let texts = texts.to_vec();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                player_host_service::with_player_host_container_view(
                    &app,
                    &label,
                    mtm,
                    |container| host.update_dynamic_text(container, texts, paused),
                )
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, label, texts, paused);
            Ok(Vec::new())
        }
    }

    pub(crate) fn teardown(&self) {
        #[cfg(target_os = "macos")]
        run_on_main(|mtm| {
            let host = self.host.get(mtm);
            host.detach();
        });
    }
}

#[cfg(target_os = "macos")]
struct NativeSceneViewHost {
    view: Retained<MTKView>,
    delegate: Retained<NativeSceneRenderDelegate>,
}

#[cfg(target_os = "macos")]
struct NativeSceneRenderDelegateIvars {
    renderer: RefCell<NativeSceneMetalRenderer>,
}

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = NativeSceneRenderDelegateIvars]
    struct NativeSceneRenderDelegate;

    unsafe impl NSObjectProtocol for NativeSceneRenderDelegate {}

    unsafe impl MTKViewDelegate for NativeSceneRenderDelegate {
        #[unsafe(method(drawInMTKView:))]
        #[allow(non_snake_case)]
        fn drawInMTKView(&self, view: &MTKView) {
            self.ivars().renderer.borrow_mut().draw(view);
        }

        #[unsafe(method(mtkView:drawableSizeWillChange:))]
        #[allow(non_snake_case)]
        fn mtkView_drawableSizeWillChange(&self, _view: &MTKView, _size: NSSize) {}
    }
);

#[cfg(target_os = "macos")]
impl NativeSceneRenderDelegate {
    fn new(
        app: AppHandle,
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<Self>, String> {
        let renderer = NativeSceneMetalRenderer::new(app, device, command_queue)?;
        let delegate =
            mtm.alloc::<NativeSceneRenderDelegate>()
                .set_ivars(NativeSceneRenderDelegateIvars {
                    renderer: RefCell::new(renderer),
                });
        Ok(unsafe { msg_send![super(delegate), init] })
    }

    fn apply_scene(
        &self,
        scene_key: &str,
        plan: SceneRenderPlan,
        phase10_graph: ScenePhase10GraphPlan,
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        self.ivars()
            .renderer
            .borrow_mut()
            .apply_scene(scene_key, plan, phase10_graph, paused)
    }

    fn apply_dynamic_text_update(
        &self,
        texts: Vec<SceneRenderTextItem>,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        self.ivars()
            .renderer
            .borrow_mut()
            .apply_dynamic_text_update(texts)
    }

    fn clear_scene(&self) {
        self.ivars().renderer.borrow_mut().clear_scene();
    }

    fn metal_device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        self.ivars().renderer.borrow().metal_device()
    }
}

#[cfg(target_os = "macos")]
impl NativeSceneViewHost {
    fn create(
        app: AppHandle,
        clear_color: SceneClearColor,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let device = create_scene_metal_device()?;
        let command_queue = create_scene_command_queue(device.as_ref())?;
        let view = create_scene_mtk_view(clear_color, device.as_ref(), mtm);
        let delegate = NativeSceneRenderDelegate::new(app, device, command_queue, mtm)?;
        view.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        Ok(Self { view, delegate })
    }

    fn sync(
        &self,
        container: &NSView,
        spec: &SceneRendererSpec,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let warnings = self.delegate.apply_scene(
            &spec.wallpaper_id,
            spec.render_plan.clone(),
            spec.phase10_graph.clone(),
            spec.paused,
        )?;
        let device = self.delegate.metal_device();
        let needs_attach = !self.view.isDescendantOf(container);

        self.view
            .setClearColor(spec.render_plan.clear_color.as_metal_clear_color());
        self.view.setDevice(Some(device.as_ref()));
        self.view.setFrame(container.bounds());
        if needs_attach {
            self.view.removeFromSuperview();
            container.addSubview(&self.view);
        }
        self.view.setPaused(spec.paused);
        if spec.paused || needs_attach {
            self.view.draw();
        }

        Ok(warnings)
    }

    fn update_dynamic_text(
        &self,
        container: &NSView,
        texts: Vec<SceneRenderTextItem>,
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let warnings = self.delegate.apply_dynamic_text_update(texts)?;
        if !self.view.isDescendantOf(container) {
            return Err(
                "native Scene view is not attached to the target player window".to_string(),
            );
        }
        if paused {
            self.view.draw();
        }
        Ok(warnings)
    }

    fn detach(&self) {
        self.view.setPaused(true);
        self.delegate.clear_scene();
        self.view.removeFromSuperview();
    }
}

#[cfg(target_os = "macos")]
fn create_scene_metal_device() -> Result<Retained<ProtocolObject<dyn MTLDevice>>, String> {
    MTLCreateSystemDefaultDevice()
        .ok_or_else(|| "Metal device is unavailable for native Scene runtime".to_string())
}

#[cfg(target_os = "macos")]
fn create_scene_command_queue(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLCommandQueue>>, String> {
    device
        .newCommandQueue()
        .ok_or_else(|| "Metal command queue could not be created".to_string())
}

#[cfg(target_os = "macos")]
fn create_scene_mtk_view(
    clear_color: SceneClearColor,
    device: &ProtocolObject<dyn MTLDevice>,
    mtm: MainThreadMarker,
) -> Retained<MTKView> {
    let view = MTKView::initWithFrame_device(
        MTKView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
        Some(device),
    );
    view.setClearColor(clear_color.as_metal_clear_color());
    view.setColorPixelFormat(MTLPixelFormat::BGRA8Unorm);
    view.setFramebufferOnly(true);
    view.setAutoResizeDrawable(true);
    view.setEnableSetNeedsDisplay(false);
    view.setPreferredFramesPerSecond(60);
    view.setPaused(true);
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    view
}
