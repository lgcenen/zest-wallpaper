mod apply_flow;
mod pause_state;
mod scene_update;
mod snapshot_sync;

use tauri::{AppHandle, Emitter};

use crate::{
    models::{PlayerRuntimeState, WallpaperRuntimeRecord},
    services::{lifecycle_service, scene_support_service, static_snapshot_service},
    store::AppState,
};

use super::runtime_document_service;

pub use apply_flow::{apply_dynamic_wallpaper, restore_player_session};
pub use pause_state::pause_resume_dynamic;

pub(crate) use apply_flow::clear_player_session_state;
pub(crate) use pause_state::{set_player_auto_pause_screen_labels, set_player_paused};
pub(crate) use scene_update::{
    sync_active_wallpaper_after_property_update, sync_native_runtime_for_active_wallpaper,
};

pub fn get_player_state_snapshot(state: &AppState) -> Result<PlayerRuntimeState, String> {
    let player = state.player.lock().map_err(|error| error.to_string())?;
    runtime_document_service::player_runtime_state(&player, state)
}

pub fn clear_active_wallpaper(app: &AppHandle, state: &AppState) -> Result<(), String> {
    clear_player_session_state(state)?;

    let _ = static_snapshot_service::clear_active_snapshot_sync(app, state);
    scene_support_service::clear_scene_support_diagnostics(app);
    scene_update::sync_native_runtime_with_transaction_lock(app, state, None, false)?;
    lifecycle_service::sync_pause_menu_state(app, false);
    app.emit("player:load", Option::<WallpaperRuntimeRecord>::None)
        .map_err(|error| error.to_string())?;
    app.emit("player:pause", false)
        .map_err(|error| error.to_string())?;
    lifecycle_service::close_player_windows(app).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
use apply_flow::{
    apply_runtime_record_transaction, build_apply_wallpaper_candidate,
    ensure_apply_record_current_by_id_with, restore_runtime_record_transaction,
    validate_scene_apply_preflight,
};
#[cfg(test)]
use pause_state::apply_pause_change;
#[cfg(test)]
use scene_update::{
    active_property_update_sync_mode, dispatch_active_property_update_sync,
    dispatch_scene_update_sync, native_host_sync_disposition, push_critical_sync_error,
    scene_requires_periodic_updates, scene_signature, scene_update_cadence,
    scene_update_sync_is_current, scene_update_sync_mode, should_emit_scene_update,
    should_start_scene_update_loop, ActivePropertyUpdateSyncMode, NativeHostKind,
    NativeHostSyncDisposition, SceneUpdateCadence, SceneUpdateSyncMode,
};

#[cfg(test)]
mod tests;
