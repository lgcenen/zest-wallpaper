use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::services::{
    scene_diagnostics::{
        SceneDiagnosticCategory, SceneDiagnosticDetail, SceneDiagnosticResourceDetail,
    },
    scene_resource_service::SceneResourceRootKind,
};

pub const DIAGNOSTIC_EVENT_NAME: &str = "player:diagnostic";
const MAX_DIAGNOSTICS: usize = 32;
const MAX_ATTEMPTED_CANDIDATES: usize = 12;
const MAX_CONTAINER_ENTRIES: usize = 3;
const MAX_NESTED_SCENE_DETAILS: usize = 4;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeDiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostic {
    pub timestamp_ms: u64,
    pub subsystem: String,
    pub code: String,
    pub severity: RuntimeDiagnosticSeverity,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Default)]
pub struct DiagnosticServiceState {
    diagnostics: Mutex<VecDeque<RuntimeDiagnostic>>,
}

pub fn current_runtime_diagnostics(app: &AppHandle) -> Result<Vec<RuntimeDiagnostic>, String> {
    app.try_state::<DiagnosticServiceState>()
        .map(|state| state.snapshot())
        .unwrap_or_else(|| Ok(Vec::new()))
}

pub fn record_warning(
    app: &AppHandle,
    subsystem: &str,
    code: &str,
    summary: impl Into<String>,
    detail: Option<String>,
) -> Result<(), String> {
    record_diagnostic(
        app,
        RuntimeDiagnosticSeverity::Warning,
        subsystem,
        code,
        summary.into(),
        detail,
    )
}

pub fn record_error(
    app: &AppHandle,
    subsystem: &str,
    code: &str,
    summary: impl Into<String>,
    detail: Option<String>,
) -> Result<(), String> {
    record_diagnostic(
        app,
        RuntimeDiagnosticSeverity::Error,
        subsystem,
        code,
        summary.into(),
        detail,
    )
}

pub fn clear_diagnostic(app: &AppHandle, subsystem: &str, code: &str) -> Result<bool, String> {
    mutate_and_emit(app, |state| {
        state.clear_matching(|entry| entry.subsystem == subsystem && entry.code == code)
    })
}

pub fn clear_subsystem(app: &AppHandle, subsystem: &str) -> Result<bool, String> {
    mutate_and_emit(app, |state| {
        state.clear_matching(|entry| entry.subsystem == subsystem)
    })
}

fn record_diagnostic(
    app: &AppHandle,
    severity: RuntimeDiagnosticSeverity,
    subsystem: &str,
    code: &str,
    summary: String,
    detail: Option<String>,
) -> Result<(), String> {
    let diagnostic = RuntimeDiagnostic {
        timestamp_ms: timestamp_ms(),
        subsystem: subsystem.to_string(),
        code: code.to_string(),
        severity,
        summary,
        detail,
    };

    emit_terminal_diagnostic("record", &diagnostic);
    let _ = mutate_and_emit(app, |state| state.upsert(diagnostic))?;
    Ok(())
}

fn emit_terminal_diagnostic(action: &str, diagnostic: &RuntimeDiagnostic) {
    eprintln!(
        "[zest-diagnostic] action={} severity={:?} subsystem={} code={} summary={}",
        action, diagnostic.severity, diagnostic.subsystem, diagnostic.code, diagnostic.summary
    );
    if let Some(detail) = diagnostic.detail.as_deref() {
        emit_terminal_diagnostic_detail(detail);
    }
}

fn emit_terminal_diagnostic_detail(detail: &str) {
    for line in format_terminal_diagnostic_detail(detail) {
        eprintln!("{line}");
    }
}

fn format_terminal_diagnostic_detail(detail: &str) -> Vec<String> {
    if let Ok(scene_detail) = serde_json::from_str::<SceneDiagnosticDetail>(detail) {
        return format_scene_diagnostic_detail(&scene_detail);
    }

    match serde_json::from_str::<Value>(detail) {
        Ok(value) => format_json_detail_container(&value, detail),
        Err(_) => vec![format!("[zest-diagnostic-detail] {}", detail)],
    }
}

fn format_scene_diagnostic_detail(detail: &SceneDiagnosticDetail) -> Vec<String> {
    let mut lines = vec![format!(
        "[zest-diagnostic-detail] category={:?} domain={:?}",
        detail.category, detail.domain
    )];

    match detail.category {
        SceneDiagnosticCategory::Resource => {
            if let Some(resource) = detail.resource.as_ref() {
                lines.extend(format_scene_resource_detail(resource));
            }
        }
        SceneDiagnosticCategory::Runtime | SceneDiagnosticCategory::Capability => {
            if let Some(stage) = detail.runtime_stage.as_deref() {
                lines.push(format!("  stage: {}", stage));
            }
            if let Some(reason) = detail.reason.as_deref() {
                lines.push(format!("  reason: {}", reason));
            }
            if let Some(underlying) = detail.underlying_diagnostic.as_deref() {
                lines.push(format!("  underlying: {}", underlying));
            }
        }
    }

    for note in &detail.notes {
        lines.push(format!("  note: {}", note));
    }

    lines
}

fn format_json_detail_container(value: &Value, raw_detail: &str) -> Vec<String> {
    let mut lines = match value {
        Value::Array(entries) => format_array_detail_container(entries),
        Value::Object(object) if looks_like_scene_support_report(object) => {
            format_scene_support_report_container(object)
        }
        Value::Object(object) => format_object_detail_container(object),
        _ => Vec::new(),
    };

    if lines.is_empty() {
        lines.push(format!(
            "[zest-diagnostic-detail] {}",
            compact_raw_detail(raw_detail)
        ));
    } else {
        lines.push(format!(
            "[zest-diagnostic-detail-raw] {}",
            compact_raw_detail(raw_detail)
        ));
    }

    lines
}

fn format_array_detail_container(entries: &[Value]) -> Vec<String> {
    let mut lines = vec![format!(
        "[zest-diagnostic-detail] json=array entries={}",
        entries.len()
    )];

    for (index, entry) in entries.iter().take(MAX_CONTAINER_ENTRIES).enumerate() {
        if let Some(summary) = summarize_diagnostic_entry(entry) {
            lines.push(format!("  entry[{index}]: {summary}"));
        }
    }
    append_remaining_count(&mut lines, entries.len(), MAX_CONTAINER_ENTRIES, "entry");
    append_nested_scene_details(&mut lines, entries.iter());
    lines
}

fn format_scene_support_report_container(object: &serde_json::Map<String, Value>) -> Vec<String> {
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let errors = object
        .get("errors")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let warnings = object
        .get("warnings")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let resource_roots = object
        .get("resourceRoots")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    let mut lines = vec![format!(
        "[zest-diagnostic-detail] json=scene-support-report title={title:?} errors={errors} warnings={warnings} resourceRoots={resource_roots}"
    )];

    append_report_entries(&mut lines, object, "errors", "error");
    append_report_entries(&mut lines, object, "warnings", "warning");

    let entries = object
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            object
                .get("warnings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    append_nested_scene_details(&mut lines, entries);
    lines
}

fn format_object_detail_container(object: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(summary) = summarize_diagnostic_entry(&Value::Object(object.clone())) else {
        return lines;
    };

    lines.push(format!("[zest-diagnostic-detail] json=object {summary}"));
    let value = Value::Object(object.clone());
    append_nested_scene_details(&mut lines, std::iter::once(&value));
    lines
}

fn append_report_entries(
    lines: &mut Vec<String>,
    object: &serde_json::Map<String, Value>,
    field: &str,
    label: &str,
) {
    let Some(entries) = object.get(field).and_then(Value::as_array) else {
        return;
    };

    for (index, entry) in entries.iter().take(MAX_CONTAINER_ENTRIES).enumerate() {
        if let Some(summary) = summarize_diagnostic_entry(entry) {
            lines.push(format!("  {label}[{index}]: {summary}"));
        }
    }
    append_remaining_count(lines, entries.len(), MAX_CONTAINER_ENTRIES, label);
}

fn append_remaining_count(lines: &mut Vec<String>, total: usize, shown: usize, label: &str) {
    let remaining = total.saturating_sub(shown);
    if remaining > 0 {
        lines.push(format!("  {label}: and {remaining} more"));
    }
}

fn append_nested_scene_details<'a>(
    lines: &mut Vec<String>,
    entries: impl Iterator<Item = &'a Value>,
) {
    let mut emitted = 0usize;
    let mut total = 0usize;

    for entry in entries {
        let Some(detail) = entry.get("detail") else {
            continue;
        };
        let Ok(scene_detail) = serde_json::from_value::<SceneDiagnosticDetail>(detail.clone())
        else {
            continue;
        };

        total += 1;
        if emitted >= MAX_NESTED_SCENE_DETAILS {
            continue;
        }

        if let Some(summary) = summarize_diagnostic_entry(entry) {
            lines.push(format!("  nested detail from {summary}:"));
        } else {
            lines.push("  nested detail:".to_string());
        }
        lines.extend(
            format_scene_diagnostic_detail(&scene_detail)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        emitted += 1;
    }

    let remaining = total.saturating_sub(emitted);
    if remaining > 0 {
        lines.push(format!("  nested detail: and {remaining} more"));
    }
}

fn summarize_diagnostic_entry(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    let code = object.get("code").and_then(Value::as_str);
    let severity = object.get("severity").and_then(Value::as_str);
    let message = object
        .get("message")
        .or_else(|| object.get("summary"))
        .and_then(Value::as_str);

    match (severity, code, message) {
        (Some(severity), Some(code), Some(message)) => Some(format!(
            "severity={severity} code={code} message={message:?}"
        )),
        (Some(severity), Some(code), None) => Some(format!("severity={severity} code={code}")),
        (None, Some(code), Some(message)) => Some(format!("code={code} message={message:?}")),
        (None, Some(code), None) => Some(format!("code={code}")),
        (Some(severity), None, Some(message)) => {
            Some(format!("severity={severity} message={message:?}"))
        }
        (None, None, Some(message)) => Some(format!("message={message:?}")),
        _ => None,
    }
}

fn looks_like_scene_support_report(object: &serde_json::Map<String, Value>) -> bool {
    object.contains_key("wallpaperId")
        && object.contains_key("errors")
        && object.contains_key("warnings")
}

fn compact_raw_detail(detail: &str) -> String {
    serde_json::from_str::<Value>(detail)
        .ok()
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_else(|| detail.to_string())
}

fn format_scene_resource_detail(resource: &SceneDiagnosticResourceDetail) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push(format!(
        "  authored reference: {}",
        resource.authored_reference
    ));
    lines.push(format!(
        "  resolved: {}",
        if resource.reference_resolved {
            "yes"
        } else {
            "no"
        }
    ));
    lines.push(format!(
        "  external assets: {}",
        external_assets_state(resource)
    ));
    lines.push(format!(
        "  builtin assets: {}",
        builtin_assets_state(resource)
    ));
    lines.push(format!(
        "  matched root: {}",
        resource
            .matched_root_kind
            .map(|kind| format!("{kind:?}"))
            .unwrap_or_else(|| "none".to_string())
    ));
    lines.push(format!(
        "  matched path: {}",
        resource.matched_path.as_deref().unwrap_or("none")
    ));
    lines.push(format!(
        "  present but unsupported: {}",
        if resource.present_but_unsupported {
            "yes"
        } else {
            "no"
        }
    ));
    if !resource.family_candidates.is_empty() {
        lines.push(format!(
            "  font family candidates: {}",
            resource.family_candidates.join(", ")
        ));
    }

    let meaning = scene_resource_detail_meaning(resource);
    lines.push(format!("  meaning: {}", meaning));

    if !resource.attempted_roots.is_empty() {
        lines.push("  attempted roots:".to_string());
        for root in &resource.attempted_roots {
            lines.push(format!(
                "    - {:?}: {} exists={} searchable={}",
                root.kind,
                root.path.display(),
                root.exists,
                root.searchable
            ));
        }
    }

    if !resource.attempted_candidates.is_empty() {
        lines.push("  attempted candidates:".to_string());
        for candidate in resource
            .attempted_candidates
            .iter()
            .take(MAX_ATTEMPTED_CANDIDATES)
        {
            lines.push(format!("    - {}", candidate));
        }
        let remaining = resource
            .attempted_candidates
            .len()
            .saturating_sub(MAX_ATTEMPTED_CANDIDATES);
        if remaining > 0 {
            lines.push(format!("    - and {} more", remaining));
        }
    }

    lines
}

fn external_assets_state(resource: &SceneDiagnosticResourceDetail) -> &'static str {
    if resource.matched_root_kind == Some(SceneResourceRootKind::ExternalAssets) {
        "matched"
    } else if resource.external_assets_available {
        "available (not matched)"
    } else {
        "not mounted or unavailable"
    }
}

fn builtin_assets_state(resource: &SceneDiagnosticResourceDetail) -> &'static str {
    if resource.matched_root_kind == Some(SceneResourceRootKind::BuiltinAssets) {
        "matched"
    } else if resource.builtin_assets_available {
        "available (not matched)"
    } else {
        "unavailable"
    }
}

fn scene_resource_detail_meaning(resource: &SceneDiagnosticResourceDetail) -> &'static str {
    if resource.present_but_unsupported {
        return "resource was found, but this Scene capability is not implemented yet";
    }

    if resource.reference_resolved {
        return match resource.matched_root_kind {
            Some(SceneResourceRootKind::ExternalAssets) => {
                "resource resolved from mounted external assets"
            }
            Some(SceneResourceRootKind::BuiltinAssets) => {
                "resource resolved from builtin compatibility assets"
            }
            Some(_) => "resource resolved from Scene-local content",
            None => "resource resolved",
        };
    }

    if !resource.external_assets_available {
        "resource unresolved; external assets are not mounted or unavailable, so compatibility may be reduced"
    } else {
        "resource unresolved even though external assets are mounted; the mounted assets may not contain this reference"
    }
}

fn mutate_and_emit<F>(app: &AppHandle, mutate: F) -> Result<bool, String>
where
    F: FnOnce(&DiagnosticServiceState) -> Result<bool, String>,
{
    let Some(state) = app.try_state::<DiagnosticServiceState>() else {
        return Ok(false);
    };
    let changed = mutate(&state)?;
    if changed {
        let snapshot = state.snapshot()?;
        app.emit(DIAGNOSTIC_EVENT_NAME, snapshot)
            .map_err(|error| error.to_string())?;
    }
    Ok(changed)
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

impl DiagnosticServiceState {
    fn snapshot(&self) -> Result<Vec<RuntimeDiagnostic>, String> {
        self.diagnostics
            .lock()
            .map(|diagnostics| diagnostics.iter().cloned().collect())
            .map_err(|error| error.to_string())
    }

    fn upsert(&self, diagnostic: RuntimeDiagnostic) -> Result<bool, String> {
        let mut diagnostics = self.diagnostics.lock().map_err(|error| error.to_string())?;
        if let Some(index) = diagnostics.iter().position(|entry| {
            entry.subsystem == diagnostic.subsystem && entry.code == diagnostic.code
        }) {
            diagnostics.remove(index);
        }
        diagnostics.push_front(diagnostic);
        while diagnostics.len() > MAX_DIAGNOSTICS {
            diagnostics.pop_back();
        }
        Ok(true)
    }

    fn clear_matching<F>(&self, predicate: F) -> Result<bool, String>
    where
        F: Fn(&RuntimeDiagnostic) -> bool,
    {
        let mut diagnostics = self.diagnostics.lock().map_err(|error| error.to_string())?;
        let before = diagnostics.len();
        diagnostics.retain(|entry| !predicate(entry));
        Ok(before != diagnostics.len())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use crate::services::{
        scene_diagnostics::{
            SceneDiagnosticCategory, SceneDiagnosticDetail, SceneDiagnosticDomain,
            SceneDiagnosticResourceDetail,
        },
        scene_resource_service::{SceneResourceRoot, SceneResourceRootKind},
    };

    use super::{
        format_scene_resource_detail, format_terminal_diagnostic_detail, DiagnosticServiceState,
        RuntimeDiagnostic, RuntimeDiagnosticSeverity, MAX_DIAGNOSTICS,
    };

    fn diagnostic(subsystem: &str, code: &str, summary: &str) -> RuntimeDiagnostic {
        RuntimeDiagnostic {
            timestamp_ms: 64,
            subsystem: subsystem.to_string(),
            code: code.to_string(),
            severity: RuntimeDiagnosticSeverity::Warning,
            summary: summary.to_string(),
            detail: None,
        }
    }

    fn resource_detail(
        external_assets_available: bool,
        builtin_assets_available: bool,
        matched_root_kind: Option<SceneResourceRootKind>,
        matched_path: Option<&str>,
        present_but_unsupported: bool,
    ) -> SceneDiagnosticResourceDetail {
        SceneDiagnosticResourceDetail {
            authored_reference: "assets/effects/missing.effect".to_string(),
            attempted_roots: vec![
                SceneResourceRoot {
                    kind: SceneResourceRootKind::ExternalAssets,
                    path: PathBuf::from("external-assets"),
                    exists: external_assets_available,
                    searchable: true,
                },
                SceneResourceRoot {
                    kind: SceneResourceRootKind::BuiltinAssets,
                    path: PathBuf::from("builtin-assets"),
                    exists: builtin_assets_available,
                    searchable: true,
                },
            ],
            attempted_candidates: (0..14)
                .map(|index| format!("candidate-{index}.effect"))
                .collect(),
            matched_root_kind,
            matched_path: matched_path.map(ToString::to_string),
            external_assets_available,
            builtin_assets_available,
            reference_resolved: matched_path.is_some(),
            present_but_unsupported,
            family_candidates: Vec::new(),
            font_reference_kind: None,
        }
    }

    fn joined(lines: Vec<String>) -> String {
        lines.join("\n")
    }

    #[test]
    fn runtime_diagnostic_api_serializes_existing_shape() {
        let diagnostic = RuntimeDiagnostic {
            timestamp_ms: 123,
            subsystem: "scene-support".to_string(),
            code: "apply-warning".to_string(),
            severity: RuntimeDiagnosticSeverity::Warning,
            summary: "Scene entered with warnings.".to_string(),
            detail: Some("{\"kind\":\"raw\"}".to_string()),
        };

        assert_eq!(
            serde_json::to_value(&diagnostic).expect("serialize diagnostic"),
            json!({
                "timestampMs": 123,
                "subsystem": "scene-support",
                "code": "apply-warning",
                "severity": "warning",
                "summary": "Scene entered with warnings.",
                "detail": "{\"kind\":\"raw\"}",
            })
        );
    }

    #[test]
    fn scene_diagnostic_detail_round_trips_through_json() {
        let detail = SceneDiagnosticDetail::resource(
            SceneDiagnosticDomain::Visual,
            resource_detail(
                true,
                true,
                Some(SceneResourceRootKind::ExternalAssets),
                Some("external-assets/assets/effects/missing.effect"),
                true,
            ),
        )
        .with_note("Resource exists but is outside the current phase capability boundary.");

        let encoded = serde_json::to_string(&detail).expect("serialize detail");
        let decoded: SceneDiagnosticDetail =
            serde_json::from_str(&encoded).expect("deserialize detail");

        assert_eq!(decoded, detail);
        assert_eq!(decoded.category, SceneDiagnosticCategory::Resource);
    }

    #[test]
    fn terminal_detail_formatter_falls_back_for_non_scene_detail() {
        let lines = format_terminal_diagnostic_detail(
            r#"{"kind":"native-video","payload":{"reason":"other runtime detail"}}"#,
        );

        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("[zest-diagnostic-detail] {\"kind\":\"native-video\""));
    }

    #[test]
    fn resource_detail_formatter_distinguishes_external_asset_states() {
        let not_mounted = joined(format_scene_resource_detail(&resource_detail(
            false, true, None, None, false,
        )));
        assert!(not_mounted.contains("external assets: not mounted or unavailable"));
        assert!(not_mounted.contains(
            "meaning: resource unresolved; external assets are not mounted or unavailable"
        ));

        let available_miss = joined(format_scene_resource_detail(&resource_detail(
            true, true, None, None, false,
        )));
        assert!(available_miss.contains("external assets: available (not matched)"));
        assert!(available_miss
            .contains("meaning: resource unresolved even though external assets are mounted"));

        let matched_external = joined(format_scene_resource_detail(&resource_detail(
            true,
            true,
            Some(SceneResourceRootKind::ExternalAssets),
            Some("external-assets/assets/effects/missing.effect"),
            false,
        )));
        assert!(matched_external.contains("external assets: matched"));
        assert!(matched_external.contains("matched root: ExternalAssets"));
        assert!(
            matched_external.contains("meaning: resource resolved from mounted external assets")
        );

        let unsupported = joined(format_scene_resource_detail(&resource_detail(
            true,
            true,
            Some(SceneResourceRootKind::ExternalAssets),
            Some("external-assets/assets/effects/missing.effect"),
            true,
        )));
        assert!(unsupported.contains("present but unsupported: yes"));
        assert!(unsupported.contains(
            "meaning: resource was found, but this Scene capability is not implemented yet"
        ));
        assert!(unsupported.contains("    - and 2 more"));
    }

    #[test]
    fn terminal_detail_formatter_summarizes_reports_and_keeps_raw_fallback() {
        let detail = SceneDiagnosticDetail::resource(
            SceneDiagnosticDomain::Text,
            resource_detail(false, true, None, None, false),
        );
        let report = json!({
            "wallpaperId": "scene-a",
            "title": "Synthetic Scene",
            "sceneJsonPath": "extracted/scene.json",
            "resourceRoots": [],
            "errors": [],
            "warnings": [{
                "severity": "warning",
                "code": "text-font-reference-unresolved",
                "message": "Synthetic Scene could not resolve a font.",
                "detail": detail
            }]
        });
        let raw = serde_json::to_string_pretty(&report).expect("report json");

        let text = joined(format_terminal_diagnostic_detail(&raw));

        assert!(text.contains("json=scene-support-report"));
        assert!(text.contains("warnings=1"));
        assert!(text.contains("warning[0]: severity=warning code=text-font-reference-unresolved"));
        assert!(text
            .contains("nested detail from severity=warning code=text-font-reference-unresolved"));
        assert!(text.contains("authored reference: assets/effects/missing.effect"));
        assert!(text.contains("[zest-diagnostic-detail-raw]"));
    }

    #[test]
    fn terminal_detail_formatter_summarizes_warning_arrays_without_expanding_all() {
        let detail = SceneDiagnosticDetail::resource(
            SceneDiagnosticDomain::Particle,
            resource_detail(true, true, None, None, false),
        );
        let raw = serde_json::to_string_pretty(&json!([
            {
                "code": "particle-resource-reference-unresolved",
                "message": "Particle resource was not found.",
                "detail": detail
            },
            {
                "code": "particle-resource-reference-unresolved",
                "message": "Another particle resource was not found."
            },
            {
                "code": "particle-resource-reference-unresolved",
                "message": "Third particle resource was not found."
            },
            {
                "code": "particle-resource-reference-unresolved",
                "message": "Fourth particle resource was not found."
            }
        ]))
        .expect("warning array json");

        let text = joined(format_terminal_diagnostic_detail(&raw));

        assert!(text.contains("json=array entries=4"));
        assert!(text.contains("entry[0]: code=particle-resource-reference-unresolved"));
        assert!(text.contains("entry: and 1 more"));
        assert!(text.contains("category=Resource domain=Particle"));
        assert!(text.contains("[zest-diagnostic-detail-raw]"));
    }

    #[test]
    fn upsert_replaces_existing_code_and_keeps_latest_at_front() {
        let state = DiagnosticServiceState::default();
        state
            .upsert(diagnostic("native-web", "sync-failed", "first"))
            .expect("insert first diagnostic");
        state
            .upsert(diagnostic("shared-audio", "capture-unavailable", "audio"))
            .expect("insert audio diagnostic");
        state
            .upsert(diagnostic("native-web", "sync-failed", "second"))
            .expect("replace first diagnostic");

        let snapshot = state.snapshot().expect("snapshot diagnostics");
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].subsystem, "native-web");
        assert_eq!(snapshot[0].summary, "second");
        assert_eq!(snapshot[1].subsystem, "shared-audio");
    }

    #[test]
    fn clear_matching_removes_only_targeted_diagnostics() {
        let state = DiagnosticServiceState::default();
        state
            .upsert(diagnostic("native-web", "sync-failed", "web"))
            .expect("insert web diagnostic");
        state
            .upsert(diagnostic("native-video", "sync-failed", "video"))
            .expect("insert video diagnostic");

        let changed = state
            .clear_matching(|entry| entry.subsystem == "native-web")
            .expect("clear matching diagnostics");

        assert!(changed);
        let snapshot = state.snapshot().expect("snapshot diagnostics");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].subsystem, "native-video");
    }

    #[test]
    fn diagnostics_ring_buffer_stays_bounded() {
        let state = DiagnosticServiceState::default();
        for index in 0..(MAX_DIAGNOSTICS + 8) {
            state
                .upsert(diagnostic(
                    "native-web",
                    &format!("code-{index}"),
                    &format!("summary-{index}"),
                ))
                .expect("insert bounded diagnostic");
        }

        let snapshot = state.snapshot().expect("snapshot diagnostics");
        assert_eq!(snapshot.len(), MAX_DIAGNOSTICS);
        assert_eq!(snapshot[0].code, format!("code-{}", MAX_DIAGNOSTICS + 7));
        assert_eq!(snapshot[MAX_DIAGNOSTICS - 1].code, "code-8");
    }
}
