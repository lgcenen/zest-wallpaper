use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use crate::models::{
    EvaluatedAudioState, EvaluatedSceneObject, EvaluatedTextState, SceneAssetKind,
    SceneParticleChildKind, SceneParticleInstanceOverride, SceneParticleKind,
    SceneParticleRendererFamily, SceneParticleRuntime, SceneParticleScheduleMode,
    SceneParticleStageRuntime, SceneRuntimeDocument, SceneTextBehavior, SceneTextLayer,
};

use super::{
    scene_particle_runtime_service::{
        build_authored_particle_runtime_for_resource,
        scene_particle_runtime_uses_input_control_points,
    },
    scene_resource_service::{
        font_reference_looks_like_path, scene_text_font_reference_kind, SceneResourceResolver,
        SceneTextFontCandidates, SceneTextFontReferenceKind,
    },
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
    UnsupportedParticleRuntime,
    ParticleNoRenderableOutput,
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
    pub uv_rect: [f32; 4],
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
    pub dynamic_input_generation: Option<u64>,
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
    pub schedule_mode: SceneParticleScheduleMode,
    pub spawn_origin: [f64; 2],
    pub color: SceneRenderColor,
    pub size: f64,
    pub emission_rate: f64,
    pub max_count: usize,
    pub lifetime_ms: f64,
    pub speed_range: [f64; 2],
    pub instantaneous: bool,
    pub start_time_ms: f64,
    pub sign: f64,
    pub spawn_radius: [f64; 2],
    pub uv_scrolling: [f64; 2],
    pub fade_alpha: f64,
    pub subdivision: u32,
    pub rope_length: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderRopeControlPointItem {
    pub id: u32,
    pub position: [f64; 2],
    pub lock_to_pointer: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderRopeParticleItem {
    pub object_id: u32,
    pub object_name: String,
    pub renderer_family: SceneParticleRendererFamily,
    pub schedule_mode: SceneParticleScheduleMode,
    pub control_points: Vec<SceneRenderRopeControlPointItem>,
    pub emission_rate: f64,
    pub segment_count: u32,
    pub subdivision: u32,
    pub length: f64,
    pub min_length: f64,
    pub max_length: f64,
    pub width: f64,
    pub lifetime_ms: f64,
    pub color: SceneRenderColor,
    pub material_path: Option<String>,
    #[cfg_attr(test, allow(dead_code))]
    pub texture_path: Option<PathBuf>,
    pub blend_mode: SceneRenderBlendMode,
    pub uv_scrolling: [f64; 2],
    pub fade_alpha: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpriteParticleFrame {
    pub uv_rect: [f32; 4],
    pub aspect_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSpriteParticleOscillationConfig {
    pub amplitude_range: [f64; 2],
    pub frequency_range: [f64; 2],
    pub phase_range: [f64; 2],
    pub axis_scale: [f64; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderSpriteControlPointItem {
    pub id: u32,
    pub position: [f64; 2],
    pub lock_to_pointer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSpriteParticleSequenceControlPointConfig {
    pub count: usize,
    pub speed_range: [[f64; 2]; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSpriteParticleAttractorConfig {
    pub origin_offset: [f64; 2],
    pub scale: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSpriteParticleVortexConfig {
    pub origin_offset: [f64; 2],
    pub distance_inner: f64,
    pub distance_outer: f64,
    pub speed_inner: f64,
    pub speed_outer: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpriteParticleConfig {
    pub texture_path: PathBuf,
    pub texture_frames: Vec<SceneSpriteParticleFrame>,
    pub blend_mode: SceneRenderBlendMode,
    pub spawn_origin: [f64; 2],
    pub spawn_radius: [f64; 2],
    pub directions: Vec<[f64; 2]>,
    pub sign: f64,
    pub orientation: f64,
    pub velocity_range: Option<[[f64; 2]; 2]>,
    pub color_min: SceneRenderColor,
    pub color_max: SceneRenderColor,
    pub alpha_range: [f64; 2],
    pub size_range: [f64; 2],
    pub lifetime_ms_range: [f64; 2],
    pub speed_range: [f64; 2],
    pub rotation_range: [f64; 2],
    pub angular_velocity_range: [f64; 2],
    pub turbulence: f64,
    pub size_change: Option<[f64; 2]>,
    pub position_oscillation: Option<SceneSpriteParticleOscillationConfig>,
    pub alpha_oscillation: Option<SceneSpriteParticleOscillationConfig>,
    pub size_oscillation: Option<SceneSpriteParticleOscillationConfig>,
    pub gravity: [f64; 2],
    pub drag: f64,
    pub fade_in_ms: f64,
    pub fade_out_ms: f64,
    pub emission_rate: f64,
    pub max_count: usize,
    pub start_time_ms: f64,
    pub instantaneous: bool,
    pub sequence_multiplier: f64,
    pub control_points: Vec<SceneRenderSpriteControlPointItem>,
    pub sequence_control_point: Option<SceneSpriteParticleSequenceControlPointConfig>,
    pub attractors: Vec<SceneSpriteParticleAttractorConfig>,
    pub vortexes: Vec<SceneSpriteParticleVortexConfig>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpriteParticleChildItem {
    pub child_type: SceneParticleChildKind,
    pub config: SceneSpriteParticleConfig,
    pub probability: f64,
    pub control_point_start_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderSpriteParticleItem {
    pub object_id: u32,
    pub object_name: String,
    pub schedule_mode: SceneParticleScheduleMode,
    pub config: SceneSpriteParticleConfig,
    pub children: Vec<SceneSpriteParticleChildItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderSoundItem {
    pub object_id: u32,
    pub object_name: String,
    pub asset_path: PathBuf,
    pub looped: bool,
    pub volume: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SceneRenderDrawKind {
    Visual,
    Text,
    Audio,
    Particle,
    RopeParticle,
    SpriteParticle,
    Sound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneRenderDrawItem {
    pub object_id: u32,
    pub kind: SceneRenderDrawKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderPlan {
    pub clear_color: SceneClearColor,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub camera: SceneRenderCamera,
    pub draw_order: Vec<SceneRenderDrawItem>,
    pub visuals: Vec<SceneRenderVisualItem>,
    pub texts: Vec<SceneRenderTextItem>,
    pub audios: Vec<SceneRenderAudioItem>,
    pub particles: Vec<SceneRenderParticleItem>,
    pub rope_particles: Vec<SceneRenderRopeParticleItem>,
    pub sprite_particles: Vec<SceneRenderSpriteParticleItem>,
    pub sounds: Vec<SceneRenderSoundItem>,
}

impl SceneRenderPlan {
    pub fn has_renderable_output(&self) -> bool {
        !self.visuals.is_empty()
            || !self.texts.is_empty()
            || !self.audios.is_empty()
            || !self.particles.is_empty()
            || !self.rope_particles.is_empty()
            || !self.sprite_particles.is_empty()
            || !self.sounds.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderPlanReport {
    pub plan: SceneRenderPlan,
    pub issues: Vec<SceneRenderIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderTextUpdateReport {
    pub texts: Vec<SceneRenderTextItem>,
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
    let mut rope_particles = Vec::new();
    let mut sprite_particles = Vec::new();
    let mut sounds = Vec::new();
    let mut draw_order = Vec::new();
    let mut issues = Vec::new();
    let source_text_layers = scene
        .source
        .text_layers
        .iter()
        .map(|layer| (layer.id, layer))
        .collect::<BTreeMap<_, _>>();
    let source_particle_runtimes = scene
        .source
        .particle_runtimes
        .iter()
        .map(|runtime| (runtime.object_id, runtime))
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

                let uv_rect = visual_texture_uv_rect(resolver, &texture_path, source_kind);
                visuals.push(SceneRenderVisualItem {
                    object_id: base.id,
                    object_name: base.name.clone(),
                    texture_path,
                    source_kind,
                    quad: quad_from_bounds(bounds, base.transform.rotation, base.opacity, base),
                    blend_mode: parse_visual_blend_mode(blend_mode.as_deref(), *color_blend_mode),
                    uv_rect,
                });
                draw_order.push(SceneRenderDrawItem {
                    object_id: base.id,
                    kind: SceneRenderDrawKind::Visual,
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
                draw_order.push(SceneRenderDrawItem {
                    object_id: base.id,
                    kind: SceneRenderDrawKind::Text,
                });
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
                draw_order.push(SceneRenderDrawItem {
                    object_id: base.id,
                    kind: SceneRenderDrawKind::Audio,
                });
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
                let runtime = source_particle_runtimes.get(&base.id).copied();

                if runtime_prefers_first_class_rope_particles(runtime) {
                    let Some(rope_item) = plan_rope_particle_item(
                        base,
                        color.as_deref(),
                        runtime,
                        resolver,
                        &mut issues,
                    ) else {
                        continue;
                    };
                    rope_particles.push(rope_item);
                    draw_order.push(SceneRenderDrawItem {
                        object_id: base.id,
                        kind: SceneRenderDrawKind::RopeParticle,
                    });
                    continue;
                }

                if runtime_prefers_first_class_sprite_particles(runtime) {
                    let Some(sprite_item) =
                        plan_sprite_particle_item(base, runtime, resolver, &mut issues)
                    else {
                        continue;
                    };
                    sprite_particles.push(sprite_item);
                    draw_order.push(SceneRenderDrawItem {
                        object_id: base.id,
                        kind: SceneRenderDrawKind::SpriteParticle,
                    });
                    continue;
                }

                let Some(particle_item) = plan_particle_item(
                    base,
                    particle_kind,
                    color.as_deref(),
                    *size,
                    *emission_rate,
                    runtime,
                    &mut issues,
                ) else {
                    continue;
                };
                particles.push(particle_item);
                draw_order.push(SceneRenderDrawItem {
                    object_id: base.id,
                    kind: SceneRenderDrawKind::Particle,
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
                draw_order.push(SceneRenderDrawItem {
                    object_id: base.id,
                    kind: SceneRenderDrawKind::Sound,
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
            parallax_mouse_influence: if scene.evaluated.parallax.enabled {
                scene.evaluated.camera.parallax_mouse_influence.max(0.0)
            } else {
                0.0
            },
        },
        draw_order,
        visuals,
        texts,
        audios,
        particles,
        rope_particles,
        sprite_particles,
        sounds,
    };

    if !plan.has_renderable_output() {
        push_unique_issue(&mut issues, SceneRenderIssue::no_renderable_visuals());
    }

    SceneRenderPlanReport { plan, issues }
}

fn runtime_prefers_first_class_sprite_particles(runtime: Option<&SceneParticleRuntime>) -> bool {
    matches!(
        runtime
            .and_then(|runtime| runtime.system.renderers.first())
            .map(|renderer| renderer.family),
        Some(SceneParticleRendererFamily::Sprite | SceneParticleRendererFamily::SpriteTrail)
    )
}

fn runtime_prefers_first_class_rope_particles(runtime: Option<&SceneParticleRuntime>) -> bool {
    runtime
        .and_then(|runtime| runtime.adapter.rope_contract.as_ref())
        .is_some()
}

pub fn build_scene_render_text_update_with_resolver(
    scene: &SceneRuntimeDocument,
    resolver: Option<&SceneResourceResolver>,
) -> SceneRenderTextUpdateReport {
    let mut texts = Vec::new();
    let mut issues = Vec::new();
    let source_text_layers = scene
        .source
        .text_layers
        .iter()
        .map(|layer| (layer.id, layer))
        .collect::<BTreeMap<_, _>>();

    for object_id in &scene.evaluated.render_list {
        let Some(EvaluatedSceneObject::Text {
            base,
            behavior,
            text,
        }) = scene.evaluated.objects.get(object_id)
        else {
            continue;
        };

        if !base.visible || base.opacity <= 0.001 {
            continue;
        }

        if let Some(text_item) = plan_text_item(
            base.id,
            &base.name,
            base,
            behavior,
            text,
            source_text_layers.get(&base.id).copied(),
            resolver,
            &mut issues,
        ) {
            texts.push(text_item);
        }
    }

    SceneRenderTextUpdateReport { texts, issues }
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

    fn unsupported_particle_runtime(
        object_id: u32,
        object_name: &str,
        detail: Option<String>,
    ) -> Self {
        Self {
            severity: SceneRenderIssueSeverity::Warning,
            code: SceneRenderIssueCode::UnsupportedParticleRuntime,
            message: format!(
                "{} authored particle runtime is outside the phase-09e adapter whitelist.",
                quoted(object_name)
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            object_kind: Some("particle".to_string()),
            resource_path: None,
            detail,
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

    fn particle_no_renderable_output(object_id: u32, object_name: &str, detail: &str) -> Self {
        Self {
            severity: SceneRenderIssueSeverity::Fatal,
            code: SceneRenderIssueCode::ParticleNoRenderableOutput,
            message: format!(
                "{} did not produce drawable rope/particle output in the native runtime.",
                quoted(object_name)
            ),
            object_id: Some(object_id),
            object_name: Some(object_name.to_string()),
            object_kind: Some("particle".to_string()),
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
        dynamic_input_generation: text.dynamic_input_generation,
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

fn plan_particle_item(
    base: &crate::models::EvaluatedSceneObjectBase,
    particle_kind: &SceneParticleKind,
    color: Option<&str>,
    size: f64,
    emission_rate: f64,
    runtime: Option<&SceneParticleRuntime>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneRenderParticleItem> {
    let Some(runtime) = runtime else {
        return Some(SceneRenderParticleItem {
            object_id: base.id,
            object_name: base.name.clone(),
            particle_kind: particle_kind.clone(),
            schedule_mode: SceneParticleScheduleMode::InputDriven,
            spawn_origin: [base.transform.position[0], base.transform.position[1]],
            color: parse_scene_color(color, particle_alpha(particle_kind.clone())),
            size: size.max(0.1),
            emission_rate: emission_rate.max(0.0),
            max_count: default_particle_max_count(particle_kind),
            lifetime_ms: default_particle_lifetime_ms(particle_kind),
            speed_range: default_particle_speed_range(particle_kind),
            instantaneous: false,
            start_time_ms: 0.0,
            sign: 1.0,
            spawn_radius: [0.0, 0.0],
            uv_scrolling: [0.0, 0.0],
            fade_alpha: 0.0,
            subdivision: 1,
            rope_length: 0.0,
        });
    };

    if !runtime.adapter.supported {
        let detail = runtime.adapter.reason.clone().or_else(|| {
            runtime
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
        });
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(base.id, &base.name, detail),
        );
        return None;
    }

    let draw_kind = runtime
        .adapter
        .draw_kind
        .clone()
        .unwrap_or_else(|| particle_kind.clone());
    let emitter = runtime.system.emitters.first();
    let emitter_origin = emitter
        .and_then(|emitter| emitter.origin)
        .unwrap_or([0.0, 0.0, 0.0]);
    let spawn_origin = [
        base.transform.position[0] + emitter_origin[0] * base.transform.scale[0],
        base.transform.position[1] + emitter_origin[1] * base.transform.scale[1],
    ];
    let schedule_mode = if scene_particle_runtime_uses_input_control_points(runtime) {
        SceneParticleScheduleMode::InputDriven
    } else {
        runtime.adapter.schedule_mode
    };
    let alpha = runtime
        .instance_override
        .alpha
        .unwrap_or_else(|| particle_alpha(draw_kind.clone()));
    let color = runtime.instance_override.color.as_deref().or(color);
    let default_speed_range = default_particle_speed_range(&draw_kind);
    let speed_range = runtime
        .instance_override
        .speed
        .map(|speed| [speed.max(0.0), speed.max(0.0)])
        .or_else(|| {
            emitter.map(|emitter| {
                let min = emitter.speed_min.unwrap_or(default_speed_range[0]).max(0.0);
                let max = emitter.speed_max.unwrap_or(default_speed_range[1]).max(min);
                [min, max]
            })
        })
        .unwrap_or(default_speed_range);

    Some(SceneRenderParticleItem {
        object_id: base.id,
        object_name: base.name.clone(),
        particle_kind: draw_kind.clone(),
        schedule_mode,
        spawn_origin,
        color: parse_scene_color(color, alpha),
        size: runtime.instance_override.size.unwrap_or(size).max(0.1),
        emission_rate: runtime
            .instance_override
            .rate
            .or_else(|| emitter.and_then(|emitter| emitter.rate))
            .unwrap_or(emission_rate)
            .max(0.0),
        max_count: runtime
            .instance_override
            .count
            .map(|value| value.round() as usize)
            .or_else(|| runtime.system.max_count.map(|value| value as usize))
            .unwrap_or_else(|| default_particle_max_count(&draw_kind))
            .clamp(1, 4096),
        lifetime_ms: runtime
            .instance_override
            .lifetime
            .map(normalize_particle_lifetime_ms)
            .unwrap_or_else(|| default_particle_lifetime_ms(&draw_kind)),
        speed_range,
        instantaneous: emitter
            .map(|emitter| emitter.instantaneous)
            .unwrap_or(false),
        start_time_ms: runtime.system.start_time.unwrap_or(0.0).max(0.0),
        sign: emitter.and_then(|emitter| emitter.sign).unwrap_or(1.0),
        spawn_radius: emitter
            .map(|emitter| {
                let min = emitter.distance_min.unwrap_or(0.0).max(0.0);
                let max = emitter.distance_max.unwrap_or(min).max(min);
                [min, max]
            })
            .unwrap_or([0.0, 0.0]),
        uv_scrolling: runtime
            .system
            .renderers
            .first()
            .and_then(|renderer| renderer.uv_scrolling)
            .unwrap_or([0.0, 0.0]),
        fade_alpha: runtime
            .system
            .renderers
            .first()
            .and_then(|renderer| renderer.fade_alpha)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0),
        subdivision: runtime
            .system
            .renderers
            .first()
            .and_then(|renderer| renderer.subdivision)
            .unwrap_or(1)
            .clamp(1, 64),
        rope_length: runtime
            .system
            .renderers
            .first()
            .and_then(|renderer| renderer.length)
            .unwrap_or(0.0)
            .max(0.0),
    })
}

fn plan_rope_particle_item(
    base: &crate::models::EvaluatedSceneObjectBase,
    color: Option<&str>,
    runtime: Option<&SceneParticleRuntime>,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneRenderRopeParticleItem> {
    let runtime = runtime?;
    if !runtime.adapter.supported {
        let detail = runtime.adapter.reason.clone().or_else(|| {
            runtime
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
        });
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(base.id, &base.name, detail),
        );
        return None;
    }

    let Some(contract) = runtime.adapter.rope_contract.as_ref() else {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                base.id,
                &base.name,
                Some("Particle runtime did not expose a rope contract.".to_string()),
            ),
        );
        return None;
    };

    let control_points = resolve_rope_control_points(base, runtime, contract);
    if control_points.len() < 2 || contract.segment_count == 0 || contract.width <= 0.0 {
        push_unique_issue(
            issues,
            SceneRenderIssue::particle_no_renderable_output(
                base.id,
                &base.name,
                "Rope runtime did not produce enough control-point topology for drawable segments.",
            ),
        );
        return None;
    }

    let material =
        match resolve_rope_particle_material(base.id, &base.name, runtime, resolver, issues) {
            RopeParticleMaterialResolution::Supported(material) => material,
            RopeParticleMaterialResolution::Unsupported => return None,
        };

    Some(SceneRenderRopeParticleItem {
        object_id: base.id,
        object_name: base.name.clone(),
        renderer_family: contract.renderer_family,
        schedule_mode: contract.schedule_mode,
        control_points,
        emission_rate: contract.emission_rate,
        segment_count: contract.segment_count,
        subdivision: contract.subdivision,
        length: contract.length,
        min_length: contract.min_length,
        max_length: contract.max_length,
        width: contract.width,
        lifetime_ms: contract.lifetime_ms,
        color: parse_scene_color(
            contract
                .color
                .as_deref()
                .or(runtime.instance_override.color.as_deref())
                .or(color),
            contract.alpha,
        ),
        material_path: contract.material_path.clone(),
        texture_path: material
            .as_ref()
            .map(|material| material.texture_path.clone()),
        blend_mode: material
            .as_ref()
            .map(|material| material.blend_mode)
            .unwrap_or(SceneRenderBlendMode::Additive),
        uv_scrolling: contract.uv_scrolling,
        fade_alpha: contract.fade_alpha,
    })
}

fn resolve_rope_control_points(
    base: &crate::models::EvaluatedSceneObjectBase,
    runtime: &SceneParticleRuntime,
    contract: &crate::models::SceneRopeParticleRuntimeContract,
) -> Vec<SceneRenderRopeControlPointItem> {
    let parent_rotation = base.transform.rotation;
    let object_rotation =
        parent_rotation + runtime.object_angles.map(|angles| angles[2]).unwrap_or(0.0);
    let object_offset = rotate_2d(
        [
            runtime.object_origin[0] * base.transform.scale[0],
            runtime.object_origin[1] * base.transform.scale[1],
        ],
        parent_rotation,
    );
    let root = [
        base.transform.position[0] + object_offset[0],
        base.transform.position[1] + object_offset[1],
    ];

    let mut resolved = Vec::with_capacity(contract.control_points.len());
    for control_point in &contract.control_points {
        let authored = control_point.override_value.unwrap_or(control_point.offset);
        let local = rotate_2d(
            [
                authored[0] * runtime.object_scale[0] * base.transform.scale[0],
                authored[1] * runtime.object_scale[1] * base.transform.scale[1],
            ],
            object_rotation,
        );
        let parent_position = control_point
            .parent_control_point
            .and_then(|parent_id| {
                resolved
                    .iter()
                    .find(|candidate: &&SceneRenderRopeControlPointItem| candidate.id == parent_id)
                    .map(|candidate| candidate.position)
            })
            .unwrap_or(root);
        resolved.push(SceneRenderRopeControlPointItem {
            id: control_point.id,
            position: [parent_position[0] + local[0], parent_position[1] + local[1]],
            lock_to_pointer: control_point.lock_to_pointer
                || control_point.override_binding.is_some(),
        });
    }

    resolved
}

fn plan_sprite_particle_item(
    base: &crate::models::EvaluatedSceneObjectBase,
    runtime: Option<&SceneParticleRuntime>,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneRenderSpriteParticleItem> {
    let runtime = runtime?;
    if !runtime.adapter.supported {
        let detail = runtime.adapter.reason.clone().or_else(|| {
            runtime
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
        });
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(base.id, &base.name, detail),
        );
        return None;
    }

    let config = plan_sprite_particle_config(base.id, &base.name, base, runtime, resolver, issues)?;
    let children = runtime
        .system
        .children
        .iter()
        .filter_map(|child| plan_sprite_particle_child(base, runtime, child, resolver, issues))
        .collect::<Vec<_>>();

    Some(SceneRenderSpriteParticleItem {
        object_id: base.id,
        object_name: base.name.clone(),
        schedule_mode: if scene_particle_runtime_uses_input_control_points(runtime) {
            SceneParticleScheduleMode::InputDriven
        } else {
            runtime.adapter.schedule_mode
        },
        config,
        children,
    })
}

fn plan_sprite_particle_child(
    base: &crate::models::EvaluatedSceneObjectBase,
    parent: &SceneParticleRuntime,
    child: &crate::models::SceneParticleChildRuntime,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneSpriteParticleChildItem> {
    if matches!(
        child.child_type,
        SceneParticleChildKind::EventSpawn | SceneParticleChildKind::Unsupported
    ) {
        return None;
    }

    let mut child_runtime = if let (Some(resolver), Some(path)) = (
        resolver,
        child
            .name
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty()),
    ) {
        match load_particle_runtime_from_resource(path, resolver) {
            Ok(runtime) => runtime,
            Err(error) => {
                push_unique_issue(
                    issues,
                    SceneRenderIssue::unsupported_particle_runtime(
                        base.id,
                        &base.name,
                        Some(error),
                    ),
                );
                return None;
            }
        }
    } else {
        parent.clone()
    };

    child_runtime.object_origin = [
        parent.object_origin[0] + child.origin.unwrap_or([0.0, 0.0, 0.0])[0],
        parent.object_origin[1] + child.origin.unwrap_or([0.0, 0.0, 0.0])[1],
        parent.object_origin[2] + child.origin.unwrap_or([0.0, 0.0, 0.0])[2],
    ];
    let child_scale = child.scale.unwrap_or([1.0, 1.0, 1.0]);
    child_runtime.object_scale = [
        parent.object_scale[0] * child_scale[0],
        parent.object_scale[1] * child_scale[1],
        parent.object_scale[2] * child_scale[2],
    ];
    child_runtime.object_angles = child
        .angles
        .map(|child_angles| {
            let parent_angles = parent.object_angles.unwrap_or([0.0, 0.0, 0.0]);
            [
                parent_angles[0] + child_angles[0],
                parent_angles[1] + child_angles[1],
                parent_angles[2] + child_angles[2],
            ]
        })
        .or(parent.object_angles);
    if let Some(max_count) = child.max_count {
        child_runtime.system.max_count = Some(max_count);
    }
    child_runtime.instance_override = inherit_particle_instance_override(
        &parent.instance_override,
        &child_runtime.instance_override,
    );

    if !child_runtime.adapter.supported
        || child_runtime
            .system
            .renderers
            .first()
            .map(|renderer| renderer.family)
            != Some(SceneParticleRendererFamily::Sprite)
    {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                base.id,
                &base.name,
                Some(
                    "Sprite child runtime is outside the phase-09e2 first-class sprite boundary."
                        .to_string(),
                ),
            ),
        );
        return None;
    }

    if let Some(cp_start) = child.control_point_start_index {
        let cp_index = cp_start as usize;
        if cp_index < parent.system.control_points.len() {
            let cp_offset = parent.system.control_points[cp_index]
                .offset
                .unwrap_or([0.0, 0.0, 0.0]);
            child_runtime.object_origin = [
                child_runtime.object_origin[0] + cp_offset[0],
                child_runtime.object_origin[1] + cp_offset[1],
                child_runtime.object_origin[2] + cp_offset[2],
            ];
        }
    }

    let config =
        plan_sprite_particle_config(base.id, &base.name, base, &child_runtime, resolver, issues)?;
    Some(SceneSpriteParticleChildItem {
        child_type: child.child_type,
        config,
        probability: child.probability.unwrap_or(1.0).clamp(0.0, 1.0),
        control_point_start_index: child.control_point_start_index,
    })
}

fn plan_sprite_particle_config(
    object_id: u32,
    object_name: &str,
    base: &crate::models::EvaluatedSceneObjectBase,
    runtime: &SceneParticleRuntime,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SceneSpriteParticleConfig> {
    let material =
        resolve_sprite_particle_material(object_id, object_name, runtime, resolver, issues)?;
    let emitter = runtime.system.emitters.first();
    let emitter_origin = emitter
        .and_then(|emitter| emitter.origin)
        .unwrap_or([0.0, 0.0, 0.0]);
    let scale_x = (base.transform.scale[0] * runtime.object_scale[0])
        .abs()
        .max(0.001);
    let scale_y = (base.transform.scale[1] * runtime.object_scale[1])
        .abs()
        .max(0.001);
    let motion_scale = scale_x.min(scale_y);
    let parent_rotation = base.transform.rotation;
    let object_rotation =
        parent_rotation + runtime.object_angles.map(|angles| angles[2]).unwrap_or(0.0);
    let object_offset = rotate_2d(
        [
            runtime.object_origin[0] * base.transform.scale[0],
            runtime.object_origin[1] * base.transform.scale[1],
        ],
        parent_rotation,
    );
    let emitter_offset = rotate_2d(
        [emitter_origin[0] * scale_x, emitter_origin[1] * scale_y],
        object_rotation,
    );
    let origin = [
        base.transform.position[0] + object_offset[0] + emitter_offset[0],
        base.transform.position[1] + object_offset[1] + emitter_offset[1],
    ];
    let control_points =
        resolve_sprite_control_points(base, runtime, scale_x, scale_y, object_rotation);
    let authored_size = stage_range(
        &runtime.system.initializers,
        &["sizerandom", "size"],
        &["sizemin", "minsize", "min"],
        &["sizemax", "maxsize", "max"],
        [24.0, 42.0],
    );
    let size_scale = runtime.instance_override.size.unwrap_or(1.0).max(0.01);
    let size_range = [
        authored_size[0].max(0.5) * size_scale,
        authored_size[1].max(authored_size[0]).max(0.5) * size_scale,
    ];
    let authored_lifetime = stage_range(
        &runtime.system.initializers,
        &["lifetimerandom", "lifetime", "life"],
        &["lifemin", "lifetimemin", "minlife", "min"],
        &["lifemax", "lifetimemax", "maxlife", "max"],
        [1_200.0, 2_400.0],
    );
    let authored_lifetime_ms = [
        normalize_particle_lifetime_ms(authored_lifetime[0]),
        normalize_particle_lifetime_ms(authored_lifetime[1].max(authored_lifetime[0])),
    ];
    let lifetime_range = runtime
        .instance_override
        .lifetime
        .map(|value| sprite_override_range(authored_lifetime_ms, value, true))
        .unwrap_or(authored_lifetime_ms);
    let emitter_speed = emitter
        .map(|emitter| {
            let min = emitter.speed_min.unwrap_or(0.0).max(0.0);
            let max = emitter.speed_max.unwrap_or(min).max(min);
            [min, max]
        })
        .unwrap_or([0.0, 18.0]);
    let speed_random = stage_range_with_vector_magnitude(
        &runtime.system.initializers,
        &["velocityrandom", "speed"],
        &["speedmin", "minspeed", "min"],
        &["speedmax", "maxspeed", "max"],
        emitter_speed,
    );
    let speed_override_multiplier =
        sprite_fractional_override_multiplier(runtime.instance_override.speed);
    let velocity_range = stage_vector_range(
        &runtime.system.initializers,
        &["velocityrandom"],
        &["velocitymin", "minvelocity", "min"],
        &["velocitymax", "maxvelocity", "max"],
    )
    .map(|range| {
        [
            [
                range[0][0] * scale_x * speed_override_multiplier,
                range[0][1] * scale_y * speed_override_multiplier,
            ],
            [
                range[1][0] * scale_x * speed_override_multiplier,
                range[1][1] * scale_y * speed_override_multiplier,
            ],
        ]
    });
    let color_range = stage_color_range(
        &runtime.system.initializers,
        runtime
            .instance_override
            .color
            .as_deref()
            .map(|color| parse_scene_color(Some(color), 1.0))
            .unwrap_or_else(|| parse_scene_color(None, 1.0)),
    );
    let alpha_override = runtime
        .instance_override
        .alpha
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let alpha_range = stage_range(
        &runtime.system.initializers,
        &["alpharandom", "alpha"],
        &["alphamin", "minalpha", "min"],
        &["alphamax", "maxalpha", "max"],
        [1.0, 1.0],
    );
    let rotation_range = stage_range(
        &runtime.system.initializers,
        &["rotationrandom", "rotation"],
        &["rotationmin", "minrotation", "min"],
        &["rotationmax", "maxrotation", "max"],
        [0.0, 360.0],
    );
    let angular_velocity_range = stage_vector_component_range(
        &runtime.system.initializers,
        &["angularvelocityrandom"],
        &["angularvelocitymin", "min"],
        &["angularvelocitymax", "max"],
        2,
    )
    .map(|range| [range[0].to_degrees(), range[1].to_degrees()])
    .unwrap_or_else(|| {
        stage_range(
            &runtime.system.initializers,
            &["angularvelocityrandom", "angular", "spin"],
            &["speedmin", "minspeed", "angularvelocitymin", "min"],
            &["speedmax", "maxspeed", "angularvelocitymax", "max"],
            [0.0, 0.0],
        )
    });
    let movement_speed = stage_range(
        &runtime.system.operators,
        &["movement"],
        &["speedmin", "minspeed", "min"],
        &["speedmax", "maxspeed", "max"],
        speed_random,
    );
    let authored_speed_range = [
        movement_speed[0].max(0.0) * motion_scale,
        movement_speed[1].max(movement_speed[0]).max(0.0) * motion_scale,
    ];
    let speed_range = runtime
        .instance_override
        .speed
        .map(|value| sprite_override_range(authored_speed_range, value, false))
        .unwrap_or(authored_speed_range);
    let turbulence = sprite_turbulence(&runtime.system.initializers, &runtime.system.operators)
        * speed_override_multiplier;
    let size_change = stage_optional_range(
        &runtime.system.operators,
        &["sizechange"],
        &["startvalue", "start", "from", "min"],
        &["endvalue", "end", "to", "max"],
    );
    let position_oscillation =
        sprite_position_oscillation(&runtime.system.operators, scale_x, scale_y);
    let alpha_oscillation = sprite_scalar_oscillation(
        &runtime.system.operators,
        &["oscillatealpha", "oscillate alpha", "oscillate_alpha"],
    );
    let size_oscillation = sprite_scalar_oscillation(
        &runtime.system.operators,
        &["oscillatesize", "oscillate size", "oscillate_size"],
    );
    let gravity = find_stage(&runtime.system.operators, &["movement"])
        .and_then(|stage| stage_vector2(stage, &["gravity"]))
        .map(|value| [value[0] * scale_x, value[1] * scale_y])
        .unwrap_or([0.0, 0.0]);
    let drag = find_stage(&runtime.system.operators, &["movement"])
        .and_then(|stage| stage_f64(stage, &["drag"]))
        .unwrap_or(0.0)
        .max(0.0);
    let sequence_control_point =
        sprite_sequence_control_point_config(&runtime.system.initializers, scale_x, scale_y);
    let attractors =
        sprite_attractor_configs(&runtime.system.operators, scale_x, scale_y, object_rotation);
    let vortexes =
        sprite_vortex_configs(&runtime.system.operators, scale_x, scale_y, object_rotation);
    let [fade_in_ms, fade_out_ms] = stage_range(
        &runtime.system.operators,
        &["alphafade"],
        &["fadeintime", "fadein", "in"],
        &["fadeouttime", "fadeout", "out"],
        [0.0, 0.0],
    )
    .map(normalize_particle_lifetime_ms);

    Some(SceneSpriteParticleConfig {
        texture_path: material.texture_path,
        texture_frames: material.texture_frames,
        blend_mode: material.blend_mode,
        spawn_origin: origin,
        spawn_radius: emitter
            .map(|emitter| {
                [
                    emitter.distance_min.unwrap_or(0.0).abs() * scale_x,
                    emitter.distance_max.unwrap_or(0.0).abs() * scale_y,
                ]
            })
            .unwrap_or([0.0, 0.0]),
        directions: emitter
            .map(|emitter| {
                emitter
                    .directions
                    .iter()
                    .map(|direction| [direction[0], direction[1]])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        sign: emitter.and_then(|emitter| emitter.sign).unwrap_or(1.0),
        orientation: object_rotation,
        velocity_range,
        color_min: SceneRenderColor {
            alpha: normalize_color_alpha(color_range[0], alpha_range[0] * alpha_override),
            ..color_range[0]
        },
        color_max: SceneRenderColor {
            alpha: normalize_color_alpha(color_range[1], alpha_range[1] * alpha_override),
            ..color_range[1]
        },
        alpha_range: [
            (alpha_range[0] * alpha_override).clamp(0.0, 1.0),
            (alpha_range[1] * alpha_override).clamp(0.0, 1.0),
        ],
        size_range,
        lifetime_ms_range: lifetime_range,
        speed_range,
        rotation_range,
        angular_velocity_range,
        turbulence,
        size_change,
        position_oscillation,
        alpha_oscillation,
        size_oscillation,
        gravity,
        drag,
        fade_in_ms,
        fade_out_ms,
        emission_rate: sprite_override_scalar(
            emitter.and_then(|emitter| emitter.rate).unwrap_or(0.0),
            runtime.instance_override.rate,
        )
        .max(0.0),
        max_count: sprite_override_count(
            runtime.system.max_count.unwrap_or(128) as f64,
            runtime.instance_override.count,
        )
        .clamp(1, 8192),
        start_time_ms: runtime
            .system
            .start_time
            .map(normalize_particle_lifetime_ms)
            .unwrap_or(0.0)
            .max(0.0),
        instantaneous: emitter
            .map(|emitter| emitter.instantaneous)
            .unwrap_or(false),
        sequence_multiplier: runtime.system.sequence_multiplier.unwrap_or(0.0).max(0.0),
        control_points,
        sequence_control_point,
        attractors,
        vortexes,
    })
}

fn resolve_sprite_control_points(
    base: &crate::models::EvaluatedSceneObjectBase,
    runtime: &SceneParticleRuntime,
    scale_x: f64,
    scale_y: f64,
    object_rotation: f64,
) -> Vec<SceneRenderSpriteControlPointItem> {
    let parent_rotation = base.transform.rotation;
    let object_offset = rotate_2d(
        [
            runtime.object_origin[0] * base.transform.scale[0],
            runtime.object_origin[1] * base.transform.scale[1],
        ],
        parent_rotation,
    );
    let root = [
        base.transform.position[0] + object_offset[0],
        base.transform.position[1] + object_offset[1],
    ];

    let mut resolved = Vec::with_capacity(runtime.system.control_points.len());
    for (index, control_point) in runtime.system.control_points.iter().enumerate() {
        let authored = control_point.offset.unwrap_or([0.0, 0.0, 0.0]);
        let local = rotate_2d([authored[0] * scale_x, authored[1] * scale_y], object_rotation);
        let parent_position = control_point
            .parent_control_point
            .and_then(|parent_id| {
                resolved
                    .iter()
                    .find(|candidate: &&SceneRenderSpriteControlPointItem| candidate.id == parent_id)
                    .map(|candidate| candidate.position)
            })
            .unwrap_or(root);
        resolved.push(SceneRenderSpriteControlPointItem {
            id: control_point.id.unwrap_or(index as u32),
            position: [parent_position[0] + local[0], parent_position[1] + local[1]],
            lock_to_pointer: control_point.lock_to_pointer
                || control_point_flags_lock_to_pointer(&control_point.flags),
        });
    }

    resolved
}

fn sprite_sequence_control_point_config(
    stages: &[SceneParticleStageRuntime],
    scale_x: f64,
    scale_y: f64,
) -> Option<SceneSpriteParticleSequenceControlPointConfig> {
    let stage = find_stage(stages, &["mapsequencearoundcontrolpoint"])?;
    let count = stage_f64(stage, &["count"])
        .map(|value| value.round().clamp(1.0, 64.0) as usize)
        .unwrap_or(0);
    if count == 0 {
        return None;
    }
    let speed_range = stage_vector_range(
        stages,
        &["mapsequencearoundcontrolpoint"],
        &["speedmin", "minspeed", "min"],
        &["speedmax", "maxspeed", "max"],
    )
    .map(|range| {
        [
            [range[0][0] * scale_x, range[0][1] * scale_y],
            [range[1][0] * scale_x, range[1][1] * scale_y],
        ]
    })
    .unwrap_or([[0.0, 0.0], [0.0, 0.0]]);
    Some(SceneSpriteParticleSequenceControlPointConfig { count, speed_range })
}

fn sprite_attractor_configs(
    stages: &[SceneParticleStageRuntime],
    scale_x: f64,
    scale_y: f64,
    object_rotation: f64,
) -> Vec<SceneSpriteParticleAttractorConfig> {
    sprite_stage_instances(stages, &["controlpointattract"])
        .into_iter()
        .map(|stage| {
            let origin = stage_vector2(stage, &["origin"]).unwrap_or([0.0, 0.0]);
            let origin_offset =
                rotate_2d([origin[0] * scale_x, origin[1] * scale_y], object_rotation);
            SceneSpriteParticleAttractorConfig {
                origin_offset,
                scale: stage_f64(stage, &["scale"]).unwrap_or(0.0),
                threshold: stage_f64(stage, &["threshold"]).unwrap_or(0.0).abs(),
            }
        })
        .collect()
}

fn sprite_vortex_configs(
    stages: &[SceneParticleStageRuntime],
    scale_x: f64,
    scale_y: f64,
    object_rotation: f64,
) -> Vec<SceneSpriteParticleVortexConfig> {
    sprite_stage_instances(stages, &["vortex"])
        .into_iter()
        .map(|stage| {
            let origin = stage_vector2(stage, &["origin"]).unwrap_or([0.0, 0.0]);
            let origin_offset =
                rotate_2d([origin[0] * scale_x, origin[1] * scale_y], object_rotation);
            let distance_inner = stage_f64(stage, &["distanceinner", "inner", "mindistance"])
                .unwrap_or(0.0)
                .abs()
                * scale_x.min(scale_y);
            let distance_outer = stage_f64(stage, &["distanceouter", "outer", "maxdistance"])
                .unwrap_or(distance_inner.max(1.0))
                .abs()
                * scale_x.min(scale_y);
            SceneSpriteParticleVortexConfig {
                origin_offset,
                distance_inner,
                distance_outer: distance_outer.max(distance_inner),
                speed_inner: stage_f64(stage, &["speedinner", "innerspeed"]).unwrap_or(0.0),
                speed_outer: stage_f64(stage, &["speedouter", "outerspeed"]).unwrap_or(0.0),
            }
        })
        .collect()
}

fn sprite_stage_instances<'a>(
    stages: &'a [SceneParticleStageRuntime],
    name_tokens: &[&str],
) -> Vec<&'a SceneParticleStageRuntime> {
    stages
        .iter()
        .filter(|stage| {
            let name = stage.name.to_ascii_lowercase();
            name_tokens
                .iter()
                .any(|token| name.contains(&token.to_ascii_lowercase()))
        })
        .collect()
}

fn control_point_flags_lock_to_pointer(flags: &[String]) -> bool {
    flags.iter().any(|flag| {
        let normalized = flag.trim().to_ascii_lowercase();
        normalized == "locktopointer"
            || normalized == "lock_to_pointer"
            || normalized == "pointer"
            || normalized
                .parse::<u32>()
                .map(|bits| (bits & 1) != 0)
                .unwrap_or(false)
    })
}

struct SpriteParticleMaterial {
    texture_path: PathBuf,
    texture_frames: Vec<SceneSpriteParticleFrame>,
    blend_mode: SceneRenderBlendMode,
}

struct RopeParticleMaterial {
    texture_path: PathBuf,
    blend_mode: SceneRenderBlendMode,
}

enum RopeParticleMaterialResolution {
    Supported(Option<RopeParticleMaterial>),
    Unsupported,
}

fn resolve_sprite_particle_material(
    object_id: u32,
    object_name: &str,
    runtime: &SceneParticleRuntime,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> Option<SpriteParticleMaterial> {
    let Some(material_path) = runtime
        .system
        .material
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_path(
                SceneRenderIssueSeverity::Warning,
                object_id,
                object_name,
                "particle material",
            ),
        );
        return None;
    };
    let Some(resolver) = resolver else {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some(
                    "Sprite particle material resolution requires Scene resource roots."
                        .to_string(),
                ),
            ),
        );
        return None;
    };
    let lookup = resolver.inspect_relative_path(material_path);
    let Some(material_file) = lookup.matched_path else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_file(
                SceneRenderIssueSeverity::Warning,
                object_id,
                object_name,
                "particle material",
                material_path.to_string(),
            ),
        );
        return None;
    };
    let material_json = match read_json_value(&material_file) {
        Ok(value) => value,
        Err(error) => {
            push_unique_issue(
                issues,
                SceneRenderIssue::unsupported_particle_runtime(object_id, object_name, Some(error)),
            );
            return None;
        }
    };
    let passes = particle_material_pass_values(&material_json);
    if passes.len() != 1 {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some("Sprite particle material bridge only supports a single pass.".to_string()),
            ),
        );
        return None;
    }
    if particle_material_requires_phase10(&material_json, passes.first()) {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some("Sprite particle material declares shader/material features reserved for phase-10.".to_string()),
            ),
        );
        return None;
    }
    let pass = &passes[0];
    let shader = pass
        .get("shader")
        .and_then(Value::as_str)
        .or_else(|| material_json.get("shader").and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !shader.contains("genericparticle") && !shader.is_empty() {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some(format!(
                    "Sprite particle material shader {shader:?} is outside the phase-09e2 genericparticle bridge."
                )),
            ),
        );
        return None;
    }
    let textures = pass
        .get("textures")
        .or_else(|| material_json.get("textures"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(texture_name) = textures
        .first()
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_path(
                SceneRenderIssueSeverity::Warning,
                object_id,
                object_name,
                "particle texture",
            ),
        );
        return None;
    };
    let texture_lookup = resolver.inspect_texture_candidates(
        Some(material_path),
        Some(material_file.as_path()),
        texture_name,
    );
    let Some(texture_path) = texture_lookup.matched_paths.first().cloned() else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_file(
                SceneRenderIssueSeverity::Warning,
                object_id,
                object_name,
                "particle texture",
                texture_name.to_string(),
            ),
        );
        return None;
    };

    Some(SpriteParticleMaterial {
        texture_frames: sprite_particle_texture_frames(resolver, &texture_path),
        texture_path,
        blend_mode: parse_particle_material_blend_mode(
            pass.get("blending")
                .or_else(|| material_json.get("blending"))
                .and_then(Value::as_str),
        ),
    })
}

fn resolve_rope_particle_material(
    object_id: u32,
    object_name: &str,
    runtime: &SceneParticleRuntime,
    resolver: Option<&SceneResourceResolver>,
    issues: &mut Vec<SceneRenderIssue>,
) -> RopeParticleMaterialResolution {
    let Some(material_path) = runtime
        .adapter
        .rope_contract
        .as_ref()
        .and_then(|contract| contract.material_path.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return RopeParticleMaterialResolution::Supported(None);
    };
    let Some(resolver) = resolver else {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some(
                    "Rope particle material resolution requires Scene resource roots.".to_string(),
                ),
            ),
        );
        return RopeParticleMaterialResolution::Unsupported;
    };
    let lookup = resolver.inspect_relative_path(material_path);
    let Some(material_file) = lookup.matched_path else {
        push_unique_issue(
            issues,
            SceneRenderIssue::missing_asset_file(
                SceneRenderIssueSeverity::Warning,
                object_id,
                object_name,
                "rope particle material",
                material_path.to_string(),
            ),
        );
        return RopeParticleMaterialResolution::Supported(None);
    };
    let material_json = match read_json_value(&material_file) {
        Ok(value) => value,
        Err(error) => {
            push_unique_issue(
                issues,
                SceneRenderIssue::unsupported_particle_runtime(object_id, object_name, Some(error)),
            );
            return RopeParticleMaterialResolution::Unsupported;
        }
    };
    let passes = particle_material_pass_values(&material_json);
    if passes.len() != 1 {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some("Rope particle material bridge only supports a single pass.".to_string()),
            ),
        );
        return RopeParticleMaterialResolution::Unsupported;
    }
    if particle_material_requires_phase10(&material_json, passes.first()) {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some(
                    "Rope particle material declares shader/material features reserved for phase-10."
                        .to_string(),
                ),
            ),
        );
        return RopeParticleMaterialResolution::Unsupported;
    }
    let pass = &passes[0];
    let shader = pass
        .get("shader")
        .and_then(Value::as_str)
        .or_else(|| material_json.get("shader").and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !(shader.is_empty() || shader.contains("genericparticle") || shader.contains("rope")) {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some(format!(
                    "Rope particle material shader {shader:?} is outside the phase-09f minimal bridge."
                )),
            ),
        );
        return RopeParticleMaterialResolution::Unsupported;
    }
    let textures = pass
        .get("textures")
        .or_else(|| material_json.get("textures"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if textures.len() > 1 {
        push_unique_issue(
            issues,
            SceneRenderIssue::unsupported_particle_runtime(
                object_id,
                object_name,
                Some(
                    "Rope particle material bridge only supports a single base texture."
                        .to_string(),
                ),
            ),
        );
        return RopeParticleMaterialResolution::Unsupported;
    }
    let texture_path = textures
        .first()
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|texture_name| {
            let lookup = resolver.inspect_texture_candidates(
                Some(material_path),
                Some(material_file.as_path()),
                texture_name,
            );
            lookup.matched_paths.first().cloned()
        });

    RopeParticleMaterialResolution::Supported(Some(RopeParticleMaterial {
        texture_path: texture_path.unwrap_or_else(|| PathBuf::from("__rope-white__")),
        blend_mode: parse_particle_material_blend_mode(
            pass.get("blending")
                .or_else(|| material_json.get("blending"))
                .and_then(Value::as_str),
        ),
    }))
}

fn sprite_particle_texture_frames(
    resolver: &SceneResourceResolver,
    texture_path: &Path,
) -> Vec<SceneSpriteParticleFrame> {
    let metadata = resolver.inspect_texture_metadata(texture_path);
    let frames = metadata
        .frames
        .into_iter()
        .map(|frame| SceneSpriteParticleFrame {
            uv_rect: frame.uv_rect,
            aspect_ratio: frame.aspect_ratio,
        })
        .collect::<Vec<_>>();
    if frames.is_empty() {
        vec![full_sprite_particle_frame()]
    } else {
        frames
    }
}

fn full_sprite_particle_frame() -> SceneSpriteParticleFrame {
    SceneSpriteParticleFrame {
        uv_rect: [0.0, 0.0, 1.0, 1.0],
        aspect_ratio: 1.0,
    }
}

fn visual_texture_uv_rect(
    resolver: Option<&SceneResourceResolver>,
    texture_path: &Path,
    source_kind: SceneRenderSourceKind,
) -> [f32; 4] {
    if !matches!(source_kind, SceneRenderSourceKind::Image) {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let Some(resolver) = resolver else {
        return [0.0, 0.0, 1.0, 1.0];
    };
    let metadata = resolver.inspect_texture_metadata(texture_path);
    metadata
        .frames
        .first()
        .map(|frame| frame.uv_rect)
        .unwrap_or([0.0, 0.0, 1.0, 1.0])
}

fn load_particle_runtime_from_resource(
    particle_path: &str,
    resolver: &SceneResourceResolver,
) -> Result<SceneParticleRuntime, String> {
    let lookup = resolver.inspect_relative_path(particle_path);
    let resolved = lookup.matched_path.ok_or_else(|| {
        format!("Sprite particle child resource {particle_path} could not be resolved")
    })?;
    let json = read_json_value(&resolved)?;
    Ok(build_authored_particle_runtime_for_resource(
        particle_path.to_string(),
        &json,
    ))
}

fn read_json_value(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("unable to parse {}: {error}", path.display()))
}

fn particle_material_pass_values(json: &Value) -> Vec<Value> {
    if let Some(passes) = json.get("passes").and_then(Value::as_array) {
        return passes.clone();
    }
    if particle_material_declares_inline_pass(json) {
        return vec![json.clone()];
    }
    Vec::new()
}

fn particle_material_declares_inline_pass(json: &Value) -> bool {
    json.get("shader")
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || json
            .get("textures")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
        || json.get("blending").and_then(Value::as_str).is_some()
}

fn particle_material_requires_phase10(root: &Value, pass: Option<&Value>) -> bool {
    if root
        .get("effects")
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    for scope in pass.into_iter().chain(std::iter::once(root)) {
        if scope
            .get("usertextures")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false)
        {
            return true;
        }
        if scope
            .get("textures")
            .and_then(Value::as_array)
            .map(|items| items.len() > 1)
            .unwrap_or(false)
        {
            return true;
        }
        if scope
            .get("combos")
            .and_then(Value::as_object)
            .map(|items| {
                items.iter().any(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let enabled = value.as_i64().unwrap_or(1) != 0;
                    enabled
                        && (lower.contains("refract")
                            || lower.contains("lighting")
                            || lower.contains("normal")
                            || lower.contains("blur")
                            || lower.contains("cutout"))
                })
            })
            .unwrap_or(false)
        {
            return true;
        }
        for key in ["refract", "lighting", "normal", "worldblur", "cutout"] {
            if scope.get(key).is_some() {
                return true;
            }
        }
    }
    false
}

fn parse_particle_material_blend_mode(value: Option<&str>) -> SceneRenderBlendMode {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "additive" | "add" => SceneRenderBlendMode::Additive,
        "translucent" | "alpha" | "blend" | "" => SceneRenderBlendMode::Normal,
        _ => SceneRenderBlendMode::Normal,
    }
}

fn stage_range(
    stages: &[SceneParticleStageRuntime],
    name_tokens: &[&str],
    min_keys: &[&str],
    max_keys: &[&str],
    default: [f64; 2],
) -> [f64; 2] {
    let Some(stage) = find_stage(stages, name_tokens) else {
        return default;
    };
    let min = stage_f64(stage, min_keys).unwrap_or(default[0]);
    let max = stage_f64(stage, max_keys).unwrap_or(default[1]).max(min);
    [min, max]
}

fn stage_range_with_vector_magnitude(
    stages: &[SceneParticleStageRuntime],
    name_tokens: &[&str],
    min_keys: &[&str],
    max_keys: &[&str],
    default: [f64; 2],
) -> [f64; 2] {
    let Some(stage) = find_stage(stages, name_tokens) else {
        return default;
    };
    let min = stage_number_or_vector_magnitude(stage, min_keys).unwrap_or(default[0]);
    let max = stage_number_or_vector_magnitude(stage, max_keys).unwrap_or(default[1]);
    [min.min(max), min.max(max)]
}

fn stage_vector_range(
    stages: &[SceneParticleStageRuntime],
    name_tokens: &[&str],
    min_keys: &[&str],
    max_keys: &[&str],
) -> Option<[[f64; 2]; 2]> {
    let stage = find_stage(stages, name_tokens)?;
    let min = stage_vector2(stage, min_keys)?;
    let max = stage_vector2(stage, max_keys).unwrap_or(min);
    Some([min, max])
}

fn stage_vector_component_range(
    stages: &[SceneParticleStageRuntime],
    name_tokens: &[&str],
    min_keys: &[&str],
    max_keys: &[&str],
    component_index: usize,
) -> Option<[f64; 2]> {
    let stage = find_stage(stages, name_tokens)?;
    let min = stage_vector_component(stage, min_keys, component_index)?;
    let max = stage_vector_component(stage, max_keys, component_index).unwrap_or(min);
    Some([min.min(max), min.max(max)])
}

fn stage_optional_range(
    stages: &[SceneParticleStageRuntime],
    name_tokens: &[&str],
    min_keys: &[&str],
    max_keys: &[&str],
) -> Option<[f64; 2]> {
    let stage = find_stage(stages, name_tokens)?;
    let min = stage_f64(stage, min_keys)?;
    let max = stage_f64(stage, max_keys).unwrap_or(min);
    Some([min, max])
}

fn sprite_override_scalar(authored: f64, override_value: Option<f64>) -> f64 {
    let authored = authored.max(0.0);
    match override_value {
        Some(value) if value.is_finite() && value <= 1.0 && authored > 0.0 => {
            authored * value.max(0.0)
        }
        Some(value) if value.is_finite() => value.max(0.0),
        _ => authored,
    }
}

fn sprite_fractional_override_multiplier(override_value: Option<f64>) -> f64 {
    match override_value {
        Some(value) if value.is_finite() && value <= 1.0 => value.max(0.0),
        _ => 1.0,
    }
}

fn sprite_override_range(
    authored: [f64; 2],
    override_value: f64,
    normalize_absolute: bool,
) -> [f64; 2] {
    let authored = [authored[0].max(0.0), authored[1].max(authored[0]).max(0.0)];
    if override_value.is_finite() && override_value <= 1.0 {
        return [
            authored[0] * override_value.max(0.0),
            authored[1] * override_value.max(0.0),
        ];
    }
    let absolute = if normalize_absolute {
        normalize_particle_lifetime_ms(override_value)
    } else {
        override_value.max(0.0)
    };
    [absolute, absolute]
}

fn sprite_override_count(authored: f64, override_value: Option<f64>) -> usize {
    sprite_override_scalar(authored, override_value)
        .round()
        .max(1.0) as usize
}

fn inherit_particle_instance_override(
    parent: &SceneParticleInstanceOverride,
    child: &SceneParticleInstanceOverride,
) -> SceneParticleInstanceOverride {
    SceneParticleInstanceOverride {
        rate: child.rate.or(parent.rate),
        rate_binding: child
            .rate_binding
            .clone()
            .or_else(|| parent.rate_binding.clone()),
        size: child.size.or(parent.size),
        size_binding: child
            .size_binding
            .clone()
            .or_else(|| parent.size_binding.clone()),
        speed: child.speed.or(parent.speed),
        speed_binding: child
            .speed_binding
            .clone()
            .or_else(|| parent.speed_binding.clone()),
        alpha: child.alpha.or(parent.alpha),
        alpha_binding: child
            .alpha_binding
            .clone()
            .or_else(|| parent.alpha_binding.clone()),
        lifetime: child.lifetime.or(parent.lifetime),
        lifetime_binding: child
            .lifetime_binding
            .clone()
            .or_else(|| parent.lifetime_binding.clone()),
        count: child.count.or(parent.count),
        count_binding: child
            .count_binding
            .clone()
            .or_else(|| parent.count_binding.clone()),
        color: child.color.clone().or_else(|| parent.color.clone()),
        color_binding: child
            .color_binding
            .clone()
            .or_else(|| parent.color_binding.clone()),
        control_points: if child.control_points.is_empty() {
            parent.control_points.clone()
        } else {
            child.control_points.clone()
        },
    }
}

fn sprite_turbulence(
    initializers: &[SceneParticleStageRuntime],
    operators: &[SceneParticleStageRuntime],
) -> f64 {
    find_stage(initializers, &["turbulentvelocityrandom", "turbulence"])
        .or_else(|| find_stage(operators, &["turbulentvelocityrandom", "turbulence"]))
        .map(|stage| {
            let speed_min = stage_f64(stage, &["speedmin", "min"]);
            let speed_max = stage_f64(stage, &["speedmax", "max"]);
            if speed_min.is_some() || speed_max.is_some() {
                let min = speed_min.unwrap_or(0.0);
                let max = speed_max.unwrap_or(min);
                let speed = [min.min(max), min.max(max)];
                let scale = stage_f64(stage, &["scale"]).unwrap_or(1.0).abs();
                ((speed[0].abs() + speed[1].abs()) * 0.5 * scale).min(500.0)
            } else {
                stage_f64(stage, &["strength", "force", "amount", "scale"])
                    .unwrap_or(0.0)
                    .abs()
                    .min(500.0)
            }
        })
        .unwrap_or(0.0)
}

fn sprite_position_oscillation(
    operators: &[SceneParticleStageRuntime],
    scale_x: f64,
    scale_y: f64,
) -> Option<SceneSpriteParticleOscillationConfig> {
    let stage = find_stage(
        operators,
        &[
            "oscillateposition",
            "oscillate position",
            "oscillate_position",
        ],
    )?;
    let amplitude_range = stage_range(
        std::slice::from_ref(stage),
        &[
            "oscillateposition",
            "oscillate position",
            "oscillate_position",
        ],
        &["scalemin", "minscale", "scale", "min"],
        &["scalemax", "maxscale", "scale", "max"],
        [0.0, 0.0],
    );
    let frequency_range = stage_range(
        std::slice::from_ref(stage),
        &[
            "oscillateposition",
            "oscillate position",
            "oscillate_position",
        ],
        &["frequencymin", "minfrequency", "frequency", "min"],
        &["frequencymax", "maxfrequency", "frequency", "max"],
        [1.0, 1.0],
    );
    let phase_range = stage_range(
        std::slice::from_ref(stage),
        &[
            "oscillateposition",
            "oscillate position",
            "oscillate_position",
        ],
        &["phasemin", "minphase", "phase", "min"],
        &["phasemax", "maxphase", "phase", "max"],
        [0.0, 0.0],
    );
    let mask = stage_vector2(stage, &["mask", "axis"]).unwrap_or([1.0, 1.0]);

    Some(SceneSpriteParticleOscillationConfig {
        amplitude_range: ordered_range(amplitude_range),
        frequency_range: ordered_range(frequency_range),
        phase_range: ordered_range(phase_range),
        axis_scale: [mask[0] * scale_x, mask[1] * scale_y],
    })
}

fn ordered_range(range: [f64; 2]) -> [f64; 2] {
    let min = if range[0].is_finite() { range[0] } else { 0.0 };
    let max = if range[1].is_finite() { range[1] } else { min };
    [min.min(max), min.max(max)]
}

fn sprite_scalar_oscillation(
    operators: &[SceneParticleStageRuntime],
    stage_names: &[&str],
) -> Option<SceneSpriteParticleOscillationConfig> {
    let stage = find_stage(operators, stage_names)?;
    let amplitude_range = stage_range(
        std::slice::from_ref(stage),
        stage_names,
        &["scalemin", "minscale", "scale", "amplitude", "min"],
        &["scalemax", "maxscale", "scale", "amplitude", "max"],
        [0.0, 0.0],
    );
    let frequency_range = stage_range(
        std::slice::from_ref(stage),
        stage_names,
        &["frequencymin", "minfrequency", "frequency", "min"],
        &["frequencymax", "maxfrequency", "frequency", "max"],
        [1.0, 1.0],
    );
    let phase_range = stage_range(
        std::slice::from_ref(stage),
        stage_names,
        &["phasemin", "minphase", "phase", "min"],
        &["phasemax", "maxphase", "phase", "max"],
        [0.0, 0.0],
    );
    Some(SceneSpriteParticleOscillationConfig {
        amplitude_range: ordered_range(amplitude_range),
        frequency_range: ordered_range(frequency_range),
        phase_range: ordered_range(phase_range),
        axis_scale: [1.0, 1.0],
    })
}

fn stage_color_range(
    stages: &[SceneParticleStageRuntime],
    default: SceneRenderColor,
) -> [SceneRenderColor; 2] {
    let Some(stage) = find_stage(stages, &["colorrandom", "colourrandom", "color"]) else {
        return [default, default];
    };
    let min = stage_string(stage, &["colormin", "mincolor", "colourmin", "min"])
        .map(|value| parse_scene_color(Some(value.as_str()), 1.0))
        .unwrap_or(default);
    let max = stage_string(stage, &["colormax", "maxcolor", "colourmax", "max"])
        .map(|value| parse_scene_color(Some(value.as_str()), 1.0))
        .unwrap_or(min);
    [min, max]
}

fn find_stage<'a>(
    stages: &'a [SceneParticleStageRuntime],
    name_tokens: &[&str],
) -> Option<&'a SceneParticleStageRuntime> {
    stages.iter().find(|stage| {
        let name = stage.name.to_ascii_lowercase();
        name_tokens
            .iter()
            .any(|token| name.contains(&token.to_ascii_lowercase()))
    })
}

fn stage_f64(stage: &SceneParticleStageRuntime, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| lookup_stage_value(stage, key).and_then(value_as_f64))
}

fn stage_number_or_vector_magnitude(
    stage: &SceneParticleStageRuntime,
    keys: &[&str],
) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = lookup_stage_value(stage, key)?;
        value_as_f64(value).or_else(|| {
            value_as_vector::<3>(value).map(|vector| {
                (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
            })
        })
    })
}

fn stage_vector2(stage: &SceneParticleStageRuntime, keys: &[&str]) -> Option<[f64; 2]> {
    keys.iter().find_map(|key| {
        lookup_stage_value(stage, key)
            .and_then(|value| value_as_vector::<3>(value).map(|vector| [vector[0], vector[1]]))
    })
}

fn stage_vector_component(
    stage: &SceneParticleStageRuntime,
    keys: &[&str],
    component_index: usize,
) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = lookup_stage_value(stage, key)?;
        value_as_f64(value).or_else(|| {
            value_as_vector::<3>(value).and_then(|vector| vector.get(component_index).copied())
        })
    })
}

fn stage_string(stage: &SceneParticleStageRuntime, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| lookup_stage_value(stage, key).and_then(value_as_string))
}

fn lookup_stage_value<'a>(stage: &'a SceneParticleStageRuntime, key: &str) -> Option<&'a Value> {
    stage
        .fields
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn value_as_string(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(array) = value.as_array() {
        return Some(
            array
                .iter()
                .filter_map(value_as_f64)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    value_as_f64(value).map(|value| value.to_string())
}

fn value_as_vector<const N: usize>(value: &Value) -> Option<[f64; N]> {
    if let Some(values) = value.as_array() {
        let mut vector = [0.0; N];
        for (index, slot) in vector.iter_mut().enumerate() {
            *slot = values.get(index).and_then(value_as_f64).unwrap_or(0.0);
        }
        return Some(vector);
    }

    let text = value.as_str()?;
    let values = text
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter_map(|part| {
            let part = part.trim();
            (!part.is_empty())
                .then(|| part.parse::<f64>().ok())
                .flatten()
        })
        .collect::<Vec<_>>();
    if values.len() < N.saturating_sub(1).max(1) {
        return None;
    }
    let mut vector = [0.0; N];
    for (index, slot) in vector.iter_mut().enumerate() {
        *slot = values.get(index).copied().unwrap_or(0.0);
    }
    Some(vector)
}

fn normalize_color_alpha(color: SceneRenderColor, alpha: f64) -> u8 {
    ((color.alpha as f64) * alpha.clamp(0.0, 1.0)).round() as u8
}

fn rotate_2d(vector: [f64; 2], radians: f64) -> [f64; 2] {
    if radians.abs() <= f64::EPSILON {
        return vector;
    }
    let cos = radians.cos();
    let sin = radians.sin();
    [
        vector[0] * cos - vector[1] * sin,
        vector[0] * sin + vector[1] * cos,
    ]
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

fn default_particle_max_count(kind: &SceneParticleKind) -> usize {
    match kind {
        SceneParticleKind::LineTrail => 32,
        SceneParticleKind::PetalTrail => 96,
    }
}

fn default_particle_lifetime_ms(kind: &SceneParticleKind) -> f64 {
    match kind {
        SceneParticleKind::LineTrail => 520.0,
        SceneParticleKind::PetalTrail => 1500.0,
    }
}

fn default_particle_speed_range(kind: &SceneParticleKind) -> [f64; 2] {
    match kind {
        SceneParticleKind::LineTrail => [24.0, 48.0],
        SceneParticleKind::PetalTrail => [20.0, 64.0],
    }
}

fn normalize_particle_lifetime_ms(value: f64) -> f64 {
    if value > 100.0 { value } else { value * 1000.0 }.clamp(50.0, 60_000.0)
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
    use std::{collections::BTreeMap, fs, path::PathBuf};

    use chrono::Utc;
    use image::{DynamicImage, Rgba, RgbaImage};
    use tempfile::tempdir;

    use crate::models::{
        EvaluatedAudioState, EvaluatedSceneCamera, EvaluatedSceneObject, EvaluatedSceneObjectBase,
        EvaluatedSceneTransform, EvaluatedTextLayout, EvaluatedTextState, EvaluatedTextStyle,
        SceneAssetKind, SceneEvaluatedDocument, SceneManifest, SceneParallax,
        SceneParticleChildKind, SceneParticleChildRuntime, SceneParticleControlPointRuntime,
        SceneParticleEmitterRuntime, SceneParticleInstanceOverride, SceneParticleKind,
        SceneParticleRendererFamily, SceneParticleRendererRuntime, SceneParticleRuntime,
        SceneParticleRuntimeAdapter, SceneParticleScheduleMode, SceneParticleStageRuntime,
        SceneParticleSystemRuntime, SceneRuntimeDocument, SceneTextBehavior, SceneTextLayer,
    };

    use super::{
        build_scene_render_plan, build_scene_render_plan_with_resolver, parse_scene_clear_color,
        SceneClearColor, SceneRenderBlendMode, SceneRenderCamera, SceneRenderDrawItem,
        SceneRenderDrawKind, SceneRenderIssueCode, SceneRenderIssueSeverity, SceneRenderPlan,
        SceneRenderSoundItem, SceneRenderSourceKind, SceneSpriteParticleFrame,
        SceneSpriteParticleOscillationConfig, SceneTextHorizontalAlign,
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
                parallax: SceneParallax {
                    enabled: true,
                    amount: None,
                    delay: None,
                },
                objects: objects.into_iter().collect::<BTreeMap<_, _>>(),
                render_list,
                evaluated_at: Utc::now(),
                diagnostics: Vec::new(),
            },
            now_playing: Default::default(),
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
            scale_binding: None,
            angles: None,
            rotation: None,
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
                dynamic_input_generation: None,
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

    fn supported_particle_runtime(
        object_id: u32,
        draw_kind: SceneParticleKind,
        schedule_mode: SceneParticleScheduleMode,
    ) -> SceneParticleRuntime {
        SceneParticleRuntime {
            object_id,
            object_name: "Trail".to_string(),
            particle_path: "particles/source.json".to_string(),
            object_origin: [200.0, 160.0, 0.0],
            object_scale: [1.0, 1.0, 1.0],
            object_angles: None,
            system: SceneParticleSystemRuntime {
                max_count: Some(64),
                emitters: vec![SceneParticleEmitterRuntime {
                    rate: Some(24.0),
                    origin: Some([10.0, 20.0, 0.0]),
                    speed_min: Some(4.0),
                    speed_max: Some(8.0),
                    schedule_mode,
                    ..SceneParticleEmitterRuntime::default()
                }],
                ..SceneParticleSystemRuntime::default()
            },
            instance_override: SceneParticleInstanceOverride {
                size: Some(4.0),
                lifetime: Some(1.2),
                count: Some(32.0),
                color: Some("0.2 0.4 1".to_string()),
                ..SceneParticleInstanceOverride::default()
            },
            adapter: SceneParticleRuntimeAdapter {
                supported: true,
                draw_kind: Some(draw_kind),
                rope_contract: None,
                schedule_mode,
                reason: None,
            },
            diagnostics: Vec::new(),
        }
    }

    fn supported_rope_particle_runtime(
        object_id: u32,
        family: SceneParticleRendererFamily,
        schedule_mode: SceneParticleScheduleMode,
    ) -> SceneParticleRuntime {
        SceneParticleRuntime {
            object_id,
            object_name: "Rope".to_string(),
            particle_path: "particles/rope.json".to_string(),
            object_origin: [12.0, -18.0, 0.0],
            object_scale: [1.0, 1.0, 1.0],
            object_angles: Some([0.0, 0.0, 0.0]),
            system: SceneParticleSystemRuntime {
                material: Some("materials/rope.material".to_string()),
                emitters: vec![SceneParticleEmitterRuntime {
                    rate: Some(28.0),
                    schedule_mode,
                    ..SceneParticleEmitterRuntime::default()
                }],
                ..SceneParticleSystemRuntime::default()
            },
            instance_override: SceneParticleInstanceOverride {
                color: Some("0.2 0.4 1".to_string()),
                ..SceneParticleInstanceOverride::default()
            },
            adapter: SceneParticleRuntimeAdapter {
                supported: true,
                draw_kind: Some(SceneParticleKind::LineTrail),
                rope_contract: Some(crate::models::SceneRopeParticleRuntimeContract {
                    renderer_family: family,
                    schedule_mode,
                    control_points: vec![
                        crate::models::SceneRopeParticleControlPointRuntime {
                            id: 0,
                            offset: [0.0, 0.0, 0.0],
                            parent_control_point: None,
                            lock_to_pointer: false,
                            override_key: None,
                            override_value: None,
                            override_binding: None,
                        },
                        crate::models::SceneRopeParticleControlPointRuntime {
                            id: 1,
                            offset: [80.0, 20.0, 0.0],
                            parent_control_point: Some(0),
                            lock_to_pointer: false,
                            override_key: None,
                            override_value: None,
                            override_binding: None,
                        },
                    ],
                    emission_rate: 32.0,
                    segment_count: 12,
                    subdivision: 4,
                    length: 240.0,
                    min_length: 60.0,
                    max_length: 360.0,
                    width: 6.0,
                    lifetime_ms: 1500.0,
                    alpha: 0.8,
                    color: Some("0.2 0.4 1".to_string()),
                    material_path: Some("materials/rope.material".to_string()),
                    uv_scrolling: [0.5, -0.25],
                    fade_alpha: 0.2,
                }),
                schedule_mode,
                reason: None,
            },
            diagnostics: Vec::new(),
        }
    }

    fn supported_sprite_particle_runtime(object_id: u32) -> SceneParticleRuntime {
        SceneParticleRuntime {
            object_id,
            object_name: "Sprite".to_string(),
            particle_path: "particles/sprite.json".to_string(),
            object_origin: [6.0, 10.0, 0.0],
            object_scale: [1.0, 1.0, 1.0],
            object_angles: None,
            system: SceneParticleSystemRuntime {
                max_count: Some(96),
                start_time: Some(0.05),
                material: Some("materials/sprite.material".to_string()),
                emitters: vec![SceneParticleEmitterRuntime {
                    rate: Some(30.0),
                    origin: Some([4.0, 5.0, 0.0]),
                    distance_min: Some(1.0),
                    distance_max: Some(8.0),
                    directions: vec![[0.0, 1.0, 0.0]],
                    speed_min: Some(2.0),
                    speed_max: Some(6.0),
                    schedule_mode: SceneParticleScheduleMode::Autonomous,
                    ..SceneParticleEmitterRuntime::default()
                }],
                renderers: vec![SceneParticleRendererRuntime {
                    family: SceneParticleRendererFamily::Sprite,
                    name: Some("sprite".to_string()),
                    ..SceneParticleRendererRuntime::default()
                }],
                children: vec![SceneParticleChildRuntime {
                    name: Some("particles/child.json".to_string()),
                    child_type: SceneParticleChildKind::EventDeath,
                    origin: Some([2.0, 3.0, 0.0]),
                    probability: Some(1.0),
                    max_count: Some(8),
                    ..SceneParticleChildRuntime::default()
                }],
                initializers: vec![
                    SceneParticleStageRuntime {
                        name: "lifetimerandom".to_string(),
                        fields: BTreeMap::from([
                            ("min".to_string(), serde_json::json!(0.5)),
                            ("max".to_string(), serde_json::json!(1.5)),
                        ]),
                    },
                    SceneParticleStageRuntime {
                        name: "sizerandom".to_string(),
                        fields: BTreeMap::from([
                            ("min".to_string(), serde_json::json!(12)),
                            ("max".to_string(), serde_json::json!(24)),
                        ]),
                    },
                    SceneParticleStageRuntime {
                        name: "colorrandom".to_string(),
                        fields: BTreeMap::from([
                            ("min".to_string(), serde_json::json!("0.2 0.3 0.4")),
                            ("max".to_string(), serde_json::json!("1 0.8 0.6")),
                        ]),
                    },
                    SceneParticleStageRuntime {
                        name: "rotationrandom".to_string(),
                        fields: BTreeMap::from([
                            ("min".to_string(), serde_json::json!(-10)),
                            ("max".to_string(), serde_json::json!(20)),
                        ]),
                    },
                ],
                operators: vec![
                    SceneParticleStageRuntime {
                        name: "movement".to_string(),
                        fields: BTreeMap::from([
                            ("speedmin".to_string(), serde_json::json!(3)),
                            ("speedmax".to_string(), serde_json::json!(7)),
                        ]),
                    },
                    SceneParticleStageRuntime {
                        name: "turbulentvelocityrandom".to_string(),
                        fields: BTreeMap::from([("strength".to_string(), serde_json::json!(5))]),
                    },
                    SceneParticleStageRuntime {
                        name: "sizechange".to_string(),
                        fields: BTreeMap::from([
                            ("startvalue".to_string(), serde_json::json!(1.0)),
                            ("endvalue".to_string(), serde_json::json!(0.3)),
                        ]),
                    },
                ],
                ..SceneParticleSystemRuntime::default()
            },
            instance_override: SceneParticleInstanceOverride {
                rate: Some(42.0),
                size: Some(0.5),
                speed: Some(11.0),
                alpha: Some(0.6),
                lifetime: Some(2.0),
                count: Some(32.0),
                color: Some("1 0.5 0.25".to_string()),
                ..SceneParticleInstanceOverride::default()
            },
            adapter: SceneParticleRuntimeAdapter {
                supported: true,
                draw_kind: None,
                rope_contract: None,
                schedule_mode: SceneParticleScheduleMode::Autonomous,
                reason: None,
            },
            diagnostics: Vec::new(),
        }
    }

    fn write_sprite_particle_material_fixture(managed_root: &std::path::Path) {
        let source = managed_root.join("source");
        fs::create_dir_all(source.join("materials")).expect("materials dir");
        fs::create_dir_all(source.join("textures")).expect("textures dir");
        fs::create_dir_all(source.join("particles")).expect("particles dir");
        fs::write(
            source.join("materials/sprite.material"),
            r#"{"shader":"genericparticle","textures":["textures/sprite.png"],"blending":"additive"}"#,
        )
        .expect("material");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255])))
            .save(source.join("textures/sprite.png"))
            .expect("texture");
        fs::write(
            source.join("particles/child.json"),
            r#"{
              "maxcount": 4,
              "material": "materials/sprite.material",
              "emitter": [{"name":"sphererandom","rate":4}],
              "renderer": [{"name":"sprite"}],
              "initializer": [{"name":"sizerandom","min":4,"max":8}]
            }"#,
        )
        .expect("child particle");
    }

    fn write_rope_particle_material_fixture(managed_root: &std::path::Path) {
        let source = managed_root.join("source");
        fs::create_dir_all(source.join("materials")).expect("materials dir");
        fs::create_dir_all(source.join("textures")).expect("textures dir");
        fs::write(
            source.join("materials/rope.material"),
            r#"{"shader":"rope","textures":["textures/rope.png"],"blending":"additive"}"#,
        )
        .expect("rope material");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 255])))
            .save(source.join("textures/rope.png"))
            .expect("rope texture");
    }

    fn unsupported_particle_runtime(object_id: u32) -> SceneParticleRuntime {
        SceneParticleRuntime {
            object_id,
            object_name: "Unsupported".to_string(),
            particle_path: "particles/source.json".to_string(),
            adapter: SceneParticleRuntimeAdapter {
                supported: false,
                draw_kind: None,
                rope_contract: None,
                schedule_mode: SceneParticleScheduleMode::InputDriven,
                reason: Some("child hierarchy is deferred".to_string()),
            },
            ..SceneParticleRuntime::default()
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
        assert_eq!(
            report.plan.draw_order,
            vec![
                SceneRenderDrawItem {
                    object_id: 1,
                    kind: SceneRenderDrawKind::Visual,
                },
                SceneRenderDrawItem {
                    object_id: 2,
                    kind: SceneRenderDrawKind::Text,
                },
                SceneRenderDrawItem {
                    object_id: 3,
                    kind: SceneRenderDrawKind::Audio,
                },
                SceneRenderDrawItem {
                    object_id: 4,
                    kind: SceneRenderDrawKind::Particle,
                },
                SceneRenderDrawItem {
                    object_id: 5,
                    kind: SceneRenderDrawKind::Sound,
                },
            ]
        );
    }

    #[test]
    fn render_plan_maps_supported_particle_runtime_to_autonomous_scheduler_item() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        scene.source.particle_runtimes = vec![supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        )];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert_eq!(report.plan.particles.len(), 1);
        let item = &report.plan.particles[0];
        assert_eq!(item.schedule_mode, SceneParticleScheduleMode::Autonomous);
        assert_eq!(item.spawn_origin, [210.0, 180.0]);
        assert_eq!(item.max_count, 32);
        assert_eq!(item.lifetime_ms, 1200.0);
        assert_eq!(item.speed_range, [4.0, 8.0]);
        assert_eq!(item.size, 4.0);
    }

    #[test]
    fn render_plan_promotes_supported_rope_runtime_to_first_class_rope_item() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_rope_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "Rope", SceneParticleKind::LineTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        scene.source.particle_runtimes = vec![supported_rope_particle_runtime(
            4,
            SceneParticleRendererFamily::Rope,
            SceneParticleScheduleMode::Autonomous,
        )];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert!(report.plan.particles.is_empty());
        assert_eq!(report.plan.rope_particles.len(), 1);
        assert_eq!(
            report.plan.draw_order,
            vec![SceneRenderDrawItem {
                object_id: 4,
                kind: SceneRenderDrawKind::RopeParticle,
            }]
        );
        let item = &report.plan.rope_particles[0];
        assert_eq!(item.renderer_family, SceneParticleRendererFamily::Rope);
        assert_eq!(item.schedule_mode, SceneParticleScheduleMode::Autonomous);
        assert_eq!(item.segment_count, 12);
        assert_eq!(item.subdivision, 4);
        assert_eq!(item.length, 240.0);
        assert_eq!(item.emission_rate, 32.0);
        assert_eq!(item.width, 6.0);
        assert_eq!(item.lifetime_ms, 1500.0);
        assert_eq!(item.control_points.len(), 2);
        assert_eq!(item.control_points[0].position, [12.0, -18.0]);
        assert_eq!(item.control_points[1].position, [92.0, 2.0]);
        assert_eq!(item.uv_scrolling, [0.5, -0.25]);
        assert!((item.fade_alpha - 0.2).abs() < 0.001);
    }

    #[test]
    fn rope_runtime_without_drawable_topology_keeps_scene_non_renderable() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Rope", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let mut runtime = supported_rope_particle_runtime(
            4,
            SceneParticleRendererFamily::RopeTrail,
            SceneParticleScheduleMode::InputDriven,
        );
        if let Some(contract) = runtime.adapter.rope_contract.as_mut() {
            contract.control_points.truncate(1);
        }
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(report.plan.rope_particles.is_empty());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::ParticleNoRenderableOutput));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::NoRenderableVisuals));
        assert!(report.is_blocked());
    }

    #[test]
    fn rope_material_bridge_resolves_single_texture_and_blend_mode() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_rope_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "Rope", SceneParticleKind::LineTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        scene.source.particle_runtimes = vec![supported_rope_particle_runtime(
            4,
            SceneParticleRendererFamily::Rope,
            SceneParticleScheduleMode::Autonomous,
        )];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        let item = &report.plan.rope_particles[0];
        assert_eq!(item.blend_mode, SceneRenderBlendMode::Additive);
        assert!(item
            .texture_path
            .as_ref()
            .is_some_and(|path| path.ends_with("textures/rope.png")));
    }

    #[test]
    fn rope_material_bridge_rejects_phase10_texture_variants() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_rope_particle_material_fixture(&managed_root);
        fs::write(
            managed_root.join("source/materials/rope.material"),
            r#"{"shader":"rope","textures":["textures/rope.png","textures/normal.png"],"blending":"additive"}"#,
        )
        .expect("complex rope material");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Rope", SceneParticleKind::LineTrail))],
            vec![4],
        );
        scene.source.particle_runtimes = vec![supported_rope_particle_runtime(
            4,
            SceneParticleRendererFamily::RopeTrail,
            SceneParticleScheduleMode::InputDriven,
        )];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(report.plan.rope_particles.is_empty());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::UnsupportedParticleRuntime));
    }

    #[test]
    fn phase_09f_acceptance_entries_produce_first_class_rope_draw_items() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_rope_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        for (entry_id, family, schedule_mode) in [
            (
                "presets/interactive/previewtrails0",
                SceneParticleRendererFamily::Rope,
                SceneParticleScheduleMode::InputDriven,
            ),
            (
                "presets/interactive/previewtrails1",
                SceneParticleRendererFamily::Rope,
                SceneParticleScheduleMode::InputDriven,
            ),
            (
                "presets/interactive/previewtrails2",
                SceneParticleRendererFamily::RopeTrail,
                SceneParticleScheduleMode::InputDriven,
            ),
            (
                "scenes/particleelementpreviews/rope",
                SceneParticleRendererFamily::Rope,
                SceneParticleScheduleMode::Autonomous,
            ),
            (
                "scenes/particleelementpreviews/ropetrail",
                SceneParticleRendererFamily::RopeTrail,
                SceneParticleScheduleMode::InputDriven,
            ),
        ] {
            let mut particle = particle_object(4, entry_id, SceneParticleKind::LineTrail);
            if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
                base.transform.position = [0.0, 0.0, 0.0];
                base.transform.rotation = 0.0;
            }
            let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
            scene.source.particle_runtimes = vec![supported_rope_particle_runtime(
                4,
                family,
                schedule_mode,
            )];

            let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

            assert!(
                !report.is_blocked(),
                "{entry_id} should remain renderable, got issues: {:?}",
                report.issues
            );
            assert!(
                report.plan.particles.is_empty(),
                "{entry_id} should not fall back to legacy particle draw items"
            );
            assert_eq!(
                report.plan.rope_particles.len(),
                1,
                "{entry_id} should produce one rope render item"
            );
            assert_eq!(
                report.plan.draw_order,
                vec![SceneRenderDrawItem {
                    object_id: 4,
                    kind: SceneRenderDrawKind::RopeParticle,
                }],
                "{entry_id} should submit a rope draw item"
            );

            let rope_item = &report.plan.rope_particles[0];
            assert_eq!(rope_item.renderer_family, family, "{entry_id} family mismatch");
            assert_eq!(
                rope_item.schedule_mode, schedule_mode,
                "{entry_id} schedule mode mismatch"
            );
            assert!(
                rope_item.control_points.len() >= 2,
                "{entry_id} should expose drawable rope topology"
            );
            assert!(
                rope_item.segment_count > 0 && rope_item.width > 0.0,
                "{entry_id} should expose drawable rope geometry"
            );
            assert!(
                rope_item.texture_path.is_some(),
                "{entry_id} should bridge the minimal rope material"
            );
        }
    }

    #[test]
    fn render_plan_keeps_pointer_control_point_trails_input_driven() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let mut runtime = supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        );
        runtime.system.control_points = vec![SceneParticleControlPointRuntime {
            id: Some(0),
            flags: vec!["1".to_string()],
            ..SceneParticleControlPointRuntime::default()
        }];
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(
            report.plan.particles[0].schedule_mode,
            SceneParticleScheduleMode::InputDriven
        );
    }

    #[test]
    fn render_plan_passes_start_time_from_runtime_to_particle_item() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let mut runtime = supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        );
        runtime.system.start_time = Some(1500.0);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].start_time_ms, 1500.0);
    }

    #[test]
    fn render_plan_defaults_start_time_to_zero_when_not_in_runtime() {
        let scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].start_time_ms, 0.0);
    }

    #[test]
    fn render_plan_passes_emitter_sign_and_distance_to_particle_item() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let mut runtime = supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        );
        runtime.system.emitters[0].sign = Some(-1.0);
        runtime.system.emitters[0].distance_min = Some(50.0);
        runtime.system.emitters[0].distance_max = Some(120.0);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].sign, -1.0);
        assert_eq!(report.plan.particles[0].spawn_radius, [50.0, 120.0]);
    }

    #[test]
    fn render_plan_defaults_sign_and_distance_for_missing_emitter_fields() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let runtime = supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        );
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].sign, 1.0);
        assert_eq!(report.plan.particles[0].spawn_radius, [0.0, 0.0]);
    }

    #[test]
    fn render_plan_passes_renderer_uv_scrolling_and_fade_alpha_to_particle_item() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let mut runtime = supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        );
        runtime.system.renderers = vec![SceneParticleRendererRuntime {
            uv_scrolling: Some([0.5, -0.25]),
            fade_alpha: Some(0.3),
            ..SceneParticleRendererRuntime::default()
        }];
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].uv_scrolling, [0.5, -0.25]);
        assert!((report.plan.particles[0].fade_alpha - 0.3).abs() < 0.001);
    }

    #[test]
    fn render_plan_defaults_uv_scrolling_and_fade_alpha_for_missing_renderer_fields() {
        let scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].uv_scrolling, [0.0, 0.0]);
        assert_eq!(report.plan.particles[0].fade_alpha, 0.0);
    }

    #[test]
    fn render_plan_passes_renderer_subdivision_and_length_to_particle_item() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        let mut runtime = supported_particle_runtime(
            4,
            SceneParticleKind::LineTrail,
            SceneParticleScheduleMode::Autonomous,
        );
        runtime.system.renderers = vec![SceneParticleRendererRuntime {
            subdivision: Some(4),
            length: Some(300.0),
            ..SceneParticleRendererRuntime::default()
        }];
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.particles[0].subdivision, 4);
        assert_eq!(report.plan.particles[0].rope_length, 300.0);
    }

    #[test]
    fn render_plan_defaults_subdivision_and_length_for_missing_renderer_fields() {
        let scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.particles[0].subdivision, 1);
        assert_eq!(report.plan.particles[0].rope_length, 0.0);
    }

    #[test]
    fn render_plan_builds_first_class_sprite_particle_item_with_material_bridge() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "Sprite", SceneParticleKind::PetalTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        scene.source.particle_runtimes = vec![supported_sprite_particle_runtime(4)];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert!(report.plan.particles.is_empty());
        assert_eq!(report.plan.sprite_particles.len(), 1);
        assert_eq!(
            report.plan.draw_order,
            vec![SceneRenderDrawItem {
                object_id: 4,
                kind: SceneRenderDrawKind::SpriteParticle,
            }]
        );
        let item = &report.plan.sprite_particles[0];
        assert_eq!(item.schedule_mode, SceneParticleScheduleMode::Autonomous);
        assert_eq!(item.config.blend_mode, SceneRenderBlendMode::Additive);
        assert!(item.config.texture_path.ends_with("textures/sprite.png"));
        assert_eq!(item.config.emission_rate, 42.0);
        assert_eq!(item.config.max_count, 32);
        assert_eq!(item.config.lifetime_ms_range, [2000.0, 2000.0]);
        assert_eq!(item.config.speed_range, [11.0, 11.0]);
        assert_eq!(item.config.size_range, [6.0, 12.0]);
        assert_eq!(
            item.config.texture_frames,
            vec![SceneSpriteParticleFrame {
                uv_rect: [0.0, 0.0, 1.0, 1.0],
                aspect_ratio: 1.0,
            }]
        );
        assert_eq!(item.children.len(), 1);
        assert_eq!(
            item.children[0].child_type,
            SceneParticleChildKind::EventDeath
        );
    }

    #[test]
    fn render_plan_promotes_spritetrail_genericparticle_to_first_class_sprite_runtime() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "SpriteTrail", SceneParticleKind::PetalTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.renderers[0].family = SceneParticleRendererFamily::SpriteTrail;
        runtime.system.renderers[0].name = Some("spritetrail".to_string());
        runtime.system.children.clear();
        runtime.system.max_count = Some(512);
        runtime.instance_override.count = Some(1.0);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert!(report.plan.particles.is_empty());
        assert_eq!(report.plan.sprite_particles.len(), 1);
        assert_eq!(
            report.plan.draw_order,
            vec![SceneRenderDrawItem {
                object_id: 4,
                kind: SceneRenderDrawKind::SpriteParticle,
            }]
        );
        let config = &report.plan.sprite_particles[0].config;
        assert_eq!(config.max_count, 512);
        assert!(config.texture_path.ends_with("textures/sprite.png"));
    }

    #[test]
    fn render_plan_consumes_input_driven_sprite_control_point_and_force_configs() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "CursorSprite", SceneParticleKind::PetalTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.renderers[0].family = SceneParticleRendererFamily::SpriteTrail;
        runtime.system.renderers[0].name = Some("spritetrail".to_string());
        runtime.system.control_points = vec![
            SceneParticleControlPointRuntime {
                id: Some(0),
                flags: vec!["1".to_string()],
                ..SceneParticleControlPointRuntime::default()
            },
            SceneParticleControlPointRuntime {
                id: Some(1),
                offset: Some([0.0, -9999.0, 0.0]),
                ..SceneParticleControlPointRuntime::default()
            },
        ];
        runtime.system.initializers.push(SceneParticleStageRuntime {
            name: "mapsequencearoundcontrolpoint".to_string(),
            fields: BTreeMap::from([
                ("count".to_string(), serde_json::json!(5)),
                ("speedmin".to_string(), serde_json::json!("0 100 0")),
                ("speedmax".to_string(), serde_json::json!("0 100 0")),
            ]),
        });
        runtime.system.operators.push(SceneParticleStageRuntime {
            name: "controlpointattract".to_string(),
            fields: BTreeMap::from([
                ("origin".to_string(), serde_json::json!("0 0 0")),
                ("scale".to_string(), serde_json::json!(500)),
                ("threshold".to_string(), serde_json::json!(200)),
            ]),
        });
        runtime.system.operators.push(SceneParticleStageRuntime {
            name: "vortex".to_string(),
            fields: BTreeMap::from([
                ("distanceinner".to_string(), serde_json::json!(0)),
                ("distanceouter".to_string(), serde_json::json!(50)),
                ("speedinner".to_string(), serde_json::json!(300)),
                ("speedouter".to_string(), serde_json::json!(0)),
            ]),
        });
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        let item = &report.plan.sprite_particles[0];
        assert_eq!(item.schedule_mode, SceneParticleScheduleMode::InputDriven);
        assert_eq!(item.config.control_points.len(), 2);
        assert!(item.config.control_points[0].lock_to_pointer);
        assert_eq!(
            item.config.sequence_control_point
                .expect("sequence config")
                .count,
            5
        );
        assert_eq!(item.config.attractors.len(), 1);
        assert_eq!(item.config.vortexes.len(), 1);
    }

    #[test]
    fn sprite_material_bridge_reads_tex_json_spritesheet_frame_uvs() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        let texture_path = managed_root.join("source/textures/sprite.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 4, Rgba([255, 255, 255, 255])))
            .save(&texture_path)
            .expect("atlas texture");
        fs::write(
            managed_root.join("source/textures/sprite.tex-json"),
            r#"{"spritesheetsequences":[{"frames":8,"width":2,"height":2}]}"#,
        )
        .expect("atlas metadata");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "Sprite", SceneParticleKind::PetalTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        scene.source.particle_runtimes = vec![supported_sprite_particle_runtime(4)];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        let frames = &report.plan.sprite_particles[0].config.texture_frames;
        assert_eq!(frames.len(), 8);
        assert_eq!(frames[0].uv_rect, [0.0, 0.0, 0.25, 0.5]);
        assert_eq!(frames[1].uv_rect, [0.25, 0.0, 0.5, 0.5]);
        assert_eq!(frames[4].uv_rect, [0.0, 0.5, 0.25, 1.0]);
        assert_eq!(frames[0].aspect_ratio, 1.0);
    }

    #[test]
    fn render_plan_consumes_scene_visual_spritesheet_uv_from_resolved_texture_metadata() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let source_root = managed_root.join("source");
        let decoded_root = managed_root.join("decoded");
        let texture_path = decoded_root.join("gifs/gifscene.png");
        let decoded_dir = texture_path.parent().expect("decoded dir");
        let source_dir = source_root.join("gifs");
        fs::create_dir_all(decoded_dir).expect("decoded dir");
        fs::create_dir_all(&source_dir).expect("source dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 4, Rgba([255, 255, 255, 255])))
            .save(&texture_path)
            .expect("decoded texture");
        fs::write(
            source_dir.join("gifscene.tex-json"),
            r#"{"spritesheetsequences":[{"frames":8,"width":2,"height":2}]}"#,
        )
        .expect("spritesheet metadata");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let scene = runtime_scene_with_objects(
            vec![(
                7,
                visual_object(
                    7,
                    "Gif Scene",
                    SceneAssetKind::Image,
                    Some(texture_path.display().to_string()),
                    Some([0.0, 0.0, 640.0, 320.0]),
                    [1.0, 1.0, 1.0],
                    None,
                    None,
                ),
            )],
            vec![7],
        );

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        assert!(report.issues.is_empty());
        assert_eq!(report.plan.visuals.len(), 1);
        assert_eq!(report.plan.visuals[0].uv_rect, [0.0, 0.0, 0.25, 0.5]);
    }

    #[test]
    fn phase_10a_acceptance_entries_consume_direct_bmp_tga_and_scene_level_spritesheet_assets() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let extracted_root = managed_root.join("extracted");
        let decoded_root = managed_root.join("decoded");
        fs::create_dir_all(extracted_root.join("images")).expect("images dir");
        fs::create_dir_all(extracted_root.join("gifs")).expect("gifs dir");
        fs::create_dir_all(decoded_root.join("gifs")).expect("decoded gifs dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        let bmp_path = extracted_root.join("images/sample.bmp");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 2, Rgba([12, 34, 56, 255])))
            .save_with_format(&bmp_path, image::ImageFormat::Bmp)
            .expect("write bmp");
        let tga_path = extracted_root.join("images/sample.tga");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 2, Rgba([90, 80, 70, 255])))
            .save_with_format(&tga_path, image::ImageFormat::Tga)
            .expect("write tga");

        let gifsheet_path = decoded_root.join("gifs/gifscene.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 4, Rgba([255, 255, 255, 255])))
            .save(&gifsheet_path)
            .expect("write decoded gifscene");
        fs::write(
            extracted_root.join("gifs").join("gifscene.tex-json"),
            r#"{"spritesheetsequences":[{"frames":8,"width":2,"height":2}]}"#,
        )
        .expect("gifscene metadata");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        for (entry_id, asset_path, expected_uv_rect) in [
            (
                "phase-10a/direct-bmp",
                bmp_path.display().to_string(),
                [0.0, 0.0, 1.0, 1.0],
            ),
            (
                "phase-10a/direct-tga",
                tga_path.display().to_string(),
                [0.0, 0.0, 1.0, 1.0],
            ),
            (
                "gifs/gifscene.json",
                gifsheet_path.display().to_string(),
                [0.0, 0.0, 0.25, 0.5],
            ),
        ] {
            let scene = runtime_scene_with_objects(
                vec![(
                    7,
                    visual_object(
                        7,
                        entry_id,
                        SceneAssetKind::Image,
                        Some(asset_path),
                        Some([0.0, 0.0, 640.0, 320.0]),
                        [1.0, 1.0, 1.0],
                        None,
                        None,
                    ),
                )],
                vec![7],
            );

            let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

            assert!(!report.is_blocked(), "{entry_id} should remain renderable");
            assert!(report.issues.is_empty(), "{entry_id} should not report planner issues");
            assert_eq!(report.plan.visuals.len(), 1, "{entry_id} should produce one visual");
            assert_eq!(
                report.plan.visuals[0].uv_rect, expected_uv_rect,
                "{entry_id} should consume the expected visual UV contract"
            );
        }
    }

    #[test]
    fn phase_10a_texture_candidate_lookup_stays_consistent_across_sidecar_spellings() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        let source_root = managed_root.join("source");
        let decoded_root = managed_root.join("decoded");
        fs::create_dir_all(source_root.join("textures")).expect("source textures dir");
        fs::create_dir_all(decoded_root.join("textures")).expect("decoded textures dir");
        fs::create_dir_all(&builtin_root).expect("builtin dir");

        let decoded_png = decoded_root.join("textures/hero.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 4, Rgba([255, 255, 255, 255])))
            .save(&decoded_png)
            .expect("decoded hero");
        fs::write(
            source_root.join("textures/hero.tex-json"),
            r#"{"spritesheetsequences":[{"frames":8,"width":2,"height":2}]}"#,
        )
        .expect("hero tex-json");
        fs::write(
            source_root.join("textures/alt.tex.json"),
            r#"{"spritesheetsequences":[{"frames":4,"width":2,"height":1}]}"#,
        )
        .expect("alt tex.json");
        let direct_png = source_root.join("textures/direct.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
            .save(&direct_png)
            .expect("direct png");

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);

        let authored_tex = resolver.inspect_texture_candidates(
            Some("materials/sample.material"),
            None,
            "textures/hero.tex",
        );
        let authored_tex_json = resolver.inspect_texture_candidates(
            Some("materials/sample.material"),
            None,
            "textures/hero.tex-json",
        );
        let authored_alt = resolver.inspect_texture_candidates(
            Some("materials/sample.material"),
            None,
            "textures/alt.tex.json",
        );
        let direct = resolver.inspect_texture_candidates(
            Some("materials/sample.material"),
            None,
            "textures/direct.png",
        );

        assert_eq!(authored_tex.matched_paths, authored_tex_json.matched_paths);
        assert!(
            authored_tex
                .matched_paths
                .first()
                .is_some_and(|path| path.ends_with("decoded/textures/hero.png"))
        );
        assert!(
            authored_alt
                .matched_paths
                .is_empty(),
            "metadata-only sidecar spellings should not be treated as consumable texture assets"
        );
        assert_eq!(direct.matched_paths, vec![direct_png]);
    }

    #[test]
    fn sprite_instanceoverride_fraction_values_scale_authored_particle_contract() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.instance_override.rate = Some(0.5);
        runtime.instance_override.speed = Some(0.5);
        runtime.instance_override.lifetime = Some(0.5);
        runtime.instance_override.count = Some(0.5);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let item = &report.plan.sprite_particles[0];
        assert_eq!(item.config.emission_rate, 15.0);
        assert_eq!(item.config.max_count, 48);
        assert_eq!(item.config.speed_range, [1.5, 3.5]);
        assert_eq!(item.config.lifetime_ms_range, [250.0, 750.0]);
    }

    #[test]
    fn sprite_child_runtime_inherits_parent_instanceoverride_contract() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.instance_override.rate = Some(0.5);
        runtime.instance_override.size = Some(0.5);
        runtime.instance_override.speed = Some(0.5);
        runtime.instance_override.lifetime = Some(0.5);
        runtime.instance_override.count = Some(0.5);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let child = &report.plan.sprite_particles[0].children[0].config;
        assert_eq!(child.emission_rate, 2.0);
        assert_eq!(child.max_count, 4);
        assert_eq!(child.size_range, [2.0, 4.0]);
        assert_eq!(child.lifetime_ms_range, [600.0, 1200.0]);
    }

    #[test]
    fn sprite_child_control_point_start_index_offsets_spawn_origin() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.control_points = vec![
            SceneParticleControlPointRuntime {
                id: Some(0),
                offset: Some([0.0, 0.0, 0.0]),
                ..SceneParticleControlPointRuntime::default()
            },
            SceneParticleControlPointRuntime {
                id: Some(1),
                offset: Some([50.0, -30.0, 0.0]),
                ..SceneParticleControlPointRuntime::default()
            },
        ];
        runtime.system.children[0].control_point_start_index = Some(1);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let child = &report.plan.sprite_particles[0].children[0];
        assert_eq!(child.control_point_start_index, Some(1));

        let mut no_offset = scene.clone();
        let mut runtime_no = supported_sprite_particle_runtime(4);
        runtime_no.system.control_points = vec![
            SceneParticleControlPointRuntime {
                id: Some(0),
                offset: Some([0.0, 0.0, 0.0]),
                ..SceneParticleControlPointRuntime::default()
            },
            SceneParticleControlPointRuntime {
                id: Some(1),
                offset: Some([50.0, -30.0, 0.0]),
                ..SceneParticleControlPointRuntime::default()
            },
        ];
        no_offset.source.particle_runtimes = vec![runtime_no];
        let report_no = build_scene_render_plan_with_resolver(&no_offset, Some(&resolver));

        let child_no = &report_no.plan.sprite_particles[0].children[0];
        assert_eq!(child_no.control_point_start_index, None);

        let dx = child.config.spawn_origin[0] - child_no.config.spawn_origin[0];
        let dy = child.config.spawn_origin[1] - child_no.config.spawn_origin[1];
        assert!(
            dx > 30.0,
            "control_point offset [50,-30] should shift spawn_origin x rightward, got dx={dx}"
        );
        assert!(
            dy < -10.0,
            "control_point offset [50,-30] should shift spawn_origin y upward, got dy={dy}"
        );
    }

    #[test]
    fn sprite_child_control_point_start_index_none_when_missing() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let runtime = supported_sprite_particle_runtime(4);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let child = &report.plan.sprite_particles[0].children[0];
        assert_eq!(child.control_point_start_index, None);
    }

    #[test]
    fn sprite_child_angle_adds_to_parent_angle() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.object_angles = Some([0.0, 0.0, 90.0]);
        runtime.system.children[0].angles = Some([0.0, 0.0, 45.0]);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let child_ori = report.plan.sprite_particles[0].children[0]
            .config
            .orientation;
        let parent_ori = report.plan.sprite_particles[0].config.orientation;
        assert!(
            child_ori > parent_ori,
            "child orientation {child_ori} should be greater than parent {parent_ori}"
        );
    }

    #[test]
    fn sprite_velocityrandom_preserves_authored_vector_direction() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.initializers.push(SceneParticleStageRuntime {
            name: "velocityrandom".to_string(),
            fields: BTreeMap::from([
                ("min".to_string(), serde_json::json!("-100 -100 0")),
                ("max".to_string(), serde_json::json!("-50 -15 0")),
            ]),
        });
        runtime.instance_override.speed = Some(0.5);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let config = &report.plan.sprite_particles[0].config;
        assert_eq!(config.velocity_range, Some([[-50.0, -50.0], [-25.0, -7.5]]));
    }

    #[test]
    fn sprite_alphafade_enters_runtime_as_edge_fade_windows() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.operators.push(SceneParticleStageRuntime {
            name: "alphafade".to_string(),
            fields: BTreeMap::from([
                ("fadeintime".to_string(), serde_json::json!(0.1)),
                ("fadeouttime".to_string(), serde_json::json!(0.9)),
            ]),
        });
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let config = &report.plan.sprite_particles[0].config;
        assert_eq!(config.fade_in_ms, 100.0);
        assert_eq!(config.fade_out_ms, 900.0);
    }

    #[test]
    fn sprite_system_sequence_multiplier_enters_config() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "Sprite", SceneParticleKind::PetalTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.sequence_multiplier = Some(2.5);
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let config = &report.plan.sprite_particles[0].config;
        assert!((config.sequence_multiplier - 2.5).abs() < 0.001);
    }

    #[test]
    fn sprite_oscillateposition_enters_runtime_as_position_oscillation() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.system.operators.push(SceneParticleStageRuntime {
            name: "oscillateposition".to_string(),
            fields: BTreeMap::from([
                ("frequencymin".to_string(), serde_json::json!(0.8)),
                ("frequencymax".to_string(), serde_json::json!(1.0)),
                ("phasemin".to_string(), serde_json::json!(0.25)),
                ("phasemax".to_string(), serde_json::json!(0.75)),
                ("scalemin".to_string(), serde_json::json!(20)),
                ("scalemax".to_string(), serde_json::json!(35)),
                ("mask".to_string(), serde_json::json!("1 0.5 0")),
            ]),
        });
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let config = &report.plan.sprite_particles[0].config;
        assert_eq!(
            config.position_oscillation,
            Some(SceneSpriteParticleOscillationConfig {
                amplitude_range: [20.0, 35.0],
                frequency_range: [0.8, 1.0],
                phase_range: [0.25, 0.75],
                axis_scale: [1.0, 0.5],
            })
        );
    }

    #[test]
    fn sprite_object_scale_expands_emitter_and_velocity_space_without_scaling_size() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut particle = particle_object(4, "Sprite", SceneParticleKind::PetalTrail);
        if let EvaluatedSceneObject::Particle { base, .. } = &mut particle {
            base.transform.position = [0.0, 0.0, 0.0];
            base.transform.rotation = 0.0;
        }
        let mut scene = runtime_scene_with_objects(vec![(4, particle)], vec![4]);
        let mut runtime = supported_sprite_particle_runtime(4);
        runtime.object_scale = [2.0, 2.0, 2.0];
        runtime.instance_override.size = None;
        runtime.instance_override.speed = None;
        runtime.system.initializers.push(SceneParticleStageRuntime {
            name: "velocityrandom".to_string(),
            fields: BTreeMap::from([
                ("min".to_string(), serde_json::json!("10 20 0")),
                ("max".to_string(), serde_json::json!("30 40 0")),
            ]),
        });
        scene.source.particle_runtimes = vec![runtime];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(!report.is_blocked());
        let config = &report.plan.sprite_particles[0].config;
        assert_eq!(config.spawn_origin, [14.0, 20.0]);
        assert_eq!(config.spawn_radius, [2.0, 16.0]);
        assert_eq!(config.size_range, [12.0, 24.0]);
        assert_eq!(config.velocity_range, Some([[20.0, 40.0], [60.0, 80.0]]));
    }

    #[test]
    fn sprite_material_bridge_rejects_phase10_texture_variants_without_trail_fallback() {
        let temp = tempdir().expect("temp dir");
        let managed_root = temp.path().join("managed");
        let builtin_root = temp.path().join("builtin");
        write_sprite_particle_material_fixture(&managed_root);
        fs::write(
            managed_root.join("source/materials/sprite.material"),
            r#"{"shader":"genericparticle","textures":["textures/sprite.png","textures/normal.png"],"blending":"translucent"}"#,
        )
        .expect("complex material");
        fs::create_dir_all(&builtin_root).expect("builtin dir");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed_root, &builtin_root);
        let mut scene = runtime_scene_with_objects(
            vec![(
                4,
                particle_object(4, "Sprite", SceneParticleKind::PetalTrail),
            )],
            vec![4],
        );
        scene.source.particle_runtimes = vec![supported_sprite_particle_runtime(4)];

        let report = build_scene_render_plan_with_resolver(&scene, Some(&resolver));

        assert!(report.plan.sprite_particles.is_empty());
        assert!(report.plan.particles.is_empty());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::UnsupportedParticleRuntime));
    }

    #[test]
    fn unsupported_particle_runtime_does_not_make_plan_renderable() {
        let mut scene = runtime_scene_with_objects(
            vec![(4, particle_object(4, "Trail", SceneParticleKind::LineTrail))],
            vec![4],
        );
        scene.source.particle_runtimes = vec![unsupported_particle_runtime(4)];

        let report = build_scene_render_plan(&scene);

        assert!(report.plan.particles.is_empty());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::UnsupportedParticleRuntime));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::NoRenderableVisuals));
        assert!(report.is_blocked());
    }

    #[test]
    fn render_plan_preserves_global_draw_order_across_typed_items() {
        let temp = tempdir().expect("temp dir");
        let texture_path = temp.path().join("hero.png");
        let sound_path = temp.path().join("loop.m4a");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255])))
            .save(&texture_path)
            .expect("save texture");
        std::fs::write(&sound_path, b"fake-audio").expect("sound fixture");

        let scene = runtime_scene_with_objects(
            vec![
                (
                    1,
                    visual_object(
                        1,
                        "Back",
                        SceneAssetKind::Image,
                        Some(texture_path.display().to_string()),
                        Some([0.0, 0.0, 100.0, 100.0]),
                        [1.0, 1.0, 1.0],
                        None,
                        None,
                    ),
                ),
                (
                    2,
                    text_object(
                        2,
                        "Clock",
                        [10.0, 10.0, 90.0, 30.0],
                        [12.0, 12.0, 80.0, 24.0],
                    ),
                ),
                (
                    3,
                    visual_object(
                        3,
                        "Front",
                        SceneAssetKind::Image,
                        Some(texture_path.display().to_string()),
                        Some([20.0, 20.0, 80.0, 80.0]),
                        [1.0, 1.0, 1.0],
                        None,
                        None,
                    ),
                ),
                (4, particle_object(4, "Trail", SceneParticleKind::LineTrail)),
                (5, audio_object(5, "Spectrum", [40.0, 40.0, 120.0, 64.0])),
                (
                    6,
                    sound_object(6, "Ambient", sound_path.display().to_string()),
                ),
                (
                    7,
                    visual_object(
                        7,
                        "Overlay",
                        SceneAssetKind::Image,
                        Some(texture_path.display().to_string()),
                        Some([30.0, 30.0, 60.0, 60.0]),
                        [1.0, 1.0, 1.0],
                        None,
                        None,
                    ),
                ),
            ],
            vec![1, 2, 3, 4, 5, 6, 7],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.visuals.len(), 3);
        assert_eq!(report.plan.texts.len(), 1);
        assert_eq!(report.plan.particles.len(), 1);
        assert_eq!(report.plan.audios.len(), 1);
        assert_eq!(report.plan.sounds.len(), 1);
        assert_eq!(
            report.plan.draw_order,
            vec![
                SceneRenderDrawItem {
                    object_id: 1,
                    kind: SceneRenderDrawKind::Visual,
                },
                SceneRenderDrawItem {
                    object_id: 2,
                    kind: SceneRenderDrawKind::Text,
                },
                SceneRenderDrawItem {
                    object_id: 3,
                    kind: SceneRenderDrawKind::Visual,
                },
                SceneRenderDrawItem {
                    object_id: 4,
                    kind: SceneRenderDrawKind::Particle,
                },
                SceneRenderDrawItem {
                    object_id: 5,
                    kind: SceneRenderDrawKind::Audio,
                },
                SceneRenderDrawItem {
                    object_id: 6,
                    kind: SceneRenderDrawKind::Sound,
                },
                SceneRenderDrawItem {
                    object_id: 7,
                    kind: SceneRenderDrawKind::Visual,
                },
            ]
        );
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
            draw_order: vec![SceneRenderDrawItem {
                object_id: 5,
                kind: SceneRenderDrawKind::Sound,
            }],
            visuals: Vec::new(),
            texts: Vec::new(),
            audios: Vec::new(),
            particles: Vec::new(),
            rope_particles: Vec::new(),
            sprite_particles: Vec::new(),
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
    fn render_plan_zeroes_mouse_influence_when_scene_parallax_is_disabled() {
        let mut scene = runtime_scene_with_objects(Vec::new(), Vec::new());
        scene.evaluated.camera.parallax_mouse_influence = 0.5;
        scene.evaluated.parallax = SceneParallax {
            enabled: false,
            amount: Some(0.5),
            delay: Some(0.1),
        };

        let report = build_scene_render_plan(&scene);

        assert_eq!(report.plan.camera.parallax_mouse_influence, 0.0);
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
    fn warning_only_skipped_visual_does_not_block_drawable_text_order() {
        let scene = runtime_scene_with_objects(
            vec![
                (
                    9,
                    visual_object(
                        9,
                        "Unsupported",
                        SceneAssetKind::Unsupported,
                        Some("/tmp/unsupported.bin".to_string()),
                        Some([0.0, 0.0, 100.0, 100.0]),
                        [1.0, 1.0, 1.0],
                        None,
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
            ],
            vec![9, 2],
        );

        let report = build_scene_render_plan(&scene);

        assert!(!report.is_blocked());
        assert_eq!(report.plan.visuals.len(), 0);
        assert_eq!(report.plan.texts.len(), 1);
        assert!(report.plan.has_renderable_output());
        assert!(report.issues.iter().any(|issue| issue.code
            == SceneRenderIssueCode::UnsupportedVisualAsset
            && issue.severity == SceneRenderIssueSeverity::Warning));
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.code == SceneRenderIssueCode::NoRenderableVisuals));
        assert_eq!(
            report.plan.draw_order,
            vec![SceneRenderDrawItem {
                object_id: 2,
                kind: SceneRenderDrawKind::Text,
            }]
        );
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
