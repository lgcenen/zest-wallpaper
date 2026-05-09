use std::{cell::Cell, sync::MutexGuard, time::Instant};

use tauri::{AppHandle, Emitter};

use crate::{
    models::{WallpaperRecord, WallpaperRuntime, WallpaperRuntimeRecord, WallpaperType},
    services::{
        lifecycle_service, scene_manifest_service, scene_support_service,
        static_snapshot_generation_service, static_snapshot_service, window_service,
    },
    store::{find_record, save_library, save_player_state, AppState, DynamicPlayerState},
};

use super::{runtime_document_service, scene_update, snapshot_sync};

#[derive(Debug, Clone)]
pub(super) struct ApplyWallpaperCandidate {
    pub(super) player_state: DynamicPlayerState,
    pub(super) effective_paused: bool,
}

struct ApplyStageTrace {
    wallpaper_id: String,
    wallpaper_type: WallpaperType,
    started_at: Instant,
}

pub fn apply_dynamic_wallpaper(
    id: &str,
    app: &AppHandle,
    state: &AppState,
) -> Result<WallpaperRuntimeRecord, String> {
    let record = ensure_apply_record_current_by_id(state, id)?;
    let trace = ApplyStageTrace::new(&record);
    trace.done("record_resolved");
    let runtime_record = runtime_document_service::runtime_record(&record);
    trace.done("runtime_record_built");
    preflight_scene_apply(app, &record, &runtime_record).map_err(|error| {
        trace.failed("scene_preflight_ok", &error);
        error
    })?;
    trace.done("scene_preflight_ok");
    let previous_player = state
        .player
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let previous_runtime = active_runtime_snapshot(state)?;
    let had_player_windows = !window_service::player_window_labels(app).is_empty();
    let snapshot_stage_logged = Cell::new(false);

    let effective_paused = match apply_runtime_record_transaction(
        &runtime_record,
        &previous_player,
        previous_runtime,
        state,
        had_player_windows,
        || {
            let result =
                snapshot_sync::sync_static_snapshot_for_active_runtime(app, state, &record);
            snapshot_stage_logged.set(true);
            trace.log_result("snapshot_sync_done", &result);
            result
        },
        || {
            let result =
                lifecycle_service::show_player_windows(app).map_err(|error| error.to_string());
            trace.log_result("player_windows_shown", &result);
            result
        },
        |runtime_record, paused| {
            let result = scene_update::sync_native_runtime(app, runtime_record, paused);
            trace.log_result("native_scene_sync_done", &result);
            result
        },
        || lifecycle_service::close_player_windows(app).map_err(|error| error.to_string()),
        |player| {
            persist_player_state(player);
            Ok(())
        },
    ) {
        Ok(effective_paused) => effective_paused,
        Err(error) => {
            if !snapshot_stage_logged.get() {
                trace.failed("snapshot_sync_done", &error);
            }
            return Err(error);
        }
    };

    lifecycle_service::sync_pause_menu_state(app, false);
    app.emit("player:load", Some(runtime_record.clone()))
        .map_err(|error| error.to_string())?;
    app.emit("player:pause", effective_paused)
        .map_err(|error| error.to_string())?;
    trace.done("apply_commit_done");
    if scene_update::should_start_scene_update_loop(&runtime_record) {
        scene_update::start_scene_update_loop(app.clone(), state);
    }
    Ok(runtime_record)
}

pub(super) fn restore_runtime_record_transaction<
    EnsurePlayerWindows,
    SyncNativeRuntime,
    SyncStaticSnapshot,
>(
    runtime_record: &WallpaperRuntimeRecord,
    effective_paused: bool,
    mut ensure_player_windows: EnsurePlayerWindows,
    mut sync_native_runtime: SyncNativeRuntime,
    mut sync_static_snapshot: SyncStaticSnapshot,
) -> Result<(), String>
where
    EnsurePlayerWindows: FnMut() -> Result<(), String>,
    SyncNativeRuntime: FnMut(Option<&WallpaperRuntimeRecord>, bool) -> Result<(), String>,
    SyncStaticSnapshot: FnMut() -> Result<(), String>,
{
    ensure_player_windows()?;
    sync_native_runtime(Some(runtime_record), effective_paused)?;
    sync_static_snapshot()?;
    Ok(())
}

pub fn restore_player_session(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let Some((active_id, effective_paused)) = active_player_selection(state)? else {
        return Ok(());
    };
    let record = ensure_apply_record_current_by_id(state, &active_id)?;
    let runtime_record = runtime_document_service::runtime_record(&record);
    preflight_scene_apply(app, &record, &runtime_record)?;

    restore_runtime_record_transaction(
        &runtime_record,
        effective_paused,
        || lifecycle_service::show_player_windows(app).map_err(|error| error.to_string()),
        |runtime_record, paused| {
            scene_update::sync_native_runtime_with_transaction_lock(
                app,
                state,
                runtime_record,
                paused,
            )
        },
        || snapshot_sync::sync_static_snapshot_for_active_runtime(app, state, &record),
    )?;
    if scene_update::should_start_scene_update_loop(&runtime_record) {
        scene_update::start_scene_update_loop(app.clone(), state);
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

pub(super) fn ensure_apply_record_current_by_id_with<EnsureStaticSnapshot>(
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
        let needs_snapshot_generation =
            snapshot_sync::supports_apply_time_static_snapshot_generation(record)
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
        && snapshot_sync::snapshot_generation_inputs_match(record, &snapshot_candidate)
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

impl ApplyStageTrace {
    fn new(record: &WallpaperRecord) -> Self {
        Self {
            wallpaper_id: record.id.clone(),
            wallpaper_type: record.wallpaper_type.clone(),
            started_at: Instant::now(),
        }
    }

    fn done(&self, stage: &str) {
        self.log(stage, "done", None);
    }

    fn failed(&self, stage: &str, error: &str) {
        self.log(stage, "failed", Some(error));
    }

    fn log_result<T>(&self, stage: &str, result: &Result<T, String>) {
        match result {
            Ok(_) => self.done(stage),
            Err(error) => self.failed(stage, error),
        }
    }

    fn log(&self, stage: &str, status: &str, error: Option<&str>) {
        let elapsed_ms = self.started_at.elapsed().as_millis();
        match error {
            Some(error) => eprintln!(
                "[wallpaper-apply] stage={stage} status={status} id={} type={:?} elapsed_ms={elapsed_ms} error={error}",
                self.wallpaper_id,
                self.wallpaper_type
            ),
            None => eprintln!(
                "[wallpaper-apply] stage={stage} status={status} id={} type={:?} elapsed_ms={elapsed_ms}",
                self.wallpaper_id,
                self.wallpaper_type
            ),
        }
    }
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

pub(super) fn active_runtime_snapshot(
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

pub(super) fn active_record_snapshot(
    state: &AppState,
) -> Result<Option<(WallpaperRecord, bool)>, String> {
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

pub(super) fn apply_runtime_record_transaction<
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

    let _runtime_sync = lock_runtime_sync_for_apply(state)?;
    commit_player_state(state, candidate.player_state.clone())?;
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

    persist_player(&candidate.player_state)?;
    Ok(candidate.effective_paused)
}

fn lock_runtime_sync_for_apply(state: &AppState) -> Result<MutexGuard<'_, ()>, String> {
    state.runtime_sync.lock().map_err(|error| error.to_string())
}

pub(super) fn build_apply_wallpaper_candidate(
    previous_player: &DynamicPlayerState,
    runtime_record: &WallpaperRuntimeRecord,
) -> ApplyWallpaperCandidate {
    let mut player_state = previous_player.clone();
    player_state.active_id = Some(runtime_record.id.clone());
    player_state.manually_paused = false;
    player_state.scene_update_generation = player_state.scene_update_generation.saturating_add(1);
    player_state.last_scene_signature = scene_update::scene_signature(runtime_record);

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

pub(super) fn persist_player_state(player: &DynamicPlayerState) {
    if let Err(error) = save_player_state(player) {
        eprintln!("failed to persist player state: {error}");
    }
}

fn persist_player_state_checked(player: &DynamicPlayerState) -> Result<(), String> {
    save_player_state(player).map_err(|error| error.to_string())
}

fn preflight_scene_apply(
    app: &AppHandle,
    record: &WallpaperRecord,
    runtime_record: &WallpaperRuntimeRecord,
) -> Result<(), String> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
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
pub(super) fn validate_scene_apply_preflight(
    record: &WallpaperRecord,
    builtin_assets_root: &std::path::Path,
) -> Result<(), String> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
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
