use std::{collections::BTreeSet, thread, time::Duration};

#[cfg(test)]
use std::path::Path;

use chrono::{Local, Timelike};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    models::{
        EvaluatedSceneCamera, EvaluatedSceneObject, PlayerRuntimeState, SceneParallax,
        SceneRuntimeDocument, SceneTextBehavior, SceneTextLayer, WallpaperRecord, WallpaperRuntime,
        WallpaperRuntimeRecord, WallpaperType,
    },
    services::{
        audio_input_service, lifecycle_service, native_video_service, native_web_service,
        scene_manifest_service, scene_native_renderer_service, scene_support_service,
        static_snapshot_generation_service, static_snapshot_service, window_service,
    },
    store::{find_record, save_library, save_player_state, AppState, DynamicPlayerState},
};

use super::runtime_document_service;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PauseTransition {
    effective_paused: bool,
    effective_changed: bool,
    manual_changed: bool,
    manual_paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeHostKind {
    Scene,
    Video,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeHostSyncDisposition {
    Critical,
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SceneUpdateCadence {
    CustomMillis(u64),
    Minute,
    TwoSeconds,
    Second,
}

const DESKTOP_WINDOW_REBUILD_SETTLE_DELAY: Duration = Duration::from_millis(180);

#[derive(Debug, Clone)]
struct ApplyWallpaperCandidate {
    player_state: DynamicPlayerState,
    effective_paused: bool,
}

#[derive(Serialize)]
struct SceneEvaluationSignature<'a> {
    canvas_width: f64,
    canvas_height: f64,
    clear_color: &'a Option<String>,
    camera: &'a EvaluatedSceneCamera,
    parallax: &'a SceneParallax,
    objects: &'a std::collections::BTreeMap<u32, EvaluatedSceneObject>,
    render_list: &'a [u32],
}

pub fn get_player_state_snapshot(state: &AppState) -> Result<PlayerRuntimeState, String> {
    let player = state.player.lock().map_err(|error| error.to_string())?;
    runtime_document_service::player_runtime_state(&player, state)
}

pub fn apply_dynamic_wallpaper(
    id: &str,
    app: &AppHandle,
    state: &AppState,
) -> Result<WallpaperRuntimeRecord, String> {
    let record = ensure_apply_record_current_by_id(state, id)?;
    let runtime_record = runtime_document_service::runtime_record(&record);
    preflight_scene_apply(app, &record, &runtime_record)?;
    let previous_player = state
        .player
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let previous_runtime = active_runtime_snapshot(state)?;
    let had_player_windows = !window_service::player_window_labels(app).is_empty();
    let has_static_snapshot = static_snapshot_service::snapshot_for_record(&record).is_ok();

    let effective_paused = apply_runtime_record_transaction(
        &runtime_record,
        &previous_player,
        previous_runtime,
        state,
        had_player_windows,
        || {
            prepare_static_snapshot_sync_before_runtime(
                app,
                state,
                &record,
                has_static_snapshot,
                had_player_windows,
            )
        },
        || lifecycle_service::show_player_windows(app).map_err(|error| error.to_string()),
        |runtime_record, paused| sync_native_runtime(app, runtime_record, paused),
        || lifecycle_service::close_player_windows(app).map_err(|error| error.to_string()),
        |player| {
            persist_player_state(player);
            Ok(())
        },
    )?;

    lifecycle_service::sync_pause_menu_state(app, false);
    if !has_static_snapshot {
        let _ = static_snapshot_service::sync_after_active_wallpaper_change(app, state, &record);
    }
    app.emit("player:load", Some(runtime_record.clone()))
        .map_err(|error| error.to_string())?;
    app.emit("player:pause", effective_paused)
        .map_err(|error| error.to_string())?;
    if should_start_scene_update_loop(&runtime_record) {
        start_scene_update_loop(app.clone(), state);
    }
    Ok(runtime_record)
}

fn prepare_static_snapshot_sync_before_runtime(
    app: &AppHandle,
    state: &AppState,
    record: &WallpaperRecord,
    has_static_snapshot: bool,
    had_player_windows: bool,
) -> Result<(), String> {
    if !has_static_snapshot {
        return Ok(());
    }

    if had_player_windows {
        let _ = lifecycle_service::close_player_windows(app);
        thread::sleep(DESKTOP_WINDOW_REBUILD_SETTLE_DELAY);
    }
    let _ = static_snapshot_service::sync_after_active_wallpaper_change(app, state, record);
    Ok(())
}

pub fn restore_player_session(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let Some((active_id, effective_paused)) = active_player_selection(state)? else {
        return Ok(());
    };
    let record = ensure_apply_record_current_by_id(state, &active_id)?;
    let runtime_record = runtime_document_service::runtime_record(&record);
    preflight_scene_apply(app, &record, &runtime_record)?;

    let _ = static_snapshot_service::sync_after_active_wallpaper_change(app, state, &record);
    lifecycle_service::show_player_windows(app).map_err(|error| error.to_string())?;
    sync_native_runtime_with_transaction_lock(app, state, Some(&runtime_record), effective_paused)?;
    if should_start_scene_update_loop(&runtime_record) {
        start_scene_update_loop(app.clone(), state);
    }
    Ok(())
}

fn ensure_apply_record_current_by_id(
    state: &AppState,
    id: &str,
) -> Result<WallpaperRecord, String> {
    ensure_apply_record_current_by_id_with(
        state,
        id,
        static_snapshot_generation_service::ensure_static_snapshot_for_record,
    )
}

fn ensure_apply_record_current_by_id_with<EnsureStaticSnapshot>(
    state: &AppState,
    id: &str,
    mut ensure_static_snapshot: EnsureStaticSnapshot,
) -> Result<WallpaperRecord, String>
where
    EnsureStaticSnapshot:
        FnMut(
            &mut WallpaperRecord,
        ) -> static_snapshot_generation_service::StaticSnapshotGenerationOutcome,
{
    let (mut snapshot_candidate, needs_snapshot_generation) = {
        let mut store = state.library.lock().map_err(|error| error.to_string())?;
        let record = store
            .wallpapers
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| format!("Wallpaper {id} was not found"))?;

        let scene_changed = scene_manifest_service::refresh_scene_manifest_for_record(record)?;
        let needs_snapshot_generation = supports_apply_time_static_snapshot_generation(record)
            && static_snapshot_service::snapshot_for_record(record).is_err();
        let snapshot_candidate = record.clone();
        if scene_changed {
            save_library(&store).map_err(|error| error.to_string())?;
        }
        (snapshot_candidate, needs_snapshot_generation)
    };

    if needs_snapshot_generation {
        let _ = ensure_static_snapshot(&mut snapshot_candidate);
    }

    let mut store = state.library.lock().map_err(|error| error.to_string())?;
    let record = store
        .wallpapers
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or_else(|| format!("Wallpaper {id} was not found"))?;

    let previous_snapshot_path = record.last_snapshot_path.clone();
    if needs_snapshot_generation
        && snapshot_generation_inputs_match(record, &snapshot_candidate)
        && static_snapshot_service::snapshot_for_record(&snapshot_candidate).is_ok()
    {
        record.last_snapshot_path = snapshot_candidate.last_snapshot_path.clone();
    }

    let snapshot_changed = record.last_snapshot_path != previous_snapshot_path;
    let cloned = record.clone();
    if snapshot_changed {
        save_library(&store).map_err(|error| error.to_string())?;
    }
    Ok(cloned)
}

fn supports_apply_time_static_snapshot_generation(record: &WallpaperRecord) -> bool {
    matches!(
        record.wallpaper_type,
        WallpaperType::Scene | WallpaperType::Video | WallpaperType::Web
    )
}

fn snapshot_generation_inputs_match(
    current: &WallpaperRecord,
    candidate: &WallpaperRecord,
) -> bool {
    current.id == candidate.id
        && current.wallpaper_type == candidate.wallpaper_type
        && current.managed_path == candidate.managed_path
        && current.entry_path == candidate.entry_path
}

pub(crate) fn clear_player_session_state(state: &AppState) -> Result<DynamicPlayerState, String> {
    let previous_player = state
        .player
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let cleared_player = build_cleared_player_state(&previous_player);
    commit_player_state(state, cleared_player.clone())?;
    persist_player_state_checked(&cleared_player)?;
    Ok(cleared_player)
}

pub fn pause_resume_dynamic(
    paused: bool,
    app: &AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    set_player_paused(paused, app, state)
}

pub fn clear_active_wallpaper(app: &AppHandle, state: &AppState) -> Result<(), String> {
    clear_player_session_state(state)?;

    let _ = static_snapshot_service::clear_active_snapshot_sync(app, state);
    scene_support_service::clear_scene_support_diagnostics(app);
    sync_native_runtime_with_transaction_lock(app, state, None, false)?;
    lifecycle_service::sync_pause_menu_state(app, false);
    app.emit("player:load", Option::<WallpaperRuntimeRecord>::None)
        .map_err(|error| error.to_string())?;
    app.emit("player:pause", false)
        .map_err(|error| error.to_string())?;
    lifecycle_service::close_player_windows(app).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn set_player_paused(
    paused: bool,
    app: &AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    let transition = {
        let mut player = state.player.lock().map_err(|error| error.to_string())?;
        let transition = apply_pause_change(&mut player, Some(paused), None);
        if transition.manual_changed {
            persist_player_state(&player);
        }
        transition
    };

    lifecycle_service::sync_pause_menu_state(app, transition.manual_paused);
    if transition.effective_changed {
        sync_native_runtime_for_active_wallpaper(app, state)?;
        app.emit("player:pause", transition.effective_paused)
            .map_err(|error| error.to_string())?;
    }
    Ok(transition.effective_paused)
}

pub(crate) fn set_player_auto_pause_screen_labels(
    labels: BTreeSet<String>,
    app: &AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    let transition = {
        let mut player = state.player.lock().map_err(|error| error.to_string())?;
        apply_pause_change(&mut player, None, Some(labels))
    };

    if transition.effective_changed {
        sync_native_runtime_for_active_wallpaper(app, state)?;
        app.emit("player:pause", transition.effective_paused)
            .map_err(|error| error.to_string())?;
    }
    Ok(transition.effective_paused)
}

pub fn sync_active_scene_signature(
    runtime_record: &WallpaperRuntimeRecord,
    state: &AppState,
) -> Result<(), String> {
    let mut player = state.player.lock().map_err(|error| error.to_string())?;
    if player.active_id.as_deref() == Some(runtime_record.id.as_str()) {
        player.last_scene_signature = scene_signature(runtime_record);
    }
    Ok(())
}

pub(crate) fn sync_native_runtime_for_active_wallpaper(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), String> {
    let _runtime_sync = state
        .runtime_sync
        .lock()
        .map_err(|error| error.to_string())?;
    match active_runtime_snapshot(state)? {
        Some((runtime_record, effective_paused)) => {
            sync_native_runtime(app, Some(&runtime_record), effective_paused)
        }
        None => sync_native_runtime(app, None, false),
    }
}

fn sync_native_runtime_with_transaction_lock(
    app: &AppHandle,
    state: &AppState,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    let _runtime_sync = state
        .runtime_sync
        .lock()
        .map_err(|error| error.to_string())?;
    sync_native_runtime(app, runtime_record, paused)
}

fn sync_native_runtime(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    let mut critical_errors = Vec::new();

    if let Err(error) =
        scene_native_renderer_service::sync_native_scene_runtime(app, runtime_record, paused)
    {
        push_critical_sync_error(
            &mut critical_errors,
            runtime_record,
            NativeHostKind::Scene,
            error,
        );
    }

    if let Err(error) =
        native_video_service::sync_native_video_playback(app, runtime_record, paused)
    {
        push_critical_sync_error(
            &mut critical_errors,
            runtime_record,
            NativeHostKind::Video,
            error,
        );
    }

    if let Err(error) = native_web_service::sync_native_web_runtime(app, runtime_record, paused) {
        push_critical_sync_error(
            &mut critical_errors,
            runtime_record,
            NativeHostKind::Web,
            error,
        );
    }

    audio_input_service::request_policy_refresh(app);
    if critical_errors.is_empty() {
        Ok(())
    } else {
        Err(critical_errors.join("; "))
    }
}

fn start_scene_update_loop(app: AppHandle, state: &AppState) {
    let generation = state
        .player
        .lock()
        .map(|player| player.scene_update_generation)
        .unwrap_or_default();

    thread::spawn(move || loop {
        let state = app.state::<AppState>();
        let active_id = {
            let player = match state.player.lock() {
                Ok(player) => player,
                Err(_) => return,
            };
            if player.scene_update_generation != generation {
                return;
            }
            match player.active_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        let record = {
            let store = match state.library.lock() {
                Ok(store) => store,
                Err(_) => return,
            };
            match find_record(&store, &active_id) {
                Some(record) => record,
                None => return,
            }
        };

        let sleep_for = match runtime_document_service::runtime_record(&record).runtime {
            WallpaperRuntime::Scene { scene } => {
                scene_update_sleep_duration(&scene, Local::now()).unwrap_or(Duration::from_secs(1))
            }
            _ => return,
        };
        thread::sleep(sleep_for);

        let runtime_record = {
            let store = match state.library.lock() {
                Ok(store) => store,
                Err(_) => return,
            };
            let Some(record) = find_record(&store, &active_id) else {
                return;
            };
            runtime_document_service::runtime_record(&record)
        };
        let signature = scene_signature(&runtime_record);
        if signature.is_none() {
            return;
        }
        let (should_emit, effective_paused) = {
            let mut player = match state.player.lock() {
                Ok(player) => player,
                Err(_) => return,
            };
            if player.scene_update_generation != generation
                || player.active_id.as_deref() != Some(active_id.as_str())
            {
                return;
            }
            if should_emit_scene_update(
                player.last_scene_signature.as_deref(),
                signature.as_deref(),
            ) {
                player.last_scene_signature = signature.clone();
                (true, player.effective_paused())
            } else {
                (false, player.effective_paused())
            }
        };

        if should_emit {
            let _runtime_sync = match state.runtime_sync.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if !scene_update_sync_is_current(&state, generation, active_id.as_str()) {
                return;
            }
            let _ = scene_native_renderer_service::sync_native_scene_runtime(
                &app,
                Some(&runtime_record),
                effective_paused,
            );
        }
    });
}

fn scene_update_sync_is_current(state: &AppState, generation: u64, active_id: &str) -> bool {
    state
        .player
        .lock()
        .map(|player| {
            player.scene_update_generation == generation
                && player.active_id.as_deref() == Some(active_id)
        })
        .unwrap_or(false)
}

fn active_runtime_snapshot(
    state: &AppState,
) -> Result<Option<(WallpaperRuntimeRecord, bool)>, String> {
    let Some((record, effective_paused)) = active_record_snapshot(state)? else {
        return Ok(None);
    };

    Ok(Some((
        runtime_document_service::runtime_record(&record),
        effective_paused,
    )))
}

fn active_record_snapshot(state: &AppState) -> Result<Option<(WallpaperRecord, bool)>, String> {
    let Some((active_id, effective_paused)) = active_player_selection(state)? else {
        return Ok(None);
    };

    let record =
        match scene_manifest_service::ensure_scene_manifest_current_by_id(state, &active_id) {
            Ok(record) => record,
            Err(_) => {
                let store = state.library.lock().map_err(|error| error.to_string())?;
                find_record(&store, &active_id)
                    .ok_or_else(|| format!("Wallpaper {active_id} was not found"))?
            }
        };

    Ok(Some((record, effective_paused)))
}

fn active_player_selection(state: &AppState) -> Result<Option<(String, bool)>, String> {
    let player = state.player.lock().map_err(|error| error.to_string())?;
    Ok(player
        .active_id
        .clone()
        .map(|active_id| (active_id, player.effective_paused())))
}

fn apply_runtime_record_transaction<
    SyncStaticSnapshot,
    EnsurePlayerWindows,
    SyncNativeRuntime,
    ClosePlayerWindows,
    PersistPlayer,
>(
    runtime_record: &WallpaperRuntimeRecord,
    previous_player: &DynamicPlayerState,
    previous_runtime: Option<(WallpaperRuntimeRecord, bool)>,
    state: &AppState,
    had_player_windows: bool,
    mut sync_static_snapshot: SyncStaticSnapshot,
    mut ensure_player_windows: EnsurePlayerWindows,
    mut sync_native_runtime: SyncNativeRuntime,
    mut close_player_windows: ClosePlayerWindows,
    persist_player: PersistPlayer,
) -> Result<bool, String>
where
    SyncStaticSnapshot: FnMut() -> Result<(), String>,
    EnsurePlayerWindows: FnMut() -> Result<(), String>,
    SyncNativeRuntime: FnMut(Option<&WallpaperRuntimeRecord>, bool) -> Result<(), String>,
    ClosePlayerWindows: FnMut() -> Result<(), String>,
    PersistPlayer: FnOnce(&DynamicPlayerState) -> Result<(), String>,
{
    let candidate = build_apply_wallpaper_candidate(previous_player, runtime_record);

    let _runtime_sync = state
        .runtime_sync
        .lock()
        .map_err(|error| error.to_string())?;
    commit_player_state(state, candidate.player_state.clone())?;
    if let Err(error) = sync_static_snapshot() {
        let state_rollback_error = commit_player_state(state, previous_player.clone())
            .err()
            .map(|rollback_error| {
                format!("failed to restore previous player state: {rollback_error}")
            });
        let runtime_rollback_error = rollback_failed_apply(
            previous_runtime.as_ref(),
            had_player_windows,
            &mut sync_native_runtime,
            &mut close_player_windows,
        )
        .err();
        let rollback_errors = [state_rollback_error, runtime_rollback_error]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !rollback_errors.is_empty() {
            return Err(format!("{error}; {}", rollback_errors.join("; ")));
        }
        return Err(error);
    }
    if let Err(error) = ensure_player_windows() {
        let state_rollback_error = commit_player_state(state, previous_player.clone())
            .err()
            .map(|rollback_error| {
                format!("failed to restore previous player state: {rollback_error}")
            });
        let runtime_rollback_error = rollback_failed_apply(
            previous_runtime.as_ref(),
            had_player_windows,
            &mut sync_native_runtime,
            &mut close_player_windows,
        )
        .err();
        let rollback_errors = [state_rollback_error, runtime_rollback_error]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !rollback_errors.is_empty() {
            return Err(format!("{error}; {}", rollback_errors.join("; ")));
        }
        return Err(error);
    }

    if let Err(error) = sync_native_runtime(Some(runtime_record), candidate.effective_paused) {
        let state_rollback_error = commit_player_state(state, previous_player.clone())
            .err()
            .map(|rollback_error| {
                format!("failed to restore previous player state: {rollback_error}")
            });
        let runtime_rollback_error = rollback_failed_apply(
            previous_runtime.as_ref(),
            had_player_windows,
            &mut sync_native_runtime,
            &mut close_player_windows,
        )
        .err();
        let rollback_errors = [state_rollback_error, runtime_rollback_error]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !rollback_errors.is_empty() {
            return Err(format!("{error}; {}", rollback_errors.join("; ")));
        }
        return Err(error);
    }

    persist_player(&candidate.player_state)?;
    Ok(candidate.effective_paused)
}

fn build_apply_wallpaper_candidate(
    previous_player: &DynamicPlayerState,
    runtime_record: &WallpaperRuntimeRecord,
) -> ApplyWallpaperCandidate {
    let mut player_state = previous_player.clone();
    player_state.active_id = Some(runtime_record.id.clone());
    player_state.manually_paused = false;
    player_state.scene_update_generation = player_state.scene_update_generation.saturating_add(1);
    player_state.last_scene_signature = scene_signature(runtime_record);

    ApplyWallpaperCandidate {
        effective_paused: player_state.effective_paused(),
        player_state,
    }
}

fn build_cleared_player_state(previous_player: &DynamicPlayerState) -> DynamicPlayerState {
    let mut player_state = previous_player.clone();
    player_state.active_id = None;
    player_state.manually_paused = false;
    player_state.auto_pause_screen_labels.clear();
    player_state.scene_update_generation = player_state.scene_update_generation.saturating_add(1);
    player_state.last_scene_signature = None;
    player_state
}

fn rollback_failed_apply<SyncNativeRuntime, ClosePlayerWindows>(
    previous_runtime: Option<&(WallpaperRuntimeRecord, bool)>,
    had_player_windows: bool,
    sync_native_runtime: &mut SyncNativeRuntime,
    close_player_windows: &mut ClosePlayerWindows,
) -> Result<(), String>
where
    SyncNativeRuntime: FnMut(Option<&WallpaperRuntimeRecord>, bool) -> Result<(), String>,
    ClosePlayerWindows: FnMut() -> Result<(), String>,
{
    let mut recovery_errors = Vec::new();

    match previous_runtime {
        Some((runtime_record, paused)) => {
            if let Err(error) = sync_native_runtime(Some(runtime_record), *paused) {
                recovery_errors.push(format!("failed to restore previous runtime: {error}"));
            }
        }
        None => {
            if let Err(error) = sync_native_runtime(None, false) {
                recovery_errors.push(format!("failed to clear failed runtime: {error}"));
            }
        }
    }

    if !had_player_windows {
        if let Err(error) = close_player_windows() {
            recovery_errors.push(format!(
                "failed to restore previous player windows: {error}"
            ));
        }
    }

    if recovery_errors.is_empty() {
        Ok(())
    } else {
        Err(recovery_errors.join("; "))
    }
}

fn commit_player_state(state: &AppState, player_state: DynamicPlayerState) -> Result<(), String> {
    let mut player = state.player.lock().map_err(|error| error.to_string())?;
    *player = player_state;
    Ok(())
}

fn apply_pause_change(
    player: &mut DynamicPlayerState,
    manual: Option<bool>,
    auto_pause_screen_labels: Option<BTreeSet<String>>,
) -> PauseTransition {
    let previous_manual = player.manually_paused;
    let previous_effective = player.effective_paused();

    if player.active_id.is_none() {
        player.manually_paused = false;
        player.auto_pause_screen_labels.clear();
    } else {
        if let Some(value) = manual {
            player.manually_paused = value;
        }
        if let Some(labels) = auto_pause_screen_labels {
            player.auto_pause_screen_labels = labels;
        }
    }

    PauseTransition {
        effective_paused: player.effective_paused(),
        effective_changed: previous_effective != player.effective_paused(),
        manual_changed: previous_manual != player.manually_paused,
        manual_paused: player.manually_paused,
    }
}

fn persist_player_state(player: &DynamicPlayerState) {
    if let Err(error) = save_player_state(player) {
        eprintln!("failed to persist player state: {error}");
    }
}

fn persist_player_state_checked(player: &DynamicPlayerState) -> Result<(), String> {
    save_player_state(player).map_err(|error| error.to_string())
}

fn scene_signature(runtime_record: &WallpaperRuntimeRecord) -> Option<String> {
    match &runtime_record.runtime {
        WallpaperRuntime::Scene { scene } => serde_json::to_string(&SceneEvaluationSignature {
            canvas_width: scene.evaluated.canvas_width,
            canvas_height: scene.evaluated.canvas_height,
            clear_color: &scene.evaluated.clear_color,
            camera: &scene.evaluated.camera,
            parallax: &scene.evaluated.parallax,
            objects: &scene.evaluated.objects,
            render_list: &scene.evaluated.render_list,
        })
        .ok(),
        _ => None,
    }
}

fn should_start_scene_update_loop(runtime_record: &WallpaperRuntimeRecord) -> bool {
    match &runtime_record.runtime {
        WallpaperRuntime::Scene { scene } => scene_update_cadence(scene).is_some(),
        _ => false,
    }
}

#[cfg(test)]
fn scene_requires_periodic_updates(scene: &SceneRuntimeDocument) -> bool {
    scene_update_cadence(scene).is_some()
}

fn scene_update_cadence(scene: &SceneRuntimeDocument) -> Option<SceneUpdateCadence> {
    scene
        .source
        .text_layers
        .iter()
        .filter_map(text_layer_update_cadence)
        .min_by_key(|cadence| cadence.interval_millis())
}

fn scene_update_sleep_duration(
    scene: &SceneRuntimeDocument,
    now: chrono::DateTime<Local>,
) -> Option<Duration> {
    let cadence = scene_update_cadence(scene)?;
    Some(match cadence {
        SceneUpdateCadence::CustomMillis(interval_millis) => {
            let now_millis = now.timestamp_millis().max(0) as u64;
            let remainder = now_millis % interval_millis.max(1);
            let remaining = if remainder == 0 {
                interval_millis
            } else {
                interval_millis - remainder
            };
            Duration::from_millis(remaining.max(1))
        }
        SceneUpdateCadence::Second => {
            let millis = now.timestamp_subsec_millis() as u64;
            Duration::from_millis((1000 - millis).max(1))
        }
        SceneUpdateCadence::TwoSeconds => {
            let millis = now.timestamp_subsec_millis() as u64;
            let seconds_to_boundary = if now.second() % 2 == 0 { 2 } else { 1 };
            Duration::from_millis(
                (seconds_to_boundary as u64)
                    .saturating_mul(1000)
                    .saturating_sub(millis)
                    .max(1),
            )
        }
        SceneUpdateCadence::Minute => {
            let millis = now.timestamp_subsec_millis() as u64;
            let seconds = now.second() as u64;
            Duration::from_millis(
                (60_u64 - seconds)
                    .saturating_mul(1000)
                    .saturating_sub(millis)
                    .max(1),
            )
        }
    })
}

impl SceneUpdateCadence {
    fn interval_millis(self) -> u64 {
        match self {
            SceneUpdateCadence::CustomMillis(interval_millis) => interval_millis.max(1),
            SceneUpdateCadence::Minute => 60_000,
            SceneUpdateCadence::TwoSeconds => 2_000,
            SceneUpdateCadence::Second => 1_000,
        }
    }
}

fn text_layer_update_cadence(layer: &SceneTextLayer) -> Option<SceneUpdateCadence> {
    if let Some(interval_millis) = layer
        .script_refresh_interval_millis
        .filter(|interval_millis| *interval_millis > 0)
    {
        return Some(match interval_millis {
            1..=999 => SceneUpdateCadence::CustomMillis(interval_millis),
            1_000 => SceneUpdateCadence::Second,
            2_000 => SceneUpdateCadence::TwoSeconds,
            60_000 => SceneUpdateCadence::Minute,
            _ => SceneUpdateCadence::CustomMillis(interval_millis),
        });
    }

    match layer.behavior {
        SceneTextBehavior::Clock if layer.show_seconds == Some(true) => {
            Some(SceneUpdateCadence::Second)
        }
        SceneTextBehavior::MediaTitle => Some(SceneUpdateCadence::TwoSeconds),
        SceneTextBehavior::Script => scripted_text_update_cadence(layer.script_text.as_deref()),
        SceneTextBehavior::Clock
        | SceneTextBehavior::Date
        | SceneTextBehavior::Weekday
        | SceneTextBehavior::DayPeriod => Some(SceneUpdateCadence::Minute),
        SceneTextBehavior::Fps | SceneTextBehavior::Static => None,
    }
}

fn scripted_text_update_cadence(script_text: Option<&str>) -> Option<SceneUpdateCadence> {
    let lower_script = script_text?.to_ascii_lowercase();

    if lower_script.contains("getseconds") || lower_script.contains("date.now") {
        Some(SceneUpdateCadence::Second)
    } else if lower_script.contains("new date")
        || lower_script.contains("getminutes")
        || lower_script.contains("gethours")
        || lower_script.contains("getday")
        || lower_script.contains("getmonth")
        || lower_script.contains("getfullyear")
    {
        Some(SceneUpdateCadence::Minute)
    } else {
        None
    }
}

fn native_host_sync_disposition(
    runtime_record: Option<&WallpaperRuntimeRecord>,
    host: NativeHostKind,
) -> NativeHostSyncDisposition {
    match (runtime_record.map(|record| &record.runtime), host) {
        (Some(WallpaperRuntime::Scene { .. }), NativeHostKind::Scene) => {
            NativeHostSyncDisposition::Critical
        }
        (Some(WallpaperRuntime::Video { .. }), NativeHostKind::Video) => {
            NativeHostSyncDisposition::Critical
        }
        (Some(WallpaperRuntime::Web { .. }), NativeHostKind::Web) => {
            NativeHostSyncDisposition::Critical
        }
        _ => NativeHostSyncDisposition::BestEffort,
    }
}

fn push_critical_sync_error(
    critical_errors: &mut Vec<String>,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    host: NativeHostKind,
    error: String,
) {
    if native_host_sync_disposition(runtime_record, host) == NativeHostSyncDisposition::Critical {
        critical_errors.push(format!(
            "{} runtime failed: {error}",
            match host {
                NativeHostKind::Scene => "native scene",
                NativeHostKind::Video => "native video",
                NativeHostKind::Web => "native web",
            }
        ));
    }
}

fn preflight_scene_apply(
    app: &AppHandle,
    record: &WallpaperRecord,
    runtime_record: &WallpaperRuntimeRecord,
) -> Result<(), String> {
    if !matches!(record.wallpaper_type, crate::models::WallpaperType::Scene) {
        scene_support_service::clear_scene_support_diagnostics(app);
        return Ok(());
    }

    let runtime_scene = match &runtime_record.runtime {
        WallpaperRuntime::Scene { scene } => Some(scene),
        _ => None,
    };
    scene_support_service::ensure_scene_supported_for_apply(app, record, runtime_scene)
}

#[cfg(test)]
fn validate_scene_apply_preflight(
    record: &WallpaperRecord,
    builtin_assets_root: &Path,
) -> Result<(), String> {
    if !matches!(record.wallpaper_type, crate::models::WallpaperType::Scene) {
        return Ok(());
    }

    let runtime_record = runtime_document_service::runtime_record(record);
    let runtime_scene = match &runtime_record.runtime {
        WallpaperRuntime::Scene { scene } => Some(scene),
        _ => None,
    };
    let report = scene_support_service::analyze_scene_support_with_builtin_root(
        record,
        builtin_assets_root,
        runtime_scene,
    );
    if report.is_supported() {
        Ok(())
    } else {
        Err(report.apply_error_message())
    }
}

fn should_emit_scene_update(current_signature: Option<&str>, next_signature: Option<&str>) -> bool {
    match (current_signature, next_signature) {
        (_, None) => false,
        (Some(current), Some(next)) if current == next => false,
        (_, Some(_)) => true,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        env, fs,
        sync::Mutex,
    };

    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    use crate::{
        models::{
            EvaluatedSceneCamera, EvaluatedSceneObject, EvaluatedSceneObjectBase,
            EvaluatedSceneTransform, EvaluatedTextLayout, EvaluatedTextState, EvaluatedTextStyle,
            LibraryStore, SceneEvaluatedDocument, SceneManifest, SceneRuntimeDocument,
            SceneRuntimeSettings, SceneTextBehavior, SceneTextLayer, WallpaperRecord,
            WallpaperRuntime, WallpaperRuntimeRecord, WallpaperType,
        },
        store::{AppState, DynamicPlayerState, HOME_ENV_LOCK},
    };

    use super::{
        apply_pause_change, apply_runtime_record_transaction, build_apply_wallpaper_candidate,
        clear_player_session_state, ensure_apply_record_current_by_id_with,
        native_host_sync_disposition, push_critical_sync_error, scene_requires_periodic_updates,
        scene_signature, scene_update_cadence, scene_update_sync_is_current,
        should_emit_scene_update, should_start_scene_update_loop, validate_scene_apply_preflight,
        NativeHostKind, NativeHostSyncDisposition, SceneUpdateCadence,
    };

    fn runtime_record(
        runtime: WallpaperRuntime,
        wallpaper_type: WallpaperType,
    ) -> WallpaperRuntimeRecord {
        WallpaperRuntimeRecord {
            id: "demo".to_string(),
            title: "Demo".to_string(),
            wallpaper_type,
            source_path: "/tmp/source".to_string(),
            managed_path: "/tmp/managed".to_string(),
            preview_path: None,
            entry_path: None,
            last_snapshot_path: None,
            property_schema: vec![],
            property_sections: vec![],
            imported_at: Utc::now(),
            tags: vec![],
            runtime,
        }
    }

    fn app_state(player: DynamicPlayerState) -> AppState {
        AppState {
            library: Mutex::new(LibraryStore::default()),
            player: Mutex::new(player),
            static_snapshot_sync: Mutex::new(crate::store::StaticSnapshotSyncState::default()),
            runtime_sync: Mutex::new(()),
            scene_runtime_settings: Mutex::new(SceneRuntimeSettings::default()),
        }
    }

    fn scene_record(managed_root: &str) -> WallpaperRecord {
        WallpaperRecord {
            id: "scene-demo".to_string(),
            title: "Scene Demo".to_string(),
            wallpaper_type: WallpaperType::Scene,
            source_path: managed_root.to_string(),
            managed_path: managed_root.to_string(),
            preview_path: None,
            entry_path: None,
            last_snapshot_path: None,
            property_schema: vec![],
            property_sections: vec![],
            scene_cache: None,
            scene_manifest: Some(SceneManifest::default()),
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec![],
        }
    }

    fn web_record(id: &str, managed_root: &str, entry_path: String) -> WallpaperRecord {
        WallpaperRecord {
            id: id.to_string(),
            title: "Web Demo".to_string(),
            wallpaper_type: WallpaperType::Web,
            source_path: managed_root.to_string(),
            managed_path: managed_root.to_string(),
            preview_path: Some(format!("{managed_root}/preview.png")),
            entry_path: Some(entry_path),
            last_snapshot_path: None,
            property_schema: vec![],
            property_sections: vec![],
            scene_cache: None,
            scene_manifest: None,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec![],
        }
    }

    fn text_object(behavior: SceneTextBehavior) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Text {
            base: EvaluatedSceneObjectBase {
                id: 7,
                name: "Clock".to_string(),
                parent_id: None,
                dependencies: vec![],
                visible: true,
                alignment: None,
                opacity: 1.0,
                transform: EvaluatedSceneTransform {
                    position: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    rotation: 0.0,
                    render_bounds: Some([0.0, 0.0, 320.0, 120.0]),
                },
            },
            behavior,
            text: EvaluatedTextState {
                value: "12:34:56".to_string(),
                style: EvaluatedTextStyle {
                    color: Some("1 1 1".to_string()),
                    alpha: 1.0,
                    point_size: 64.0,
                    font_path: None,
                    effect_paths: vec![],
                    horizontal_align: Some("center".to_string()),
                    vertical_align: Some("center".to_string()),
                    padding: Some(0.0),
                    max_rows: None,
                    max_width: None,
                    limit_width: None,
                    limit_use_ellipsis: None,
                    block_align: None,
                },
                layout: EvaluatedTextLayout {
                    size: Some([320.0, 120.0]),
                    render_bounds: Some([0.0, 0.0, 320.0, 120.0]),
                    content_bounds: Some([0.0, 0.0, 320.0, 120.0]),
                    scaled_point_size: 64.0,
                    scaled_padding: 0.0,
                    world_scale: [1.0, 1.0, 1.0],
                },
            },
        }
    }

    fn source_text_layer(
        behavior: SceneTextBehavior,
        show_seconds: Option<bool>,
        script_refresh_interval_millis: Option<u64>,
    ) -> SceneTextLayer {
        SceneTextLayer {
            id: 7,
            name: "Clock".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: Some("center".to_string()),
            vertical_align: Some("center".to_string()),
            content: "12:34".to_string(),
            behavior,
            delimiter: Some(":".to_string()),
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds,
            use_24h_format: Some(true),
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [0.0, 0.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: Some([320.0, 120.0]),
            render_bounds: Some([0.0, 0.0, 320.0, 120.0]),
            parallax_depth: None,
            color: Some("1 1 1".to_string()),
            color_binding: None,
            alpha: Some(1.0),
            alpha_binding: None,
            point_size: Some(64.0),
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: None,
            script_refresh_interval_millis,
            padding: Some(0.0),
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        }
    }

    fn scene_runtime_with_objects(
        objects: BTreeMap<u32, EvaluatedSceneObject>,
        source_text_layers: Vec<SceneTextLayer>,
        evaluated_at: chrono::DateTime<Utc>,
    ) -> SceneRuntimeDocument {
        SceneRuntimeDocument {
            runtime_owner_key: None,
            source: SceneManifest {
                text_layers: source_text_layers,
                ..SceneManifest::default()
            },
            evaluated: SceneEvaluatedDocument {
                canvas_width: 1920.0,
                canvas_height: 1080.0,
                clear_color: None,
                camera: EvaluatedSceneCamera {
                    zoom: 1.0,
                    center: [0.0, 0.0],
                    camera_shake: false,
                    camera_shake_amplitude: 0.0,
                    camera_shake_speed: 0.0,
                    parallax_mouse_influence: 0.0,
                },
                parallax: Default::default(),
                objects,
                render_list: vec![7],
                evaluated_at,
            },
        }
    }

    #[test]
    fn scene_update_emits_only_when_evaluated_signature_changes() {
        assert!(!should_emit_scene_update(Some("same"), Some("same")));
        assert!(should_emit_scene_update(Some("before"), Some("after")));
        assert!(should_emit_scene_update(None, Some("first")));
        assert!(!should_emit_scene_update(Some("existing"), None));
    }

    #[test]
    fn scene_signature_ignores_evaluated_timestamp_only_changes() {
        let mut objects = BTreeMap::new();
        objects.insert(7, text_object(SceneTextBehavior::Clock));
        let earlier = runtime_record(
            WallpaperRuntime::Scene {
                scene: scene_runtime_with_objects(
                    objects.clone(),
                    vec![source_text_layer(
                        SceneTextBehavior::Clock,
                        Some(true),
                        None,
                    )],
                    Utc.with_ymd_and_hms(2026, 4, 12, 13, 0, 0).unwrap(),
                ),
            },
            WallpaperType::Scene,
        );
        let later = runtime_record(
            WallpaperRuntime::Scene {
                scene: scene_runtime_with_objects(
                    objects,
                    vec![source_text_layer(
                        SceneTextBehavior::Clock,
                        Some(true),
                        None,
                    )],
                    Utc.with_ymd_and_hms(2026, 4, 12, 13, 0, 1).unwrap(),
                ),
            },
            WallpaperType::Scene,
        );

        assert_eq!(scene_signature(&earlier), scene_signature(&later));
    }

    #[test]
    fn scene_update_loop_starts_only_for_dynamic_scene_content() {
        let static_scene = runtime_record(
            WallpaperRuntime::Scene {
                scene: scene_runtime_with_objects(
                    BTreeMap::from([(7, text_object(SceneTextBehavior::Static))]),
                    vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                    Utc::now(),
                ),
            },
            WallpaperType::Scene,
        );
        let clock_scene = runtime_record(
            WallpaperRuntime::Scene {
                scene: scene_runtime_with_objects(
                    BTreeMap::from([(7, text_object(SceneTextBehavior::Clock))]),
                    vec![source_text_layer(
                        SceneTextBehavior::Clock,
                        Some(true),
                        None,
                    )],
                    Utc::now(),
                ),
            },
            WallpaperType::Scene,
        );

        let static_runtime = match &static_scene.runtime {
            WallpaperRuntime::Scene { scene } => scene,
            _ => unreachable!(),
        };
        let clock_runtime = match &clock_scene.runtime {
            WallpaperRuntime::Scene { scene } => scene,
            _ => unreachable!(),
        };

        assert!(!scene_requires_periodic_updates(static_runtime));
        assert!(scene_requires_periodic_updates(clock_runtime));
        assert!(!should_start_scene_update_loop(&static_scene));
        assert!(should_start_scene_update_loop(&clock_scene));
    }

    #[test]
    fn apply_time_record_resolution_generates_missing_web_snapshot() {
        let _lock = HOME_ENV_LOCK.lock().expect("home lock");
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let managed_root = temp.path().join("managed-web");
            let source_root = managed_root.join("source");
            fs::create_dir_all(&source_root).expect("source dir");
            let entry = source_root.join("index.html");
            fs::write(&entry, "<html><body>Web</body></html>").expect("entry");
            let snapshot = managed_root.join("snapshot.png");

            let state = app_state(DynamicPlayerState::default());
            {
                let mut library = state.library.lock().expect("library lock");
                library.wallpapers.push(web_record(
                    "web-demo",
                    managed_root.to_str().expect("managed root"),
                    entry.display().to_string(),
                ));
            }

            let record =
                ensure_apply_record_current_by_id_with(&state, "web-demo", |record| {
                    fs::write(&snapshot, b"snapshot").expect("snapshot");
                    record.last_snapshot_path = Some(snapshot.display().to_string());
                    crate::services::static_snapshot_generation_service::StaticSnapshotGenerationOutcome::Generated {
                        snapshot_path: snapshot.clone(),
                    }
                })
                .expect("apply record");

            assert_eq!(
                record.last_snapshot_path.as_deref(),
                Some(snapshot.to_str().expect("snapshot path"))
            );
            let saved = fs::read_to_string(
                temp.path()
                    .join("Library/Application Support/WallpaperWorkbench/library.json"),
            )
            .expect("saved library");
            assert!(saved.contains(snapshot.to_str().expect("snapshot path")));
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn apply_time_record_resolution_generates_missing_scene_snapshot() {
        let _lock = HOME_ENV_LOCK.lock().expect("home lock");
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let managed_root = temp.path().join("managed-scene");
            let source_root = managed_root.join("source");
            fs::create_dir_all(&source_root).expect("source dir");
            fs::write(
                source_root.join("project.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "title": "Scene Demo",
                    "type": "scene",
                    "file": "scene.json"
                }))
                .expect("project json"),
            )
            .expect("project");
            fs::write(
                source_root.join("scene.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "general": { "orthogonalprojection": { "width": 200, "height": 120 } },
                    "objects": []
                }))
                .expect("scene json"),
            )
            .expect("scene");
            let snapshot = managed_root.join("snapshot.png");

            let state = app_state(DynamicPlayerState::default());
            {
                let mut library = state.library.lock().expect("library lock");
                library
                    .wallpapers
                    .push(scene_record(managed_root.to_str().expect("managed root")));
            }

            let record =
                ensure_apply_record_current_by_id_with(&state, "scene-demo", |record| {
                    fs::write(&snapshot, b"snapshot").expect("snapshot");
                    record.last_snapshot_path = Some(snapshot.display().to_string());
                    crate::services::static_snapshot_generation_service::StaticSnapshotGenerationOutcome::Generated {
                        snapshot_path: snapshot.clone(),
                    }
                })
                .expect("apply record");

            assert_eq!(
                record.last_snapshot_path.as_deref(),
                Some(snapshot.to_str().expect("snapshot path"))
            );
            let saved = fs::read_to_string(
                temp.path()
                    .join("Library/Application Support/WallpaperWorkbench/library.json"),
            )
            .expect("saved library");
            assert!(saved.contains(snapshot.to_str().expect("snapshot path")));
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn scene_update_cadence_prefers_second_precision_then_media_then_minute() {
        let second_runtime = scene_runtime_with_objects(
            BTreeMap::from([(7, text_object(SceneTextBehavior::Clock))]),
            vec![source_text_layer(
                SceneTextBehavior::Clock,
                Some(true),
                None,
            )],
            Utc::now(),
        );
        let media_runtime = scene_runtime_with_objects(
            BTreeMap::from([(7, text_object(SceneTextBehavior::MediaTitle))]),
            vec![source_text_layer(SceneTextBehavior::MediaTitle, None, None)],
            Utc::now(),
        );
        let minute_runtime = scene_runtime_with_objects(
            BTreeMap::from([(7, text_object(SceneTextBehavior::Date))]),
            vec![source_text_layer(SceneTextBehavior::Date, None, None)],
            Utc::now(),
        );

        assert_eq!(
            scene_update_cadence(&second_runtime),
            Some(SceneUpdateCadence::Second)
        );
        assert_eq!(
            scene_update_cadence(&media_runtime),
            Some(SceneUpdateCadence::TwoSeconds)
        );
        assert_eq!(
            scene_update_cadence(&minute_runtime),
            Some(SceneUpdateCadence::Minute)
        );
    }

    #[test]
    fn scene_update_cadence_prefers_script_refresh_interval_when_present() {
        let custom_runtime = scene_runtime_with_objects(
            BTreeMap::from([(7, text_object(SceneTextBehavior::Clock))]),
            vec![source_text_layer(
                SceneTextBehavior::Clock,
                Some(true),
                Some(1500),
            )],
            Utc::now(),
        );

        assert_eq!(
            scene_update_cadence(&custom_runtime),
            Some(SceneUpdateCadence::CustomMillis(1500))
        );
    }

    #[test]
    fn scene_update_cadence_tracks_time_aware_script_text_layers() {
        let mut script_layer = source_text_layer(SceneTextBehavior::Script, None, None);
        script_layer.script_text = Some(
            "'use strict'; export function update() { return new Date().getHours().toString(); }"
                .to_string(),
        );
        let script_runtime = scene_runtime_with_objects(
            BTreeMap::from([(7, text_object(SceneTextBehavior::Script))]),
            vec![script_layer],
            Utc::now(),
        );

        assert_eq!(
            scene_update_cadence(&script_runtime),
            Some(SceneUpdateCadence::Minute)
        );
    }

    #[test]
    fn manual_and_auto_pause_state_remain_isolated() {
        let mut player = DynamicPlayerState {
            active_id: Some("demo".to_string()),
            ..DynamicPlayerState::default()
        };

        let auto_pause = apply_pause_change(
            &mut player,
            None,
            Some(BTreeSet::from([String::from("player")])),
        );
        assert!(auto_pause.effective_paused);
        assert!(auto_pause.effective_changed);
        assert!(!auto_pause.manual_paused);

        let manual_pause = apply_pause_change(&mut player, Some(true), None);
        assert!(manual_pause.effective_paused);
        assert!(!manual_pause.effective_changed);
        assert!(manual_pause.manual_changed);
        assert!(manual_pause.manual_paused);

        let auto_resume = apply_pause_change(&mut player, None, Some(BTreeSet::new()));
        assert!(auto_resume.effective_paused);
        assert!(!auto_resume.effective_changed);
        assert!(auto_resume.manual_paused);

        let manual_resume = apply_pause_change(&mut player, Some(false), None);
        assert!(!manual_resume.effective_paused);
        assert!(manual_resume.effective_changed);
        assert!(manual_resume.manual_changed);
        assert!(!manual_resume.manual_paused);
    }

    #[test]
    fn pause_flags_reset_when_no_active_wallpaper_exists() {
        let mut player = DynamicPlayerState {
            active_id: None,
            manually_paused: true,
            auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
            scene_update_generation: 0,
            last_scene_signature: None,
        };

        let transition = apply_pause_change(&mut player, Some(true), None);
        assert!(!transition.effective_paused);
        assert!(transition.effective_changed);
        assert!(transition.manual_changed);
        assert!(!player.manually_paused);
        assert!(player.auto_pause_screen_labels.is_empty());
    }

    #[test]
    fn inactive_native_hosts_only_fail_best_effort_sync() {
        let scene = runtime_record(
            WallpaperRuntime::Scene {
                scene: Default::default(),
            },
            WallpaperType::Scene,
        );
        let video = runtime_record(
            WallpaperRuntime::Video {
                video: Default::default(),
            },
            WallpaperType::Video,
        );
        let web = runtime_record(
            WallpaperRuntime::Web {
                web: Default::default(),
            },
            WallpaperType::Web,
        );

        assert_eq!(
            native_host_sync_disposition(Some(&scene), NativeHostKind::Scene),
            NativeHostSyncDisposition::Critical
        );
        assert_eq!(
            native_host_sync_disposition(Some(&scene), NativeHostKind::Video),
            NativeHostSyncDisposition::BestEffort
        );
        assert_eq!(
            native_host_sync_disposition(Some(&scene), NativeHostKind::Web),
            NativeHostSyncDisposition::BestEffort
        );
        assert_eq!(
            native_host_sync_disposition(Some(&video), NativeHostKind::Video),
            NativeHostSyncDisposition::Critical
        );
        assert_eq!(
            native_host_sync_disposition(Some(&video), NativeHostKind::Web),
            NativeHostSyncDisposition::BestEffort
        );
        assert_eq!(
            native_host_sync_disposition(Some(&web), NativeHostKind::Video),
            NativeHostSyncDisposition::BestEffort
        );
        assert_eq!(
            native_host_sync_disposition(Some(&web), NativeHostKind::Web),
            NativeHostSyncDisposition::Critical
        );
        assert_eq!(
            native_host_sync_disposition(None, NativeHostKind::Video),
            NativeHostSyncDisposition::BestEffort
        );
    }

    #[test]
    fn critical_apply_failure_keeps_previous_player_state_and_skips_persist() {
        let previous_player = DynamicPlayerState {
            active_id: Some("known-good".to_string()),
            manually_paused: true,
            auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
            scene_update_generation: 4,
            last_scene_signature: Some("previous-signature".to_string()),
        };
        let previous_runtime = runtime_record(
            WallpaperRuntime::Web {
                web: Default::default(),
            },
            WallpaperType::Web,
        );
        let state = app_state(previous_player.clone());
        let candidate_runtime = runtime_record(
            WallpaperRuntime::Video {
                video: Default::default(),
            },
            WallpaperType::Video,
        );
        let sync_calls = RefCell::new(Vec::new());
        let persist_calls = RefCell::new(Vec::new());

        let result = apply_runtime_record_transaction(
            &candidate_runtime,
            &previous_player,
            Some((previous_runtime.clone(), previous_player.effective_paused())),
            &state,
            true,
            || Ok(()),
            || Ok(()),
            |runtime_record, paused| {
                sync_calls
                    .borrow_mut()
                    .push((runtime_record.map(|record| record.id.clone()), paused));
                if runtime_record.map(|record| record.id.as_str())
                    == Some(candidate_runtime.id.as_str())
                {
                    Err("native video runtime failed".to_string())
                } else {
                    Ok(())
                }
            },
            || Ok(()),
            |player| {
                persist_calls.borrow_mut().push(player.active_id.clone());
                Ok(())
            },
        );

        assert!(result.is_err());
        assert_eq!(
            sync_calls.into_inner(),
            vec![
                (
                    Some(candidate_runtime.id.clone()),
                    build_apply_wallpaper_candidate(&previous_player, &candidate_runtime)
                        .effective_paused,
                ),
                (
                    Some(previous_runtime.id.clone()),
                    previous_player.effective_paused(),
                ),
            ]
        );
        assert!(persist_calls.into_inner().is_empty());
        let player = state.player.lock().expect("lock player").clone();
        assert_eq!(player.active_id, previous_player.active_id);
        assert_eq!(player.manually_paused, previous_player.manually_paused);
        assert_eq!(
            player.auto_pause_screen_labels,
            previous_player.auto_pause_screen_labels
        );
        assert_eq!(
            player.scene_update_generation,
            previous_player.scene_update_generation
        );
        assert_eq!(
            player.last_scene_signature,
            previous_player.last_scene_signature
        );
    }

    #[test]
    fn apply_transaction_commits_active_state_before_native_sync() {
        let previous_player = DynamicPlayerState {
            active_id: Some("old-wallpaper".to_string()),
            manually_paused: true,
            auto_pause_screen_labels: BTreeSet::new(),
            scene_update_generation: 11,
            last_scene_signature: Some("old-signature".to_string()),
        };
        let state = app_state(previous_player.clone());
        let mut candidate_runtime = runtime_record(
            WallpaperRuntime::Video {
                video: Default::default(),
            },
            WallpaperType::Video,
        );
        candidate_runtime.id = "new-wallpaper".to_string();
        let observed_sync_state = RefCell::new(Vec::new());
        let observed_static_state = RefCell::new(Vec::new());
        let order = RefCell::new(Vec::new());

        let result = apply_runtime_record_transaction(
            &candidate_runtime,
            &previous_player,
            None,
            &state,
            true,
            || {
                let player = state.player.lock().expect("player lock").clone();
                observed_static_state
                    .borrow_mut()
                    .push((player.active_id, player.scene_update_generation));
                order.borrow_mut().push("static");
                Ok(())
            },
            || {
                order.borrow_mut().push("windows");
                Ok(())
            },
            |runtime_record, paused| {
                order.borrow_mut().push("native");
                let player = state.player.lock().expect("player lock").clone();
                observed_sync_state.borrow_mut().push((
                    runtime_record.map(|record| record.id.clone()),
                    paused,
                    player.active_id,
                    player.scene_update_generation,
                    player.manually_paused,
                ));
                Ok(())
            },
            || Ok(()),
            |_| Ok(()),
        );

        assert_eq!(result, Ok(false));
        assert_eq!(
            observed_static_state.into_inner(),
            vec![(Some("new-wallpaper".to_string()), 12)]
        );
        assert_eq!(order.into_inner(), vec!["static", "windows", "native"]);
        assert_eq!(
            observed_sync_state.into_inner(),
            vec![(
                Some("new-wallpaper".to_string()),
                false,
                Some("new-wallpaper".to_string()),
                12,
                false,
            )]
        );
        let player = state.player.lock().expect("player lock").clone();
        assert_eq!(player.active_id.as_deref(), Some("new-wallpaper"));
        assert_eq!(player.scene_update_generation, 12);
    }

    #[test]
    fn scene_update_sync_guard_rejects_stale_active_state() {
        let state = app_state(DynamicPlayerState {
            active_id: Some("new-wallpaper".to_string()),
            manually_paused: false,
            auto_pause_screen_labels: BTreeSet::new(),
            scene_update_generation: 8,
            last_scene_signature: None,
        });

        assert!(scene_update_sync_is_current(&state, 8, "new-wallpaper"));
        assert!(!scene_update_sync_is_current(&state, 7, "new-wallpaper"));
        assert!(!scene_update_sync_is_current(&state, 8, "old-wallpaper"));
    }

    #[test]
    fn inactive_host_sync_errors_do_not_accumulate_as_critical_failures() {
        let scene = runtime_record(
            WallpaperRuntime::Scene {
                scene: Default::default(),
            },
            WallpaperType::Scene,
        );
        let video = runtime_record(
            WallpaperRuntime::Video {
                video: Default::default(),
            },
            WallpaperType::Video,
        );
        let mut critical_errors = Vec::new();

        push_critical_sync_error(
            &mut critical_errors,
            Some(&scene),
            NativeHostKind::Scene,
            "scene host failed".to_string(),
        );
        assert_eq!(
            critical_errors,
            vec!["native scene runtime failed: scene host failed".to_string()]
        );

        critical_errors.clear();
        push_critical_sync_error(
            &mut critical_errors,
            Some(&scene),
            NativeHostKind::Video,
            "cleanup failed".to_string(),
        );
        assert!(critical_errors.is_empty());

        push_critical_sync_error(
            &mut critical_errors,
            Some(&video),
            NativeHostKind::Video,
            "start failed".to_string(),
        );
        assert_eq!(
            critical_errors,
            vec!["native video runtime failed: start failed".to_string()]
        );
    }

    #[test]
    fn clearing_player_session_state_persists_empty_restore_state_for_restart() {
        let _lock = HOME_ENV_LOCK.lock().expect("home lock");
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let state = app_state(DynamicPlayerState {
                active_id: Some("broken".to_string()),
                manually_paused: true,
                auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
                scene_update_generation: 7,
                last_scene_signature: Some("scene-signature".to_string()),
            });

            let cleared = clear_player_session_state(&state).expect("clear player state");
            assert!(cleared.active_id.is_none());
            assert!(!cleared.manually_paused);
            assert!(cleared.auto_pause_screen_labels.is_empty());
            assert_eq!(cleared.scene_update_generation, 8);
            assert!(cleared.last_scene_signature.is_none());

            let reloaded = AppState::load().expect("reload state after persisted clear");
            let player = reloaded.player.lock().expect("player lock").clone();
            assert!(player.active_id.is_none());
            assert!(!player.manually_paused);
            assert!(player.auto_pause_screen_labels.is_empty());
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn unsupported_scene_apply_preflight_returns_explicit_error() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");

        fs::create_dir_all(extracted_root.join("models")).expect("models dir");
        fs::create_dir_all(extracted_root.join("materials")).expect("materials dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        fs::write(
            extracted_root.join("scene.json"),
            r#"{"objects":[{"id":1,"name":"Hero","image":"models/hero.json"}]}"#,
        )
        .expect("scene json");
        fs::write(
            extracted_root.join("models").join("hero.json"),
            r#"{"material":"materials/hero.material"}"#,
        )
        .expect("hero model");
        fs::write(
            extracted_root.join("materials").join("hero.material"),
            r#"{"passes":[{"shader":"shaders/hero.frag"}]}"#,
        )
        .expect("hero material");

        let mut record = scene_record(&managed_root.display().to_string());
        record.scene_manifest = Some(
            crate::scene::parse_scene_manifest(
                &extracted_root.join("scene.json"),
                &extracted_root,
                &BTreeMap::new(),
            )
            .expect("parse scene manifest"),
        );

        let error = validate_scene_apply_preflight(&record, &builtin_root)
            .expect_err("unsupported scene should be blocked");

        assert!(error.contains("Scene native apply is blocked in the native Scene runtime"));
        assert!(error.contains("no renderable output"));
    }
}
