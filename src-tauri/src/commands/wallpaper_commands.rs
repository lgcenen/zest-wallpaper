use std::{collections::BTreeMap, path::PathBuf};

use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::{
    importer::{import_wallpaper_path, update_property_values},
    models::WallpaperRuntimeRecord,
    services::{player_service, runtime_document_service, scene_manifest_service},
    store::{remove_dir_if_exists, save_library, AppState},
};

#[tauri::command]
pub fn list_wallpapers(state: State<'_, AppState>) -> Result<Vec<WallpaperRuntimeRecord>, String> {
    let store = state.library.lock().map_err(|error| error.to_string())?;
    let mut records = store.wallpapers.clone();
    records.sort_by(|left, right| right.imported_at.cmp(&left.imported_at));
    Ok(runtime_document_service::runtime_records(&records))
}

#[tauri::command]
pub fn import_wallpaper(
    path: String,
    state: State<'_, AppState>,
) -> Result<WallpaperRuntimeRecord, String> {
    let record = import_wallpaper_path(&PathBuf::from(path)).map_err(|error| error.to_string())?;
    let mut store = state.library.lock().map_err(|error| error.to_string())?;
    store.wallpapers.push(record.clone());
    save_library(&store).map_err(|error| error.to_string())?;
    Ok(runtime_document_service::runtime_record(&record))
}

#[tauri::command]
pub fn get_wallpaper_details(
    id: String,
    state: State<'_, AppState>,
) -> Result<WallpaperRuntimeRecord, String> {
    let record = scene_manifest_service::ensure_scene_manifest_current_by_id(&state, &id)?;
    Ok(runtime_document_service::runtime_record(&record))
}

#[tauri::command]
pub fn set_wallpaper_properties(
    id: String,
    values: BTreeMap<String, Value>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WallpaperRuntimeRecord, String> {
    let mut store = state.library.lock().map_err(|error| error.to_string())?;
    let record = store
        .wallpapers
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or_else(|| format!("Wallpaper {id} was not found"))?;

    scene_manifest_service::refresh_scene_manifest_for_record(record)?;
    update_property_values(record, &values).map_err(|error| error.to_string())?;
    let updated = record.clone();
    save_library(&store).map_err(|error| error.to_string())?;
    drop(store);
    let runtime_record = runtime_document_service::runtime_record(&updated);

    let should_emit = state
        .player
        .lock()
        .map_err(|error| error.to_string())?
        .active_id
        .as_deref()
        == Some(updated.id.as_str());
    if should_emit {
        player_service::sync_active_scene_signature(&runtime_record, &state)?;
        player_service::sync_native_runtime_for_active_wallpaper(&app, &state)?;
        app.emit("player:update", runtime_record.clone())
            .map_err(|error| error.to_string())?;
    }
    Ok(runtime_record)
}

#[tauri::command]
pub fn remove_wallpaper(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let removed_active_player = {
        let player = state.player.lock().map_err(|error| error.to_string())?;
        player.active_id.as_deref() == Some(id.as_str())
    };
    let mut store = state.library.lock().map_err(|error| error.to_string())?;
    let index = store
        .wallpapers
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| format!("Wallpaper {id} was not found"))?;
    let record = store.wallpapers.remove(index);
    remove_dir_if_exists(&PathBuf::from(&record.managed_path))
        .map_err(|error| error.to_string())?;
    save_library(&store).map_err(|error| error.to_string())?;
    drop(store);
    if removed_active_player {
        player_service::clear_active_wallpaper(&app, &state)?;
    }
    Ok(true)
}
