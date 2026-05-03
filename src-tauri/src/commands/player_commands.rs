use tauri::{AppHandle, State};

use crate::{
    models::{PlayerRuntimeState, WallpaperRuntimeRecord},
    services::{
        diagnostic_service::{self, RuntimeDiagnostic},
        player_service,
    },
    store::AppState,
};

#[tauri::command]
pub fn apply_dynamic_wallpaper(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WallpaperRuntimeRecord, String> {
    player_service::apply_dynamic_wallpaper(&id, &app, &state)
}

#[tauri::command]
pub fn pause_resume_dynamic(
    paused: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    player_service::pause_resume_dynamic(paused, &app, &state)
}

#[tauri::command]
pub fn get_player_state(state: State<'_, AppState>) -> Result<PlayerRuntimeState, String> {
    player_service::get_player_state_snapshot(&state)
}

#[tauri::command]
pub fn get_player_diagnostics(app: AppHandle) -> Result<Vec<RuntimeDiagnostic>, String> {
    diagnostic_service::current_runtime_diagnostics(&app)
}
