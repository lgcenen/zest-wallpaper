use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    ffi::CStr,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::{
    models::{WallpaperRuntime, WallpaperRuntimeRecord},
    services::{
        audio_input_service::{self, AudioSnapshot},
        diagnostic_service,
        input_service::SharedInputSnapshot,
        player_host_service,
        runtime_audio_settings_service::normalize_output_volume_percent,
        web_runtime_service,
    },
};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{NSObject, ProtocolObject},
    DeclaredClass, MainThreadMarker, MainThreadOnly,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSAutoresizingMaskOptions, NSColor, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL};
#[cfg(target_os = "macos")]
use objc2_web_kit::{
    WKAudiovisualMediaTypes, WKScriptMessage, WKScriptMessageHandler, WKUserContentController,
    WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration,
};

const BOOTSTRAP_RETRY_INTERVAL: Duration = Duration::from_millis(350);
const HTML_WALLPAPER_BRIDGE: &str = include_str!("html_wallpaper_bridge.js");
const BRIDGE_MESSAGE_HANDLER_NAME: &str = "wallpaperBridge";
const DIAGNOSTIC_SUBSYSTEM: &str = "native-web";
const MISSING_ENTRY_CODE: &str = "missing-entry";
const SYNC_FAILED_CODE: &str = "sync-failed";

pub struct NativeWebServiceState {
    runtime: Mutex<NativeWebRuntime>,
    output_volume: Mutex<f64>,
}

impl Default for NativeWebServiceState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(NativeWebRuntime::default()),
            output_volume: Mutex::new(1.0),
        }
    }
}

#[derive(Default)]
struct NativeWebRuntime {
    spec: Option<WebRuntimeSpec>,
    views: BTreeMap<String, Arc<NativeWebViewHandle>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebRuntimeSpec {
    runtime_id: String,
    runtime_url: String,
    paused: bool,
    property_payload_json: String,
    window_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeWebRuntimePlan {
    session: WebSessionPlan,
    ensure_labels: Vec<String>,
    remove_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WebSessionPlan {
    Keep,
    Stop,
    Start { spec: WebRuntimeSpec },
    Replace { spec: WebRuntimeSpec },
    UpdateBridge { spec: WebRuntimeSpec },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeWebRuntimeSnapshot {
    runtime_id: Option<String>,
    paused: bool,
    property_payload_json: String,
    labels: Vec<String>,
}

impl Default for NativeWebRuntimeSnapshot {
    fn default() -> Self {
        Self {
            runtime_id: None,
            paused: false,
            property_payload_json: "{}".to_string(),
            labels: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebBridgeBootstrapPayload {
    property_payload_json: String,
    paused: bool,
    hash: String,
}

impl WebBridgeBootstrapPayload {
    fn from_spec(spec: &WebRuntimeSpec) -> Self {
        Self {
            property_payload_json: spec.property_payload_json.clone(),
            paused: spec.paused,
            hash: bootstrap_hash(&spec.property_payload_json, spec.paused),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CursorPayload {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct WebBridgeDispatchBatch {
    properties_json: Option<String>,
    paused: Option<bool>,
    cursor: Option<CursorPayload>,
    audio_samples: Option<Vec<f32>>,
}

impl WebBridgeDispatchBatch {
    fn bootstrap(payload: &WebBridgeBootstrapPayload) -> Self {
        Self {
            properties_json: Some(payload.property_payload_json.clone()),
            paused: Some(payload.paused),
            cursor: None,
            audio_samples: None,
        }
    }

    fn cursor(cursor: CursorPayload) -> Self {
        Self {
            properties_json: None,
            paused: None,
            cursor: Some(cursor),
            audio_samples: None,
        }
    }

    fn audio(snapshot: &AudioSnapshot) -> Self {
        Self {
            properties_json: None,
            paused: None,
            cursor: None,
            audio_samples: Some(snapshot.smoothed_bands.clone()),
        }
    }
}

#[derive(Debug, Default)]
struct NativeWebBridgeState {
    current_runtime_id: Option<String>,
    bridge_ready: bool,
    audio_listener_active: bool,
    bootstrap_pending: bool,
    last_bootstrap_hash: Option<String>,
    next_bootstrap_retry_at: Option<Instant>,
    pending_bootstrap: Option<WebBridgeBootstrapPayload>,
    applied_bootstrap: Option<WebBridgeBootstrapPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct NativeWebBridgeMessage {
    #[serde(rename = "type")]
    kind: String,
    href: Option<String>,
    active: Option<bool>,
}

struct NativeWebRuntimeActions {
    spec: Option<WebRuntimeSpec>,
    teardown_views: Vec<Arc<NativeWebViewHandle>>,
    sync_views: Vec<(String, Arc<NativeWebViewHandle>)>,
    create_labels: Vec<String>,
}

struct NativeWebDispatchState {
    spec: WebRuntimeSpec,
    views: Vec<Arc<NativeWebViewHandle>>,
}

struct NativeWebBootstrapRetryState {
    spec: WebRuntimeSpec,
    views: Vec<Arc<NativeWebViewHandle>>,
}

pub fn start_bridge_retry_worker(app: AppHandle) {
    thread::spawn(move || loop {
        let _ = dispatch_bootstrap_retries(&app);
        thread::sleep(BOOTSTRAP_RETRY_INTERVAL);
    });
}

pub fn sync_native_web_runtime(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    let result = (|| {
        let Some(state) = app.try_state::<NativeWebServiceState>() else {
            return Ok(());
        };

        let spec = desired_web_runtime_spec(app, runtime_record, paused)?;
        let actions = {
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            let plan = plan_native_web_runtime(&runtime.snapshot(), spec.as_ref());
            prepare_runtime_actions(&mut runtime, plan)
        };
        let output_volume = *state
            .output_volume
            .lock()
            .map_err(|error| error.to_string())?;

        execute_runtime_actions(app, &state, actions, output_volume)
    })();
    match &result {
        Ok(()) => {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, SYNC_FAILED_CODE);
        }
        Err(error) => {
            let _ = diagnostic_service::record_error(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                SYNC_FAILED_CODE,
                "Native web runtime failed to sync.",
                Some(error.clone()),
            );
        }
    }
    result
}

pub fn set_native_web_output_volume(app: &AppHandle, volume: f64) -> Result<(), String> {
    let normalized = normalize_output_volume_percent(volume);
    let Some(state) = app.try_state::<NativeWebServiceState>() else {
        return Ok(());
    };

    {
        let mut current = state
            .output_volume
            .lock()
            .map_err(|error| error.to_string())?;
        *current = normalized;
    }

    let views = {
        let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
        runtime.views.values().cloned().collect::<Vec<_>>()
    };
    for view in views {
        view.set_output_volume(normalized)?;
    }

    Ok(())
}

pub fn dispatch_shared_input(
    app: &AppHandle,
    snapshot: &SharedInputSnapshot,
) -> Result<(), String> {
    let Some(state) = app.try_state::<NativeWebServiceState>() else {
        return Ok(());
    };
    let Some(dispatch_state) = snapshot_dispatch_state(&state)? else {
        return Ok(());
    };

    for view in &dispatch_state.views {
        view.dispatch_input(&dispatch_state.spec, snapshot)?;
    }

    Ok(())
}

pub fn dispatch_shared_audio(app: &AppHandle, snapshot: &AudioSnapshot) -> Result<(), String> {
    let Some(state) = app.try_state::<NativeWebServiceState>() else {
        return Ok(());
    };
    let Some(dispatch_state) = snapshot_audio_dispatch_state(&state)? else {
        return Ok(());
    };

    for view in &dispatch_state.views {
        view.dispatch_audio(&dispatch_state.spec, snapshot)?;
    }

    Ok(())
}

pub fn audio_consumers_active(app: &AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<NativeWebServiceState>() else {
        return Ok(false);
    };
    let Some(consumer_state) = snapshot_audio_consumer_state(&state)? else {
        return Ok(false);
    };

    Ok(consumer_state.views.iter().any(|view| {
        view.audio_listener_active(&consumer_state.spec)
            .unwrap_or(false)
    }))
}

fn dispatch_bootstrap_retries(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<NativeWebServiceState>() else {
        return Ok(());
    };
    let Some(retry_state) = snapshot_bootstrap_retry_state(&state)? else {
        return Ok(());
    };
    let now = Instant::now();

    for view in &retry_state.views {
        view.retry_pending_bootstrap(&retry_state.spec, now)?;
    }

    Ok(())
}

fn desired_web_runtime_spec(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<Option<WebRuntimeSpec>, String> {
    let record = match runtime_record {
        Some(record) => record,
        None => {
            clear_web_diagnostics(app);
            return Ok(None);
        }
    };
    let entry_path = match resolve_active_web_entry_path(runtime_record) {
        Ok(Some(entry_path)) => entry_path,
        Ok(None) => {
            clear_web_diagnostics(app);
            return Ok(None);
        }
        Err(error) => {
            let _ = diagnostic_service::record_error(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                MISSING_ENTRY_CODE,
                "Native web runtime is missing its entry HTML file.",
                error_detail(&error),
            );
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, SYNC_FAILED_CODE);
            return Err(error);
        }
    };
    let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, MISSING_ENTRY_CODE);

    let runtime_session = web_runtime_service::prepare_web_runtime_session(&entry_path)?;
    let labels = player_host_service::live_player_host_label_set(app)
        .into_iter()
        .collect::<Vec<_>>();
    if labels.is_empty() {
        return Ok(None);
    }

    Ok(Some(WebRuntimeSpec {
        runtime_id: runtime_session.runtime_id().to_string(),
        runtime_url: runtime_session.runtime_url().to_string(),
        paused,
        property_payload_json: serialize_property_payload(record)?,
        window_labels: labels,
    }))
}

fn clear_web_diagnostics(app: &AppHandle) {
    let _ = diagnostic_service::clear_subsystem(app, DIAGNOSTIC_SUBSYSTEM);
}

fn resolve_active_web_entry_path(
    runtime_record: Option<&WallpaperRuntimeRecord>,
) -> Result<Option<String>, String> {
    let Some(record) = runtime_record else {
        return Ok(None);
    };
    let web = match &record.runtime {
        WallpaperRuntime::Web { web } => web,
        _ => return Ok(None),
    };

    let entry_path = web
        .entry_path
        .as_ref()
        .or(record.entry_path.as_ref())
        .cloned()
        .ok_or_else(|| "native web runtime is missing its entry HTML file".to_string())?;
    if !Path::new(&entry_path).is_file() {
        return Err(format!(
            "native web runtime entry HTML file is missing: {entry_path}"
        ));
    }

    Ok(Some(entry_path))
}

fn error_detail(error: &str) -> Option<String> {
    let detail = error.trim();
    if detail.is_empty() {
        None
    } else {
        Some(detail.to_string())
    }
}

fn serialize_property_payload(record: &WallpaperRuntimeRecord) -> Result<String, String> {
    #[derive(Serialize)]
    struct WebPropertyValue {
        value: serde_json::Value,
    }

    let payload = record
        .property_schema
        .iter()
        .map(|property| {
            (
                property.key.clone(),
                WebPropertyValue {
                    value: property.value.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

fn bootstrap_hash(property_payload_json: &str, paused: bool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(property_payload_json.as_bytes());
    hasher.update([paused as u8]);
    format!("{:x}", hasher.finalize())
}

fn plan_native_web_runtime(
    current: &NativeWebRuntimeSnapshot,
    desired: Option<&WebRuntimeSpec>,
) -> NativeWebRuntimePlan {
    let current_labels = current.labels.iter().cloned().collect::<BTreeSet<_>>();
    let desired_labels = desired
        .map(|spec| spec.window_labels.iter().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();

    let remove_labels = current_labels
        .difference(&desired_labels)
        .cloned()
        .collect::<Vec<_>>();
    let ensure_labels = desired_labels.iter().cloned().collect::<Vec<_>>();

    let session = match desired {
        None => {
            if current.runtime_id.is_some() || !current.labels.is_empty() {
                WebSessionPlan::Stop
            } else {
                WebSessionPlan::Keep
            }
        }
        Some(spec) => match current.runtime_id.as_deref() {
            None => WebSessionPlan::Start { spec: spec.clone() },
            Some(current_id) if current_id != spec.runtime_id => {
                WebSessionPlan::Replace { spec: spec.clone() }
            }
            Some(_)
                if current.paused != spec.paused
                    || current.property_payload_json != spec.property_payload_json =>
            {
                WebSessionPlan::UpdateBridge { spec: spec.clone() }
            }
            Some(_) => WebSessionPlan::Keep,
        },
    };

    NativeWebRuntimePlan {
        session,
        ensure_labels,
        remove_labels,
    }
}

fn prepare_runtime_actions(
    runtime: &mut NativeWebRuntime,
    plan: NativeWebRuntimePlan,
) -> NativeWebRuntimeActions {
    let mut teardown_views = Vec::new();
    for label in &plan.remove_labels {
        if let Some(view) = runtime.views.remove(label) {
            teardown_views.push(view);
        }
    }

    match plan.session {
        WebSessionPlan::Keep => {}
        WebSessionPlan::Stop => {
            for label in runtime.views.keys().cloned().collect::<Vec<_>>() {
                if let Some(view) = runtime.views.remove(&label) {
                    teardown_views.push(view);
                }
            }
            runtime.clear_spec();
            return NativeWebRuntimeActions {
                spec: None,
                teardown_views,
                sync_views: vec![],
                create_labels: vec![],
            };
        }
        WebSessionPlan::Start { spec }
        | WebSessionPlan::Replace { spec }
        | WebSessionPlan::UpdateBridge { spec } => {
            runtime.spec = Some(spec);
        }
    }

    let spec = runtime.spec.clone();
    let mut sync_views = Vec::new();
    let mut create_labels = Vec::new();

    if spec.is_some() {
        for label in &plan.ensure_labels {
            if let Some(view) = runtime.views.get(label).cloned() {
                sync_views.push((label.clone(), view));
            } else {
                create_labels.push(label.clone());
            }
        }
    }

    NativeWebRuntimeActions {
        spec,
        teardown_views,
        sync_views,
        create_labels,
    }
}

fn execute_runtime_actions(
    app: &AppHandle,
    state: &NativeWebServiceState,
    actions: NativeWebRuntimeActions,
    output_volume: f64,
) -> Result<(), String> {
    for view in &actions.teardown_views {
        view.teardown();
    }

    let Some(spec) = actions.spec else {
        return Ok(());
    };

    for (label, view) in &actions.sync_views {
        view.sync(app, label, &spec)?;
        view.set_output_volume(output_volume)?;
    }

    for label in &actions.create_labels {
        let view = Arc::new(NativeWebViewHandle::create(app)?);
        view.sync(app, label, &spec)?;
        view.set_output_volume(output_volume)?;

        let inserted = {
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            if runtime.spec.as_ref() != Some(&spec) {
                false
            } else {
                match runtime.views.entry(label.clone()) {
                    Entry::Occupied(_) => false,
                    Entry::Vacant(entry) => {
                        entry.insert(Arc::clone(&view));
                        true
                    }
                }
            }
        };

        if !inserted {
            view.teardown();
        }
    }

    Ok(())
}

fn snapshot_dispatch_state(
    state: &NativeWebServiceState,
) -> Result<Option<NativeWebDispatchState>, String> {
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    let Some(spec) = runtime.spec.clone() else {
        return Ok(None);
    };
    if runtime.views.is_empty() || spec.paused {
        return Ok(None);
    }

    Ok(Some(NativeWebDispatchState {
        spec,
        views: runtime.views.values().cloned().collect(),
    }))
}

fn snapshot_audio_dispatch_state(
    state: &NativeWebServiceState,
) -> Result<Option<NativeWebDispatchState>, String> {
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    let Some(spec) = runtime.spec.clone() else {
        return Ok(None);
    };
    if runtime.views.is_empty() || spec.paused {
        return Ok(None);
    }

    Ok(Some(NativeWebDispatchState {
        spec,
        views: runtime.views.values().cloned().collect(),
    }))
}

fn snapshot_audio_consumer_state(
    state: &NativeWebServiceState,
) -> Result<Option<NativeWebDispatchState>, String> {
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    let Some(spec) = runtime.spec.clone() else {
        return Ok(None);
    };
    if spec.paused || runtime.views.is_empty() {
        return Ok(None);
    }

    Ok(Some(NativeWebDispatchState {
        spec,
        views: runtime.views.values().cloned().collect(),
    }))
}

fn snapshot_bootstrap_retry_state(
    state: &NativeWebServiceState,
) -> Result<Option<NativeWebBootstrapRetryState>, String> {
    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    let Some(spec) = runtime.spec.clone() else {
        return Ok(None);
    };
    if runtime.views.is_empty() {
        return Ok(None);
    }

    Ok(Some(NativeWebBootstrapRetryState {
        spec,
        views: runtime.views.values().cloned().collect(),
    }))
}

impl NativeWebRuntime {
    fn snapshot(&self) -> NativeWebRuntimeSnapshot {
        NativeWebRuntimeSnapshot {
            runtime_id: self.spec.as_ref().map(|spec| spec.runtime_id.clone()),
            paused: self.spec.as_ref().map(|spec| spec.paused).unwrap_or(false),
            property_payload_json: self
                .spec
                .as_ref()
                .map(|spec| spec.property_payload_json.clone())
                .unwrap_or_else(|| "{}".to_string()),
            labels: self.views.keys().cloned().collect(),
        }
    }

    fn clear_spec(&mut self) {
        self.spec = None;
    }
}

impl NativeWebBridgeState {
    fn reset_for_navigation(
        &mut self,
        runtime_id: &str,
        payload: WebBridgeBootstrapPayload,
        now: Instant,
    ) {
        self.current_runtime_id = Some(runtime_id.to_string());
        self.bridge_ready = false;
        self.bootstrap_pending = true;
        self.last_bootstrap_hash = None;
        self.next_bootstrap_retry_at = Some(now + BOOTSTRAP_RETRY_INTERVAL);
        self.pending_bootstrap = Some(payload);
        self.applied_bootstrap = None;
    }

    fn update_pending_bootstrap(
        &mut self,
        payload: WebBridgeBootstrapPayload,
        now: Instant,
    ) -> Option<WebBridgeDispatchBatch> {
        self.pending_bootstrap = Some(payload.clone());

        if !self.bridge_ready {
            self.bootstrap_pending = true;
            self.next_bootstrap_retry_at = Some(now + BOOTSTRAP_RETRY_INTERVAL);
            return None;
        }

        let previous = self.applied_bootstrap.as_ref();
        let properties_changed = previous
            .map(|previous| previous.property_payload_json != payload.property_payload_json)
            .unwrap_or(true);
        let paused_changed = previous
            .map(|previous| previous.paused != payload.paused)
            .unwrap_or(true);

        self.bootstrap_pending = false;
        self.next_bootstrap_retry_at = None;

        if !properties_changed && !paused_changed {
            self.last_bootstrap_hash = Some(payload.hash.clone());
            self.applied_bootstrap = Some(payload);
            return None;
        }

        self.last_bootstrap_hash = Some(payload.hash.clone());
        self.applied_bootstrap = Some(payload.clone());

        Some(WebBridgeDispatchBatch {
            properties_json: properties_changed.then(|| payload.property_payload_json.clone()),
            paused: paused_changed.then_some(payload.paused),
            cursor: None,
            audio_samples: None,
        })
    }

    fn mark_bridge_ready(&mut self, href: Option<&str>) -> Option<WebBridgeDispatchBatch> {
        if let (Some(expected), Some(actual)) = (self.current_runtime_id.as_deref(), href) {
            if !runtime_urls_match(expected, actual) {
                return None;
            }
        }

        self.bridge_ready = true;
        let payload = self.pending_bootstrap.clone()?;
        if self.last_bootstrap_hash.as_deref() == Some(payload.hash.as_str())
            && !self.bootstrap_pending
        {
            return None;
        }

        self.bootstrap_pending = false;
        self.next_bootstrap_retry_at = None;
        self.last_bootstrap_hash = Some(payload.hash.clone());
        self.applied_bootstrap = Some(payload.clone());
        Some(WebBridgeDispatchBatch::bootstrap(&payload))
    }

    fn retry_batch_if_due(
        &mut self,
        runtime_id: &str,
        now: Instant,
    ) -> Option<WebBridgeDispatchBatch> {
        if self.current_runtime_id.as_deref() != Some(runtime_id) || !self.bootstrap_pending {
            return None;
        }

        let due = self
            .next_bootstrap_retry_at
            .map(|deadline| deadline <= now)
            .unwrap_or(true);
        if !due {
            return None;
        }

        self.next_bootstrap_retry_at = Some(now + BOOTSTRAP_RETRY_INTERVAL);
        self.pending_bootstrap
            .as_ref()
            .map(WebBridgeDispatchBatch::bootstrap)
    }

    fn cursor_dispatch_allowed(&self, paused: bool) -> bool {
        self.bridge_ready && !paused
    }

    fn audio_dispatch_allowed(&self, paused: bool) -> bool {
        self.bridge_ready && self.audio_listener_active && !paused
    }

    fn current_runtime_id_matches(&self, runtime_id: &str) -> bool {
        self.current_runtime_id.as_deref() == Some(runtime_id)
    }

    fn mark_audio_listener(&mut self, active: bool, href: Option<&str>) -> bool {
        if let (Some(expected), Some(actual)) = (self.current_runtime_id.as_deref(), href) {
            if !runtime_urls_match(expected, actual) {
                return false;
            }
        }

        let changed = self.audio_listener_active != active;
        self.audio_listener_active = active;
        changed
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

fn runtime_urls_match(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }

    fn strip_query_and_fragment(value: &str) -> &str {
        value.split(['?', '#']).next().unwrap_or(value)
    }

    fn extract_runtime_id_from_url(value: &str) -> Option<&str> {
        let normalized = strip_query_and_fragment(value);
        normalized
            .split("/web-runtime/")
            .nth(1)
            .map(|path| path.trim_start_matches('/'))
            .filter(|path| !path.is_empty())
    }

    fn strip_fragment(value: &str) -> &str {
        value.split('#').next().unwrap_or(value)
    }

    if let Some(actual_runtime_id) = extract_runtime_id_from_url(actual) {
        return expected == actual_runtime_id;
    }

    strip_fragment(expected) == strip_fragment(actual)
}

struct NativeWebViewHandle {
    #[cfg(target_os = "macos")]
    host: MainThreadBound<NativeWebViewHost>,
}

impl NativeWebViewHandle {
    fn create(app: &AppHandle) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let host = run_on_main(|mtm| {
                NativeWebViewHost::create(app, mtm).map(|host| MainThreadBound::new(host, mtm))
            })?;
            return Ok(Self { host });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            Ok(Self {})
        }
    }

    fn sync(&self, app: &AppHandle, label: &str, spec: &WebRuntimeSpec) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let label = label.to_string();
            let spec = spec.clone();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                player_host_service::with_player_host_container_view(&app, &label, mtm, |container| {
                    host.sync(container, &spec)
                })
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, label, spec);
            Ok(())
        }
    }

    fn dispatch_input(
        &self,
        spec: &WebRuntimeSpec,
        snapshot: &SharedInputSnapshot,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let spec = spec.clone();
            let snapshot = snapshot.clone();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                host.dispatch_input(&spec, &snapshot)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (spec, snapshot);
            Ok(())
        }
    }

    fn retry_pending_bootstrap(&self, spec: &WebRuntimeSpec, now: Instant) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let spec = spec.clone();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                host.retry_pending_bootstrap(&spec, now)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (spec, now);
            Ok(())
        }
    }

    fn dispatch_audio(
        &self,
        spec: &WebRuntimeSpec,
        snapshot: &AudioSnapshot,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let spec = spec.clone();
            let snapshot = snapshot.clone();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                host.dispatch_audio(&spec, &snapshot)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (spec, snapshot);
            Ok(())
        }
    }

    fn audio_listener_active(&self, spec: &WebRuntimeSpec) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        {
            let spec = spec.clone();
            return run_on_main(move |mtm| {
                let host = self.host.get(mtm);
                host.audio_listener_active(&spec)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = spec;
            Ok(false)
        }
    }

    fn set_output_volume(&self, output_volume: f64) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            return run_on_main(|mtm| {
                let host = self.host.get(mtm);
                host.set_output_volume(output_volume)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = output_volume;
            Ok(())
        }
    }

    fn teardown(&self) {
        #[cfg(target_os = "macos")]
        run_on_main(|mtm| {
            let host = self.host.get(mtm);
            host.detach();
        });
    }
}

#[cfg(target_os = "macos")]
struct NativeWebViewHost {
    view: Retained<WKWebView>,
    controller: Retained<WKUserContentController>,
    delegate: Retained<NativeWebBridgeDelegate>,
    bridge_state: Arc<Mutex<NativeWebBridgeState>>,
}

#[cfg(target_os = "macos")]
struct NativeWebBridgeDelegateIvars {
    app_handle: AppHandle,
    view: Retained<WKWebView>,
    bridge_state: Arc<Mutex<NativeWebBridgeState>>,
}

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = NativeWebBridgeDelegateIvars]
    struct NativeWebBridgeDelegate;

    unsafe impl NSObjectProtocol for NativeWebBridgeDelegate {}

    unsafe impl WKScriptMessageHandler for NativeWebBridgeDelegate {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(
            this: &NativeWebBridgeDelegate,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            handle_bridge_message(this, message);
        }
    }
);

#[cfg(target_os = "macos")]
fn handle_bridge_message(this: &NativeWebBridgeDelegate, message: &WKScriptMessage) {
    let Some(message) = parse_bridge_message(message) else {
        return;
    };
    match message.kind.as_str() {
        "wallpaper:bridge-ready" => {
            let batch = {
                let mut state = match this.ivars().bridge_state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                state.mark_bridge_ready(message.href.as_deref())
            };

            if let Some(batch) = batch {
                let view = &this.ivars().view;
                let _ = evaluate_dispatch(view, &batch);
            }
        }
        "wallpaper:audio-listener" => {
            let changed = {
                let mut state = match this.ivars().bridge_state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                state.mark_audio_listener(message.active.unwrap_or(false), message.href.as_deref())
            };

            if changed {
                audio_input_service::request_policy_refresh(&this.ivars().app_handle);
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
fn parse_bridge_message(message: &WKScriptMessage) -> Option<NativeWebBridgeMessage> {
    unsafe {
        let body = message.body();
        let body = body.downcast::<NSString>().ok()?;
        let body_ptr = body.UTF8String();
        let body_str = CStr::from_ptr(body_ptr).to_str().ok()?;
        serde_json::from_str(body_str).ok()
    }
}

#[cfg(target_os = "macos")]
impl NativeWebBridgeDelegate {
    fn new(
        app_handle: AppHandle,
        view: Retained<WKWebView>,
        bridge_state: Arc<Mutex<NativeWebBridgeState>>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let delegate =
            mtm.alloc::<NativeWebBridgeDelegate>()
                .set_ivars(NativeWebBridgeDelegateIvars {
                    app_handle,
                    view,
                    bridge_state,
                });

        unsafe { msg_send![super(delegate), init] }
    }
}

#[cfg(target_os = "macos")]
impl NativeWebViewHost {
    fn create(app_handle: AppHandle, mtm: MainThreadMarker) -> Result<Self, String> {
        let controller = unsafe { WKUserContentController::new(mtm) };
        let bridge_source = NSString::from_str(HTML_WALLPAPER_BRIDGE);
        let bridge_script = unsafe {
            WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &bridge_source,
                WKUserScriptInjectionTime::AtDocumentStart,
                false,
            )
        };
        unsafe {
            controller.addUserScript(&bridge_script);
        }

        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe {
            configuration.setUserContentController(&controller);
            configuration.setUpgradeKnownHostsToHTTPS(false);
            configuration.setAllowsAirPlayForMediaPlayback(false);
            configuration
                .setMediaTypesRequiringUserActionForPlayback(WKAudiovisualMediaTypes::None);
        }

        let view = unsafe {
            WKWebView::initWithFrame_configuration(
                WKWebView::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
                &configuration,
            )
        };
        unsafe {
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            view.setUnderPageBackgroundColor(Some(&NSColor::clearColor()));
            #[cfg(debug_assertions)]
            view.setInspectable(true);
        }

        let bridge_state = Arc::new(Mutex::new(NativeWebBridgeState::default()));
        let delegate =
            NativeWebBridgeDelegate::new(app_handle, view.clone(), Arc::clone(&bridge_state), mtm);
        let delegate_proto = ProtocolObject::from_ref(&*delegate);
        let handler_name = NSString::from_str(BRIDGE_MESSAGE_HANDLER_NAME);
        unsafe {
            controller.addScriptMessageHandler_name(delegate_proto, &handler_name);
        }

        Ok(Self {
            view,
            controller,
            delegate,
            bridge_state,
        })
    }

    fn sync(&self, container: &NSView, spec: &WebRuntimeSpec) -> Result<(), String> {
        self.view.setFrame(container.bounds());
        if !self.view.isDescendantOf(container) {
            self.view.removeFromSuperview();
            container.addSubview(&self.view);
        }

        let payload = WebBridgeBootstrapPayload::from_spec(spec);
        let url_changed = {
            let state = self
                .bridge_state
                .lock()
                .map_err(|error| error.to_string())?;
            !state.current_runtime_id_matches(&spec.runtime_id)
        };

        if url_changed {
            self.load_runtime_url(&spec.runtime_url)?;
            let mut state = self
                .bridge_state
                .lock()
                .map_err(|error| error.to_string())?;
            state.reset_for_navigation(&spec.runtime_id, payload, Instant::now());
            return Ok(());
        }

        let batch = {
            let mut state = self
                .bridge_state
                .lock()
                .map_err(|error| error.to_string())?;
            state.update_pending_bootstrap(payload, Instant::now())
        };
        if let Some(batch) = batch {
            self.evaluate_dispatch(&batch)?;
        }

        Ok(())
    }

    fn dispatch_input(
        &self,
        spec: &WebRuntimeSpec,
        snapshot: &SharedInputSnapshot,
    ) -> Result<(), String> {
        let cursor = self.cursor_payload_from_snapshot(snapshot);
        let should_dispatch = {
            let state = self
                .bridge_state
                .lock()
                .map_err(|error| error.to_string())?;
            state.cursor_dispatch_allowed(spec.paused)
        };
        if !should_dispatch {
            return Ok(());
        }

        let Some(cursor) = cursor else {
            return Ok(());
        };
        self.evaluate_dispatch(&WebBridgeDispatchBatch::cursor(cursor))
    }

    fn dispatch_audio(
        &self,
        spec: &WebRuntimeSpec,
        snapshot: &AudioSnapshot,
    ) -> Result<(), String> {
        let should_dispatch = {
            let state = self
                .bridge_state
                .lock()
                .map_err(|error| error.to_string())?;
            state.current_runtime_id_matches(&spec.runtime_id)
                && state.audio_dispatch_allowed(spec.paused)
        };
        if !should_dispatch {
            return Ok(());
        }

        self.evaluate_dispatch(&WebBridgeDispatchBatch::audio(snapshot))
    }

    fn audio_listener_active(&self, spec: &WebRuntimeSpec) -> Result<bool, String> {
        let state = self
            .bridge_state
            .lock()
            .map_err(|error| error.to_string())?;
        Ok(state.current_runtime_id_matches(&spec.runtime_id)
            && state.audio_dispatch_allowed(spec.paused))
    }

    fn set_output_volume(&self, output_volume: f64) -> Result<(), String> {
        let script = build_web_output_volume_script(output_volume);
        let script = NSString::from_str(&script);
        unsafe {
            self.view
                .evaluateJavaScript_completionHandler(&script, None);
        }
        Ok(())
    }

    fn retry_pending_bootstrap(&self, spec: &WebRuntimeSpec, now: Instant) -> Result<(), String> {
        let batch = {
            let mut state = self
                .bridge_state
                .lock()
                .map_err(|error| error.to_string())?;
            state.retry_batch_if_due(&spec.runtime_id, now)
        };
        if let Some(batch) = batch {
            self.evaluate_dispatch(&batch)?;
        }
        Ok(())
    }

    fn load_runtime_url(&self, runtime_url: &str) -> Result<(), String> {
        let url_string = NSString::from_str(runtime_url);
        let url = NSURL::URLWithString(&url_string)
            .ok_or_else(|| format!("native web runtime rejected invalid URL: {runtime_url}"))?;
        let request = NSURLRequest::requestWithURL(&url);

        unsafe {
            self.view.stopLoading();
            self.view.loadRequest(&request);
        }
        Ok(())
    }

    fn evaluate_dispatch(&self, batch: &WebBridgeDispatchBatch) -> Result<(), String> {
        evaluate_dispatch(&self.view, batch)
    }

    fn cursor_payload_from_snapshot(
        &self,
        snapshot: &SharedInputSnapshot,
    ) -> Option<CursorPayload> {
        let window = self.view.window()?;
        let bounds = self.view.bounds();
        let frame = window.frame();

        project_shared_input_to_view_bounds(
            snapshot,
            frame.origin.x,
            frame.origin.y,
            bounds.size.width,
            bounds.size.height,
        )
    }

    fn detach(&self) {
        if let Ok(mut state) = self.bridge_state.lock() {
            state.clear();
        }

        let handler_name = NSString::from_str(BRIDGE_MESSAGE_HANDLER_NAME);
        unsafe {
            self.view.stopLoading();
            self.controller
                .removeScriptMessageHandlerForName(&handler_name);
            let _ = &self.delegate;
            self.view.removeFromSuperview();
        }
    }
}

#[cfg(target_os = "macos")]
fn evaluate_dispatch(view: &WKWebView, batch: &WebBridgeDispatchBatch) -> Result<(), String> {
    let Some(script) = build_bridge_dispatch_script(batch)? else {
        return Ok(());
    };
    let script = NSString::from_str(&script);
    unsafe {
        view.evaluateJavaScript_completionHandler(&script, None);
    }
    Ok(())
}

fn build_bridge_dispatch_script(batch: &WebBridgeDispatchBatch) -> Result<Option<String>, String> {
    let mut messages = Vec::new();

    if let Some(properties_json) = batch.properties_json.as_deref() {
        messages.push(format!(
            r#"{{"type":"wallpaper:properties","properties":{properties_json}}}"#
        ));
    }

    if let Some(paused) = batch.paused {
        messages.push(
            serde_json::json!({
                "type": "wallpaper:paused",
                "paused": paused,
            })
            .to_string(),
        );
    }

    if let Some(cursor) = batch.cursor {
        messages.push(
            serde_json::json!({
                "type": "wallpaper:cursor",
                "cursor": {
                    "x": cursor.x,
                    "y": cursor.y,
                },
            })
            .to_string(),
        );
    }

    if let Some(audio_samples) = batch.audio_samples.as_ref() {
        messages.push(
            serde_json::json!({
                "type": "wallpaper:audio",
                "samples": audio_samples,
            })
            .to_string(),
        );
    }

    if messages.is_empty() {
        return Ok(None);
    }

    Ok(Some(format!(
        "(() => {{ const apply = window.__wallpaperApplyRuntimeMessage; if (typeof apply !== 'function') {{ return false; }} const messages = [{}]; for (const message of messages) {{ apply(message); }} return true; }})();",
        messages.join(",")
    )))
}

fn build_web_output_volume_script(output_volume: f64) -> String {
    let normalized = if output_volume.is_finite() {
        output_volume.clamp(0.0, 1.0)
    } else {
        1.0
    };
    format!(
        r#"(() => {{
  const volume = {normalized:.4};
  const applyVolume = (root = document) => {{
    const nodes = root && typeof root.querySelectorAll === 'function'
      ? root.querySelectorAll('audio, video')
      : [];
    for (const node of nodes) {{
      node.volume = volume;
      node.muted = volume <= 0.001;
    }}
  }};
  window.__wallpaperAudioOutputVolume = volume;
  window.__wallpaperApplyAudioOutputVolume = applyVolume;
  applyVolume();
  if (!window.__wallpaperAudioOutputVolumeObserver && typeof MutationObserver === 'function') {{
    window.__wallpaperAudioOutputVolumeObserver = new MutationObserver((records) => {{
      for (const record of records) {{
        for (const node of record.addedNodes || []) {{
          if (node && node.nodeType === 1) {{
            if (node.matches && node.matches('audio, video')) {{
              node.volume = window.__wallpaperAudioOutputVolume;
              node.muted = window.__wallpaperAudioOutputVolume <= 0.001;
            }}
            applyVolume(node);
          }}
        }}
      }}
    }});
    window.__wallpaperAudioOutputVolumeObserver.observe(document.documentElement, {{
      childList: true,
      subtree: true
    }});
  }}
  return true;
}})();"#
    )
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn project_shared_input_to_view_bounds(
    snapshot: &SharedInputSnapshot,
    origin_x: f64,
    origin_y: f64,
    width: f64,
    height: f64,
) -> Option<CursorPayload> {
    if !snapshot.active || width <= 0.0 || height <= 0.0 {
        return None;
    }

    let local_x = clamp(snapshot.system_x - origin_x, 0.0, width);
    let local_y = clamp(height - (snapshot.system_y - origin_y), 0.0, height);

    Some(CursorPayload {
        x: local_x,
        y: local_y,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::models::{
        WallpaperRuntime, WallpaperRuntimeRecord, WallpaperType, WebRuntimeDocument,
    };

    use super::{
        bootstrap_hash, build_bridge_dispatch_script, build_web_output_volume_script,
        plan_native_web_runtime, project_shared_input_to_view_bounds,
        resolve_active_web_entry_path, CursorPayload, NativeWebBridgeState,
        NativeWebRuntimeSnapshot, WebBridgeBootstrapPayload, WebBridgeDispatchBatch,
        WebRuntimeSpec, WebSessionPlan, BOOTSTRAP_RETRY_INTERVAL,
    };
    use crate::services::audio_input_service::AudioSnapshot;
    use crate::services::input_service::{InputModifierSnapshot, SharedInputSnapshot};
    use std::time::{Duration, Instant};

    #[test]
    fn web_output_volume_script_controls_existing_and_new_media_elements() {
        let script = build_web_output_volume_script(0.35);
        assert!(script.contains("const volume = 0.3500"));
        assert!(script.contains("querySelectorAll('audio, video')"));
        assert!(script.contains("node.volume = volume"));
        assert!(script.contains("node.muted = volume <= 0.001"));
        assert!(script.contains("MutationObserver"));

        let muted_script = build_web_output_volume_script(0.0);
        assert!(muted_script.contains("const volume = 0.0000"));

        let fallback_script = build_web_output_volume_script(f64::NAN);
        assert!(fallback_script.contains("const volume = 1.0000"));
    }

    fn runtime_spec(
        runtime_id: &str,
        runtime_url: &str,
        paused: bool,
        property_payload_json: &str,
        labels: &[&str],
    ) -> WebRuntimeSpec {
        WebRuntimeSpec {
            runtime_id: runtime_id.to_string(),
            runtime_url: runtime_url.to_string(),
            paused,
            property_payload_json: property_payload_json.to_string(),
            window_labels: labels.iter().map(|label| (*label).to_string()).collect(),
        }
    }

    fn runtime_record(
        runtime: WallpaperRuntime,
        entry_path: Option<&str>,
    ) -> WallpaperRuntimeRecord {
        let wallpaper_type = match &runtime {
            WallpaperRuntime::Scene { .. } => WallpaperType::Scene,
            WallpaperRuntime::Video { .. } => WallpaperType::Video,
            WallpaperRuntime::Web { .. } => WallpaperType::Web,
            WallpaperRuntime::Application => WallpaperType::Application,
            WallpaperRuntime::Unknown => WallpaperType::Unknown,
        };

        WallpaperRuntimeRecord {
            id: "demo".to_string(),
            title: "Demo".to_string(),
            wallpaper_type,
            source_path: "/tmp/source".to_string(),
            managed_path: "/tmp/managed".to_string(),
            preview_path: None,
            entry_path: entry_path.map(str::to_string),
            last_snapshot_path: None,
            property_schema: vec![],
            property_sections: vec![],
            imported_at: Utc::now(),
            tags: vec![],
            runtime,
        }
    }

    fn bootstrap_payload(property_payload_json: &str, paused: bool) -> WebBridgeBootstrapPayload {
        WebBridgeBootstrapPayload {
            property_payload_json: property_payload_json.to_string(),
            paused,
            hash: bootstrap_hash(property_payload_json, paused),
        }
    }

    fn audio_snapshot(active: bool) -> AudioSnapshot {
        AudioSnapshot {
            timestamp_ms: 64,
            bands: vec![0.1, 0.2],
            smoothed_bands: vec![0.2, 0.4],
            peak: if active { 0.6 } else { 0.0 },
            rms: if active { 0.3 } else { 0.0 },
            muted: !active,
            active,
        }
    }

    #[test]
    fn player_startup_and_shutdown_create_and_remove_native_web_runtime() {
        let start = plan_native_web_runtime(
            &NativeWebRuntimeSnapshot::default(),
            Some(&runtime_spec(
                "demo/index.html",
                "http://127.0.0.1:9000/web-runtime/demo/index.html",
                false,
                r#"{"speed":{"value":1}}"#,
                &["player", "player-screen-1"],
            )),
        );
        assert!(matches!(
            start.session,
            WebSessionPlan::Start { ref spec }
                if spec.runtime_url == "http://127.0.0.1:9000/web-runtime/demo/index.html"
                    && !spec.paused
                    && spec.property_payload_json == r#"{"speed":{"value":1}}"#
        ));
        assert_eq!(start.ensure_labels, vec!["player", "player-screen-1"]);
        assert!(start.remove_labels.is_empty());

        let stop = plan_native_web_runtime(
            &NativeWebRuntimeSnapshot {
                runtime_id: Some("demo/index.html".to_string()),
                paused: false,
                property_payload_json: r#"{"speed":{"value":1}}"#.to_string(),
                labels: vec!["player".to_string(), "player-screen-1".to_string()],
            },
            None,
        );
        assert!(matches!(stop.session, WebSessionPlan::Stop));
        assert_eq!(stop.remove_labels, vec!["player", "player-screen-1"]);
        assert!(stop.ensure_labels.is_empty());
    }

    #[test]
    fn pause_and_property_changes_only_refresh_bridge_state() {
        let plan = plan_native_web_runtime(
            &NativeWebRuntimeSnapshot {
                runtime_id: Some("demo/index.html".to_string()),
                paused: false,
                property_payload_json: r#"{"speed":{"value":1}}"#.to_string(),
                labels: vec!["player".to_string()],
            },
            Some(&runtime_spec(
                "demo/index.html",
                "http://127.0.0.1:9000/web-runtime/demo/index.html",
                true,
                r#"{"speed":{"value":2}}"#,
                &["player"],
            )),
        );

        assert!(matches!(
            plan.session,
            WebSessionPlan::UpdateBridge { ref spec }
                if spec.paused
                    && spec.property_payload_json == r#"{"speed":{"value":2}}"#
        ));
        assert_eq!(plan.ensure_labels, vec!["player"]);
        assert!(plan.remove_labels.is_empty());
    }

    #[test]
    fn switching_web_wallpaper_reloads_the_runtime_url() {
        let plan = plan_native_web_runtime(
            &NativeWebRuntimeSnapshot {
                runtime_id: Some("demo-a/index.html".to_string()),
                paused: false,
                property_payload_json: "{}".to_string(),
                labels: vec!["player".to_string()],
            },
            Some(&runtime_spec(
                "demo-b/index.html",
                "http://127.0.0.1:9000/web-runtime/demo-b/index.html",
                false,
                "{}",
                &["player"],
            )),
        );

        assert!(matches!(
            plan.session,
            WebSessionPlan::Replace { ref spec }
                if spec.runtime_url == "http://127.0.0.1:9000/web-runtime/demo-b/index.html"
        ));
        assert_eq!(plan.ensure_labels, vec!["player"]);
        assert!(plan.remove_labels.is_empty());
    }

    #[test]
    fn same_runtime_id_only_updates_bridge_even_if_runtime_url_string_changes() {
        let plan = plan_native_web_runtime(
            &NativeWebRuntimeSnapshot {
                runtime_id: Some("demo/index.html".to_string()),
                paused: false,
                property_payload_json: r#"{"speed":{"value":1}}"#.to_string(),
                labels: vec!["player".to_string()],
            },
            Some(&runtime_spec(
                "demo/index.html",
                "http://127.0.0.1:9000/web-runtime/demo/index.html?cache-bust=1",
                true,
                r#"{"speed":{"value":2}}"#,
                &["player"],
            )),
        );

        assert!(matches!(
            plan.session,
            WebSessionPlan::UpdateBridge { ref spec }
                if spec.runtime_id == "demo/index.html"
                    && spec.runtime_url
                        == "http://127.0.0.1:9000/web-runtime/demo/index.html?cache-bust=1"
                    && spec.paused
                    && spec.property_payload_json == r#"{"speed":{"value":2}}"#
        ));
    }

    #[test]
    fn bridge_ready_flushes_first_bootstrap_once() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        let payload = bootstrap_payload(r#"{"speed":{"value":1}}"#, true);
        state.reset_for_navigation(
            "demo/index.html",
            payload.clone(),
            now,
        );

        let batch = state
            .mark_bridge_ready(Some("http://127.0.0.1:9000/web-runtime/demo/index.html"))
            .expect("bootstrap batch after ready");
        assert_eq!(
            batch,
            WebBridgeDispatchBatch {
                properties_json: Some(r#"{"speed":{"value":1}}"#.to_string()),
                paused: Some(true),
                cursor: None,
                audio_samples: None,
            }
        );
        assert!(state.bridge_ready);
        assert!(!state.bootstrap_pending);
        assert_eq!(
            state.last_bootstrap_hash.as_deref(),
            Some(payload.hash.as_str())
        );
        assert!(state
            .mark_bridge_ready(Some("http://127.0.0.1:9000/web-runtime/demo/index.html"))
            .is_none());
    }

    #[test]
    fn bridge_ready_accepts_runtime_href_with_hash_fragment() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        let payload = bootstrap_payload(r#"{"speed":{"value":1}}"#, false);
        state.reset_for_navigation(
            "demo/index.html",
            payload.clone(),
            now,
        );

        let batch = state
            .mark_bridge_ready(Some(
                "http://127.0.0.1:9000/web-runtime/demo/index.html#%5B0,1,2%5D",
            ))
            .expect("bootstrap batch after hash-bearing href");
        assert_eq!(batch, WebBridgeDispatchBatch::bootstrap(&payload));
        assert!(state.bridge_ready);
    }

    #[test]
    fn bootstrap_retries_are_low_frequency_and_never_include_cursor() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        state.reset_for_navigation(
            "demo/index.html",
            bootstrap_payload(r#"{"speed":{"value":1}}"#, false),
            now,
        );

        assert!(state
            .retry_batch_if_due("demo/index.html", now + Duration::from_millis(100))
            .is_none());

        let batch = state
            .retry_batch_if_due("demo/index.html", now + BOOTSTRAP_RETRY_INTERVAL)
            .expect("retry batch");
        assert!(batch.properties_json.is_some());
        assert_eq!(batch.paused, Some(false));
        assert!(batch.cursor.is_none());

        assert!(state
            .retry_batch_if_due(
                "demo/index.html",
                now + BOOTSTRAP_RETRY_INTERVAL + Duration::from_millis(100),
            )
            .is_none());
    }

    #[test]
    fn cursor_dispatch_waits_for_bridge_ready() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        state.reset_for_navigation("demo/index.html", bootstrap_payload("{}", false), now);

        assert!(!state.cursor_dispatch_allowed(false));
        let _ = state.mark_bridge_ready(Some("http://127.0.0.1:9000/web-runtime/demo/index.html"));
        assert!(state.cursor_dispatch_allowed(false));
        assert!(!state.cursor_dispatch_allowed(true));
    }

    #[test]
    fn property_changes_dispatch_incrementally_after_ready() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        state.reset_for_navigation(
            "demo/index.html",
            bootstrap_payload(r#"{"speed":{"value":1}}"#, false),
            now,
        );
        let _ = state.mark_bridge_ready(Some("http://127.0.0.1:9000/web-runtime/demo/index.html"));

        let batch = state
            .update_pending_bootstrap(
                bootstrap_payload(r#"{"speed":{"value":2}}"#, false),
                now + Duration::from_secs(1),
            )
            .expect("incremental properties");
        assert_eq!(
            batch.properties_json.as_deref(),
            Some(r#"{"speed":{"value":2}}"#)
        );
        assert_eq!(batch.paused, None);
        assert!(batch.cursor.is_none());
        assert!(batch.audio_samples.is_none());
    }

    #[test]
    fn paused_changes_dispatch_incrementally_after_ready() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        state.reset_for_navigation(
            "demo/index.html",
            bootstrap_payload(r#"{"speed":{"value":1}}"#, false),
            now,
        );
        let _ = state.mark_bridge_ready(Some("http://127.0.0.1:9000/web-runtime/demo/index.html"));

        let batch = state
            .update_pending_bootstrap(
                bootstrap_payload(r#"{"speed":{"value":1}}"#, true),
                now + Duration::from_secs(1),
            )
            .expect("incremental paused");
        assert_eq!(batch.properties_json, None);
        assert_eq!(batch.paused, Some(true));
        assert!(batch.cursor.is_none());
        assert!(batch.audio_samples.is_none());
    }

    #[test]
    fn audio_listener_state_tracks_runtime_url_and_bridge_readiness() {
        let mut state = NativeWebBridgeState::default();
        let runtime_id = "demo/index.html";
        let runtime_url = "http://127.0.0.1:9000/web-runtime/demo/index.html";
        state.reset_for_navigation(runtime_id, bootstrap_payload("{}", false), Instant::now());

        assert!(!state.audio_dispatch_allowed(false));
        state.mark_audio_listener(true, Some(runtime_url));
        assert!(!state.audio_dispatch_allowed(false));

        let _ = state.mark_bridge_ready(Some(runtime_url));
        assert!(state.audio_dispatch_allowed(false));
        assert!(!state.audio_dispatch_allowed(true));

        state.mark_audio_listener(false, Some(runtime_url));
        assert!(!state.audio_dispatch_allowed(false));
    }

    #[test]
    fn url_switch_resets_bridge_ready_and_pending_state() {
        let mut state = NativeWebBridgeState::default();
        let now = Instant::now();
        state.reset_for_navigation(
            "demo-a/index.html",
            bootstrap_payload("{}", false),
            now,
        );
        let _ =
            state.mark_bridge_ready(Some("http://127.0.0.1:9000/web-runtime/demo-a/index.html"));

        state.reset_for_navigation(
            "demo-b/index.html",
            bootstrap_payload(r#"{"theme":{"value":"b"}}"#, true),
            now + Duration::from_secs(1),
        );

        assert_eq!(
            state.current_runtime_id.as_deref(),
            Some("demo-b/index.html")
        );
        assert!(!state.bridge_ready);
        assert!(state.bootstrap_pending);
        assert_eq!(state.last_bootstrap_hash, None);
    }

    #[test]
    fn bridge_script_separates_state_and_cursor_messages() {
        let state_script = build_bridge_dispatch_script(&WebBridgeDispatchBatch {
            properties_json: Some(r#"{"speed":{"value":1}}"#.to_string()),
            paused: Some(true),
            cursor: None,
            audio_samples: None,
        })
        .expect("state script result")
        .expect("state script");

        assert!(state_script.contains("\"wallpaper:properties\""));
        assert!(state_script.contains("\"wallpaper:paused\""));
        assert!(!state_script.contains("\"wallpaper:cursor\""));

        let cursor_script =
            build_bridge_dispatch_script(&WebBridgeDispatchBatch::cursor(CursorPayload {
                x: 24.5,
                y: 90.0,
            }))
            .expect("cursor script result")
            .expect("cursor script");

        assert!(!cursor_script.contains("\"wallpaper:properties\""));
        assert!(!cursor_script.contains("\"wallpaper:paused\""));
        assert!(cursor_script.contains("\"wallpaper:cursor\""));
        assert!(cursor_script.contains("\"x\":24.5"));
        assert!(cursor_script.contains("\"y\":90.0"));

        let audio_script =
            build_bridge_dispatch_script(&WebBridgeDispatchBatch::audio(&audio_snapshot(true)))
                .expect("audio script result")
                .expect("audio script");

        assert!(audio_script.contains("\"wallpaper:audio\""));
        assert!(audio_script.contains("\"samples\":"));
    }

    #[test]
    fn shared_input_projection_uses_window_bounds_for_multi_screen_cursor() {
        let snapshot = SharedInputSnapshot {
            global_x: 4120.0,
            global_y: 180.0,
            system_x: 4120.0,
            system_y: 1260.0,
            desktop_width: 5120.0,
            desktop_height: 1440.0,
            timestamp_ms: 77,
            active: true,
            modifiers: InputModifierSnapshot::default(),
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
        };

        let cursor = project_shared_input_to_view_bounds(&snapshot, 3840.0, 0.0, 1280.0, 1440.0)
            .expect("projected cursor");

        assert_eq!(cursor.x, 280.0);
        assert_eq!(cursor.y, 180.0);
    }

    #[test]
    fn missing_web_entry_path_returns_error() {
        let record = runtime_record(
            WallpaperRuntime::Web {
                web: WebRuntimeDocument::default(),
            },
            None,
        );

        let error = resolve_active_web_entry_path(Some(&record)).expect_err("missing entry error");
        assert!(error.contains("missing its entry HTML file"));
    }

    #[test]
    fn missing_web_entry_file_returns_error() {
        let record = runtime_record(
            WallpaperRuntime::Web {
                web: WebRuntimeDocument {
                    entry_path: Some("/tmp/definitely-missing-index.html".to_string()),
                    ..WebRuntimeDocument::default()
                },
            },
            None,
        );

        let error = resolve_active_web_entry_path(Some(&record)).expect_err("missing html error");
        assert!(error.contains("/tmp/definitely-missing-index.html"));
    }
}
