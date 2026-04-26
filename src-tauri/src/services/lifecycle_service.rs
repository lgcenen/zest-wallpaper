use std::{sync::Mutex, thread, time::Duration};

use anyhow::anyhow;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem},
    tray::TrayIconBuilder,
    ActivationPolicy, App, AppHandle, Emitter, Manager, RunEvent, UserAttentionType, Window,
    WindowEvent,
};

use crate::{
    services::{
        audio_input_service, auto_pause_service, diagnostic_service, input_service,
        native_video_service, native_web_service, player_service, scene_native_renderer_service,
        static_snapshot_service, window_service,
    },
    store::{AppState, DynamicPlayerState},
};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "wallpaper-tray";
const TRAY_SHOW_ID: &str = "show";
const TRAY_PAUSE_ID: &str = "pause";
const TRAY_QUIT_ID: &str = "quit";
const PLAYER_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(1);
const RESTORE_DIAGNOSTIC_SUBSYSTEM: &str = "player-restore";
const RESTORE_FAILED_CODE: &str = "restore-failed";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LifecycleSnapshot {
    workbench_visible: bool,
    player_visible: bool,
    quit_requested: bool,
}

#[derive(Debug, Default)]
pub struct AppLifecycleState {
    snapshot: Mutex<LifecycleSnapshot>,
}

struct TrayMenuState {
    pause_item: CheckMenuItem<tauri::Wry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SetupRestoreOutcome {
    Restored,
    ClearedFailed { error: String },
}

impl AppLifecycleState {
    fn set_workbench_visible(&self, visible: bool) {
        self.mutate(|snapshot| {
            snapshot.workbench_visible = visible;
        });
    }

    fn set_player_visible(&self, visible: bool) {
        self.mutate(|snapshot| {
            snapshot.player_visible = visible;
        });
    }

    fn set_quit_requested(&self, requested: bool) {
        self.mutate(|snapshot| {
            snapshot.quit_requested = requested;
        });
    }

    fn is_quit_requested(&self) -> bool {
        self.snapshot()
            .map(|snapshot| snapshot.quit_requested)
            .unwrap_or_default()
    }

    fn workbench_is_hidden(&self) -> bool {
        self.snapshot()
            .map(|snapshot| !snapshot.workbench_visible)
            .unwrap_or(false)
    }

    fn snapshot(&self) -> Option<LifecycleSnapshot> {
        self.snapshot.lock().ok().map(|snapshot| snapshot.clone())
    }

    fn mutate<F>(&self, update: F)
    where
        F: FnOnce(&mut LifecycleSnapshot),
    {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            update(&mut snapshot);
        }
    }
}

pub fn configure_app_on_setup(app: &mut App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(ActivationPolicy::Regular);

    let _ = app.manage(AppLifecycleState::default());
    let _ = app.manage(audio_input_service::SharedAudioServiceState::default());
    let _ = app.manage(diagnostic_service::DiagnosticServiceState::default());
    let _ = app.manage(input_service::SharedInputServiceState::default());
    let _ = app.manage(scene_native_renderer_service::NativeSceneRendererServiceState::default());
    let _ = app.manage(native_video_service::NativeVideoServiceState::default());
    let _ = app.manage(native_web_service::NativeWebServiceState::default());
    build_tray(app)?;
    show_workbench(&app.handle())?;

    let state = app.state::<AppState>();
    restore_player_session_on_setup(&app.handle(), &state)
        .map_err(|error| tauri::Error::Anyhow(anyhow!(error)))?;
    input_service::start_input_worker(app.handle().clone());
    audio_input_service::start_audio_worker(app.handle().clone());
    native_web_service::start_bridge_retry_worker(app.handle().clone());
    start_player_lifecycle_worker(app.handle().clone());

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    }

    Ok(())
}

fn restore_player_session_on_setup(app: &AppHandle, state: &AppState) -> Result<(), String> {
    match resolve_setup_restore_outcome(
        || player_service::restore_player_session(app, state),
        || recover_failed_restore(app, state),
    )? {
        SetupRestoreOutcome::Restored => {
            let _ = diagnostic_service::clear_diagnostic(
                app,
                RESTORE_DIAGNOSTIC_SUBSYSTEM,
                RESTORE_FAILED_CODE,
            );
        }
        SetupRestoreOutcome::ClearedFailed { error } => {
            let _ = diagnostic_service::record_error(
                app,
                RESTORE_DIAGNOSTIC_SUBSYSTEM,
                RESTORE_FAILED_CODE,
                "Persisted active wallpaper failed to restore and was cleared.",
                Some(error),
            );
        }
    }

    Ok(())
}

fn resolve_setup_restore_outcome<Restore, Recover>(
    restore: Restore,
    recover: Recover,
) -> Result<SetupRestoreOutcome, String>
where
    Restore: FnOnce() -> Result<(), String>,
    Recover: FnOnce() -> Result<(), String>,
{
    match restore() {
        Ok(()) => Ok(SetupRestoreOutcome::Restored),
        Err(error) => {
            recover().map_err(|recover_error| {
                format!(
                    "failed to recover from persisted wallpaper restore error: {recover_error}; original restore error: {error}"
                )
            })?;
            Ok(SetupRestoreOutcome::ClearedFailed { error })
        }
    }
}

fn recover_failed_restore(app: &AppHandle, state: &AppState) -> Result<(), String> {
    player_service::clear_player_session_state(state)?;

    let _ = static_snapshot_service::clear_active_snapshot_sync(app, state);
    let _ = player_service::sync_native_runtime_for_active_wallpaper(app, state);
    let _ = close_player_windows(app);
    sync_pause_menu_state(app, false);
    let _ = app.emit(
        "player:load",
        Option::<crate::models::WallpaperRuntimeRecord>::None,
    );
    let _ = app.emit("player:pause", false);
    Ok(())
}

pub fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        TRAY_SHOW_ID => {
            let _ = show_workbench(app);
        }
        TRAY_PAUSE_ID => {
            toggle_pause_from_tray(app);
        }
        TRAY_QUIT_ID => {
            record_quit_requested(app, true);
            app.exit(0);
        }
        _ => {}
    }
}

pub fn handle_run_event(app: &AppHandle, event: &RunEvent) {
    match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if workbench_is_hidden(app) {
                let _ = show_workbench(app);
            }
        }
        RunEvent::ExitRequested { .. } => {
            record_quit_requested(app, true);
        }
        _ => {}
    }
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        if quit_requested(window.app_handle()) {
            return;
        }

        api.prevent_close();
        let _ = hide_workbench(window.app_handle());
    }
}

pub fn show_player_windows(app: &AppHandle) -> tauri::Result<()> {
    let visible_count = window_service::show_player_windows(app)?;
    let _ = audio_input_service::prune_scene_audio_interest(app);
    record_player_visible(app, visible_count > 0);
    Ok(())
}

pub fn close_player_windows(app: &AppHandle) -> tauri::Result<()> {
    window_service::close_player_windows(app)?;
    let _ = audio_input_service::prune_scene_audio_interest(app);
    record_player_visible(app, false);
    Ok(())
}

pub fn sync_pause_menu_state(app: &AppHandle, paused: bool) {
    if let Some(menu_state) = app.try_state::<TrayMenuState>() {
        let _ = menu_state.pause_item.set_checked(paused);
    }
}

fn build_tray(app: &App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, TRAY_SHOW_ID, "Open Workbench", true, None::<&str>)?;
    let pause_item = CheckMenuItem::with_id(
        app,
        TRAY_PAUSE_ID,
        "Pause Player",
        true,
        false,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &pause_item, &quit_item])?;

    let _ = app.manage(TrayMenuState {
        pause_item: pause_item.clone(),
    });

    TrayIconBuilder::with_id(TRAY_ID).menu(&menu).build(app)?;

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(player) = state.player.lock() {
            sync_pause_menu_state(&app.handle(), player.manually_paused);
        }
    }

    Ok(())
}

fn show_workbench(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(ActivationPolicy::Regular);

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window_service::configure_workbench_window(&window);
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }

    record_quit_requested(app, false);
    record_workbench_visible(app, true);
    Ok(())
}

fn hide_workbench(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.hide();
    }

    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);

    record_workbench_visible(app, false);
    Ok(())
}

fn toggle_pause_from_tray(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    let paused = match state.player.lock() {
        Ok(player) => !player.manually_paused,
        Err(_) => return,
    };

    let _ = player_service::set_player_paused(paused, app, &state);
}

fn start_player_lifecycle_worker(app: AppHandle) {
    thread::spawn(move || {
        let app_bundle_id = app.config().identifier.clone();
        let mut previous_reconcile_signature: Option<PlayerLifecycleReconcileSignature> = None;

        loop {
            thread::sleep(PLAYER_MAINTENANCE_INTERVAL);

            let state = app.state::<AppState>();
            let player_snapshot = match state.player.lock() {
                Ok(player) => PlayerLifecycleSnapshot::from_player(&player),
                Err(_) => continue,
            };

            if player_snapshot.active_id.is_none() {
                let _ = player_service::sync_native_runtime_for_active_wallpaper(&app, &state);
                if !window_service::player_window_labels(&app).is_empty() {
                    let _ = close_player_windows(&app);
                }
                let _ = player_service::set_player_auto_pause_screen_labels(
                    Default::default(),
                    &app,
                    &state,
                );
                previous_reconcile_signature = None;
                continue;
            }

            let expected_labels = match window_service::expected_player_window_labels(&app) {
                Ok(labels) => labels,
                Err(_) => continue,
            };
            let current_labels = window_service::player_window_labels(&app);
            let plan_signature = match window_service::player_window_plan_signature(&app) {
                Ok(signature) => signature,
                Err(_) => continue,
            };
            let reconcile_signature =
                player_lifecycle_reconcile_signature(plan_signature, &player_snapshot);

            let needs_reconcile = previous_reconcile_signature.as_ref()
                != Some(&reconcile_signature)
                || current_labels != expected_labels;
            if needs_reconcile {
                let _ = show_player_windows(&app);
                let _ = player_service::sync_native_runtime_for_active_wallpaper(&app, &state);
            } else {
                record_player_visible(&app, !expected_labels.is_empty());
            }

            previous_reconcile_signature = Some(reconcile_signature);

            let auto_pause_labels =
                auto_pause_service::sample_auto_pause_screen_labels(&app, &app_bundle_id)
                    .unwrap_or_default();
            let _ = player_service::set_player_auto_pause_screen_labels(
                auto_pause_labels,
                &app,
                &state,
            );
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerLifecycleSnapshot {
    active_id: Option<String>,
    scene_update_generation: u64,
}

impl PlayerLifecycleSnapshot {
    fn from_player(player: &DynamicPlayerState) -> Self {
        Self {
            active_id: player.active_id.clone(),
            scene_update_generation: player.scene_update_generation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerLifecycleReconcileSignature {
    plan_signature: String,
    active_id: Option<String>,
    scene_update_generation: u64,
}

fn player_lifecycle_reconcile_signature(
    plan_signature: String,
    player: &PlayerLifecycleSnapshot,
) -> PlayerLifecycleReconcileSignature {
    PlayerLifecycleReconcileSignature {
        plan_signature,
        active_id: player.active_id.clone(),
        scene_update_generation: player.scene_update_generation,
    }
}

fn record_workbench_visible(app: &AppHandle, visible: bool) {
    if let Some(state) = app.try_state::<AppLifecycleState>() {
        state.set_workbench_visible(visible);
    }
}

fn record_player_visible(app: &AppHandle, visible: bool) {
    if let Some(state) = app.try_state::<AppLifecycleState>() {
        state.set_player_visible(visible);
    }
}

fn record_quit_requested(app: &AppHandle, requested: bool) {
    if let Some(state) = app.try_state::<AppLifecycleState>() {
        state.set_quit_requested(requested);
    }
}

fn workbench_is_hidden(app: &AppHandle) -> bool {
    app.try_state::<AppLifecycleState>()
        .map(|state| state.workbench_is_hidden())
        .unwrap_or(false)
}

fn quit_requested(app: &AppHandle) -> bool {
    app.try_state::<AppLifecycleState>()
        .map(|state| state.is_quit_requested())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        player_lifecycle_reconcile_signature, resolve_setup_restore_outcome, AppLifecycleState,
        PlayerLifecycleSnapshot, SetupRestoreOutcome,
    };

    #[test]
    fn main_window_close_hides_workbench_without_touching_player_visibility() {
        let state = AppLifecycleState::default();
        state.set_workbench_visible(true);
        state.set_player_visible(true);

        state.set_workbench_visible(false);

        let snapshot = state.snapshot().unwrap();
        assert!(!snapshot.workbench_visible);
        assert!(snapshot.player_visible);
        assert!(!snapshot.quit_requested);
    }

    #[test]
    fn reopening_workbench_preserves_player_visibility() {
        let state = AppLifecycleState::default();
        state.set_workbench_visible(false);
        state.set_player_visible(true);

        state.set_workbench_visible(true);

        let snapshot = state.snapshot().unwrap();
        assert!(snapshot.workbench_visible);
        assert!(snapshot.player_visible);
        assert!(!snapshot.quit_requested);
    }

    #[test]
    fn quit_request_isolated_from_visibility_flags() {
        let state = AppLifecycleState::default();
        state.set_workbench_visible(false);
        state.set_player_visible(true);

        state.set_quit_requested(true);

        let snapshot = state.snapshot().unwrap();
        assert!(snapshot.quit_requested);
        assert!(snapshot.player_visible);
        assert!(!snapshot.workbench_visible);
    }

    #[test]
    fn close_and_reopen_cycle_does_not_look_like_quit() {
        let state = AppLifecycleState::default();
        state.set_workbench_visible(true);
        state.set_player_visible(true);

        state.set_workbench_visible(false);
        state.set_workbench_visible(true);

        let snapshot = state.snapshot().unwrap();
        assert!(snapshot.workbench_visible);
        assert!(snapshot.player_visible);
        assert!(!snapshot.quit_requested);
    }

    #[test]
    fn lifecycle_reconcile_signature_tracks_active_runtime_identity() {
        let old = player_lifecycle_reconcile_signature(
            "display:player:0:0:1920:1080".to_string(),
            &PlayerLifecycleSnapshot {
                active_id: Some("old-wallpaper".to_string()),
                scene_update_generation: 4,
            },
        );
        let new_same_windows = player_lifecycle_reconcile_signature(
            "display:player:0:0:1920:1080".to_string(),
            &PlayerLifecycleSnapshot {
                active_id: Some("new-wallpaper".to_string()),
                scene_update_generation: 5,
            },
        );

        assert_ne!(old, new_same_windows);
    }

    #[test]
    fn restore_failure_clears_bad_wallpaper_without_fatal_setup_error() {
        let mut recovered = false;

        let outcome = resolve_setup_restore_outcome(
            || {
                Err(
                    "native web runtime failed: native web runtime entry HTML file is missing"
                        .to_string(),
                )
            },
            || {
                recovered = true;
                Ok(())
            },
        )
        .expect("non fatal restore outcome");

        assert!(recovered);
        assert_eq!(
            outcome,
            SetupRestoreOutcome::ClearedFailed {
                error: "native web runtime failed: native web runtime entry HTML file is missing"
                    .to_string()
            }
        );
    }

    #[test]
    fn restore_failure_only_bubbles_when_recovery_cannot_converge_state() {
        let error = resolve_setup_restore_outcome(
            || Err("native video runtime failed: source file is missing".to_string()),
            || Err("failed to persist cleared player state".to_string()),
        )
        .expect_err("fatal only when recovery fails");

        assert!(error.contains("failed to persist cleared player state"));
        assert!(error.contains("native video runtime failed: source file is missing"));
    }
}
