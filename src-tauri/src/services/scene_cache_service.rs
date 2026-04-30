use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    models::{SceneCacheMetadata, SceneManifest, WallpaperRecord, WallpaperType},
    services::asset_resolver::AssetResolver,
};

pub const SCENE_PARSER_REVISION: &str = "scene-parser:2026-04-30-2";
pub const SCENE_EVALUATOR_REVISION: &str = "scene-evaluator:2026-05-01-1";

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
        .filter(|file| record.scene_cache.as_ref() == Some(&cache_metadata_from_file(file)))
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
    if metadata.source_fingerprint != current_fingerprint
        || metadata.parser_revision != SCENE_PARSER_REVISION
        || metadata.evaluator_revision != SCENE_EVALUATOR_REVISION
    {
        return Ok(true);
    }

    let cache_file = match load_scene_cache_file(record) {
        Ok(Some(file)) => file,
        Ok(None) | Err(_) => return Ok(true),
    };
    Ok(cache_metadata_from_file(&cache_file) != *metadata)
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
    compute_source_fingerprint_with_baseline_engine(record, &phase_08_baseline_engine_fingerprint())
}

fn compute_source_fingerprint_with_baseline_engine(
    record: &WallpaperRecord,
    baseline_engine_fingerprint: &str,
) -> Result<String> {
    let resolver = AssetResolver::for_record(record);
    let mut hasher = Sha256::new();

    hash_labeled_bytes(
        &mut hasher,
        "phase-08-baseline-engine",
        baseline_engine_fingerprint.as_bytes(),
    );
    hash_project_metadata_file(&mut hasher, resolver.source_root().join("project.json"))?;
    if let Some(scene_json_path) = resolver.scene_json_path() {
        hash_optional_file(&mut hasher, &scene_json_path)?;
        hash_scene_baseline_dependencies(&mut hasher, resolver.extracted_root(), &scene_json_path)?;
    } else {
        hasher.update(b"scene.json:missing");
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn cache_metadata_from_file(file: &SceneCacheFile) -> SceneCacheMetadata {
    SceneCacheMetadata {
        source_fingerprint: file.source_fingerprint.clone(),
        parser_revision: file.parser_revision.clone(),
        evaluator_revision: file.evaluator_revision.clone(),
        updated_at: file.updated_at,
    }
}

fn phase_08_baseline_engine_fingerprint() -> String {
    let mut hasher = Sha256::new();
    let scene_source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/scene.rs"));
    hash_source_section(
        &mut hasher,
        "scene:baseline-object-parser",
        scene_source,
        "pub fn parse_scene_manifest(",
        "fn parse_effect_instances(",
    );
    hash_source_section(
        &mut hasher,
        "scene:baseline-visual-resource-parser",
        scene_source,
        "fn parse_material_info(",
        "fn detect_text_behavior(",
    );
    hash_source_section(
        &mut hasher,
        "scene:baseline-text-layout-parser",
        scene_source,
        "fn detect_text_behavior(",
        "fn compute_render_bounds(",
    );
    hash_source_section(
        &mut hasher,
        "scene:baseline-bounds-and-draw-graph-parser",
        scene_source,
        "fn compute_render_bounds(",
        "fn build_material_passes(",
    );
    hash_source_section(
        &mut hasher,
        "scene:baseline-audio-source-parser",
        scene_source,
        "fn build_audio_sources(",
        "#[cfg(test)]",
    );
    hash_source_section(
        &mut hasher,
        "scene:phase-09e-particle-runtime-parser",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_particle_runtime_service.rs"
        )),
        "pub fn build_scene_particle_runtime(",
        "#[cfg(test)]",
    );
    hash_source_section(
        &mut hasher,
        "scene:baseline-evaluator",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_evaluator_service.rs"
        )),
        "pub fn evaluate_scene_runtime_document_with_runtime_key(",
        "#[cfg(test)]",
    );
    format!("{:x}", hasher.finalize())
}

fn hash_source_section(
    hasher: &mut Sha256,
    label: &str,
    source: &str,
    start_marker: &str,
    end_marker: &str,
) {
    hasher.update(label.as_bytes());
    let Some(start) = source.find(start_marker) else {
        hasher.update(b":missing-start:");
        hasher.update(start_marker.as_bytes());
        return;
    };
    let relative_end = source[start..]
        .find(end_marker)
        .unwrap_or_else(|| source[start..].len());
    hasher.update(source[start..start + relative_end].as_bytes());
}

fn hash_scene_baseline_dependencies(
    hasher: &mut Sha256,
    extracted_root: &Path,
    scene_json_path: &Path,
) -> Result<()> {
    let raw = fs::read(scene_json_path).with_context(|| {
        format!(
            "Unable to read cache dependency input {}",
            scene_json_path.display()
        )
    })?;
    let scene_json: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            hash_labeled_bytes(hasher, "scene-json:invalid-baseline-deps", &raw);
            return Ok(());
        }
    };
    let objects = scene_json
        .get("objects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for object in &objects {
        hash_visual_baseline_dependencies(hasher, extracted_root, object)?;
        hash_particle_baseline_dependencies(hasher, extracted_root, object)?;
        hash_sound_baseline_dependencies(hasher, extracted_root, object)?;
    }

    Ok(())
}

fn hash_visual_baseline_dependencies(
    hasher: &mut Sha256,
    extracted_root: &Path,
    object: &Value,
) -> Result<()> {
    let Some(image_path) = object.get("image").and_then(as_cache_string) else {
        return Ok(());
    };
    let model_path = extracted_root.join(&image_path);
    let Some(model_json) = hash_projected_json_file(
        hasher,
        "visual-model-baseline",
        &model_path,
        model_baseline_projection,
    )?
    else {
        return Ok(());
    };

    let material_path = model_json
        .get("material")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let Some(material_path) = material_path else {
        return Ok(());
    };

    let material_file_path = extracted_root.join(&material_path);
    let Some(material_json) = hash_projected_json_file(
        hasher,
        "visual-material-baseline",
        &material_file_path,
        material_baseline_projection,
    )?
    else {
        return Ok(());
    };

    let texture_names = material_texture_names(&material_json);
    for texture_path in
        baseline_texture_candidates(extracted_root, Some(&material_path), &texture_names)
    {
        hash_optional_file(hasher, texture_path)?;
    }

    Ok(())
}

fn hash_particle_baseline_dependencies(
    hasher: &mut Sha256,
    extracted_root: &Path,
    object: &Value,
) -> Result<()> {
    let Some(particle_path) = object.get("particle").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(particle_json) = hash_projected_json_file(
        hasher,
        "particle-baseline",
        extracted_root.join(particle_path),
        particle_baseline_projection,
    )?
    else {
        return Ok(());
    };
    hash_particle_material_dependencies(hasher, extracted_root, &particle_json)?;
    for child_path in particle_child_paths(&particle_json) {
        let Some(child_json) = hash_projected_json_file(
            hasher,
            "particle-child-baseline",
            extracted_root.join(&child_path),
            particle_baseline_projection,
        )?
        else {
            continue;
        };
        hash_particle_material_dependencies(hasher, extracted_root, &child_json)?;
    }
    Ok(())
}

fn hash_particle_material_dependencies(
    hasher: &mut Sha256,
    extracted_root: &Path,
    particle_json: &Value,
) -> Result<()> {
    let Some(material_path) = particle_json.get("material").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(material_json) = hash_projected_json_file(
        hasher,
        "particle-material-baseline",
        extracted_root.join(material_path),
        material_baseline_projection,
    )?
    else {
        return Ok(());
    };
    let texture_names = material_texture_names(&material_json);
    for texture_path in
        baseline_texture_candidates(extracted_root, Some(material_path), &texture_names)
    {
        hash_optional_file(hasher, texture_path)?;
    }
    Ok(())
}

fn hash_sound_baseline_dependencies(
    hasher: &mut Sha256,
    extracted_root: &Path,
    object: &Value,
) -> Result<()> {
    let Some(sound_path) = object
        .get("sound")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find_map(Value::as_str))
    else {
        return Ok(());
    };
    hash_optional_file_state(hasher, extracted_root.join(sound_path))?;
    Ok(())
}

fn hash_projected_json_file(
    hasher: &mut Sha256,
    label: &str,
    path: impl AsRef<Path>,
    projection: fn(&Value) -> Value,
) -> Result<Option<Value>> {
    let path = path.as_ref();
    hasher.update(label.as_bytes());
    hasher.update(path.display().to_string().as_bytes());
    if !path.exists() {
        hasher.update(b":missing");
        return Ok(None);
    }

    let raw = fs::read(path)
        .with_context(|| format!("Unable to read cache dependency input {}", path.display()))?;
    let value = match serde_json::from_slice::<Value>(&raw) {
        Ok(value) => value,
        Err(_) => {
            hasher.update(b":invalid:");
            hasher.update(&raw);
            return Ok(None);
        }
    };
    let projected = serde_json::to_vec(&projection(&value))?;
    hasher.update(b":present:");
    hasher.update(projected);
    Ok(Some(value))
}

fn model_baseline_projection(value: &Value) -> Value {
    json!({
        "material": value.get("material").cloned().unwrap_or(Value::Null),
        "width": value.get("width").cloned().unwrap_or(Value::Null),
        "height": value.get("height").cloned().unwrap_or(Value::Null),
        "fullscreen": value.get("fullscreen").cloned().unwrap_or(Value::Null),
        "autosize": value.get("autosize").cloned().unwrap_or(Value::Null),
        "solidlayer": value.get("solidlayer").cloned().unwrap_or(Value::Null),
        "passthrough": value.get("passthrough").cloned().unwrap_or(Value::Null),
        "nopadding": value.get("nopadding").cloned().unwrap_or(Value::Null),
    })
}

fn material_baseline_projection(value: &Value) -> Value {
    let Some(pass) = first_material_pass(value) else {
        return Value::Null;
    };
    json!({
        "blending": pass.get("blending").cloned().unwrap_or(Value::Null),
        "textures": pass.get("textures").cloned().unwrap_or_else(|| json!([])),
        "systemUserTextures": pass
            .get("usertextures")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let object = item.as_object()?;
                        (object.get("type").and_then(Value::as_str)? == "system").then(|| {
                            object.get("name").cloned().unwrap_or(Value::Null)
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    })
}

fn particle_baseline_projection(value: &Value) -> Value {
    json!({
        "maxcount": value.get("maxcount").cloned().unwrap_or(Value::Null),
        "starttime": value.get("starttime").cloned().unwrap_or(Value::Null),
        "material": value.get("material").cloned().unwrap_or(Value::Null),
        "renderer": value
            .get("renderer")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(Value::Null),
        "emitter": value
            .get("emitter")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(Value::Null),
        "initializer": value.get("initializer").cloned().unwrap_or(Value::Null),
        "operator": value.get("operator").cloned().unwrap_or(Value::Null),
        "children": value.get("children").cloned().unwrap_or(Value::Null),
    })
}

fn particle_child_paths(value: &Value) -> Vec<String> {
    value
        .get("children")
        .or_else(|| value.get("child"))
        .and_then(Value::as_array)
        .map(|children| {
            children
                .iter()
                .filter_map(|child| child.get("name").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn first_material_pass(value: &Value) -> Option<&Value> {
    value
        .get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .or_else(|| material_declares_inline_pass(value).then_some(value))
}

fn material_declares_inline_pass(value: &Value) -> bool {
    value
        .get("textures")
        .and_then(Value::as_array)
        .map(|textures| !textures.is_empty())
        .unwrap_or(false)
        || value
            .get("usertextures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || value.get("blending").and_then(Value::as_str).is_some()
}

fn material_texture_names(value: &Value) -> Vec<String> {
    first_material_pass(value)
        .and_then(|pass| pass.get("textures"))
        .and_then(Value::as_array)
        .map(|textures| {
            textures
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn baseline_texture_candidates(
    extracted_root: &Path,
    material_path: Option<&str>,
    texture_names: &[String],
) -> Vec<PathBuf> {
    let material_dir = material_path
        .map(|path| extracted_root.join(path))
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let mut candidates = Vec::new();

    for texture_name in texture_names {
        let path = Path::new(texture_name);
        let has_relative_segments = path.components().count() > 1;
        let mut local_candidates = Vec::new();

        if path.extension().is_some() {
            local_candidates.push(extracted_root.join(path));
            if has_relative_segments {
                local_candidates.push(extracted_root.join("materials").join(path));
            }
            if let Some(material_dir) = material_dir.as_ref() {
                local_candidates.push(material_dir.join(path));
            }
        } else {
            if has_relative_segments {
                local_candidates.push(extracted_root.join(path).with_extension("tex"));
                local_candidates.push(
                    extracted_root
                        .join("materials")
                        .join(path)
                        .with_extension("tex"),
                );
            }
            if let Some(material_dir) = material_dir.as_ref() {
                local_candidates.push(material_dir.join(path).with_extension("tex"));
            }
            if !has_relative_segments {
                local_candidates.push(extracted_root.join(format!("{texture_name}.tex")));
            }
        }

        for candidate in local_candidates {
            if candidate.exists() && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    candidates
}

fn as_cache_string(value: &Value) -> Option<String> {
    let resolved = value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value);
    match resolved {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn hash_labeled_bytes(hasher: &mut Sha256, label: &str, bytes: &[u8]) {
    hasher.update(label.as_bytes());
    hasher.update(b":");
    hasher.update(bytes);
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

fn hash_project_metadata_file(hasher: &mut Sha256, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    hasher.update(path.display().to_string().as_bytes());
    if !path.exists() {
        hasher.update(b":missing");
        return Ok(());
    }
    let contents = fs::read(path)
        .with_context(|| format!("Unable to read cache fingerprint input {}", path.display()))?;
    let Ok(project_json) = serde_json::from_slice::<Value>(&contents) else {
        hasher.update(b":present-invalid:");
        hasher.update(contents);
        return Ok(());
    };
    hasher.update(b":present:");
    hasher.update(serde_json::to_vec(&project_metadata_projection(
        &project_json,
    ))?);
    Ok(())
}

fn project_metadata_projection(project_json: &Value) -> Value {
    let mut projected = project_json.clone();
    if let Some(properties) = projected
        .get_mut("general")
        .and_then(|general| general.get_mut("properties"))
        .and_then(Value::as_object_mut)
    {
        for property in properties.values_mut() {
            if let Some(property) = property.as_object_mut() {
                property.remove("value");
            }
        }
    }
    projected
}

fn hash_optional_file_state(hasher: &mut Sha256, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    hasher.update(path.display().to_string().as_bytes());
    if !path.exists() {
        hasher.update(b":missing");
        return Ok(());
    }
    let metadata = fs::metadata(path).with_context(|| {
        format!(
            "Unable to read cache dependency metadata {}",
            path.display()
        )
    })?;
    hasher.update(b":present:");
    hasher.update(metadata.len().to_string().as_bytes());
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
        compute_source_fingerprint_with_baseline_engine, load_scene_manifest,
        record_needs_cache_refresh, refresh_cache_for_record, scene_cache_path_for_record,
        SCENE_EVALUATOR_REVISION, SCENE_PARSER_REVISION,
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
            last_snapshot_path: None,
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

    #[test]
    fn cache_refresh_ignores_project_property_value_only_changes() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(
            source.join("project.json"),
            json!({
                "title": "Scene",
                "type": "scene",
                "general": {
                    "properties": {
                        "enabled": {
                            "type": "bool",
                            "text": "Enabled",
                            "value": true
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());

        fs::write(
            source.join("project.json"),
            json!({
                "title": "Scene",
                "type": "scene",
                "general": {
                    "properties": {
                        "enabled": {
                            "type": "bool",
                            "text": "Enabled",
                            "value": false
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        assert!(!record_needs_cache_refresh(&record).unwrap());
    }

    #[test]
    fn cache_refresh_detects_project_property_schema_changes() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(
            source.join("project.json"),
            json!({
                "title": "Scene",
                "type": "scene",
                "general": {
                    "properties": {
                        "enabled": {
                            "type": "bool",
                            "text": "Enabled",
                            "value": true
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());

        fs::write(
            source.join("project.json"),
            json!({
                "title": "Scene",
                "type": "scene",
                "general": {
                    "properties": {
                        "enabled": {
                            "type": "bool",
                            "text": "Enabled renamed",
                            "value": true
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        assert!(record_needs_cache_refresh(&record).unwrap());
    }

    #[test]
    fn cache_refresh_detects_baseline_engine_fingerprint_changes() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("source")).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(managed.join("source").join("project.json"), "{}").unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());

        let stale_fingerprint =
            compute_source_fingerprint_with_baseline_engine(&record, "old-phase-08-baseline")
                .unwrap();
        record.scene_cache.as_mut().unwrap().source_fingerprint = stale_fingerprint.clone();
        let cache_path = scene_cache_path_for_record(&record);
        fs::write(
            cache_path,
            serde_json::to_vec_pretty(&json!({
                "sourceFingerprint": stale_fingerprint,
                "parserRevision": SCENE_PARSER_REVISION,
                "evaluatorRevision": SCENE_EVALUATOR_REVISION,
                "updatedAt": record.scene_cache.as_ref().unwrap().updated_at,
                "manifest": SceneManifest::default(),
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(record_needs_cache_refresh(&record).unwrap());
    }

    #[test]
    fn cache_refresh_detects_baseline_dependency_metadata_changes() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        let extracted = managed.join("extracted");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(extracted.join("models")).unwrap();
        fs::write(source.join("project.json"), "{}").unwrap();
        fs::write(
            extracted.join("scene.json"),
            json!({
                "objects": [{
                    "id": 1,
                    "name": "Visual",
                    "image": "models/visual.json"
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            extracted.join("models").join("visual.json"),
            json!({
                "width": 100,
                "height": 50
            })
            .to_string(),
        )
        .unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());

        fs::write(
            extracted.join("models").join("visual.json"),
            json!({
                "width": 200,
                "height": 50
            })
            .to_string(),
        )
        .unwrap();
        assert!(record_needs_cache_refresh(&record).unwrap());
    }

    #[test]
    fn baseline_unrelated_material_shader_change_does_not_refresh_cache() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        let extracted = managed.join("extracted");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(extracted.join("models")).unwrap();
        fs::create_dir_all(extracted.join("materials")).unwrap();
        fs::write(source.join("project.json"), "{}").unwrap();
        fs::write(
            extracted.join("scene.json"),
            json!({
                "objects": [{
                    "id": 1,
                    "name": "Visual",
                    "image": "models/visual.json"
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            extracted.join("models").join("visual.json"),
            json!({
                "width": 100,
                "height": 50,
                "material": "materials/visual.json"
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            extracted.join("materials").join("visual.json"),
            json!({
                "shader": "effects/phase10-only-a.frag"
            })
            .to_string(),
        )
        .unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = record.scene_manifest.clone().unwrap();
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());

        fs::write(
            extracted.join("materials").join("visual.json"),
            json!({
                "shader": "effects/phase10-only-b.frag"
            })
            .to_string(),
        )
        .unwrap();
        assert!(!record_needs_cache_refresh(&record).unwrap());
    }

    #[test]
    fn stale_cache_file_metadata_blocks_old_manifest_reuse() {
        let temp = tempdir().unwrap();
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("source")).unwrap();
        fs::create_dir_all(managed.join("extracted")).unwrap();
        fs::write(managed.join("source").join("project.json"), "{}").unwrap();
        fs::write(managed.join("extracted").join("scene.json"), "{}").unwrap();

        let mut record = sample_scene_record(managed.display().to_string());
        let manifest = SceneManifest {
            object_count: 1,
            ..SceneManifest::default()
        };
        refresh_cache_for_record(&mut record, Some(manifest)).unwrap();
        assert_eq!(load_scene_manifest(&record).unwrap().object_count, 1);

        let metadata = record.scene_cache.clone().unwrap();
        fs::write(
            scene_cache_path_for_record(&record),
            serde_json::to_vec_pretty(&json!({
                "sourceFingerprint": "stale-fingerprint",
                "parserRevision": metadata.parser_revision,
                "evaluatorRevision": metadata.evaluator_revision,
                "updatedAt": metadata.updated_at,
                "manifest": {
                    "objectCount": 99
                },
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(record_needs_cache_refresh(&record).unwrap());
        assert!(load_scene_manifest(&record).is_none());
    }
}
