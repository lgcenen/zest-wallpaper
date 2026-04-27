use std::{
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
    let normalized = validate_external_assets_path(path)?;
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

fn snapshot_from_settings(settings: &SceneRuntimeSettings) -> SceneRuntimeSettingsSnapshot {
    let external_assets_exists = settings
        .external_assets_path
        .as_deref()
        .map(Path::new)
        .map(Path::is_dir)
        .unwrap_or(false);

    SceneRuntimeSettingsSnapshot {
        external_assets_path: settings.external_assets_path.clone(),
        external_assets_exists,
    }
}

fn validate_external_assets_path(path: Option<String>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };

    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let normalized = PathBuf::from(trimmed);
    if !normalized.is_absolute() {
        return Err("Scene external assets path must be absolute.".to_string());
    }
    if !normalized.exists() {
        return Err(format!(
            "Scene external assets path {} does not exist.",
            normalized.display()
        ));
    }
    if !normalized.is_dir() {
        return Err(format!(
            "Scene external assets path {} is not a directory.",
            normalized.display()
        ));
    }

    Ok(Some(normalized.display().to_string()))
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use tempfile::tempdir;

    use crate::{models::SceneRuntimeSettings, store::HOME_ENV_LOCK};

    use super::{persisted_external_assets_root, snapshot_from_settings};

    #[test]
    fn snapshot_marks_missing_external_assets_path_as_unavailable() {
        let temp = tempdir().expect("temp dir");
        let missing = temp.path().join("missing-assets");
        let snapshot = snapshot_from_settings(&SceneRuntimeSettings {
            external_assets_path: Some(missing.display().to_string()),
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
}
