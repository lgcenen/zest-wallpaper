use std::{
    collections::BTreeMap,
    thread,
    time::Duration,
};

use chrono::{Local, Timelike};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{
    models::{
        EvaluatedSceneCamera, EvaluatedSceneObject, SceneManifest, SceneParallax,
        SceneRuntimeDocument, SceneTextBehavior, WallpaperRuntime, WallpaperRuntimeRecord,
    },
    services::{
        audio_input_service, native_video_service, native_web_service,
        scene_native_renderer_service, scene_now_playing_provider_service,
        scene_text_behavior_service, window_service,
    },
    store::AppState,
};

use super::{apply_flow, runtime_document_service};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeHostKind {
    Scene,
    Video,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeHostSyncDisposition {
    Critical,
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SceneUpdateCadence {
    CustomMillis(u64),
    Minute,
    TwoSeconds,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SceneUpdateSyncMode {
    Current,
    LightweightDynamicText,
    FullNativeSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivePropertyUpdateSyncMode {
    Current,
    LightweightDynamicText,
    LightweightSceneRuntime,
    FullNativeSync,
}

impl From<scene_text_behavior_service::SceneTextRefreshCadence> for SceneUpdateCadence {
    fn from(cadence: scene_text_behavior_service::SceneTextRefreshCadence) -> Self {
        match cadence {
            scene_text_behavior_service::SceneTextRefreshCadence::CustomMillis(interval_millis) => {
                SceneUpdateCadence::CustomMillis(interval_millis)
            }
            scene_text_behavior_service::SceneTextRefreshCadence::Minute => {
                SceneUpdateCadence::Minute
            }
            scene_text_behavior_service::SceneTextRefreshCadence::TwoSeconds => {
                SceneUpdateCadence::TwoSeconds
            }
            scene_text_behavior_service::SceneTextRefreshCadence::Second => {
                SceneUpdateCadence::Second
            }
        }
    }
}

#[derive(Serialize)]
struct SceneEvaluationSignature<'a> {
    version: u8,
    structural: SceneStructuralSignature<'a>,
    dynamic_text: Vec<SceneDynamicTextSignature<'a>>,
}

#[derive(Serialize)]
struct SceneStructuralSignature<'a> {
    canvas_width: f64,
    canvas_height: f64,
    clear_color: &'a Option<String>,
    camera: &'a EvaluatedSceneCamera,
    parallax: &'a SceneParallax,
    source: &'a SceneManifest,
    objects: BTreeMap<u32, SceneStructuralObjectSignature<'a>>,
    render_list: &'a [u32],
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SceneStructuralObjectSignature<'a> {
    Text {
        id: u32,
        name: &'a str,
        parent_id: &'a Option<u32>,
        dependencies: &'a [u32],
        visible: bool,
        opacity: f64,
        behavior: &'a SceneTextBehavior,
    },
    NonText {
        object: &'a EvaluatedSceneObject,
    },
}

#[derive(Serialize)]
struct SceneDynamicTextSignature<'a> {
    object_id: u32,
    object: &'a EvaluatedSceneObject,
}

pub(crate) fn sync_active_wallpaper_after_property_update(
    app: &AppHandle,
    state: &AppState,
    previous_runtime_record: Option<&WallpaperRuntimeRecord>,
    runtime_record: &WallpaperRuntimeRecord,
    requires_metadata_refresh: bool,
) -> Result<(), String> {
    {
        let player = state.player.lock().map_err(|error| error.to_string())?;
        if player.active_id.as_deref() != Some(runtime_record.id.as_str()) {
            return Ok(());
        }
    }

    let sync_mode = active_property_update_sync_mode(
        previous_runtime_record,
        runtime_record,
        requires_metadata_refresh,
    );
    let next_signature = scene_signature(runtime_record);
    if sync_mode != ActivePropertyUpdateSyncMode::Current {
        set_active_scene_signature(state, runtime_record.id.as_str(), next_signature.clone())?;
    }

    dispatch_active_property_update_sync(
        sync_mode,
        || sync_scene_dynamic_text_update_for_runtime_record(app, state, runtime_record),
        || sync_scene_runtime_update_for_runtime_record(app, state, runtime_record),
        || sync_native_runtime_for_active_wallpaper(app, state),
        || {
            scene_update_sync_is_current_for_signature(
                state,
                runtime_record.id.as_str(),
                next_signature.as_deref(),
            )
        },
    )?;
    Ok(())
}

fn set_active_scene_signature(
    state: &AppState,
    active_id: &str,
    signature: Option<String>,
) -> Result<(), String> {
    let mut player = state.player.lock().map_err(|error| error.to_string())?;
    if player.active_id.as_deref() == Some(active_id) {
        player.last_scene_signature = signature;
    }
    Ok(())
}

pub(crate) fn sync_native_runtime_for_active_wallpaper(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let _ = state;
        return dispatch2::run_on_main(move |_mtm| {
            let state = app.state::<AppState>();
            let _runtime_sync = state.runtime_sync.lock().map_err(|error| error.to_string())?;
            match active_runtime_snapshot_for_runtime_sync(&app, &state)? {
                Some((runtime_record, effective_paused)) => {
                    sync_native_runtime(&app, Some(&runtime_record), effective_paused)
                }
                None => sync_native_runtime(&app, None, false),
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _runtime_sync = state.runtime_sync.lock().map_err(|error| error.to_string())?;
        match active_runtime_snapshot_for_runtime_sync(app, state)? {
            Some((runtime_record, effective_paused)) => {
                sync_native_runtime(app, Some(&runtime_record), effective_paused)
            }
            None => sync_native_runtime(app, None, false),
        }
    }
}

fn sync_scene_dynamic_text_update_for_runtime_record(
    app: &AppHandle,
    state: &AppState,
    runtime_record: &WallpaperRuntimeRecord,
) -> Result<(), String> {
    let _runtime_sync = state.runtime_sync.lock().map_err(|error| error.to_string())?;
    {
        let player = state.player.lock().map_err(|error| error.to_string())?;
        if player.active_id.as_deref() != Some(runtime_record.id.as_str()) {
            return Err(format!(
                "native Scene dynamic text update targeted {}, but it is no longer active",
                runtime_record.id
            ));
        }
    }
    scene_native_renderer_service::update_native_scene_dynamic_text(app, runtime_record)
}

fn sync_scene_runtime_update_for_runtime_record(
    app: &AppHandle,
    state: &AppState,
    runtime_record: &WallpaperRuntimeRecord,
) -> Result<(), String> {
    let _runtime_sync = state.runtime_sync.lock().map_err(|error| error.to_string())?;
    let effective_paused = {
        let (active_id, paused) = active_runtime_pause_snapshot(app, state)?;
        if active_id.as_deref() != Some(runtime_record.id.as_str()) {
            return Err(format!(
                "native Scene property update targeted {}, but it is no longer active",
                runtime_record.id
            ));
        }
        paused
    };
    scene_native_renderer_service::sync_native_scene_runtime(
        app,
        Some(runtime_record),
        effective_paused,
    )
}

pub(super) fn sync_native_runtime_with_transaction_lock(
    app: &AppHandle,
    state: &AppState,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let app = app.clone();
        let runtime_record = runtime_record.cloned();
        let _ = state;
        return dispatch2::run_on_main(move |_mtm| {
            let state = app.state::<AppState>();
            let _runtime_sync = state.runtime_sync.lock().map_err(|error| error.to_string())?;
            sync_native_runtime(&app, runtime_record.as_ref(), paused)
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _runtime_sync = state.runtime_sync.lock().map_err(|error| error.to_string())?;
        sync_native_runtime(app, runtime_record, paused)
    }
}

pub(super) fn sync_native_runtime(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    let mut critical_errors = Vec::new();

    if let Err(error) =
        scene_native_renderer_service::sync_native_scene_runtime(app, runtime_record, paused)
    {
        push_critical_sync_error(
            &mut critical_errors,
            runtime_record,
            NativeHostKind::Scene,
            error,
        );
    }

    if let Err(error) =
        native_video_service::sync_native_video_playback(app, runtime_record, paused)
    {
        push_critical_sync_error(
            &mut critical_errors,
            runtime_record,
            NativeHostKind::Video,
            error,
        );
    }

    if let Err(error) = native_web_service::sync_native_web_runtime(app, runtime_record, paused) {
        push_critical_sync_error(
            &mut critical_errors,
            runtime_record,
            NativeHostKind::Web,
            error,
        );
    }

    audio_input_service::request_policy_refresh(app);
    if critical_errors.is_empty() {
        Ok(())
    } else {
        Err(critical_errors.join("; "))
    }
}

pub(super) fn start_scene_update_loop(app: AppHandle, state: &AppState) {
    let generation = state
        .player
        .lock()
        .map(|player| player.scene_update_generation)
        .unwrap_or_default();

    thread::spawn(move || loop {
        let state = app.state::<AppState>();
        let active_id = {
            let player = match state.player.lock() {
                Ok(player) => player,
                Err(_) => return,
            };
            if player.scene_update_generation != generation {
                return;
            }
            match player.active_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        let record = {
            let store = match state.library.lock() {
                Ok(store) => store,
                Err(_) => return,
            };
            match crate::store::find_record(&store, &active_id) {
                Some(record) => record,
                None => return,
            }
        };

        let sleep_for = match runtime_document_service::runtime_record(&record).runtime {
            WallpaperRuntime::Scene { scene } => {
                scene_update_sleep_duration(&scene, Local::now()).unwrap_or(Duration::from_secs(1))
            }
            _ => return,
        };
        thread::sleep(sleep_for);

        let runtime_record = {
            let store = match state.library.lock() {
                Ok(store) => store,
                Err(_) => return,
            };
            let Some(record) = crate::store::find_record(&store, &active_id) else {
                return;
            };
            runtime_document_service::runtime_record(&record)
        };
        let signature = scene_signature(&runtime_record);
        if signature.is_none() {
            return;
        }
        let sync_mode = {
            let mut player = match state.player.lock() {
                Ok(player) => player,
                Err(_) => return,
            };
            if player.scene_update_generation != generation
                || player.active_id.as_deref() != Some(active_id.as_str())
            {
                return;
            }
            match scene_update_sync_mode(
                player.last_scene_signature.as_deref(),
                signature.as_deref(),
            ) {
                SceneUpdateSyncMode::Current => SceneUpdateSyncMode::Current,
                mode => {
                    player.last_scene_signature = signature.clone();
                    mode
                }
            }
        };

        if sync_mode != SceneUpdateSyncMode::Current {
            if !scene_update_sync_is_current(&state, generation, active_id.as_str()) {
                return;
            }
            let _ = dispatch_scene_update_sync(
                sync_mode,
                || sync_scene_dynamic_text_update_for_runtime_record(&app, &state, &runtime_record),
                || sync_native_runtime_for_active_wallpaper(&app, &state),
                || scene_update_sync_is_current(&state, generation, active_id.as_str()),
            );
        }
    });
}

pub(super) fn scene_update_sync_is_current(
    state: &AppState,
    generation: u64,
    active_id: &str,
) -> bool {
    state
        .player
        .lock()
        .map(|player| {
            player.scene_update_generation == generation
                && player.active_id.as_deref() == Some(active_id)
        })
        .unwrap_or(false)
}

fn scene_update_sync_is_current_for_signature(
    state: &AppState,
    active_id: &str,
    signature: Option<&str>,
) -> bool {
    state
        .player
        .lock()
        .map(|player| {
            player.active_id.as_deref() == Some(active_id)
                && player.last_scene_signature.as_deref() == signature
        })
        .unwrap_or(false)
}

pub(super) fn dispatch_scene_update_sync<LightweightSync, FullSync, IsCurrent>(
    sync_mode: SceneUpdateSyncMode,
    mut lightweight_sync: LightweightSync,
    mut full_sync: FullSync,
    mut is_current: IsCurrent,
) -> Result<SceneUpdateSyncMode, String>
where
    LightweightSync: FnMut() -> Result<(), String>,
    FullSync: FnMut() -> Result<(), String>,
    IsCurrent: FnMut() -> bool,
{
    match sync_mode {
        SceneUpdateSyncMode::Current => Ok(SceneUpdateSyncMode::Current),
        SceneUpdateSyncMode::LightweightDynamicText => match lightweight_sync() {
            Ok(()) => Ok(SceneUpdateSyncMode::LightweightDynamicText),
            Err(_) if is_current() => {
                full_sync()?;
                Ok(SceneUpdateSyncMode::FullNativeSync)
            }
            Err(error) => Err(error),
        },
        SceneUpdateSyncMode::FullNativeSync => {
            full_sync()?;
            Ok(SceneUpdateSyncMode::FullNativeSync)
        }
    }
}

pub(super) fn dispatch_active_property_update_sync<
    DynamicTextSync,
    SceneRuntimeSync,
    FullSync,
    IsCurrent,
>(
    sync_mode: ActivePropertyUpdateSyncMode,
    mut dynamic_text_sync: DynamicTextSync,
    mut scene_runtime_sync: SceneRuntimeSync,
    mut full_sync: FullSync,
    mut is_current: IsCurrent,
) -> Result<ActivePropertyUpdateSyncMode, String>
where
    DynamicTextSync: FnMut() -> Result<(), String>,
    SceneRuntimeSync: FnMut() -> Result<(), String>,
    FullSync: FnMut() -> Result<(), String>,
    IsCurrent: FnMut() -> bool,
{
    match sync_mode {
        ActivePropertyUpdateSyncMode::Current => Ok(ActivePropertyUpdateSyncMode::Current),
        ActivePropertyUpdateSyncMode::LightweightDynamicText => match dynamic_text_sync() {
            Ok(()) => Ok(ActivePropertyUpdateSyncMode::LightweightDynamicText),
            Err(_) if is_current() => {
                full_sync()?;
                Ok(ActivePropertyUpdateSyncMode::FullNativeSync)
            }
            Err(error) => Err(error),
        },
        ActivePropertyUpdateSyncMode::LightweightSceneRuntime => match scene_runtime_sync() {
            Ok(()) => Ok(ActivePropertyUpdateSyncMode::LightweightSceneRuntime),
            Err(_) if is_current() => {
                full_sync()?;
                Ok(ActivePropertyUpdateSyncMode::FullNativeSync)
            }
            Err(error) => Err(error),
        },
        ActivePropertyUpdateSyncMode::FullNativeSync => {
            full_sync()?;
            Ok(ActivePropertyUpdateSyncMode::FullNativeSync)
        }
    }
}

fn active_runtime_snapshot_for_runtime_sync(
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<(WallpaperRuntimeRecord, bool)>, String> {
    let Some((record, _)) = apply_flow::active_record_snapshot(state)? else {
        return Ok(None);
    };
    let paused = active_runtime_pause_snapshot(app, state)?.1;
    Ok(Some((runtime_document_service::runtime_record(&record), paused)))
}

fn active_runtime_pause_snapshot(
    app: &AppHandle,
    state: &AppState,
) -> Result<(Option<String>, bool), String> {
    let labels = window_service::player_window_labels(app)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let player = state.player.lock().map_err(|error| error.to_string())?;
    Ok((
        player.active_id.clone(),
        player.effective_runtime_paused_for_labels(&labels),
    ))
}

pub(super) fn scene_signature(runtime_record: &WallpaperRuntimeRecord) -> Option<String> {
    match &runtime_record.runtime {
        WallpaperRuntime::Scene { scene } => {
            let (objects, dynamic_text) = scene_signature_objects(scene);
            serde_json::to_string(&SceneEvaluationSignature {
                version: 2,
                structural: SceneStructuralSignature {
                    canvas_width: scene.evaluated.canvas_width,
                    canvas_height: scene.evaluated.canvas_height,
                    clear_color: &scene.evaluated.clear_color,
                    camera: &scene.evaluated.camera,
                    parallax: &scene.evaluated.parallax,
                    source: &scene.source,
                    objects,
                    render_list: &scene.evaluated.render_list,
                },
                dynamic_text,
            })
            .ok()
        }
        _ => None,
    }
}

fn scene_signature_objects(
    scene: &SceneRuntimeDocument,
) -> (
    BTreeMap<u32, SceneStructuralObjectSignature<'_>>,
    Vec<SceneDynamicTextSignature<'_>>,
) {
    let mut structural = BTreeMap::new();
    let mut dynamic_text = Vec::new();

    for (object_id, object) in &scene.evaluated.objects {
        match object {
            EvaluatedSceneObject::Text {
                base,
                behavior,
                text: _,
            } => {
                structural.insert(
                    *object_id,
                    SceneStructuralObjectSignature::Text {
                        id: base.id,
                        name: &base.name,
                        parent_id: &base.parent_id,
                        dependencies: &base.dependencies,
                        visible: base.visible,
                        opacity: base.opacity,
                        behavior,
                    },
                );
                dynamic_text.push(SceneDynamicTextSignature { object_id: *object_id, object });
            }
            _ => {
                structural.insert(
                    *object_id,
                    SceneStructuralObjectSignature::NonText { object },
                );
            }
        }
    }

    (structural, dynamic_text)
}

pub(super) fn scene_update_sync_mode(
    previous_signature: Option<&str>,
    current_signature: Option<&str>,
) -> SceneUpdateSyncMode {
    let Some(current_signature) = current_signature else {
        return SceneUpdateSyncMode::Current;
    };
    let Some(previous_signature) = previous_signature else {
        return SceneUpdateSyncMode::FullNativeSync;
    };
    if previous_signature == current_signature {
        return SceneUpdateSyncMode::Current;
    }

    let Some(previous) = parse_scene_signature(previous_signature) else {
        return SceneUpdateSyncMode::FullNativeSync;
    };
    let Some(current) = parse_scene_signature(current_signature) else {
        return SceneUpdateSyncMode::FullNativeSync;
    };
    if previous.get("version") != Some(&serde_json::Value::from(2))
        || current.get("version") != Some(&serde_json::Value::from(2))
    {
        return SceneUpdateSyncMode::FullNativeSync;
    }

    if previous.get("structural") == current.get("structural")
        && previous.get("dynamic_text") != current.get("dynamic_text")
    {
        SceneUpdateSyncMode::LightweightDynamicText
    } else {
        SceneUpdateSyncMode::FullNativeSync
    }
}

pub(super) fn active_property_update_sync_mode(
    previous_runtime_record: Option<&WallpaperRuntimeRecord>,
    runtime_record: &WallpaperRuntimeRecord,
    requires_metadata_refresh: bool,
) -> ActivePropertyUpdateSyncMode {
    if requires_metadata_refresh {
        return ActivePropertyUpdateSyncMode::FullNativeSync;
    }

    let Some(previous_runtime_record) = previous_runtime_record else {
        return ActivePropertyUpdateSyncMode::FullNativeSync;
    };

    let (previous_scene, current_scene) =
        match (&previous_runtime_record.runtime, &runtime_record.runtime) {
            (
                WallpaperRuntime::Scene { scene: previous },
                WallpaperRuntime::Scene { scene: current },
            ) => (previous, current),
            (WallpaperRuntime::Web { .. }, WallpaperRuntime::Web { .. })
                if previous_runtime_record.runtime == runtime_record.runtime =>
            {
                return if previous_runtime_record.property_schema == runtime_record.property_schema
                {
                    ActivePropertyUpdateSyncMode::Current
                } else {
                    ActivePropertyUpdateSyncMode::FullNativeSync
                };
            }
            _ => {
                return if previous_runtime_record.runtime == runtime_record.runtime {
                    ActivePropertyUpdateSyncMode::Current
                } else {
                    ActivePropertyUpdateSyncMode::FullNativeSync
                };
            }
        };

    match scene_update_sync_mode(
        scene_signature(previous_runtime_record).as_deref(),
        scene_signature(runtime_record).as_deref(),
    ) {
        SceneUpdateSyncMode::Current => return ActivePropertyUpdateSyncMode::Current,
        SceneUpdateSyncMode::LightweightDynamicText => {
            return ActivePropertyUpdateSyncMode::LightweightDynamicText
        }
        SceneUpdateSyncMode::FullNativeSync => {}
    }

    if scene_value_update_preserves_runtime_structure(previous_scene, current_scene) {
        ActivePropertyUpdateSyncMode::LightweightSceneRuntime
    } else {
        ActivePropertyUpdateSyncMode::FullNativeSync
    }
}

fn scene_value_update_preserves_runtime_structure(
    previous: &SceneRuntimeDocument,
    current: &SceneRuntimeDocument,
) -> bool {
    previous.source == current.source
        && previous.evaluated.render_list == current.evaluated.render_list
        && scene_object_topology(previous) == scene_object_topology(current)
}

fn scene_object_topology(scene: &SceneRuntimeDocument) -> BTreeMap<u32, &'static str> {
    scene
        .evaluated
        .objects
        .iter()
        .map(|(id, object)| (*id, scene_object_kind(object)))
        .collect()
}

fn scene_object_kind(object: &EvaluatedSceneObject) -> &'static str {
    match object {
        EvaluatedSceneObject::Container { .. } => "container",
        EvaluatedSceneObject::Visual { .. } => "visual",
        EvaluatedSceneObject::Text { .. } => "text",
        EvaluatedSceneObject::Audio { .. } => "audio",
        EvaluatedSceneObject::Particle { .. } => "particle",
        EvaluatedSceneObject::Sound { .. } => "sound",
    }
}

fn parse_scene_signature(signature: &str) -> Option<serde_json::Value> {
    serde_json::from_str(signature).ok()
}

pub(super) fn should_start_scene_update_loop(runtime_record: &WallpaperRuntimeRecord) -> bool {
    match &runtime_record.runtime {
        WallpaperRuntime::Scene { scene } => scene_update_cadence(scene).is_some(),
        _ => false,
    }
}

#[cfg(test)]
pub(super) fn scene_requires_periodic_updates(scene: &SceneRuntimeDocument) -> bool {
    scene_update_cadence(scene).is_some()
}

pub(super) fn scene_update_cadence(scene: &SceneRuntimeDocument) -> Option<SceneUpdateCadence> {
    let text_cadence = scene
        .source
        .text_layers
        .iter()
        .filter_map(scene_text_behavior_service::text_layer_update_cadence)
        .min_by_key(|cadence| cadence.interval_millis())
        .map(SceneUpdateCadence::from);
    let now_playing_cadence =
        scene_now_playing_provider_service::uses_now_playing_provider(&scene.source).then(|| {
            SceneUpdateCadence::CustomMillis(
                scene_now_playing_provider_service::provider_refresh_interval_millis(
                    &scene.now_playing,
                ),
            )
        });

    [text_cadence, now_playing_cadence]
        .into_iter()
        .flatten()
        .min_by_key(|cadence| cadence.interval_millis())
}

fn scene_update_sleep_duration(
    scene: &SceneRuntimeDocument,
    now: chrono::DateTime<Local>,
) -> Option<Duration> {
    let cadence = scene_update_cadence(scene)?;
    Some(match cadence {
        SceneUpdateCadence::CustomMillis(interval_millis) => {
            let now_millis = now.timestamp_millis().max(0) as u64;
            let remainder = now_millis % interval_millis.max(1);
            let remaining = if remainder == 0 {
                interval_millis
            } else {
                interval_millis - remainder
            };
            Duration::from_millis(remaining.max(1))
        }
        SceneUpdateCadence::Second => {
            let millis = now.timestamp_subsec_millis() as u64;
            Duration::from_millis((1000 - millis).max(1))
        }
        SceneUpdateCadence::TwoSeconds => {
            let millis = now.timestamp_subsec_millis() as u64;
            let seconds_to_boundary = if now.second() % 2 == 0 { 2 } else { 1 };
            Duration::from_millis(
                (seconds_to_boundary as u64)
                    .saturating_mul(1000)
                    .saturating_sub(millis)
                    .max(1),
            )
        }
        SceneUpdateCadence::Minute => {
            let millis = now.timestamp_subsec_millis() as u64;
            let seconds = now.second() as u64;
            Duration::from_millis(
                (60_u64 - seconds)
                    .saturating_mul(1000)
                    .saturating_sub(millis)
                    .max(1),
            )
        }
    })
}

impl SceneUpdateCadence {
    fn interval_millis(self) -> u64 {
        match self {
            SceneUpdateCadence::CustomMillis(interval_millis) => interval_millis.max(1),
            SceneUpdateCadence::Minute => 60_000,
            SceneUpdateCadence::TwoSeconds => 2_000,
            SceneUpdateCadence::Second => 1_000,
        }
    }
}

pub(super) fn native_host_sync_disposition(
    runtime_record: Option<&WallpaperRuntimeRecord>,
    host: NativeHostKind,
) -> NativeHostSyncDisposition {
    match (runtime_record.map(|record| &record.runtime), host) {
        (Some(WallpaperRuntime::Scene { .. }), NativeHostKind::Scene) => {
            NativeHostSyncDisposition::Critical
        }
        (Some(WallpaperRuntime::Video { .. }), NativeHostKind::Video) => {
            NativeHostSyncDisposition::Critical
        }
        (Some(WallpaperRuntime::Web { .. }), NativeHostKind::Web) => {
            NativeHostSyncDisposition::Critical
        }
        _ => NativeHostSyncDisposition::BestEffort,
    }
}

pub(super) fn push_critical_sync_error(
    critical_errors: &mut Vec<String>,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    host: NativeHostKind,
    error: String,
) {
    if native_host_sync_disposition(runtime_record, host) == NativeHostSyncDisposition::Critical {
        critical_errors.push(format!(
            "{} runtime failed: {error}",
            match host {
                NativeHostKind::Scene => "native scene",
                NativeHostKind::Video => "native video",
                NativeHostKind::Web => "native web",
            }
        ));
    }
}

#[cfg(test)]
pub(super) fn should_emit_scene_update(
    current_signature: Option<&str>,
    next_signature: Option<&str>,
) -> bool {
    scene_update_sync_mode(current_signature, next_signature) != SceneUpdateSyncMode::Current
}
