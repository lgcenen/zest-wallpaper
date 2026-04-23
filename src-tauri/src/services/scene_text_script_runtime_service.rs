use std::{
    collections::BTreeMap,
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
    let script = layer
        .script_text
        .as_deref()
        .map(str::trim)
        .filter(|script| !script.is_empty())
        .ok_or_else(|| format!("Text layer {} has no script text", layer.name))?;

    let cache_key = runtime_owner_key.map(|owner| format!("{owner}:{}", layer.id));
    let script_hash = stable_hash(script);
    let property_hash = stable_hash_json(properties);

    let (previous_state_json, previous_rendered_text, should_apply_user_properties) =
        load_cached_script_state(cache_key.as_deref(), script_hash, property_hash)?;

    let execution = execute_scripted_text_layer(
        script,
        layer,
        properties,
        now,
        previous_rendered_text.as_deref(),
        previous_state_json.as_deref().unwrap_or("{}"),
        should_apply_user_properties,
    )?;

    let rendered_text = execution
        .result
        .as_ref()
        .and_then(string_from_json_value)
        .or(execution.text.clone());

    if let Some(cache_key) = cache_key.as_deref() {
        save_cached_script_state(
            cache_key,
            script_hash,
            property_hash,
            execution.state.clone(),
            rendered_text.clone(),
        )?;
    }

    Ok(rendered_text)
}

fn load_cached_script_state(
    cache_key: Option<&str>,
    script_hash: u64,
    property_hash: u64,
) -> Result<(Option<String>, Option<String>, bool), String> {
    let Some(cache_key) = cache_key else {
        return Ok((None, None, true));
    };

    let cache = scene_text_script_runtime_cache()
        .lock()
        .map_err(|error| error.to_string())?;
    let Some(entry) = cache.get(cache_key) else {
        return Ok((None, None, true));
    };
    if entry.script_hash != script_hash {
        return Ok((None, None, true));
    }

    Ok((
        Some(entry.state_json.clone()),
        entry.rendered_text.clone(),
        entry.property_hash != property_hash,
    ))
}

fn save_cached_script_state(
    cache_key: &str,
    script_hash: u64,
    property_hash: u64,
    state: Map<String, Value>,
    rendered_text: Option<String>,
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

    let output_json = context
        .eval_as::<String>(&source)
        .map_err(|error| error.to_string())?;
    let parsed = serde_json::from_str::<SceneTextScriptExecutionOutput>(&output_json)
        .map_err(|error| format!("Failed to decode text script output: {error}"))?;

    if let Some(error) = parsed.error.as_ref() {
        return Err(error.clone());
    }

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

        if trimmed.starts_with("export default ") {
            normalized.push_str(line.replacen("export default ", "", 1).as_str());
        } else if trimmed.starts_with("export ") {
            normalized.push_str(line.replacen("export ", "", 1).as_str());
        } else {
            normalized.push_str(line);
        }
        normalized.push('\n');
    }

    normalized
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

globalThis.console = globalThis.console || { log() {}, warn() {}, error() {} };

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
"#;

const TEXT_SCRIPT_RUNNER: &str = r#"
(() => {
  try {
    for (const key of __hostTrackedStateKeys) {
      if (Object.prototype.hasOwnProperty.call(__hostState, key)) {
        globalThis[key] = __hostState[key];
      }
    }

    if (__hostShouldApplyUserProperties && typeof applyUserProperties === "function") {
      applyUserProperties(__hostUserProperties);
    }

    let __hostResult = null;
    if (typeof update === "function") {
      __hostResult = update(thisLayer.text);
    }

    const __hostStateExport = {};
    for (const key of __hostTrackedStateKeys) {
      const value = globalThis[key];
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
    });
  } catch (error) {
    return JSON.stringify({
      result: null,
      text: null,
      state: {},
      error: String((error && error.message) || error),
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
        normalize_script_source, tracked_script_state_keys,
    };

    fn sample_script_layer(script_text: &str) -> SceneTextLayer {
        SceneTextLayer {
            id: 7,
            name: "Greeting".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
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
            "import * as WEMath from 'WEMath';\nexport var value = 1;\nexport function update() { return value; }\n",
        );

        assert!(!normalized.contains("import * as WEMath"));
        assert!(normalized.contains("var value = 1;"));
        assert!(normalized.contains("function update() { return value; }"));
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

        let first = evaluate_scripted_text_layer(Some("demo"), &layer, &properties, &now)
            .expect("first evaluation")
            .expect("first text");
        let second = evaluate_scripted_text_layer(Some("demo"), &layer, &properties, &now)
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
            Some("demo"),
            &layer,
            &BTreeMap::from([(String::from("name"), json!("Alice"))]),
            &now,
        )
        .expect("alice evaluation")
        .expect("alice text");
        let bob = evaluate_scripted_text_layer(
            Some("demo"),
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

        let first = evaluate_scripted_text_layer(Some("demo"), &layer, &properties, &now)
            .expect("first evaluation")
            .expect("first text");
        let second = evaluate_scripted_text_layer(Some("demo"), &layer, &properties, &now)
            .expect("second evaluation")
            .expect("second text");

        assert_eq!(first, "Good evening, Alice!");
        assert_eq!(second, "Good evening, Alice!");
    }
}
