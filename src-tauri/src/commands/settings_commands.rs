use tauri::{AppHandle, State};

use crate::{
    models::SceneRuntimeSettingsSnapshot, services::scene_runtime_settings_service, store::AppState,
};

#[tauri::command]
pub fn get_scene_runtime_settings(
    state: State<'_, AppState>,
) -> Result<SceneRuntimeSettingsSnapshot, String> {
    scene_runtime_settings_service::snapshot(&state)
}

#[tauri::command]
pub fn set_scene_external_assets_path(
    path: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SceneRuntimeSettingsSnapshot, String> {
    scene_runtime_settings_service::set_external_assets_path(&app, &state, path)
}
