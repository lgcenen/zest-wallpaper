use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    models::{
        PropertyKind, PropertyPresentation, PropertySection, PropertySectionItem,
        PropertySectionItemKind, SceneManifest, WallpaperOption, WallpaperProperty,
        WallpaperRecord, WallpaperType,
    },
    pkg::extract_pkg,
    scene::parse_scene_manifest,
    services::{
        asset_resolver::AssetResolver, scene_cache_service, static_snapshot_generation_service,
        static_snapshot_service,
    },
    store::wallpaper_dir,
};

#[derive(Debug)]
struct ImportedProject {
    title: String,
    wallpaper_type: WallpaperType,
    managed_root: PathBuf,
    preview_path: Option<PathBuf>,
    entry_path: Option<PathBuf>,
    property_schema: Vec<WallpaperProperty>,
    property_sections: Vec<PropertySection>,
    scene_manifest: Option<SceneManifest>,
    tags: Vec<String>,
}

fn clean_markup(input: &str) -> String {
    let normalized = input
        .replace("<br/>", " / ")
        .replace("<br />", " / ")
        .replace("<br>", " / ")
        .replace("<BR/>", " / ")
        .replace("<BR />", " / ")
        .replace("<BR>", " / ");
    let mut output = String::with_capacity(normalized.len());
    let mut in_tag = false;
    for char in normalized.chars() {
        match char {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag && !is_invisible_markup_char(char) => output.push(char),
            _ => {}
        }
    }
    let compact = output
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("：", ":");
    let mut collapsed = String::with_capacity(compact.len());
    let mut last_was_space = false;
    for char in compact.chars() {
        let normalized = if char.is_whitespace() { ' ' } else { char };
        if normalized == ' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
            collapsed.push(' ');
        } else {
            last_was_space = false;
            collapsed.push(normalized);
        }
    }
    collapsed
        .trim_matches(|char: char| {
            char.is_whitespace() || matches!(char, '🔘' | '•' | '·' | ':' | '|' | '/')
        })
        .to_string()
}

fn is_invisible_markup_char(char: char) -> bool {
    matches!(
        char,
        '\u{00ad}'
            | '\u{200b}'
            | '\u{200c}'
            | '\u{200d}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2060}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
            | '\u{202a}'
            | '\u{202b}'
            | '\u{202c}'
            | '\u{202d}'
            | '\u{202e}'
            | '\u{feff}'
    )
}

fn suffix_number(key: &str, prefix: &str) -> Option<u32> {
    let suffix = key.strip_prefix(prefix)?;
    suffix.parse::<u32>().ok()
}

fn looks_like_decoration_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.starts_with("imgsrc")
        || lower.starts_with("brimgsrc")
        || lower.contains("imgsrchttp")
        || lower.contains("hrefhttps")
        || (key.len() > 96 && !key.contains('_'))
}

fn is_decoration_markup(raw_text: &str) -> bool {
    let lower = raw_text.to_ascii_lowercase();
    lower.contains("<img") || lower.contains("<center>") || lower.contains("rf=viewer")
}

fn property_presentation(
    key: &str,
    kind: &PropertyKind,
    raw_text: &str,
    cleaned_label: &str,
) -> PropertyPresentation {
    if raw_text.to_ascii_lowercase().contains("<img") || looks_like_decoration_key(key) {
        return PropertyPresentation::Decoration;
    }

    match kind {
        PropertyKind::Group => {
            if cleaned_label.is_empty() {
                PropertyPresentation::Decoration
            } else {
                PropertyPresentation::Group
            }
        }
        PropertyKind::Text => {
            if is_decoration_markup(raw_text) {
                return PropertyPresentation::Decoration;
            }
            if cleaned_label.is_empty() {
                PropertyPresentation::Decoration
            } else {
                PropertyPresentation::Group
            }
        }
        _ => {
            if cleaned_label.is_empty() && is_decoration_markup(raw_text) {
                PropertyPresentation::Decoration
            } else {
                PropertyPresentation::Control
            }
        }
    }
}

fn fallback_property_label(key: &str, cleaned: &str) -> String {
    if let Some(number) = suffix_number(key, "appdockapplyusertexture") {
        if cleaned.is_empty() || !cleaned.chars().any(|char| char.is_ascii_digit()) {
            return format!("自定义图标 #{number}");
        }
    }
    if let Some(number) = suffix_number(key, "appdockenable") {
        if cleaned.is_empty()
            || cleaned.contains("启用Enable")
            || !cleaned.chars().any(|char| char.is_ascii_digit())
        {
            return format!("启用图标 #{number}");
        }
    }
    if cleaned.is_empty() {
        key.to_string()
    } else {
        cleaned.to_string()
    }
}

fn needs_group_context(label: &str) -> bool {
    let compact = label
        .to_ascii_lowercase()
        .replace(' ', "")
        .replace("／", "/");
    matches!(
        compact.as_str(),
        "大小size"
            | "颜色color"
            | "位置xpositionx"
            | "位置ypositiony"
            | "文本位置xpositionx"
            | "文本位置ypositiony"
            | "opacity"
            | "musicsize"
    )
}

fn contextualize_property_labels(values: &mut [WallpaperProperty]) {
    let mut current_group: Option<String> = None;

    for property in values.iter_mut() {
        match property.presentation {
            PropertyPresentation::Decoration => {}
            PropertyPresentation::Group => {
                current_group = Some(property.label.clone());
            }
            PropertyPresentation::Control => {
                if needs_group_context(&property.label) {
                    if let Some(group) = current_group.as_ref() {
                        if !property.label.starts_with(group) {
                            property.label = format!("{group} / {}", property.label);
                        }
                    }
                }
            }
        }
    }
}

fn property_values_map(properties: &[WallpaperProperty]) -> BTreeMap<String, Value> {
    properties
        .iter()
        .map(|property| (property.key.clone(), property.value.clone()))
        .collect()
}

fn decoration_to_section_item(property: &WallpaperProperty) -> Option<PropertySectionItem> {
    let lower_key = property.key.to_ascii_lowercase();
    let markup = property.markup.clone();
    let markup_lower = markup
        .as_deref()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if lower_key.contains("hr")
        && !markup_lower.contains("<img")
        && property.label.trim().is_empty()
    {
        return Some(PropertySectionItem {
            kind: PropertySectionItemKind::Separator,
            key: None,
            text: None,
            markup,
            order: property.order,
            condition: property.condition.clone(),
        });
    }
    if markup_lower.contains("<hr")
        && !markup_lower.contains("<img")
        && property.label.trim().is_empty()
    {
        return Some(PropertySectionItem {
            kind: PropertySectionItemKind::Separator,
            key: None,
            text: None,
            markup,
            order: property.order,
            condition: property.condition.clone(),
        });
    }

    let text = property.label.trim();
    if text.is_empty() && markup.is_none() {
        return None;
    }

    Some(PropertySectionItem {
        kind: PropertySectionItemKind::Description,
        key: None,
        text: (!text.is_empty()).then(|| text.to_string()),
        markup,
        order: property.order,
        condition: property.condition.clone(),
    })
}

fn make_fallback_section() -> PropertySection {
    PropertySection {
        key: "general".to_string(),
        label: "General".to_string(),
        order: Some(0),
        condition: None,
        items: Vec::new(),
    }
}

fn build_property_sections(properties: &[WallpaperProperty]) -> Vec<PropertySection> {
    let mut sections = Vec::new();
    let mut current_section: Option<PropertySection> = None;

    for property in properties {
        match property.presentation {
            PropertyPresentation::Group => {
                if let Some(section) = current_section.take() {
                    sections.push(section);
                }
                current_section = Some(PropertySection {
                    key: property.key.clone(),
                    label: property.label.clone(),
                    order: property.order,
                    condition: property.condition.clone(),
                    items: Vec::new(),
                });
            }
            PropertyPresentation::Decoration => {
                let Some(item) = decoration_to_section_item(property) else {
                    continue;
                };
                if current_section.is_none() {
                    current_section = Some(make_fallback_section());
                }
                if let Some(section) = current_section.as_mut() {
                    section.items.push(item);
                }
            }
            PropertyPresentation::Control => {
                if current_section.is_none() {
                    current_section = Some(make_fallback_section());
                }
                if let Some(section) = current_section.as_mut() {
                    section.items.push(PropertySectionItem {
                        kind: PropertySectionItemKind::Property,
                        key: Some(property.key.clone()),
                        text: None,
                        markup: None,
                        order: property.order,
                        condition: property.condition.clone(),
                    });
                }
            }
        }
    }

    if let Some(section) = current_section.take() {
        sections.push(section);
    }

    sections.sort_by(|left, right| {
        left.order
            .unwrap_or(u32::MAX)
            .cmp(&right.order.unwrap_or(u32::MAX))
            .then_with(|| left.key.cmp(&right.key))
    });

    sections
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(anyhow!(
                "Refusing to import symlinked path {}",
                entry.path().display()
            ));
        }
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn import_source_copy(source_path: &Path, managed_root: &Path) -> Result<PathBuf> {
    if fs::symlink_metadata(source_path)?.file_type().is_symlink() {
        return Err(anyhow!(
            "Refusing to import symlinked source path {}",
            source_path.display()
        ));
    }
    let destination = managed_root.join("source");
    if source_path.is_dir() {
        copy_dir_all(source_path, &destination)?;
        Ok(destination)
    } else {
        fs::create_dir_all(&destination)?;
        let file_name = source_path
            .file_name()
            .ok_or_else(|| anyhow!("Input path is missing a file name"))?;
        let target = destination.join(file_name);
        fs::copy(source_path, &target)?;
        Ok(target)
    }
}

fn find_first_with_extensions(root: &Path, extensions: &[&str]) -> Option<PathBuf> {
    let expected: Vec<String> = extensions
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|entry| {
            if !entry.file_type().is_file() {
                return None;
            }
            let extension = entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())?;
            if expected.iter().any(|candidate| candidate == &extension) {
                Some(entry.path().to_path_buf())
            } else {
                None
            }
        })
}

fn parse_options(value: &Value) -> Vec<WallpaperOption> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if let Some(object) = item.as_object() {
                    let value = object
                        .get("value")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let label = object
                        .get("label")
                        .and_then(Value::as_str)
                        .filter(|label| !label.is_empty())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| value.clone());
                    WallpaperOption { label, value }
                } else {
                    let text = item.as_str().unwrap_or_default().to_string();
                    WallpaperOption {
                        label: text.clone(),
                        value: text,
                    }
                }
            })
            .collect(),
        Value::Object(map) => map
            .iter()
            .map(|(key, item)| WallpaperOption {
                label: item.as_str().unwrap_or(key).to_string(),
                value: key.clone(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn lookup_localized_text<'a>(project_json: &'a Value, key: &str) -> Option<&'a str> {
    let localizations = project_json
        .get("general")
        .and_then(|general| general.get("localization"))
        .and_then(Value::as_object)?;

    ["zh-chs", "zh-cn", "en-us", "en", "ja-jp"]
        .into_iter()
        .find_map(|locale| {
            localizations
                .get(locale)
                .and_then(Value::as_object)
                .and_then(|catalog| catalog.get(key))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            localizations.values().find_map(|catalog| {
                catalog
                    .as_object()
                    .and_then(|entries| entries.get(key))
                    .and_then(Value::as_str)
            })
        })
}

fn resolve_property_text<'a>(project_json: &'a Value, raw_text: &'a str) -> &'a str {
    lookup_localized_text(project_json, raw_text).unwrap_or(raw_text)
}

fn property_kind(value: Option<&str>) -> PropertyKind {
    match value.unwrap_or_default() {
        "bool" => PropertyKind::Bool,
        "slider" => PropertyKind::Slider,
        "color" => PropertyKind::Color,
        "combo" => PropertyKind::Combo,
        "textinput" => PropertyKind::Textinput,
        "file" => PropertyKind::Textinput,
        "label" => PropertyKind::Text,
        "text" => PropertyKind::Text,
        "group" => PropertyKind::Group,
        _ => PropertyKind::Unknown,
    }
}

fn property_schema(project_json: &Value) -> Vec<WallpaperProperty> {
    project_json
        .get("general")
        .and_then(|general| general.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| {
            let mut values = properties
                .iter()
                .filter_map(|(key, raw)| {
                    let object = raw.as_object()?;
                    let raw_kind = object.get("type").and_then(Value::as_str);
                    let kind = property_kind(raw_kind);
                    let raw_text = object.get("text").and_then(Value::as_str).unwrap_or(key);
                    let resolved_text = resolve_property_text(project_json, raw_text);
                    let cleaned_label = clean_markup(resolved_text);
                    let presentation = if matches!(raw_kind, Some("label")) {
                        PropertyPresentation::Decoration
                    } else {
                        property_presentation(key, &kind, resolved_text, &cleaned_label)
                    };
                    let label = if matches!(presentation, PropertyPresentation::Decoration)
                        && cleaned_label.is_empty()
                    {
                        String::new()
                    } else {
                        fallback_property_label(key, &cleaned_label)
                    };
                    Some(WallpaperProperty {
                        key: key.clone(),
                        label,
                        markup: Some(raw_text.to_string()),
                        kind,
                        value: object.get("value").cloned().unwrap_or(Value::Null),
                        default_value: object.get("value").cloned().unwrap_or(Value::Null),
                        min: object.get("min").and_then(Value::as_f64),
                        max: object.get("max").and_then(Value::as_f64),
                        step: object.get("step").and_then(Value::as_f64),
                        condition: object
                            .get("condition")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        order: object
                            .get("order")
                            .and_then(Value::as_u64)
                            .map(|value| value as u32)
                            .or_else(|| {
                                object
                                    .get("index")
                                    .and_then(Value::as_u64)
                                    .map(|value| value as u32)
                            }),
                        presentation,
                        options: object.get("options").map(parse_options).unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            values.sort_by(|left, right| {
                left.order
                    .unwrap_or(u32::MAX)
                    .cmp(&right.order.unwrap_or(u32::MAX))
                    .then_with(|| left.key.cmp(&right.key))
            });
            contextualize_property_labels(&mut values);
            values
        })
        .unwrap_or_default()
}

fn detect_from_project_json(
    root: &Path,
    _project_json_path: &Path,
    project_json: &Value,
) -> Result<ImportedProject> {
    let managed_root = root
        .parent()
        .ok_or_else(|| anyhow!("Managed source path is missing a parent"))?
        .to_path_buf();
    let resolver = AssetResolver::for_managed_root(&managed_root);
    let file_hint = project_json.get("file").and_then(Value::as_str);
    let wallpaper_type = match project_json.get("type").and_then(Value::as_str) {
        Some("scene") => WallpaperType::Scene,
        Some("video") => WallpaperType::Video,
        Some("web") => WallpaperType::Web,
        Some("application") => WallpaperType::Application,
        _ => match file_hint
            .and_then(|value| {
                Path::new(value)
                    .extension()
                    .and_then(|extension| extension.to_str())
            })
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref()
        {
            Some("html") | Some("htm") => WallpaperType::Web,
            Some("mp4") | Some("webm") | Some("mov") | Some("mkv") => WallpaperType::Video,
            Some("pkg") | Some("json") => WallpaperType::Scene,
            _ => WallpaperType::Unknown,
        },
    };

    let title = project_json
        .get("title")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "Untitled Wallpaper".to_string());

    let preview_hint = project_json.get("preview").and_then(Value::as_str);
    let properties = property_schema(project_json);
    let property_sections = build_property_sections(&properties);
    let property_values = property_values_map(&properties);
    let tags = project_json
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let preview_path = resolver
        .resolve_preview_path(preview_hint)
        .map(PathBuf::from);

    let mut entry_path = resolver
        .resolve_entry_path(
            file_hint,
            match wallpaper_type {
                WallpaperType::Scene => "scene",
                WallpaperType::Video => "video",
                WallpaperType::Web => "web",
                WallpaperType::Application => "application",
                WallpaperType::Unknown => "unknown",
            },
        )
        .map(PathBuf::from);

    let mut scene_manifest = None;

    if matches!(wallpaper_type, WallpaperType::Scene) {
        let pkg_path = resolver
            .scene_pkg_path()
            .unwrap_or_else(|| root.join("scene.pkg"));
        let extracted_root = resolver.extracted_root().to_path_buf();
        let _package = if pkg_path.exists() {
            Some(extract_pkg(&pkg_path, &extracted_root)?)
        } else {
            None
        };
        entry_path = resolver.scene_json_path();
        if let Some(scene_json) = entry_path.as_ref() {
            scene_manifest = Some(parse_scene_manifest(
                scene_json,
                &extracted_root,
                &property_values,
            )?);
        }
    } else if matches!(wallpaper_type, WallpaperType::Video) {
        if entry_path.is_none() {
            entry_path = find_first_with_extensions(root, &["mp4", "webm", "mov", "mkv"]);
        }
    } else if matches!(wallpaper_type, WallpaperType::Web) {
        if entry_path.is_none() {
            entry_path = find_first_with_extensions(root, &["html", "htm"]);
        }
    }

    Ok(ImportedProject {
        title,
        wallpaper_type,
        managed_root,
        preview_path,
        entry_path,
        property_schema: properties,
        property_sections,
        scene_manifest,
        tags,
    })
}

fn detect_without_project_json(root: &Path) -> Result<ImportedProject> {
    let title = root
        .file_stem()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .or_else(|| {
            root.file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "Untitled Wallpaper".to_string());

    if root.is_file() {
        let extension = root
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let wallpaper_type = match extension.as_str() {
            "mp4" | "webm" | "mov" | "mkv" => WallpaperType::Video,
            "html" | "htm" => WallpaperType::Web,
            _ => WallpaperType::Unknown,
        };

        return Ok(ImportedProject {
            title,
            wallpaper_type,
            managed_root: root
                .parent()
                .ok_or_else(|| anyhow!("Managed source path is missing a parent"))?
                .to_path_buf(),
            preview_path: None,
            entry_path: Some(root.to_path_buf()),
            property_schema: Vec::new(),
            property_sections: Vec::new(),
            scene_manifest: None,
            tags: Vec::new(),
        });
    }

    let html = find_first_with_extensions(root, &["html", "htm"]);
    if html.is_some() {
        return Ok(ImportedProject {
            title,
            wallpaper_type: WallpaperType::Web,
            managed_root: root
                .parent()
                .ok_or_else(|| anyhow!("Managed source path is missing a parent"))?
                .to_path_buf(),
            preview_path: find_first_with_extensions(root, &["gif", "png", "jpg", "jpeg"]),
            entry_path: html.clone(),
            property_schema: Vec::new(),
            property_sections: Vec::new(),
            scene_manifest: None,
            tags: Vec::new(),
        });
    }

    let video = find_first_with_extensions(root, &["mp4", "webm", "mov", "mkv"]);
    if video.is_some() {
        return Ok(ImportedProject {
            title,
            wallpaper_type: WallpaperType::Video,
            managed_root: root
                .parent()
                .ok_or_else(|| anyhow!("Managed source path is missing a parent"))?
                .to_path_buf(),
            preview_path: find_first_with_extensions(root, &["gif", "png", "jpg", "jpeg"]),
            entry_path: video.clone(),
            property_schema: Vec::new(),
            property_sections: Vec::new(),
            scene_manifest: None,
            tags: Vec::new(),
        });
    }

    Ok(ImportedProject {
        title,
        wallpaper_type: WallpaperType::Unknown,
        managed_root: root
            .parent()
            .ok_or_else(|| anyhow!("Managed source path is missing a parent"))?
            .to_path_buf(),
        preview_path: None,
        entry_path: None,
        property_schema: Vec::new(),
        property_sections: Vec::new(),
        scene_manifest: None,
        tags: Vec::new(),
    })
}

fn detect_project(root: &Path) -> Result<ImportedProject> {
    let explicit_project_json = root.is_file()
        && root
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("project.json"))
            .unwrap_or(false);
    let project_json_path = if root.is_dir() {
        root.join("project.json")
    } else if explicit_project_json {
        root.to_path_buf()
    } else {
        PathBuf::new()
    };

    if !project_json_path.as_os_str().is_empty() && project_json_path.exists() {
        let contents = fs::read_to_string(&project_json_path)
            .with_context(|| format!("Unable to read {}", project_json_path.display()))?;
        let project_json: Value = serde_json::from_str(&contents)
            .with_context(|| format!("Unable to parse {}", project_json_path.display()))?;
        let effective_root = if root.is_dir() {
            root.to_path_buf()
        } else {
            project_json_path
                .parent()
                .ok_or_else(|| anyhow!("project.json is missing a parent directory"))?
                .to_path_buf()
        };
        detect_from_project_json(&effective_root, &project_json_path, &project_json)
    } else {
        detect_without_project_json(root)
    }
}

pub fn import_wallpaper_path(input_path: &Path) -> Result<WallpaperRecord> {
    import_wallpaper_path_inner(input_path, |record| {
        static_snapshot_generation_service::ensure_static_snapshot_for_record(record)
    })
}

fn import_wallpaper_path_inner<GenerateStaticSnapshot>(
    input_path: &Path,
    generate_static_snapshot: GenerateStaticSnapshot,
) -> Result<WallpaperRecord>
where
    GenerateStaticSnapshot:
        FnOnce(
            &mut WallpaperRecord,
        ) -> static_snapshot_generation_service::StaticSnapshotGenerationOutcome,
{
    let source = input_path
        .canonicalize()
        .with_context(|| format!("Input path does not exist: {}", input_path.display()))?;
    let id = Uuid::new_v4().to_string();
    let managed_root = wallpaper_dir(&id)?;
    let imported_source = import_source_copy(&source, &managed_root)?;
    let project = detect_project(&imported_source)?;
    let mut record = WallpaperRecord {
        id,
        title: project.title,
        wallpaper_type: project.wallpaper_type,
        source_path: source.display().to_string(),
        managed_path: project.managed_root.display().to_string(),
        preview_path: project.preview_path.map(|path| path.display().to_string()),
        entry_path: project.entry_path.map(|path| path.display().to_string()),
        last_snapshot_path: None,
        property_schema: project.property_schema,
        property_sections: project.property_sections,
        scene_cache: None,
        scene_manifest: project.scene_manifest,
        scene_manifest_version: None,
        scene_manifest_dirty: false,
        imported_at: Utc::now(),
        tags: project.tags,
    };
    let manifest = record.scene_manifest.clone();
    scene_cache_service::refresh_cache_for_record(&mut record, manifest)?;
    let _ = generate_static_snapshot(&mut record);
    Ok(record)
}

pub fn refresh_record_metadata(record: &mut WallpaperRecord) -> Result<bool> {
    refresh_record_metadata_inner(record, |record, refresh_existing| {
        if refresh_existing {
            static_snapshot_generation_service::regenerate_static_snapshot_for_record(record)
        } else {
            static_snapshot_generation_service::ensure_static_snapshot_for_record(record)
        }
    })
}

fn refresh_record_metadata_inner<GenerateStaticSnapshot>(
    record: &mut WallpaperRecord,
    mut generate_static_snapshot: GenerateStaticSnapshot,
) -> Result<bool>
where
    GenerateStaticSnapshot:
        FnMut(
            &mut WallpaperRecord,
            bool,
        ) -> static_snapshot_generation_service::StaticSnapshotGenerationOutcome,
{
    let resolver = AssetResolver::for_record(record);
    let managed_source = resolver.source_root().to_path_buf();
    if !managed_source.exists() {
        return Ok(false);
    }

    let previous_wallpaper_type = record.wallpaper_type.clone();
    let previous_entry_path = record.entry_path.clone();
    let previous_snapshot_path = record.last_snapshot_path.clone();

    let project = detect_project(&managed_source)?;
    let updated_preview_path = project.preview_path.map(|path| path.display().to_string());
    let updated_entry_path = project.entry_path.map(|path| path.display().to_string());
    let updated_scene_manifest = project.scene_manifest;

    let changed = record.title != project.title
        || record.wallpaper_type != project.wallpaper_type
        || record.preview_path != updated_preview_path
        || record.entry_path != updated_entry_path
        || record.property_schema != project.property_schema
        || record.property_sections != project.property_sections;

    record.title = project.title;
    record.wallpaper_type = project.wallpaper_type;
    record.preview_path = updated_preview_path;
    record.entry_path = updated_entry_path;
    record.property_schema = project.property_schema;
    record.property_sections = project.property_sections;
    record.scene_manifest = updated_scene_manifest;
    record.tags = project.tags;
    let manifest = record.scene_manifest.clone();
    let cache_changed = scene_cache_service::refresh_cache_for_record(record, manifest)?;
    let snapshot_needs_refresh = previous_wallpaper_type != record.wallpaper_type
        || previous_entry_path != record.entry_path
        || !registered_snapshot_is_usable(record);
    if previous_wallpaper_type != record.wallpaper_type || previous_entry_path != record.entry_path
    {
        record.last_snapshot_path = None;
    }
    if snapshot_needs_refresh {
        let refresh_existing = previous_wallpaper_type != record.wallpaper_type
            || previous_entry_path != record.entry_path;
        let _ = generate_static_snapshot(record, refresh_existing);
    }
    let snapshot_changed = record.last_snapshot_path != previous_snapshot_path;

    Ok(changed || cache_changed || snapshot_changed)
}

pub fn update_property_values(
    record: &mut WallpaperRecord,
    values: &BTreeMap<String, Value>,
) -> Result<()> {
    let schema_by_key: BTreeMap<String, usize> = record
        .property_schema
        .iter()
        .enumerate()
        .map(|(index, property)| (property.key.clone(), index))
        .collect();

    for (key, value) in values {
        let index = schema_by_key
            .get(key)
            .copied()
            .ok_or_else(|| anyhow!("Unknown wallpaper property: {key}"))?;
        record.property_schema[index].value = value.clone();
    }

    let resolver = AssetResolver::for_record(record);
    let project_json_path = resolver.source_root().join("project.json");
    if project_json_path.exists() {
        let raw = fs::read_to_string(&project_json_path)?;
        let mut project_json: Value = serde_json::from_str(&raw)?;
        if let Some(properties) = project_json
            .get_mut("general")
            .and_then(|general| general.get_mut("properties"))
            .and_then(Value::as_object_mut)
        {
            for (key, value) in values {
                if let Some(property) = properties.get_mut(key).and_then(Value::as_object_mut) {
                    property.insert("value".to_string(), value.clone());
                }
            }
        }
        fs::write(
            project_json_path,
            serde_json::to_string_pretty(&project_json)?,
        )?;
    }

    if matches!(
        record.wallpaper_type,
        WallpaperType::Scene | WallpaperType::Video | WallpaperType::Web
    ) {
        let resolver = AssetResolver::for_record(record);
        let refreshed = detect_project(resolver.source_root())?;
        record.title = refreshed.title;
        record.wallpaper_type = refreshed.wallpaper_type;
        record.preview_path = refreshed
            .preview_path
            .map(|path| path.display().to_string());
        record.entry_path = refreshed.entry_path.map(|path| path.display().to_string());
        record.property_schema = refreshed.property_schema;
        record.property_sections = refreshed.property_sections;
        record.scene_manifest = refreshed.scene_manifest;
        record.tags = refreshed.tags;
        let manifest = record.scene_manifest.clone();
        scene_cache_service::refresh_cache_for_record(record, manifest)?;
    }

    Ok(())
}

fn registered_snapshot_is_usable(record: &WallpaperRecord) -> bool {
    static_snapshot_service::snapshot_for_record(record).is_ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{cell::Cell, env};

    use tempfile::tempdir;

    use super::{
        build_property_sections, clean_markup, detect_project, fallback_property_label,
        import_source_copy, import_wallpaper_path_inner, parse_options, property_schema,
        refresh_record_metadata_inner,
    };
    use crate::{
        models::{PropertyPresentation, PropertySectionItemKind, WallpaperRecord, WallpaperType},
        services::static_snapshot_generation_service::{
            ensure_static_snapshot_for_record_with, regenerate_static_snapshot_for_record_with,
            STATIC_SNAPSHOT_FILE_NAME,
        },
    };
    use serde_json::json;

    #[test]
    fn strips_basic_markup() {
        let text = clean_markup("<big><b>Hello<br/>World</b></big>");
        assert_eq!(text, "Hello / World");
    }

    #[test]
    fn assigns_stable_fallback_names_to_dock_icon_properties() {
        assert_eq!(
            fallback_property_label("appdockapplyusertexture1", "自定义图标 Use Custom Icon"),
            "自定义图标 #1"
        );
        assert_eq!(
            fallback_property_label("appdockenable7", "启用Enable #7"),
            "启用图标 #7"
        );
    }

    #[test]
    fn builds_property_sections_from_grouped_project_schema() {
        let project = json!({
            "general": {
                "properties": {
                    "mainsettings": {
                        "type": "group",
                        "text": "Main settings",
                        "order": 1
                    },
                    "showclock": {
                        "type": "bool",
                        "text": "Show clock",
                        "value": true,
                        "order": 2
                    },
                    "divider": {
                        "type": "text",
                        "text": "<hr/>",
                        "order": 3
                    },
                    "music": {
                        "type": "group",
                        "text": "Music",
                        "order": 4
                    },
                    "musicsize": {
                        "type": "slider",
                        "text": "Music Size",
                        "value": 0.7,
                        "min": 0,
                        "max": 1,
                        "step": 0.01,
                        "order": 5
                    }
                }
            }
        });

        let properties = property_schema(&project);
        let sections = build_property_sections(&properties);

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].label, "Main settings");
        assert_eq!(sections[1].label, "Music");
        assert!(sections[0]
            .items
            .iter()
            .any(|item| item.kind == PropertySectionItemKind::Property
                && item.key.as_deref() == Some("showclock")));
        assert!(sections[0]
            .items
            .iter()
            .any(|item| item.kind == PropertySectionItemKind::Separator));
        assert!(sections[1]
            .items
            .iter()
            .any(|item| item.kind == PropertySectionItemKind::Property
                && item.key.as_deref() == Some("musicsize")));
    }

    #[test]
    fn keeps_hr_wrapped_bool_and_combo_controls_as_controls() {
        let project = json!({
            "general": {
                "properties": {
                    "audioinductionsettings": {
                        "type": "group",
                        "text": "Audio induction settings",
                        "order": 1
                    },
                    "hrbigb": {
                        "type": "bool",
                        "text": "<hr><big><b>🔘音频条1<br/>Audio Bar 1",
                        "value": true,
                        "order": 2
                    },
                    "hrbigb2braudiobar2": {
                        "type": "combo",
                        "text": "<hr><big><b>🔘音频条2<br/>Audio Bar 2",
                        "value": "1",
                        "order": 3,
                        "options": [
                            { "label": "关闭 Close", "value": "0" },
                            { "label": "style 1", "value": "1" }
                        ]
                    }
                }
            }
        });

        let properties = property_schema(&project);
        assert_eq!(
            properties
                .iter()
                .find(|property| property.key == "hrbigb")
                .map(|property| property.presentation.clone()),
            Some(PropertyPresentation::Control)
        );
        assert_eq!(
            properties
                .iter()
                .find(|property| property.key == "hrbigb2braudiobar2")
                .map(|property| property.presentation.clone()),
            Some(PropertyPresentation::Control)
        );
        assert_eq!(
            properties
                .iter()
                .find(|property| property.key == "hrbigb2braudiobar2")
                .map(|property| property.options.len()),
            Some(2)
        );
    }

    #[test]
    fn parses_combo_options_from_object_array() {
        let options = parse_options(&json!([
            { "label": "关闭 Close", "value": "0" },
            { "label": "style 3", "value": "3" }
        ]));
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].label, "关闭 Close");
        assert_eq!(options[0].value, "0");
        assert_eq!(options[1].label, "style 3");
        assert_eq!(options[1].value, "3");
    }

    #[test]
    fn detects_standalone_video_file_without_project_json() {
        let temp = tempfile::tempdir().expect("temp dir");
        let video_path = temp.path().join("sample.mp4");
        fs::write(&video_path, b"not-a-real-video").expect("video fixture");

        let project = detect_project(&video_path).expect("project detected");
        assert_eq!(project.wallpaper_type, WallpaperType::Video);
        assert_eq!(project.entry_path.as_deref(), Some(video_path.as_path()));
        assert!(project.scene_manifest.is_none());
        assert_eq!(project.property_schema.len(), 0);
    }

    #[test]
    fn detects_standalone_web_file_without_project_json() {
        let temp = tempfile::tempdir().expect("temp dir");
        let html_path = temp.path().join("index.html");
        fs::write(&html_path, "<html><body>hello</body></html>").expect("html fixture");

        let project = detect_project(&html_path).expect("project detected");
        assert_eq!(project.wallpaper_type, WallpaperType::Web);
        assert_eq!(project.entry_path.as_deref(), Some(html_path.as_path()));
        assert!(project.scene_manifest.is_none());
        assert_eq!(project.property_schema.len(), 0);
    }

    #[test]
    fn detects_web_directory_without_project_json() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("web-wallpaper");
        fs::create_dir_all(&root).expect("web root");
        let entry_path = root.join("index.html");
        let preview_path = root.join("preview.png");
        fs::write(&entry_path, "<html><body>hello</body></html>").expect("html fixture");
        fs::write(&preview_path, b"png").expect("preview fixture");

        let project = detect_project(&root).expect("project detected");
        assert_eq!(project.wallpaper_type, WallpaperType::Web);
        assert_eq!(project.entry_path.as_deref(), Some(entry_path.as_path()));
        assert_eq!(
            project.preview_path.as_deref(),
            Some(preview_path.as_path())
        );
        assert!(project.scene_manifest.is_none());
        assert_eq!(project.property_sections.len(), 0);
    }

    #[test]
    fn portable_scene_project_builds_manifest_without_machine_specific_samples() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        let extracted = managed.join("extracted");
        fs::create_dir_all(&source).expect("source root");
        fs::create_dir_all(&extracted).expect("extracted root");
        fs::write(source.join("preview.png"), b"png").expect("preview");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Portable Scene",
                "type": "scene",
                "file": "scene.json",
                "preview": "preview.png",
                "general": {
                    "properties": {
                        "dockenabled": {
                            "type": "bool",
                            "text": "Dock Enabled",
                            "value": true
                        }
                    }
                }
            }))
            .expect("project json"),
        )
        .expect("write project json");
        fs::write(
            extracted.join("scene.json"),
            serde_json::to_string_pretty(&json!({
                "general": {
                    "orthogonalprojection": {
                        "width": 1280,
                        "height": 720
                    }
                },
                "objects": []
            }))
            .expect("scene json"),
        )
        .expect("write scene json");

        let project = detect_project(&source).expect("project detected");
        assert_eq!(project.wallpaper_type, WallpaperType::Scene);
        let manifest = project.scene_manifest.expect("scene manifest");
        assert_eq!(manifest.object_count, 0);
        assert_eq!(
            project
                .entry_path
                .as_deref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str()),
            Some("scene.json")
        );
        assert_eq!(
            project
                .property_schema
                .iter()
                .find(|property| property.key == "dockenabled")
                .map(|property| property.label.as_str()),
            Some("Dock Enabled")
        );
    }

    #[test]
    fn portable_video_project_is_detected_from_project_json() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).expect("source root");
        fs::write(source.join("clip.mp4"), b"not-a-real-video").expect("video");
        fs::write(source.join("preview.gif"), b"gif").expect("preview");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Portable Video",
                "type": "video",
                "file": "clip.mp4",
                "preview": "preview.gif",
                "general": {
                    "properties": {
                        "schemecolor": {
                            "type": "color",
                            "text": "ui_browse_properties_scheme_color",
                            "value": "0.1 0.2 0.3"
                        }
                    }
                }
            }))
            .expect("project json"),
        )
        .expect("write project json");

        let project = detect_project(&source).expect("project detected");
        assert_eq!(project.wallpaper_type, WallpaperType::Video);
        assert_eq!(
            project
                .entry_path
                .as_deref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str()),
            Some("clip.mp4")
        );
        assert!(project
            .property_schema
            .iter()
            .any(|property| property.key == "schemecolor"));
    }

    #[test]
    fn portable_web_project_is_detected_from_project_json() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).expect("source root");
        fs::write(source.join("index.html"), "<html><body>Hello</body></html>").expect("html");
        fs::write(source.join("preview.gif"), b"gif").expect("preview");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Portable Web",
                "type": "web",
                "file": "index.html",
                "preview": "preview.gif",
                "general": {
                    "properties": {
                        "backgroundcolor": {
                            "type": "color",
                            "text": "background color",
                            "value": "0 0 0"
                        },
                        "clockcolor": {
                            "type": "color",
                            "text": "clock color",
                            "value": "1 1 1"
                        }
                    }
                }
            }))
            .expect("project json"),
        )
        .expect("write project json");

        let project = detect_project(&source).expect("project detected");
        assert_eq!(project.wallpaper_type, WallpaperType::Web);
        assert_eq!(
            project
                .entry_path
                .as_deref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str()),
            Some("index.html")
        );
        assert!(project
            .property_schema
            .iter()
            .any(|property| property.key == "backgroundcolor"));
        assert!(project
            .property_schema
            .iter()
            .any(|property| property.key == "clockcolor"));
    }

    #[test]
    fn video_import_generates_and_registers_static_snapshot() {
        let _lock = crate::store::HOME_ENV_LOCK.lock().unwrap();
        let temp = tempdir().expect("temp dir");
        let previous_home = env::var_os("HOME");
        env::set_var("HOME", temp.path().join("home"));
        let source = temp.path().join("video-source");
        fs::create_dir_all(&source).expect("source root");
        fs::write(source.join("clip.mp4"), b"video").expect("video");
        fs::write(source.join("preview.png"), b"preview").expect("preview");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Import Snapshot Video",
                "type": "video",
                "file": "clip.mp4",
                "preview": "preview.png"
            }))
            .expect("project json"),
        )
        .expect("write project json");

        let result = (|| {
            import_wallpaper_path_inner(&source, |record| {
                ensure_static_snapshot_for_record_with(record, |_source_path, output| {
                    fs::write(output, b"snapshot").map_err(|error| error.to_string())
                })
            })
        })();

        match previous_home {
            Some(home) => env::set_var("HOME", home),
            None => env::remove_var("HOME"),
        }

        let record = result.expect("imported record");
        assert_eq!(record.wallpaper_type, WallpaperType::Video);
        let snapshot_path = record
            .last_snapshot_path
            .as_deref()
            .expect("snapshot registered");
        assert!(snapshot_path.ends_with(STATIC_SNAPSHOT_FILE_NAME));
        assert_ne!(record.preview_path.as_deref(), Some(snapshot_path));
        assert_eq!(fs::read(snapshot_path).expect("snapshot"), b"snapshot");
    }

    #[test]
    fn refresh_record_metadata_preserves_existing_valid_video_snapshot() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).expect("source root");
        fs::write(source.join("clip.mp4"), b"video").expect("video");
        fs::write(source.join("preview.png"), b"preview").expect("preview");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Refresh Snapshot Video",
                "type": "video",
                "file": "clip.mp4",
                "preview": "preview.png",
                "general": {
                    "properties": {
                        "speed": {
                            "type": "slider",
                            "text": "Speed",
                            "value": 1
                        }
                    }
                }
            }))
            .expect("project json"),
        )
        .expect("write project json");
        let project = detect_project(&source).expect("project");
        let snapshot = managed.join(STATIC_SNAPSHOT_FILE_NAME);
        fs::write(&snapshot, b"existing").expect("snapshot");
        let mut record =
            record_from_project("refresh-video", managed.display().to_string(), project);
        record.last_snapshot_path = Some(snapshot.display().to_string());

        let changed = refresh_record_metadata_inner(&mut record, |_record, _refresh_existing| {
            panic!("valid registered video snapshot should not be regenerated")
        })
        .expect("refresh");

        assert!(!changed);
        assert_eq!(
            record.last_snapshot_path.as_deref(),
            Some(snapshot.to_string_lossy().as_ref())
        );
        assert_eq!(fs::read(&snapshot).expect("snapshot"), b"existing");
    }

    #[test]
    fn refresh_record_metadata_generates_missing_video_snapshot() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).expect("source root");
        fs::write(source.join("clip.mp4"), b"video").expect("video");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Missing Snapshot Video",
                "type": "video",
                "file": "clip.mp4"
            }))
            .expect("project json"),
        )
        .expect("write project json");
        let project = detect_project(&source).expect("project");
        let mut record = record_from_project(
            "missing-snapshot-video",
            managed.display().to_string(),
            project,
        );
        let invoked = Cell::new(false);

        let changed = refresh_record_metadata_inner(&mut record, |record, refresh_existing| {
            assert!(!refresh_existing);
            invoked.set(true);
            ensure_static_snapshot_for_record_with(record, |_source_path, output| {
                fs::write(output, b"generated").map_err(|error| error.to_string())
            })
        })
        .expect("refresh");

        assert!(changed);
        assert!(invoked.get());
        let snapshot_path = record
            .last_snapshot_path
            .as_deref()
            .expect("snapshot registered");
        assert_eq!(fs::read(snapshot_path).expect("snapshot"), b"generated");
    }

    #[test]
    fn refresh_record_metadata_clears_stale_snapshot_when_video_entry_changes_and_generation_fails()
    {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let source = managed.join("source");
        fs::create_dir_all(&source).expect("source root");
        fs::write(source.join("clip-a.mp4"), b"video-a").expect("video a");
        fs::write(source.join("clip-b.mp4"), b"video-b").expect("video b");
        fs::write(
            source.join("project.json"),
            serde_json::to_string_pretty(&json!({
                "title": "Changed Entry Video",
                "type": "video",
                "file": "clip-b.mp4"
            }))
            .expect("project json"),
        )
        .expect("write project json");
        let project = detect_project(&source).expect("project");
        let stale_snapshot = managed.join(STATIC_SNAPSHOT_FILE_NAME);
        fs::write(&stale_snapshot, b"stale").expect("stale snapshot");
        let mut record =
            record_from_project("changed-entry", managed.display().to_string(), project);
        record.entry_path = Some(source.join("clip-a.mp4").display().to_string());
        record.last_snapshot_path = Some(stale_snapshot.display().to_string());

        let changed = refresh_record_metadata_inner(&mut record, |record, refresh_existing| {
            assert!(refresh_existing);
            regenerate_static_snapshot_for_record_with(record, |_source_path, _output| {
                Err("simulated generator failure".to_string())
            })
        })
        .expect("refresh");

        assert!(changed);
        assert!(record
            .entry_path
            .as_deref()
            .is_some_and(|entry| entry.ends_with("clip-b.mp4")));
        assert!(record.last_snapshot_path.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_entries_during_directory_import() {
        let temp = tempdir().expect("temp dir");
        let source = temp.path().join("wallpaper");
        let outside = temp.path().join("outside.txt");
        let managed_root = temp.path().join("managed");
        fs::create_dir_all(&source).expect("source root");
        fs::write(&outside, b"outside").expect("outside fixture");
        symlink(&outside, source.join("preview.png")).expect("symlink fixture");

        let error =
            import_source_copy(&source, &managed_root).expect_err("symlinked import should fail");
        assert!(
            error.to_string().contains("symlinked path"),
            "unexpected error: {error:#}"
        );
    }

    fn record_from_project(
        id: &str,
        managed_path: String,
        project: super::ImportedProject,
    ) -> WallpaperRecord {
        WallpaperRecord {
            id: id.to_string(),
            title: project.title,
            wallpaper_type: project.wallpaper_type,
            source_path: managed_path.clone(),
            managed_path,
            preview_path: project.preview_path.map(|path| path.display().to_string()),
            entry_path: project.entry_path.map(|path| path.display().to_string()),
            last_snapshot_path: None,
            property_schema: project.property_schema,
            property_sections: project.property_sections,
            scene_cache: None,
            scene_manifest: project.scene_manifest,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: chrono::Utc::now(),
            tags: project.tags,
        }
    }
}
