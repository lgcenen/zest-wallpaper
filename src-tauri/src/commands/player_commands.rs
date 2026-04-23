use tauri::{AppHandle, State, Window};

use crate::{
    models::{PlayerRuntimeState, WallpaperRuntimeRecord},
    services::{
        audio_input_service::{self, AudioSnapshot},
        diagnostic_service::{self, RuntimeDiagnostic},
        input_service::{self, SharedInputSnapshot},
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
pub fn get_player_input_snapshot(app: AppHandle) -> Result<SharedInputSnapshot, String> {
    input_service::current_input_snapshot(&app)
}

#[tauri::command]
pub fn get_player_audio_snapshot(app: AppHandle) -> Result<AudioSnapshot, String> {
    audio_input_service::current_audio_snapshot(&app)
}

#[tauri::command]
pub fn get_player_diagnostics(app: AppHandle) -> Result<Vec<RuntimeDiagnostic>, String> {
    diagnostic_service::current_runtime_diagnostics(&app)
}

#[tauri::command]
pub fn set_scene_audio_interest(
    active: bool,
    window: Window,
    app: AppHandle,
) -> Result<(), String> {
    audio_input_service::set_scene_audio_interest(&app, window.label(), active)
}
