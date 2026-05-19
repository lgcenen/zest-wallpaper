use std::{
    sync::RwLock,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::services::{native_web_service, player_host_service};

#[cfg(target_os = "macos")]
use dispatch2::run_on_main;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSScreen};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(34);

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputModifierSnapshot {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedInputSnapshot {
    pub global_x: f64,
    pub global_y: f64,
    pub system_x: f64,
    pub system_y: f64,
    pub desktop_width: f64,
    pub desktop_height: f64,
    pub timestamp_ms: u64,
    pub active: bool,
    pub modifiers: InputModifierSnapshot,
    pub scroll_delta_x: f64,
    pub scroll_delta_y: f64,
}

#[derive(Default)]
pub struct SharedInputServiceState {
    snapshot: RwLock<SharedInputSnapshot>,
    initialized: RwLock<bool>,
}

impl SharedInputServiceState {
    fn replace_snapshot(&self, snapshot: SharedInputSnapshot) -> Result<(), String> {
        let mut stored = self.snapshot.write().map_err(|error| error.to_string())?;
        *stored = snapshot;
        let mut initialized = self
            .initialized
            .write()
            .map_err(|error| error.to_string())?;
        *initialized = true;
        Ok(())
    }

    fn snapshot(&self) -> Result<SharedInputSnapshot, String> {
        self.snapshot
            .read()
            .map(|snapshot| snapshot.clone())
            .map_err(|error| error.to_string())
    }

    fn initialized(&self) -> Result<bool, String> {
        self.initialized
            .read()
            .map(|initialized| *initialized)
            .map_err(|error| error.to_string())
    }
}

pub fn start_input_worker(app: AppHandle) {
    thread::spawn(move || loop {
        let _ = sample_and_dispatch(&app);
        thread::sleep(INPUT_POLL_INTERVAL);
    });
}

pub fn current_input_snapshot(app: &AppHandle) -> Result<SharedInputSnapshot, String> {
    app.try_state::<SharedInputServiceState>()
        .map(|state| state.snapshot())
        .unwrap_or_else(|| Ok(SharedInputSnapshot::default()))
}

pub fn input_snapshot_initialized(app: &AppHandle) -> bool {
    app.try_state::<SharedInputServiceState>()
        .and_then(|state| state.initialized().ok())
        .unwrap_or(false)
}

fn sample_and_dispatch(app: &AppHandle) -> Result<(), String> {
    let snapshot = snapshot_for_current_frame(sample_input_snapshot(), timestamp_ms());
    if let Some(state) = app.try_state::<SharedInputServiceState>() {
        state.replace_snapshot(snapshot.clone())?;
    }

    for label in player_host_service::live_player_host_labels(app) {
        let _ = app.emit_to(label, "player:input", snapshot.clone());
    }
    native_web_service::dispatch_shared_input(app, &snapshot)?;
    Ok(())
}

fn snapshot_for_current_frame(
    sampled_snapshot: Result<SharedInputSnapshot, String>,
    frame_timestamp_ms: u64,
) -> SharedInputSnapshot {
    sampled_snapshot.unwrap_or_else(|_| inactive_input_snapshot(frame_timestamp_ms))
}

fn sample_input_snapshot() -> Result<SharedInputSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        return run_on_main(|mtm| {
            let screens = NSScreen::screens(mtm);
            if screens.is_empty() {
                return Err("No active screens are available".to_string());
            }

            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;

            for screen in screens.iter() {
                let frame = screen.frame();
                min_x = min_x.min(frame.origin.x);
                min_y = min_y.min(frame.origin.y);
                max_x = max_x.max(frame.origin.x + frame.size.width);
                max_y = max_y.max(frame.origin.y + frame.size.height);
            }

            let position = NSEvent::mouseLocation();
            let modifiers = NSEvent::modifierFlags_class();

            Ok(SharedInputSnapshot {
                global_x: position.x,
                global_y: max_y - position.y,
                system_x: position.x,
                system_y: position.y,
                desktop_width: (max_x - min_x).max(0.0),
                desktop_height: (max_y - min_y).max(0.0),
                timestamp_ms: timestamp_ms(),
                active: position.x >= min_x
                    && position.x <= max_x
                    && position.y >= min_y
                    && position.y <= max_y,
                modifiers: InputModifierSnapshot {
                    shift: modifiers.contains(NSEventModifierFlags::Shift),
                    control: modifiers.contains(NSEventModifierFlags::Control),
                    option: modifiers.contains(NSEventModifierFlags::Option),
                    command: modifiers.contains(NSEventModifierFlags::Command),
                },
                scroll_delta_x: 0.0,
                scroll_delta_y: 0.0,
            })
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("Shared input sampling is only implemented on macOS".to_string())
    }
}

fn inactive_input_snapshot(frame_timestamp_ms: u64) -> SharedInputSnapshot {
    SharedInputSnapshot {
        timestamp_ms: frame_timestamp_ms,
        ..SharedInputSnapshot::default()
    }
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        inactive_input_snapshot, snapshot_for_current_frame, InputModifierSnapshot,
        SharedInputServiceState, SharedInputSnapshot,
    };

    fn sample_snapshot() -> SharedInputSnapshot {
        SharedInputSnapshot {
            global_x: 128.0,
            global_y: 256.0,
            system_x: 128.0,
            system_y: 1440.0,
            desktop_width: 5120.0,
            desktop_height: 2880.0,
            timestamp_ms: 42,
            active: true,
            modifiers: InputModifierSnapshot {
                shift: true,
                control: false,
                option: true,
                command: false,
            },
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
        }
    }

    #[test]
    fn shared_input_snapshot_state_round_trips_latest_frame() {
        let state = SharedInputServiceState::default();
        let snapshot = sample_snapshot();

        assert!(!state.initialized().expect("read initial state"));

        state
            .replace_snapshot(snapshot.clone())
            .expect("store snapshot");

        assert!(state.initialized().expect("read initialized state"));
        assert_eq!(state.snapshot().expect("read snapshot"), snapshot);
    }

    #[test]
    fn failed_sampling_returns_inactive_fallback_frame() {
        let snapshot = snapshot_for_current_frame(Err("sample failed".to_string()), 99);

        assert_eq!(snapshot, inactive_input_snapshot(99));
        assert_eq!(snapshot.timestamp_ms, 99);
        assert!(!snapshot.active);
        assert_eq!(snapshot.global_x, 0.0);
        assert_eq!(snapshot.global_y, 0.0);
        assert_eq!(snapshot.system_x, 0.0);
        assert_eq!(snapshot.system_y, 0.0);
        assert_eq!(snapshot.desktop_width, 0.0);
        assert_eq!(snapshot.desktop_height, 0.0);
        assert_eq!(snapshot.scroll_delta_x, 0.0);
        assert_eq!(snapshot.scroll_delta_y, 0.0);
        assert_eq!(snapshot.modifiers, InputModifierSnapshot::default());
    }

    #[test]
    fn failed_sampling_overwrites_previous_active_snapshot_with_inactive_fallback() {
        let state = SharedInputServiceState::default();
        state
            .replace_snapshot(sample_snapshot())
            .expect("store active snapshot");

        let fallback = snapshot_for_current_frame(Err("sample failed".to_string()), 1337);
        state
            .replace_snapshot(fallback.clone())
            .expect("store fallback snapshot");

        let stored = state.snapshot().expect("read fallback snapshot");
        assert_eq!(stored, fallback);
        assert!(!stored.active);
        assert_eq!(stored.timestamp_ms, 1337);
        assert_eq!(stored.global_x, 0.0);
        assert_eq!(stored.global_y, 0.0);
        assert_eq!(stored.system_x, 0.0);
        assert_eq!(stored.system_y, 0.0);
    }
}
