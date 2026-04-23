use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::{
    importer::{import_wallpaper_path, update_property_values},
    models::{LibraryStore, WallpaperRecord, WallpaperRuntimeRecord},
    services::{player_service, runtime_document_service, scene_manifest_service},
    store::{remove_dir_if_exists, save_library, AppState},
};

fn persist_library_snapshot(store: &LibraryStore) -> Result<(), String> {
    save_library(store).map_err(|error| error.to_string())
}

fn remove_wallpaper_from_store<F>(
    store: &mut LibraryStore,
    id: &str,
    persist_library: F,
) -> Result<WallpaperRecord, String>
where
    F: Fn(&LibraryStore) -> Result<(), String>,
{
    let index = store
        .wallpapers
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| format!("Wallpaper {id} was not found"))?;
    let mut candidate_store = store.clone();
    let removed_record = candidate_store.wallpapers.remove(index);
    let managed_path = PathBuf::from(&removed_record.managed_path);
    let staged_managed_path = stage_managed_path_for_removal(&managed_path)?;

    if let Err(error) = persist_library(&candidate_store) {
        return match rollback_staged_managed_path(&managed_path, staged_managed_path.as_deref()) {
            Ok(()) => Err(format!("Failed to persist wallpaper removal for {id}: {error}")),
            Err(rollback_error) => Err(format!(
                "Failed to persist wallpaper removal for {id}: {error}. Rollback failed: {rollback_error}"
            )),
        };
    }

    if let Err(error) = discard_staged_managed_path(staged_managed_path.as_deref()) {
        return match rollback_committed_wallpaper_removal(
            store,
            &managed_path,
            staged_managed_path.as_deref(),
            &persist_library,
        ) {
            Ok(()) => Err(format!(
                "Failed to delete managed wallpaper files for {id}: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "Failed to delete managed wallpaper files for {id}: {error}. Rollback failed: {rollback_error}"
            )),
        };
    }

    *store = candidate_store;
    Ok(removed_record)
}

fn stage_managed_path_for_removal(managed_path: &Path) -> Result<Option<PathBuf>, String> {
    if !managed_path.exists() {
        return Ok(None);
    }

    let staged_path = next_staged_managed_path(managed_path);
    fs::rename(managed_path, &staged_path).map_err(|error| {
        format!(
            "Failed to stage managed wallpaper path {} for removal: {error}",
            managed_path.display()
        )
    })?;
    Ok(Some(staged_path))
}

fn discard_staged_managed_path(staged_path: Option<&Path>) -> Result<(), String> {
    let Some(staged_path) = staged_path else {
        return Ok(());
    };

    remove_dir_if_exists(staged_path).map_err(|error| {
        format!(
            "Failed to remove staged managed wallpaper path {}: {error}",
            staged_path.display()
        )
    })
}

fn rollback_staged_managed_path(
    managed_path: &Path,
    staged_path: Option<&Path>,
) -> Result<(), String> {
    let Some(staged_path) = staged_path else {
        return Ok(());
    };
    if !staged_path.exists() {
        return Ok(());
    }

    fs::rename(staged_path, managed_path).map_err(|error| {
        format!(
            "Failed to restore staged wallpaper path {} back to {}: {error}",
            staged_path.display(),
            managed_path.display()
        )
    })
}

fn rollback_committed_wallpaper_removal<F>(
    original_store: &LibraryStore,
    managed_path: &Path,
    staged_path: Option<&Path>,
    persist_library: &F,
) -> Result<(), String>
where
    F: Fn(&LibraryStore) -> Result<(), String>,
{
    let mut rollback_errors = Vec::new();

    if let Err(error) = rollback_staged_managed_path(managed_path, staged_path) {
        rollback_errors.push(error);
    }
    if let Err(error) = persist_library(original_store) {
        rollback_errors.push(format!("Failed to restore library.json: {error}"));
    }

    if rollback_errors.is_empty() {
        Ok(())
    } else {
        Err(rollback_errors.join("; "))
    }
}

fn next_staged_managed_path(managed_path: &Path) -> PathBuf {
    let parent = managed_path.parent().unwrap_or_else(|| Path::new("."));
    let name = managed_path
        .file_name()
        .map(|segment| segment.to_string_lossy().into_owned())
        .filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| "wallpaper".to_string());

    let mut attempt = 0usize;
    loop {
        let candidate = parent.join(format!(".{name}.removing-{attempt}"));
        if !candidate.exists() {
            return candidate;
        }
        attempt += 1;
    }
}

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
    let _removed_record = remove_wallpaper_from_store(&mut store, &id, persist_library_snapshot)?;
    drop(store);
    if removed_active_player {
        player_service::clear_active_wallpaper(&app, &state)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::{
        models::{LibraryStore, WallpaperRecord, WallpaperType},
        store::{app_support_dir, load_library, HOME_ENV_LOCK},
    };

    use super::{persist_library_snapshot, remove_wallpaper_from_store};

    fn wallpaper_record(id: &str, managed_path: &str) -> WallpaperRecord {
        WallpaperRecord {
            id: id.to_string(),
            title: format!("Wallpaper {id}"),
            wallpaper_type: WallpaperType::Video,
            source_path: format!("/source/{id}"),
            managed_path: managed_path.to_string(),
            preview_path: None,
            entry_path: None,
            property_schema: Vec::new(),
            property_sections: Vec::new(),
            scene_cache: None,
            scene_manifest: None,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn remove_wallpaper_commits_memory_disk_and_persistence_together() {
        let _lock = HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let managed_path = temp.path().join("managed").join("aurora");
            fs::create_dir_all(&managed_path).expect("managed dir");
            fs::write(managed_path.join("wallpaper.txt"), "content").expect("managed contents");

            let mut store = LibraryStore {
                wallpapers: vec![wallpaper_record(
                    "aurora",
                    managed_path.to_str().expect("managed path"),
                )],
            };
            persist_library_snapshot(&store).expect("initial library save");

            let removed =
                remove_wallpaper_from_store(&mut store, "aurora", persist_library_snapshot)
                    .expect("wallpaper removal");

            assert_eq!(removed.id, "aurora");
            assert!(store.wallpapers.is_empty());
            assert!(!managed_path.exists());

            let persisted = load_library().expect("load library");
            assert!(persisted.wallpapers.is_empty());
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn remove_wallpaper_save_failure_keeps_memory_persistence_and_files_aligned() {
        let _lock = HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let managed_path = temp.path().join("managed").join("aurora");
            fs::create_dir_all(&managed_path).expect("managed dir");
            fs::write(managed_path.join("wallpaper.txt"), "content").expect("managed contents");

            let original = wallpaper_record("aurora", managed_path.to_str().expect("managed path"));
            let mut store = LibraryStore {
                wallpapers: vec![original.clone()],
            };
            persist_library_snapshot(&store).expect("initial library save");

            let error = remove_wallpaper_from_store(&mut store, "aurora", |_| {
                Err("simulated library save failure".to_string())
            })
            .expect_err("save failure");

            assert!(error.contains("simulated library save failure"));
            assert_eq!(store.wallpapers.len(), 1);
            assert_eq!(store.wallpapers[0].id, original.id);
            assert!(managed_path.exists());
            assert!(fs::read_dir(managed_path.parent().expect("managed parent"))
                .expect("managed parent entries")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .contains(".removing-")));

            let persisted = load_library().expect("load library");
            assert_eq!(persisted.wallpapers.len(), 1);
            assert_eq!(persisted.wallpapers[0].id, original.id);
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn remove_wallpaper_delete_failure_rolls_back_library_and_memory() {
        let _lock = HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let support_dir = app_support_dir().expect("support dir");
            fs::create_dir_all(&support_dir).expect("support dir exists");

            let managed_path = temp.path().join("managed").join("aurora");
            fs::create_dir_all(managed_path.parent().expect("managed parent"))
                .expect("managed parent dir");
            fs::write(&managed_path, "not a directory").expect("managed path file");

            let original = wallpaper_record("aurora", managed_path.to_str().expect("managed path"));
            let mut store = LibraryStore {
                wallpapers: vec![original.clone()],
            };
            persist_library_snapshot(&store).expect("initial library save");

            let error = remove_wallpaper_from_store(&mut store, "aurora", persist_library_snapshot)
                .expect_err("delete failure");

            assert!(error.contains("Failed to delete managed wallpaper files"));
            assert_eq!(store.wallpapers.len(), 1);
            assert_eq!(store.wallpapers[0].id, original.id);
            assert!(managed_path.exists());
            assert!(managed_path.is_file());

            let persisted = load_library().expect("load library");
            assert_eq!(persisted.wallpapers.len(), 1);
            assert_eq!(persisted.wallpapers[0].id, original.id);
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }
}
