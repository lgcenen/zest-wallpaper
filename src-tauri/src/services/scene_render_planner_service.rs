use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

use crate::models::{
    EvaluatedAudioState, EvaluatedSceneObject, EvaluatedTextState, SceneAssetKind,
    SceneParticleKind, SceneRuntimeDocument, SceneTextBehavior, SceneTextLayer,
};

use super::scene_resource_service::{
    font_reference_looks_like_path, scene_text_font_reference_kind, SceneResourceResolver,
    SceneTextFontCandidates, SceneTextFontReferenceKind,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum SceneRenderIssueSeverity {
    Warning,
    Fatal,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum SceneRenderIssueCode {
    UnsupportedVisualAsset,
    MissingAssetPath,
    MissingAssetFile,
    MissingRenderBounds,
    NoRenderableVisuals,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneRenderIssue {
    pub severity: SceneRenderIssueSeverity,
    pub code: SceneRenderIssueCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "camelCase")]
pub struct SceneClearColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SceneRenderBlendMode {
    #[default]
    Normal,
    Additive,
    Multiply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SceneRenderSourceKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SceneTextHorizontalAlign {
    Left,
    Right,
    #[default]
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SceneTextVerticalAlign {
    Top,
    Bottom,
    #[default]
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct SceneRenderColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderCamera {
    pub zoom: f64,
    pub center: [f64; 2],
    pub camera_shake: bool,
    pub camera_shake_amplitude: f64,
    pub camera_shake_speed: f64,
    pub parallax_mouse_influence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneRenderQuad {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub opacity: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderVisualItem {
    pub object_id: u32,
    pub object_name: String,
    pub texture_path: PathBuf,
    pub source_kind: SceneRenderSourceKind,
    pub quad: SceneRenderQuad,
    pub blend_mode: SceneRenderBlendMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderTextItem {
    pub object_id: u32,
    pub object_name: String,
    pub behavior: SceneTextBehavior,
    pub quad: SceneRenderQuad,
    pub content_left: f64,
    pub content_top: f64,
    pub content_width: f64,
    pub content_height: f64,
    pub text: String,
    pub font: SceneRenderTextFontBinding,
    pub point_size: f64,
    pub color: SceneRenderColor,
    pub horizontal_align: SceneTextHorizontalAlign,
    pub vertical_align: SceneTextVerticalAlign,
    pub blur_enabled: bool,
    pub blur_radius: f64,
    pub effect_paths: Vec<String>,
    pub max_rows: Option<usize>,
    pub limit_width: bool,
    pub limit_use_ellipsis: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRenderTextFontBinding {
    pub authored_reference: Option<String>,
    pub reference_kind: Option<SceneTextFontReferenceKind>,
    pub file_candidates: Vec<PathBuf>,
    pub family_candidates: Vec<String>,
    pub cache_key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderAudioItem {
    pub object_id: u32,
    pub object_name: String,
    pub quad: SceneRenderQuad,
    pub bar_count: usize,
    pub gap: f64,
    pub bar_width: f64,
    pub bar_radius: f64,
    pub drawable_top: f64,
    pub drawable_height: f64,
    pub min_scale: f64,
    pub normalized_lower_bound: f64,
    pub volume_factor: f64,
    pub color: SceneRenderColor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderParticleItem {
    pub object_id: u32,
    pub object_name: String,
    pub particle_kind: SceneParticleKind,
    pub color: SceneRenderColor,
    pub size: f64,
    pub emission_rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderSoundItem {
    pub object_id: u32,
    pub object_name: String,
    pub asset_path: PathBuf,
    pub looped: bool,
    pub volume: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderPlan {
    pub clear_color: SceneClearColor,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub camera: SceneRenderCamera,
    pub visuals: Vec<SceneRenderVisualItem>,
    pub texts: Vec<SceneRenderTextItem>,
    pub audios: Vec<SceneRenderAudioItem>,
    pub particles: Vec<SceneRenderParticleItem>,
    pub sounds: Vec<SceneRenderSoundItem>,
}

impl SceneRenderPlan {
    pub fn has_renderable_output(&self) -> bool {
        !self.visuals.is_empty()
            || !self.texts.is_empty()
            || !self.audios.is_empty()
            || !self.particles.is_empty()
            || !self.sounds.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderPlanReport {
    pub plan: SceneRenderPlan,
    pub issues: Vec<SceneRenderIssue>,
}

impl SceneRenderPlanReport {
    pub fn is_blocked(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == SceneRenderIssueSeverity::Fatal)
    }

    pub fn warnings(&self) -> Vec<SceneRenderIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == SceneRenderIssueSeverity::Warning)
            .cloned()
            .collect()
    }

    pub fn fatal_errors(&self) -> Vec<SceneRenderIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == SceneRenderIssueSeverity::Fatal)
            .cloned()
            .collect()
    }
}

pub fn parse_scene_clear_color(value: Option<&str>) -> SceneClearColor {
    let Some(value) = value else {
        return SceneClearColor::default();
    };

    let parts = value
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter_map(|segment| {
            let trimmed = segment.trim();
            (!trimmed.is_empty())
                .then(|| trimmed.parse::<f64>().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    if parts.len() < 3 {
        return SceneClearColor::default();
    }

    SceneClearColor {
        red: normalize_color_channel(parts[0]),
        green: normalize_color_channel(parts[1]),
        blue: normalize_color_channel(parts[2]),
        alpha: parts
            .get(3)
            .copied()
            .map(normalize_color_channel)
            .unwrap_or(255),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn build_scene_render_plan(scene: &SceneRuntimeDocument) -> SceneRenderPlanReport {
    build_scene_render_plan_with_resolver(scene, None)
}

pub fn build_scene_render_plan_with_resolver(
    scene: &SceneRuntimeDocument,
    resolver: Option<&SceneResourceResolver>,
) -> SceneRenderPlanReport {
    let mut visuals = Vec::new();
    let mut texts = Vec::new();
    let mut audios = Vec::new();
    let mut particles = Vec::new();
    let mut sounds = Vec::new();
    let mut issues = Vec::new();
    let source_text_layers = scene
        .source
        .text_layers
        .iter()
        .map(|layer| (layer.id, layer))
        .collect::<BTreeMap<_, _>>();

    for object_id in &scene.evaluated.render_list {
        let Some(object) = scene.evaluated.objects.get(object_id) else {
            continue;
        };

        match object {
            EvaluatedSceneObject::Container { .. } => {}
            EvaluatedSceneObject::Visual {
                base,
                asset_kind,
                asset_path,
                blend_mode,
                color_blend_mode,
                ..
            } => {
                if !base.visible || base.opacity <= 0.001 {
                    continue;
                }

                let Some(bounds) = base.transform.render_bounds.filter(valid_render_bounds) else {
                    push_unique_issue(
                        &mut issues,
                        SceneRenderIssue::missing_render_bounds(
                            base.id,
                            &base.name,
                            "visual",
                            "Visual has no evaluated renderBounds in the phase-10 native plan.",
                        ),
                    );
                    continue;
                };

                let source_kind = match asset_kind {
                    SceneAssetKind::Image | SceneAssetKind::System => SceneRenderSourceKind::Image,
                    SceneAssetKind::Video => SceneRenderSourceKind::Video,
                    SceneAssetKind::Unsupported => {
                        push_unique_issue(
                            &mut issues,
                            SceneRenderIssue::unsupported_visual_asset(
                                base.id,
                                &base.name,
                                asset_kind_name(asset_kind),
                            ),
                        );
                        continue;
                    }
                };

                let Some(texture_path) = required_file_path(
                    &mut issues,
                    base.id,
                    &base.name,
                    "visual",
                    asset_path.as_deref(),
                    match asset_kind {
                        SceneAssetKind::Video => SceneRenderIssueSeverity::Fatal,
                        _ => SceneRenderIssueSeverity::Warning,
                    },
                ) else {
                    continue;
                };

                visuals.push(SceneRenderVisualItem {
                    object_id: base.id,
                    object_name: base.name.clone(),
                    texture_path,
                    source_kind,
                    quad: quad_from_bounds(bounds, base.transform.rotation, base.opacity, base),
                    blend_mode: parse_visual_blend_mode(blend_mode.as_deref(), *color_blend_mode),
                });
            }
            EvaluatedSceneObject::Text {
                base,
                behavior,
                text,
            } => {
                if !base.visible || base.opacity <= 0.001 {
                    continue;
                }

                let Some(text_item) = plan_text_item(
                    base.id,
                    &base.name,
                    base,
                    behavior,
                    text,
                    source_text_layers.get(&base.id).copied(),
                    resolver,
                    &mut issues,
                ) else {
                    continue;
                };
                texts.push(text_item);
            }
            EvaluatedSceneObject::Audio { base, audio } => {
                if !base.visible || base.opacity <= 0.001 {
                    continue;
                }

                let Some(audio_item) =
                    plan_audio_item(base.id, &base.name, base, audio, &mut issues)
                else {
                    continue;
                };
                audios.push(audio_item);
            }
            EvaluatedSceneObject::Particle {
                base,
                particle_kind,
                color,
                size,
                emission_rate,
                ..
            } => {
                if !base.visible {
                    continue;
                }

                particles.push(SceneRenderParticleItem {
                    object_id: base.id,
                    object_name: base.name.clone(),
                    particle_kind: particle_kind.clone(),
                    color: parse_scene_color(
                        color.as_deref(),
                        particle_alpha(particle_kind.clone()),
                    ),
                    size: size.max(0.1),
                    emission_rate: emission_rate.max(0.0),
                });
            }
            EvaluatedSceneObject::Sound {
                base,
                asset_path,
                looped,
                volume,
            } => {
                if !base.visible || *volume <= 0.001 {
                    continue;
                }

                let Some(sound_path) = required_file_path(
                    &mut issues,
                    base.id,
                    &base.name,
                    "sound",
                    Some(asset_path.as_str()),
                    SceneRenderIssueSeverity::Warning,
                ) else {
                    continue;
                };

                sounds.push(SceneRenderSoundItem {
                    object_id: base.id,
                    object_name: base.name.clone(),
                    asset_path: sound_path,
                    looped: *looped,
                    volume: volume.clamp(0.0, 1.0),
                });
            }
        }
    }

    let plan = SceneRenderPlan {
        clear_color: parse_scene_clear_color(scene.evaluated.clear_color.as_deref()),
        canvas_width: scene.evaluated.canvas_width.max(1.0),
        canvas_height: scene.evaluated.canvas_height.max(1.0),
        camera: SceneRenderCamera {
            zoom: scene.evaluated.camera.zoom.max(0.001),
            center: scene.evaluated.camera.center,
            camera_shake: scene.evaluated.camera.camera_shake,
            camera_shake_amplitude: scene.evaluated.camera.camera_shake_amplitude.max(0.0),
            camera_shake_speed: scene.evaluated.camera.camera_shake_speed.max(0.0),
            parallax_mouse_influence: scene.evaluated.camera.parallax_mouse_influence.max(0.0),
        },
        visuals,
        texts,
        audios,
        particles,
        sounds,
    };

    if !plan.has_renderable_output() {
        push_unique_issue(&mut issues, SceneRenderIssue::no_renderable_visuals());
    }

    SceneRenderPlanReport { plan, issues }
}

impl SceneRenderIssue {
    fn unsupported_visual_asset(object_id: u32, object_name: &str, asset_kind: &str) -> Self {
        Self {
            severity: SceneRenderIssueSeverity::Warning,
            code: SceneRenderIssueCode::UnsupportedVisualAsset,
            message: format!(
                "{} uses unsupported {asset_kind} visual asset semantics in phase-10.",
                quoted(object_name)
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            object_kind: Some("visual".to_string()),
            resource_path: None,
            detail: None,
        }
    }

    fn missing_asset_path(
        severity: SceneRenderIssueSeverity,
        object_id: u32,
        object_name: &str,
        object_kind: &str,
    ) -> Self {
        Self {
            severity,
            code: SceneRenderIssueCode::MissingAssetPath,
            message: format!(
                "{} has no resolved asset for the native phase-10 {object_kind}.",
                quoted(object_name)
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            object_kind: Some(object_kind.to_string()),
            resource_path: None,
            detail: None,
        }
    }

    fn missing_asset_file(
        severity: SceneRenderIssueSeverity,
        object_id: u32,
        object_name: &str,
        object_kind: &str,
        resource_path: String,
    ) -> Self {
        Self {
            severity,
            code: SceneRenderIssueCode::MissingAssetFile,
            message: format!(
                "{} resolved asset is missing: {}.",
                quoted(object_name),
                resource_path
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            object_kind: Some(object_kind.to_string()),
            resource_path: Some(resource_path),
            detail: None,
        }
    }

    fn missing_render_bounds(
        object_id: u32,
        object_name: &str,
        object_kind: &str,
        detail: &str,
    ) -> Self {
        Self {
            severity: SceneRenderIssueSeverity::Warning,
            code: SceneRenderIssueCode::MissingRenderBounds,
            message: format!(
                "{} has no usable renderBounds for the native phase-10 renderer.",
                quoted(object_name)
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            object_kind: Some(object_kind.to_string()),
            resource_path: None,
            detail: Some(detail.to_string()),
        }
    }

    fn no_renderable_visuals() -> Self {
        Self {
            severity: SceneRenderIssueSeverity::Fatal,
            code: SceneRenderIssueCode::NoRenderableVisuals,
            message: "Scene has no renderable output after phase-10 warning skips.".to_string(),
            object_id: None,
            object_name: None,
            object_kind: Some("scene".to_string()),
            resource_path: None,
            detail: Some(
                "Phase-10 allows warning-only best-effort entry, but apply must fail when the native Scene plan has no drawable visual/text/audio/particle/sound content."
                    .to_string(),
            ),
        }
    }
}

fn quad_from_bounds(
    bounds: [f64; 4],
    rotation: f64,
    opacity: f64,
    base: &crate::models::EvaluatedSceneObjectBase,
) -> SceneRenderQuad {
    let [left, top, width, height] = bounds;
    SceneRenderQuad {
        left,
        top,
        width,
        height,
        rotation,
        opacity: opacity.clamp(0.0, 1.0),
        flip_x: base.transform.scale[0].is_sign_negative(),
        flip_y: base.transform.scale[1].is_sign_negative(),
    }
}

fn plan_text_item(
    object_id: u32,
    object_name: &str,
    base: &crate::models::EvaluatedSceneObjectBase,
    behavior: &SceneTextBehavior,
    text: &EvaluatedTextState,
    source_layer: Option<&SceneTextLayer>,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneRenderTextItem> {
    let bounds = text
        .layout
        .render_bounds
        .or(base.transform.render_bounds)
        .filter(valid_render_bounds);
    let Some(bounds) = bounds else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_render_bounds(
                object_id,
                object_name,
                "text",
                "Text has no evaluated text.layout.renderBounds or base renderBounds.",
            ),
        );
        return None;
    };

    let content_bounds = text
        .layout
        .content_bounds
        .filter(valid_render_bounds)
        .unwrap_or(bounds);
    let quad_bounds = union_bounds(bounds, content_bounds);
    let [left, top, _, _] = quad_bounds;
    let [content_left, content_top, content_width, content_height] = content_bounds;

    Some(SceneRenderTextItem {
        object_id,
        object_name: object_name.to_string(),
        behavior: behavior.clone(),
        quad: quad_from_bounds(quad_bounds, base.transform.rotation, base.opacity, base),
        content_left: (content_left - left).max(0.0),
        content_top: (content_top - top).max(0.0),
        content_width: content_width.max(1.0),
        content_height: content_height.max(1.0),
        text: text.value.clone(),
        font: resolve_text_font_binding(source_layer, text, resolver),
        point_size: text.layout.scaled_point_size.max(1.0),
        color: parse_scene_color(text.style.color.as_deref(), text.style.alpha),
        horizontal_align: parse_horizontal_align(
            text.style
                .horizontal_align
                .as_deref()
                .or(base.alignment.as_deref()),
        ),
        vertical_align: parse_vertical_align(text.style.vertical_align.as_deref()),
        blur_enabled: text
            .style
            .effect_paths
            .iter()
            .any(|path| path.to_ascii_lowercase().contains("blur")),
        blur_radius: (text.layout.scaled_point_size * 0.22).max(4.0),
        effect_paths: text.style.effect_paths.clone(),
        max_rows: text.style.max_rows.map(|rows| rows as usize),
        limit_width: text.style.limit_width.unwrap_or(false),
        limit_use_ellipsis: text.style.limit_use_ellipsis.unwrap_or(false),
    })
}

fn resolve_text_font_binding(
    source_layer: Option<&SceneTextLayer>,
    text: &EvaluatedTextState,
    resolver: Option<&SceneResourceResolver>,
) -> SceneRenderTextFontBinding {
    let authored_reference = source_layer
        .and_then(|layer| layer.font_reference.as_deref())
        .or(text.style.font_path.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let Some(authored_reference) = authored_reference else {
        return SceneRenderTextFontBinding {
            authored_reference: None,
            reference_kind: None,
            file_candidates: Vec::new(),
            family_candidates: Vec::new(),
            cache_key: "font:system".to_string(),
        };
    };
    let reference_kind = scene_text_font_reference_kind(&authored_reference)
        .unwrap_or(SceneTextFontReferenceKind::FamilyLike);

    if let Some(resolver) = resolver {
        return scene_render_text_font_binding_from_candidates(
            resolver.resolve_text_font(&authored_reference),
        );
    }

    let file_candidates = if font_reference_looks_like_path(&authored_reference) {
        vec![PathBuf::from(&authored_reference)]
    } else {
        Vec::new()
    };
    let stem = PathBuf::from(&authored_reference)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string);
    let mut family_candidates = if font_reference_looks_like_path(&authored_reference) {
        optional_primary_text_font_family(stem.as_deref())
    } else {
        vec![authored_reference.clone()]
    };
    family_candidates = dedup_strings(family_candidates);

    SceneRenderTextFontBinding {
        cache_key: format!("font:authored:{authored_reference}"),
        authored_reference: Some(authored_reference),
        reference_kind: Some(reference_kind),
        file_candidates,
        family_candidates,
    }
}

fn scene_render_text_font_binding_from_candidates(
    candidates: SceneTextFontCandidates,
) -> SceneRenderTextFontBinding {
    SceneRenderTextFontBinding {
        authored_reference: Some(candidates.authored_reference),
        reference_kind: Some(candidates.reference_kind),
        file_candidates: candidates.file_candidates,
        family_candidates: dedup_strings(candidates.family_candidates),
        cache_key: candidates.cache_key,
    }
}

fn optional_primary_text_font_family(primary: Option<&str>) -> Vec<String> {
    if let Some(primary) = primary
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    {
        vec![primary]
    } else {
        Vec::new()
    }
}

fn dedup_strings(values: Vec<String>) -> Vec<String> {
    let mut ordered = Vec::new();
    for value in values {
        if !ordered.iter().any(|existing| existing == &value) {
            ordered.push(value);
        }
    }
    ordered
}

fn plan_audio_item(
    object_id: u32,
    object_name: &str,
    base: &crate::models::EvaluatedSceneObjectBase,
    audio: &EvaluatedAudioState,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneRenderAudioItem> {
    let Some(bounds) = base.transform.render_bounds.filter(valid_render_bounds) else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_render_bounds(
                object_id,
                object_name,
                "audio",
                "Audio layer has no evaluated renderBounds in the phase-10 native plan.",
            ),
        );
        return None;
    };

    let [_, _, width, height] = bounds;
    let count = clamp_usize(audio.bar_count, 8, 72);
    let gap =
        (width / count as f64) * clamp_f64(audio.bar_spacing.unwrap_or(0.42), 0.05, 2.5) * 0.22;
    let gap = gap.max(1.0);
    let bar_width = ((width - gap * (count.saturating_sub(1) as f64)) / count as f64).max(2.0);
    let bar_radius = (bar_width * clamp_f64(audio.radius.unwrap_or(0.6), 0.0, 2.5)).max(2.0);
    let [raw_lower_bound, raw_upper_bound] = audio.bar_bounds.unwrap_or([0.0, 0.58]);
    let upper_bound = clamp_f64(raw_upper_bound, 0.01, 1.0);
    let lower_bound = clamp_f64(raw_lower_bound, 0.0, upper_bound);
    let volume_factor = clamp_f64(audio.volume_factor.unwrap_or(1.0), 0.1, 4.0);
    let drawable_height = (height * upper_bound).max(1.0);
    let drawable_top = height - drawable_height;
    let normalized_lower_bound = clamp_f64(lower_bound / upper_bound, 0.0, 1.0);
    let minimum_height = clamp_f64(audio.minimum_height.unwrap_or(0.12), 0.0, 3.0);
    let min_scale = clamp_f64(
        ((bar_width * minimum_height) / drawable_height)
            .max(normalized_lower_bound)
            .max(0.02),
        0.02,
        1.0,
    );
    let opacity = audio.opacity.unwrap_or(base.opacity).clamp(0.05, 1.0);

    Some(SceneRenderAudioItem {
        object_id,
        object_name: object_name.to_string(),
        quad: quad_from_bounds(bounds, base.transform.rotation, opacity, base),
        bar_count: count,
        gap,
        bar_width,
        bar_radius,
        drawable_top,
        drawable_height,
        min_scale,
        normalized_lower_bound,
        volume_factor,
        color: parse_scene_color(audio.color.as_deref(), 0.96),
    })
}

fn required_file_path(
    issues: &mut Vec<SceneRenderIssue>,
    object_id: u32,
    object_name: &str,
    object_kind: &str,
    raw_path: Option<&str>,
    severity: SceneRenderIssueSeverity,
) -> Option<PathBuf> {
    let Some(path) = raw_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_path(severity, object_id, object_name, object_kind),
        );
        return None;
    };

    if !path.is_file() {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_file(
                severity,
                object_id,
                object_name,
                object_kind,
                path.display().to_string(),
            ),
        );
        return None;
    }

    Some(path)
}

fn valid_render_bounds(bounds: &[f64; 4]) -> bool {
    bounds[2].is_finite()
        && bounds[3].is_finite()
        && bounds[2] > 0.0
        && bounds[3] > 0.0
        && bounds[0].is_finite()
        && bounds[1].is_finite()
}

fn union_bounds(primary: [f64; 4], secondary: [f64; 4]) -> [f64; 4] {
    let left = primary[0].min(secondary[0]);
    let top = primary[1].min(secondary[1]);
    let right = (primary[0] + primary[2]).max(secondary[0] + secondary[2]);
    let bottom = (primary[1] + primary[3]).max(secondary[1] + secondary[3]);
    [left, top, (right - left).max(1.0), (bottom - top).max(1.0)]
}

pub(crate) fn parse_visual_blend_mode(
    value: Option<&str>,
    color_blend_mode: Option<i64>,
) -> SceneRenderBlendMode {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("additive") => SceneRenderBlendMode::Additive,
        Some("multiply") => SceneRenderBlendMode::Multiply,
        _ => match color_blend_mode {
            Some(2) => SceneRenderBlendMode::Multiply,
            Some(7 | 9) => SceneRenderBlendMode::Additive,
            _ => SceneRenderBlendMode::Normal,
        },
    }
}

fn parse_horizontal_align(value: Option<&str>) -> SceneTextHorizontalAlign {
    let normalized = value.unwrap_or_default().to_ascii_lowercase();
    if normalized.contains("left") {
        SceneTextHorizontalAlign::Left
    } else if normalized.contains("right") {
        SceneTextHorizontalAlign::Right
    } else {
        SceneTextHorizontalAlign::Center
    }
}

fn parse_vertical_align(value: Option<&str>) -> SceneTextVerticalAlign {
    let normalized = value.unwrap_or_default().to_ascii_lowercase();
    if normalized.contains("top") {
        SceneTextVerticalAlign::Top
    } else if normalized.contains("bottom") {
        SceneTextVerticalAlign::Bottom
    } else {
        SceneTextVerticalAlign::Center
    }
}

pub(crate) fn parse_scene_color(value: Option<&str>, alpha: f64) -> SceneRenderColor {
    let parsed = value
        .map(|value| {
            value
                .split(|character: char| character.is_whitespace() || character == ',')
                .filter_map(|segment| {
                    let trimmed = segment.trim();
                    (!trimmed.is_empty())
                        .then(|| trimmed.parse::<f64>().ok())
                        .flatten()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if parsed.len() >= 3 {
        SceneRenderColor {
            red: normalize_color_channel(parsed[0]),
            green: normalize_color_channel(parsed[1]),
            blue: normalize_color_channel(parsed[2]),
            alpha: normalize_color_channel(alpha),
        }
    } else {
        SceneRenderColor {
            red: 255,
            green: 255,
            blue: 255,
            alpha: normalize_color_channel(alpha),
        }
    }
}

fn normalize_color_channel(component: f64) -> u8 {
    let scaled = if component > 1.0 {
        component.clamp(0.0, 255.0)
    } else {
        (component.clamp(0.0, 1.0) * 255.0).round()
    };
    scaled as u8
}

fn particle_alpha(kind: SceneParticleKind) -> f64 {
    match kind {
        SceneParticleKind::LineTrail => 0.92,
        SceneParticleKind::PetalTrail => 0.88,
    }
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

fn clamp_usize(value: usize, min: usize, max: usize) -> usize {
    value.clamp(min, max)
}

fn asset_kind_name(asset_kind: &SceneAssetKind) -> &'static str {
    match asset_kind {
        SceneAssetKind::Image => "image",
        SceneAssetKind::Video => "video",
        SceneAssetKind::System => "system",
        SceneAssetKind::Unsupported => "unsupported",
    }
}

fn push_unique_issue(issues: &mut Vec<SceneRenderIssue>, candidate: SceneRenderIssue) {
    if issues.iter().any(|existing| existing == &candidate) {
        return;
    }
    issues.push(candidate);
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use chrono::Utc;
    use image::{DynamicImage, Rgba, RgbaImage};
    use tempfile::tempdir;

    use crate::models::{
        EvaluatedAudioState, EvaluatedSceneCamera, EvaluatedSceneObject, EvaluatedSceneObjectBase,
        EvaluatedSceneTransform, EvaluatedTextLayout, EvaluatedTextState, EvaluatedTextStyle,
        SceneAssetKind, SceneEvaluatedDocument, SceneManifest, SceneParticleKind,
        SceneRuntimeDocument, SceneTextBehavior, SceneTextLayer,
    };

    use super::{
        build_scene_render_plan, build_scene_render_plan_with_resolver, parse_scene_clear_color,
        SceneClearColor, SceneRenderBlendMode, SceneRenderCamera, SceneRenderIssueCode,
        SceneRenderIssueSeverity, SceneRenderPlan, SceneRenderSoundItem, SceneRenderSourceKind,
        SceneTextHorizontalAlign,
    };
    use crate::services::scene_resource_service::{
        SceneResourceResolver, SceneTextFontReferenceKind,
    };

    fn runtime_scene_with_objects(
        objects: Vec<(u32, EvaluatedSceneObject)>,
        render_list: Vec<u32>,
    ) -> SceneRuntimeDocument {
        SceneRuntimeDocument {
            runtime_owner_key: None,
            source: Default::default(),
            evaluated: SceneEvaluatedDocument {
                canvas_width: 1920.0,
                canvas_height: 1080.0,
                clear_color: Some("0.2 0.3 0.4 0.5".to_string()),
                camera: EvaluatedSceneCamera {
                    zoom: 1.2,
                    center: [0.0, 0.0],
                    camera_shake: true,
                    camera_shake_amplitude: 0.6,
                    camera_shake_speed: 1.4,
                    parallax_mouse_influence: 0.25,
                },
                parallax: Default::default(),
                objects: objects.into_iter().collect::<BTreeMap<_, _>>(),
                render_list,
                evaluated_at: Utc::now(),
            },
        }
    }

    fn runtime_scene_with_text_source(
        objects: Vec<(u32, EvaluatedSceneObject)>,
        render_list: Vec<u32>,
        text_layers: Vec<SceneTextLayer>,
    ) -> SceneRuntimeDocument {
        let mut scene = runtime_scene_with_objects(objects, render_list);
        scene.source = SceneManifest {
            text_layers,
            ..SceneManifest::default()
        };
        scene
    }

    fn source_text_layer(id: u32, font_reference: Option<&str>) -> SceneTextLayer {
        SceneTextLayer {
            id,
            name: "Clock".to_string(),
            dependencies: vec![],
            parent_id: None,
            alignment: Some("top-left".to_string()),
            anchor: None,
            horizontal_align: Some("right".to_string()),
            vertical_align: Some("top".to_string()),
            content: "12:34".to_string(),
            behavior: SceneTextBehavior::Static,
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
            size: Some([200.0, 50.0]),
            render_bounds: Some([100.0, 100.0, 200.0, 50.0]),
            parallax_depth: None,
            color: Some("1 0.5 0".to_string()),
            color_binding: None,
            alpha: Some(0.8),
            alpha_binding: None,
            point_size: Some(24.0),
            point_size_binding: None,
            font_reference: font_reference.map(str::to_string),
            font_path: None,
            effect_paths: vec!["effects/blur.json".to_string()],
            script_text: None,
            script_refresh_interval_millis: None,
            padding: None,
            max_rows: Some(2),
            max_width: None,
            limit_width: Some(true),
            limit_use_ellipsis: Some(true),
            block_align: None,
        }
    }

    fn object_base(
        id: u32,
        name: &str,
        render_bounds: Option<[f64; 4]>,
    ) -> EvaluatedSceneObjectBase {
        EvaluatedSceneObjectBase {
            id,
            name: name.to_string(),
            parent_id: None,
            dependencies: vec![],
            visible: true,
            alignment: Some("top-left".to_string()),
            opacity: 0.75,
            transform: EvaluatedSceneTransform {
                position: [200.0, 160.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                rotation: 0.3,
                render_bounds,
            },
        }
    }

    fn visual_object(
        id: u32,
        name: &str,
        asset_kind: SceneAssetKind,
        asset_path: Option<String>,
        render_bounds: Option<[f64; 4]>,
        scale: [f64; 3],
        blend_mode: Option<&str>,
        color_blend_mode: Option<i64>,
    ) -> EvaluatedSceneObject {
        let mut base = object_base(id, name, render_bounds);
        base.transform.scale = scale;
        EvaluatedSceneObject::Visual {
            base,
            asset_kind,
            asset_path,
            system_texture_key: None,
            texture_names: vec![],
            blend_mode: blend_mode.map(ToString::to_string),
            color: None,
            brightness: None,
            color_blend_mode,
            parallax_depth: None,
            angles: None,
            fullscreen: false,
            autosize: false,
            solid_layer: false,
            passthrough: false,
            no_padding: false,
            puppet_path: None,
            animation_layers: vec![],
            primary: false,
            background_candidate: false,
        }
    }

    fn text_object(
        id: u32,
        name: &str,
        bounds: [f64; 4],
        content_bounds: [f64; 4],
    ) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Text {
            base: object_base(id, name, Some(bounds)),
            behavior: SceneTextBehavior::Static,
            text: EvaluatedTextState {
                value: "12:34".to_string(),
                style: EvaluatedTextStyle {
                    color: Some("1 0.5 0".to_string()),
                    alpha: 0.8,
                    point_size: 24.0,
                    font_path: None,
                    effect_paths: vec!["effects/blur.json".to_string()],
                    horizontal_align: Some("right".to_string()),
                    vertical_align: Some("top".to_string()),
                    padding: None,
                    max_rows: Some(2),
                    max_width: None,
                    limit_width: Some(true),
                    limit_use_ellipsis: Some(true),
                    block_align: None,
                },
                layout: EvaluatedTextLayout {
                    size: Some([bounds[2], bounds[3]]),
                    render_bounds: Some(bounds),
                    content_bounds: Some(content_bounds),
                    scaled_point_size: 26.0,
                    scaled_padding: 0.0,
                    world_scale: [1.0, 1.0, 1.0],
                },
            },
        }
    }

    fn audio_object(id: u32, name: &str, bounds: [f64; 4]) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Audio {
            base: object_base(id, name, Some(bounds)),
            audio: EvaluatedAudioState {
                bar_count: 32,
                color: Some("0.5 0.8 1".to_string()),
                bar_spacing: Some(0.42),
                bar_bounds: Some([0.1, 0.7]),
                minimum_height: Some(0.12),
                radius: Some(0.6),
                volume_factor: Some(1.2),
                opacity: Some(0.55),
            },
        }
    }

    fn particle_object(id: u32, name: &str, kind: SceneParticleKind) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Particle {
            base: object_base(id, name, None),
            particle_path: "particles/petal.json".to_string(),
            particle_kind: kind,
            color: Some("1 0.4 0.6".to_string()),
            size: 3.0,
            emission_rate: 80.0,
        }
    }

    fn sound_object(id: u32, name: &str, asset_path: String) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Sound {
            base: object_base(id, name, None),
            asset_path,
            looped: true,
            volume: 0.8,
        }
    }

    #[test]
    fn clear_color_parser_accepts_scene_vectors() {
        assert_eq!(
            parse_scene_clear_color(Some("0.5 0.25 1 0.8")),
            SceneClearColor {
                red: 128,
                green: 64,
                blue: 255,
                alpha: 204,
            }
        );
        assert_eq!(
            parse_scene_clear_color(Some("32 64 96")),
            SceneClearColor {
                red: 32,
                green: 64,
                blue: 96,
                alpha: 255,
            }
        );
    }

    #[test]
    fn render_plan_uses_evaluated_rect_and_scale_signs_directly() {
        let temp = tempdir().expect("temp dir");
        let texture_path = temp.path().join("hero.png");
        let mut image = RgbaImage::new(4, 4);
        for pixel in image.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        DynamicImage::ImageRgba8(image)
            .save(&texture_path)
            .expect("save fixture");

        let scene = runtime_scene_with_objects(
            vec![(
                7,
                visual_object(
                    7,
                    "Hero",
                    SceneAssetKind::Image,
                    Some(texture_path.display().to_string()),
                    Some([320.0, 180.0, 640.0, 360.0]),
                    [-1.0, 2.0, 1.0],
                    Some("multiply"),
                    None,
                ),
            )],
            vec![7],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert_eq!(report.plan.clear_color.red, 51);
        assert_eq!(report.plan.visuals.len(), 1);
        assert_eq!(report.plan.visuals[0].quad.left, 320.0);
        assert_eq!(report.plan.visuals[0].quad.top, 180.0);
        assert_eq!(report.plan.visuals[0].quad.width, 640.0);
        assert_eq!(report.plan.visuals[0].quad.height, 360.0);
        assert!(report.plan.visuals[0].quad.flip_x);
        assert!(!report.plan.visuals[0].quad.flip_y);
        assert_eq!(
            report.plan.visuals[0].blend_mode,
            SceneRenderBlendMode::Multiply
        );
    }

    #[test]
    fn render_plan_migrates_text_audio_particle_and_sound_items() {
        let temp = tempdir().expect("temp dir");
        let texture_path = temp.path().join("hero.png");
        let sound_path = temp.path().join("loop.m4a");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([255, 128, 0, 255])))
            .save(&texture_path)
            .expect("save texture");
        std::fs::write(&sound_path, b"fake-audio").expect("sound fixture");

        let scene = runtime_scene_with_objects(
            vec![
                (
                    1,
                    visual_object(
                        1,
                        "Background",
                        SceneAssetKind::Image,
                        Some(texture_path.display().to_string()),
                        Some([0.0, 0.0, 1920.0, 1080.0]),
                        [1.0, 1.0, 1.0],
                        Some("additive"),
                        None,
                    ),
                ),
                (
                    2,
                    text_object(
                        2,
                        "Clock",
                        [100.0, 100.0, 200.0, 50.0],
                        [120.0, 108.0, 160.0, 32.0],
                    ),
                ),
                (3, audio_object(3, "Spectrum", [420.0, 80.0, 300.0, 120.0])),
                (4, particle_object(4, "Trail", SceneParticleKind::LineTrail)),
                (
                    5,
                    sound_object(5, "Ambient", sound_path.display().to_string()),
                ),
            ],
            vec![1, 2, 3, 4, 5],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert_eq!(report.plan.visuals.len(), 1);
        assert_eq!(report.plan.texts.len(), 1);
        assert_eq!(report.plan.audios.len(), 1);
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.sounds.len(), 1);
        assert_eq!(report.plan.texts[0].content_left, 20.0);
        assert_eq!(report.plan.texts[0].content_top, 8.0);
        assert_eq!(
            report.plan.texts[0].horizontal_align,
            SceneTextHorizontalAlign::Right
        );
        assert!(report.plan.texts[0].blur_enabled);
        assert_eq!(
            report.plan.texts[0].effect_paths,
            vec!["effects/blur.json".to_string()]
        );
        assert!(report.plan.texts[0].limit_width);
        assert!(report.plan.texts[0].limit_use_ellipsis);
        assert!(report.plan.audios[0].bar_width > 0.0);
        assert!(report.plan.audios[0].drawable_height > 0.0);
        assert_eq!(
            report.plan.particles[0].particle_kind,
            SceneParticleKind::LineTrail
        );
        assert_eq!(report.plan.sounds[0].volume, 0.8);
    }

    #[test]
    fn render_plan_resolves_text_font_binding_from_builtin_assets() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let builtin_font = builtin_root.join("assets").join("fonts").join("clock.ttf");

        std::fs::create_dir_all(managed_root.join("source")).expect("source dir");
        std::fs::create_dir_all(managed_root.join("extracted")).expect("extracted dir");
        std::fs::create_dir_all(builtin_font.parent().expect("builtin font dir"))
            .expect("builtin font parent");
        std::fs::write(&builtin_font, b"font").expect("builtin font");

        let scene = runtime_scene_with_text_source(
            vec![(
                2,
                text_object(
                    2,
                    "Clock",
                    [100.0, 100.0, 200.0, 50.0],
                    [120.0, 108.0, 160.0, 32.0],
                ),
            )],
            vec![2],
            vec![source_text_layer(2, Some("clock"))],
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert_eq!(report.plan.texts.len(), 1);
        assert_eq!(
            report.plan.texts[0].font.authored_reference.as_deref(),
            Some("clock")
        );
        assert_eq!(
            report.plan.texts[0].font.reference_kind,
            Some(SceneTextFontReferenceKind::FamilyLike)
        );
        assert_eq!(
            report.plan.texts[0].font.file_candidates,
            vec![builtin_font]
        );
        assert!(report.plan.texts[0]
            .font
            .family_candidates
            .contains(&"clock".to_string()));
    }

    #[test]
    fn has_renderable_output_counts_sound_tracks() {
        let plan = SceneRenderPlan {
            clear_color: SceneClearColor::default(),
            canvas_width: 1920.0,
            canvas_height: 1080.0,
            camera: SceneRenderCamera {
                zoom: 1.0,
                center: [0.0, 0.0],
                camera_shake: false,
                camera_shake_amplitude: 0.0,
                camera_shake_speed: 0.0,
                parallax_mouse_influence: 0.0,
            },
            visuals: Vec::new(),
            texts: Vec::new(),
            audios: Vec::new(),
            particles: Vec::new(),
            sounds: vec![SceneRenderSoundItem {
                object_id: 5,
                object_name: "Ambient".to_string(),
                asset_path: PathBuf::from("/tmp/ambient-loop.m4a"),
                looped: true,
                volume: 0.8,
            }],
        };

        assert!(plan.has_renderable_output());
    }

    #[test]
    fn render_plan_expands_text_quad_when_evaluated_text_rect_overflows_box() {
        let scene = runtime_scene_with_objects(
            vec![(
                2,
                text_object(
                    2,
                    "Clock",
                    [100.0, 100.0, 200.0, 50.0],
                    [72.0, 96.0, 256.0, 60.0],
                ),
            )],
            vec![2],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.texts.len(), 1);
        let item = &report.plan.texts[0];
        assert_eq!(item.quad.left, 72.0);
        assert_eq!(item.quad.top, 96.0);
        assert_eq!(item.quad.width, 256.0);
        assert_eq!(item.quad.height, 60.0);
        assert_eq!(item.content_left, 0.0);
        assert_eq!(item.content_top, 0.0);
        assert_eq!(item.content_width, 256.0);
        assert_eq!(item.content_height, 60.0);
    }

    #[test]
    fn video_visual_enters_render_plan_as_dynamic_scene_source() {
        let temp = tempdir().expect("temp dir");
        let video_path = temp.path().join("loop.mp4");
        std::fs::write(&video_path, b"fake-mp4").expect("video placeholder");

        let scene = runtime_scene_with_objects(
            vec![(
                11,
                visual_object(
                    11,
                    "Loop",
                    SceneAssetKind::Video,
                    Some(video_path.display().to_string()),
                    Some([12.0, 24.0, 640.0, 360.0]),
                    [1.0, 1.0, 1.0],
                    None,
                    None,
                ),
            )],
            vec![11],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.visuals.len(), 1);
        assert_eq!(
            report.plan.visuals[0].source_kind,
            SceneRenderSourceKind::Video
        );
        assert!(report.issues.is_empty());
    }

    #[test]
    fn missing_video_visual_asset_blocks_phase_08_plan() {
        let video_path = PathBuf::from("/tmp/phase-08-missing-loop.mp4");
        let scene = runtime_scene_with_objects(
            vec![(
                11,
                visual_object(
                    11,
                    "Loop",
                    SceneAssetKind::Video,
                    Some(video_path.display().to_string()),
                    Some([12.0, 24.0, 640.0, 360.0]),
                    [1.0, 1.0, 1.0],
                    None,
                    None,
                ),
            )],
            vec![11],
        );

        let report = build_scene_render_plan(&scene);

        assert!(report.is_blocked());
        assert!(report.issues.iter().any(|issue| {
            issue.code == SceneRenderIssueCode::MissingAssetFile
                && issue.severity == SceneRenderIssueSeverity::Fatal
        }));
    }

    #[test]
    fn unsupported_visual_skips_become_fatal_when_nothing_is_drawable() {
        let scene = runtime_scene_with_objects(
            vec![(
                9,
                visual_object(
                    9,
                    "Movie",
                    SceneAssetKind::Unsupported,
                    Some("/tmp/movie.mp4".to_string()),
                    Some([0.0, 0.0, 1920.0, 1080.0]),
                    [1.0, 1.0, 1.0],
                    None,
                    None,
                ),
            )],
            vec![9],
        );

        let report = build_scene_render_plan(&scene);

        assert!(report.is_blocked());
        assert_eq!(report.plan.visuals.len(), 0);
        assert!(report.issues.iter().any(|issue| issue.code
            == SceneRenderIssueCode::UnsupportedVisualAsset
            && issue.severity == SceneRenderIssueSeverity::Warning));
        assert!(report.issues.iter().any(|issue| issue.code
            == SceneRenderIssueCode::NoRenderableVisuals
            && issue.severity == SceneRenderIssueSeverity::Fatal));
    }

    #[test]
    fn sound_only_scene_is_valid_phase_08_output() {
        let temp = tempdir().expect("temp dir");
        let sound_path = temp.path().join("loop.m4a");
        std::fs::write(&sound_path, b"fake-audio").expect("sound fixture");

        let scene = runtime_scene_with_objects(
            vec![(
                3,
                sound_object(3, "Ambient", sound_path.display().to_string()),
            )],
            vec![3],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.sounds.len(), 1);
        assert!(report.plan.has_renderable_output());
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::NoRenderableVisuals));
    }

    #[test]
    fn render_plan_maps_supported_numeric_color_blend_modes_to_native_blends() {
        let temp = tempdir().expect("temp dir");
        let texture_path = temp.path().join("clouds.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255])))
            .save(&texture_path)
            .expect("save texture");

        let scene = runtime_scene_with_objects(
            vec![(
                208,
                visual_object(
                    208,
                    "Clouds",
                    SceneAssetKind::Image,
                    Some(texture_path.display().to_string()),
                    Some([0.0, 0.0, 1920.0, 1080.0]),
                    [1.0, 1.0, 1.0],
                    None,
                    Some(7),
                ),
            )],
            vec![208],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.visuals.len(), 1);
        assert_eq!(
            report.plan.visuals[0].blend_mode,
            SceneRenderBlendMode::Additive
        );
    }
}
