use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    models::{SceneCacheMetadata, SceneManifest, WallpaperRecord, WallpaperType},
    services::asset_resolver::AssetResolver,
};

pub const SCENE_PARSER_REVISION: &str = "scene-parser:2026-04-26-2";
pub const SCENE_EVALUATOR_REVISION: &str = "scene-evaluator:2026-04-26-2";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SceneCacheFile {
    source_fingerprint: String,
    parser_revision: String,
    evaluator_revision: String,
    updated_at: DateTime<Utc>,
    manifest: SceneManifest,
}

pub fn scene_cache_path_for_record(record: &WallpaperRecord) -> PathBuf {
    PathBuf::from(&record.managed_path)
        .join("cache")
        .join("scene-manifest.json")
}

pub fn load_scene_manifest(record: &WallpaperRecord) -> Option<SceneManifest> {
    load_scene_cache_file(record)
        .ok()
        .flatten()
        .map(|file| file.manifest)
        .or_else(|| record.scene_manifest.clone())
}

pub fn refresh_cache_for_record(
    record: &mut WallpaperRecord,
    manifest: Option<SceneManifest>,
) -> Result<bool> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
        record.scene_cache = None;
        record.scene_manifest = None;
        record.scene_manifest_version = None;
        record.scene_manifest_dirty = false;
        return Ok(false);
    }

    let Some(manifest) = manifest else {
        record.scene_cache = None;
        record.scene_manifest = None;
        record.scene_manifest_version = None;
        record.scene_manifest_dirty = false;
        let cache_path = scene_cache_path_for_record(record);
        if cache_path.exists() {
            let _ = fs::remove_file(cache_path);
        }
        return Ok(true);
    };

    let source_fingerprint = compute_source_fingerprint(record)?;
    let cache_file = SceneCacheFile {
        source_fingerprint: source_fingerprint.clone(),
        parser_revision: SCENE_PARSER_REVISION.to_string(),
        evaluator_revision: SCENE_EVALUATOR_REVISION.to_string(),
        updated_at: Utc::now(),
        manifest,
    };

    let cache_path = scene_cache_path_for_record(record);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cache_path, serde_json::to_vec_pretty(&cache_file)?)?;

    let metadata = SceneCacheMetadata {
        source_fingerprint,
        parser_revision: cache_file.parser_revision.clone(),
        evaluator_revision: cache_file.evaluator_revision.clone(),
        updated_at: cache_file.updated_at,
    };

    let changed = record.scene_cache.as_ref() != Some(&metadata)
        || record.scene_manifest.is_some()
        || record.scene_manifest_version.is_some()
        || record.scene_manifest_dirty;

    record.scene_cache = Some(metadata);
    record.scene_manifest = None;
    record.scene_manifest_version = None;
    record.scene_manifest_dirty = false;
    Ok(changed)
}

pub fn record_needs_cache_refresh(record: &WallpaperRecord) -> Result<bool> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
        return Ok(false);
    }

    let cache_path = scene_cache_path_for_record(record);
    if !cache_path.exists() {
        return Ok(true);
    }

    let Some(metadata) = record.scene_cache.as_ref() else {
        return Ok(true);
    };

    let current_fingerprint = compute_source_fingerprint(record)?;
    Ok(metadata.source_fingerprint != current_fingerprint
        || metadata.parser_revision != SCENE_PARSER_REVISION
        || metadata.evaluator_revision != SCENE_EVALUATOR_REVISION)
}

fn load_scene_cache_file(record: &WallpaperRecord) -> Result<Option<SceneCacheFile>> {
    let path = scene_cache_path_for_record(record);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    let cache_file: SceneCacheFile = serde_json::from_str(&raw)?;
    Ok(Some(cache_file))
}

fn compute_source_fingerprint(record: &WallpaperRecord) -> Result<String> {
    let resolver = AssetResolver::for_record(record);
    let mut hasher = Sha256::new();

    hash_optional_file(&mut hasher, resolver.source_root().join("project.json"))?;
    if let Some(scene_json_path) = resolver.scene_json_path() {
        hash_optional_file(&mut hasher, scene_json_path)?;
    } else {
        hasher.update(b"scene.json:missing");
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_optional_file(hasher: &mut Sha256, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    hasher.update(path.display().to_string().as_bytes());
    if !path.exists() {
        hasher.update(b":missing");
        return Ok(());
    }
    let contents = fs::read(path)
        .with_context(|| format!("Unable to read cache fingerprint input {}", path.display()))?;
    hasher.update(b":present:");
    hasher.update(contents);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::models::{PropertyKind, PropertyPresentation, SceneManifest, WallpaperProperty};

    use super::{
        load_scene_manifest, record_needs_cache_refresh, refresh_cache_for_record,
        scene_cache_path_for_record,
    };

    fn sample_scene_record(managed_path: String) -> crate::models::WallpaperRecord {
        crate::models::WallpaperRecord {
            id: "scene".to_string(),
            title: "Scene".to_string(),
            wallpaper_type: crate::models::WallpaperType::Scene,
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
    fn persists_and_loads_independent_scene_cache_file() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("source")).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(managed.join("source").join("project.json"), "{}").unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();

        assert!(scene_cache_path_for_record(&record).exists());
        assert!(record.scene_cache.is_some());
        assert!(record.scene_manifest.is_none());
        assert!(load_scene_manifest(&record).is_some());
    }

    #[test]
    fn cache_refresh_detects_source_fingerprint_changes() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("source")).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(managed.join("source").join("project.json"), "{\"a\":1}").unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());

        fs::write(managed.join("source").join("project.json"), "{\"a\":2}").unwrap();
        assert!(record_needs_cache_refresh(&record).unwrap());
    }
}
