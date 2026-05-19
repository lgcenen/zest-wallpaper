use std::{
    collections::BTreeMap,
    sync::Mutex,
};

use tauri::{AppHandle, Manager, Runtime};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use objc2::{msg_send, rc::Retained, MainThreadMarker, MainThreadOnly};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
#[cfg(target_os = "macos")]
use objc2_core_graphics::kCGDesktopWindowLevel;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::window_service::{self, PlayerHostPlan};

pub struct PlayerHostServiceState {
    runtime: Mutex<PlayerHostRuntime>,
}

impl Default for PlayerHostServiceState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(PlayerHostRuntime::default()),
        }
    }
}

#[derive(Default)]
struct PlayerHostRuntime {
    #[cfg(target_os = "macos")]
    hosts: BTreeMap<String, MainThreadBound<NativePlayerHostWindow>>,
}

#[cfg(target_os = "macos")]
struct NativePlayerHostWindow {
    window: Retained<NSWindow>,
    container: Retained<NSView>,
}

pub fn show_player_hosts<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<usize> {
    #[cfg(target_os = "macos")]
    {
        let plan = window_service::current_player_host_plan(app)?;
        let app = app.clone();
        return run_on_main(move |mtm| {
            let state = app.state::<PlayerHostServiceState>();
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            runtime.sync_hosts(&plan, mtm)?;
            runtime.show_all(&plan, mtm);
            Ok(plan.len())
        })
        .map_err(tauri_anyhow);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(0)
    }
}

pub fn close_player_hosts<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        return run_on_main(move |mtm| {
            let state = app.state::<PlayerHostServiceState>();
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            runtime.close_all(mtm);
            Ok(())
        })
        .map_err(tauri_anyhow);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

pub fn live_player_host_labels<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let Some(state) = app.try_state::<PlayerHostServiceState>() else {
        return Vec::new();
    };
    let Ok(runtime) = state.runtime.lock() else {
        return Vec::new();
    };
    runtime.labels()
}

pub fn live_player_host_label_set<R: Runtime>(
    app: &AppHandle<R>,
) -> std::collections::BTreeSet<String> {
    live_player_host_labels(app).into_iter().collect()
}

pub fn expected_player_host_labels<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<String>> {
    window_service::expected_player_window_labels(app)
}

pub fn expected_player_host_label_set<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<std::collections::BTreeSet<String>> {
    window_service::expected_player_window_label_set(app)
}

pub fn player_host_plan_signature<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<String> {
    window_service::player_window_plan_signature(app)
}

#[cfg(target_os = "macos")]
pub fn with_player_host_container_view<T, F>(
    app: &AppHandle,
    label: &str,
    mtm: MainThreadMarker,
    access: F,
) -> Result<T, String>
where
    F: FnOnce(&NSView) -> Result<T, String>,
{
    let state = app
        .try_state::<PlayerHostServiceState>()
        .ok_or_else(|| "player host service state is unavailable".to_string())?;
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    let host = runtime
        .hosts
        .get(label)
        .ok_or_else(|| format!("player host {label} was not found"))?;
    access(&host.get(mtm).container)
}

pub fn set_player_host_snapshot_background_color<R: Runtime>(
    app: &AppHandle<R>,
    red: f64,
    green: f64,
    blue: f64,
) -> Result<usize, String> {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        return run_on_main(move |mtm| {
            let state = app
                .try_state::<PlayerHostServiceState>()
                .ok_or_else(|| "player host service state is unavailable".to_string())?;
            let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            let labels = runtime.labels();
            for label in &labels {
                if let Some(host) = runtime.hosts.get(label) {
                    host.get(mtm)
                        .set_snapshot_background_color(red, green, blue);
                }
            }
            Ok(labels.len())
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, red, green, blue);
        Ok(0)
    }
}

impl PlayerHostRuntime {
    fn labels(&self) -> Vec<String> {
        #[cfg(target_os = "macos")]
        {
            self.hosts.keys().cloned().collect()
        }

        #[cfg(not(target_os = "macos"))]
        {
            Vec::new()
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_hosts(&mut self, plan: &[PlayerHostPlan], mtm: MainThreadMarker) -> Result<(), String> {
        let expected = plan
            .iter()
            .map(|entry| entry.label.clone())
            .collect::<std::collections::BTreeSet<_>>();

        for entry in plan {
            let host = self
                .hosts
                .entry(entry.label.clone())
                .or_insert_with(|| MainThreadBound::new(NativePlayerHostWindow::create(mtm), mtm));
            host.get(mtm).update_layout(entry);
        }

        let stale = self
            .hosts
            .keys()
            .filter(|label| !expected.contains(*label))
            .cloned()
            .collect::<Vec<_>>();
        for label in stale {
            if let Some(host) = self.hosts.remove(&label) {
                host.get(mtm).close();
            }
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn show_all(&self, plan: &[PlayerHostPlan], mtm: MainThreadMarker) {
        for entry in plan {
            if let Some(host) = self.hosts.get(&entry.label) {
                host.get(mtm).show();
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn close_all(&mut self, mtm: MainThreadMarker) {
        for (_, host) in std::mem::take(&mut self.hosts) {
            host.get(mtm).close();
        }
    }
}

#[cfg(target_os = "macos")]
impl NativePlayerHostWindow {
    fn create(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
        let container = NSView::initWithFrame(NSView::alloc(mtm), frame);
        container.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setLevel(kCGDesktopWindowLevel as _);
            window.setHasShadow(false);
            window.setOpaque(false);
            window.setBackgroundColor(Some(&NSColor::clearColor()));
            window.setIgnoresMouseEvents(true);
            window.setMovable(false);
            window.setHidesOnDeactivate(false);
            window.setCanBecomeVisibleWithoutLogin(true);
            window.setReleasedWhenClosed(false);
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
        }
        window.setContentView(Some(&container));
        window.orderBack(None);

        Self { window, container }
    }

    fn update_layout(&self, plan: &PlayerHostPlan) {
        let frame = NSRect::new(
            NSPoint::new(plan.position.0 as f64, plan.position.1 as f64),
            NSSize::new(plan.size.0 as f64, plan.size.1 as f64),
        );
        unsafe {
            let _: () = msg_send![&*self.window, setFrame: frame, display: false];
            self.container.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(frame.size.width, frame.size.height),
            ));
        }
        self.reset_dynamic_background();
    }

    fn reset_dynamic_background(&self) {
        self.window.setOpaque(false);
        self.window.setBackgroundColor(Some(&NSColor::clearColor()));
    }

    fn show(&self) {
        self.reset_dynamic_background();
        self.window.orderBack(None);
    }

    fn close(&self) {
        self.window.orderOut(None);
        self.window.setContentView(None);
        self.container.removeFromSuperview();
    }

    fn set_snapshot_background_color(&self, red: f64, green: f64, blue: f64) {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 1.0);
        self.window.setOpaque(true);
        self.window.setBackgroundColor(Some(&color));
        self.window.orderBack(None);
    }
}

fn tauri_anyhow(error: String) -> tauri::Error {
    tauri::Error::Anyhow(anyhow::anyhow!(error))
}
