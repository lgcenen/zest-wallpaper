use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Local};
use quickjs_rs::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::models::SceneTextLayer;

#[derive(Debug, Clone, Default)]
struct SceneTextScriptRuntimeEntry {
    script_hash: u64,
    property_hash: u64,
    state_json: String,
    rendered_text: Option<String>,
    initialized: bool,
    diagnostics: Vec<SceneTextScriptDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneTextScriptDiagnostic {
    pub object_id: u32,
    pub object_name: String,
    pub code: String,
    pub message: String,
    pub runtime_stage: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTextScriptEvaluation {
    pub rendered_text: Option<String>,
    pub diagnostics: Vec<SceneTextScriptDiagnostic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SceneTextScriptExecutionOutput {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    state: Map<String, Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_stage: Option<String>,
    #[serde(default)]
    update_entry_missing: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneTextScriptLayerHost {
    name: String,
    text: String,
    visible: bool,
    alpha: f64,
    point_size: f64,
}

fn scene_text_script_runtime_cache() -> &'static Mutex<BTreeMap<String, SceneTextScriptRuntimeEntry>>
{
    static CACHE: OnceLock<Mutex<BTreeMap<String, SceneTextScriptRuntimeEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn evaluate_scripted_text_layer(
    runtime_owner_key: Option<&str>,
    layer: &SceneTextLayer,
    properties: &BTreeMap<String, Value>,
    now: &DateTime<Local>,
) -> Result<Option<String>, String> {
    Ok(
        evaluate_scripted_text_layer_detailed(runtime_owner_key, layer, properties, now)?
            .rendered_text,
    )
}

pub fn text_script_requires_runtime(script_text: Option<&str>) -> bool {
    let lower_script = script_text.unwrap_or_default().to_ascii_lowercase();
    lower_script.contains("export function update")
        || lower_script.contains("function update")
        || lower_script.contains("function init")
        || lower_script.contains("module.update")
        || lower_script.contains("module.init")
        || lower_script.contains("module.exports.update")
        || lower_script.contains("module.exports.init")
        || lower_script.contains("export default")
        || lower_script.contains("export function applyuserproperties")
        || lower_script.contains("function applyuserproperties")
        || lower_script.contains("module.applyuserproperties")
        || lower_script.contains("module.exports.applyuserproperties")
        || lower_script.contains("thislayer.")
        || lower_script.contains("engine.userproperties")
}

pub fn evaluate_scripted_text_layer_detailed(
    runtime_owner_key: Option<&str>,
    layer: &SceneTextLayer,
    properties: &BTreeMap<String, Value>,
    now: &DateTime<Local>,
) -> Result<SceneTextScriptEvaluation, String> {
    let script = layer
        .script_text
        .as_deref()
        .map(str::trim)
        .filter(|script| !script.is_empty())
        .ok_or_else(|| format!("Text layer {} has no script text", layer.name))?;

    let cache_key = runtime_owner_key.map(|owner| format!("{owner}:{}", layer.id));
    let script_hash = stable_hash(script);
    let property_hash = stable_hash_json(properties);

    let cached = load_cached_script_state(cache_key.as_deref(), script_hash, property_hash)?;
    let previous_text = cached.previous_rendered_text.as_deref();

    let execution = execute_scripted_text_layer(
        script,
        layer,
        properties,
        now,
        previous_text,
        cached.previous_state_json.as_deref().unwrap_or("{}"),
        cached.should_apply_user_properties,
        cached.should_init,
    );

    let (rendered_text, state, diagnostics) = match execution {
        Ok(execution) => {
            let mut diagnostics = Vec::new();
            if execution.update_entry_missing {
                diagnostics.push(script_diagnostic(
                    layer,
                    "text-script-entry-missing",
                    "update",
                    "Text script did not expose update() or module.update; previous text was preserved.",
                ));
            }
            if let Some(error) = execution.error.as_ref() {
                diagnostics.push(script_diagnostic(
                    layer,
                    "text-script-run-failed",
                    execution.error_stage.as_deref().unwrap_or("update"),
                    error,
                ));
            }

            let rendered_text = execution
                .result
                .as_ref()
                .and_then(string_from_json_value)
                .or(execution.text.clone())
                .or_else(|| previous_text.map(ToString::to_string));
            (rendered_text, execution.state, diagnostics)
        }
        Err(error) => {
            let diagnostics = vec![script_diagnostic(
                layer,
                "text-script-compile-failed",
                "compile",
                &error,
            )];
            let fallback_text = cached
                .previous_rendered_text
                .or_else(|| Some(layer.content.clone()));
            (fallback_text, Map::new(), diagnostics)
        }
    };

    if let Some(cache_key) = cache_key.as_deref() {
        save_cached_script_state(
            cache_key,
            script_hash,
            property_hash,
            state,
            rendered_text.clone(),
            true,
            diagnostics.clone(),
        )?;
    }

    Ok(SceneTextScriptEvaluation {
        rendered_text,
        diagnostics,
    })
}

#[derive(Debug, Clone, Default)]
struct SceneTextScriptCachedState {
    previous_state_json: Option<String>,
    previous_rendered_text: Option<String>,
    should_apply_user_properties: bool,
    should_init: bool,
}

fn load_cached_script_state(
    cache_key: Option<&str>,
    script_hash: u64,
    property_hash: u64,
) -> Result<SceneTextScriptCachedState, String> {
    let Some(cache_key) = cache_key else {
        return Ok(SceneTextScriptCachedState {
            should_apply_user_properties: true,
            should_init: true,
            ..SceneTextScriptCachedState::default()
        });
    };

    let mut cache = scene_text_script_runtime_cache()
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(entry) = cache.get(cache_key) else {
        return Ok(SceneTextScriptCachedState {
            should_apply_user_properties: true,
            should_init: true,
            ..SceneTextScriptCachedState::default()
        });
    };
    if entry.script_hash != script_hash {
        cache.remove(cache_key);
        return Ok(SceneTextScriptCachedState {
            should_apply_user_properties: true,
            should_init: true,
            ..SceneTextScriptCachedState::default()
        });
    }

    Ok(SceneTextScriptCachedState {
        previous_state_json: Some(entry.state_json.clone()),
        previous_rendered_text: entry.rendered_text.clone(),
        should_apply_user_properties: entry.property_hash != property_hash,
        should_init: !entry.initialized,
    })
}

fn save_cached_script_state(
    cache_key: &str,
    script_hash: u64,
    property_hash: u64,
    state: Map<String, Value>,
    rendered_text: Option<String>,
    initialized: bool,
    diagnostics: Vec<SceneTextScriptDiagnostic>,
) -> Result<(), String> {
    let state_json = serde_json::to_string(&state).map_err(|error| error.to_string())?;
    let mut cache = scene_text_script_runtime_cache()
        .lock()
        .map_err(|error| error.to_string())?;
    cache.insert(
        cache_key.to_string(),
        SceneTextScriptRuntimeEntry {
            script_hash,
            property_hash,
            state_json,
            rendered_text,
            initialized,
            diagnostics,
        },
    );
    Ok(())
}

fn execute_scripted_text_layer(
    script: &str,
    layer: &SceneTextLayer,
    properties: &BTreeMap<String, Value>,
    now: &DateTime<Local>,
    previous_rendered_text: Option<&str>,
    previous_state_json: &str,
    should_apply_user_properties: bool,
    should_init: bool,
) -> Result<SceneTextScriptExecutionOutput, String> {
    let context = Context::new().map_err(|error| error.to_string())?;
    let property_json = serde_json::to_string(properties).map_err(|error| error.to_string())?;
    let tracked_state_keys_json = serde_json::to_string(&tracked_script_state_keys(script))
        .map_err(|error| error.to_string())?;
    let this_layer_json = serde_json::to_string(&SceneTextScriptLayerHost {
        name: layer.name.clone(),
        text: previous_rendered_text.unwrap_or(&layer.content).to_string(),
        visible: layer.visible,
        alpha: layer.alpha.unwrap_or(1.0),
        point_size: layer.point_size.unwrap_or(24.0),
    })
    .map_err(|error| error.to_string())?;
    let now_iso = now.to_rfc3339();
    let source = format!(
        "{}\n{}\n{}",
        TEXT_SCRIPT_BOOTSTRAP,
        normalize_script_source(script),
        TEXT_SCRIPT_RUNNER
    );

    register_script_global(&context, "__hostUserPropertiesJson", property_json)?;
    register_script_global(
        &context,
        "__hostTrackedStateKeysJson",
        tracked_state_keys_json,
    )?;
    register_script_global(&context, "__hostThisLayerJson", this_layer_json)?;
    register_script_global(&context, "__hostStateJson", previous_state_json.to_string())?;
    register_script_global(&context, "__hostNowIso", now_iso)?;
    register_script_global(
        &context,
        "__hostShouldApplyUserProperties",
        should_apply_user_properties,
    )?;
    register_script_global(&context, "__hostShouldInit", should_init)?;

    let output_json = context
        .eval_as::<String>(&source)
        .map_err(|error| error.to_string())?;
    let parsed = serde_json::from_str::<SceneTextScriptExecutionOutput>(&output_json)
        .map_err(|error| format!("Failed to decode text script output: {error}"))?;

    Ok(parsed)
}

fn register_script_global<V>(context: &Context, key: &str, value: V) -> Result<(), String>
where
    V: Into<quickjs_rs::JsValue>,
{
    context
        .set_global(key, value)
        .map_err(|error| error.to_string())
}

fn normalize_script_source(script: &str) -> String {
    let mut normalized = String::new();

    for line in script.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import ") {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("export default ") {
            let indent_len = line.len() - trimmed.len();
            normalized.push_str(&line[..indent_len]);
            normalized.push_str("__hostDefaultModule = ");
            normalized.push_str(rest);
        } else if trimmed.starts_with("export ") {
            normalized.push_str(line.replacen("export ", "", 1).as_str());
        } else {
            normalized.push_str(line);
        }
        normalized.push('\n');
    }

    normalized
}

fn script_diagnostic(
    layer: &SceneTextLayer,
    code: &str,
    runtime_stage: &str,
    reason: &str,
) -> SceneTextScriptDiagnostic {
    let message = if code == "text-script-entry-missing" {
        format!(
            "Scene text {} script is missing the {runtime_stage} entry.",
            layer.name
        )
    } else {
        format!("Scene text {} script {runtime_stage} failed.", layer.name)
    };
    SceneTextScriptDiagnostic {
        object_id: layer.id,
        object_name: layer.name.clone(),
        code: code.to_string(),
        message,
        runtime_stage: runtime_stage.to_string(),
        reason: reason.to_string(),
    }
}

pub fn scene_text_script_runtime_diagnostics(
    runtime_owner_key: Option<&str>,
) -> Vec<SceneTextScriptDiagnostic> {
    let Some(runtime_owner_key) = runtime_owner_key else {
        return Vec::new();
    };
    let prefix = format!("{runtime_owner_key}:");
    let Ok(cache) = scene_text_script_runtime_cache().lock() else {
        return Vec::new();
    };

    cache
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .flat_map(|(_, entry)| entry.diagnostics.clone())
        .collect()
}

pub fn retain_scene_text_script_runtime_owner_layers(
    runtime_owner_key: Option<&str>,
    active_layer_ids: &BTreeSet<u32>,
) {
    let Some(runtime_owner_key) = runtime_owner_key else {
        return;
    };
    let prefix = format!("{runtime_owner_key}:");
    let Ok(mut cache) = scene_text_script_runtime_cache().lock() else {
        return;
    };

    cache.retain(|key, _| {
        if !key.starts_with(&prefix) {
            return true;
        }
        let Some(layer_id) = key
            .strip_prefix(&prefix)
            .and_then(|value| value.parse::<u32>().ok())
        else {
            return false;
        };
        active_layer_ids.contains(&layer_id)
    });
}

fn tracked_script_state_keys(script: &str) -> Vec<String> {
    let mut keys = Vec::new();

    for line in script.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("var ") else {
            continue;
        };
        for segment in rest.split(',') {
            let candidate = segment
                .split('=')
                .next()
                .unwrap_or_default()
                .trim()
                .trim_end_matches(';');
            if candidate.is_empty() {
                continue;
            }
            if !keys.iter().any(|existing| existing == candidate) {
                keys.push(candidate.to_string());
            }
        }
    }

    keys
}

fn stable_hash(input: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

fn stable_hash_json(properties: &BTreeMap<String, Value>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(properties)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

fn string_from_json_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(_) | Value::Object(_) => Some(value.to_string()),
    }
}

const TEXT_SCRIPT_BOOTSTRAP: &str = r#"
const __hostBaselineKeys = new Set(Object.keys(globalThis));
const __hostTrackedStateKeys = JSON.parse(__hostTrackedStateKeysJson || "[]");
const __hostUserProperties = JSON.parse(__hostUserPropertiesJson || "{}");
const __hostThisLayer = JSON.parse(__hostThisLayerJson || "{}");
const __hostState = JSON.parse(__hostStateJson || "{}");
const __hostOriginalDate = globalThis.Date;
var __hostDefaultModule = null;

globalThis.console = globalThis.console || { log() {}, warn() {}, error() {} };
globalThis.module = globalThis.module || {};
globalThis.module.exports = globalThis.module.exports || {};
globalThis.exports = globalThis.module.exports;

function __hostFixedDate(...args) {
  if (new.target) {
    if (args.length === 0) {
      return new __hostOriginalDate(__hostNowIso);
    }
    return new __hostOriginalDate(...args);
  }

  if (args.length === 0) {
    return new __hostOriginalDate(__hostNowIso).toString();
  }
  return __hostOriginalDate(...args);
}

__hostFixedDate.now = function now() {
  return new __hostOriginalDate(__hostNowIso).getTime();
};
__hostFixedDate.parse = __hostOriginalDate.parse;
__hostFixedDate.UTC = __hostOriginalDate.UTC;
__hostFixedDate.prototype = __hostOriginalDate.prototype;

globalThis.Date = __hostFixedDate;
globalThis.engine = {
  userProperties: __hostUserProperties,
  frametime: 1 / 60,
};
globalThis.thisLayer = __hostThisLayer;

function __hostPropertyValue(name, fallback) {
  let value = fallback;
  if (__hostUserProperties && Object.prototype.hasOwnProperty.call(__hostUserProperties, name)) {
    value = __hostUserProperties[name];
  }
  if (value && typeof value === "object" && Object.prototype.hasOwnProperty.call(value, "value")) {
    value = value.value;
  }
  return value;
}

function __hostBool(value) {
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    return !(normalized === "" || normalized === "0" || normalized === "false" || normalized === "off");
  }
  return !!value;
}

function __hostNumber(value, fallback) {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : fallback;
}

function __hostText(value, fallback) {
  if (value === null || typeof value === "undefined") {
    return fallback;
  }
  return String(value);
}

function __hostSetScriptProperty(target, definition, fallback, coerce) {
  if (!definition || !definition.name) {
    return;
  }
  const authoredFallback = Object.prototype.hasOwnProperty.call(definition, "value")
    ? definition.value
    : fallback;
  target[definition.name] = coerce(__hostPropertyValue(definition.name, authoredFallback), authoredFallback);
}

globalThis.createScriptProperties = function createScriptProperties() {
  const values = {};
  const builder = {
    addSlider(definition) {
      __hostSetScriptProperty(values, definition, 0, __hostNumber);
      return builder;
    },
    addCheckbox(definition) {
      __hostSetScriptProperty(values, definition, false, (value) => __hostBool(value));
      return builder;
    },
    addText(definition) {
      __hostSetScriptProperty(values, definition, "", __hostText);
      return builder;
    },
    addCombo(definition) {
      __hostSetScriptProperty(values, definition, 0, (value) => value);
      return builder;
    },
    addColor(definition) {
      __hostSetScriptProperty(values, definition, "1 1 1", (value) => value);
      return builder;
    },
    addFile(definition) {
      __hostSetScriptProperty(values, definition, "", __hostText);
      return builder;
    },
    addDirectory(definition) {
      __hostSetScriptProperty(values, definition, "", __hostText);
      return builder;
    },
    finish() {
      return values;
    },
  };
  return builder;
};
"#;

const TEXT_SCRIPT_RUNNER: &str = r#"
(() => {
  try {
    const __hostStateKeyPattern = /^[A-Za-z_$][0-9A-Za-z_$]*$/;
    for (const key of __hostTrackedStateKeys) {
      if (Object.prototype.hasOwnProperty.call(__hostState, key)) {
        globalThis[key] = __hostState[key];
        if (__hostStateKeyPattern.test(key)) {
          try {
            eval(key + " = __hostState[" + JSON.stringify(key) + "]");
          } catch (_) {}
        }
      }
    }

    const __hostModule = globalThis.module || {};
    const __hostModuleExports = __hostModule.exports || {};
    const __hostEntry = (name) => {
      if (typeof globalThis[name] === "function") {
        return { fn: globalThis[name], target: globalThis };
      }
      if (__hostDefaultModule && typeof __hostDefaultModule === "function" && name === "update") {
        return { fn: __hostDefaultModule, target: globalThis };
      }
      if (__hostDefaultModule && typeof __hostDefaultModule[name] === "function") {
        return { fn: __hostDefaultModule[name], target: __hostDefaultModule };
      }
      if (__hostModule && typeof __hostModule[name] === "function") {
        return { fn: __hostModule[name], target: __hostModule };
      }
      if (__hostModuleExports && typeof __hostModuleExports[name] === "function") {
        return { fn: __hostModuleExports[name], target: __hostModuleExports };
      }
      return null;
    };

    if (__hostShouldInit) {
      const initEntry = __hostEntry("init");
      if (initEntry) {
        try {
          initEntry.fn.call(initEntry.target, thisLayer);
        } catch (error) {
          return JSON.stringify({
            result: null,
            text: thisLayer && typeof thisLayer.text === "string" ? thisLayer.text : null,
            state: {},
            error: String((error && error.message) || error),
            errorStage: "init",
            updateEntryMissing: false,
          });
        }
      }
    }

    if (__hostShouldApplyUserProperties) {
      const applyEntry = __hostEntry("applyUserProperties");
      if (applyEntry) {
        try {
          applyEntry.fn.call(applyEntry.target, __hostUserProperties);
        } catch (error) {
          return JSON.stringify({
            result: null,
            text: thisLayer && typeof thisLayer.text === "string" ? thisLayer.text : null,
            state: {},
            error: String((error && error.message) || error),
            errorStage: "applyUserProperties",
            updateEntryMissing: false,
          });
        }
      }
    }

    let __hostResult = null;
    let __hostUpdateMissing = false;
    const updateEntry = __hostEntry("update");
    if (updateEntry) {
      try {
        __hostResult = updateEntry.fn.call(updateEntry.target, thisLayer.text);
      } catch (error) {
        return JSON.stringify({
          result: null,
          text: thisLayer && typeof thisLayer.text === "string" ? thisLayer.text : null,
          state: {},
          error: String((error && error.message) || error),
          errorStage: "update",
          updateEntryMissing: false,
        });
      }
    } else {
      __hostUpdateMissing = true;
    }

    const __hostStateExport = {};
    for (const key of __hostTrackedStateKeys) {
      let value = globalThis[key];
      if (__hostStateKeyPattern.test(key)) {
        try {
          value = eval(key);
        } catch (_) {}
      }
      if (typeof value === "undefined" || typeof value === "function") {
        continue;
      }
      __hostStateExport[key] = value;
    }

    return JSON.stringify({
      result: __hostResult,
      text: thisLayer && typeof thisLayer.text === "string" ? thisLayer.text : null,
      state: __hostStateExport,
      error: null,
      errorStage: null,
      updateEntryMissing: __hostUpdateMissing,
    });
  } catch (error) {
    return JSON.stringify({
      result: null,
      text: null,
      state: {},
      error: String((error && error.message) || error),
      errorStage: "runtime",
      updateEntryMissing: false,
    });
  }
})()
"#;

#[cfg(test)]
pub(crate) fn clear_scene_text_script_runtime_cache() {
    if let Ok(mut cache) = scene_text_script_runtime_cache().lock() {
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{Local, TimeZone};
    use serde_json::json;

    use crate::models::{SceneTextBehavior, SceneTextLayer};

    use super::{
        clear_scene_text_script_runtime_cache, evaluate_scripted_text_layer,
        evaluate_scripted_text_layer_detailed, normalize_script_source,
        scene_text_script_runtime_diagnostics, text_script_requires_runtime,
        tracked_script_state_keys,
    };

    fn sample_script_layer(script_text: &str) -> SceneTextLayer {
        SceneTextLayer {
            id: 7,
            name: "Greeting".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: None,
            vertical_align: None,
            content: "Placeholder".to_string(),
            behavior: SceneTextBehavior::Script,
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
            size: None,
            render_bounds: None,
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: Some(1.0),
            alpha_binding: None,
            point_size: Some(24.0),
            point_size_binding: None,
            font_reference: None,
            font_path: None,
            effect_paths: vec![],
            script_text: Some(script_text.to_string()),
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: None,
            max_width: None,
            limit_width: None,
            limit_use_ellipsis: None,
            block_align: None,
        }
    }

    #[test]
    fn normalizes_exported_text_scripts_to_plain_script_source() {
        let normalized = normalize_script_source(
            "import * as WEMath from 'WEMath';\nexport var value = 1;\nexport function update() { return value; }\nexport default { update };\n",
        );

        assert!(!normalized.contains("import * as WEMath"));
        assert!(normalized.contains("var value = 1;"));
        assert!(normalized.contains("function update() { return value; }"));
        assert!(normalized.contains("__hostDefaultModule = { update };"));
    }

    #[test]
    fn tracks_var_declared_script_state_keys() {
        let tracked = tracked_script_state_keys(
            "var lastString;\nvar lastTimeTag;\nvar stringIndex = 0;\nfunction update() {\n  var stringList;\n}\n",
        );

        assert!(tracked.contains(&"lastString".to_string()));
        assert!(tracked.contains(&"lastTimeTag".to_string()));
        assert!(tracked.contains(&"stringIndex".to_string()));
        assert!(tracked.contains(&"stringList".to_string()));
    }

    #[test]
    fn scripted_text_layer_preserves_runtime_state_between_updates() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nvar counter = 0;\nexport function update() {\n  counter += 1;\n  thisLayer.text = String(counter);\n}\n",
        );
        let properties = BTreeMap::new();
        let now = Local.with_ymd_and_hms(2026, 4, 18, 17, 0, 0).unwrap();

        let first = evaluate_scripted_text_layer(Some("demo-preserve-runtime-state"), &layer, &properties, &now)
            .expect("first evaluation")
            .expect("first text");
        let second = evaluate_scripted_text_layer(Some("demo-preserve-runtime-state"), &layer, &properties, &now)
            .expect("second evaluation")
            .expect("second text");

        assert_eq!(first, "1");
        assert_eq!(second, "2");
    }

    #[test]
    fn scripted_text_layer_reapplies_user_properties_when_values_change() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nlet greetings = ['Good evening, $!'];\nvar lastString;\nexport function update() {\n  let newString = greetings[0];\n  if (newString != lastString) {\n    lastString = newString;\n    thisLayer.text = newString.replace('$', engine.userProperties.name);\n  }\n}\nexport function applyUserProperties() {\n  lastString = '';\n}\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();

        let alice = evaluate_scripted_text_layer(
            Some("demo-reapply-user-properties"),
            &layer,
            &BTreeMap::from([(String::from("name"), json!("Alice"))]),
            &now,
        )
        .expect("alice evaluation")
        .expect("alice text");
        let bob = evaluate_scripted_text_layer(
            Some("demo-reapply-user-properties"),
            &layer,
            &BTreeMap::from([(String::from("name"), json!("Bob"))]),
            &now,
        )
        .expect("bob evaluation")
        .expect("bob text");

        assert_eq!(alice, "Good evening, Alice!");
        assert_eq!(bob, "Good evening, Bob!");
    }

    #[test]
    fn scripted_text_layer_preserves_this_layer_text_when_script_skips_reassignment() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nlet greetings = ['Good evening, $!'];\nvar lastString;\nexport function update() {\n  let newString = greetings[0];\n  if (newString != lastString) {\n    lastString = newString;\n    thisLayer.text = newString.replace('$', engine.userProperties.name);\n  }\n}\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let properties = BTreeMap::from([(String::from("name"), json!("Alice"))]);

        let first = evaluate_scripted_text_layer(Some("demo-preserve-authored-text"), &layer, &properties, &now)
            .expect("first evaluation")
            .expect("first text");
        let second = evaluate_scripted_text_layer(Some("demo-preserve-authored-text"), &layer, &properties, &now)
            .expect("second evaluation")
            .expect("second text");

        assert_eq!(first, "Good evening, Alice!");
        assert_eq!(second, "Good evening, Alice!");
    }

    #[test]
    fn scripted_text_layer_supports_script_properties_defaults_and_overrides() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nexport var scriptProperties = createScriptProperties()\n  .addCheckbox({ name: 'use24hFormat', value: true })\n  .addText({ name: 'delimiter', value: '·' })\n  .finish();\nexport function update(value) {\n  return String(scriptProperties.use24hFormat) + ' ' + scriptProperties.delimiter;\n}\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();

        let defaults = evaluate_scripted_text_layer(
            Some("script-properties-demo"),
            &layer,
            &BTreeMap::new(),
            &now,
        )
        .expect("default script properties evaluation")
        .expect("default script properties text");
        let overrides = evaluate_scripted_text_layer(
            Some("script-properties-demo"),
            &layer,
            &BTreeMap::from([
                (String::from("use24hFormat"), json!(false)),
                (String::from("delimiter"), json!(":")),
            ]),
            &now,
        )
        .expect("override script properties evaluation")
        .expect("override script properties text");

        assert_eq!(defaults, "true ·");
        assert_eq!(overrides, "false :");
    }

    #[test]
    fn scripted_text_layer_supports_init_and_module_update_entries() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nvar counter = 0;\nmodule.init = function () { counter = 10; };\nmodule.update = function () { counter += 1; thisLayer.text = String(counter); };\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let properties = BTreeMap::new();

        let first = evaluate_scripted_text_layer(Some("module-demo"), &layer, &properties, &now)
            .expect("first module evaluation")
            .expect("first text");
        let second = evaluate_scripted_text_layer(Some("module-demo"), &layer, &properties, &now)
            .expect("second module evaluation")
            .expect("second text");

        assert_eq!(first, "11");
        assert_eq!(second, "12");
    }

    #[test]
    fn script_gate_recognizes_module_exports_entries() {
        assert!(text_script_requires_runtime(Some(
            "module.exports.update = function () { thisLayer.text = 'ready'; };"
        )));
        assert!(text_script_requires_runtime(Some(
            "module.exports.applyUserProperties = function () {};"
        )));
        assert!(text_script_requires_runtime(Some(
            "module.exports.init = function () {};"
        )));
    }

    #[test]
    fn scripted_text_layer_supports_module_exports_update_entry() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nvar counter = 0;\nmodule.exports.init = function () { counter = 20; };\nmodule.exports.update = function () { counter += 2; thisLayer.text = String(counter); };\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let properties = BTreeMap::new();

        let rendered =
            evaluate_scripted_text_layer(Some("module-exports-demo"), &layer, &properties, &now)
                .expect("module.exports evaluation")
                .expect("module.exports text");

        assert_eq!(rendered, "22");
    }

    #[test]
    fn scripted_text_layer_supports_default_module_entries() {
        clear_scene_text_script_runtime_cache();
        let layer = sample_script_layer(
            "'use strict';\nvar counter = 0;\nexport default {\n  init() { counter = 2; },\n  update() { counter += 3; thisLayer.text = String(counter); }\n};\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let properties = BTreeMap::new();

        let rendered =
            evaluate_scripted_text_layer(Some("default-demo"), &layer, &properties, &now)
                .expect("default module evaluation")
                .expect("default module text");

        assert_eq!(rendered, "5");
    }

    #[test]
    fn scripted_text_layer_clears_state_when_source_changes() {
        clear_scene_text_script_runtime_cache();
        let first_layer = sample_script_layer(
            "'use strict';\nvar counter = 0;\nexport function update() { counter += 1; thisLayer.text = String(counter); }\n",
        );
        let second_layer = sample_script_layer(
            "'use strict';\nvar counter = 100;\nexport function update() { counter += 1; thisLayer.text = String(counter); }\n",
        );
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let properties = BTreeMap::new();

        let first =
            evaluate_scripted_text_layer(Some("source-change"), &first_layer, &properties, &now)
                .expect("first source evaluation")
                .expect("first source text");
        let second =
            evaluate_scripted_text_layer(Some("source-change"), &second_layer, &properties, &now)
                .expect("second source evaluation")
                .expect("second source text");

        assert_eq!(first, "1");
        assert_eq!(second, "101");
    }

    #[test]
    fn scripted_text_layer_records_entry_and_compile_diagnostics() {
        clear_scene_text_script_runtime_cache();
        let now = Local.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let properties = BTreeMap::new();

        let missing_update = sample_script_layer("'use strict';\nvar counter = 0;\n");
        let missing = evaluate_scripted_text_layer_detailed(
            Some("diagnostic-demo"),
            &missing_update,
            &properties,
            &now,
        )
        .expect("missing update evaluation");
        assert!(missing
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "text-script-entry-missing"));

        let broken = sample_script_layer("export function update( {");
        let broken = evaluate_scripted_text_layer_detailed(
            Some("diagnostic-demo"),
            &broken,
            &properties,
            &now,
        )
        .expect("compile failure still returns fallback text");
        assert!(broken
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "text-script-compile-failed"));

        let cached = scene_text_script_runtime_diagnostics(Some("diagnostic-demo"));
        assert!(cached
            .iter()
            .any(|diagnostic| diagnostic.code == "text-script-compile-failed"));
    }
}
