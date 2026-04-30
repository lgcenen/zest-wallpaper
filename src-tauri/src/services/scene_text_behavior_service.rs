use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Local, Timelike};

use crate::models::{
    SceneNowPlayingSnapshot, SceneNowPlayingState, SceneTextBehavior, SceneTextLayer,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTextBehaviorEvaluation {
    pub value: String,
    pub dynamic_input_generation: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneTextRefreshCadence {
    CustomMillis(u64),
    Minute,
    TwoSeconds,
    Second,
}

impl SceneTextRefreshCadence {
    pub fn interval_millis(self) -> u64 {
        match self {
            SceneTextRefreshCadence::CustomMillis(interval_millis) => interval_millis.max(1),
            SceneTextRefreshCadence::Minute => 60_000,
            SceneTextRefreshCadence::TwoSeconds => 2_000,
            SceneTextRefreshCadence::Second => 1_000,
        }
    }
}

pub fn evaluate_text_behavior(
    layer: &SceneTextLayer,
    now: &DateTime<Local>,
    now_playing: Option<&SceneNowPlayingSnapshot>,
) -> SceneTextBehaviorEvaluation {
    let value = match layer.behavior {
        SceneTextBehavior::Clock => format_clock_for_layer(layer, now),
        SceneTextBehavior::Date => format_calendar_text(layer, now, false),
        SceneTextBehavior::Weekday => format_calendar_text(layer, now, true),
        SceneTextBehavior::DayPeriod => format_day_period(layer, now),
        SceneTextBehavior::MediaTitle => now_playing_title(layer, now_playing),
        SceneTextBehavior::Fps => "60 FPS".to_string(),
        SceneTextBehavior::Script | SceneTextBehavior::Static => fallback_text(layer),
    };

    SceneTextBehaviorEvaluation {
        value,
        dynamic_input_generation: text_dynamic_input_generation(layer, now_playing),
    }
}

pub fn fallback_text(layer: &SceneTextLayer) -> String {
    if layer.content.trim().is_empty() {
        layer.name.clone()
    } else {
        layer.content.clone()
    }
}

pub fn text_dynamic_input_generation(
    layer: &SceneTextLayer,
    now_playing: Option<&SceneNowPlayingSnapshot>,
) -> Option<u64> {
    if layer.behavior == SceneTextBehavior::MediaTitle {
        now_playing.map(|snapshot| snapshot.generation)
    } else {
        None
    }
}

pub fn text_layer_update_cadence(layer: &SceneTextLayer) -> Option<SceneTextRefreshCadence> {
    if let Some(interval_millis) = layer
        .script_refresh_interval_millis
        .filter(|interval_millis| *interval_millis > 0)
    {
        return Some(match interval_millis {
            1..=999 => SceneTextRefreshCadence::CustomMillis(interval_millis),
            1_000 => SceneTextRefreshCadence::Second,
            2_000 => SceneTextRefreshCadence::TwoSeconds,
            60_000 => SceneTextRefreshCadence::Minute,
            _ => SceneTextRefreshCadence::CustomMillis(interval_millis),
        });
    }

    if let Some(cadence) = scripted_text_update_cadence(layer.script_text.as_deref()) {
        return Some(cadence);
    }

    match layer.behavior {
        SceneTextBehavior::Clock if layer.show_seconds == Some(true) => {
            Some(SceneTextRefreshCadence::Second)
        }
        SceneTextBehavior::Script => None,
        SceneTextBehavior::Clock
        | SceneTextBehavior::Date
        | SceneTextBehavior::Weekday
        | SceneTextBehavior::DayPeriod => Some(SceneTextRefreshCadence::Minute),
        SceneTextBehavior::MediaTitle | SceneTextBehavior::Fps | SceneTextBehavior::Static => None,
    }
}

fn scripted_text_update_cadence(script_text: Option<&str>) -> Option<SceneTextRefreshCadence> {
    let lower_script = script_text?.to_ascii_lowercase();

    if lower_script.contains("getseconds") || lower_script.contains("date.now") {
        Some(SceneTextRefreshCadence::Second)
    } else if lower_script.contains("new date")
        || lower_script.contains("getminutes")
        || lower_script.contains("gethours")
        || lower_script.contains("getday")
        || lower_script.contains("getmonth")
        || lower_script.contains("getfullyear")
    {
        Some(SceneTextRefreshCadence::Minute)
    } else {
        None
    }
}

fn now_playing_title(
    layer: &SceneTextLayer,
    now_playing: Option<&SceneNowPlayingSnapshot>,
) -> String {
    now_playing
        .filter(|snapshot| snapshot.state == SceneNowPlayingState::Ready)
        .and_then(|snapshot| snapshot.title.as_ref())
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback_text(layer))
}

fn format_clock_for_layer(layer: &SceneTextLayer, now: &DateTime<Local>) -> String {
    let mut hours = now.hour() as i32;
    if layer.use_24h_format == Some(false) {
        hours %= 12;
        if hours == 0 {
            hours = 12;
        }
    }
    let delimiter = layer
        .delimiter
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(":");
    let hour_text = format!("{hours:02}");
    let minute_text = format!("{:02}", now.minute());
    let second_text = format!("{:02}", now.second());
    if layer.show_seconds == Some(true) {
        format!("{hour_text}{delimiter}{minute_text}{delimiter}{second_text}")
    } else {
        format!("{hour_text}{delimiter}{minute_text}")
    }
}

fn format_day_period(layer: &SceneTextLayer, now: &DateTime<Local>) -> String {
    authored_day_period_label(layer, now.hour())
        .unwrap_or_else(|| default_day_period_label(&layer.content, now.hour()).to_string())
}

fn authored_day_period_label(layer: &SceneTextLayer, hour: u32) -> Option<String> {
    let script = layer.script_text.as_deref()?.trim();
    if script.is_empty() {
        return None;
    }

    let rules = parse_day_period_hour_rules(script)?;
    let assignments = parse_day_period_switch_assignments(script);
    let label_sets = parse_day_period_label_sets(script);
    let selector = select_day_period_selector(&rules, hour)?;
    let assignment = assignments.get(selector.as_str())?;
    resolve_day_period_assignment(assignment, &label_sets)
}

fn default_day_period_label<'a>(content: &str, hour: u32) -> &'a str
where
    'static: 'a,
{
    let zh = if hour < 2 {
        "凌晨"
    } else if hour < 6 {
        "夜间"
    } else if hour < 8 {
        "早晨"
    } else if hour < 11 {
        "上午"
    } else if hour < 13 {
        "中午"
    } else if hour < 17 {
        "下午"
    } else if hour < 20 {
        "傍晚"
    } else {
        "晚上"
    };
    let en = if hour < 2 {
        "Before dawn"
    } else if hour < 6 {
        "At night"
    } else if hour < 11 {
        "Morning"
    } else if hour < 13 {
        "Noon"
    } else if hour < 17 {
        "Afternoon"
    } else if hour < 20 {
        "Evening"
    } else {
        "Night"
    };
    let has_ascii = content.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_chinese = content
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    if has_ascii && !has_chinese {
        en
    } else if has_ascii && has_chinese {
        if hour < 2 {
            "凌晨 / Before dawn"
        } else if hour < 6 {
            "夜间 / At night"
        } else if hour < 11 {
            "上午 / Morning"
        } else if hour < 13 {
            "中午 / Noon"
        } else if hour < 17 {
            "下午 / Afternoon"
        } else if hour < 20 {
            "傍晚 / Evening"
        } else {
            "晚上 / Night"
        }
    } else {
        zh
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DayPeriodRule {
    upper_hour_exclusive: Option<u32>,
    selector: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DayPeriodAssignment {
    Direct(String),
    Indexed { set_name: String, index: i64 },
}

fn parse_day_period_hour_rules(script: &str) -> Option<Vec<DayPeriodRule>> {
    let time_tag_pos = script.find("timeTag")?;
    let assignment = &script[time_tag_pos..];
    let equals = assignment.find('=')?;
    let expression = assignment[equals + 1..].split(';').next()?.trim();
    let mut remaining = expression;
    let mut rules = Vec::new();

    loop {
        let trimmed = remaining.trim();
        if trimmed.is_empty() {
            break;
        }
        let Some(question) = trimmed.find('?') else {
            let (selector, _) = parse_quoted_literal(trimmed)?;
            rules.push(DayPeriodRule {
                upper_hour_exclusive: None,
                selector,
            });
            break;
        };

        let upper_hour_exclusive = parse_hour_upper_bound(&trimmed[..question])?;
        let after_question = &trimmed[question + 1..];
        let (selector, remainder) = parse_quoted_literal(after_question)?;
        rules.push(DayPeriodRule {
            upper_hour_exclusive: Some(upper_hour_exclusive),
            selector,
        });
        let colon = remainder.find(':')?;
        remaining = &remainder[colon + 1..];
    }

    (!rules.is_empty()).then_some(rules)
}

fn parse_hour_upper_bound(condition: &str) -> Option<u32> {
    let compact: String = condition.chars().filter(|ch| !ch.is_whitespace()).collect();
    if let Some(index) = compact.find("<=") {
        return compact[index + 2..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .ok()
            .map(|value| value.saturating_add(1));
    }

    let index = compact.find('<')?;
    compact[index + 1..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse::<u32>()
        .ok()
}

fn select_day_period_selector(rules: &[DayPeriodRule], hour: u32) -> Option<String> {
    for rule in rules {
        match rule.upper_hour_exclusive {
            Some(upper) if hour < upper => return Some(rule.selector.clone()),
            None => return Some(rule.selector.clone()),
            _ => {}
        }
    }
    rules.last().map(|rule| rule.selector.clone())
}

fn parse_day_period_switch_assignments(script: &str) -> BTreeMap<String, DayPeriodAssignment> {
    let mut assignments = BTreeMap::new();
    let mut remaining = script;

    while let Some(case_pos) = remaining.find("case") {
        remaining = &remaining[case_pos + 4..];
        let Some((selector, after_selector)) = parse_quoted_literal(remaining) else {
            continue;
        };
        let Some(body_start) = after_selector.find(':') else {
            continue;
        };
        let body = &after_selector[body_start + 1..];
        let body_end = body
            .find("break")
            .or_else(|| body.find("case"))
            .unwrap_or(body.len());
        if let Some(assignment) = parse_day_period_assignment(&body[..body_end]) {
            assignments.insert(selector, assignment);
        }
        remaining = &body[body_end..];
    }

    assignments
}

fn parse_day_period_assignment(body: &str) -> Option<DayPeriodAssignment> {
    let equals = body.find('=')?;
    let expression = body[equals + 1..].split(';').next()?.trim();
    if let Some((direct, _)) = parse_quoted_literal(expression) {
        return Some(DayPeriodAssignment::Direct(direct));
    }

    let bracket = expression.find('[')?;
    let close = expression[bracket + 1..].find(']')? + bracket + 1;
    let set_name = expression[..bracket]
        .trim()
        .trim_end_matches('.')
        .to_string();
    let index = expression[bracket + 1..close].trim().parse::<i64>().ok()?;
    Some(DayPeriodAssignment::Indexed { set_name, index })
}

fn parse_day_period_label_sets(script: &str) -> BTreeMap<String, BTreeMap<i64, String>> {
    let mut label_sets = BTreeMap::new();
    let mut remaining = script;

    while let Some((keyword_index, keyword_len)) = ["let ", "const ", "var "]
        .into_iter()
        .filter_map(|keyword| remaining.find(keyword).map(|index| (index, keyword.len())))
        .min_by_key(|(index, _)| *index)
    {
        remaining = &remaining[keyword_index + keyword_len..];
        let identifier_end = remaining
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(remaining.len());
        let identifier = remaining[..identifier_end].trim();
        let after_identifier = remaining[identifier_end..].trim_start();
        if !after_identifier.starts_with('=') {
            continue;
        }
        let after_equals = after_identifier[1..].trim_start();
        if !after_equals.starts_with('{') {
            continue;
        }
        let Some(close_index) = after_equals.find('}') else {
            continue;
        };
        let object_body = &after_equals[1..close_index];
        let entries = parse_string_object_entries(object_body);
        if !entries.is_empty() {
            label_sets.insert(identifier.to_string(), entries);
        }
        remaining = &after_equals[close_index + 1..];
    }

    label_sets
}

fn parse_string_object_entries(body: &str) -> BTreeMap<i64, String> {
    body.split(',')
        .filter_map(|entry| {
            let mut parts = entry.splitn(2, ':');
            let key = parts.next()?.trim().trim_matches('\'').trim_matches('"');
            let value = parts.next()?.trim();
            let index = key.parse::<i64>().ok()?;
            let (label, _) = parse_quoted_literal(value)?;
            Some((index, label))
        })
        .collect()
}

fn resolve_day_period_assignment(
    assignment: &DayPeriodAssignment,
    label_sets: &BTreeMap<String, BTreeMap<i64, String>>,
) -> Option<String> {
    match assignment {
        DayPeriodAssignment::Direct(value) => Some(value.clone()),
        DayPeriodAssignment::Indexed { set_name, index } => label_sets
            .get(set_name)
            .and_then(|set| set.get(index))
            .cloned(),
    }
}

fn parse_quoted_literal(input: &str) -> Option<(String, &str)> {
    let start = input.find(['\'', '"'])?;
    let quote = input[start..].chars().next()?;
    let mut escaped = false;
    let mut value = String::new();

    for (offset, ch) in input[start + quote.len_utf8()..].char_indices() {
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            let end = start + quote.len_utf8() + offset + ch.len_utf8();
            return Some((value, &input[end..]));
        }
        value.push(ch);
    }

    None
}

fn format_calendar_text(
    layer: &SceneTextLayer,
    now: &DateTime<Local>,
    weekday_only: bool,
) -> String {
    let short = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
    let full = [
        "SUNDAY",
        "MONDAY",
        "TUESDAY",
        "WEDNESDAY",
        "THURSDAY",
        "FRIDAY",
        "SATURDAY",
    ];
    let months_numeric = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
    ];
    let months_abbr = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let months_full = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let align_vertical = layer.align_vertical.unwrap_or(weekday_only);
    let use_delimiter = layer.use_delimiter.unwrap_or(!align_vertical);
    let show_day = layer.show_day.unwrap_or(weekday_only);
    let month_format = layer.month_format.as_deref().unwrap_or("2");
    let day_format = layer.day_format.as_deref().unwrap_or("1");
    let wants_vertical_tokens = align_vertical
        && (layer.content.contains('\n')
            || !use_delimiter
            || script_wants_vertical_calendar_tokens(layer.script_text.as_deref()));
    let wants_spaced_weekday = script_wants_spaced_weekday(layer.script_text.as_deref());
    let weekday_base = if day_format == "2" {
        full[now.weekday().num_days_from_sunday() as usize]
    } else {
        short[now.weekday().num_days_from_sunday() as usize]
    };
    let weekday = if align_vertical {
        join_glyphs(&weekday_base.replace(char::is_whitespace, ""), "\n")
    } else if wants_spaced_weekday {
        join_glyphs(&weekday_base.replace(char::is_whitespace, ""), " ")
    } else {
        weekday_base.to_string()
    };

    if weekday_only && show_day {
        return weekday;
    }

    let day_value = now.day();
    let day_text = if wants_vertical_tokens {
        join_glyphs(&format!("{day_value:02}"), "\n")
    } else {
        day_value.to_string()
    };
    let month_source = if month_format == "3" {
        months_full[now.month0() as usize]
    } else if month_format == "2" {
        months_abbr[now.month0() as usize]
    } else {
        months_numeric[now.month0() as usize]
    };
    let month_text = if wants_vertical_tokens && month_format != "3" {
        join_glyphs(&month_source.replace(char::is_whitespace, ""), "\n")
    } else {
        month_source.to_string()
    };
    let year_text = if wants_vertical_tokens {
        join_glyphs(&now.year().to_string(), "\n")
    } else {
        now.year().to_string()
    };
    let delimiter = if use_delimiter {
        layer
            .delimiter
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("/")
            .to_string()
    } else if wants_vertical_tokens {
        "\n\n".to_string()
    } else {
        " ".to_string()
    };
    let date_text = format!("{day_text}{delimiter}{month_text}{delimiter}{year_text}");
    if show_day {
        if align_vertical {
            format!("{weekday}\n\n{date_text}")
        } else {
            format!("{weekday} {date_text}")
        }
    } else {
        date_text
    }
}

fn script_wants_spaced_weekday(script_text: Option<&str>) -> bool {
    let script = script_text.unwrap_or_default().to_ascii_lowercase();
    script.contains("'s u n'")
        || script.contains("'m o n'")
        || script.contains("'s u n d a y'")
        || script.contains("'m o n d a y'")
}

fn script_wants_vertical_calendar_tokens(script_text: Option<&str>) -> bool {
    let script = script_text.unwrap_or_default().to_ascii_lowercase();
    script.contains("+ newline +")
        || script.contains("+ nl +")
        || (script.contains("delimitervalue = [") && script.contains("\\n\\n"))
}

fn join_glyphs(text: &str, separator: &str) -> String {
    text.chars()
        .map(|ch| ch.to_string())
        .collect::<Vec<_>>()
        .join(separator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SceneAxisBindings, SceneNowPlayingAvailability};
    use chrono::TimeZone;

    fn text_layer(behavior: SceneTextBehavior) -> SceneTextLayer {
        SceneTextLayer {
            id: 7,
            name: "Layer".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: None,
            anchor: None,
            horizontal_align: None,
            vertical_align: None,
            content: "Fallback".to_string(),
            behavior,
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
            position_bindings: Option::<SceneAxisBindings>::None,
            scale: [1.0, 1.0, 1.0],
            scale_binding: None,
            angles: None,
            rotation: None,
            size: None,
            render_bounds: None,
            parallax_depth: None,
            color: None,
            color_binding: None,
            alpha: None,
            alpha_binding: None,
            point_size: None,
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

    #[test]
    fn evaluates_core_text_behaviors_from_one_module_boundary() {
        let now = Local
            .with_ymd_and_hms(2026, 4, 29, 17, 5, 6)
            .single()
            .expect("local time");
        let mut clock = text_layer(SceneTextBehavior::Clock);
        clock.show_seconds = Some(true);
        clock.use_24h_format = Some(true);
        assert_eq!(evaluate_text_behavior(&clock, &now, None).value, "17:05:06");

        let mut date = text_layer(SceneTextBehavior::Date);
        date.month_format = Some("2".to_string());
        date.use_delimiter = Some(false);
        assert_eq!(
            evaluate_text_behavior(&date, &now, None).value,
            "29 APR 2026"
        );

        let weekday = text_layer(SceneTextBehavior::Weekday);
        assert_eq!(
            evaluate_text_behavior(&weekday, &now, None).value,
            "W\nE\nD"
        );

        let mut day_period = text_layer(SceneTextBehavior::DayPeriod);
        day_period.content = "Before dawn".to_string();
        assert_eq!(
            evaluate_text_behavior(&day_period, &now, None).value,
            "Evening"
        );

        assert_eq!(
            evaluate_text_behavior(&text_layer(SceneTextBehavior::Fps), &now, None).value,
            "60 FPS"
        );
        assert_eq!(
            evaluate_text_behavior(&text_layer(SceneTextBehavior::Static), &now, None).value,
            "Fallback"
        );
    }

    #[test]
    fn media_title_uses_ready_now_playing_and_tracks_generation() {
        let now = Local
            .with_ymd_and_hms(2026, 4, 29, 17, 5, 6)
            .single()
            .expect("local time");
        let snapshot = SceneNowPlayingSnapshot {
            availability: SceneNowPlayingAvailability::Available,
            state: SceneNowPlayingState::Ready,
            title: Some("Track".to_string()),
            artist: None,
            album: None,
            source: None,
            generation: 42,
            updated_at: now.with_timezone(&chrono::Utc),
            refresh_interval_millis: 1500,
            diagnostics: Vec::new(),
        };

        let output = evaluate_text_behavior(
            &text_layer(SceneTextBehavior::MediaTitle),
            &now,
            Some(&snapshot),
        );
        assert_eq!(output.value, "Track");
        assert_eq!(output.dynamic_input_generation, Some(42));
    }

    #[test]
    fn text_refresh_cadence_covers_clock_script_and_calendar_behaviors() {
        let mut clock = text_layer(SceneTextBehavior::Clock);
        clock.show_seconds = Some(true);
        assert_eq!(
            text_layer_update_cadence(&clock),
            Some(SceneTextRefreshCadence::Second)
        );

        let mut scripted = text_layer(SceneTextBehavior::Script);
        scripted.script_text =
            Some("export function update() { return new Date().getHours(); }".to_string());
        assert_eq!(
            text_layer_update_cadence(&scripted),
            Some(SceneTextRefreshCadence::Minute)
        );

        assert_eq!(
            text_layer_update_cadence(&text_layer(SceneTextBehavior::Date)),
            Some(SceneTextRefreshCadence::Minute)
        );
        assert_eq!(
            text_layer_update_cadence(&text_layer(SceneTextBehavior::Static)),
            None
        );
    }

    #[test]
    fn explicit_script_refresh_interval_overrides_behavior_cadence() {
        let mut layer = text_layer(SceneTextBehavior::Clock);
        layer.show_seconds = Some(true);
        layer.script_refresh_interval_millis = Some(1500);

        let cadence = text_layer_update_cadence(&layer).expect("cadence");
        assert_eq!(cadence, SceneTextRefreshCadence::CustomMillis(1500));
        assert_eq!(cadence.interval_millis(), 1500);
    }
}
