use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WallpaperType {
    Scene,
    Video,
    Web,
    Application,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PropertyKind {
    Bool,
    Slider,
    Color,
    Combo,
    Textinput,
    Text,
    Group,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PropertyPresentation {
    #[default]
    Control,
    Group,
    Decoration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperProperty {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub markup: Option<String>,
    pub kind: PropertyKind,
    pub value: Value,
    pub default_value: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub condition: Option<String>,
    #[serde(default)]
    pub order: Option<u32>,
    #[serde(default)]
    pub presentation: PropertyPresentation,
    pub options: Vec<WallpaperOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PropertySectionItemKind {
    Property,
    Description,
    Separator,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropertySectionItem {
    pub kind: PropertySectionItemKind,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub markup: Option<String>,
    #[serde(default)]
    pub order: Option<u32>,
    #[serde(default)]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PropertySection {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub order: Option<u32>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub items: Vec<PropertySectionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneBinding {
    pub property_key: String,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneNodeState {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<u32>,
    #[serde(default)]
    pub parent_id: Option<u32>,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    #[serde(default)]
    pub position: [f64; 3],
    #[serde(default)]
    pub position_bindings: Option<SceneAxisBindings>,
    #[serde(default)]
    pub scale: [f64; 3],
    #[serde(default)]
    pub angles: Option<[f64; 3]>,
    #[serde(default)]
    pub rotation: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SceneTextBehavior {
    Static,
    Script,
    Clock,
    Date,
    Weekday,
    DayPeriod,
    Fps,
    MediaTitle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SceneNowPlayingAvailability {
    Available,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SceneNowPlayingState {
    #[default]
    Unavailable,
    Idle,
    PlayingWithoutTitle,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SceneNowPlayingDiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneNowPlayingDiagnostic {
    pub severity: SceneNowPlayingDiagnosticSeverity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneNowPlayingSnapshot {
    pub availability: SceneNowPlayingAvailability,
    pub state: SceneNowPlayingState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub generation: u64,
    pub updated_at: DateTime<Utc>,
    pub refresh_interval_millis: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SceneNowPlayingDiagnostic>,
}

impl Default for SceneNowPlayingSnapshot {
    fn default() -> Self {
        Self {
            availability: SceneNowPlayingAvailability::Unavailable,
            state: SceneNowPlayingState::Unavailable,
            title: None,
            artist: None,
            album: None,
            source: None,
            generation: 0,
            updated_at: Utc::now(),
            refresh_interval_millis: 0,
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SceneAssetKind {
    Image,
    Video,
    System,
    #[default]
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SceneParticleKind {
    LineTrail,
    PetalTrail,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneParallax {
    pub enabled: bool,
    pub amount: Option<f64>,
    pub delay: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneAxisBindings {
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneCamera {
    pub zoom: f64,
    #[serde(default)]
    pub center: [f64; 2],
    pub camera_shake: bool,
    pub camera_shake_amplitude: f64,
    pub camera_shake_speed: f64,
    pub parallax_mouse_influence: f64,
    #[serde(default)]
    pub zoom_binding: Option<String>,
    #[serde(default)]
    pub camera_shake_binding: Option<String>,
    #[serde(default)]
    pub parallax_mouse_influence_binding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneVisualLayer {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<u32>,
    #[serde(default)]
    pub parent_id: Option<u32>,
    #[serde(default)]
    pub alignment: Option<String>,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    pub position: [f64; 3],
    #[serde(default)]
    pub position_bindings: Option<SceneAxisBindings>,
    pub scale: [f64; 3],
    #[serde(default)]
    pub scale_binding: Option<String>,
    #[serde(default)]
    pub angles: Option<[f64; 3]>,
    #[serde(default)]
    pub size: Option<[f64; 2]>,
    #[serde(default)]
    pub intrinsic_size: Option<[f64; 2]>,
    #[serde(default)]
    pub parallax_depth: Option<[f64; 2]>,
    #[serde(default)]
    pub parallax_depth_binding: Option<String>,
    #[serde(default)]
    #[serde(alias = "angle")]
    pub rotation: Option<f64>,
    #[serde(default)]
    pub opacity: Option<f64>,
    #[serde(default)]
    pub opacity_binding: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub color_binding: Option<String>,
    #[serde(default)]
    pub brightness: Option<f64>,
    #[serde(default)]
    pub brightness_binding: Option<String>,
    #[serde(default)]
    pub color_blend_mode: Option<i64>,
    #[serde(default)]
    pub render_bounds: Option<[f64; 4]>,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub autosize: bool,
    #[serde(default)]
    pub solid_layer: bool,
    #[serde(default)]
    pub passthrough: bool,
    #[serde(default)]
    pub no_padding: bool,
    #[serde(default)]
    pub model_width: Option<f64>,
    #[serde(default)]
    pub model_height: Option<f64>,
    #[serde(default)]
    pub puppet_path: Option<String>,
    #[serde(default)]
    pub animation_layers: Vec<SceneAnimationLayer>,
    #[serde(default)]
    pub effect_instances: Vec<SceneVisualEffect>,
    #[serde(default)]
    pub model_path: Option<String>,
    #[serde(default)]
    pub material_path: Option<String>,
    #[serde(default)]
    pub shader_path: Option<String>,
    #[serde(default)]
    pub texture_names: Vec<String>,
    #[serde(default)]
    pub asset_kind: SceneAssetKind,
    #[serde(default, alias = "decodedPath", alias = "decoded_path")]
    pub asset_path: Option<String>,
    #[serde(default)]
    pub system_texture_key: Option<String>,
    #[serde(default)]
    pub blend_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneTextLayer {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<u32>,
    #[serde(default)]
    pub parent_id: Option<u32>,
    #[serde(default)]
    pub alignment: Option<String>,
    #[serde(default)]
    pub anchor: Option<String>,
    #[serde(default)]
    pub horizontal_align: Option<String>,
    #[serde(default)]
    pub vertical_align: Option<String>,
    pub content: String,
    pub behavior: SceneTextBehavior,
    #[serde(default)]
    pub delimiter: Option<String>,
    #[serde(default)]
    pub month_format: Option<String>,
    #[serde(default)]
    pub day_format: Option<String>,
    #[serde(default)]
    pub show_day: Option<bool>,
    #[serde(default)]
    pub align_vertical: Option<bool>,
    #[serde(default)]
    pub use_delimiter: Option<bool>,
    #[serde(default)]
    pub show_seconds: Option<bool>,
    #[serde(default)]
    pub use_24h_format: Option<bool>,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    #[serde(default)]
    pub text_binding: Option<String>,
    pub position: [f64; 3],
    #[serde(default)]
    pub position_bindings: Option<SceneAxisBindings>,
    pub scale: [f64; 3],
    #[serde(default)]
    pub scale_binding: Option<String>,
    #[serde(default)]
    pub angles: Option<[f64; 3]>,
    #[serde(default)]
    pub rotation: Option<f64>,
    #[serde(default)]
    pub size: Option<[f64; 2]>,
    #[serde(default)]
    pub render_bounds: Option<[f64; 4]>,
    #[serde(default)]
    pub parallax_depth: Option<[f64; 2]>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub color_binding: Option<String>,
    #[serde(default)]
    pub alpha: Option<f64>,
    #[serde(default)]
    pub alpha_binding: Option<String>,
    #[serde(default)]
    pub point_size: Option<f64>,
    #[serde(default)]
    pub point_size_binding: Option<String>,
    #[serde(default)]
    pub font_reference: Option<String>,
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default)]
    pub effect_paths: Vec<String>,
    #[serde(default)]
    pub script_text: Option<String>,
    #[serde(default)]
    pub script_refresh_interval_millis: Option<u64>,
    #[serde(default)]
    pub padding: Option<f64>,
    #[serde(default)]
    pub max_rows: Option<u32>,
    #[serde(default)]
    pub max_width: Option<f64>,
    #[serde(default)]
    pub limit_width: Option<bool>,
    #[serde(default)]
    pub limit_use_ellipsis: Option<bool>,
    #[serde(default)]
    pub block_align: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneAudioLayer {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<u32>,
    #[serde(default)]
    pub parent_id: Option<u32>,
    #[serde(default)]
    pub alignment: Option<String>,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    pub position: [f64; 3],
    #[serde(default)]
    pub position_bindings: Option<SceneAxisBindings>,
    pub scale: [f64; 3],
    #[serde(default)]
    pub scale_binding: Option<String>,
    #[serde(default)]
    pub angles: Option<[f64; 3]>,
    #[serde(default)]
    pub rotation: Option<f64>,
    #[serde(default)]
    pub size: Option<[f64; 2]>,
    #[serde(default)]
    pub render_bounds: Option<[f64; 4]>,
    #[serde(default)]
    pub angle: Option<f64>,
    pub bar_count: usize,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub bar_spacing: Option<f64>,
    #[serde(default)]
    pub bar_bounds: Option<[f64; 2]>,
    #[serde(default)]
    pub minimum_height: Option<f64>,
    #[serde(default)]
    pub radius: Option<f64>,
    #[serde(default)]
    pub volume_factor: Option<f64>,
    #[serde(default)]
    pub opacity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneSoundTrack {
    pub id: u32,
    pub name: String,
    pub asset_path: String,
    #[serde(default)]
    pub looped: bool,
    pub volume: f64,
    #[serde(default)]
    pub volume_binding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneParticleLayer {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<u32>,
    #[serde(default)]
    pub parent_id: Option<u32>,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    pub kind: SceneParticleKind,
    pub particle_path: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub color_binding: Option<String>,
    pub size: f64,
    #[serde(default)]
    pub size_binding: Option<String>,
    #[serde(default)]
    pub emission_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SceneLogicNodeKind {
    Container,
    Visual,
    Text,
    Audio,
    Particle,
    Sound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneLogicNode {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<u32>,
    pub kind: SceneLogicNodeKind,
    pub visible: bool,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub bindings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SceneRenderNodeKind {
    Container,
    Sprite,
    Video,
    Text,
    AudioReactive,
    Particle,
    Sound,
    SystemTexture,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneRenderNode {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<u32>,
    pub kind: SceneRenderNodeKind,
    pub visible: bool,
    #[serde(default)]
    pub asset_path: Option<String>,
    #[serde(default)]
    pub material_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneMaterialPass {
    pub owner_id: u32,
    #[serde(default)]
    pub material_path: Option<String>,
    #[serde(default)]
    pub shader_path: Option<String>,
    #[serde(default)]
    pub blend_mode: Option<String>,
    #[serde(default)]
    pub texture_names: Vec<String>,
    #[serde(default)]
    pub system_texture_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneAnimationLayer {
    pub id: i64,
    pub rate: f64,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    pub blend: String,
    pub animation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneVisualEffectPass {
    #[serde(default)]
    pub constants: BTreeMap<String, Value>,
    #[serde(default)]
    pub textures: Vec<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneVisualEffect {
    pub effect_path: String,
    pub visible: bool,
    #[serde(default)]
    pub visibility_binding: Option<SceneBinding>,
    #[serde(default)]
    pub passes: Vec<SceneVisualEffectPass>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneAudioSource {
    pub id: u32,
    pub name: String,
    pub source_type: String,
    #[serde(default)]
    pub asset_path: Option<String>,
    pub reactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneManifest {
    #[serde(default)]
    pub canvas_width: Option<f64>,
    #[serde(default)]
    pub canvas_height: Option<f64>,
    #[serde(default)]
    pub clear_color: Option<String>,
    #[serde(default)]
    pub camera: SceneCamera,
    #[serde(default)]
    pub parallax: SceneParallax,
    #[serde(default)]
    pub nodes: Vec<SceneNodeState>,
    #[serde(default)]
    pub primary_visual: Option<SceneVisualLayer>,
    #[serde(default)]
    pub visual_layers: Vec<SceneVisualLayer>,
    #[serde(default)]
    pub text_layers: Vec<SceneTextLayer>,
    #[serde(default)]
    pub audio_layers: Vec<SceneAudioLayer>,
    #[serde(default)]
    pub sound_tracks: Vec<SceneSoundTrack>,
    #[serde(default)]
    pub particle_layers: Vec<SceneParticleLayer>,
    #[serde(default)]
    pub logic_graph: Vec<SceneLogicNode>,
    #[serde(default)]
    pub render_graph: Vec<SceneRenderNode>,
    #[serde(default)]
    pub material_passes: Vec<SceneMaterialPass>,
    #[serde(default)]
    pub audio_sources: Vec<SceneAudioSource>,
    #[serde(default)]
    pub object_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedSceneTransform {
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub rotation: f64,
    #[serde(default)]
    pub render_bounds: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedSceneCamera {
    pub zoom: f64,
    pub center: [f64; 2],
    pub camera_shake: bool,
    pub camera_shake_amplitude: f64,
    pub camera_shake_speed: f64,
    pub parallax_mouse_influence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedTextStyle {
    #[serde(default)]
    pub color: Option<String>,
    pub alpha: f64,
    pub point_size: f64,
    #[serde(default)]
    pub font_path: Option<String>,
    #[serde(default)]
    pub effect_paths: Vec<String>,
    #[serde(default)]
    pub horizontal_align: Option<String>,
    #[serde(default)]
    pub vertical_align: Option<String>,
    #[serde(default)]
    pub padding: Option<f64>,
    #[serde(default)]
    pub max_rows: Option<u32>,
    #[serde(default)]
    pub max_width: Option<f64>,
    #[serde(default)]
    pub limit_width: Option<bool>,
    #[serde(default)]
    pub limit_use_ellipsis: Option<bool>,
    #[serde(default)]
    pub block_align: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedTextLayout {
    #[serde(default)]
    pub size: Option<[f64; 2]>,
    #[serde(default)]
    pub render_bounds: Option<[f64; 4]>,
    #[serde(default)]
    pub content_bounds: Option<[f64; 4]>,
    pub scaled_point_size: f64,
    pub scaled_padding: f64,
    pub world_scale: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedTextState {
    pub value: String,
    pub style: EvaluatedTextStyle,
    pub layout: EvaluatedTextLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_input_generation: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedAudioState {
    pub bar_count: usize,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub bar_spacing: Option<f64>,
    #[serde(default)]
    pub bar_bounds: Option<[f64; 2]>,
    #[serde(default)]
    pub minimum_height: Option<f64>,
    #[serde(default)]
    pub radius: Option<f64>,
    #[serde(default)]
    pub volume_factor: Option<f64>,
    #[serde(default)]
    pub opacity: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedSceneObjectBase {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<u32>,
    #[serde(default)]
    pub dependencies: Vec<u32>,
    pub visible: bool,
    #[serde(default)]
    pub alignment: Option<String>,
    pub opacity: f64,
    pub transform: EvaluatedSceneTransform,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EvaluatedSceneObject {
    Container {
        #[serde(flatten)]
        base: EvaluatedSceneObjectBase,
    },
    Visual {
        #[serde(flatten)]
        base: EvaluatedSceneObjectBase,
        asset_kind: SceneAssetKind,
        #[serde(default)]
        asset_path: Option<String>,
        #[serde(default)]
        system_texture_key: Option<String>,
        #[serde(default)]
        texture_names: Vec<String>,
        #[serde(default)]
        blend_mode: Option<String>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        brightness: Option<f64>,
        #[serde(default)]
        color_blend_mode: Option<i64>,
        #[serde(default)]
        parallax_depth: Option<[f64; 2]>,
        #[serde(default)]
        angles: Option<[f64; 3]>,
        #[serde(default)]
        fullscreen: bool,
        #[serde(default)]
        autosize: bool,
        #[serde(default)]
        solid_layer: bool,
        #[serde(default)]
        passthrough: bool,
        #[serde(default)]
        no_padding: bool,
        #[serde(default)]
        puppet_path: Option<String>,
        #[serde(default)]
        animation_layers: Vec<SceneAnimationLayer>,
        #[serde(default)]
        primary: bool,
        #[serde(default)]
        background_candidate: bool,
    },
    Text {
        #[serde(flatten)]
        base: EvaluatedSceneObjectBase,
        behavior: SceneTextBehavior,
        text: EvaluatedTextState,
    },
    Audio {
        #[serde(flatten)]
        base: EvaluatedSceneObjectBase,
        audio: EvaluatedAudioState,
    },
    Particle {
        #[serde(flatten)]
        base: EvaluatedSceneObjectBase,
        particle_path: String,
        particle_kind: SceneParticleKind,
        #[serde(default)]
        color: Option<String>,
        size: f64,
        emission_rate: f64,
    },
    Sound {
        #[serde(flatten)]
        base: EvaluatedSceneObjectBase,
        asset_path: String,
        looped: bool,
        volume: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneEvaluatedDocument {
    pub canvas_width: f64,
    pub canvas_height: f64,
    #[serde(default)]
    pub clear_color: Option<String>,
    pub camera: EvaluatedSceneCamera,
    pub parallax: SceneParallax,
    pub objects: BTreeMap<u32, EvaluatedSceneObject>,
    pub render_list: Vec<u32>,
    pub evaluated_at: DateTime<Utc>,
}

impl Default for SceneEvaluatedDocument {
    fn default() -> Self {
        Self {
            canvas_width: 3840.0,
            canvas_height: 2160.0,
            clear_color: None,
            camera: EvaluatedSceneCamera {
                zoom: 1.0,
                center: [0.0, 0.0],
                camera_shake: false,
                camera_shake_amplitude: 1.0,
                camera_shake_speed: 1.0,
                parallax_mouse_influence: 0.3,
            },
            parallax: SceneParallax::default(),
            objects: BTreeMap::new(),
            render_list: Vec::new(),
            evaluated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneRuntimeDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_owner_key: Option<String>,
    pub source: SceneManifest,
    pub evaluated: SceneEvaluatedDocument,
    #[serde(default)]
    pub now_playing: SceneNowPlayingSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoRuntimeDocument {
    #[serde(default)]
    pub entry_path: Option<String>,
    #[serde(default)]
    pub preview_path: Option<String>,
    pub source_path: String,
    pub managed_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebRuntimeDocument {
    #[serde(default)]
    pub entry_path: Option<String>,
    #[serde(default)]
    pub preview_path: Option<String>,
    pub source_path: String,
    pub managed_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WallpaperRuntime {
    Scene { scene: SceneRuntimeDocument },
    Video { video: VideoRuntimeDocument },
    Web { web: WebRuntimeDocument },
    Application,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperRuntimeRecord {
    pub id: String,
    pub title: String,
    pub wallpaper_type: WallpaperType,
    pub source_path: String,
    pub managed_path: String,
    #[serde(default)]
    pub preview_path: Option<String>,
    #[serde(default)]
    pub entry_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_snapshot_path: Option<String>,
    #[serde(default)]
    pub property_schema: Vec<WallpaperProperty>,
    #[serde(default)]
    pub property_sections: Vec<PropertySection>,
    pub imported_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub runtime: WallpaperRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperRecord {
    pub id: String,
    pub title: String,
    pub wallpaper_type: WallpaperType,
    pub source_path: String,
    pub managed_path: String,
    pub preview_path: Option<String>,
    pub entry_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_snapshot_path: Option<String>,
    pub property_schema: Vec<WallpaperProperty>,
    #[serde(default)]
    pub property_sections: Vec<PropertySection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_cache: Option<SceneCacheMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_manifest: Option<SceneManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_manifest_version: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub scene_manifest_dirty: bool,
    pub imported_at: DateTime<Utc>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStore {
    pub wallpapers: Vec<WallpaperRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneRuntimeSettings {
    #[serde(default)]
    pub external_assets_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneRuntimeSettingsSnapshot {
    #[serde(default)]
    pub external_assets_path: Option<String>,
    pub external_assets_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneCacheMetadata {
    pub source_fingerprint: String,
    pub parser_revision: String,
    pub evaluator_revision: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRuntimeState {
    pub active: Option<WallpaperRuntimeRecord>,
    pub paused: bool,
}
