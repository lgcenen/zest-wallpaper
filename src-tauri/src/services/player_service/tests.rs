use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    env, fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier, Mutex,
    },
    thread,
    time::Duration,
};

use chrono::{TimeZone, Utc};
use tempfile::tempdir;

use crate::{
    models::{
        EvaluatedSceneCamera, EvaluatedSceneObject, EvaluatedSceneObjectBase,
        EvaluatedSceneTransform, EvaluatedTextLayout, EvaluatedTextState, EvaluatedTextStyle,
        LibraryStore, PropertyKind, PropertyPresentation, SceneEvaluatedDocument, SceneManifest,
        SceneRuntimeDocument, SceneRuntimeSettings, SceneTextBehavior, SceneTextLayer,
        WallpaperProperty, WallpaperRecord, WallpaperRuntime, WallpaperRuntimeRecord,
        WallpaperType,
    },
    services::scene_now_playing_provider_service,
    store::{AppState, DynamicPlayerState, HOME_ENV_LOCK},
};

use super::{
    ActivePropertyUpdateSyncMode, NativeHostKind, NativeHostSyncDisposition, SceneUpdateCadence,
    SceneUpdateSyncMode, active_property_update_sync_mode, apply_pause_change,
    apply_runtime_record_transaction, build_apply_wallpaper_candidate,
    clear_player_session_state, dispatch_active_property_update_sync,
    dispatch_scene_update_sync, ensure_apply_record_current_by_id_with,
    native_host_sync_disposition, push_critical_sync_error,
    restore_runtime_record_transaction, scene_requires_periodic_updates, scene_signature,
    scene_update_cadence, scene_update_sync_is_current, scene_update_sync_mode,
    should_emit_scene_update, should_start_scene_update_loop, validate_scene_apply_preflight,
};

fn runtime_record(
    runtime: WallpaperRuntime,
    wallpaper_type: WallpaperType,
) -> WallpaperRuntimeRecord {
    WallpaperRuntimeRecord {
        id: "demo".to_string(),
        title: "Demo".to_string(),
        wallpaper_type,
        source_path: "/tmp/source".to_string(),
        managed_path: "/tmp/managed".to_string(),
        preview_path: None,
        entry_path: None,
        last_snapshot_path: None,
        property_schema: vec![],
        property_sections: vec![],
        imported_at: Utc::now(),
        tags: vec![],
        runtime,
    }
}

fn app_state(player: DynamicPlayerState) -> AppState {
    AppState {
        library: Mutex::new(LibraryStore::default()),
        player: Mutex::new(player),
        static_snapshot_sync: Mutex::new(crate::store::StaticSnapshotSyncState::default()),
        runtime_sync: Mutex::new(()),
        scene_runtime_settings: Mutex::new(SceneRuntimeSettings::default()),
    }
}

fn scene_record(managed_root: &str) -> WallpaperRecord {
    WallpaperRecord {
        id: "scene-demo".to_string(),
        title: "Scene Demo".to_string(),
        wallpaper_type: WallpaperType::Scene,
        source_path: managed_root.to_string(),
        managed_path: managed_root.to_string(),
        preview_path: None,
        entry_path: None,
        last_snapshot_path: None,
        property_schema: vec![],
        property_sections: vec![],
        scene_cache: None,
        scene_manifest: Some(SceneManifest::default()),
        scene_manifest_version: None,
        scene_manifest_dirty: false,
        imported_at: Utc::now(),
        tags: vec![],
    }
}

fn web_record(id: &str, managed_root: &str, entry_path: String) -> WallpaperRecord {
    WallpaperRecord {
        id: id.to_string(),
        title: "Web Demo".to_string(),
        wallpaper_type: WallpaperType::Web,
        source_path: managed_root.to_string(),
        managed_path: managed_root.to_string(),
        preview_path: Some(format!("{managed_root}/preview.png")),
        entry_path: Some(entry_path),
        last_snapshot_path: None,
        property_schema: vec![],
        property_sections: vec![],
        scene_cache: None,
        scene_manifest: None,
        scene_manifest_version: None,
        scene_manifest_dirty: false,
        imported_at: Utc::now(),
        tags: vec![],
    }
}

fn web_property(key: &str, value: serde_json::Value) -> WallpaperProperty {
    WallpaperProperty {
        key: key.to_string(),
        label: key.to_string(),
        markup: None,
        kind: PropertyKind::Slider,
        value: value.clone(),
        default_value: value,
        min: None,
        max: None,
        step: None,
        condition: None,
        order: None,
        presentation: PropertyPresentation::Control,
        options: vec![],
    }
}

fn text_object(behavior: SceneTextBehavior) -> EvaluatedSceneObject {
    EvaluatedSceneObject::Text {
        base: EvaluatedSceneObjectBase {
            id: 7,
            name: "Clock".to_string(),
            parent_id: None,
            dependencies: vec![],
            visible: true,
            alignment: None,
            opacity: 1.0,
            transform: EvaluatedSceneTransform {
                position: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                rotation: 0.0,
                render_bounds: Some([0.0, 0.0, 320.0, 120.0]),
            },
        },
        behavior,
        text: EvaluatedTextState {
            value: "12:34:56".to_string(),
            style: EvaluatedTextStyle {
                color: Some("1 1 1".to_string()),
                alpha: 1.0,
                point_size: 64.0,
                font_path: None,
                effect_paths: vec![],
                horizontal_align: Some("center".to_string()),
                vertical_align: Some("center".to_string()),
                padding: Some(0.0),
                max_rows: None,
                max_width: None,
                limit_width: None,
                limit_use_ellipsis: None,
                block_align: None,
            },
            layout: EvaluatedTextLayout {
                size: Some([320.0, 120.0]),
                render_bounds: Some([0.0, 0.0, 320.0, 120.0]),
                content_bounds: Some([0.0, 0.0, 320.0, 120.0]),
                scaled_point_size: 64.0,
                scaled_padding: 0.0,
                world_scale: [1.0, 1.0, 1.0],
            },
            dynamic_input_generation: None,
        },
    }
}

fn source_text_layer(
    behavior: SceneTextBehavior,
    show_seconds: Option<bool>,
    script_refresh_interval_millis: Option<u64>,
) -> SceneTextLayer {
    SceneTextLayer {
        id: 7,
        name: "Clock".to_string(),
        dependencies: vec![],
        parent_id: None,
        alignment: None,
        anchor: None,
        horizontal_align: Some("center".to_string()),
        vertical_align: Some("center".to_string()),
        content: "12:34".to_string(),
        behavior,
        delimiter: Some(":".to_string()),
        month_format: None,
        day_format: None,
        show_day: None,
        align_vertical: None,
        use_delimiter: None,
        show_seconds,
        use_24h_format: Some(true),
        visible: true,
        visibility_binding: None,
        text_binding: None,
        position: [0.0, 0.0, 0.0],
        position_bindings: None,
        scale: [1.0, 1.0, 1.0],
        scale_binding: None,
        angles: None,
        rotation: None,
        size: Some([320.0, 120.0]),
        render_bounds: Some([0.0, 0.0, 320.0, 120.0]),
        parallax_depth: None,
        color: Some("1 1 1".to_string()),
        color_binding: None,
        alpha: Some(1.0),
        alpha_binding: None,
        point_size: Some(64.0),
        point_size_binding: None,
        font_reference: None,
        font_path: None,
        effect_paths: vec![],
        script_text: None,
        script_refresh_interval_millis,
        padding: Some(0.0),
        max_rows: None,
        max_width: None,
        limit_width: None,
        limit_use_ellipsis: None,
        block_align: None,
    }
}

fn scene_runtime_with_objects(
    objects: BTreeMap<u32, EvaluatedSceneObject>,
    source_text_layers: Vec<SceneTextLayer>,
    evaluated_at: chrono::DateTime<Utc>,
) -> SceneRuntimeDocument {
    SceneRuntimeDocument {
        runtime_owner_key: None,
        source: SceneManifest {
            text_layers: source_text_layers,
            ..SceneManifest::default()
        },
        evaluated: SceneEvaluatedDocument {
            canvas_width: 1920.0,
            canvas_height: 1080.0,
            clear_color: None,
            camera: EvaluatedSceneCamera {
                zoom: 1.0,
                center: [0.0, 0.0],
                camera_shake: false,
                camera_shake_amplitude: 0.0,
                camera_shake_speed: 0.0,
                parallax_mouse_influence: 0.0,
            },
            parallax: Default::default(),
            objects,
            render_list: vec![7],
            evaluated_at,
            diagnostics: Vec::new(),
        },
        now_playing: Default::default(),
    }
}

#[test]
fn scene_update_emits_only_when_evaluated_signature_changes() {
    assert!(!should_emit_scene_update(Some("same"), Some("same")));
    assert!(should_emit_scene_update(Some("before"), Some("after")));
    assert!(should_emit_scene_update(None, Some("first")));
    assert!(!should_emit_scene_update(Some("existing"), None));
}

#[test]
fn scene_signature_ignores_evaluated_timestamp_only_changes() {
    let mut objects = BTreeMap::new();
    objects.insert(7, text_object(SceneTextBehavior::Clock));
    let earlier = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                objects.clone(),
                vec![source_text_layer(SceneTextBehavior::Clock, Some(true), None)],
                Utc.with_ymd_and_hms(2026, 4, 12, 13, 0, 0).unwrap(),
            ),
        },
        WallpaperType::Scene,
    );
    let later = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                objects,
                vec![source_text_layer(SceneTextBehavior::Clock, Some(true), None)],
                Utc.with_ymd_and_hms(2026, 4, 12, 13, 0, 1).unwrap(),
            ),
        },
        WallpaperType::Scene,
    );

    assert_eq!(scene_signature(&earlier), scene_signature(&later));
}

#[test]
fn scene_signature_classifies_text_runtime_changes_as_lightweight() {
    let mut earlier_objects = BTreeMap::new();
    earlier_objects.insert(7, text_object(SceneTextBehavior::Clock));
    let mut later_objects = earlier_objects.clone();
    if let Some(EvaluatedSceneObject::Text { text, .. }) = later_objects.get_mut(&7) {
        text.value = "12:34:57".to_string();
        text.layout.content_bounds = Some([8.0, 0.0, 304.0, 120.0]);
        text.layout.scaled_point_size = 60.0;
    }

    let earlier = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                earlier_objects,
                vec![source_text_layer(SceneTextBehavior::Clock, Some(true), None)],
                Utc.with_ymd_and_hms(2026, 4, 12, 13, 0, 0).unwrap(),
            ),
        },
        WallpaperType::Scene,
    );
    let later = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                later_objects,
                vec![source_text_layer(SceneTextBehavior::Clock, Some(true), None)],
                Utc.with_ymd_and_hms(2026, 4, 12, 13, 0, 1).unwrap(),
            ),
        },
        WallpaperType::Scene,
    );

    assert_eq!(
        scene_update_sync_mode(
            scene_signature(&earlier).as_deref(),
            scene_signature(&later).as_deref()
        ),
        SceneUpdateSyncMode::LightweightDynamicText
    );
}

#[test]
fn active_property_update_classifies_text_value_change_as_lightweight_text_patch() {
    let mut earlier_objects = BTreeMap::new();
    earlier_objects.insert(7, text_object(SceneTextBehavior::Static));
    let mut later_objects = earlier_objects.clone();
    if let Some(EvaluatedSceneObject::Text { text, .. }) = later_objects.get_mut(&7) {
        text.value = "Updated".to_string();
    }

    let earlier = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                earlier_objects,
                vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );
    let later = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                later_objects,
                vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );

    assert_eq!(
        active_property_update_sync_mode(Some(&earlier), &later, false),
        ActivePropertyUpdateSyncMode::LightweightDynamicText
    );
}

#[test]
fn active_property_update_classifies_same_topology_evaluated_change_as_scene_runtime_patch() {
    let mut earlier_objects = BTreeMap::new();
    earlier_objects.insert(7, text_object(SceneTextBehavior::Static));
    let mut later_objects = earlier_objects.clone();
    if let Some(EvaluatedSceneObject::Text { base, .. }) = later_objects.get_mut(&7) {
        base.opacity = 0.5;
    }

    let earlier = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                earlier_objects,
                vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );
    let later = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                later_objects,
                vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );

    assert_eq!(
        active_property_update_sync_mode(Some(&earlier), &later, false),
        ActivePropertyUpdateSyncMode::LightweightSceneRuntime
    );
}

#[test]
fn active_property_update_keeps_structural_changes_on_full_sync() {
    let mut earlier_objects = BTreeMap::new();
    earlier_objects.insert(7, text_object(SceneTextBehavior::Static));
    let mut later_scene = scene_runtime_with_objects(
        earlier_objects.clone(),
        vec![source_text_layer(SceneTextBehavior::Static, None, None)],
        Utc::now(),
    );
    later_scene.evaluated.render_list.clear();

    let earlier = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                earlier_objects,
                vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );
    let later = runtime_record(
        WallpaperRuntime::Scene { scene: later_scene },
        WallpaperType::Scene,
    );

    assert_eq!(
        active_property_update_sync_mode(Some(&earlier), &later, false),
        ActivePropertyUpdateSyncMode::FullNativeSync
    );
    assert_eq!(
        active_property_update_sync_mode(Some(&earlier), &later, true),
        ActivePropertyUpdateSyncMode::FullNativeSync
    );
}

#[test]
fn lightweight_scene_update_dispatch_does_not_call_full_sync() {
    let lightweight_calls = Cell::new(0);
    let full_sync_calls = Cell::new(0);

    let mode = dispatch_scene_update_sync(
        SceneUpdateSyncMode::LightweightDynamicText,
        || {
            lightweight_calls.set(lightweight_calls.get() + 1);
            Ok(())
        },
        || {
            full_sync_calls.set(full_sync_calls.get() + 1);
            Ok(())
        },
        || true,
    )
    .expect("lightweight update should succeed");

    assert_eq!(mode, SceneUpdateSyncMode::LightweightDynamicText);
    assert_eq!(lightweight_calls.get(), 1);
    assert_eq!(full_sync_calls.get(), 0);
}

#[test]
fn active_web_property_update_triggers_native_bridge_sync() {
    let mut earlier = runtime_record(
        WallpaperRuntime::Web {
            web: Default::default(),
        },
        WallpaperType::Web,
    );
    let mut later = earlier.clone();
    earlier.property_schema = vec![web_property("speed", serde_json::json!(1))];
    later.property_schema = vec![web_property("speed", serde_json::json!(2))];

    assert_eq!(
        active_property_update_sync_mode(Some(&earlier), &later, false),
        ActivePropertyUpdateSyncMode::FullNativeSync
    );
}

#[test]
fn unchanged_web_property_payload_stays_current() {
    let mut earlier = runtime_record(
        WallpaperRuntime::Web {
            web: Default::default(),
        },
        WallpaperType::Web,
    );
    earlier.property_schema = vec![web_property("speed", serde_json::json!(1))];
    let later = earlier.clone();

    assert_eq!(
        active_property_update_sync_mode(Some(&earlier), &later, false),
        ActivePropertyUpdateSyncMode::Current
    );
}

#[test]
fn active_property_update_dispatch_uses_scene_runtime_patch_without_full_sync() {
    let dynamic_text_calls = Cell::new(0);
    let scene_runtime_calls = Cell::new(0);
    let full_sync_calls = Cell::new(0);

    let mode = dispatch_active_property_update_sync(
        ActivePropertyUpdateSyncMode::LightweightSceneRuntime,
        || {
            dynamic_text_calls.set(dynamic_text_calls.get() + 1);
            Ok(())
        },
        || {
            scene_runtime_calls.set(scene_runtime_calls.get() + 1);
            Ok(())
        },
        || {
            full_sync_calls.set(full_sync_calls.get() + 1);
            Ok(())
        },
        || true,
    )
    .expect("scene runtime update should succeed");

    assert_eq!(mode, ActivePropertyUpdateSyncMode::LightweightSceneRuntime);
    assert_eq!(dynamic_text_calls.get(), 0);
    assert_eq!(scene_runtime_calls.get(), 1);
    assert_eq!(full_sync_calls.get(), 0);
}

#[test]
fn scene_update_loop_starts_only_for_dynamic_scene_content() {
    let static_scene = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                BTreeMap::from([(7, text_object(SceneTextBehavior::Static))]),
                vec![source_text_layer(SceneTextBehavior::Static, None, None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );
    let clock_scene = runtime_record(
        WallpaperRuntime::Scene {
            scene: scene_runtime_with_objects(
                BTreeMap::from([(7, text_object(SceneTextBehavior::Clock))]),
                vec![source_text_layer(SceneTextBehavior::Clock, Some(true), None)],
                Utc::now(),
            ),
        },
        WallpaperType::Scene,
    );

    let static_runtime = match &static_scene.runtime {
        WallpaperRuntime::Scene { scene } => scene,
        _ => unreachable!(),
    };
    let clock_runtime = match &clock_scene.runtime {
        WallpaperRuntime::Scene { scene } => scene,
        _ => unreachable!(),
    };

    assert!(!scene_requires_periodic_updates(static_runtime));
    assert!(scene_requires_periodic_updates(clock_runtime));
    assert!(!should_start_scene_update_loop(&static_scene));
    assert!(should_start_scene_update_loop(&clock_scene));
}

#[test]
fn apply_time_record_resolution_generates_missing_web_snapshot() {
    let _lock = HOME_ENV_LOCK.lock().expect("home lock");
    let temp = tempdir().expect("temp dir");
    let previous_home = env::var_os("HOME");
    env::set_var("HOME", temp.path());

    let result = (|| {
        let managed_root = temp.path().join("managed-web");
        let source_root = managed_root.join("source");
        fs::create_dir_all(&source_root).expect("source dir");
        let entry = source_root.join("index.html");
        fs::write(&entry, "<html><body>Web</body></html>").expect("entry");
        let snapshot = managed_root.join("snapshot.png");

        let state = app_state(DynamicPlayerState::default());
        {
            let mut library = state.library.lock().expect("library lock");
            library.wallpapers.push(web_record(
                "web-demo",
                managed_root.to_str().expect("managed root"),
                entry.display().to_string(),
            ));
        }

        let record = ensure_apply_record_current_by_id_with(&state, "web-demo", |record| {
            fs::write(&snapshot, b"snapshot").expect("snapshot");
            record.last_snapshot_path = Some(snapshot.display().to_string());
            crate::services::static_snapshot_generation_service::StaticSnapshotGenerationOutcome::Generated {
                snapshot_path: snapshot.clone(),
            }
        })
        .expect("apply record");

        assert_eq!(
            record.last_snapshot_path.as_deref(),
            Some(snapshot.to_str().expect("snapshot path"))
        );
        let saved = fs::read_to_string(
            temp.path()
                .join("Library/Application Support/WallpaperWorkbench/library.json"),
        )
        .expect("saved library");
        assert!(saved.contains(snapshot.to_str().expect("snapshot path")));
    })();

    match previous_home {
        Some(home) => env::set_var("HOME", home),
        None => env::remove_var("HOME"),
    }

    result
}

#[test]
fn apply_time_record_resolution_generates_missing_scene_snapshot() {
    let _lock = HOME_ENV_LOCK.lock().expect("home lock");
    let temp = tempdir().expect("temp dir");
    let previous_home = env::var_os("HOME");
    env::set_var("HOME", temp.path());

    let result = (|| {
        let managed_root = temp.path().join("managed-scene");
        let source_root = managed_root.join("source");
        fs::create_dir_all(&source_root).expect("source dir");
        fs::write(
            source_root.join("project.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "title": "Scene Demo",
                "type": "scene",
                "file": "scene.json"
            }))
            .expect("project json"),
        )
        .expect("project");
        fs::write(
            source_root.join("scene.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "general": { "orthogonalprojection": { "width": 200, "height": 120 } },
                "objects": []
            }))
            .expect("scene json"),
        )
        .expect("scene");
        let snapshot = managed_root.join("snapshot.png");

        let state = app_state(DynamicPlayerState::default());
        {
            let mut library = state.library.lock().expect("library lock");
            library
                .wallpapers
                .push(scene_record(managed_root.to_str().expect("managed root")));
        }

        let record = ensure_apply_record_current_by_id_with(&state, "scene-demo", |record| {
            fs::write(&snapshot, b"snapshot").expect("snapshot");
            record.last_snapshot_path = Some(snapshot.display().to_string());
            crate::services::static_snapshot_generation_service::StaticSnapshotGenerationOutcome::Generated {
                snapshot_path: snapshot.clone(),
            }
        })
        .expect("apply record");

        assert_eq!(
            record.last_snapshot_path.as_deref(),
            Some(snapshot.to_str().expect("snapshot path"))
        );
        let saved = fs::read_to_string(
            temp.path()
                .join("Library/Application Support/WallpaperWorkbench/library.json"),
        )
        .expect("saved library");
        assert!(saved.contains(snapshot.to_str().expect("snapshot path")));
    })();

    match previous_home {
        Some(home) => env::set_var("HOME", home),
        None => env::remove_var("HOME"),
    }

    result
}

#[test]
fn scene_update_cadence_prefers_second_precision_then_media_then_minute() {
    let second_runtime = scene_runtime_with_objects(
        BTreeMap::from([(7, text_object(SceneTextBehavior::Clock))]),
        vec![source_text_layer(SceneTextBehavior::Clock, Some(true), None)],
        Utc::now(),
    );
    let media_runtime = scene_runtime_with_objects(
        BTreeMap::from([(7, text_object(SceneTextBehavior::MediaTitle))]),
        vec![source_text_layer(SceneTextBehavior::MediaTitle, None, None)],
        Utc::now(),
    );
    let minute_runtime = scene_runtime_with_objects(
        BTreeMap::from([(7, text_object(SceneTextBehavior::Date))]),
        vec![source_text_layer(SceneTextBehavior::Date, None, None)],
        Utc::now(),
    );

    assert_eq!(
        scene_update_cadence(&second_runtime),
        Some(SceneUpdateCadence::Second)
    );
    assert_eq!(
        scene_update_cadence(&media_runtime),
        Some(SceneUpdateCadence::CustomMillis(
            scene_now_playing_provider_service::DEFAULT_NOW_PLAYING_REFRESH_INTERVAL_MILLIS
        ))
    );
    assert_eq!(
        scene_update_cadence(&minute_runtime),
        Some(SceneUpdateCadence::Minute)
    );
}

#[test]
fn scene_update_cadence_prefers_script_refresh_interval_when_present() {
    let custom_runtime = scene_runtime_with_objects(
        BTreeMap::from([(7, text_object(SceneTextBehavior::Clock))]),
        vec![source_text_layer(
            SceneTextBehavior::Clock,
            Some(true),
            Some(1500),
        )],
        Utc::now(),
    );

    assert_eq!(
        scene_update_cadence(&custom_runtime),
        Some(SceneUpdateCadence::CustomMillis(1500))
    );
}

#[test]
fn scene_update_cadence_tracks_time_aware_script_text_layers() {
    let mut script_layer = source_text_layer(SceneTextBehavior::Script, None, None);
    script_layer.script_text =
        Some("'use strict'; export function update() { return new Date().getHours().toString(); }"
            .to_string());
    let script_runtime = scene_runtime_with_objects(
        BTreeMap::from([(7, text_object(SceneTextBehavior::Script))]),
        vec![script_layer],
        Utc::now(),
    );

    assert_eq!(
        scene_update_cadence(&script_runtime),
        Some(SceneUpdateCadence::Minute)
    );
}

#[test]
fn manual_and_auto_pause_state_remain_isolated() {
    let mut player = DynamicPlayerState {
        active_id: Some("demo".to_string()),
        ..DynamicPlayerState::default()
    };

    let auto_pause = apply_pause_change(&mut player, None, Some(BTreeSet::from([String::from("player")])));
    assert!(auto_pause.effective_paused);
    assert!(auto_pause.effective_changed);
    assert!(!auto_pause.manual_paused);

    let manual_pause = apply_pause_change(&mut player, Some(true), None);
    assert!(manual_pause.effective_paused);
    assert!(!manual_pause.effective_changed);
    assert!(manual_pause.manual_changed);
    assert!(manual_pause.manual_paused);

    let auto_resume = apply_pause_change(&mut player, None, Some(BTreeSet::new()));
    assert!(auto_resume.effective_paused);
    assert!(!auto_resume.effective_changed);
    assert!(auto_resume.manual_paused);

    let manual_resume = apply_pause_change(&mut player, Some(false), None);
    assert!(!manual_resume.effective_paused);
    assert!(manual_resume.effective_changed);
    assert!(manual_resume.manual_changed);
    assert!(!manual_resume.manual_paused);
}

#[test]
fn pause_flags_reset_when_no_active_wallpaper_exists() {
    let mut player = DynamicPlayerState {
        active_id: None,
        manually_paused: true,
        auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
        scene_update_generation: 0,
        last_scene_signature: None,
    };

    let transition = apply_pause_change(&mut player, Some(true), None);
    assert!(!transition.effective_paused);
    assert!(transition.effective_changed);
    assert!(transition.manual_changed);
    assert!(!player.manually_paused);
    assert!(player.auto_pause_screen_labels.is_empty());
}

#[test]
fn inactive_native_hosts_only_fail_best_effort_sync() {
    let scene = runtime_record(
        WallpaperRuntime::Scene {
            scene: Default::default(),
        },
        WallpaperType::Scene,
    );
    let video = runtime_record(
        WallpaperRuntime::Video {
            video: Default::default(),
        },
        WallpaperType::Video,
    );
    let web = runtime_record(
        WallpaperRuntime::Web {
            web: Default::default(),
        },
        WallpaperType::Web,
    );

    assert_eq!(
        native_host_sync_disposition(Some(&scene), NativeHostKind::Scene),
        NativeHostSyncDisposition::Critical
    );
    assert_eq!(
        native_host_sync_disposition(Some(&scene), NativeHostKind::Video),
        NativeHostSyncDisposition::BestEffort
    );
    assert_eq!(
        native_host_sync_disposition(Some(&scene), NativeHostKind::Web),
        NativeHostSyncDisposition::BestEffort
    );
    assert_eq!(
        native_host_sync_disposition(Some(&video), NativeHostKind::Video),
        NativeHostSyncDisposition::Critical
    );
    assert_eq!(
        native_host_sync_disposition(Some(&video), NativeHostKind::Web),
        NativeHostSyncDisposition::BestEffort
    );
    assert_eq!(
        native_host_sync_disposition(Some(&web), NativeHostKind::Video),
        NativeHostSyncDisposition::BestEffort
    );
    assert_eq!(
        native_host_sync_disposition(Some(&web), NativeHostKind::Web),
        NativeHostSyncDisposition::Critical
    );
    assert_eq!(
        native_host_sync_disposition(None, NativeHostKind::Video),
        NativeHostSyncDisposition::BestEffort
    );
}

#[test]
fn critical_apply_failure_keeps_previous_player_state_and_skips_persist() {
    let previous_player = DynamicPlayerState {
        active_id: Some("known-good".to_string()),
        manually_paused: true,
        auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
        scene_update_generation: 4,
        last_scene_signature: Some("previous-signature".to_string()),
    };
    let previous_runtime = runtime_record(
        WallpaperRuntime::Web {
            web: Default::default(),
        },
        WallpaperType::Web,
    );
    let state = app_state(previous_player.clone());
    let candidate_runtime = runtime_record(
        WallpaperRuntime::Video {
            video: Default::default(),
        },
        WallpaperType::Video,
    );
    let snapshot_calls = RefCell::new(0usize);
    let sync_calls = RefCell::new(Vec::new());
    let persist_calls = RefCell::new(Vec::new());

    let result = apply_runtime_record_transaction(
        &candidate_runtime,
        &previous_player,
        Some((previous_runtime.clone(), previous_player.effective_paused())),
        &state,
        true,
        || {
            *snapshot_calls.borrow_mut() += 1;
            Ok(())
        },
        || Ok(()),
        |runtime_record, paused| {
            sync_calls
                .borrow_mut()
                .push((runtime_record.map(|record| record.id.clone()), paused));
            if runtime_record.map(|record| record.id.as_str())
                == Some(candidate_runtime.id.as_str())
            {
                Err("native video runtime failed".to_string())
            } else {
                Ok(())
            }
        },
        || Ok(()),
        |player| {
            persist_calls.borrow_mut().push(player.active_id.clone());
            Ok(())
        },
    );

    assert!(result.is_err());
    assert_eq!(*snapshot_calls.borrow(), 0);
    assert_eq!(
        sync_calls.into_inner(),
        vec![
            (
                Some(candidate_runtime.id.clone()),
                build_apply_wallpaper_candidate(&previous_player, &candidate_runtime)
                    .effective_paused,
            ),
            (
                Some(previous_runtime.id.clone()),
                previous_player.effective_paused(),
            ),
        ]
    );
    assert!(persist_calls.into_inner().is_empty());
    let player = state.player.lock().expect("lock player").clone();
    assert_eq!(player.active_id, previous_player.active_id);
    assert_eq!(player.manually_paused, previous_player.manually_paused);
    assert_eq!(
        player.auto_pause_screen_labels,
        previous_player.auto_pause_screen_labels
    );
    assert_eq!(
        player.scene_update_generation,
        previous_player.scene_update_generation
    );
    assert_eq!(
        player.last_scene_signature,
        previous_player.last_scene_signature
    );
}

#[test]
fn apply_transaction_blocks_until_runtime_sync_released() {
    let previous_player = DynamicPlayerState {
        active_id: Some("known-good".to_string()),
        manually_paused: false,
        auto_pause_screen_labels: BTreeSet::new(),
        scene_update_generation: 4,
        last_scene_signature: Some("previous-signature".to_string()),
    };
    let state = Arc::new(app_state(previous_player.clone()));
    let candidate_runtime = runtime_record(
        WallpaperRuntime::Scene {
            scene: Default::default(),
        },
        WallpaperType::Scene,
    );

    let state_clone = state.clone();
    let released = Arc::new(AtomicBool::new(false));
    let released_clone = released.clone();
    let holder_ready = Arc::new(Barrier::new(2));
    let holder_ready_clone = holder_ready.clone();

    let handle = thread::spawn(move || {
        let guard = state_clone.runtime_sync.lock().expect("runtime sync lock");
        holder_ready_clone.wait();
        thread::sleep(Duration::from_millis(20));
        drop(guard);
        released_clone.store(true, Ordering::SeqCst);
    });

    holder_ready.wait();

    let snapshot_called = RefCell::new(false);
    let windows_called = RefCell::new(false);
    let native_sync_called = RefCell::new(false);
    let persist_called = RefCell::new(false);

    let result = apply_runtime_record_transaction(
        &candidate_runtime,
        &previous_player,
        None,
        &state,
        true,
        || {
            *snapshot_called.borrow_mut() = true;
            Ok(())
        },
        || {
            *windows_called.borrow_mut() = true;
            Ok(())
        },
        |_runtime_record, _paused| {
            *native_sync_called.borrow_mut() = true;
            Ok(())
        },
        || Ok(()),
        |_player| {
            *persist_called.borrow_mut() = true;
            Ok(())
        },
    );

    handle.join().expect("holder thread should complete");

    assert!(released.load(Ordering::SeqCst));
    assert!(
        result.is_ok(),
        "apply should succeed after lock released: {:?}",
        result.err()
    );
    assert!(*snapshot_called.borrow());
    assert!(*windows_called.borrow());
    assert!(*native_sync_called.borrow());
    assert!(*persist_called.borrow());
    let player = state.player.lock().expect("player lock").clone();
    assert_eq!(
        player.active_id.as_deref(),
        Some(candidate_runtime.id.as_str())
    );
}

#[test]
fn apply_transaction_syncs_static_snapshot_after_native_runtime_succeeds() {
    let previous_player = DynamicPlayerState {
        active_id: Some("old-wallpaper".to_string()),
        manually_paused: true,
        auto_pause_screen_labels: BTreeSet::new(),
        scene_update_generation: 11,
        last_scene_signature: Some("old-signature".to_string()),
    };
    let state = app_state(previous_player.clone());
    let mut candidate_runtime = runtime_record(
        WallpaperRuntime::Video {
            video: Default::default(),
        },
        WallpaperType::Video,
    );
    candidate_runtime.id = "new-wallpaper".to_string();
    let observed_sync_state = RefCell::new(Vec::new());
    let observed_static_state = RefCell::new(Vec::new());
    let order = RefCell::new(Vec::new());

    let result = apply_runtime_record_transaction(
        &candidate_runtime,
        &previous_player,
        None,
        &state,
        true,
        || {
            let player = state.player.lock().expect("player lock").clone();
            observed_static_state
                .borrow_mut()
                .push((player.active_id, player.scene_update_generation));
            order.borrow_mut().push("static");
            Ok(())
        },
        || {
            order.borrow_mut().push("windows");
            Ok(())
        },
        |runtime_record, paused| {
            order.borrow_mut().push("native");
            let player = state.player.lock().expect("player lock").clone();
            observed_sync_state.borrow_mut().push((
                runtime_record.map(|record| record.id.clone()),
                paused,
                player.active_id,
                player.scene_update_generation,
                player.manually_paused,
            ));
            Ok(())
        },
        || Ok(()),
        |_| Ok(()),
    );

    assert_eq!(result, Ok(false));
    assert_eq!(
        observed_static_state.into_inner(),
        vec![(Some("new-wallpaper".to_string()), 12)]
    );
    assert_eq!(order.into_inner(), vec!["windows", "native", "static"]);
    assert_eq!(
        observed_sync_state.into_inner(),
        vec![(
            Some("new-wallpaper".to_string()),
            false,
            Some("new-wallpaper".to_string()),
            12,
            false,
        )]
    );
    let player = state.player.lock().expect("player lock").clone();
    assert_eq!(player.active_id.as_deref(), Some("new-wallpaper"));
    assert_eq!(player.scene_update_generation, 12);
}

#[test]
fn static_sync_runs_after_native_window_reuse() {
    let previous_player = DynamicPlayerState {
        active_id: Some("old-wallpaper".to_string()),
        manually_paused: false,
        auto_pause_screen_labels: BTreeSet::new(),
        scene_update_generation: 2,
        last_scene_signature: None,
    };
    let state = app_state(previous_player.clone());
    let mut candidate_runtime = runtime_record(
        WallpaperRuntime::Scene {
            scene: Default::default(),
        },
        WallpaperType::Scene,
    );
    candidate_runtime.id = "new-wallpaper".to_string();
    let live_window_labels = RefCell::new(vec!["player".to_string()]);
    let events = RefCell::new(Vec::new());

    let result = apply_runtime_record_transaction(
        &candidate_runtime,
        &previous_player,
        None,
        &state,
        true,
        || {
            events.borrow_mut().push("static-sync");
            Ok(())
        },
        || {
            events.borrow_mut().push("ensure-player-windows");
            assert_eq!(
                live_window_labels.borrow().as_slice(),
                ["player".to_string()]
            );
            Ok(())
        },
        |_runtime_record, _paused| {
            events.borrow_mut().push("native-sync");
            Ok(())
        },
        || Ok(()),
        |_| Ok(()),
    );

    assert_eq!(result, Ok(false));
    assert_eq!(live_window_labels.into_inner(), vec!["player"]);
    assert_eq!(
        events.into_inner(),
        vec!["ensure-player-windows", "native-sync", "static-sync"]
    );
}

#[test]
fn restore_transaction_skips_static_snapshot_when_native_sync_fails() {
    let runtime = runtime_record(
        WallpaperRuntime::Web {
            web: Default::default(),
        },
        WallpaperType::Web,
    );
    let events = RefCell::new(Vec::new());

    let result = restore_runtime_record_transaction(
        &runtime,
        true,
        || {
            events.borrow_mut().push("ensure-player-windows");
            Ok(())
        },
        |runtime_record, paused| {
            events.borrow_mut().push("native-sync");
            assert_eq!(
                runtime_record.map(|record| record.id.as_str()),
                Some(runtime.id.as_str())
            );
            assert!(paused);
            Err("native web runtime failed".to_string())
        },
        || {
            events.borrow_mut().push("static-sync");
            Ok(())
        },
    );

    assert_eq!(result, Err("native web runtime failed".to_string()));
    assert_eq!(
        events.into_inner(),
        vec!["ensure-player-windows", "native-sync"]
    );
}

#[test]
fn repeated_apply_transactions_keep_one_player_window_label() {
    let initial_player = DynamicPlayerState {
        active_id: Some("first-wallpaper".to_string()),
        manually_paused: false,
        auto_pause_screen_labels: BTreeSet::new(),
        scene_update_generation: 4,
        last_scene_signature: None,
    };
    let state = app_state(initial_player.clone());
    let live_window_labels = RefCell::new(vec!["player".to_string()]);
    let ensure_count = RefCell::new(0usize);

    for wallpaper_id in ["second-wallpaper", "third-wallpaper"] {
        let previous_player = state.player.lock().expect("player lock").clone();
        let mut runtime = runtime_record(
            WallpaperRuntime::Video {
                video: Default::default(),
            },
            WallpaperType::Video,
        );
        runtime.id = wallpaper_id.to_string();

        let result = apply_runtime_record_transaction(
            &runtime,
            &previous_player,
            None,
            &state,
            true,
            || Ok(()),
            || {
                *ensure_count.borrow_mut() += 1;
                assert_eq!(
                    live_window_labels.borrow().as_slice(),
                    ["player".to_string()]
                );
                Ok(())
            },
            |_runtime_record, _paused| Ok(()),
            || Ok(()),
            |_| Ok(()),
        );

        assert_eq!(result, Ok(false));
        assert_eq!(
            live_window_labels.borrow().as_slice(),
            ["player".to_string()]
        );
    }

    assert_eq!(*ensure_count.borrow(), 2);
    let player = state.player.lock().expect("player lock").clone();
    assert_eq!(player.active_id.as_deref(), Some("third-wallpaper"));
}

#[test]
fn scene_update_sync_guard_rejects_stale_active_state() {
    let state = app_state(DynamicPlayerState {
        active_id: Some("new-wallpaper".to_string()),
        manually_paused: false,
        auto_pause_screen_labels: BTreeSet::new(),
        scene_update_generation: 8,
        last_scene_signature: None,
    });

    assert!(scene_update_sync_is_current(&state, 8, "new-wallpaper"));
    assert!(!scene_update_sync_is_current(&state, 7, "new-wallpaper"));
    assert!(!scene_update_sync_is_current(&state, 8, "old-wallpaper"));
}

#[test]
fn inactive_host_sync_errors_do_not_accumulate_as_critical_failures() {
    let scene = runtime_record(
        WallpaperRuntime::Scene {
            scene: Default::default(),
        },
        WallpaperType::Scene,
    );
    let video = runtime_record(
        WallpaperRuntime::Video {
            video: Default::default(),
        },
        WallpaperType::Video,
    );
    let mut critical_errors = Vec::new();

    push_critical_sync_error(
        &mut critical_errors,
        Some(&scene),
        NativeHostKind::Scene,
        "scene host failed".to_string(),
    );
    assert_eq!(
        critical_errors,
        vec!["native scene runtime failed: scene host failed".to_string()]
    );

    critical_errors.clear();
    push_critical_sync_error(
        &mut critical_errors,
        Some(&scene),
        NativeHostKind::Video,
        "cleanup failed".to_string(),
    );
    assert!(critical_errors.is_empty());

    push_critical_sync_error(
        &mut critical_errors,
        Some(&video),
        NativeHostKind::Video,
        "start failed".to_string(),
    );
    assert_eq!(
        critical_errors,
        vec!["native video runtime failed: start failed".to_string()]
    );
}

#[test]
fn clearing_player_session_state_persists_empty_restore_state_for_restart() {
    let _lock = HOME_ENV_LOCK.lock().expect("home lock");
    let temp = tempdir().expect("temp dir");
    let previous_home = env::var_os("HOME");
    env::set_var("HOME", temp.path());

    let result = (|| {
        let state = app_state(DynamicPlayerState {
            active_id: Some("broken".to_string()),
            manually_paused: true,
            auto_pause_screen_labels: BTreeSet::from([String::from("player")]),
            scene_update_generation: 7,
            last_scene_signature: Some("scene-signature".to_string()),
        });

        let cleared = clear_player_session_state(&state).expect("clear player state");
        assert!(cleared.active_id.is_none());
        assert!(!cleared.manually_paused);
        assert!(cleared.auto_pause_screen_labels.is_empty());
        assert_eq!(cleared.scene_update_generation, 8);
        assert!(cleared.last_scene_signature.is_none());

        let reloaded = AppState::load().expect("reload state after persisted clear");
        let player = reloaded.player.lock().expect("player lock").clone();
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
fn unsupported_scene_apply_preflight_returns_explicit_error() {
    let temp = tempdir().expect("temp dir");
    let managed_root = temp.path().join("managed");
    let builtin_root = temp.path().join("builtin");
    let extracted_root = managed_root.join("extracted");

    fs::create_dir_all(extracted_root.join("models")).expect("models dir");
    fs::create_dir_all(extracted_root.join("materials")).expect("materials dir");
    fs::create_dir_all(&builtin_root).expect("builtin dir");
    fs::write(
        extracted_root.join("scene.json"),
        r#"{"objects":[{"id":1,"name":"Hero","image":"models/hero.json"}]}"#,
    )
    .expect("scene json");
    fs::write(
        extracted_root.join("models").join("hero.json"),
        r#"{"material":"materials/hero.material"}"#,
    )
    .expect("hero model");
    fs::write(
        extracted_root.join("materials").join("hero.material"),
        r#"{"passes":[{"shader":"shaders/hero.frag"}]}"#,
    )
    .expect("hero material");

    let mut record = scene_record(&managed_root.display().to_string());
    record.scene_manifest = Some(
        crate::scene::parse_scene_manifest(
            &extracted_root.join("scene.json"),
            &extracted_root,
            &BTreeMap::new(),
        )
        .expect("parse scene manifest"),
    );

    let error = validate_scene_apply_preflight(&record, &builtin_root)
        .expect_err("unsupported scene should be blocked");

    assert!(error.contains("Scene native apply is blocked in the native Scene runtime"));
    assert!(error.contains("no renderable output"));
}
