use std::{
    collections::HashSet,
    sync::{Mutex, MutexGuard},
};

use anyhow::anyhow;
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindowBuilder,
};

#[cfg(target_os = "macos")]
use objc2::{msg_send, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSColor, NSWindow, NSWindowCollectionBehavior, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
#[cfg(target_os = "macos")]
use objc2_core_graphics::kCGDesktopWindowLevel;
#[cfg(target_os = "macos")]
use objc2_foundation::{ns_string, NSNumber, NSObjectNSKeyValueCoding};
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

const PRIMARY_PLAYER_LABEL: &str = "player";
const SECONDARY_PLAYER_PREFIX: &str = "player-screen-";

static PLAYER_WINDOW_TRANSACTION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayTopology {
    name: Option<String>,
    position: (i32, i32),
    size: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerWindowPlan {
    label: String,
    position: (i32, i32),
    size: (u32, u32),
}

#[cfg(target_os = "macos")]
fn configure_player_window<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.with_webview(|webview| unsafe {
        let _marker = MainThreadMarker::new()
            .expect("player window configuration must run on the main thread");
        let ns_window: &NSWindow = &*webview.ns_window().cast();
        ns_window.setLevel(kCGDesktopWindowLevel as _);
        ns_window.setHasShadow(false);
        ns_window.setOpaque(false);
        let clear = NSColor::clearColor();
        ns_window.setBackgroundColor(Some(&clear));
        ns_window.setIgnoresMouseEvents(true);
        ns_window.setMovable(false);
        ns_window.setHidesOnDeactivate(false);
        ns_window.setCanBecomeVisibleWithoutLogin(true);
        ns_window.setReleasedWhenClosed(false);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        ns_window.orderBack(None);
    });
}

#[cfg(not(target_os = "macos"))]
fn configure_player_window<R: Runtime>(_window: &tauri::WebviewWindow<R>) {}

#[cfg(target_os = "macos")]
pub fn configure_workbench_window<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.with_webview(|webview| unsafe {
        let _marker = MainThreadMarker::new()
            .expect("workbench window configuration must run on the main thread");
        let ns_window: &NSWindow = &*webview.ns_window().cast();
        let webview_view: &WKWebView = &*webview.inner().cast();
        let clear = NSColor::clearColor();
        let no = NSNumber::numberWithBool(false);

        ns_window.setOpaque(false);
        ns_window.setHasShadow(false);
        ns_window.setBackgroundColor(Some(&clear));
        ns_window.setTitlebarAppearsTransparent(true);
        ns_window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        ns_window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
        ns_window.setMovable(true);
        ns_window.setMovableByWindowBackground(true);
        ns_window.invalidateShadow();

        webview_view.setValue_forKey(Some(&no), ns_string!("drawsBackground"));
        let _: () = msg_send![webview_view, setOpaque: false];
        webview_view.setUnderPageBackgroundColor(Some(&clear));
    });
}

#[cfg(not(target_os = "macos"))]
pub fn configure_workbench_window<R: Runtime>(_window: &tauri::WebviewWindow<R>) {}

fn ensure_player_windows_locked<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<String>> {
    let plan = current_player_window_plan(app)?;
    let expected_labels = plan
        .iter()
        .map(|entry| entry.label.clone())
        .collect::<HashSet<_>>();

    for entry in &plan {
        create_or_update_player_window(app, entry)?;
    }

    for label in player_window_labels(app) {
        if expected_labels.contains(&label) {
            continue;
        }
        destroy_player_window_for_label(app, &label).map_err(tauri_anyhow)?;
    }

    Ok(plan.into_iter().map(|entry| entry.label).collect())
}

pub fn show_player_windows<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<usize> {
    let _transaction = lock_player_window_transaction()?;
    let labels = ensure_player_windows_locked(app)?;
    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.show();
        }
    }
    Ok(labels.len())
}

#[cfg(target_os = "macos")]
pub fn set_player_windows_snapshot_background_color<R: Runtime>(
    app: &AppHandle<R>,
    red: f64,
    green: f64,
    blue: f64,
) -> Result<usize, String> {
    let labels = player_window_labels(app);
    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            window
                .with_webview(move |webview| unsafe {
                    let _marker = MainThreadMarker::new()
                        .expect("player window tint must run on the main thread");
                    let ns_window: &NSWindow = &*webview.ns_window().cast();
                    let color = NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 1.0);
                    ns_window.setOpaque(true);
                    ns_window.setBackgroundColor(Some(&color));
                    ns_window.orderBack(None);
                })
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(labels.len())
}

#[cfg(not(target_os = "macos"))]
pub fn set_player_windows_snapshot_background_color<R: Runtime>(
    _app: &AppHandle<R>,
    _red: f64,
    _green: f64,
    _blue: f64,
) -> Result<usize, String> {
    Ok(0)
}

pub fn close_player_windows<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let _transaction = lock_player_window_transaction()?;
    for label in player_window_labels(app) {
        destroy_player_window_for_label(app, &label).map_err(tauri_anyhow)?;
    }
    Ok(())
}

pub fn player_window_labels<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let mut labels = app
        .webview_windows()
        .keys()
        .filter(|label| is_player_window_label(label))
        .cloned()
        .collect::<Vec<_>>();
    labels.sort();
    labels
}

pub fn player_window_label_set<R: Runtime>(app: &AppHandle<R>) -> std::collections::BTreeSet<String> {
    player_window_labels(app).into_iter().collect()
}

pub fn expected_player_window_labels<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<String>> {
    Ok(current_player_window_plan(app)?
        .into_iter()
        .map(|entry| entry.label)
        .collect())
}

pub fn expected_player_window_label_set<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<std::collections::BTreeSet<String>> {
    Ok(expected_player_window_labels(app)?.into_iter().collect())
}

pub fn player_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<tauri::WebviewWindow<R>, String> {
    app.get_webview_window(label)
        .ok_or_else(|| format!("player window {label} was not found"))
}

pub fn player_window_plan_signature<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<String> {
    Ok(plan_signature(&current_player_window_plan(app)?))
}

fn current_player_window_plan<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<Vec<PlayerWindowPlan>> {
    let mut displays = app
        .available_monitors()?
        .into_iter()
        .map(|monitor| display_topology(&monitor))
        .collect::<Vec<_>>();

    if displays.is_empty() {
        if let Some(primary) = app.primary_monitor()? {
            displays.push(display_topology(&primary));
        }
    }

    Ok(plan_player_windows(&displays))
}

fn create_or_update_player_window<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PlayerWindowPlan,
) -> tauri::Result<()> {
    let window = match app.get_webview_window(&plan.label) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, &plan.label, WebviewUrl::App("index.html".into()))
            .title("Wallpaper Player")
            .decorations(false)
            .shadow(false)
            .skip_taskbar(true)
            .always_on_bottom(true)
            .resizable(false)
            .visible(false)
            .initialization_script("window.__WALLPAPER_PLAYER__ = true;")
            .build()?,
    };

    let _ = window.set_position(PhysicalPosition::new(plan.position.0, plan.position.1));
    let _ = window.set_size(PhysicalSize::new(plan.size.0, plan.size.1));
    configure_player_window(&window);
    let _ = window.set_ignore_cursor_events(true);
    let _ = window.set_visible_on_all_workspaces(true);
    Ok(())
}

fn destroy_player_window_for_label<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(label) {
        remove_player_window_from_desktop(&window);
        window.destroy().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_player_window_from_desktop<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.hide();
    let _ = window.set_visible_on_all_workspaces(false);
    let _ = window.with_webview(|webview| unsafe {
        let _marker =
            MainThreadMarker::new().expect("player window removal must run on the main thread");
        let ns_window: &NSWindow = &*webview.ns_window().cast();
        ns_window.orderOut(None);
    });
}

#[cfg(not(target_os = "macos"))]
fn remove_player_window_from_desktop<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.hide();
}

fn lock_player_window_transaction() -> tauri::Result<MutexGuard<'static, ()>> {
    PLAYER_WINDOW_TRANSACTION_LOCK.lock().map_err(|error| {
        tauri_anyhow(format!(
            "player window transaction lock is poisoned: {error}"
        ))
    })
}

fn tauri_anyhow(error: String) -> tauri::Error {
    tauri::Error::Anyhow(anyhow!(error))
}

fn display_topology(monitor: &Monitor) -> DisplayTopology {
    DisplayTopology {
        name: monitor.name().cloned(),
        position: (monitor.position().x, monitor.position().y),
        size: (monitor.size().width, monitor.size().height),
    }
}

fn plan_player_windows(displays: &[DisplayTopology]) -> Vec<PlayerWindowPlan> {
    let mut sorted = displays.to_vec();
    sorted.sort_by(|left, right| {
        left.position
            .1
            .cmp(&right.position.1)
            .then_with(|| left.position.0.cmp(&right.position.0))
            .then_with(|| left.name.cmp(&right.name))
    });

    sorted
        .into_iter()
        .enumerate()
        .map(|(index, display)| PlayerWindowPlan {
            label: player_window_label(index),
            position: display.position,
            size: display.size,
        })
        .collect()
}

fn player_window_label(index: usize) -> String {
    match index {
        0 => PRIMARY_PLAYER_LABEL.to_string(),
        value => format!("{SECONDARY_PLAYER_PREFIX}{value}"),
    }
}

fn plan_signature(plan: &[PlayerWindowPlan]) -> String {
    plan.iter()
        .map(|entry| {
            format!(
                "{}:{}:{}:{}:{}",
                entry.label, entry.position.0, entry.position.1, entry.size.0, entry.size.1
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn is_player_window_label(label: &str) -> bool {
    label == PRIMARY_PLAYER_LABEL || label.starts_with(SECONDARY_PLAYER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::{plan_player_windows, plan_signature, DisplayTopology};

    #[test]
    fn multi_screen_plan_uses_stable_labels_in_topology_order() {
        let plan = plan_player_windows(&[
            DisplayTopology {
                name: Some("Right".to_string()),
                position: (2560, 0),
                size: (2560, 1440),
            },
            DisplayTopology {
                name: Some("Left".to_string()),
                position: (0, 0),
                size: (2560, 1440),
            },
            DisplayTopology {
                name: Some("Top".to_string()),
                position: (0, -900),
                size: (1600, 900),
            },
        ]);

        assert_eq!(plan[0].label, "player");
        assert_eq!(plan[0].position, (0, -900));
        assert_eq!(plan[1].label, "player-screen-1");
        assert_eq!(plan[1].position, (0, 0));
        assert_eq!(plan[2].label, "player-screen-2");
        assert_eq!(plan[2].position, (2560, 0));
    }

    #[test]
    fn plan_signature_changes_when_screen_layout_changes() {
        let before = plan_signature(&plan_player_windows(&[DisplayTopology {
            name: Some("Main".to_string()),
            position: (0, 0),
            size: (1920, 1080),
        }]));
        let after = plan_signature(&plan_player_windows(&[DisplayTopology {
            name: Some("Main".to_string()),
            position: (0, 0),
            size: (2560, 1440),
        }]));

        assert_ne!(before, after);
    }
}
