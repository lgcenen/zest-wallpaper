use std::path::Path;

pub(super) const HTML_WALLPAPER_BRIDGE: &str = include_str!("../html_wallpaper_bridge.js");

fn is_html_asset(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("html") | Some("htm")
    )
}

fn escape_inline_script(value: &str) -> String {
    value
        .replace("</script", "<\\/script")
        .replace("</SCRIPT", "<\\/script")
}

pub(super) fn inject_html_wallpaper_bridge(html: &str) -> String {
    let bridge = format!(
        "<script>{}</script>",
        escape_inline_script(HTML_WALLPAPER_BRIDGE)
    );

    let lower = html.to_ascii_lowercase();
    if lower.contains("<head") {
        return inject_after_head_tag(html, &bridge);
    }

    if let Some(start) = lower.find("<html") {
        if let Some(close_offset) = lower[start..].find('>') {
            let insert_at = start + close_offset + 1;
            let mut injected = String::with_capacity(html.len() + bridge.len() + 13);
            injected.push_str(&html[..insert_at]);
            injected.push_str("<head>");
            injected.push_str(&bridge);
            injected.push_str("</head>");
            injected.push_str(&html[insert_at..]);
            return injected;
        }
    }

    format!("<!DOCTYPE html><html><head>{bridge}</head><body>{html}</body></html>")
}

fn inject_after_head_tag(html: &str, bridge: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(start) = lower.find("<head") {
        if let Some(close_offset) = lower[start..].find('>') {
            let insert_at = start + close_offset + 1;
            let mut injected = String::with_capacity(html.len() + bridge.len());
            injected.push_str(&html[..insert_at]);
            injected.push_str(bridge);
            injected.push_str(&html[insert_at..]);
            return injected;
        }
    }
    html.to_string()
}

pub(super) fn prepare_web_runtime_body(path: &Path, body: Vec<u8>) -> Vec<u8> {
    if !is_html_asset(path) {
        return body;
    }

    match String::from_utf8(body) {
        Ok(html) => inject_html_wallpaper_bridge(&html).into_bytes(),
        Err(error) => error.into_bytes(),
    }
}
