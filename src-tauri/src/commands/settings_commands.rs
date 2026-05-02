use std::process::Command;

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

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    if !matches!(url.split_once(":"), Some((scheme, _)) if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return Err("only http and https URLs can be opened externally".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg(&url)
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("failed to open external URL: {url}"));
    }

    #[cfg(target_os = "linux")]
    {
        let status = Command::new("xdg-open")
            .arg(&url)
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("failed to open external URL: {url}"));
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd")
            .args(["/C", "start", "", &url])
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err(format!("failed to open external URL: {url}"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        Err("opening external URLs is not supported on this platform".to_string())
    }
}
