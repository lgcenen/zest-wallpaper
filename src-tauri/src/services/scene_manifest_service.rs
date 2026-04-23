use crate::{
    importer::refresh_record_metadata,
    models::{WallpaperRecord, WallpaperType},
    store::{save_library, AppState},
};

use super::scene_cache_service;

pub fn record_needs_scene_manifest_refresh(record: &WallpaperRecord) -> Result<bool, String> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
        return Ok(false);
    }
    scene_cache_service::record_needs_cache_refresh(record).map_err(|error| error.to_string())
}

pub fn refresh_scene_manifest_for_record(record: &mut WallpaperRecord) -> Result<bool, String> {
    if !record_needs_scene_manifest_refresh(record)? {
        return Ok(false);
    }
    refresh_record_metadata(record).map_err(|error| error.to_string())
}

pub fn ensure_scene_manifest_current_by_id(
    state: &AppState,
    id: &str,
) -> Result<WallpaperRecord, String> {
    let mut store = state.library.lock().map_err(|error| error.to_string())?;
    let record = store
        .wallpapers
        .iter_mut()
        .find(|record| record.id == id)
        .ok_or_else(|| format!("Wallpaper {id} was not found"))?;

    let changed = refresh_scene_manifest_for_record(record)?;
    let cloned = record.clone();
    if changed {
        save_library(&store).map_err(|error| error.to_string())?;
    }
    Ok(cloned)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::models::{
        PropertyKind, PropertyPresentation, SceneManifest, WallpaperProperty, WallpaperRecord,
        WallpaperType,
    };

    use super::{record_needs_scene_manifest_refresh, refresh_scene_manifest_for_record};

    fn sample_scene_record(managed_path: String) -> WallpaperRecord {
        WallpaperRecord {
            id: "scene".to_string(),
            title: "Scene".to_string(),
            wallpaper_type: WallpaperType::Scene,
            source_path: managed_path.clone(),
            managed_path,
            preview_path: None,
            entry_path: None,
            property_schema: vec![WallpaperProperty {
                key: "enabled".to_string(),
                label: "Enabled".to_string(),
                markup: None,
                kind: PropertyKind::Bool,
                value: json!(true),
                default_value: json!(true),
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
            scene_manifest: Some(SceneManifest::default()),
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec![],
        }
    }

    #[test]
    fn marks_scene_without_cache_metadata_for_refresh() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("source")).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(managed.join("source").join("project.json"), "{}").unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let record = sample_scene_record(managed.display().to_string());
        assert!(record_needs_scene_manifest_refresh(&record).unwrap());
    }

    #[test]
    fn refresh_uses_content_fingerprint_and_independent_cache() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("source")).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(
            managed.join("source").join("project.json"),
            r#"{"title":"Scene","type":"scene"}"#,
        )
        .unwrap();
        fs::write(
            managed.join("extracted").join("scene.json"),
            r#"{"camera":{"zoom":1},"objects":[]}"#,
        )
        .unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        refresh_scene_manifest_for_record(&mut record).unwrap();
        assert!(record.scene_cache.is_some());
        assert!(record.scene_manifest.is_none());
        assert!(!record_needs_scene_manifest_refresh(&record).unwrap());

        fs::write(
            managed.join("source").join("project.json"),
            r#"{"title":"Updated","type":"scene"}"#,
        )
        .unwrap();
        assert!(record_needs_scene_manifest_refresh(&record).unwrap());
    }
}
