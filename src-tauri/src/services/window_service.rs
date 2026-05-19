use tauri::{AppHandle, Monitor, Runtime};

#[cfg(target_os = "macos")]
use objc2::{msg_send, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSColor, NSWindow, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{ns_string, NSNumber, NSObjectNSKeyValueCoding};
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

const PRIMARY_PLAYER_LABEL: &str = "player";
const SECONDARY_PLAYER_PREFIX: &str = "player-screen-";

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayTopology {
    name: Option<String>,
    position: (i32, i32),
    size: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerHostPlan {
    pub label: String,
    pub position: (i32, i32),
    pub size: (u32, u32),
}

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

pub fn expected_player_window_labels<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<String>> {
    Ok(current_player_host_plan(app)?
        .into_iter()
        .map(|entry| entry.label)
        .collect())
}

pub fn expected_player_window_label_set<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<std::collections::BTreeSet<String>> {
    Ok(expected_player_window_labels(app)?.into_iter().collect())
}

pub fn player_window_plan_signature<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<String> {
    Ok(plan_signature(&current_player_host_plan(app)?))
}

pub fn current_player_host_plan<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<Vec<PlayerHostPlan>> {
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

fn display_topology(monitor: &Monitor) -> DisplayTopology {
    DisplayTopology {
        name: monitor.name().cloned(),
        position: (monitor.position().x, monitor.position().y),
        size: (monitor.size().width, monitor.size().height),
    }
}

fn plan_player_windows(displays: &[DisplayTopology]) -> Vec<PlayerHostPlan> {
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
        .map(|(index, display)| PlayerHostPlan {
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

fn plan_signature(plan: &[PlayerHostPlan]) -> String {
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
