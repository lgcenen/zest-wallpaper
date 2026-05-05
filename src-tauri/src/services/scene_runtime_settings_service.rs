use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager};

use crate::{
    models::{SceneRuntimeSettings, SceneRuntimeSettingsSnapshot},
    store::{save_scene_runtime_settings, AppState},
};

use super::player_service;

pub fn snapshot(state: &AppState) -> Result<SceneRuntimeSettingsSnapshot, String> {
    let settings = state
        .scene_runtime_settings
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(snapshot_from_settings(&settings))
}

pub fn external_assets_root_for_app(app: &AppHandle) -> Option<PathBuf> {
    let state = app.try_state::<AppState>()?;
    let settings = state.scene_runtime_settings.lock().ok()?;
    settings.external_assets_path.as_deref().map(PathBuf::from)
}

pub fn persisted_external_assets_root() -> Option<PathBuf> {
    let path = crate::store::scene_runtime_settings_path().ok()?;
    let contents = fs::read_to_string(path).ok()?;
    let settings: SceneRuntimeSettings = serde_json::from_str(&contents).ok()?;
    settings
        .external_assets_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

pub fn set_external_assets_path(
    app: &AppHandle,
    state: &AppState,
    path: Option<String>,
) -> Result<SceneRuntimeSettingsSnapshot, String> {
    let normalized = validate_directory_path(path, "Scene external assets")?;
    let next_settings = {
        let current = state
            .scene_runtime_settings
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        if current.external_assets_path == normalized {
            return Ok(snapshot_from_settings(&current));
        }

        SceneRuntimeSettings {
            external_assets_path: normalized,
            cache_storage_path: current.cache_storage_path.clone(),
        }
    };

    save_scene_runtime_settings(&next_settings).map_err(|error| error.to_string())?;
    {
        let mut current = state
            .scene_runtime_settings
            .lock()
            .map_err(|error| error.to_string())?;
        *current = next_settings.clone();
    }

    let _ = player_service::sync_native_runtime_for_active_wallpaper(app, state);
    Ok(snapshot_from_settings(&next_settings))
}

pub fn set_cache_storage_path(
    _app: &AppHandle,
    state: &AppState,
    path: Option<String>,
) -> Result<SceneRuntimeSettingsSnapshot, String> {
    let normalized = validate_directory_path(path, "Cache storage")?;
    let next_settings = {
        let current = state
            .scene_runtime_settings
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        if current.cache_storage_path == normalized {
            return Ok(snapshot_from_settings(&current));
        }

        SceneRuntimeSettings {
            external_assets_path: current.external_assets_path.clone(),
            cache_storage_path: normalized,
        }
    };

    save_scene_runtime_settings(&next_settings).map_err(|error| error.to_string())?;
    {
        let mut current = state
            .scene_runtime_settings
            .lock()
            .map_err(|error| error.to_string())?;
        *current = next_settings.clone();
    }

    Ok(snapshot_from_settings(&next_settings))
}

pub fn clear_scene_cache(app: &AppHandle, state: &AppState) -> Result<(), String> {
    clear_scene_cache_with_app_cache(state, app.path().app_cache_dir().ok())
}

pub fn get_scene_cache_size(app: &AppHandle, state: &AppState) -> Result<u64, String> {
    get_scene_cache_size_with_app_cache(state, app.path().app_cache_dir().ok())
}

fn clear_scene_cache_with_app_cache(
    state: &AppState,
    app_cache_root: Option<PathBuf>,
) -> Result<(), String> {
    for cache_root in cache_roots(state, app_cache_root)? {
        clear_directory_contents(&cache_root)
            .map_err(|error| format!("Failed to clear cache {}: {error}", cache_root.display()))?;
    }
    Ok(())
}

fn get_scene_cache_size_with_app_cache(
    state: &AppState,
    app_cache_root: Option<PathBuf>,
) -> Result<u64, String> {
    let mut total: u64 = 0;
    for cache_root in cache_roots(state, app_cache_root)? {
        if cache_root.exists() {
            total += dir_size(&cache_root).unwrap_or(0);
        }
    }
    Ok(total)
}

fn cache_roots(state: &AppState, app_cache_root: Option<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let library_cache_roots = {
        let library = state.library.lock().map_err(|error| error.to_string())?;
        library
            .wallpapers
            .iter()
            .map(|record| PathBuf::from(&record.managed_path).join("cache"))
            .collect::<Vec<_>>()
    };

    let settings = state
        .scene_runtime_settings
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    let mut roots = BTreeSet::new();
    roots.extend(library_cache_roots);
    if let Some(path) = settings.cache_storage_path {
        roots.insert(PathBuf::from(path));
    }
    if let Some(path) = app_cache_root {
        roots.insert(path);
    }

    Ok(roots.into_iter().collect())
}

fn clear_directory_contents(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_file() {
        fs::remove_file(path)?;
        return Ok(());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry_path)?;
        } else {
            fs::remove_file(entry_path)?;
        }
    }

    Ok(())
}

fn dir_size(path: &Path) -> Result<u64, std::io::Error> {
    let mut total: u64 = 0;
    let entries = fs::read_dir(path)?;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            total += dir_size(&entry.path())?;
        } else if file_type.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

fn snapshot_from_settings(settings: &SceneRuntimeSettings) -> SceneRuntimeSettingsSnapshot {
    let external_assets_exists = settings
        .external_assets_path
        .as_deref()
        .map(Path::new)
        .map(Path::is_dir)
        .unwrap_or(false);

    let cache_storage_exists = settings
        .cache_storage_path
        .as_deref()
        .map(Path::new)
        .map(Path::is_dir)
        .unwrap_or(false);

    SceneRuntimeSettingsSnapshot {
        external_assets_path: settings.external_assets_path.clone(),
        external_assets_exists,
        cache_storage_path: settings.cache_storage_path.clone(),
        cache_storage_exists,
    }
}

fn validate_directory_path(path: Option<String>, label: &str) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };

    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let normalized = PathBuf::from(trimmed);
    if !normalized.is_absolute() {
        return Err(format!("{label} path must be absolute."));
    }
    if !normalized.exists() {
        return Err(format!(
            "{label} path {} does not exist.",
            normalized.display()
        ));
    }
    if !normalized.is_dir() {
        return Err(format!(
            "{label} path {} is not a directory.",
            normalized.display()
        ));
    }
    if is_overbroad_directory_path(&normalized, home_directory().as_deref()) {
        return Err(format!(
            "{label} path {} is too broad. Choose a specific subdirectory instead.",
            normalized.display()
        ));
    }

    Ok(Some(normalized.display().to_string()))
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn is_overbroad_directory_path(path: &Path, home_dir: Option<&Path>) -> bool {
    if path == Path::new("/") || path == Path::new("/Volumes") {
        return true;
    }

    if path.parent() == Some(Path::new("/Volumes")) {
        return true;
    }

    if let Some(home_dir) = home_dir {
        if path == home_dir
            || path == home_dir.join("Library")
            || path == home_dir.join("Library").join("Application Support")
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::Path,
        sync::Mutex,
    };

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::{
        models::{
            LibraryStore, PropertyKind, PropertyPresentation, SceneRuntimeSettings,
            WallpaperProperty, WallpaperRecord, WallpaperType,
        },
        store::{AppState, DynamicPlayerState, StaticSnapshotSyncState, HOME_ENV_LOCK},
    };

    use super::{
        clear_scene_cache_with_app_cache, get_scene_cache_size_with_app_cache,
        is_overbroad_directory_path, persisted_external_assets_root, snapshot_from_settings,
    };

    #[test]
    fn snapshot_marks_missing_external_assets_path_as_unavailable() {
        let temp = tempdir().expect("temp dir");
        let missing = temp.path().join("missing-assets");
        let snapshot = snapshot_from_settings(&SceneRuntimeSettings {
            external_assets_path: Some(missing.display().to_string()),
            cache_storage_path: None,
        });

        assert_eq!(
            snapshot.external_assets_path.as_deref(),
            Some(missing.to_str().expect("missing path"))
        );
        assert!(!snapshot.external_assets_exists);
    }

    #[test]
    fn snapshot_marks_existing_external_assets_path_as_available() {
        let temp = tempdir().expect("temp dir");
        let external = temp.path().join("external-assets");
        fs::create_dir_all(&external).expect("external dir");

        let snapshot = snapshot_from_settings(&SceneRuntimeSettings {
            external_assets_path: Some(external.display().to_string()),
            cache_storage_path: None,
        });

        assert!(snapshot.external_assets_exists);
    }

    #[test]
    fn persisted_external_assets_root_reads_saved_valid_directory() {
        let _lock = HOME_ENV_LOCK.lock().expect("home lock");
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let external = temp.path().join("external-assets");
            fs::create_dir_all(&external).expect("external dir");
            crate::store::save_scene_runtime_settings(&SceneRuntimeSettings {
                external_assets_path: Some(external.display().to_string()),
                cache_storage_path: None,
            })
            .expect("save settings");

            assert_eq!(persisted_external_assets_root(), Some(external));
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    fn sample_record(managed_path: String) -> WallpaperRecord {
        WallpaperRecord {
            id: "scene".to_string(),
            title: "Scene".to_string(),
            wallpaper_type: WallpaperType::Scene,
            source_path: managed_path.clone(),
            managed_path,
            preview_path: None,
            entry_path: None,
            last_snapshot_path: None,
            property_schema: vec![WallpaperProperty {
                key: "enabled".to_string(),
                label: "Enabled".to_string(),
                markup: None,
                kind: PropertyKind::Bool,
                value: serde_json::json!(true),
                default_value: serde_json::json!(true),
                min: None,
                max: None,
                step: None,
                condition: None,
                order: None,
                presentation: PropertyPresentation::Control,
                options: vec![],
            }],
            property_sections: vec![],
            scene_cache: None,
            scene_manifest: None,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec![],
        }
    }

    fn app_state(record: WallpaperRecord, cache_storage_path: Option<String>) -> AppState {
        AppState {
            library: Mutex::new(LibraryStore {
                wallpapers: vec![record],
            }),
            player: Mutex::new(DynamicPlayerState::default()),
            static_snapshot_sync: Mutex::new(StaticSnapshotSyncState::default()),
            runtime_sync: Mutex::new(()),
            scene_runtime_settings: Mutex::new(SceneRuntimeSettings {
                external_assets_path: None,
                cache_storage_path,
            }),
        }
    }

    #[test]
    fn get_scene_cache_size_counts_record_custom_and_app_cache_roots() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let record_cache = managed.join("cache");
        fs::create_dir_all(&record_cache).expect("record cache dir");
        fs::write(record_cache.join("scene-manifest.json"), b"abc").expect("record cache file");

        let custom_cache = temp.path().join("custom-cache");
        fs::create_dir_all(&custom_cache).expect("custom cache dir");
        fs::write(custom_cache.join("custom.bin"), b"12345").expect("custom cache file");

        let app_cache = temp.path().join("app-cache");
        fs::create_dir_all(&app_cache).expect("app cache dir");
        fs::write(app_cache.join("runtime.tmp"), b"1234567").expect("app cache file");

        let state = app_state(
            sample_record(managed.display().to_string()),
            Some(custom_cache.display().to_string()),
        );

        let total =
            get_scene_cache_size_with_app_cache(&state, Some(app_cache)).expect("cache size");

        assert_eq!(total, 3 + 5 + 7);
    }

    #[test]
    fn clear_scene_cache_removes_contents_from_all_cache_roots() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let record_cache = managed.join("cache");
        fs::create_dir_all(record_cache.join("nested")).expect("record cache dir");
        fs::write(
            record_cache.join("nested").join("scene-manifest.json"),
            b"abc",
        )
        .expect("record cache file");

        let custom_cache = temp.path().join("custom-cache");
        fs::create_dir_all(custom_cache.join("sub")).expect("custom cache dir");
        fs::write(custom_cache.join("sub").join("custom.bin"), b"12345")
            .expect("custom cache file");

        let app_cache = temp.path().join("app-cache");
        fs::create_dir_all(app_cache.join("more")).expect("app cache dir");
        fs::write(app_cache.join("more").join("runtime.tmp"), b"1234567").expect("app cache file");

        let state = app_state(
            sample_record(managed.display().to_string()),
            Some(custom_cache.display().to_string()),
        );

        clear_scene_cache_with_app_cache(&state, Some(app_cache.clone())).expect("clear cache");

        assert!(record_cache.exists());
        assert!(custom_cache.exists());
        assert!(app_cache.exists());
        assert_eq!(
            get_scene_cache_size_with_app_cache(&state, Some(app_cache)).expect("cache size"),
            0
        );
    }

    #[test]
    fn broad_directory_guard_rejects_root_mount_and_home_containers() {
        let home = Path::new("/Users/tester");

        assert!(is_overbroad_directory_path(Path::new("/"), Some(home)));
        assert!(is_overbroad_directory_path(Path::new("/Volumes"), Some(home)));
        assert!(is_overbroad_directory_path(
            Path::new("/Volumes/ExternalDisk"),
            Some(home)
        ));
        assert!(is_overbroad_directory_path(home, Some(home)));
        assert!(is_overbroad_directory_path(
            &home.join("Library/Application Support"),
            Some(home)
        ));
        assert!(!is_overbroad_directory_path(
            Path::new("/Volumes/ExternalDisk/WallpaperAssets"),
            Some(home)
        ));
        assert!(!is_overbroad_directory_path(
            &home.join("Documents/WallpaperAssets"),
            Some(home)
        ));
    }
}
