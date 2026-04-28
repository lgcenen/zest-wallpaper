use crate::{
    models::{
        PlayerRuntimeState, SceneManifest, SceneNowPlayingSnapshot, VideoRuntimeDocument,
        WallpaperRecord, WallpaperRuntime, WallpaperRuntimeRecord, WallpaperType,
        WebRuntimeDocument,
    },
    store::{find_record, AppState, DynamicPlayerState},
};

use chrono::Utc;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use super::{
    asset_resolver::AssetResolver, scene_cache_service, scene_evaluator_service,
    scene_now_playing_provider_service,
};

fn scene_manifest_runtime_cache() -> &'static Mutex<BTreeMap<String, SceneManifest>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, SceneManifest>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn scene_manifest_cache_key(record: &WallpaperRecord) -> Option<String> {
    let metadata = record.scene_cache.as_ref()?;
    Some(format!(
        "{}|{}|{}|{}|{}",
        record.managed_path,
        metadata.source_fingerprint,
        metadata.parser_revision,
        metadata.evaluator_revision,
        metadata.updated_at.timestamp_millis(),
    ))
}

fn cached_scene_manifest(record: &WallpaperRecord) -> SceneManifest {
    if let Some(manifest) = record.scene_manifest.clone() {
        return manifest;
    }

    let Some(cache_key) = scene_manifest_cache_key(record) else {
        return scene_cache_service::load_scene_manifest(record).unwrap_or_default();
    };

    if let Ok(cache) = scene_manifest_runtime_cache().lock() {
        if let Some(manifest) = cache.get(&cache_key) {
            return manifest.clone();
        }
    }

    let manifest = scene_cache_service::load_scene_manifest(record).unwrap_or_default();
    if let Ok(mut cache) = scene_manifest_runtime_cache().lock() {
        let prefix = format!("{}|", record.managed_path);
        cache.retain(|key, _| key == &cache_key || !key.starts_with(&prefix));
        cache.insert(cache_key, manifest.clone());
    }
    manifest
}

#[cfg(test)]
fn clear_scene_manifest_runtime_cache() {
    if let Ok(mut cache) = scene_manifest_runtime_cache().lock() {
        cache.clear();
    }
}

pub fn runtime_record(record: &WallpaperRecord) -> WallpaperRuntimeRecord {
    runtime_record_with_context(record, Utc::now(), None)
}

pub fn runtime_record_with_context(
    record: &WallpaperRecord,
    now: chrono::DateTime<Utc>,
    now_playing: Option<&SceneNowPlayingSnapshot>,
) -> WallpaperRuntimeRecord {
    let runtime_owner_key = scene_runtime_owner_key(record);
    let resolver = AssetResolver::for_record(record);
    let preview_path = record
        .preview_path
        .clone()
        .or_else(|| resolver.resolve_preview_path(None));
    let entry_path = record
        .entry_path
        .clone()
        .or_else(|| resolver.resolve_entry_path(None, wallpaper_type_name(&record.wallpaper_type)));
    let runtime = match record.wallpaper_type {
        WallpaperType::Scene => {
            let manifest = cached_scene_manifest(record);
            let now_playing_snapshot = scene_now_playing_snapshot(&manifest, now_playing);
            WallpaperRuntime::Scene {
                scene: scene_evaluator_service::evaluate_scene_runtime_document_with_runtime_key(
                    Some(runtime_owner_key.as_str()),
                    manifest,
                    &scene_evaluator_service::property_values_from_schema(&record.property_schema),
                    &scene_evaluator_service::property_values_from_schema(&record.property_schema),
                    &scene_evaluator_service::property_definitions_from_schema(
                        &record.property_schema,
                    ),
                    Some(&now_playing_snapshot),
                    now,
                ),
            }
        }
        WallpaperType::Video => WallpaperRuntime::Video {
            video: VideoRuntimeDocument {
                entry_path: entry_path.clone(),
                preview_path: preview_path.clone(),
                source_path: record.source_path.clone(),
                managed_path: record.managed_path.clone(),
            },
        },
        WallpaperType::Web => WallpaperRuntime::Web {
            web: WebRuntimeDocument {
                entry_path: entry_path.clone(),
                preview_path: preview_path.clone(),
                source_path: record.source_path.clone(),
                managed_path: record.managed_path.clone(),
            },
        },
        WallpaperType::Application => WallpaperRuntime::Application,
        WallpaperType::Unknown => WallpaperRuntime::Unknown,
    };

    WallpaperRuntimeRecord {
        id: record.id.clone(),
        title: record.title.clone(),
        wallpaper_type: record.wallpaper_type.clone(),
        source_path: record.source_path.clone(),
        managed_path: record.managed_path.clone(),
        preview_path,
        entry_path,
        last_snapshot_path: record.last_snapshot_path.clone(),
        property_schema: record.property_schema.clone(),
        property_sections: record.property_sections.clone(),
        imported_at: record.imported_at,
        tags: record.tags.clone(),
        runtime,
    }
}

pub fn runtime_records(records: &[WallpaperRecord]) -> Vec<WallpaperRuntimeRecord> {
    let now = Utc::now();
    records
        .iter()
        .map(|record| runtime_record_with_context(record, now, None))
        .collect()
}

pub fn player_runtime_state(
    player: &DynamicPlayerState,
    state: &AppState,
) -> Result<PlayerRuntimeState, String> {
    let store = state.library.lock().map_err(|error| error.to_string())?;
    let now = Utc::now();
    let active = player
        .active_id
        .as_deref()
        .and_then(|id| find_record(&store, id))
        .map(|record| runtime_record_with_context(&record, now, None));
    Ok(PlayerRuntimeState {
        active,
        paused: player.effective_paused(),
    })
}

fn scene_now_playing_snapshot(
    manifest: &SceneManifest,
    explicit: Option<&SceneNowPlayingSnapshot>,
) -> SceneNowPlayingSnapshot {
    if !scene_now_playing_provider_service::uses_now_playing_provider(manifest) {
        return SceneNowPlayingSnapshot::default();
    }

    explicit
        .cloned()
        .unwrap_or_else(scene_now_playing_provider_service::now_playing_snapshot)
}

fn wallpaper_type_name(value: &WallpaperType) -> &'static str {
    match value {
        WallpaperType::Scene => "scene",
        WallpaperType::Video => "video",
        WallpaperType::Web => "web",
        WallpaperType::Application => "application",
        WallpaperType::Unknown => "unknown",
    }
}

fn scene_runtime_owner_key(record: &WallpaperRecord) -> String {
    scene_manifest_cache_key(record).unwrap_or_else(|| {
        format!(
            "{}|{}|{}",
            record.id,
            record.managed_path,
            record.entry_path.clone().unwrap_or_default()
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use chrono::Utc;
    use serde_json::json;
    use tempfile::tempdir;

    use crate::{
        models::{
            EvaluatedSceneObject, LibraryStore, PlayerRuntimeState, PropertyKind,
            PropertyPresentation, SceneManifest, SceneNowPlayingAvailability,
            SceneNowPlayingSnapshot, SceneNowPlayingState, SceneRuntimeSettings, SceneTextBehavior,
            SceneTextLayer, WallpaperProperty, WallpaperType,
        },
        store::{AppState, DynamicPlayerState},
    };

    use super::{
        clear_scene_manifest_runtime_cache, player_runtime_state, runtime_record,
        runtime_record_with_context,
    };

    fn sample_record(wallpaper_type: WallpaperType) -> crate::models::WallpaperRecord {
        crate::models::WallpaperRecord {
            id: "demo".to_string(),
            title: "Demo".to_string(),
            wallpaper_type,
            source_path: "/tmp/source".to_string(),
            managed_path: "/tmp/managed".to_string(),
            preview_path: Some("/tmp/preview.png".to_string()),
            entry_path: Some("/tmp/entry".to_string()),
            last_snapshot_path: Some("/tmp/snapshot.png".to_string()),
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
                order: Some(1),
                presentation: PropertyPresentation::Control,
                options: vec![],
            }],
            property_sections: vec![],
            scene_cache: None,
            scene_manifest: Some(SceneManifest::default()),
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec!["demo".to_string()],
        }
    }

    fn media_title_layer() -> SceneTextLayer {
        SceneTextLayer {
            id: 44,
            name: "Media Title".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: Some("center".to_string()),
            vertical_align: Some("center".to_string()),
            content: "Fallback".to_string(),
            behavior: SceneTextBehavior::MediaTitle,
            delimiter: None,
            month_format: None,
            day_format: None,
            show_day: None,
            align_vertical: None,
            use_delimiter: None,
            show_seconds: None,
            use_24h_format: None,
            visible: true,
            visibility_binding: None,
            text_binding: None,
            position: [0.0, 0.0, 0.0],
            position_bindings: None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: Some([300.0, 80.0]),
            render_bounds: Some([0.0, 0.0, 300.0, 80.0]),
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: None,
            alpha_binding: None,
            point_size: Some(32.0),
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: None,
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        }
    }

    fn ready_now_playing_snapshot(title: &str, generation: u64) -> SceneNowPlayingSnapshot {
        SceneNowPlayingSnapshot {
            availability: SceneNowPlayingAvailability::Available,
            state: SceneNowPlayingState::Ready,
            title: Some(title.to_string()),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            source: Some("Music".to_string()),
            generation,
            updated_at: Utc::now(),
            refresh_interval_millis: 1500,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn scene_runtime_reuses_cached_manifest_when_cache_metadata_is_unchanged() {
        clear_scene_manifest_runtime_cache();
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(managed.join("cache")).expect("cache dir");
        fs::create_dir_all(managed.join("source")).expect("source dir");
        fs::create_dir_all(managed.join("extracted")).expect("extracted dir");
        fs::write(managed.join("source").join("project.json"), "{}").expect("project");
        fs::write(managed.join("extracted").join("scene.json"), "{}").expect("scene");

        let mut record = sample_record(WallpaperType::Scene);
        record.managed_path = managed.display().to_string();
        record.source_path = managed.display().to_string();
        let first_manifest = SceneManifest {
            object_count: 1,
            ..SceneManifest::default()
        };
        crate::services::scene_cache_service::refresh_cache_for_record(
            &mut record,
            Some(first_manifest.clone()),
        )
        .expect("write first cache");

        let first_runtime = runtime_record(&record);
        match first_runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                assert_eq!(scene.source.object_count, 1);
            }
            _ => panic!("expected scene runtime"),
        }

        let metadata = record.scene_cache.clone().expect("scene cache metadata");
        let cache_path = crate::services::scene_cache_service::scene_cache_path_for_record(&record);
        let second_manifest = SceneManifest {
            object_count: 9,
            ..SceneManifest::default()
        };
        fs::write(
            cache_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "sourceFingerprint": metadata.source_fingerprint,
                "parserRevision": metadata.parser_revision,
                "evaluatorRevision": metadata.evaluator_revision,
                "updatedAt": metadata.updated_at,
                "manifest": second_manifest,
            }))
            .expect("serialize cache file"),
        )
        .expect("overwrite cache file");

        let second_runtime = runtime_record(&record);
        match second_runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                assert_eq!(scene.source.object_count, 1);
            }
            _ => panic!("expected scene runtime"),
        }

        clear_scene_manifest_runtime_cache();
    }

    #[test]
    fn builds_scene_runtime_document_from_record() {
        let runtime = runtime_record(&sample_record(WallpaperType::Scene));
        match runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                assert_eq!(scene.source.object_count, 0);
                assert_eq!(scene.evaluated.canvas_width, 3840.0);
                assert_eq!(scene.evaluated.canvas_height, 2160.0);
            }
            _ => panic!("expected scene runtime"),
        }
    }

    #[test]
    fn scene_runtime_consumes_now_playing_provider_snapshot_for_media_title() {
        let mut record = sample_record(WallpaperType::Scene);
        record.scene_manifest = Some(SceneManifest {
            canvas_width: Some(800.0),
            canvas_height: Some(450.0),
            text_layers: vec![media_title_layer()],
            ..SceneManifest::default()
        });
        let snapshot = ready_now_playing_snapshot("Provider Track", 12);

        let runtime = runtime_record_with_context(&record, Utc::now(), Some(&snapshot));
        match runtime.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => {
                assert_eq!(scene.now_playing.generation, 12);
                assert_eq!(scene.now_playing.title.as_deref(), Some("Provider Track"));
                let object = scene.evaluated.objects.get(&44).expect("media text");
                let EvaluatedSceneObject::Text { text, .. } = object else {
                    panic!("expected text object");
                };
                assert_eq!(text.value, "Provider Track");
                assert_eq!(text.dynamic_input_generation, Some(12));
            }
            _ => panic!("expected scene runtime"),
        }
    }

    #[test]
    fn builds_video_runtime_document_from_record() {
        let runtime = runtime_record(&sample_record(WallpaperType::Video));
        match runtime.runtime {
            crate::models::WallpaperRuntime::Video { video } => {
                assert_eq!(video.entry_path.as_deref(), Some("/tmp/entry"));
                assert_eq!(video.preview_path.as_deref(), Some("/tmp/preview.png"));
                let payload = serde_json::to_value(&video).unwrap();
                assert!(payload.get("lastSnapshotPath").is_none());
            }
            _ => panic!("expected video runtime"),
        }
    }

    #[test]
    fn builds_web_runtime_document_from_record() {
        let runtime = runtime_record(&sample_record(WallpaperType::Web));
        match runtime.runtime {
            crate::models::WallpaperRuntime::Web { web } => {
                assert_eq!(web.entry_path.as_deref(), Some("/tmp/entry"));
                assert_eq!(web.preview_path.as_deref(), Some("/tmp/preview.png"));
            }
            _ => panic!("expected web runtime"),
        }
    }

    #[test]
    fn player_state_uses_runtime_record_payload() {
        let record = sample_record(WallpaperType::Scene);
        let state = AppState {
            library: std::sync::Mutex::new(LibraryStore {
                wallpapers: vec![record.clone()],
            }),
            player: std::sync::Mutex::new(DynamicPlayerState {
                active_id: Some(record.id.clone()),
                manually_paused: false,
                auto_pause_screen_labels: BTreeSet::new(),
                scene_update_generation: 0,
                last_scene_signature: None,
            }),
            static_snapshot_sync: std::sync::Mutex::new(
                crate::store::StaticSnapshotSyncState::default(),
            ),
            runtime_sync: std::sync::Mutex::new(()),
            scene_runtime_settings: std::sync::Mutex::new(SceneRuntimeSettings::default()),
        };
        let player = DynamicPlayerState {
            active_id: Some(record.id.clone()),
            manually_paused: true,
            auto_pause_screen_labels: BTreeSet::new(),
            scene_update_generation: 0,
            last_scene_signature: None,
        };

        let runtime: PlayerRuntimeState = player_runtime_state(&player, &state).unwrap();
        assert!(runtime.active.is_some());
        assert!(runtime.paused);
        assert!(matches!(
            runtime.active.unwrap().runtime,
            crate::models::WallpaperRuntime::Scene { .. }
        ));
    }

    #[test]
    fn runtime_record_keeps_snapshot_metadata_out_of_runtime_documents() {
        let runtime = runtime_record(&sample_record(WallpaperType::Video));
        let payload = serde_json::to_value(&runtime).unwrap();

        assert_eq!(payload["lastSnapshotPath"], "/tmp/snapshot.png");
        assert!(payload["runtime"]["video"]
            .get("lastSnapshotPath")
            .is_none());
    }
}
