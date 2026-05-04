use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    importer::refresh_record_metadata,
    models::{LibraryStore, SceneRuntimeSettings, WallpaperRecord},
};

#[cfg(test)]
pub(crate) static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default)]
pub struct DynamicPlayerState {
    pub active_id: Option<String>,
    pub manually_paused: bool,
    pub auto_pause_screen_labels: BTreeSet<String>,
    pub scene_update_generation: u64,
    pub last_scene_signature: Option<String>,
}

impl DynamicPlayerState {
    pub fn auto_paused(&self) -> bool {
        !self.auto_pause_screen_labels.is_empty()
    }

    pub fn effective_paused(&self) -> bool {
        self.manually_paused || self.auto_paused()
    }

    pub fn effective_runtime_paused_for_labels(&self, labels: &BTreeSet<String>) -> bool {
        if self.manually_paused {
            return true;
        }
        if labels.is_empty() {
            return self.auto_paused();
        }
        labels
            .iter()
            .all(|label| self.auto_pause_screen_labels.contains(label))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StaticSnapshotSyncState {
    #[serde(default)]
    pub active_record_id: Option<String>,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub last_applied_snapshot_path: Option<String>,
    #[serde(default)]
    pub last_failure_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct PersistedPlayerState {
    #[serde(default)]
    active_id: Option<String>,
    #[serde(default)]
    manually_paused: bool,
}

pub struct AppState {
    pub library: Mutex<LibraryStore>,
    pub player: Mutex<DynamicPlayerState>,
    pub static_snapshot_sync: Mutex<StaticSnapshotSyncState>,
    pub runtime_sync: Mutex<()>,
    pub scene_runtime_settings: Mutex<SceneRuntimeSettings>,
}

impl AppState {
    pub fn load() -> Result<Self> {
        let library = load_library()?;
        let mut player = load_player_state().unwrap_or_default();
        let mut static_snapshot_sync = load_static_snapshot_sync_state().unwrap_or_default();
        let scene_runtime_settings = load_scene_runtime_settings().unwrap_or_default();

        let active_is_valid = player
            .active_id
            .as_deref()
            .map(|id| find_record(&library, id).is_some())
            .unwrap_or(true);
        if !active_is_valid {
            player.active_id = None;
            player.manually_paused = false;
            let _ = save_player_state(&player);
        }
        if static_snapshot_sync
            .active_record_id
            .as_deref()
            .map(|id| {
                player.active_id.as_deref() != Some(id) || find_record(&library, id).is_none()
            })
            .unwrap_or(false)
        {
            static_snapshot_sync.active_record_id = None;
            static_snapshot_sync.last_failure_code = None;
            let _ = save_static_snapshot_sync_state(&static_snapshot_sync);
        }

        Ok(Self {
            library: Mutex::new(library),
            player: Mutex::new(player),
            static_snapshot_sync: Mutex::new(static_snapshot_sync),
            runtime_sync: Mutex::new(()),
            scene_runtime_settings: Mutex::new(scene_runtime_settings),
        })
    }
}

pub fn app_support_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").context("HOME is not set")?;
        let root = PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("WallpaperWorkbench");
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let root = std::env::current_dir()?.join(".wallpaper-workbench");
        fs::create_dir_all(&root)?;
        Ok(root)
    }
}

pub fn library_path() -> Result<PathBuf> {
    Ok(app_support_dir()?.join("library.json"))
}

pub fn player_state_path() -> Result<PathBuf> {
    Ok(app_support_dir()?.join("player-state.json"))
}

pub fn scene_runtime_settings_path() -> Result<PathBuf> {
    Ok(app_support_dir()?.join("scene-runtime-settings.json"))
}

pub fn static_snapshot_sync_state_path() -> Result<PathBuf> {
    Ok(app_support_dir()?.join("static-snapshot-sync-state.json"))
}

pub fn library_root_dir() -> Result<PathBuf> {
    let path = app_support_dir()?.join("wallpapers");
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn wallpaper_dir(id: &str) -> Result<PathBuf> {
    let path = library_root_dir()?.join(id);
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn load_library() -> Result<LibraryStore> {
    let path = library_path()?;
    if !path.exists() {
        return Ok(LibraryStore::default());
    }
    let contents = fs::read_to_string(path)?;
    let mut store: LibraryStore = serde_json::from_str(&contents)?;
    let mut changed = false;
    for record in &mut store.wallpapers {
        if record_needs_metadata_refresh(record) {
            if let Ok(did_refresh) = refresh_record_metadata(record) {
                changed |= did_refresh;
            }
        }
    }
    if changed {
        save_library(&store)?;
    }
    Ok(store)
}

fn record_needs_metadata_refresh(record: &WallpaperRecord) -> bool {
    if record.title.trim().is_empty() {
        return true;
    }

    if record.entry_path.is_none() && record.preview_path.is_none() {
        return true;
    }

    if record.property_schema.is_empty() && record.property_sections.is_empty() {
        return true;
    }

    false
}

pub fn save_library(store: &LibraryStore) -> Result<()> {
    let path = library_path()?;
    let serialized = serde_json::to_string_pretty(store)?;
    fs::write(path, serialized)?;
    Ok(())
}

pub fn save_player_state(player: &DynamicPlayerState) -> Result<()> {
    let path = player_state_path()?;
    let serialized = serde_json::to_string_pretty(&persisted_player_state(player))?;
    fs::write(path, serialized)?;
    Ok(())
}

pub fn save_scene_runtime_settings(settings: &SceneRuntimeSettings) -> Result<()> {
    let path = scene_runtime_settings_path()?;
    let serialized =
        serde_json::to_string_pretty(&normalized_scene_runtime_settings(settings.clone()))?;
    fs::write(path, serialized)?;
    Ok(())
}

pub fn save_static_snapshot_sync_state(state: &StaticSnapshotSyncState) -> Result<()> {
    let path = static_snapshot_sync_state_path()?;
    let serialized = serde_json::to_string_pretty(state)?;
    fs::write(path, serialized)?;
    Ok(())
}

pub fn find_record(store: &LibraryStore, id: &str) -> Option<WallpaperRecord> {
    store
        .wallpapers
        .iter()
        .find(|record| record.id == id)
        .cloned()
}

pub fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn load_player_state() -> Result<DynamicPlayerState> {
    let path = player_state_path()?;
    if !path.exists() {
        return Ok(DynamicPlayerState::default());
    }

    let contents = fs::read_to_string(path)?;
    let persisted: PersistedPlayerState = serde_json::from_str(&contents)?;
    Ok(dynamic_player_state(persisted))
}

fn load_scene_runtime_settings() -> Result<SceneRuntimeSettings> {
    let path = scene_runtime_settings_path()?;
    if !path.exists() {
        return Ok(SceneRuntimeSettings::default());
    }

    let contents = fs::read_to_string(path)?;
    let settings: SceneRuntimeSettings = serde_json::from_str(&contents)?;
    Ok(normalized_scene_runtime_settings(settings))
}

fn load_static_snapshot_sync_state() -> Result<StaticSnapshotSyncState> {
    let path = static_snapshot_sync_state_path()?;
    if !path.exists() {
        return Ok(StaticSnapshotSyncState::default());
    }

    let contents = fs::read_to_string(path)?;
    serde_json::from_str(&contents).map_err(Into::into)
}

fn dynamic_player_state(persisted: PersistedPlayerState) -> DynamicPlayerState {
    DynamicPlayerState {
        active_id: persisted.active_id.clone(),
        manually_paused: persisted.active_id.is_some() && persisted.manually_paused,
        auto_pause_screen_labels: BTreeSet::new(),
        scene_update_generation: 0,
        last_scene_signature: None,
    }
}

fn persisted_player_state(player: &DynamicPlayerState) -> PersistedPlayerState {
    PersistedPlayerState {
        active_id: player.active_id.clone(),
        manually_paused: player.active_id.is_some() && player.manually_paused,
    }
}

fn normalized_scene_runtime_settings(mut settings: SceneRuntimeSettings) -> SceneRuntimeSettings {
    settings.external_assets_path = settings.external_assets_path.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    settings.cache_storage_path = settings.cache_storage_path.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    settings
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, env, fs};

    use tempfile::tempdir;

    use crate::models::{PropertySection, SceneRuntimeSettings, WallpaperRecord, WallpaperType};

    use super::{
        app_support_dir, load_library, load_player_state, save_library, save_player_state,
        save_scene_runtime_settings, save_static_snapshot_sync_state, scene_runtime_settings_path,
        static_snapshot_sync_state_path, AppState, DynamicPlayerState, StaticSnapshotSyncState,
    };

    #[test]
    fn load_and_save_library_preserves_active_wallpaper_snapshot_cache() {
        let _lock = super::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let support_dir = app_support_dir().unwrap();
            fs::create_dir_all(&support_dir).unwrap();
            fs::write(
                support_dir.join("library.json"),
                r#"{
                  "wallpapers": [
                    {
                      "id": "legacy",
                      "title": "Legacy",
                      "wallpaperType": "video",
                      "sourcePath": "/tmp/source",
                      "managedPath": "/tmp/managed",
                      "previewPath": "/tmp/preview.png",
                      "entryPath": "/tmp/entry.mp4",
                      "propertySchema": [],
                      "propertySections": [],
                      "lastSnapshotPath": "/tmp/old.png",
                      "importedAt": "2024-01-01T00:00:00Z",
                      "tags": []
                    }
                  ]
                }"#,
            )
            .unwrap();

            let store = load_library().unwrap();
            assert_eq!(store.wallpapers.len(), 1);
            assert_eq!(store.wallpapers[0].id, "legacy");
            assert_eq!(
                store.wallpapers[0].preview_path.as_deref(),
                Some("/tmp/preview.png")
            );
            assert_eq!(
                store.wallpapers[0].last_snapshot_path.as_deref(),
                Some("/tmp/old.png")
            );

            save_library(&store).unwrap();
            let saved = fs::read_to_string(support_dir.join("library.json")).unwrap();
            assert!(saved.contains("\"lastSnapshotPath\": \"/tmp/old.png\""));
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn missing_snapshot_does_not_force_startup_metadata_refresh() {
        let temp = tempdir().unwrap();
        let entry_path = temp.path().join("clip.mp4");
        let preview_path = temp.path().join("preview.png");
        fs::write(&entry_path, b"video").unwrap();
        fs::write(&preview_path, b"preview").unwrap();
        let mut record = WallpaperRecord {
            id: "video".to_string(),
            title: "Video".to_string(),
            wallpaper_type: WallpaperType::Video,
            source_path: temp.path().display().to_string(),
            managed_path: temp.path().display().to_string(),
            preview_path: Some(preview_path.display().to_string()),
            entry_path: Some(entry_path.display().to_string()),
            last_snapshot_path: None,
            property_schema: Vec::new(),
            property_sections: vec![PropertySection {
                key: "general".to_string(),
                label: "General".to_string(),
                order: None,
                condition: None,
                items: Vec::new(),
            }],
            scene_cache: None,
            scene_manifest: None,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: chrono::Utc::now(),
            tags: Vec::new(),
        };

        assert!(!super::record_needs_metadata_refresh(&record));

        let snapshot_path = temp.path().join("snapshot.png");
        fs::write(&snapshot_path, b"snapshot").unwrap();
        record.last_snapshot_path = Some(snapshot_path.display().to_string());

        assert!(!super::record_needs_metadata_refresh(&record));

        record.wallpaper_type = WallpaperType::Scene;
        record.last_snapshot_path = None;
        assert!(!super::record_needs_metadata_refresh(&record));

        record.wallpaper_type = WallpaperType::Web;
        assert!(!super::record_needs_metadata_refresh(&record));
    }

    #[test]
    fn load_and_save_player_state_round_trips_manual_pause_and_ignores_unknown_fields() {
        let _lock = super::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let player = DynamicPlayerState {
                active_id: Some("demo".to_string()),
                manually_paused: true,
                auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
                scene_update_generation: 2,
                last_scene_signature: Some("scene".to_string()),
            };
            save_player_state(&player).unwrap();

            let support_dir = app_support_dir().unwrap();
            fs::write(
                support_dir.join("player-state.json"),
                r#"{
                  "activeId": "demo",
                  "manuallyPaused": true,
                  "autoPaused": true,
                  "unexpected": "ignored"
                }"#,
            )
            .unwrap();

            let restored = load_player_state().unwrap();
            assert_eq!(restored.active_id.as_deref(), Some("demo"));
            assert!(restored.manually_paused);
            assert!(restored.auto_pause_screen_labels.is_empty());
            assert_eq!(restored.scene_update_generation, 0);
            assert!(restored.last_scene_signature.is_none());
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn runtime_pause_only_collapses_auto_pause_when_all_labels_are_covered() {
        let mut player = DynamicPlayerState {
            active_id: Some("demo".to_string()),
            manually_paused: false,
            auto_pause_screen_labels: BTreeSet::from([String::from("player-screen-1")]),
            scene_update_generation: 0,
            last_scene_signature: None,
        };
        let labels = BTreeSet::from([String::from("player"), String::from("player-screen-1")]);

        assert!(player.effective_paused());
        assert!(!player.effective_runtime_paused_for_labels(&labels));

        player
            .auto_pause_screen_labels
            .insert(String::from("player"));
        assert!(player.effective_runtime_paused_for_labels(&labels));

        player.manually_paused = true;
        player.auto_pause_screen_labels.clear();
        assert!(player.effective_runtime_paused_for_labels(&labels));
    }

    #[test]
    fn load_and_save_static_snapshot_sync_state_round_trips_active_snapshot_state() {
        let _lock = super::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let sync_state = StaticSnapshotSyncState {
                active_record_id: Some("demo".to_string()),
                generation: 9,
                last_applied_snapshot_path: Some("/portable/snapshot.png".to_string()),
                last_failure_code: Some("snapshot-unavailable".to_string()),
            };

            save_static_snapshot_sync_state(&sync_state).unwrap();
            let restored = super::load_static_snapshot_sync_state().unwrap();

            assert_eq!(restored, sync_state);
            let saved = fs::read_to_string(static_snapshot_sync_state_path().unwrap()).unwrap();
            assert!(saved.contains("\"activeRecordId\": \"demo\""));
            assert!(saved.contains("\"lastAppliedSnapshotPath\": \"/portable/snapshot.png\""));
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn app_state_load_restores_static_snapshot_sync_state_for_valid_active_wallpaper() {
        let _lock = super::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let support_dir = app_support_dir().unwrap();
            fs::create_dir_all(&support_dir).unwrap();
            fs::write(
                support_dir.join("library.json"),
                r#"{
                  "wallpapers": [
                    {
                      "id": "demo",
                      "title": "Demo",
                      "wallpaperType": "video",
                      "sourcePath": "/tmp/source",
                      "managedPath": "/tmp/managed",
                      "previewPath": "/tmp/preview.png",
                      "entryPath": "/tmp/entry.mp4",
                      "propertySchema": [],
                      "propertySections": [],
                      "importedAt": "2024-01-01T00:00:00Z",
                      "tags": []
                    }
                  ]
                }"#,
            )
            .unwrap();
            fs::write(
                support_dir.join("player-state.json"),
                r#"{ "activeId": "demo", "manuallyPaused": false }"#,
            )
            .unwrap();
            save_static_snapshot_sync_state(&StaticSnapshotSyncState {
                active_record_id: Some("demo".to_string()),
                generation: 3,
                last_applied_snapshot_path: Some("/portable/snapshot.png".to_string()),
                last_failure_code: None,
            })
            .unwrap();

            let state = AppState::load().unwrap();
            let sync_state = state.static_snapshot_sync.lock().unwrap().clone();

            assert_eq!(sync_state.active_record_id.as_deref(), Some("demo"));
            assert_eq!(sync_state.generation, 3);
            assert_eq!(
                sync_state.last_applied_snapshot_path.as_deref(),
                Some("/portable/snapshot.png")
            );
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn app_state_load_clears_stale_persisted_player_state() {
        let _lock = super::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let support_dir = app_support_dir().unwrap();
            fs::create_dir_all(&support_dir).unwrap();
            fs::write(support_dir.join("library.json"), r#"{ "wallpapers": [] }"#).unwrap();
            fs::write(
                support_dir.join("player-state.json"),
                r#"{
                  "activeId": "missing",
                  "manuallyPaused": true
                }"#,
            )
            .unwrap();

            let state = AppState::load().unwrap();
            let player = state.player.lock().unwrap().clone();
            assert!(player.active_id.is_none());
            assert!(!player.manually_paused);
            assert!(player.auto_pause_screen_labels.is_empty());
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }

    #[test]
    fn app_state_load_restores_scene_runtime_settings() {
        let _lock = super::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().unwrap();
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path());

        let result = (|| {
            let external_assets_path = temp.path().join("external-assets");
            fs::create_dir_all(&external_assets_path).unwrap();

            save_scene_runtime_settings(&SceneRuntimeSettings {
                external_assets_path: Some(external_assets_path.display().to_string()),
                cache_storage_path: None,
            })
            .unwrap();

            let saved = fs::read_to_string(scene_runtime_settings_path().unwrap()).unwrap();
            assert!(saved.contains("externalAssetsPath"));

            let state = AppState::load().unwrap();
            let settings = state.scene_runtime_settings.lock().unwrap().clone();
            assert_eq!(
                settings.external_assets_path.as_deref(),
                Some(external_assets_path.to_str().unwrap())
            );
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        result
    }
}
