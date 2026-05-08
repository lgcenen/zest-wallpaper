use std::process::Command;

use tauri::{AppHandle, State};

use crate::{
    models::SceneRuntimeSettingsSnapshot,
    services::{
        native_video_service, native_web_service, scene_native_renderer_service,
        scene_runtime_settings_service,
    },
    store::AppState,
};

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_CHARS[((triple >> 6) & 0x3F) as usize]
        } else {
            b'='
        } as char);
        out.push(if chunk.len() > 2 {
            BASE64_CHARS[(triple & 0x3F) as usize]
        } else {
            b'='
        } as char);
    }
    out
}

fn detect_image_mime(data: &[u8]) -> &'static str {
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        "image/jpeg"
    } else if data.len() >= 4
        && data[0] == 0x89
        && data[1] == 0x50
        && data[2] == 0x4E
        && data[3] == 0x47
    {
        "image/png"
    } else if data.len() >= 4
        && data[0] == 0x47
        && data[1] == 0x49
        && data[2] == 0x46
        && data[3] == 0x38
    {
        "image/gif"
    } else if data.len() >= 12
        && data[0] == 0x52
        && data[1] == 0x49
        && data[2] == 0x46
        && data[3] == 0x46
        && data[8] == 0x57
        && data[9] == 0x45
        && data[10] == 0x42
        && data[11] == 0x50
    {
        "image/webp"
    } else {
        "image/png"
    }
}

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
pub fn set_cache_storage_path(
    path: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SceneRuntimeSettingsSnapshot, String> {
    scene_runtime_settings_service::set_cache_storage_path(&app, &state, path)
}

#[tauri::command]
pub fn clear_scene_cache(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    scene_runtime_settings_service::clear_scene_cache(&app, &state)
}

#[tauri::command]
pub fn set_runtime_audio_output_volume(app: AppHandle, volume: f64) -> Result<(), String> {
    scene_native_renderer_service::set_scene_audio_output_volume(&app, volume)?;
    native_video_service::set_native_video_output_volume(&app, volume)?;
    native_web_service::set_native_web_output_volume(&app, volume)?;
    Ok(())
}

#[tauri::command]
pub fn get_scene_cache_size(app: AppHandle, state: State<'_, AppState>) -> Result<u64, String> {
    scene_runtime_settings_service::get_scene_cache_size(&app, &state)
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    if !matches!(url.split_once(":"), Some((scheme, _)) if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    {
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

#[tauri::command]
pub fn fetch_external_image(url: String) -> Result<String, String> {
    if !matches!(
        url.split_once(":"),
        Some((scheme, _)) if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    ) {
        return Err("only http and https URLs can be proxied".to_string());
    }

    let output = Command::new("curl")
        .args([
            "-sS",
            "-L",
            "--max-time",
            "15",
            "-A",
            "Mozilla/5.0 (compatible; ZestWallpaper/1.0)",
        ])
        .arg(&url)
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "curl request failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let mime = detect_image_mime(&output.stdout);
    let encoded = base64_encode(&output.stdout);
    Ok(format!("data:{mime};base64,{encoded}"))
}
