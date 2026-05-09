use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    ffi::c_void,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{
    models::{
        SceneNowPlayingDiagnostic, SceneNowPlayingDiagnosticSeverity, SceneRuntimeDocument,
        WallpaperRuntimeRecord,
    },
    services::{
        audio_input_service, diagnostic_service, input_service,
        runtime_audio_settings_service::{
            effective_output_volume, normalize_output_volume_percent,
        },
        scene_audio_coordinator_service::SceneAudioCoordinator,
        scene_diagnostics::{SceneDiagnosticDetail, SceneDiagnosticDomain},
        scene_input_response_service::{
            project_shared_input_to_scene, project_shared_input_to_scene_with_cover,
            SceneInputCoordinator, SceneInputCoordinatorFrame, SceneInputCoordinatorUpdate,
            SceneInputResponse, SceneInputSceneBounds, SceneInputViewport,
        },
        scene_mdl_service::{evaluate_scene_mdl_mesh, parse_scene_mdl_file, SceneMdlDocument},
        scene_now_playing_provider_service,
        scene_particle_scheduler_service::{
            SceneParticleCursor, SceneParticlePrimitive, SceneParticleScheduler,
            SceneRopeParticleScheduler,
        },
        scene_render_graph_service::{
            build_scene_phase10_graph, ScenePhase10EffectPassNode, ScenePhase10GraphPlan,
            ScenePhase10InputSource, ScenePhase10VisualPlan,
        },
        scene_render_planner_service::{
            build_scene_render_plan_with_resolver, build_scene_render_text_update_with_resolver,
            SceneClearColor, SceneRenderAudioItem, SceneRenderBlendMode, SceneRenderColor,
            SceneRenderDrawItem, SceneRenderDrawKind, SceneRenderIssue, SceneRenderParticleItem,
            SceneRenderPlan, SceneRenderQuad, SceneRenderRopeParticleItem, SceneRenderSoundItem,
            SceneRenderSourceKind, SceneRenderSpriteParticleItem, SceneRenderTextItem,
            SceneRenderVisualItem,
            SceneTextHorizontalAlign,
        },
        scene_resource_service::{builtin_scene_assets_root_for_app, SceneResourceResolver},
        scene_runtime_settings_service,
        scene_shader_material_service::{
            load_shader_program_source, merged_shader_defines, phase10b_effect_contract_for_kind,
            SceneCompatEffectKind, SceneMaterialPassPlan, SceneMaterialTextureBinding,
            SceneMaterialUniformValue, SceneShaderProgram, SceneShaderProgramKind,
        },
        scene_sound_lifecycle_service::{
            plan_scene_sound_clear, plan_scene_sound_lifecycle, scene_sound_meter_level,
            scene_sound_playback_load_warning, scene_sound_reactive_levels,
            SceneSoundLifecycleAction, SceneSoundMeterReading, SceneSoundPlaybackWarning,
            SceneSoundRuntimeState,
        },
        scene_sprite_particle_scheduler_service::{
            SceneSpriteParticlePrimitive, SceneSpriteParticleScheduler,
        },
        scene_text_script_runtime_service,
        scene_video_texture_service::{
            self, SceneVideoTextureLifecycleAction, SceneVideoTextureSourceSpec,
            SceneVideoTextureSourceState, SceneVideoTextureWarning,
        },
        window_service,
    },
};

#[cfg(target_os = "macos")]
use std::{cell::RefCell, ptr::NonNull, time::Instant};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use image::DynamicImage;
#[cfg(target_os = "macos")]
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, ProtocolObject},
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSGraphicsContext, NSImageInterpolation, NSLineBreakMode,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStringDrawingContext,
    NSStringDrawingOptions, NSStringNSExtendedStringDrawing, NSTextAlignment, NSView,
};
#[cfg(target_os = "macos")]
use objc2_av_foundation::{
    AVPlayer, AVPlayerActionAtItemEnd, AVPlayerItem, AVPlayerItemStatus, AVPlayerItemVideoOutput,
};
#[cfg(target_os = "macos")]
use objc2_avf_audio::AVAudioPlayer;
#[cfg(target_os = "macos")]
use objc2_core_foundation::{CFArray, CFRetained, CFString, CGPoint, CGRect, CGSize};
#[cfg(target_os = "macos")]
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo,
};
#[cfg(target_os = "macos")]
use objc2_core_media::CMTime;
#[cfg(target_os = "macos")]
use objc2_core_text::{CTFontDescriptor, CTFontManagerCreateFontDescriptorsFromURL};
#[cfg(target_os = "macos")]
use objc2_core_video::{
    kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess, CVMetalTexture, CVMetalTextureCache,
    CVMetalTextureGetTexture, CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{
    ns_string, NSAttributedStringKey, NSDictionary, NSNumber, NSObjectProtocol, NSPoint, NSRect,
    NSSize, NSString, NSURL,
};
#[cfg(target_os = "macos")]
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineColorAttachmentDescriptor, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLResourceOptions, MTLStorageMode, MTLStoreAction, MTLTexture,
    MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};
#[cfg(target_os = "macos")]
use objc2_metal_kit::{MTKView, MTKViewDelegate};
#[cfg(target_os = "macos")]
use objc2_quartz_core::CACurrentMediaTime;

const DIAGNOSTIC_SUBSYSTEM: &str = "native-scene";
const SYNC_FAILED_CODE: &str = "sync-failed";
const AUDIO_INPUT_UNAVAILABLE_CODE: &str = "audio-input-unavailable";
const INPUT_SNAPSHOT_UNAVAILABLE_CODE: &str = "input-snapshot-unavailable";
#[cfg(target_os = "macos")]
const WHITE_TEXTURE_KEY: &str = "procedural:white";
#[cfg(target_os = "macos")]
const PETAL_TEXTURE_KEY: &str = "procedural:petal";
#[cfg(target_os = "macos")]
const INLINE_VERTEX_BYTES_LIMIT: usize = 4096;

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct SceneCachedTextFont {
    font: Retained<NSFont>,
    fallback_detail: Option<SceneTextFontFallbackDetail>,
}

#[cfg(target_os = "macos")]
thread_local! {
    static SCENE_TEXT_FONT_CACHE: RefCell<BTreeMap<String, SceneCachedTextFont>> = const {
        RefCell::new(BTreeMap::new())
    };
}

#[cfg(target_os = "macos")]
const SCENE_SHADER_SOURCE: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct SceneVertex {
    packed_float2 position;
    packed_float2 uv;
    packed_float4 color;
    float opacity;
};

struct SceneRasterizerData {
    float4 position [[position]];
    float2 uv;
    float4 color;
    float opacity;
};

vertex SceneRasterizerData scene_vertex(
    uint vertex_id [[vertex_id]],
    constant SceneVertex *vertices [[buffer(0)]]
) {
    SceneRasterizerData raster_data;
    SceneVertex vertex_input = vertices[vertex_id];
    raster_data.position = float4(float2(vertex_input.position), 0.0, 1.0);
    raster_data.uv = float2(vertex_input.uv);
    raster_data.color = float4(vertex_input.color);
    raster_data.opacity = vertex_input.opacity;
    return raster_data;
}

fragment float4 scene_fragment(
    SceneRasterizerData raster_input [[stage_in]],
    texture2d<float> color_texture [[texture(0)]]
) {
    constexpr sampler texture_sampler(address::clamp_to_edge, filter::linear);
    float4 sampled_color = color_texture.sample(texture_sampler, raster_input.uv);
    sampled_color *= raster_input.color;
    sampled_color.a *= saturate(raster_input.opacity);
    return saturate(sampled_color);
}
"#;

pub struct NativeSceneRendererServiceState {
    runtime: Mutex<NativeSceneRendererRuntime>,
    audio_output_volume: Mutex<f64>,
    #[cfg(target_os = "macos")]
    soundscape: Mutex<Option<MainThreadBound<NativeSceneSoundscape>>>,
}

impl Default for NativeSceneRendererServiceState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(NativeSceneRendererRuntime::default()),
            audio_output_volume: Mutex::new(1.0),
            #[cfg(target_os = "macos")]
            soundscape: Mutex::new(None),
        }
    }
}

#[derive(Default)]
struct NativeSceneRendererRuntime {
    spec: Option<SceneRendererSpec>,
    views: BTreeMap<String, Arc<NativeSceneViewHandle>>,
}

#[derive(Debug, Clone, PartialEq)]
struct SceneRendererSpec {
    wallpaper_id: String,
    render_plan: SceneRenderPlan,
    phase10_graph: ScenePhase10GraphPlan,
    window_labels: Vec<String>,
    paused: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeSceneRendererSnapshot {
    spec: Option<SceneRendererSpec>,
    labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct NativeSceneRendererPlan {
    session: SceneSessionPlan,
    ensure_labels: Vec<String>,
    remove_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum SceneSessionPlan {
    Keep,
    Stop,
    Start { spec: SceneRendererSpec },
    Replace { spec: SceneRendererSpec },
    UpdateScene { spec: SceneRendererSpec },
}

struct NativeSceneRuntimeActions {
    spec: Option<SceneRendererSpec>,
    teardown_views: Vec<Arc<NativeSceneViewHandle>>,
    sync_views: Vec<(String, Arc<NativeSceneViewHandle>)>,
    create_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NativeSceneWarning {
    code: String,
    message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<SceneDiagnosticDetail>,
}

struct DesiredSceneRendererSpec {
    spec: Option<SceneRendererSpec>,
    warnings: Vec<NativeSceneWarning>,
}

pub fn sync_native_scene_runtime(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    let result = (|| -> Result<Vec<NativeSceneWarning>, String> {
        let Some(state) = app.try_state::<NativeSceneRendererServiceState>() else {
            return Ok(Vec::new());
        };

        let desired = desired_scene_renderer_spec(app, runtime_record, paused)?;
        let mut warnings = desired.warnings;
        let actions = {
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            let plan =
                plan_native_scene_renderer_runtime(&runtime.snapshot(), desired.spec.as_ref());
            prepare_runtime_actions(&mut runtime, plan)
        };
        warnings.extend(execute_runtime_actions(app, &state, actions)?);
        warnings.extend(sync_scene_soundscape(app, &state, desired.spec.as_ref())?);
        sync_scene_audio_interest(app, desired.spec.as_ref())?;
        dedup_native_warnings(&mut warnings);
        Ok(warnings)
    })();

    match &result {
        Ok(warnings) => {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, SYNC_FAILED_CODE);
            record_render_warnings(app, warnings);
        }
        Err(error) => {
            clear_native_scene_warning_diagnostics(app);
            let _ = diagnostic_service::record_error(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                SYNC_FAILED_CODE,
                "Native Scene runtime failed to sync.",
                Some(error.clone()),
            );
        }
    }

    result.map(|_| ())
}

pub fn set_scene_audio_output_volume(app: &AppHandle, volume: f64) -> Result<(), String> {
    let normalized = normalize_output_volume_percent(volume);
    let Some(state) = app.try_state::<NativeSceneRendererServiceState>() else {
        return Ok(());
    };

    {
        let mut current = state
            .audio_output_volume
            .lock()
            .map_err(|error| error.to_string())?;
        *current = normalized;
    }

    #[cfg(target_os = "macos")]
    {
        run_on_main(|mtm| {
            let slot = state.soundscape.lock().map_err(|error| error.to_string())?;
            if let Some(soundscape) = slot.as_ref() {
                soundscape.get(mtm).set_output_volume(normalized);
            }
            Ok::<(), String>(())
        })?;
    }

    Ok(())
}

pub fn update_native_scene_dynamic_text(
    app: &AppHandle,
    runtime_record: &WallpaperRuntimeRecord,
) -> Result<(), String> {
    let result = (|| -> Result<Vec<NativeSceneWarning>, String> {
        let Some(state) = app.try_state::<NativeSceneRendererServiceState>() else {
            return Ok(Vec::new());
        };
        let scene = match &runtime_record.runtime {
            crate::models::WallpaperRuntime::Scene { scene } => scene,
            _ => return Ok(Vec::new()),
        };

        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &runtime_record.managed_path,
            builtin_scene_assets_root_for_app(app),
            scene_runtime_settings_service::external_assets_root_for_app(app),
        );
        let report = build_scene_render_text_update_with_resolver(scene, Some(&resolver));
        let mut warnings = report
            .issues
            .into_iter()
            .map(NativeSceneWarning::from_render_issue)
            .collect::<Vec<_>>();
        warnings.extend(text_script_runtime_warnings_for_scene(scene));
        warnings.extend(now_playing_runtime_warnings_for_scene(scene));

        let (views, paused) = {
            let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            let Some(spec) = runtime.spec.as_ref() else {
                return Err(
                    "native Scene dynamic text update skipped because no Scene is active"
                        .to_string(),
                );
            };
            if spec.wallpaper_id != runtime_record.id {
                return Err(format!(
                    "native Scene dynamic text update targeted {}, but active native Scene is {}",
                    runtime_record.id, spec.wallpaper_id
                ));
            }
            (
                runtime
                    .views
                    .iter()
                    .map(|(label, view)| (label.clone(), Arc::clone(view)))
                    .collect::<Vec<_>>(),
                spec.paused,
            )
        };

        for (label, view) in views {
            warnings.extend(view.update_dynamic_text(app, &label, &report.texts, paused)?);
        }

        {
            let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
            if let Some(spec) = runtime.spec.as_mut() {
                if spec.wallpaper_id == runtime_record.id {
                    spec.render_plan.texts = report.texts.clone();
                }
            }
        }

        dedup_native_warnings(&mut warnings);
        Ok(warnings)
    })();

    match &result {
        Ok(warnings) => {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, SYNC_FAILED_CODE);
            record_dynamic_text_warnings(app, warnings);
        }
        Err(error) => {
            let _ = diagnostic_service::record_warning(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                "dynamic-text-update-failed",
                "Native Scene dynamic text update failed; the caller may fall back to a full sync.",
                Some(error.clone()),
            );
        }
    }

    result.map(|_| ())
}

fn desired_scene_renderer_spec(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<DesiredSceneRendererSpec, String> {
    let Some(record) = runtime_record else {
        return Ok(DesiredSceneRendererSpec {
            spec: None,
            warnings: Vec::new(),
        });
    };
    let scene = match &record.runtime {
        crate::models::WallpaperRuntime::Scene { scene } => scene,
        _ => {
            return Ok(DesiredSceneRendererSpec {
                spec: None,
                warnings: Vec::new(),
            })
        }
    };

    let mut labels = window_service::player_window_labels(app);
    labels.sort();
    labels.dedup();
    if labels.is_empty() {
        return Ok(DesiredSceneRendererSpec {
            spec: None,
            warnings: Vec::new(),
        });
    }

    let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
        &record.managed_path,
        builtin_scene_assets_root_for_app(app),
        scene_runtime_settings_service::external_assets_root_for_app(app),
    );
    let plan_report = build_scene_render_plan_with_resolver(scene, Some(&resolver));
    if plan_report.is_blocked() {
        let preview = plan_report
            .fatal_errors()
            .into_iter()
            .take(4)
            .map(|issue| issue.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "Scene native renderer could not build a phase-10 native scene plan: {preview}"
        ));
    }

    let graph_report = build_scene_phase10_graph(scene, &resolver);
    if graph_report.is_blocked() {
        let preview = graph_report
            .fatal_errors()
            .into_iter()
            .take(4)
            .map(|issue| issue.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "Scene native renderer could not build a phase-10 model/material graph: {preview}"
        ));
    }

    let mut warnings = plan_report
        .warnings()
        .into_iter()
        .map(NativeSceneWarning::from_render_issue)
        .collect::<Vec<_>>();
    warnings.extend(text_script_runtime_warnings_for_scene(scene));
    warnings.extend(now_playing_runtime_warnings_for_scene(scene));
    warnings.extend(text_font_runtime_warnings_for_plan(&plan_report.plan));
    warnings.extend(
        graph_report
            .warnings()
            .into_iter()
            .map(NativeSceneWarning::from_graph_issue),
    );

    Ok(DesiredSceneRendererSpec {
        spec: Some(SceneRendererSpec {
            wallpaper_id: record.id.clone(),
            render_plan: plan_report.plan,
            phase10_graph: graph_report.graph,
            window_labels: labels,
            paused,
        }),
        warnings,
    })
}

fn plan_native_scene_renderer_runtime(
    current: &NativeSceneRendererSnapshot,
    desired: Option<&SceneRendererSpec>,
) -> NativeSceneRendererPlan {
    let current_labels = current.labels.iter().cloned().collect::<BTreeSet<_>>();
    let desired_labels = desired
        .map(|spec| spec.window_labels.iter().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();

    let ensure_labels = desired_labels.iter().cloned().collect::<Vec<_>>();

    let session = match (current.spec.as_ref(), desired) {
        (None, None) => SceneSessionPlan::Keep,
        (Some(_), None) => SceneSessionPlan::Stop,
        (None, Some(spec)) => SceneSessionPlan::Start { spec: spec.clone() },
        (Some(current_spec), Some(spec)) if current_spec.wallpaper_id != spec.wallpaper_id => {
            SceneSessionPlan::Replace { spec: spec.clone() }
        }
        (Some(current_spec), Some(spec)) if current_spec != spec => {
            SceneSessionPlan::UpdateScene { spec: spec.clone() }
        }
        (Some(_), Some(_)) => SceneSessionPlan::Keep,
    };
    let remove_labels = match session {
        SceneSessionPlan::Replace { .. } => current_labels.iter().cloned().collect::<Vec<_>>(),
        _ => current_labels
            .difference(&desired_labels)
            .cloned()
            .collect::<Vec<_>>(),
    };

    NativeSceneRendererPlan {
        session,
        ensure_labels,
        remove_labels,
    }
}

fn prepare_runtime_actions(
    runtime: &mut NativeSceneRendererRuntime,
    plan: NativeSceneRendererPlan,
) -> NativeSceneRuntimeActions {
    let mut teardown_views = Vec::new();
    for label in &plan.remove_labels {
        if let Some(view) = runtime.views.remove(label) {
            teardown_views.push(view);
        }
    }

    match plan.session {
        SceneSessionPlan::Keep => {}
        SceneSessionPlan::Stop => {
            for label in runtime.views.keys().cloned().collect::<Vec<_>>() {
                if let Some(view) = runtime.views.remove(&label) {
                    teardown_views.push(view);
                }
            }
            runtime.spec = None;
            return NativeSceneRuntimeActions {
                spec: None,
                teardown_views,
                sync_views: vec![],
                create_labels: vec![],
            };
        }
        SceneSessionPlan::Start { spec }
        | SceneSessionPlan::Replace { spec }
        | SceneSessionPlan::UpdateScene { spec } => {
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

    NativeSceneRuntimeActions {
        spec,
        teardown_views,
        sync_views,
        create_labels,
    }
}

fn execute_runtime_actions(
    app: &AppHandle,
    state: &NativeSceneRendererServiceState,
    actions: NativeSceneRuntimeActions,
) -> Result<Vec<NativeSceneWarning>, String> {
    for view in &actions.teardown_views {
        view.teardown();
    }

    let Some(spec) = actions.spec else {
        return Ok(Vec::new());
    };

    let mut warnings = Vec::new();
    for (label, view) in &actions.sync_views {
        warnings.extend(view.sync(app, label, &spec)?);
    }

    for label in &actions.create_labels {
        let view = Arc::new(NativeSceneViewHandle::create(
            app,
            spec.render_plan.clear_color,
        )?);
        warnings.extend(view.sync(app, label, &spec)?);

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

    warnings.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then(left.message.cmp(&right.message))
    });
    warnings.dedup();
    Ok(warnings)
}

impl NativeSceneRendererRuntime {
    fn snapshot(&self) -> NativeSceneRendererSnapshot {
        NativeSceneRendererSnapshot {
            spec: self.spec.clone(),
            labels: self.views.keys().cloned().collect(),
        }
    }
}

impl NativeSceneWarning {
    fn from_render_issue(issue: SceneRenderIssue) -> Self {
        let code = format!("{:?}", issue.code);
        let code = kebab_case_diagnostic_code(&code);
        let domain = match issue.object_kind.as_deref() {
            Some("text") => SceneDiagnosticDomain::Text,
            Some("audio") => SceneDiagnosticDomain::Audio,
            Some("sound") => SceneDiagnosticDomain::Sound,
            Some("particle") => SceneDiagnosticDomain::Particle,
            _ => SceneDiagnosticDomain::Visual,
        };

        Self {
            code,
            message: issue.message,
            detail: issue
                .detail
                .map(|detail| SceneDiagnosticDetail::capability(domain, detail)),
        }
    }

    fn from_graph_issue(
        issue: crate::services::scene_render_graph_service::SceneGraphIssue,
    ) -> Self {
        Self {
            code: kebab_case_diagnostic_code(&format!("{:?}", issue.code)),
            message: issue.message,
            detail: issue.detail.map(|detail| {
                SceneDiagnosticDetail::capability(SceneDiagnosticDomain::Visual, detail)
            }),
        }
    }

    #[cfg(target_os = "macos")]
    fn texture_load(path: &Path, message: String) -> Self {
        Self {
            code: "visual-texture-load-failed".to_string(),
            message: format!(
                "Scene texture {} could not be prepared for Metal.",
                path.display()
            ),
            detail: Some(SceneDiagnosticDetail::runtime(
                SceneDiagnosticDomain::Visual,
                "texture-upload",
                message,
            )),
        }
    }

    #[cfg(target_os = "macos")]
    fn from_sound_playback_warning(warning: SceneSoundPlaybackWarning) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
            detail: Some(
                SceneDiagnosticDetail::runtime(
                    SceneDiagnosticDomain::Sound,
                    warning.runtime_stage,
                    warning.reason,
                )
                .with_note(format!(
                    "object {}: {}",
                    warning.object_id, warning.object_name
                ))
                .with_note(format!("asset path: {}", warning.asset_path.display())),
            ),
        }
    }

    #[cfg(target_os = "macos")]
    fn text_font_fallback(item: &SceneRenderTextItem, detail: SceneTextFontFallbackDetail) -> Self {
        let mut diagnostic = SceneDiagnosticDetail::runtime(
            SceneDiagnosticDomain::Text,
            "text-font-resolution",
            detail.reason,
        );
        if let Some(reference) = item.font.authored_reference.as_deref() {
            diagnostic = diagnostic.with_note(format!("authored font reference: {reference}"));
        }
        if let Some(reference_kind) = item.font.reference_kind {
            diagnostic = diagnostic.with_note(format!("font reference kind: {reference_kind:?}"));
        }
        if !detail.attempted_files.is_empty() {
            diagnostic = diagnostic.with_note(format!(
                "attempted font files: {}",
                detail.attempted_files.join(", ")
            ));
        }
        if !detail.family_candidates.is_empty() {
            diagnostic = diagnostic.with_note(format!(
                "family candidates: {}",
                detail.family_candidates.join(", ")
            ));
        }

        Self {
            code: "text-font-fallback".to_string(),
            message: format!(
                "Scene text {} fell back to the system font during native rasterization.",
                item.object_name
            ),
            detail: Some(diagnostic),
        }
    }

    #[cfg(target_os = "macos")]
    fn unsupported_text_effect(object_name: &str, effect_path: &str) -> Self {
        Self {
            code: "text-effect-unsupported".to_string(),
            message: format!(
                "Scene text {} uses unsupported native text effect {}.",
                object_name, effect_path
            ),
            detail: Some(
                SceneDiagnosticDetail::capability(
                    SceneDiagnosticDomain::Text,
                    "Phase-09 keeps blur support in the text raster path, but deeper effect/material semantics still stay outside the native text baseline.",
                )
                .with_note(format!("effect path: {effect_path}")),
            ),
        }
    }

    #[cfg(target_os = "macos")]
    fn phase10_draw(object_name: &str, detail: String) -> Self {
        Self {
            code: "visual-draw-preparation-failed".to_string(),
            message: format!(
                "Scene visual {} could not be prepared for phase-10 native drawing.",
                object_name
            ),
            detail: Some(SceneDiagnosticDetail::runtime(
                SceneDiagnosticDomain::Visual,
                "phase10-draw",
                detail,
            )),
        }
    }

    fn audio_input_unavailable() -> Self {
        Self {
            code: AUDIO_INPUT_UNAVAILABLE_CODE.to_string(),
            message:
                "Scene audio layers are falling back to silence because shared audio capture is unavailable."
                    .to_string(),
            detail: Some(
                SceneDiagnosticDetail::runtime(
                    SceneDiagnosticDomain::Audio,
                    "shared-audio",
                    "Shared audio capture is unavailable for Scene audio-reactive layers.",
                )
                .with_underlying_diagnostic("shared-audio/capture-unavailable"),
            ),
        }
    }

    fn input_snapshot_unavailable() -> Self {
        Self {
            code: INPUT_SNAPSHOT_UNAVAILABLE_CODE.to_string(),
            message:
                "Scene input-driven motion is using a neutral fallback because the shared input snapshot is unavailable."
                    .to_string(),
            detail: Some(SceneDiagnosticDetail::runtime(
                SceneDiagnosticDomain::Input,
                "shared-input",
                "Shared input did not provide a usable desktop snapshot for cursor/parallax-driven Scene effects.",
            )),
        }
    }

    fn now_playing_provider(diagnostic: &SceneNowPlayingDiagnostic) -> Self {
        let detail_text = diagnostic
            .detail
            .as_deref()
            .unwrap_or(diagnostic.message.as_str());
        Self {
            code: format!("now-playing-{}", diagnostic.code),
            message: diagnostic.message.clone(),
            detail: Some(
                SceneDiagnosticDetail::runtime(
                    SceneDiagnosticDomain::Text,
                    "now-playing-provider",
                    detail_text,
                )
                .with_underlying_diagnostic(format!("now-playing/{}", diagnostic.code)),
            ),
        }
    }

    fn from_video_texture_warning(warning: SceneVideoTextureWarning) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
            detail: warning.detail,
        }
    }

    fn detail_json(&self) -> Option<String> {
        serde_json::to_string_pretty(self).ok()
    }
}

fn kebab_case_diagnostic_code(value: &str) -> String {
    value
        .chars()
        .enumerate()
        .flat_map(|(index, character)| {
            if character.is_ascii_uppercase() && index > 0 {
                ['-', character.to_ascii_lowercase()]
            } else {
                ['\0', character.to_ascii_lowercase()]
            }
        })
        .filter(|character| *character != '\0')
        .collect::<String>()
}

fn record_render_warnings(app: &AppHandle, warnings: &[NativeSceneWarning]) {
    let existing_codes = diagnostic_service::current_runtime_diagnostics(app)
        .unwrap_or_default()
        .into_iter()
        .filter(|diagnostic| {
            diagnostic.subsystem == DIAGNOSTIC_SUBSYSTEM && diagnostic.code != SYNC_FAILED_CODE
        })
        .map(|diagnostic| diagnostic.code)
        .collect::<BTreeSet<_>>();

    let mut grouped = BTreeMap::<String, Vec<&NativeSceneWarning>>::new();
    for warning in warnings {
        grouped
            .entry(warning.code.clone())
            .or_default()
            .push(warning);
    }

    let next_codes = grouped.keys().cloned().collect::<BTreeSet<_>>();
    for stale in existing_codes.difference(&next_codes) {
        let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, stale);
    }

    for (code, entries) in grouped {
        let preview = entries
            .iter()
            .take(3)
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let remaining = entries.len().saturating_sub(3);
        let summary = if remaining == 0 {
            preview
        } else {
            format!("{preview}; and {remaining} more warning(s)")
        };
        let detail = serde_json::to_string_pretty(
            &entries
                .into_iter()
                .cloned()
                .collect::<Vec<NativeSceneWarning>>(),
        )
        .ok();
        let _ =
            diagnostic_service::record_warning(app, DIAGNOSTIC_SUBSYSTEM, &code, summary, detail);
    }
}

fn record_dynamic_text_warnings(app: &AppHandle, warnings: &[NativeSceneWarning]) {
    let existing_codes = diagnostic_service::current_runtime_diagnostics(app)
        .unwrap_or_default()
        .into_iter()
        .filter(|diagnostic| {
            diagnostic.subsystem == DIAGNOSTIC_SUBSYSTEM
                && is_dynamic_text_warning_code(&diagnostic.code)
        })
        .map(|diagnostic| diagnostic.code)
        .collect::<BTreeSet<_>>();

    let mut grouped = BTreeMap::<String, Vec<&NativeSceneWarning>>::new();
    for warning in warnings
        .iter()
        .filter(|warning| is_dynamic_text_warning_code(&warning.code))
    {
        grouped
            .entry(warning.code.clone())
            .or_default()
            .push(warning);
    }

    let next_codes = grouped.keys().cloned().collect::<BTreeSet<_>>();
    for stale in existing_codes.difference(&next_codes) {
        let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, stale);
    }

    for (code, entries) in grouped {
        let preview = entries
            .iter()
            .take(3)
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let remaining = entries.len().saturating_sub(3);
        let summary = if remaining == 0 {
            preview
        } else {
            format!("{preview}; and {remaining} more warning(s)")
        };
        let detail = serde_json::to_string_pretty(
            &entries
                .into_iter()
                .cloned()
                .collect::<Vec<NativeSceneWarning>>(),
        )
        .ok();
        let _ =
            diagnostic_service::record_warning(app, DIAGNOSTIC_SUBSYSTEM, &code, summary, detail);
    }
}

fn is_dynamic_text_warning_code(code: &str) -> bool {
    code == "missing-render-bounds"
        || code == "text-raster-failed"
        || code == "text-font-fallback"
        || code == "text-effect-unsupported"
        || code == "dynamic-text-update-failed"
        || code.starts_with("now-playing-")
}

fn dedup_native_warnings(warnings: &mut Vec<NativeSceneWarning>) {
    let mut ordered = Vec::new();
    for warning in warnings.drain(..) {
        if !ordered.iter().any(|existing| existing == &warning) {
            ordered.push(warning);
        }
    }
    *warnings = ordered;
}

fn text_script_runtime_warnings_for_scene(scene: &SceneRuntimeDocument) -> Vec<NativeSceneWarning> {
    scene_text_script_runtime_service::scene_text_script_runtime_diagnostics(
        scene.runtime_owner_key.as_deref(),
    )
    .into_iter()
    .map(|diagnostic| NativeSceneWarning {
        code: diagnostic.code,
        message: diagnostic.message,
        detail: Some(SceneDiagnosticDetail::runtime(
            SceneDiagnosticDomain::Text,
            diagnostic.runtime_stage,
            diagnostic.reason,
        )),
    })
    .collect()
}

fn now_playing_runtime_warnings_for_scene(scene: &SceneRuntimeDocument) -> Vec<NativeSceneWarning> {
    if !scene_now_playing_provider_service::uses_now_playing_provider(&scene.source) {
        return Vec::new();
    }

    scene
        .now_playing
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == SceneNowPlayingDiagnosticSeverity::Warning)
        .map(NativeSceneWarning::now_playing_provider)
        .collect()
}

#[cfg(target_os = "macos")]
fn text_font_runtime_warnings_for_plan(plan: &SceneRenderPlan) -> Vec<NativeSceneWarning> {
    text_font_runtime_warnings_for_texts(&plan.texts)
}

#[cfg(target_os = "macos")]
fn text_font_runtime_warnings_for_texts(texts: &[SceneRenderTextItem]) -> Vec<NativeSceneWarning> {
    texts
        .iter()
        .filter_map(|item| {
            scene_text_font_uncached(item, item.point_size.max(1.0))
                .ok()
                .and_then(|font| font.fallback_detail)
                .map(|detail| NativeSceneWarning::text_font_fallback(item, detail))
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn text_font_runtime_warnings_for_plan(_plan: &SceneRenderPlan) -> Vec<NativeSceneWarning> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
fn text_font_runtime_warnings_for_texts(_texts: &[SceneRenderTextItem]) -> Vec<NativeSceneWarning> {
    Vec::new()
}

fn clear_native_scene_warning_diagnostics(app: &AppHandle) {
    let codes = diagnostic_service::current_runtime_diagnostics(app)
        .unwrap_or_default()
        .into_iter()
        .filter(|diagnostic| {
            diagnostic.subsystem == DIAGNOSTIC_SUBSYSTEM && diagnostic.code != SYNC_FAILED_CODE
        })
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    for code in codes {
        let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, &code);
    }
}

fn shared_audio_capture_unavailable(app: &AppHandle) -> bool {
    diagnostic_service::current_runtime_diagnostics(app)
        .unwrap_or_default()
        .into_iter()
        .any(|diagnostic| {
            diagnostic.subsystem == "shared-audio" && diagnostic.code == "capture-unavailable"
        })
}

fn shared_input_snapshot_unavailable(app: &AppHandle) -> bool {
    input_service::current_input_snapshot(app)
        .map(|snapshot| snapshot.desktop_width <= 0.0 || snapshot.desktop_height <= 0.0)
        .unwrap_or(true)
}

fn scene_plan_uses_input(plan: &SceneRenderPlan) -> bool {
    !plan.particles.is_empty() || plan.camera.parallax_mouse_influence > 0.0
}

fn runtime_dependency_warnings_for_plan(
    plan: &SceneRenderPlan,
    audio_capture_unavailable: bool,
    input_snapshot_initialized: bool,
    input_snapshot_unavailable: bool,
) -> Vec<NativeSceneWarning> {
    let mut warnings = Vec::new();

    if !plan.audios.is_empty() && audio_capture_unavailable {
        warnings.push(NativeSceneWarning::audio_input_unavailable());
    }

    if scene_plan_uses_input(plan) && input_snapshot_initialized && input_snapshot_unavailable {
        warnings.push(NativeSceneWarning::input_snapshot_unavailable());
    }

    warnings
}

fn video_texture_frame_warning(object_name: &str, error: String) -> NativeSceneWarning {
    NativeSceneWarning::from_video_texture_warning(
        scene_video_texture_service::video_texture_frame_warning(object_name, error),
    )
}

fn video_texture_source_warning(
    source: &SceneVideoTextureSourceSpec,
    error: String,
) -> NativeSceneWarning {
    NativeSceneWarning::from_video_texture_warning(
        scene_video_texture_service::video_texture_source_warning(
            &source.object_name,
            &source.asset_path,
            error,
        ),
    )
}

#[cfg(target_os = "macos")]
impl SceneClearColor {
    fn as_metal_clear_color(self) -> objc2_metal::MTLClearColor {
        objc2_metal::MTLClearColor {
            red: self.red as f64 / 255.0,
            green: self.green as f64 / 255.0,
            blue: self.blue as f64 / 255.0,
            alpha: self.alpha as f64 / 255.0,
        }
    }
}

fn sync_scene_audio_interest(
    app: &AppHandle,
    spec: Option<&SceneRendererSpec>,
) -> Result<(), String> {
    let interested_labels = spec
        .filter(|spec| !spec.paused && !spec.render_plan.audios.is_empty())
        .map(|spec| spec.window_labels.iter().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();

    for label in window_service::player_window_labels(app) {
        let interested = interested_labels.contains(&label);
        audio_input_service::set_scene_audio_interest(app, &label, interested)?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn sync_scene_soundscape(
    _app: &AppHandle,
    state: &NativeSceneRendererServiceState,
    spec: Option<&SceneRendererSpec>,
) -> Result<Vec<NativeSceneWarning>, String> {
    let spec = spec.cloned();
    let output_volume = *state
        .audio_output_volume
        .lock()
        .map_err(|error| error.to_string())?;
    run_on_main(|mtm| {
        let mut slot = state.soundscape.lock().map_err(|error| error.to_string())?;
        if slot.is_none() {
            slot.replace(MainThreadBound::new(NativeSceneSoundscape::default(), mtm));
        }
        let soundscape = slot
            .as_ref()
            .expect("soundscape should exist after initialization");
        let soundscape = soundscape.get(mtm);
        match spec {
            Some(spec) => soundscape.sync(&spec.render_plan.sounds, spec.paused, output_volume),
            None => {
                soundscape.clear();
                Ok(Vec::new())
            }
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn sync_scene_soundscape(
    _app: &AppHandle,
    _state: &NativeSceneRendererServiceState,
    _spec: Option<&SceneRendererSpec>,
) -> Result<Vec<NativeSceneWarning>, String> {
    Ok(Vec::new())
}

struct NativeSceneViewHandle {
    #[cfg(target_os = "macos")]
    host: MainThreadBound<NativeSceneViewHost>,
}

impl NativeSceneViewHandle {
    fn create(app: &AppHandle, clear_color: SceneClearColor) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let host = run_on_main(move |mtm| {
                NativeSceneViewHost::create(app, clear_color, mtm)
                    .map(|host| MainThreadBound::new(host, mtm))
            })?;
            return Ok(Self { host });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, clear_color);
            Ok(Self {})
        }
    }

    fn sync(
        &self,
        app: &AppHandle,
        label: &str,
        spec: &SceneRendererSpec,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let label = label.to_string();
            let spec = spec.clone();
            return run_on_main(move |mtm| {
                let window = app
                    .get_webview_window(&label)
                    .ok_or_else(|| format!("player window {label} was not found"))?;
                let host = self.host.get(mtm);
                host.sync(&window, &spec)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, label, spec);
            Ok(Vec::new())
        }
    }

    fn update_dynamic_text(
        &self,
        app: &AppHandle,
        label: &str,
        texts: &[SceneRenderTextItem],
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let label = label.to_string();
            let texts = texts.to_vec();
            return run_on_main(move |mtm| {
                let window = app
                    .get_webview_window(&label)
                    .ok_or_else(|| format!("player window {label} was not found"))?;
                let host = self.host.get(mtm);
                host.update_dynamic_text(&window, texts, paused)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, label, texts, paused);
            Ok(Vec::new())
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
struct NativeSceneViewHost {
    view: Retained<MTKView>,
    delegate: Retained<NativeSceneRenderDelegate>,
}

#[cfg(target_os = "macos")]
struct NativeSceneRenderDelegateIvars {
    renderer: RefCell<NativeSceneMetalRenderer>,
}

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = NativeSceneRenderDelegateIvars]
    struct NativeSceneRenderDelegate;

    unsafe impl NSObjectProtocol for NativeSceneRenderDelegate {}

    unsafe impl MTKViewDelegate for NativeSceneRenderDelegate {
        #[unsafe(method(drawInMTKView:))]
        #[allow(non_snake_case)]
        fn drawInMTKView(&self, view: &MTKView) {
            self.ivars().renderer.borrow_mut().draw(view);
        }

        #[unsafe(method(mtkView:drawableSizeWillChange:))]
        #[allow(non_snake_case)]
        fn mtkView_drawableSizeWillChange(&self, _view: &MTKView, _size: NSSize) {}
    }
);

#[cfg(target_os = "macos")]
impl NativeSceneRenderDelegate {
    fn new(
        app: AppHandle,
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<Self>, String> {
        let renderer = NativeSceneMetalRenderer::new(app, device, command_queue)?;
        let delegate =
            mtm.alloc::<NativeSceneRenderDelegate>()
                .set_ivars(NativeSceneRenderDelegateIvars {
                    renderer: RefCell::new(renderer),
                });
        Ok(unsafe { msg_send![super(delegate), init] })
    }

    fn apply_scene(
        &self,
        scene_key: &str,
        plan: SceneRenderPlan,
        phase10_graph: ScenePhase10GraphPlan,
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        self.ivars()
            .renderer
            .borrow_mut()
            .apply_scene(scene_key, plan, phase10_graph, paused)
    }

    fn apply_dynamic_text_update(
        &self,
        texts: Vec<SceneRenderTextItem>,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        self.ivars()
            .renderer
            .borrow_mut()
            .apply_dynamic_text_update(texts)
    }

    fn clear_scene(&self) {
        self.ivars().renderer.borrow_mut().clear_scene();
    }

    fn metal_device(&self) -> Retained<ProtocolObject<dyn MTLDevice>> {
        self.ivars().renderer.borrow().device.clone()
    }
}

#[cfg(target_os = "macos")]
impl NativeSceneViewHost {
    fn create(
        app: AppHandle,
        clear_color: SceneClearColor,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let device = create_scene_metal_device()?;
        let command_queue = create_scene_command_queue(device.as_ref())?;
        let view = create_scene_mtk_view(clear_color, device.as_ref(), mtm);
        let delegate = NativeSceneRenderDelegate::new(app, device, command_queue, mtm)?;
        view.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        Ok(Self { view, delegate })
    }

    fn sync(
        &self,
        window: &tauri::WebviewWindow,
        spec: &SceneRendererSpec,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let warnings = self.delegate.apply_scene(
            &spec.wallpaper_id,
            spec.render_plan.clone(),
            spec.phase10_graph.clone(),
            spec.paused,
        )?;
        let container_ptr = window.ns_view().map_err(|error| error.to_string())?;
        let container = unsafe { &*(container_ptr.cast::<NSView>()) };
        let device = self.delegate.metal_device();
        let needs_attach = !self.view.isDescendantOf(container);

        self.view
            .setClearColor(spec.render_plan.clear_color.as_metal_clear_color());
        self.view.setDevice(Some(device.as_ref()));
        self.view.setFrame(container.bounds());
        if needs_attach {
            self.view.removeFromSuperview();
            container.addSubview(&self.view);
        }
        self.view.setPaused(spec.paused);
        if spec.paused || needs_attach {
            self.view.draw();
        }

        Ok(warnings)
    }

    fn update_dynamic_text(
        &self,
        window: &tauri::WebviewWindow,
        texts: Vec<SceneRenderTextItem>,
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let warnings = self.delegate.apply_dynamic_text_update(texts)?;
        let container_ptr = window.ns_view().map_err(|error| error.to_string())?;
        let container = unsafe { &*(container_ptr.cast::<NSView>()) };
        if !self.view.isDescendantOf(container) {
            return Err(
                "native Scene view is not attached to the target player window".to_string(),
            );
        }
        if paused {
            self.view.draw();
        }
        Ok(warnings)
    }

    fn detach(&self) {
        self.view.setPaused(true);
        self.delegate.clear_scene();
        self.view.removeFromSuperview();
    }
}

#[cfg(target_os = "macos")]
fn create_scene_metal_device() -> Result<Retained<ProtocolObject<dyn MTLDevice>>, String> {
    MTLCreateSystemDefaultDevice()
        .ok_or_else(|| "Metal device is unavailable for native Scene runtime".to_string())
}

#[cfg(target_os = "macos")]
fn create_scene_command_queue(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLCommandQueue>>, String> {
    device
        .newCommandQueue()
        .ok_or_else(|| "Metal command queue could not be created".to_string())
}

#[cfg(target_os = "macos")]
fn create_scene_mtk_view(
    clear_color: SceneClearColor,
    device: &ProtocolObject<dyn MTLDevice>,
    mtm: MainThreadMarker,
) -> Retained<MTKView> {
    let view = MTKView::initWithFrame_device(
        MTKView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
        Some(device),
    );
    view.setClearColor(clear_color.as_metal_clear_color());
    view.setColorPixelFormat(MTLPixelFormat::BGRA8Unorm);
    view.setFramebufferOnly(true);
    view.setAutoResizeDrawable(true);
    view.setEnableSetNeedsDisplay(false);
    view.setPreferredFramesPerSecond(60);
    view.setPaused(true);
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    view
}

#[cfg(target_os = "macos")]
struct NativeSceneSoundscape {
    players: RefCell<BTreeMap<u32, NativeSceneSoundPlayer>>,
    output_volume: RefCell<f64>,
}

#[cfg(target_os = "macos")]
impl Default for NativeSceneSoundscape {
    fn default() -> Self {
        Self {
            players: RefCell::new(BTreeMap::new()),
            output_volume: RefCell::new(1.0),
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeSceneSoundPlayer {
    state: SceneSoundRuntimeState,
    player: Retained<AVAudioPlayer>,
}

#[cfg(target_os = "macos")]
impl NativeSceneSoundscape {
    fn sync(
        &self,
        sounds: &[SceneRenderSoundItem],
        paused: bool,
        output_volume: f64,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        self.set_output_volume(output_volume);
        let current_states = self.playback_states();
        let actions = plan_scene_sound_lifecycle(&current_states, sounds, paused);
        let sounds_by_id = sounds
            .iter()
            .map(|sound| (sound.object_id, sound))
            .collect::<BTreeMap<_, _>>();
        let mut players = self.players.borrow_mut();
        let mut warnings = Vec::new();

        for action in actions {
            match action {
                SceneSoundLifecycleAction::Start { state } => {
                    if let Some(sound) = sounds_by_id.get(&state.object_id) {
                        match create_sound_player(sound, state.paused, self.current_output_volume())
                        {
                            Ok(player) => {
                                players.insert(state.object_id, player);
                            }
                            Err(error) => {
                                warnings.push(NativeSceneWarning::from_sound_playback_warning(
                                    scene_sound_playback_load_warning(sound, error),
                                ))
                            }
                        }
                    }
                }
                SceneSoundLifecycleAction::Replace { previous: _, state } => {
                    if let Some(existing) = players.remove(&state.object_id) {
                        stop_sound_player(&existing);
                    }
                    if let Some(sound) = sounds_by_id.get(&state.object_id) {
                        match create_sound_player(sound, state.paused, self.current_output_volume())
                        {
                            Ok(player) => {
                                players.insert(state.object_id, player);
                            }
                            Err(error) => {
                                warnings.push(NativeSceneWarning::from_sound_playback_warning(
                                    scene_sound_playback_load_warning(sound, error),
                                ))
                            }
                        }
                    }
                }
                SceneSoundLifecycleAction::Update { state } => {
                    if let Some(player) = players.get_mut(&state.object_id) {
                        configure_sound_player(player, &state, self.current_output_volume());
                    }
                }
                SceneSoundLifecycleAction::SetPaused { object_id, paused } => {
                    if let Some(player) = players.get_mut(&object_id) {
                        player.state.paused = paused;
                        set_sound_player_paused(player, paused);
                    }
                }
                SceneSoundLifecycleAction::Stop { state } => {
                    if let Some(player) = players.remove(&state.object_id) {
                        stop_sound_player(&player);
                    }
                }
            }
        }

        Ok(warnings)
    }

    fn clear(&self) {
        let actions = plan_scene_sound_clear(&self.playback_states());
        let mut players = self.players.borrow_mut();
        for action in actions {
            if let SceneSoundLifecycleAction::Stop { state } = action {
                if let Some(player) = players.remove(&state.object_id) {
                    stop_sound_player(&player);
                }
            }
        }
    }

    fn reactive_levels(&self, count: usize) -> Option<Vec<f64>> {
        if count == 0 {
            return None;
        }

        let current = self.players.borrow();
        let mut meters = Vec::new();
        for player in current.values() {
            if !unsafe { player.player.isPlaying() } {
                continue;
            }

            unsafe {
                player.player.updateMeters();
            }
            let channel_count = unsafe { player.player.numberOfChannels() }.max(1) as usize;
            let mut level = 0.0_f64;
            for channel in 0..channel_count {
                let average = unsafe { player.player.averagePowerForChannel(channel) } as f64;
                let peak = unsafe { player.player.peakPowerForChannel(channel) } as f64;
                level = level.max(scene_sound_meter_level(average, peak));
            }
            if level <= 0.001 {
                continue;
            }

            let phase = unsafe { player.player.currentTime() } as f64;
            meters.push(SceneSoundMeterReading { level, phase });
        }

        scene_sound_reactive_levels(&meters, count)
    }

    fn set_output_volume(&self, output_volume: f64) {
        let normalized = output_volume.clamp(0.0, 1.0);
        *self.output_volume.borrow_mut() = normalized;
        for player in self.players.borrow_mut().values_mut() {
            unsafe {
                player
                    .player
                    .setVolume(effective_output_volume(player.state.volume, normalized) as f32);
            }
        }
    }

    fn current_output_volume(&self) -> f64 {
        *self.output_volume.borrow()
    }

    fn playback_states(&self) -> BTreeMap<u32, SceneSoundRuntimeState> {
        self.players
            .borrow()
            .iter()
            .map(|(object_id, player)| (*object_id, player.state.clone()))
            .collect()
    }
}

#[cfg(target_os = "macos")]
struct NativeSceneMetalRenderer {
    app: AppHandle,
    command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    video_texture_cache: CFRetained<CVMetalTextureCache>,
    pipelines: NativeScenePipelineStates,
    scene_key: Option<String>,
    plan: Option<SceneRenderPlan>,
    phase10_graph: ScenePhase10GraphPlan,
    texture_cache: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    texture_resolution_cache: BTreeMap<String, Phase10TextureMetrics>,
    text_texture_cache: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_output_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_scratch_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_named_target_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    phase10_background_textures: BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    video_sources: BTreeMap<u32, NativeSceneVideoSource>,
    mdl_cache: BTreeMap<PathBuf, SceneMdlDocument>,
    compiled_shader_variants:
        BTreeMap<String, Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
    started_at: Instant,
    last_frame_at: Instant,
    animation_time_seconds: f64,
    audio_coordinator: SceneAudioCoordinator,
    input_coordinator: SceneInputCoordinator,
    particle_signature: Option<u64>,
    particle_scheduler: SceneParticleScheduler,
    rope_particle_scheduler: SceneRopeParticleScheduler,
    sprite_particle_scheduler: SceneSpriteParticleScheduler,
    paused: bool,
}

#[cfg(target_os = "macos")]
struct NativeScenePipelineStates {
    normal: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    additive: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    multiply: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct NativeSceneVideoSource {
    object_id: u32,
    asset_path: PathBuf,
    paused: bool,
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    current_cv_texture: Option<CFRetained<CVMetalTexture>>,
    current_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct SceneVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    opacity: f32,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct SceneQuadPrimitive {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    rotation: f64,
    opacity: f64,
    flip_x: bool,
    flip_y: bool,
    uv_rect: [f32; 4],
    color: SceneRenderColor,
    transform_origin_x: f64,
    transform_origin_y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct SceneProjection {
    scene_origin_x: f64,
    scene_origin_y: f64,
    scene_canvas_height: f64,
    camera_scale: f64,
    view_width: f64,
    view_height: f64,
}

#[cfg(target_os = "macos")]
impl NativeSceneMetalRenderer {
    fn new(
        app: AppHandle,
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    ) -> Result<Self, String> {
        Ok(Self {
            app,
            command_queue,
            device: device.clone(),
            video_texture_cache: create_scene_video_texture_cache(device.as_ref())?,
            pipelines: build_scene_pipeline_states(&device)?,
            scene_key: None,
            plan: None,
            phase10_graph: ScenePhase10GraphPlan::default(),
            texture_cache: BTreeMap::new(),
            texture_resolution_cache: BTreeMap::new(),
            text_texture_cache: BTreeMap::new(),
            phase10_output_textures: BTreeMap::new(),
            phase10_scratch_textures: BTreeMap::new(),
            phase10_named_target_textures: BTreeMap::new(),
            phase10_background_textures: BTreeMap::new(),

            video_sources: BTreeMap::new(),
            mdl_cache: BTreeMap::new(),
            compiled_shader_variants: BTreeMap::new(),
            started_at: Instant::now(),
            last_frame_at: Instant::now(),
            animation_time_seconds: 0.0,
            audio_coordinator: SceneAudioCoordinator::default(),
            input_coordinator: SceneInputCoordinator::default(),
            particle_signature: None,
            particle_scheduler: SceneParticleScheduler::default(),
            rope_particle_scheduler: SceneRopeParticleScheduler::default(),
            sprite_particle_scheduler: SceneSpriteParticleScheduler::default(),
            paused: false,
        })
    }

    fn apply_scene(
        &mut self,
        scene_key: &str,
        mut plan: SceneRenderPlan,
        phase10_graph: ScenePhase10GraphPlan,
        paused: bool,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let mut warnings = Vec::new();
        let mut required_keys = BTreeSet::new();
        let mut required_text_keys = BTreeSet::new();
        let mut retained_visuals = Vec::new();
        let mut retained_texts = Vec::new();
        let phase10_consumed_ids = phase10_graph
            .consumed_visual_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let next_particle_signature = particle_plan_signature(&plan);

        if self.scene_key.as_deref() != Some(scene_key) {
            self.clear_video_sources();
        }
        warnings.extend(self.sync_video_sources(&plan.visuals, paused));

        for item in &plan.visuals {
            if should_retain_visual_in_draw_plan(item, &phase10_consumed_ids, false) {
                retained_visuals.push(item.clone());
                continue;
            }
            match item.source_kind {
                SceneRenderSourceKind::Image => {
                    let key = visual_texture_cache_key(item);
                    match self.ensure_visual_texture_loaded(item, &key) {
                        Ok(()) => {
                            required_keys.insert(key);
                            if should_retain_visual_in_draw_plan(item, &phase10_consumed_ids, true)
                            {
                                retained_visuals.push(item.clone());
                            }
                        }
                        Err(error) => {
                            warnings
                                .push(NativeSceneWarning::texture_load(&item.texture_path, error));
                        }
                    }
                }
                SceneRenderSourceKind::Video => {
                    retained_visuals.push(item.clone());
                }
            }
        }

        for item in &plan.texts {
            let key = text_texture_cache_key(item);
            match self.ensure_text_texture_loaded(item, &key) {
                Ok(text_warnings) => {
                    required_text_keys.insert(key);
                    retained_texts.push(item.clone());
                    warnings.extend(text_warnings);
                }
                Err(error) => warnings.push(NativeSceneWarning {
                    code: "text-raster-failed".to_string(),
                    message: format!(
                        "Scene text {} could not be rasterized for Metal.",
                        item.object_name
                    ),
                    detail: Some(SceneDiagnosticDetail::runtime(
                        SceneDiagnosticDomain::Text,
                        "text-raster",
                        error,
                    )),
                }),
            }
        }

        for item in &plan.sprite_particles {
            for texture_path in sprite_particle_texture_paths(item) {
                match self.ensure_phase10_texture_loaded(&texture_path, &mut required_keys) {
                    Ok(()) => {}
                    Err(error) => warnings.push(NativeSceneWarning::texture_load(
                        &texture_path,
                        format!("Sprite particle texture could not be prepared: {error}"),
                    )),
                }
            }
        }

        let (prepared_phase10_graph, phase10_warnings) =
            self.prepare_phase10_graph(&phase10_graph, &mut required_keys)?;
        warnings.extend(phase10_warnings);

        if !plan.audios.is_empty() || !plan.particles.is_empty() {
            self.ensure_procedural_texture(
                WHITE_TEXTURE_KEY,
                build_white_texture_image(),
                &mut required_keys,
            )?;
        }
        if plan.particles.iter().any(|item| {
            matches!(
                item.particle_kind,
                crate::models::SceneParticleKind::PetalTrail
            )
        }) {
            self.ensure_procedural_texture(
                PETAL_TEXTURE_KEY,
                build_petal_texture_image(),
                &mut required_keys,
            )?;
        }

        self.texture_cache
            .retain(|key, _| required_keys.contains(key));
        self.texture_resolution_cache
            .retain(|key, _| required_keys.contains(key));
        self.text_texture_cache
            .retain(|key, _| required_text_keys.contains(key));
        let active_audio_counts = plan
            .audios
            .iter()
            .map(|item| item.bar_count)
            .collect::<BTreeSet<_>>();
        self.audio_coordinator.retain_counts(&active_audio_counts);

        plan.visuals = retained_visuals;
        plan.texts = retained_texts;
        self.phase10_graph = prepared_phase10_graph;
        self.paused = paused;

        if scene_debug_layout_enabled() {
            eprintln!(
                "[scene-layout] scene_key={scene_key} visuals={} texts={} audios={}",
                plan.visuals.len(),
                plan.texts.len(),
                plan.audios.len()
            );
            for item in plan.texts.iter().take(12) {
                eprintln!(
                    "[scene-layout][text] id={} name={} behavior={:?} quad=({:.1},{:.1},{:.1},{:.1}) content=({:.1},{:.1},{:.1},{:.1}) point={:.1} text={:?}",
                    item.object_id,
                    item.object_name,
                    item.behavior,
                    item.quad.left,
                    item.quad.top,
                    item.quad.width,
                    item.quad.height,
                    item.content_left,
                    item.content_top,
                    item.content_width,
                    item.content_height,
                    item.point_size,
                    item.text
                );
            }
            for item in plan.audios.iter().take(6) {
                eprintln!(
                    "[scene-layout][audio] id={} name={} quad=({:.1},{:.1},{:.1},{:.1}) rot={:.3} bars={} gap={:.2} width={:.2} top={:.2} height={:.2}",
                    item.object_id,
                    item.object_name,
                    item.quad.left,
                    item.quad.top,
                    item.quad.width,
                    item.quad.height,
                    item.quad.rotation,
                    item.bar_count,
                    item.gap,
                    item.bar_width,
                    item.drawable_top,
                    item.drawable_height
                );
            }
        }

        if !plan.has_renderable_output() && self.phase10_graph.visuals.is_empty() {
            let preview = warnings
                .iter()
                .take(4)
                .map(|warning| warning.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let suffix = if preview.is_empty() {
                String::new()
            } else {
                format!(": {preview}")
            };
            return Err(format!(
                "Scene native renderer has no draw-ready output after texture preparation{suffix}"
            ));
        }

        if self.scene_key.as_deref() != Some(scene_key)
            || self.particle_signature != next_particle_signature
        {
            self.particle_scheduler.reset();
            self.rope_particle_scheduler.reset();
            self.sprite_particle_scheduler.reset();
        }
        if self.scene_key.as_deref() != Some(scene_key) {
            self.audio_coordinator.reset();
            self.input_coordinator.reset();
            self.animation_time_seconds = 0.0;
        }

        warnings.extend(self.runtime_dependency_warnings(&plan));
        self.scene_key = Some(scene_key.to_string());
        self.particle_signature = next_particle_signature;
        self.plan = Some(plan);
        Ok(warnings)
    }

    fn apply_dynamic_text_update(
        &mut self,
        texts: Vec<SceneRenderTextItem>,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        let mut retained_texts = Vec::new();
        let mut required_text_keys = BTreeSet::new();
        let mut warnings = Vec::new();

        if self.plan.is_none() {
            return Err("native Scene renderer has no active plan for dynamic text update".into());
        }

        for item in texts {
            let key = text_texture_cache_key(&item);
            match self.ensure_text_texture_loaded(&item, &key) {
                Ok(text_warnings) => {
                    required_text_keys.insert(key);
                    retained_texts.push(item);
                    warnings.extend(text_warnings);
                }
                Err(error) => warnings.push(NativeSceneWarning {
                    code: "text-raster-failed".to_string(),
                    message: format!(
                        "Scene text {} could not be rasterized for Metal.",
                        item.object_name
                    ),
                    detail: Some(SceneDiagnosticDetail::runtime(
                        SceneDiagnosticDomain::Text,
                        "text-raster",
                        error,
                    )),
                }),
            }
        }

        self.text_texture_cache
            .retain(|key, _| required_text_keys.contains(key));
        if let Some(plan) = self.plan.as_mut() {
            plan.texts = retained_texts;
        }

        Ok(warnings)
    }

    fn runtime_dependency_warnings(&self, plan: &SceneRenderPlan) -> Vec<NativeSceneWarning> {
        runtime_dependency_warnings_for_plan(
            plan,
            shared_audio_capture_unavailable(&self.app),
            input_service::input_snapshot_initialized(&self.app),
            shared_input_snapshot_unavailable(&self.app),
        )
    }

    fn clear_scene(&mut self) {
        self.scene_key = None;
        self.plan = None;
        self.phase10_graph = ScenePhase10GraphPlan::default();
        self.texture_cache.clear();
        self.texture_resolution_cache.clear();
        self.text_texture_cache.clear();
        self.phase10_output_textures.clear();
        self.phase10_scratch_textures.clear();
        self.clear_video_sources();
        self.mdl_cache.clear();
        self.compiled_shader_variants.clear();
        self.particle_signature = None;
        self.audio_coordinator.reset();
        self.input_coordinator.reset();
        self.particle_scheduler.reset();
        self.rope_particle_scheduler.reset();
        self.sprite_particle_scheduler.reset();
        self.last_frame_at = Instant::now();
        self.animation_time_seconds = 0.0;
        self.paused = false;
    }

    fn draw(&mut self, view: &MTKView) {
        let Some(current_drawable) = view.currentDrawable() else {
            return;
        };
        let Some(pass_descriptor) = view.currentRenderPassDescriptor() else {
            return;
        };
        let Some(command_buffer) = self.command_queue.commandBuffer() else {
            return;
        };

        let plan = self.plan.clone();
        if let Some(plan) = plan.as_ref() {
            self.draw_plan(view, &command_buffer, &pass_descriptor, plan);
        }
        command_buffer.presentDrawable(ProtocolObject::from_ref(&*current_drawable));
        command_buffer.commit();
    }

    fn draw_plan(
        &mut self,
        view: &MTKView,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        pass_descriptor: &MTLRenderPassDescriptor,
        plan: &SceneRenderPlan,
    ) {
        let delta_seconds = self.frame_delta_seconds();
        if !self.paused {
            self.animation_time_seconds += delta_seconds;
        }
        let input_frame = self.update_input_frame(view, plan, delta_seconds);
        let projection = scene_projection(view, plan, input_frame.camera_offset);
        let shared_audio_snapshot = audio_input_service::current_audio_snapshot(&self.app).ok();
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        let phase10_graph = self.phase10_graph.clone();
        let phase10_outputs = self.render_phase10_outputs(
            command_buffer,
            plan,
            &phase10_graph,
            self.animation_time_seconds,
        );
        let phase10_visuals = phase10_graph
            .visuals
            .iter()
            .map(|visual| (visual.object_id, visual))
            .collect::<BTreeMap<_, _>>();
        let visual_items = plan
            .visuals
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let text_items = plan
            .texts
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let audio_items = plan
            .audios
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let particle_items = plan
            .particles
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let rope_particle_items = plan
            .rope_particles
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let sprite_particle_items = plan
            .sprite_particles
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let mut drawn_phase10_ids = BTreeSet::new();
        let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(pass_descriptor)
        else {
            return;
        };
        let white_texture = self.texture_cache.get(WHITE_TEXTURE_KEY).cloned();
        let petal_texture = self.texture_cache.get(PETAL_TEXTURE_KEY).cloned();
        let now_ms_f64 = now_ms as f64;

        if white_texture.is_some() && !plan.particles.is_empty() {
            self.advance_particle_items(&plan.particles, input_frame.response, now_ms_f64);
        }
        if !plan.rope_particles.is_empty() {
            self.advance_rope_particle_items(&plan.rope_particles, input_frame.response, now_ms_f64);
        }
        if !plan.sprite_particles.is_empty() {
            if !self.paused {
                self.sprite_particle_scheduler
                    .advance(&plan.sprite_particles, now_ms_f64);
            } else {
                self.sprite_particle_scheduler.pause();
            }
        }

        for draw_item in &plan.draw_order {
            match draw_item.kind {
                SceneRenderDrawKind::Visual => {
                    if let Some(visual) = phase10_visuals.get(&draw_item.object_id) {
                        self.draw_phase10_visual(
                            &encoder,
                            &projection,
                            visual,
                            phase10_outputs.get(&draw_item.object_id),
                            self.animation_time_seconds,
                        );
                        drawn_phase10_ids.insert(draw_item.object_id);
                        continue;
                    }
                    let Some(item) = visual_items.get(&draw_item.object_id) else {
                        continue;
                    };
                    let texture = match item.source_kind {
                        SceneRenderSourceKind::Image => self
                            .texture_cache
                            .get(&visual_texture_cache_key(item))
                            .cloned(),
                        SceneRenderSourceKind::Video => self.video_texture_for_item(item),
                    };
                    let Some(texture) = texture.as_ref() else {
                        continue;
                    };
                    let quad =
                        quad_primitive_from_render_quad(item.quad, SceneRenderColor::default());
                    self.draw_quad(
                        &encoder,
                        texture.as_ref(),
                        item.blend_mode,
                        &projection,
                        quad,
                    );
                }
                SceneRenderDrawKind::Text => {
                    let Some(item) = text_items.get(&draw_item.object_id) else {
                        continue;
                    };
                    let key = text_texture_cache_key(item);
                    let Some(texture) = self.text_texture_cache.get(&key) else {
                        continue;
                    };
                    let quad =
                        quad_primitive_from_render_quad(item.quad, SceneRenderColor::default());
                    self.draw_quad(
                        &encoder,
                        texture.as_ref(),
                        SceneRenderBlendMode::Normal,
                        &projection,
                        quad,
                    );
                }
                SceneRenderDrawKind::Audio => {
                    let (Some(item), Some(white_texture)) = (
                        audio_items.get(&draw_item.object_id),
                        white_texture.as_ref(),
                    ) else {
                        continue;
                    };
                    self.draw_audio_item(
                        &encoder,
                        white_texture.as_ref(),
                        &projection,
                        item,
                        shared_audio_snapshot.as_ref(),
                        now_ms,
                    );
                }
                SceneRenderDrawKind::Particle => {
                    let (Some(item), Some(white_texture)) = (
                        particle_items.get(&draw_item.object_id),
                        white_texture.as_ref(),
                    ) else {
                        continue;
                    };
                    self.draw_particle_item(
                        &encoder,
                        white_texture.as_ref(),
                        petal_texture.as_ref().map(|texture| texture.as_ref()),
                        &projection,
                        plan,
                        item,
                        now_ms_f64,
                    );
                }
                SceneRenderDrawKind::RopeParticle => {
                    let _ = rope_particle_items.get(&draw_item.object_id);
                }
                SceneRenderDrawKind::SpriteParticle => {
                    let Some(item) = sprite_particle_items.get(&draw_item.object_id) else {
                        continue;
                    };
                    self.draw_sprite_particle_item(&encoder, &projection, plan, item, now_ms_f64);
                }
                SceneRenderDrawKind::Sound => {}
            }
        }

        for visual in &phase10_graph.visuals {
            if drawn_phase10_ids.insert(visual.object_id) {
                self.draw_phase10_visual(
                    &encoder,
                    &projection,
                    visual,
                    phase10_outputs.get(&visual.object_id),
                    self.animation_time_seconds,
                );
            }
        }

        encoder.endEncoding();
    }

    fn render_phase10_outputs(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        plan: &SceneRenderPlan,
        graph: &ScenePhase10GraphPlan,
        elapsed_seconds: f64,
    ) -> BTreeMap<u32, Retained<ProtocolObject<dyn MTLTexture>>> {
        let mut outputs = BTreeMap::new();
        let mut required_output_keys = BTreeSet::new();
        let mut required_scratch_keys = BTreeSet::new();
        let mut required_named_target_keys = BTreeSet::new();
        let mut required_background_keys = BTreeSet::new();
        let phase10_visuals = graph
            .visuals
            .iter()
            .map(|visual| (visual.object_id, visual))
            .collect::<BTreeMap<_, _>>();
        let visual_items = plan
            .visuals
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let text_items = plan
            .texts
            .iter()
            .map(|item| (item.object_id, item))
            .collect::<BTreeMap<_, _>>();
        let mut rendered_ids = BTreeSet::new();
        let mut previous_layers = Vec::<Phase10BackgroundLayer>::new();

        for (draw_item, source_kind) in phase10_background_source_order(plan, graph) {
            if source_kind == Phase10BackgroundSourceKind::Phase10Visual {
                if let Some(visual) = phase10_visuals.get(&draw_item.object_id) {
                    if phase10_visual_requires_offscreen_chain(visual) {
                        let background_snapshot =
                            if phase10_visual_needs_background_snapshot(visual) {
                                self.render_phase10_background_snapshot(
                                    command_buffer,
                                    visual,
                                    &previous_layers,
                                    &mut required_background_keys,
                                )
                            } else {
                                None
                            };
                        if let Some(texture) = self.render_phase10_output_for_visual(
                            command_buffer,
                            plan.canvas_height,
                            visual,
                            background_snapshot.as_ref(),
                            elapsed_seconds,
                            &mut required_output_keys,
                            &mut required_scratch_keys,
                            &mut required_named_target_keys,
                        ) {
                            previous_layers.push(Phase10BackgroundLayer {
                                quad: visual.quad,
                                blend_mode: visual.blend_mode,
                                texture: texture.clone(),
                            });
                            outputs.insert(visual.object_id, texture);
                        }
                    }
                    rendered_ids.insert(visual.object_id);
                    continue;
                }
            }

            if let Some(layer) =
                self.phase10_background_layer_for_draw_item(&draw_item, &visual_items, &text_items)
            {
                previous_layers.push(layer);
            }
        }

        for visual in &graph.visuals {
            if rendered_ids.contains(&visual.object_id)
                || !phase10_visual_requires_offscreen_chain(visual)
            {
                continue;
            }
            let background_snapshot = if phase10_visual_needs_background_snapshot(visual) {
                self.render_phase10_background_snapshot(
                    command_buffer,
                    visual,
                    &previous_layers,
                    &mut required_background_keys,
                )
            } else {
                None
            };
            if let Some(texture) = self.render_phase10_output_for_visual(
                command_buffer,
                plan.canvas_height,
                visual,
                background_snapshot.as_ref(),
                elapsed_seconds,
                &mut required_output_keys,
                &mut required_scratch_keys,
                &mut required_named_target_keys,
            ) {
                previous_layers.push(Phase10BackgroundLayer {
                    quad: visual.quad,
                    blend_mode: visual.blend_mode,
                    texture: texture.clone(),
                });
                outputs.insert(visual.object_id, texture);
            }
        }

        self.phase10_output_textures
            .retain(|key, _| required_output_keys.contains(key));
        self.phase10_scratch_textures
            .retain(|key, _| required_scratch_keys.contains(key));
        self.phase10_named_target_textures
            .retain(|key, _| required_named_target_keys.contains(key));
        self.phase10_background_textures
            .retain(|key, _| required_background_keys.contains(key));
        outputs
    }

    fn render_phase10_output_for_visual(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        canvas_height: f64,
        visual: &ScenePhase10VisualPlan,
        background_snapshot: Option<&Phase10TextureHandle>,
        elapsed_seconds: f64,
        required_output_keys: &mut BTreeSet<String>,
        required_scratch_keys: &mut BTreeSet<String>,
        required_named_target_keys: &mut BTreeSet<String>,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let base_texture = self.phase10_base_texture_for_visual(visual)?;
        let mut previous_texture = base_texture.clone();
        let mut named_targets = BTreeMap::<String, Phase10TextureHandle>::new();
        let passes = phase10_visual_pass_chain(visual);
        if passes.is_empty() {
            return None;
        }

        let (width, height) = phase10_render_target_size(visual);
        let output_key = phase10_output_texture_key(visual.object_id, width, height);
        required_output_keys.insert(output_key.clone());
        let output_texture = self.ensure_phase10_output_target(&output_key, width, height)?;

        if visual.puppet_path.is_some() {
            return self.render_phase10_puppet_output_for_visual(
                command_buffer,
                canvas_height,
                visual,
                &base_texture,
                &passes,
                width,
                height,
                output_texture,
                elapsed_seconds,
                required_scratch_keys,
                required_named_target_keys,
            );
        }

        if phase10_visual_first_pass_is_mask_alpha(visual) && passes.len() == 1 {
            return self.render_phase10_mask_chain(
                command_buffer,
                visual,
                &base_texture,
                &passes[0],
                width,
                height,
                required_scratch_keys,
            );
        }

        for (index, resolved_pass) in passes.iter().enumerate() {
            let target_name = phase10_resolved_pass_target_name(resolved_pass);
            let is_last = index + 1 == passes.len();
            let target = if let Some(target_name) = target_name {
                let key =
                    phase10_named_target_texture_key(visual.object_id, target_name, width, height);
                required_named_target_keys.insert(key.clone());
                self.ensure_phase10_named_target(&key, width, height)?
            } else if is_last {
                output_texture.clone()
            } else {
                let scratch_key = phase10_scratch_texture_key(width, height, index % 2);
                required_scratch_keys.insert(scratch_key.clone());
                self.ensure_phase10_scratch_target(&scratch_key, width, height)?
            };

            let input_scope = Phase10PassInputScope {
                local_current: Some(&base_texture),
                previous_pass: Some(&previous_texture),
                background: background_snapshot,
                copied_background: background_snapshot,
                named_targets: &named_targets,
            };
            let pass_textures = self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
            let uniforms = self.phase10_effect_uniforms_for_pass(
                resolved_pass,
                &pass_textures,
                width,
                height,
                elapsed_seconds,
            );
            let shader_defines = phase10_pass_shader_defines(resolved_pass);
            if !self.encode_phase10_pass(
                command_buffer,
                &target.texture,
                resolved_pass.pass,
                &shader_defines,
                &pass_textures,
                &uniforms,
                input_scope
                    .previous_pass
                    .map(|texture| texture.texture.clone()),
            ) {
                return None;
            }

            previous_texture = target.clone();
            if let Some(target_name) = target_name {
                named_targets.insert(target_name.to_string(), target);
            }
        }

        Some(previous_texture.texture)
    }

    fn render_phase10_puppet_output_for_visual(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        canvas_height: f64,
        visual: &ScenePhase10VisualPlan,
        base_texture: &Phase10TextureHandle,
        passes: &[Phase10ResolvedPass<'_>],
        width: usize,
        height: usize,
        output_texture: Phase10TextureHandle,
        elapsed_seconds: f64,
        required_scratch_keys: &mut BTreeSet<String>,
        required_named_target_keys: &mut BTreeSet<String>,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        use crate::services::scene_mdl_service::SceneMdlMeshFrame;

        let puppet_path = visual.puppet_path.as_ref()?;
        let document = self.mdl_cache.get(puppet_path)?;
        let mesh_frame =
            evaluate_scene_mdl_mesh(document, &visual.animation_layers, elapsed_seconds)?;
        if mesh_frame.positions.is_empty() || mesh_frame.indices.len() < 3 {
            return None;
        }

        let submesh_frames: Vec<SceneMdlMeshFrame> = if visual.submeshes.is_empty() {
            vec![mesh_frame.clone()]
        } else {
            visual
                .submeshes
                .iter()
                .filter_map(|submesh| mesh_frame.extract_submesh(submesh))
                .collect()
        };
        if submesh_frames.is_empty() {
            return None;
        }

        let projection = phase10_puppet_offscreen_projection(visual, canvas_height, width, height);
        let mut previous_texture = base_texture.clone();
        let mut named_targets = BTreeMap::<String, Phase10TextureHandle>::new();

        for (index, resolved_pass) in passes.iter().enumerate() {
            let target_name = phase10_resolved_pass_target_name(resolved_pass);
            let is_last = index + 1 == passes.len();
            let target = if let Some(target_name) = target_name {
                let key =
                    phase10_named_target_texture_key(visual.object_id, target_name, width, height);
                required_named_target_keys.insert(key.clone());
                self.ensure_phase10_named_target(&key, width, height)?
            } else if is_last {
                output_texture.clone()
            } else {
                let scratch_key = phase10_scratch_texture_key(width, height, index % 2);
                required_scratch_keys.insert(scratch_key.clone());
                self.ensure_phase10_scratch_target(&scratch_key, width, height)?
            };

            let input_scope = Phase10PassInputScope {
                local_current: Some(base_texture),
                previous_pass: Some(&previous_texture),
                background: None,
                copied_background: None,
                named_targets: &named_targets,
            };
            let pass_textures = self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
            if pass_textures
                .slots
                .first()
                .and_then(|slot| slot.as_ref())
                .is_none()
            {
                return None;
            }
            let uniforms = self.phase10_effect_uniforms_for_pass(
                resolved_pass,
                &pass_textures,
                width,
                height,
                elapsed_seconds,
            );
            let shader_defines = phase10_pass_shader_defines(resolved_pass);

            let descriptor = MTLRenderPassDescriptor::new();
            unsafe {
                let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
                attachment.setTexture(Some(target.texture.as_ref()));
                attachment.setLoadAction(MTLLoadAction::Clear);
                attachment.setStoreAction(MTLStoreAction::Store);
                attachment.setClearColor(objc2_metal::MTLClearColor {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 0.0,
                });
            }
            let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor)
            else {
                return None;
            };

            if phase10_alpha_prefill_required(resolved_pass.pass.blend_mode) {
                if let Some(previous_texture) = input_scope.previous_pass.map(|t| t.texture.clone())
                {
                    self.draw_phase10_fullscreen_texture(
                        &encoder,
                        previous_texture,
                        SceneRenderColor::default(),
                        SceneRenderBlendMode::Normal,
                    );
                }
            }

            for sub_frame in &submesh_frames {
                self.draw_phase10_mesh(
                    &encoder,
                    &projection,
                    visual,
                    sub_frame,
                    resolved_pass.pass,
                    &shader_defines,
                    &pass_textures,
                    &uniforms,
                );
            }
            encoder.endEncoding();

            previous_texture = target.clone();
            if let Some(target_name) = target_name {
                named_targets.insert(target_name.to_string(), target);
            }
        }

        Some(previous_texture.texture)
    }

    fn render_phase10_mask_chain(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        visual: &ScenePhase10VisualPlan,
        base_texture: &Phase10TextureHandle,
        resolved_pass: &Phase10ResolvedPass<'_>,
        width: usize,
        height: usize,
        required_scratch_keys: &mut BTreeSet<String>,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let output_key = phase10_output_texture_key(visual.object_id, width, height);
        let output_texture = self.ensure_phase10_output_target(&output_key, width, height)?;

        let scratch_key = phase10_scratch_texture_key(width, height, 0);
        required_scratch_keys.insert(scratch_key.clone());
        let scratch_texture = self.ensure_phase10_scratch_target(&scratch_key, width, height)?;

        let input_scope = Phase10PassInputScope {
            local_current: Some(base_texture),
            previous_pass: Some(base_texture),
            background: None,
            copied_background: None,
            named_targets: &BTreeMap::new(),
        };
        let mask_alpha_pass_textures =
            self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
        let mask_alpha_defines = phase10_pass_shader_defines(resolved_pass);
        let mask_alpha_uniforms = self.phase10_effect_uniforms_for_pass(
            resolved_pass,
            &mask_alpha_pass_textures,
            width,
            height,
            0.0,
        );

        if !self.encode_phase10_pass(
            command_buffer,
            &scratch_texture.texture,
            resolved_pass.pass,
            &mask_alpha_defines,
            &mask_alpha_pass_textures,
            &mask_alpha_uniforms,
            None,
        ) {
            return None;
        }

        let mask_apply_program = phase10_mask_apply_shader_program();
        let mask_apply_key =
            phase10_shader_variant_key(&mask_apply_program, &BTreeMap::new(), visual.blend_mode);
        let Some(mask_apply_pipeline) = self.compiled_shader_variants.get(&mask_apply_key) else {
            return None;
        };

        let mask_apply_pass_textures = Phase10PassTextures {
            slots: vec![
                Some(base_texture.clone()),
                Some(scratch_texture),
                None,
                None,
            ],
        };

        let descriptor = MTLRenderPassDescriptor::new();
        unsafe {
            let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
            attachment.setTexture(Some(output_texture.texture.as_ref()));
            attachment.setLoadAction(MTLLoadAction::Clear);
            attachment.setStoreAction(MTLStoreAction::Store);
            attachment.setClearColor(objc2_metal::MTLClearColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            });
        }
        let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor) else {
            return None;
        };

        self.draw_phase10_fullscreen_pass(
            &encoder,
            mask_apply_pipeline.as_ref(),
            &mask_apply_pass_textures,
            &Phase10EffectUniforms::default(),
            visual.base_color,
        );
        encoder.endEncoding();

        Some(output_texture.texture)
    }

    fn phase10_background_layer_for_draw_item(
        &mut self,
        draw_item: &SceneRenderDrawItem,
        visual_items: &BTreeMap<u32, &SceneRenderVisualItem>,
        text_items: &BTreeMap<u32, &SceneRenderTextItem>,
    ) -> Option<Phase10BackgroundLayer> {
        match draw_item.kind {
            SceneRenderDrawKind::Visual => {
                let item = visual_items.get(&draw_item.object_id)?;
                let texture = match item.source_kind {
                    SceneRenderSourceKind::Image => self
                        .texture_cache
                        .get(&visual_texture_cache_key(item))
                        .cloned(),
                    SceneRenderSourceKind::Video => self.video_texture_for_item(item),
                }?;
                Some(Phase10BackgroundLayer {
                    quad: item.quad,
                    blend_mode: item.blend_mode,
                    texture,
                })
            }
            SceneRenderDrawKind::Text => {
                let item = text_items.get(&draw_item.object_id)?;
                let texture = self
                    .text_texture_cache
                    .get(&text_texture_cache_key(item))?
                    .clone();
                Some(Phase10BackgroundLayer {
                    quad: item.quad,
                    blend_mode: SceneRenderBlendMode::Normal,
                    texture,
                })
            }
            SceneRenderDrawKind::Audio
            | SceneRenderDrawKind::Particle
            | SceneRenderDrawKind::RopeParticle
            | SceneRenderDrawKind::SpriteParticle
            | SceneRenderDrawKind::Sound => None,
        }
    }

    fn render_phase10_background_snapshot(
        &mut self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        visual: &ScenePhase10VisualPlan,
        layers: &[Phase10BackgroundLayer],
        required_background_keys: &mut BTreeSet<String>,
    ) -> Option<Phase10TextureHandle> {
        let (width, height) = phase10_render_target_size(visual);
        let key = phase10_background_texture_key(visual.object_id, width, height);
        required_background_keys.insert(key.clone());
        let target = self.ensure_phase10_background_target(&key, width, height)?;

        let descriptor = MTLRenderPassDescriptor::new();
        unsafe {
            let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
            attachment.setTexture(Some(target.texture.as_ref()));
            attachment.setLoadAction(MTLLoadAction::Clear);
            attachment.setStoreAction(MTLStoreAction::Store);
            attachment.setClearColor(objc2_metal::MTLClearColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            });
        }
        let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor) else {
            return None;
        };
        let projection = phase10_local_background_projection(visual, width, height);
        for layer in layers {
            self.draw_quad(
                &encoder,
                layer.texture.as_ref(),
                layer.blend_mode,
                &projection,
                quad_primitive_from_render_quad(layer.quad, SceneRenderColor::default()),
            );
        }
        encoder.endEncoding();
        Some(target)
    }

    fn prepare_phase10_graph(
        &mut self,
        graph: &ScenePhase10GraphPlan,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(ScenePhase10GraphPlan, Vec<NativeSceneWarning>), String> {
        let mut warnings = Vec::new();
        let mut visuals = Vec::new();
        let required_models = graph
            .visuals
            .iter()
            .filter_map(|visual| visual.puppet_path.clone())
            .collect::<BTreeSet<_>>();
        let mut required_shader_variants = BTreeSet::new();
        let mut required_output_keys = BTreeSet::new();
        let mut required_scratch_keys = BTreeSet::new();
        let mut required_named_target_keys = BTreeSet::new();
        let mut required_background_keys = BTreeSet::new();

        self.mdl_cache
            .retain(|path, _| required_models.contains(path));

        for visual in &graph.visuals {
            if let Some(puppet_path) = visual.puppet_path.as_ref() {
                if !self.mdl_cache.contains_key(puppet_path) {
                    let document = parse_scene_mdl_file(puppet_path).map_err(|error| {
                        format!(
                            "phase-10 Scene puppet {} could not be loaded: {error}",
                            puppet_path.display()
                        )
                    })?;
                    self.mdl_cache.insert(puppet_path.clone(), document);
                }
            }

            let mut draw_ready = self.ensure_phase10_base_texture_loaded(visual, required_keys)?;
            let passes = phase10_visual_pass_chain(visual);
            if phase10_visual_requires_offscreen_chain(visual) {
                let (width, height) = phase10_render_target_size(visual);
                required_output_keys.insert(phase10_output_texture_key(
                    visual.object_id,
                    width,
                    height,
                ));
                if passes.len() > 1 {
                    required_scratch_keys.insert(phase10_scratch_texture_key(width, height, 0));
                    required_scratch_keys.insert(phase10_scratch_texture_key(width, height, 1));
                }
                if phase10_visual_needs_background_snapshot(visual) {
                    required_background_keys.insert(phase10_background_texture_key(
                        visual.object_id,
                        width,
                        height,
                    ));
                }
                for resolved_pass in &passes {
                    if let Some(target_name) = phase10_resolved_pass_target_name(resolved_pass) {
                        required_named_target_keys.insert(phase10_named_target_texture_key(
                            visual.object_id,
                            target_name,
                            width,
                            height,
                        ));
                    }
                }
            }
            if passes.is_empty() {
                if draw_ready {
                    visuals.push(visual.clone());
                } else {
                    warnings.push(NativeSceneWarning::phase10_draw(
                        &visual.object_name,
                        "No draw-ready material or base texture remained after phase-10 preparation."
                            .to_string(),
                    ));
                }
                continue;
            } else {
                let mut shader_failed = false;
                for resolved_pass in &passes {
                    let shader_defines = phase10_pass_shader_defines(resolved_pass);
                    let variant_key = phase10_shader_variant_key(
                        &resolved_pass.pass.program,
                        &shader_defines,
                        resolved_pass.pass.blend_mode,
                    );
                    required_shader_variants.insert(variant_key.clone());
                    if !self.compiled_shader_variants.contains_key(&variant_key) {
                        match self.compile_phase10_shader_variant(
                            &resolved_pass.pass.program,
                            &shader_defines,
                            resolved_pass.pass.blend_mode,
                        ) {
                            Ok(pipeline) => {
                                self.compiled_shader_variants.insert(variant_key, pipeline);
                            }
                            Err(error) => {
                                warnings.push(NativeSceneWarning::phase10_draw(
                                    &visual.object_name,
                                    error,
                                ));
                                shader_failed = true;
                                break;
                            }
                        }
                    }
                    match self.ensure_phase10_pass_textures_loaded(resolved_pass, required_keys) {
                        Ok(pass_ready) => draw_ready |= pass_ready,
                        Err(error) => warnings
                            .push(NativeSceneWarning::phase10_draw(&visual.object_name, error)),
                    }
                }
                if shader_failed {
                    continue;
                }
            }

            if phase10_visual_first_pass_is_mask_alpha(visual) {
                let mask_apply_program = phase10_mask_apply_shader_program();
                let mask_apply_key = phase10_shader_variant_key(
                    &mask_apply_program,
                    &BTreeMap::new(),
                    visual.blend_mode,
                );
                required_shader_variants.insert(mask_apply_key.clone());
                if !self.compiled_shader_variants.contains_key(&mask_apply_key) {
                    match self.compile_phase10_shader_variant(
                        &mask_apply_program,
                        &BTreeMap::new(),
                        visual.blend_mode,
                    ) {
                        Ok(pipeline) => {
                            self.compiled_shader_variants
                                .insert(mask_apply_key, pipeline);
                        }
                        Err(error) => {
                            warnings
                                .push(NativeSceneWarning::phase10_draw(&visual.object_name, error));
                            continue;
                        }
                    }
                }
            }

            if draw_ready {
                visuals.push(visual.clone());
            } else {
                warnings.push(NativeSceneWarning::phase10_draw(
                    &visual.object_name,
                    "No draw-ready material or base texture remained after phase-10 preparation."
                        .to_string(),
                ));
            }
        }

        self.compiled_shader_variants
            .retain(|key, _| required_shader_variants.contains(key));
        self.phase10_output_textures
            .retain(|key, _| required_output_keys.contains(key));
        self.phase10_scratch_textures
            .retain(|key, _| required_scratch_keys.contains(key));
        self.phase10_named_target_textures
            .retain(|key, _| required_named_target_keys.contains(key));
        self.phase10_background_textures
            .retain(|key, _| required_background_keys.contains(key));

        Ok((
            ScenePhase10GraphPlan {
                visuals,
                consumed_visual_ids: graph.consumed_visual_ids.clone(),
            },
            warnings,
        ))
    }

    fn ensure_phase10_base_texture_loaded(
        &mut self,
        visual: &ScenePhase10VisualPlan,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<bool, String> {
        match visual.base_source_kind {
            Some(SceneRenderSourceKind::Image) => {
                let Some(base_texture_path) = visual.base_texture_path.as_ref() else {
                    return Ok(false);
                };
                self.ensure_phase10_texture_loaded(base_texture_path, required_keys)?;
                Ok(true)
            }
            Some(SceneRenderSourceKind::Video) => {
                Ok(self.video_sources.contains_key(&visual.object_id))
            }
            None => {
                if visual.base_color.alpha == 0 {
                    return Ok(false);
                }
                let (width, height) = phase10_render_target_size(visual);
                self.ensure_phase10_solid_texture(visual.base_color, width, height, required_keys)?;
                Ok(true)
            }
        }
    }

    fn ensure_phase10_pass_textures_loaded(
        &mut self,
        resolved_pass: &Phase10ResolvedPass<'_>,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<bool, String> {
        let mut pass_ready = false;
        for texture_path in resolved_pass
            .pass
            .textures
            .iter()
            .filter_map(|binding| binding.resolved_path.as_ref())
        {
            self.ensure_phase10_texture_loaded(texture_path, required_keys)?;
            pass_ready = true;
        }
        if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
            for texture_path in effect_pass.texture_overrides.iter().flatten() {
                self.ensure_phase10_texture_loaded(texture_path, required_keys)?;
                pass_ready = true;
            }
        }
        Ok(pass_ready)
    }

    fn ensure_phase10_solid_texture(
        &mut self,
        color: SceneRenderColor,
        width: usize,
        height: usize,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        let key = phase10_solid_texture_key(color, width, height);
        required_keys.insert(key.clone());
        if self.texture_cache.contains_key(&key) {
            return Ok(());
        }
        let texture = load_texture(
            &self.device,
            build_solid_texture_image(color, width, height),
        )
        .map_err(|error| format!("unable to upload phase-10 solid texture {key}: {error}"))?;
        self.texture_resolution_cache.insert(
            key.clone(),
            phase10_texture_metrics_from_size(width, height),
        );
        self.texture_cache.insert(key, texture);
        Ok(())
    }

    fn ensure_phase10_texture_loaded(
        &mut self,
        path: &Path,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        let key = phase10_texture_cache_key(path);
        required_keys.insert(key.clone());
        if self.texture_cache.contains_key(&key) {
            return Ok(());
        }

        let decoded = load_phase10_texture_source(path)?;
        let texture = load_texture(&self.device, decoded.image).map_err(|error| {
            format!(
                "unable to upload phase-10 texture {}: {error}",
                path.display()
            )
        })?;
        self.texture_resolution_cache
            .insert(key.clone(), decoded.metrics);
        self.texture_cache.insert(key, texture);
        Ok(())
    }
    fn compile_phase10_shader_variant(
        &self,
        program: &SceneShaderProgram,
        defines: &BTreeMap<String, i32>,
        blend_mode: SceneRenderBlendMode,
    ) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, String> {
        let source = load_shader_program_source(program, defines)?;
        let source = NSString::from_str(source.as_str());
        let library = self
            .device
            .newLibraryWithSource_options_error(&source, None)
            .map_err(|error| {
                format!(
                    "failed to compile phase-10 Scene shader {}: {error:?}",
                    program.metal_source_path.display()
                )
            })?;
        let vertex_name = NSString::from_str(program.vertex_entry);
        let fragment_name = NSString::from_str(program.fragment_entry);
        let vertex_function = library.newFunctionWithName(&vertex_name).ok_or_else(|| {
            format!(
                "phase-10 shader {} is missing vertex entry {}",
                program.metal_source_path.display(),
                program.vertex_entry
            )
        })?;
        let fragment_function = library.newFunctionWithName(&fragment_name).ok_or_else(|| {
            format!(
                "phase-10 shader {} is missing fragment entry {}",
                program.metal_source_path.display(),
                program.fragment_entry
            )
        })?;
        build_pipeline_state(
            self.device.as_ref(),
            vertex_function.as_ref(),
            fragment_function.as_ref(),
            blend_mode,
        )
    }

    fn draw_phase10_visual(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        projection: &SceneProjection,
        visual: &ScenePhase10VisualPlan,
        rendered_output: Option<&Retained<ProtocolObject<dyn MTLTexture>>>,
        elapsed_seconds: f64,
    ) {
        if let Some(output_texture) = rendered_output {
            self.draw_quad(
                encoder,
                output_texture.as_ref(),
                visual.blend_mode,
                projection,
                quad_primitive_from_render_quad(
                    visual.quad,
                    visual
                        .base_source_kind
                        .map(|_| visual.base_color)
                        .unwrap_or_default(),
                ),
            );
            return;
        }

        let passes = phase10_visual_pass_chain(visual);
        if passes.is_empty() {
            let Some(texture) = self.phase10_base_texture_for_visual(visual) else {
                return;
            };
            self.draw_quad(
                encoder,
                texture.texture.as_ref(),
                visual.blend_mode,
                projection,
                quad_primitive_from_render_quad(visual.quad, visual.base_color),
            );
            return;
        }

        if let Some(puppet_path) = visual.puppet_path.as_ref() {
            let Some(document) = self.mdl_cache.get(puppet_path) else {
                return;
            };
            let Some(mesh_frame) =
                evaluate_scene_mdl_mesh(document, &visual.animation_layers, elapsed_seconds)
            else {
                return;
            };
            if mesh_frame.positions.is_empty() || mesh_frame.indices.len() < 3 {
                return;
            }
            let base_texture = self.phase10_base_texture_for_visual(visual);
            let named_targets = BTreeMap::<String, Phase10TextureHandle>::new();
            for resolved_pass in &passes {
                let input_scope = Phase10PassInputScope {
                    local_current: base_texture.as_ref(),
                    previous_pass: base_texture.as_ref(),
                    background: None,
                    copied_background: None,
                    named_targets: &named_targets,
                };
                let pass_textures =
                    self.phase10_pass_textures_for(visual, resolved_pass, &input_scope);
                if pass_textures
                    .slots
                    .first()
                    .and_then(|slot| slot.as_ref())
                    .is_none()
                {
                    continue;
                }
                let uniforms = self.phase10_effect_uniforms_for_pass(
                    resolved_pass,
                    &pass_textures,
                    phase10_render_target_size(visual).0,
                    phase10_render_target_size(visual).1,
                    elapsed_seconds,
                );
                let shader_defines = phase10_pass_shader_defines(resolved_pass);
                self.draw_phase10_mesh(
                    encoder,
                    projection,
                    visual,
                    &mesh_frame,
                    resolved_pass.pass,
                    &shader_defines,
                    &pass_textures,
                    &uniforms,
                );
            }
        } else {
            let Some(texture) = self.phase10_base_texture_for_visual(visual) else {
                return;
            };
            self.draw_quad(
                encoder,
                texture.texture.as_ref(),
                visual.blend_mode,
                projection,
                quad_primitive_from_render_quad(visual.quad, visual.base_color),
            );
        }
    }

    fn phase10_base_texture_for_visual(
        &mut self,
        visual: &ScenePhase10VisualPlan,
    ) -> Option<Phase10TextureHandle> {
        match visual.base_source_kind {
            Some(SceneRenderSourceKind::Image) => visual
                .base_texture_path
                .as_deref()
                .and_then(|path| self.phase10_texture_for_path(path)),
            Some(SceneRenderSourceKind::Video) => {
                let source = self.video_sources.get_mut(&visual.object_id)?;
                match source.current_texture(&self.video_texture_cache, self.paused) {
                    Ok(texture) => texture.map(|texture| Phase10TextureHandle {
                        metrics: phase10_texture_metrics_from_texture(texture.as_ref()),
                        texture,
                    }),
                    Err(error) => {
                        let detail = video_texture_frame_warning(&visual.object_name, error);
                        let _ = diagnostic_service::record_warning(
                            &self.app,
                            DIAGNOSTIC_SUBSYSTEM,
                            &detail.code,
                            detail.message.clone(),
                            detail.detail_json(),
                        );
                        None
                    }
                }
            }
            None => {
                let (width, height) = phase10_render_target_size(visual);
                self.texture_cache
                    .get(&phase10_solid_texture_key(visual.base_color, width, height))
                    .cloned()
                    .map(|texture| Phase10TextureHandle {
                        texture,
                        metrics: *self
                            .texture_resolution_cache
                            .get(&phase10_solid_texture_key(visual.base_color, width, height))
                            .unwrap_or(&phase10_texture_metrics_from_size(width, height)),
                    })
            }
        }
    }

    fn phase10_texture_for_path(&self, path: &Path) -> Option<Phase10TextureHandle> {
        let key = phase10_texture_cache_key(path);
        let texture = self.texture_cache.get(&key)?.clone();
        let metrics = self
            .texture_resolution_cache
            .get(&key)
            .copied()
            .unwrap_or_else(|| phase10_texture_metrics_from_texture(texture.as_ref()));
        Some(Phase10TextureHandle { texture, metrics })
    }

    fn phase10_pass_textures_for(
        &self,
        visual: &ScenePhase10VisualPlan,
        resolved_pass: &Phase10ResolvedPass<'_>,
        input_scope: &Phase10PassInputScope<'_>,
    ) -> Phase10PassTextures {
        let mut slots = BTreeMap::<usize, Phase10TextureHandle>::new();

        match resolved_pass.context {
            Phase10PassContext::Base => {
                for binding in &resolved_pass.pass.textures {
                    let Some(texture) = binding
                        .resolved_path
                        .as_deref()
                        .and_then(|path| self.phase10_texture_for_path(path))
                    else {
                        continue;
                    };
                    slots.insert(binding.slot_index, texture);
                }
                if let Some(texture) = input_scope.local_current {
                    slots.entry(0).or_insert(texture.clone());
                }
            }
            Phase10PassContext::Effect(effect_pass) => {
                for (slot, source) in
                    phase10_effect_texture_slot_plan(&resolved_pass.pass.textures, effect_pass)
                {
                    let texture = match source {
                        Phase10EffectTextureSource::GraphInput(input_source) => {
                            input_scope.texture_for(&input_source)
                        }
                        Phase10EffectTextureSource::MaterialSlot(binding_slot) => resolved_pass
                            .pass
                            .textures
                            .iter()
                            .find(|binding| binding.slot_index == binding_slot)
                            .and_then(|binding| binding.resolved_path.as_deref())
                            .and_then(|path| self.phase10_texture_for_path(path)),
                        Phase10EffectTextureSource::OverrideSlot(override_slot) => effect_pass
                            .texture_overrides
                            .get(override_slot)
                            .and_then(|path| path.as_deref())
                            .and_then(|path| self.phase10_texture_for_path(path)),
                    };
                    if let Some(texture) = texture {
                        slots.insert(slot, texture);
                    }
                }
            }
        }

        if slots.is_empty() && visual.base_source_kind.is_none() && visual.base_color.alpha > 0 {
            let (width, height) = phase10_render_target_size(visual);
            if let Some(texture) = self
                .texture_cache
                .get(&phase10_solid_texture_key(visual.base_color, width, height))
                .cloned()
            {
                slots.insert(
                    0,
                    Phase10TextureHandle {
                        metrics: *self
                            .texture_resolution_cache
                            .get(&phase10_solid_texture_key(visual.base_color, width, height))
                            .unwrap_or(&phase10_texture_metrics_from_size(width, height)),
                        texture,
                    },
                );
            }
        }

        let max_slot = slots.keys().next_back().copied().unwrap_or(0);
        let mut ordered = vec![None; max_slot + 1];
        for (slot, texture) in slots {
            ordered[slot] = Some(texture);
        }

        Phase10PassTextures { slots: ordered }
    }

    fn phase10_effect_uniforms_for_pass(
        &self,
        resolved_pass: &Phase10ResolvedPass<'_>,
        pass_textures: &Phase10PassTextures,
        width: usize,
        height: usize,
        elapsed_seconds: f64,
    ) -> Phase10EffectUniforms {
        let effect_kind = phase10_effect_family_from_program(&resolved_pass.pass.program);
        let mut uniforms = Phase10EffectUniforms {
            color: [1.0, 1.0, 1.0, 1.0],
            user0: [0.0, 0.0, 0.0, 0.0],
            user1: [0.0, 0.0, 0.0, 0.0],
            primary_resolution: phase10_texture_resolution(
                pass_textures.slots.first().and_then(|slot| slot.as_ref()),
            ),
            slot1_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(1).and_then(|slot| slot.as_ref()),
            ),
            slot2_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(2).and_then(|slot| slot.as_ref()),
            ),
            slot3_resolution: phase10_optional_texture_resolution(
                pass_textures.slots.get(3).and_then(|slot| slot.as_ref()),
            ),
            texel_size: phase10_texel_size(
                pass_textures.slots.first().and_then(|slot| slot.as_ref()),
            ),
            aux_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(1).and_then(|slot| slot.as_ref()),
            ),
            aux2_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(2).and_then(|slot| slot.as_ref()),
            ),
            aux3_texel_size: phase10_optional_texel_size(
                pass_textures.slots.get(3).and_then(|slot| slot.as_ref()),
            ),
            screen_size: [width.max(1) as f32, height.max(1) as f32],
            time: elapsed_seconds as f32,
            intensity: 1.0,
            speed: 1.0,
            radius: 1.0,
            angle: 0.0,
        };
        let uniform_values = phase10_effect_uniform_values(resolved_pass);

        match effect_kind {
            Some(SceneCompatEffectKind::Pulse) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["amount", "pulseamount"], 1.0);
                uniforms.speed =
                    phase10_uniform_float(&uniform_values, &["speed", "pulsespeed"], 3.0);
                uniforms.user0[0] =
                    phase10_uniform_float(&uniform_values, &["phase", "pulsephase"], 0.0);
                uniforms.user0[1] =
                    phase10_uniform_float(&uniform_values, &["power"], 1.0).max(0.001);
                let bounds = phase10_uniform_vec2(
                    &uniform_values,
                    &["bounds", "pulsethresholds"],
                    [0.0, 1.0],
                );
                uniforms.user0[2] = bounds[0];
                uniforms.user0[3] = bounds[1];
                uniforms.radius =
                    phase10_uniform_float(&uniform_values, &["noisespeed"], 0.5).max(0.0);
                uniforms.angle = phase10_uniform_float(&uniform_values, &["noiseamount"], 0.0);
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["tintlow", "tintcolor1"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                let tint_high = phase10_uniform_color(
                    &uniform_values,
                    &["tinthigh", "tintcolor2"],
                    [1.0, 1.0, 1.0, 1.0],
                );
                uniforms.user1 = [tint_high[0], tint_high[1], tint_high[2], 1.0];
            }
            Some(SceneCompatEffectKind::Shake) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["strength", "amp"], 0.1);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
                let bounds =
                    phase10_uniform_vec2(&uniform_values, &["bounds", "gbounds"], [0.0, 1.0]);
                let friction =
                    phase10_uniform_vec2(&uniform_values, &["friction", "gfriction"], [1.0, 1.0]);
                uniforms.user0[0] = bounds[0];
                uniforms.user0[1] = bounds[1];
                uniforms.user1[0] = friction[0];
                uniforms.user1[1] = friction[1];
            }
            Some(SceneCompatEffectKind::WaterRipple) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["ripplestrength", "strength"], 0.1);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["animationspeed"], 0.15);
                uniforms.radius = phase10_uniform_float(&uniform_values, &["scale"], 1.0);
                uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["scrollspeed"], 0.0);
                uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["ratio"], 1.0);
                uniforms.angle =
                    phase10_uniform_float(&uniform_values, &["scrolldirection", "direction"], 0.0);
            }
            Some(SceneCompatEffectKind::WaterWaves) => {
                uniforms.intensity = phase10_uniform_float(&uniform_values, &["strength"], 0.1);
                uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 5.0);
                uniforms.angle = phase10_uniform_float(&uniform_values, &["direction"], 0.0);
                let direction = rotate2d([0.0, 1.0], uniforms.angle);
                uniforms.user0[0] = direction[0];
                uniforms.user0[1] = direction[1];
                uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["scale"], 200.0);
                uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["exponent"], 1.0);
            }
            Some(SceneCompatEffectKind::Tint) => {
                uniforms.intensity =
                    phase10_uniform_float(&uniform_values, &["alpha", "blendalpha"], 1.0);
                uniforms.color = phase10_uniform_color(
                    &uniform_values,
                    &["color", "tintcolor"],
                    [1.0, 0.0, 0.0, 1.0],
                );
            }
            Some(SceneCompatEffectKind::Scroll) => {
                uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["speedx"], 0.2);
                uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["speedy"], 0.2);
                let repeat =
                    phase10_uniform_vec2(&uniform_values, &["repeat", "scale"], [1.0, 1.0]);
                uniforms.user0[2] = repeat[0];
                uniforms.user0[3] = repeat[1];
            }
            _ => {}
        }

        uniforms
    }

    fn encode_phase10_pass(
        &self,
        command_buffer: &ProtocolObject<dyn MTLCommandBuffer>,
        target: &Retained<ProtocolObject<dyn MTLTexture>>,
        pass: &SceneMaterialPassPlan,
        shader_defines: &BTreeMap<String, i32>,
        pass_textures: &Phase10PassTextures,
        uniforms: &Phase10EffectUniforms,
        previous_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    ) -> bool {
        let variant_key =
            phase10_shader_variant_key(&pass.program, shader_defines, pass.blend_mode);
        let Some(pipeline) = self.compiled_shader_variants.get(&variant_key) else {
            return false;
        };

        let descriptor = MTLRenderPassDescriptor::new();
        unsafe {
            let attachment = descriptor.colorAttachments().objectAtIndexedSubscript(0);
            attachment.setTexture(Some(target.as_ref()));
            attachment.setLoadAction(MTLLoadAction::Clear);
            attachment.setStoreAction(MTLStoreAction::Store);
            attachment.setClearColor(objc2_metal::MTLClearColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            });
        }
        let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&descriptor) else {
            return false;
        };

        if phase10_alpha_prefill_required(pass.blend_mode) {
            if let Some(previous_texture) = previous_texture {
                self.draw_phase10_fullscreen_texture(
                    &encoder,
                    previous_texture,
                    SceneRenderColor::default(),
                    SceneRenderBlendMode::Normal,
                );
            }
        }

        self.draw_phase10_fullscreen_pass(
            &encoder,
            pipeline.as_ref(),
            pass_textures,
            uniforms,
            SceneRenderColor::default(),
        );
        encoder.endEncoding();
        true
    }

    fn ensure_phase10_output_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_output_textures,
            key,
            width,
            height,
        )
    }

    fn ensure_phase10_scratch_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_scratch_textures,
            key,
            width,
            height,
        )
    }

    fn ensure_phase10_named_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_named_target_textures,
            key,
            width,
            height,
        )
    }

    fn ensure_phase10_background_target(
        &mut self,
        key: &str,
        width: usize,
        height: usize,
    ) -> Option<Phase10TextureHandle> {
        ensure_phase10_render_target_in_store(
            self.device.as_ref(),
            &mut self.phase10_background_textures,
            key,
            width,
            height,
        )
    }

    fn draw_phase10_fullscreen_texture(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        texture: Retained<ProtocolObject<dyn MTLTexture>>,
        tint: SceneRenderColor,
        blend_mode: SceneRenderBlendMode,
    ) {
        let pass_textures = Phase10PassTextures {
            slots: vec![Some(Phase10TextureHandle {
                metrics: phase10_texture_metrics_from_texture(texture.as_ref()),
                texture,
            })],
        };
        self.draw_phase10_fullscreen_pass(
            encoder,
            self.pipeline_for(blend_mode),
            &pass_textures,
            &Phase10EffectUniforms::default(),
            tint,
        );
    }

    fn draw_phase10_fullscreen_pass(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
        pass_textures: &Phase10PassTextures,
        uniforms: &Phase10EffectUniforms,
        tint: SceneRenderColor,
    ) {
        let vertices = phase10_fullscreen_vertices(tint);
        self.draw_phase10_vertices_with_pipeline(
            encoder,
            pipeline,
            pass_textures,
            uniforms,
            &vertices,
        );
    }

    fn draw_phase10_mesh(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        projection: &SceneProjection,
        visual: &ScenePhase10VisualPlan,
        mesh_frame: &crate::services::scene_mdl_service::SceneMdlMeshFrame,
        pass: &SceneMaterialPassPlan,
        shader_defines: &BTreeMap<String, i32>,
        pass_textures: &Phase10PassTextures,
        uniforms: &Phase10EffectUniforms,
    ) {
        let Some(vertices) = build_projected_puppet_mesh_vertices(
            mesh_frame,
            visual.world_position,
            visual.world_scale,
            visual.world_angles,
            visual.quad.opacity,
            projection,
        ) else {
            return;
        };
        let variant_key =
            phase10_shader_variant_key(&pass.program, shader_defines, pass.blend_mode);
        let Some(pipeline) = self.compiled_shader_variants.get(&variant_key) else {
            return;
        };
        self.draw_phase10_vertices_with_pipeline(
            encoder,
            pipeline.as_ref(),
            pass_textures,
            uniforms,
            &vertices,
        );
    }

    fn draw_phase10_vertices_with_pipeline(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
        pass_textures: &Phase10PassTextures,
        uniforms: &Phase10EffectUniforms,
        vertices: &[SceneVertex],
    ) {
        let byte_len = std::mem::size_of_val(vertices);
        if byte_len == 0 {
            return;
        }
        let Some(vertices_bytes) = NonNull::new(vertices.as_ptr() as *mut c_void) else {
            return;
        };
        let Some(uniform_bytes) =
            NonNull::new(uniforms as *const Phase10EffectUniforms as *mut c_void)
        else {
            return;
        };

        encoder.setRenderPipelineState(pipeline);
        unsafe {
            match scene_vertex_upload_strategy(byte_len) {
                SceneVertexUploadStrategy::InlineBytes => {
                    encoder.setVertexBytes_length_atIndex(vertices_bytes, byte_len, 0);
                }
                SceneVertexUploadStrategy::SharedBuffer => {
                    let Some(vertex_buffer) = self.device.newBufferWithBytes_length_options(
                        vertices_bytes,
                        byte_len,
                        MTLResourceOptions::StorageModeShared,
                    ) else {
                        return;
                    };
                    encoder.setVertexBuffer_offset_atIndex(Some(vertex_buffer.as_ref()), 0, 0);
                }
            }
            encoder.setVertexBytes_length_atIndex(
                uniform_bytes,
                std::mem::size_of::<Phase10EffectUniforms>(),
                1,
            );
            encoder.setFragmentBytes_length_atIndex(
                uniform_bytes,
                std::mem::size_of::<Phase10EffectUniforms>(),
                0,
            );
            for slot in 0..4 {
                let texture = pass_textures
                    .slots
                    .get(slot)
                    .and_then(|texture| texture.as_ref())
                    .map(|texture| texture.texture.as_ref());
                encoder.setFragmentTexture_atIndex(texture, slot);
            }
            encoder.drawPrimitives_vertexStart_vertexCount(
                MTLPrimitiveType::Triangle,
                0,
                vertices.len(),
            );
        }
    }

    fn frame_delta_seconds(&mut self) -> f64 {
        let now = Instant::now();
        let delta_seconds = now.duration_since(self.last_frame_at).as_secs_f64();
        self.last_frame_at = now;
        delta_seconds.clamp(1.0 / 240.0, 0.25)
    }

    fn update_input_frame(
        &mut self,
        view: &MTKView,
        plan: &SceneRenderPlan,
        delta_seconds: f64,
    ) -> SceneInputCoordinatorFrame {
        let projection = scene_input_projection_for_view(&self.app, view, plan);
        self.input_coordinator.update(SceneInputCoordinatorUpdate {
            target: projection.target,
            camera: plan.camera.clone(),
            canvas_width: plan.canvas_width,
            canvas_height: plan.canvas_height,
            delta_seconds,
            scene_time_seconds: self.animation_time_seconds,
            projection_diagnostics: projection.diagnostics,
        })
    }

    fn pipeline_for(
        &self,
        blend_mode: SceneRenderBlendMode,
    ) -> &ProtocolObject<dyn MTLRenderPipelineState> {
        match blend_mode {
            SceneRenderBlendMode::Normal => self.pipelines.normal.as_ref(),
            SceneRenderBlendMode::Additive => self.pipelines.additive.as_ref(),
            SceneRenderBlendMode::Multiply => self.pipelines.multiply.as_ref(),
        }
    }

    fn ensure_visual_texture_loaded(
        &mut self,
        item: &SceneRenderVisualItem,
        key: &str,
    ) -> Result<(), String> {
        if self.texture_cache.contains_key(key) {
            return Ok(());
        }

        let image = image::open(&item.texture_path).map_err(|error| {
            format!(
                "unable to decode texture {}: {error}",
                item.texture_path.display()
            )
        })?;
        let metrics =
            phase10_texture_metrics_from_size(image.width() as usize, image.height() as usize);
        let texture = load_texture(&self.device, image).map_err(|error| {
            format!(
                "unable to upload texture {}: {error}",
                item.texture_path.display()
            )
        })?;
        self.texture_resolution_cache
            .insert(key.to_string(), metrics);
        self.texture_cache.insert(key.to_string(), texture);
        Ok(())
    }

    fn ensure_text_texture_loaded(
        &mut self,
        item: &SceneRenderTextItem,
        key: &str,
    ) -> Result<Vec<NativeSceneWarning>, String> {
        if self.text_texture_cache.contains_key(key) {
            return Ok(Vec::new());
        }

        let rasterized = rasterize_text_texture(item)?;
        let texture = load_texture(&self.device, rasterized.image).map_err(|error| {
            format!(
                "unable to upload text texture {}: {error}",
                item.object_name
            )
        })?;
        self.text_texture_cache.insert(key.to_string(), texture);
        Ok(rasterized.warnings)
    }

    fn ensure_procedural_texture(
        &mut self,
        key: &str,
        image: DynamicImage,
        required_keys: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        required_keys.insert(key.to_string());
        if self.texture_cache.contains_key(key) {
            return Ok(());
        }

        let texture = load_texture(&self.device, image)
            .map_err(|error| format!("unable to upload procedural texture {key}: {error}"))?;
        self.texture_resolution_cache.insert(
            key.to_string(),
            phase10_texture_metrics_from_texture(texture.as_ref()),
        );
        self.texture_cache.insert(key.to_string(), texture);
        Ok(())
    }

    fn sync_video_sources(
        &mut self,
        visuals: &[SceneRenderVisualItem],
        paused: bool,
    ) -> Vec<NativeSceneWarning> {
        let current_paths = self
            .video_sources
            .iter()
            .map(|(object_id, source)| (*object_id, source.state()))
            .collect::<BTreeMap<_, _>>();
        let desired = scene_video_texture_service::desired_video_texture_sources(visuals);
        let plan = scene_video_texture_service::plan_video_texture_source_sync(
            &current_paths,
            &desired,
            paused,
        );
        let mut warnings = Vec::new();

        for action in plan.actions {
            match action {
                SceneVideoTextureLifecycleAction::Remove { object_id, .. } => {
                    if let Some(mut source) = self.video_sources.remove(&object_id) {
                        source.stop();
                    }
                }
                SceneVideoTextureLifecycleAction::SetPaused { object_id, paused } => {
                    if let Some(source) = self.video_sources.get_mut(&object_id) {
                        source.set_paused(paused);
                    }
                }
                SceneVideoTextureLifecycleAction::Replace { source, paused, .. } => {
                    if let Some(existing) = self.video_sources.get_mut(&source.object_id) {
                        existing.stop();
                    }
                    match NativeSceneVideoSource::new(
                        source.object_id,
                        source.asset_path.clone(),
                        paused,
                    ) {
                        Ok(next_source) => {
                            self.video_sources.insert(source.object_id, next_source);
                        }
                        Err(error) => {
                            self.video_sources.remove(&source.object_id);
                            warnings.push(video_texture_source_warning(&source, error));
                        }
                    }
                }
                SceneVideoTextureLifecycleAction::Create { source, paused } => {
                    match NativeSceneVideoSource::new(
                        source.object_id,
                        source.asset_path.clone(),
                        paused,
                    ) {
                        Ok(next_source) => {
                            self.video_sources.insert(source.object_id, next_source);
                        }
                        Err(error) => warnings.push(video_texture_source_warning(&source, error)),
                    }
                }
            }
        }
        warnings
    }

    fn clear_video_sources(&mut self) {
        for (_, source) in self.video_sources.iter_mut() {
            source.stop();
        }
        self.video_sources.clear();
        self.video_texture_cache.flush(0);
    }

    fn video_texture_for_item(
        &mut self,
        item: &SceneRenderVisualItem,
    ) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
        let source = self.video_sources.get_mut(&item.object_id)?;
        match source.current_texture(&self.video_texture_cache, self.paused) {
            Ok(texture) => texture,
            Err(error) => {
                let detail = video_texture_frame_warning(&item.object_name, error);
                let _ = diagnostic_service::record_warning(
                    &self.app,
                    DIAGNOSTIC_SUBSYSTEM,
                    &detail.code,
                    detail.message.clone(),
                    detail.detail_json(),
                );
                None
            }
        }
    }

    fn draw_quad(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        texture: &ProtocolObject<dyn MTLTexture>,
        blend_mode: SceneRenderBlendMode,
        projection: &SceneProjection,
        quad: SceneQuadPrimitive,
    ) {
        let vertices = build_projected_quad_vertices(quad, projection);
        self.draw_vertices(encoder, texture, blend_mode, &vertices);
    }

    fn draw_vertices(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        texture: &ProtocolObject<dyn MTLTexture>,
        blend_mode: SceneRenderBlendMode,
        vertices: &[SceneVertex],
    ) {
        let byte_len = std::mem::size_of_val(vertices);
        if byte_len == 0 {
            return;
        }
        let Some(vertices_bytes) = NonNull::new(vertices.as_ptr() as *mut c_void) else {
            return;
        };
        encoder.setRenderPipelineState(self.pipeline_for(blend_mode));
        unsafe {
            match scene_vertex_upload_strategy(byte_len) {
                SceneVertexUploadStrategy::InlineBytes => {
                    encoder.setVertexBytes_length_atIndex(vertices_bytes, byte_len, 0);
                }
                SceneVertexUploadStrategy::SharedBuffer => {
                    let Some(vertex_buffer) = self.device.newBufferWithBytes_length_options(
                        vertices_bytes,
                        byte_len,
                        MTLResourceOptions::StorageModeShared,
                    ) else {
                        return;
                    };
                    encoder.setVertexBuffer_offset_atIndex(Some(vertex_buffer.as_ref()), 0, 0);
                }
            }
            encoder.setFragmentTexture_atIndex(Some(texture), 0);
            encoder.drawPrimitives_vertexStart_vertexCount(
                MTLPrimitiveType::Triangle,
                0,
                vertices.len(),
            );
        }
    }

    fn draw_audio_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        white_texture: &ProtocolObject<dyn MTLTexture>,
        projection: &SceneProjection,
        item: &SceneRenderAudioItem,
        shared_audio_snapshot: Option<&audio_input_service::AudioSnapshot>,
        now_ms: u64,
    ) {
        let sound_levels = scene_soundscape_audio_levels(&self.app, item.bar_count);
        let levels = self.audio_coordinator.levels_for_count(
            shared_audio_snapshot,
            sound_levels.as_deref(),
            item.bar_count,
            now_ms,
        );
        let origin_x = item.quad.left + item.quad.width / 2.0;
        let origin_y = item.quad.top + item.quad.height / 2.0;
        for index in 0..item.bar_count {
            let level = clamp_f64(
                levels.get(index).copied().unwrap_or_default() * item.volume_factor,
                0.0,
                1.0,
            );
            let bounded = item.normalized_lower_bound + (1.0 - item.normalized_lower_bound) * level;
            let scale_y = clamp_f64(bounded.max(item.min_scale), item.min_scale, 1.0);
            let bar_height = item.drawable_height * scale_y;
            let left = item.quad.left + index as f64 * (item.bar_width + item.gap);
            let top = item.quad.top + item.drawable_top + (item.drawable_height - bar_height);
            let quad = SceneQuadPrimitive {
                left,
                top,
                width: item.bar_width,
                height: bar_height.max(1.0),
                rotation: scene_audio_bar_rotation(item.quad.rotation),
                opacity: item.quad.opacity * clamp_f64(0.35 + scale_y * 0.65, 0.35, 1.0),
                flip_x: item.quad.flip_x,
                flip_y: item.quad.flip_y,
                uv_rect: full_quad_uv_rect(),
                color: item.color,
                transform_origin_x: origin_x,
                transform_origin_y: origin_y,
            };
            self.draw_quad(
                encoder,
                white_texture,
                SceneRenderBlendMode::Normal,
                projection,
                quad,
            );
        }
    }

    fn advance_particle_items(
        &mut self,
        items: &[SceneRenderParticleItem],
        input_response: SceneInputResponse,
        now_ms: f64,
    ) {
        let cursor = input_response
            .cursor()
            .map(|(x, y)| SceneParticleCursor { x, y });
        if !self.paused {
            self.particle_scheduler.advance(cursor, items, now_ms);
        } else {
            self.particle_scheduler.pause_cursor(cursor);
        }
    }

    fn advance_rope_particle_items(
        &mut self,
        items: &[SceneRenderRopeParticleItem],
        input_response: SceneInputResponse,
        now_ms: f64,
    ) {
        let cursor = input_response
            .cursor()
            .map(|(x, y)| SceneParticleCursor { x, y });
        if !self.paused {
            self.rope_particle_scheduler.advance(cursor, items, now_ms);
        } else {
            self.rope_particle_scheduler.pause_cursor(cursor);
        }
    }

    fn draw_particle_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        white_texture: &ProtocolObject<dyn MTLTexture>,
        petal_texture: Option<&ProtocolObject<dyn MTLTexture>>,
        projection: &SceneProjection,
        plan: &SceneRenderPlan,
        item: &SceneRenderParticleItem,
        now_ms: f64,
    ) {
        match item.particle_kind {
            crate::models::SceneParticleKind::LineTrail => {
                for segment in
                    self.particle_scheduler
                        .line_primitives(item, plan.canvas_height, now_ms)
                {
                    self.draw_quad(
                        encoder,
                        white_texture,
                        SceneRenderBlendMode::Additive,
                        projection,
                        quad_primitive_from_particle(segment),
                    );
                }
            }
            crate::models::SceneParticleKind::PetalTrail => {
                let Some(petal_texture) = petal_texture else {
                    return;
                };
                for petal in
                    self.particle_scheduler
                        .petal_primitives(item, plan.canvas_height, now_ms)
                {
                    self.draw_quad(
                        encoder,
                        petal_texture,
                        SceneRenderBlendMode::Normal,
                        projection,
                        quad_primitive_from_particle(petal),
                    );
                }
            }
        }
    }

    fn draw_sprite_particle_item(
        &mut self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        projection: &SceneProjection,
        plan: &SceneRenderPlan,
        item: &SceneRenderSpriteParticleItem,
        now_ms: f64,
    ) {
        for primitive in self
            .sprite_particle_scheduler
            .primitives(item, plan.canvas_height, now_ms)
        {
            let key = phase10_texture_cache_key(&primitive.texture_path);
            let Some(texture) = self.texture_cache.get(&key).cloned() else {
                continue;
            };
            self.draw_quad(
                encoder,
                texture.as_ref(),
                primitive.blend_mode,
                projection,
                quad_primitive_from_sprite_particle(primitive),
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn scene_audio_bar_rotation(rotation: f64) -> f64 {
    -rotation
}

#[cfg(target_os = "macos")]
fn scene_debug_layout_enabled() -> bool {
    matches!(
        std::env::var("SCENE_DEBUG_LAYOUT").ok().as_deref(),
        Some("1" | "true" | "TRUE")
    )
}

#[cfg(target_os = "macos")]
fn visual_texture_cache_key(item: &SceneRenderVisualItem) -> String {
    let source_prefix = match item.source_kind {
        SceneRenderSourceKind::Image => "image",
        SceneRenderSourceKind::Video => "video",
    };
    format!("{source_prefix}:{}", item.texture_path.display())
}

#[cfg(target_os = "macos")]
fn should_retain_visual_in_draw_plan(
    item: &SceneRenderVisualItem,
    phase10_consumed_ids: &BTreeSet<u32>,
    image_texture_loaded: bool,
) -> bool {
    phase10_consumed_ids.contains(&item.object_id)
        || matches!(item.source_kind, SceneRenderSourceKind::Video)
        || image_texture_loaded
}

#[cfg(all(target_os = "macos", test))]
fn compile_scene_shader_program_pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
    blend_mode: SceneRenderBlendMode,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, String> {
    let source = load_shader_program_source(program, defines)?;
    let source = NSString::from_str(source.as_str());
    let library = device
        .newLibraryWithSource_options_error(&source, None)
        .map_err(|error| {
            format!(
                "failed to compile phase-10 Scene shader {}: {error:?}",
                program.metal_source_path.display()
            )
        })?;
    let vertex_name = NSString::from_str(program.vertex_entry);
    let fragment_name = NSString::from_str(program.fragment_entry);
    let vertex_function = library.newFunctionWithName(&vertex_name).ok_or_else(|| {
        format!(
            "phase-10 shader {} is missing vertex entry {}",
            program.metal_source_path.display(),
            program.vertex_entry
        )
    })?;
    let fragment_function = library.newFunctionWithName(&fragment_name).ok_or_else(|| {
        format!(
            "phase-10 shader {} is missing fragment entry {}",
            program.metal_source_path.display(),
            program.fragment_entry
        )
    })?;
    build_pipeline_state(
        device,
        vertex_function.as_ref(),
        fragment_function.as_ref(),
        blend_mode,
    )
}

#[cfg(target_os = "macos")]
fn phase10_texture_cache_key(path: &Path) -> String {
    format!("phase10:texture:{}", path.display())
}

#[cfg(target_os = "macos")]
fn phase10_shader_variant_key(
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
    blend_mode: SceneRenderBlendMode,
) -> String {
    let mut key = format!(
        "phase10:shader:{:?}:{}:{:?}",
        program.kind,
        program.metal_source_path.display(),
        blend_mode
    );
    for (name, value) in merged_shader_defines(program, defines) {
        key.push('|');
        key.push_str(&name);
        key.push('=');
        key.push_str(&value.to_string());
    }
    key
}

#[cfg(target_os = "macos")]
fn phase10_base_passes(visual: &ScenePhase10VisualPlan) -> &[SceneMaterialPassPlan] {
    &visual.material.passes
}

#[cfg(target_os = "macos")]
fn phase10_visual_requires_offscreen_chain(visual: &ScenePhase10VisualPlan) -> bool {
    if visual.puppet_path.is_some() {
        return phase10_visual_pass_chain(visual).len() > 1;
    }
    !phase10_base_passes(visual).is_empty() || !visual.effect_chain.is_empty()
}

#[cfg(target_os = "macos")]
fn phase10_visual_first_pass_is_mask_alpha(visual: &ScenePhase10VisualPlan) -> bool {
    phase10_base_passes(visual)
        .first()
        .map(|pass| pass.program.kind == SceneShaderProgramKind::MaskAlpha)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn phase10_mask_apply_shader_program() -> SceneShaderProgram {
    use crate::services::scene_shader_material_service::SceneShaderProgramKind;
    SceneShaderProgram {
        key: "clippingmaskimage4-apply".to_string(),
        kind: SceneShaderProgramKind::MaskApply,
        metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-mask-apply.metal"),
        vertex_entry: "compat_mask_apply_vertex",
        fragment_entry: "compat_mask_apply_fragment",
        variant_defines: BTreeMap::new(),
    }
}

#[cfg(target_os = "macos")]
fn phase10_render_target_size(visual: &ScenePhase10VisualPlan) -> (usize, usize) {
    (
        visual.quad.width.abs().max(1.0).ceil() as usize,
        visual.quad.height.abs().max(1.0).ceil() as usize,
    )
}

#[cfg(target_os = "macos")]
fn phase10_visual_needs_background_snapshot(visual: &ScenePhase10VisualPlan) -> bool {
    visual.effect_chain.iter().any(|effect| {
        effect.passes.iter().any(|pass| {
            pass.copy_background
                || pass.input_bindings.iter().any(|binding| {
                    matches!(
                        binding.source,
                        ScenePhase10InputSource::Background
                            | ScenePhase10InputSource::CopiedBackground
                    )
                })
        })
    })
}

#[cfg(target_os = "macos")]
fn phase10_resolved_pass_target_name<'a>(
    resolved_pass: &'a Phase10ResolvedPass<'_>,
) -> Option<&'a str> {
    match resolved_pass.context {
        Phase10PassContext::Effect(effect_pass) => effect_pass.target_name.as_deref(),
        Phase10PassContext::Base => None,
    }
}

#[cfg(target_os = "macos")]
fn phase10_local_background_projection(
    visual: &ScenePhase10VisualPlan,
    width: usize,
    height: usize,
) -> SceneProjection {
    SceneProjection {
        scene_origin_x: -visual.quad.left,
        scene_origin_y: -visual.quad.top,
        scene_canvas_height: visual.quad.height.abs().max(1.0),
        camera_scale: 1.0,
        view_width: width.max(1) as f64,
        view_height: height.max(1) as f64,
    }
}

#[cfg(target_os = "macos")]
fn phase10_puppet_offscreen_projection(
    visual: &ScenePhase10VisualPlan,
    canvas_height: f64,
    width: usize,
    height: usize,
) -> SceneProjection {
    SceneProjection {
        scene_origin_x: -visual.quad.left,
        scene_origin_y: -visual.quad.top,
        scene_canvas_height: canvas_height,
        camera_scale: 1.0,
        view_width: width.max(1) as f64,
        view_height: height.max(1) as f64,
    }
}

#[cfg(target_os = "macos")]
fn phase10_fullscreen_vertices(tint: SceneRenderColor) -> [SceneVertex; 6] {
    let color = color_to_shader(tint);
    [
        SceneVertex {
            position: [-1.0, -1.0],
            uv: [0.0, 1.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [1.0, -1.0],
            uv: [1.0, 1.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [-1.0, 1.0],
            uv: [0.0, 0.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [-1.0, 1.0],
            uv: [0.0, 0.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [1.0, -1.0],
            uv: [1.0, 1.0],
            color,
            opacity: 1.0,
        },
        SceneVertex {
            position: [1.0, 1.0],
            uv: [1.0, 0.0],
            color,
            opacity: 1.0,
        },
    ]
}

#[cfg(target_os = "macos")]
fn phase10_solid_texture_key(color: SceneRenderColor, width: usize, height: usize) -> String {
    format!(
        "phase10:solid:{:02x}{:02x}{:02x}{:02x}:{width}x{height}",
        color.red, color.green, color.blue, color.alpha,
    )
}

#[cfg(target_os = "macos")]
fn phase10_output_texture_key(object_id: u32, width: usize, height: usize) -> String {
    format!("phase10:output:{object_id}:{width}x{height}")
}

#[cfg(target_os = "macos")]
fn phase10_scratch_texture_key(width: usize, height: usize, slot: usize) -> String {
    format!("phase10:scratch:{width}x{height}:{slot}")
}

#[cfg(target_os = "macos")]
fn phase10_named_target_texture_key(
    object_id: u32,
    target_name: &str,
    width: usize,
    height: usize,
) -> String {
    format!("phase10:named:{object_id}:{target_name}:{width}x{height}")
}

#[cfg(target_os = "macos")]
fn phase10_background_texture_key(object_id: u32, width: usize, height: usize) -> String {
    format!("phase10:background:{object_id}:{width}x{height}")
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase10EffectTextureSource {
    GraphInput(ScenePhase10InputSource),
    MaterialSlot(usize),
    OverrideSlot(usize),
}

#[cfg(target_os = "macos")]
fn phase10_effect_texture_slot_plan(
    material_textures: &[SceneMaterialTextureBinding],
    effect_pass: &ScenePhase10EffectPassNode,
) -> BTreeMap<usize, Phase10EffectTextureSource> {
    let mut slots = BTreeMap::new();
    for binding in &effect_pass.input_bindings {
        slots.insert(
            binding.slot,
            Phase10EffectTextureSource::GraphInput(binding.source.clone()),
        );
    }
    for binding in material_textures {
        if binding.texture_name.is_some() && binding.resolved_path.is_some() {
            slots.insert(
                binding.slot_index,
                Phase10EffectTextureSource::MaterialSlot(binding.slot_index),
            );
        }
    }
    for (slot, texture) in effect_pass.texture_overrides.iter().enumerate() {
        if texture.is_some() {
            slots.insert(slot, Phase10EffectTextureSource::OverrideSlot(slot));
        }
    }
    slots
}

#[cfg(target_os = "macos")]
fn phase10_pass_shader_defines(resolved_pass: &Phase10ResolvedPass<'_>) -> BTreeMap<String, i32> {
    let Some(effect_kind) = phase10_effect_family_from_program(&resolved_pass.pass.program) else {
        return resolved_pass.pass.combos.clone();
    };
    let Some(contract) = phase10b_effect_contract_for_kind(effect_kind) else {
        return resolved_pass.pass.combos.clone();
    };
    let mut defines = contract
        .supported_combo_defaults
        .iter()
        .map(|(name, value)| ((*name).to_string(), *value))
        .collect::<BTreeMap<_, _>>();
    for (name, value) in &resolved_pass.pass.combos {
        defines.insert(name.clone(), *value);
    }
    if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
        let slot_plan = phase10_effect_texture_slot_plan(&resolved_pass.pass.textures, effect_pass);
        for slot_contract in contract.runtime_binding_layout {
            if !slot_plan.contains_key(&slot_contract.slot) {
                continue;
            }
            match slot_contract.semantic {
                crate::services::scene_shader_material_service::ScenePhase10bBindingSemantic::OpacityMask => {
                    if !resolved_pass
                        .pass
                        .combos
                        .keys()
                        .any(|name| name.eq_ignore_ascii_case("MASK"))
                    {
                        defines.insert("MASK".to_string(), 1);
                    }
                }
                crate::services::scene_shader_material_service::ScenePhase10bBindingSemantic::TimeOffset => {
                    if !resolved_pass
                        .pass
                        .combos
                        .keys()
                        .any(|name| name.eq_ignore_ascii_case("TIMEOFFSET"))
                    {
                        defines.insert("TIMEOFFSET".to_string(), 1);
                    }
                }
                _ => {}
            }
        }
    }
    defines
}

#[cfg(target_os = "macos")]
fn phase10_effect_uniform_values(
    resolved_pass: &Phase10ResolvedPass<'_>,
) -> BTreeMap<String, SceneMaterialUniformValue> {
    let mut values = BTreeMap::new();
    for (name, value) in &resolved_pass.pass.uniforms {
        values.insert(normalized_effect_uniform_name(name), value.clone());
    }
    if let Phase10PassContext::Effect(effect_pass) = resolved_pass.context {
        for (name, value) in &effect_pass.constants {
            values.insert(normalized_effect_uniform_name(name), value.clone());
        }
    }
    values
}

#[cfg(target_os = "macos")]
fn normalized_effect_uniform_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(target_os = "macos")]
fn phase10_uniform_float(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: f32,
) -> f32 {
    aliases
        .iter()
        .find_map(|alias| values.get(*alias))
        .and_then(SceneMaterialUniformValue::as_float)
        .unwrap_or(default)
}

#[cfg(target_os = "macos")]
fn phase10_uniform_vec2(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: [f32; 2],
) -> [f32; 2] {
    for alias in aliases {
        let Some(value) = values.get(*alias) else {
            continue;
        };
        if let Some(vector) = value.as_float2() {
            return vector;
        }
        if let Some(number) = value.as_float() {
            return [number, number];
        }
    }
    default
}

#[cfg(target_os = "macos")]
fn phase10_uniform_color(
    values: &BTreeMap<String, SceneMaterialUniformValue>,
    aliases: &[&str],
    default: [f32; 4],
) -> [f32; 4] {
    for alias in aliases {
        let Some(value) = values.get(*alias) else {
            continue;
        };
        if let Some(color) = value.as_float4() {
            return color;
        }
        if let Some(color) = value.as_float3() {
            return [color[0], color[1], color[2], 1.0];
        }
    }
    default
}

#[cfg(target_os = "macos")]
fn rotate2d(vector: [f32; 2], angle: f32) -> [f32; 2] {
    let sine = angle.sin();
    let cosine = angle.cos();
    [
        vector[0] * cosine - vector[1] * sine,
        vector[0] * sine + vector[1] * cosine,
    ]
}

#[cfg(target_os = "macos")]
fn phase10_alpha_prefill_required(blend_mode: SceneRenderBlendMode) -> bool {
    !matches!(blend_mode, SceneRenderBlendMode::Normal)
}

#[cfg(target_os = "macos")]
fn phase10_effect_family_from_program(
    program: &SceneShaderProgram,
) -> Option<SceneCompatEffectKind> {
    match program.kind {
        SceneShaderProgramKind::EffectCompat(kind) => Some(kind),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Phase10EffectUniforms {
    color: [f32; 4],
    user0: [f32; 4],
    user1: [f32; 4],
    primary_resolution: [f32; 4],
    slot1_resolution: [f32; 4],
    slot2_resolution: [f32; 4],
    slot3_resolution: [f32; 4],
    texel_size: [f32; 2],
    aux_texel_size: [f32; 2],
    aux2_texel_size: [f32; 2],
    aux3_texel_size: [f32; 2],
    screen_size: [f32; 2],
    time: f32,
    intensity: f32,
    speed: f32,
    radius: f32,
    angle: f32,
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct Phase10TextureHandle {
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
    metrics: Phase10TextureMetrics,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq)]
struct Phase10TextureMetrics {
    texture_size: [f32; 2],
    content_size: [f32; 2],
}

#[cfg(target_os = "macos")]
impl Default for Phase10TextureMetrics {
    fn default() -> Self {
        Self {
            texture_size: [1.0, 1.0],
            content_size: [1.0, 1.0],
        }
    }
}

#[cfg(target_os = "macos")]
impl Phase10TextureMetrics {
    fn resolution(self) -> [f32; 4] {
        [
            self.texture_size[0].max(1.0),
            self.texture_size[1].max(1.0),
            self.content_size[0].max(1.0),
            self.content_size[1].max(1.0),
        ]
    }

    fn texel_size(self) -> [f32; 2] {
        [
            1.0 / self.texture_size[0].max(1.0),
            1.0 / self.texture_size[1].max(1.0),
        ]
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct Phase10PassTextures {
    slots: Vec<Option<Phase10TextureHandle>>,
}

#[cfg(target_os = "macos")]
struct Phase10PassInputScope<'a> {
    local_current: Option<&'a Phase10TextureHandle>,
    previous_pass: Option<&'a Phase10TextureHandle>,
    background: Option<&'a Phase10TextureHandle>,
    copied_background: Option<&'a Phase10TextureHandle>,
    named_targets: &'a BTreeMap<String, Phase10TextureHandle>,
}

#[cfg(target_os = "macos")]
impl Phase10PassInputScope<'_> {
    fn texture_for(&self, source: &ScenePhase10InputSource) -> Option<Phase10TextureHandle> {
        match source {
            ScenePhase10InputSource::LocalCurrentVisual => self.local_current.cloned(),
            ScenePhase10InputSource::PreviousPass => self.previous_pass.cloned(),
            ScenePhase10InputSource::Background => self.background.cloned(),
            ScenePhase10InputSource::CopiedBackground => self.copied_background.cloned(),
            ScenePhase10InputSource::NamedTarget(target_name) => {
                self.named_targets.get(target_name).cloned()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase10BackgroundSourceKind {
    Phase10Visual,
    Visual,
    Text,
}

fn phase10_background_source_order(
    plan: &SceneRenderPlan,
    graph: &ScenePhase10GraphPlan,
) -> Vec<(SceneRenderDrawItem, Phase10BackgroundSourceKind)> {
    let phase10_visual_ids = graph
        .visuals
        .iter()
        .map(|visual| visual.object_id)
        .collect::<BTreeSet<_>>();
    let visual_ids = plan
        .visuals
        .iter()
        .map(|visual| visual.object_id)
        .collect::<BTreeSet<_>>();
    let text_ids = plan
        .texts
        .iter()
        .map(|text| text.object_id)
        .collect::<BTreeSet<_>>();

    plan.draw_order
        .iter()
        .filter_map(|draw_item| match draw_item.kind {
            SceneRenderDrawKind::Visual if phase10_visual_ids.contains(&draw_item.object_id) => {
                Some((*draw_item, Phase10BackgroundSourceKind::Phase10Visual))
            }
            SceneRenderDrawKind::Visual if visual_ids.contains(&draw_item.object_id) => {
                Some((*draw_item, Phase10BackgroundSourceKind::Visual))
            }
            SceneRenderDrawKind::Text if text_ids.contains(&draw_item.object_id) => {
                Some((*draw_item, Phase10BackgroundSourceKind::Text))
            }
            SceneRenderDrawKind::Audio
            | SceneRenderDrawKind::Particle
            | SceneRenderDrawKind::RopeParticle
            | SceneRenderDrawKind::SpriteParticle
            | SceneRenderDrawKind::Sound
            | SceneRenderDrawKind::Visual
            | SceneRenderDrawKind::Text => None,
        })
        .collect()
}

#[cfg(target_os = "macos")]
struct Phase10BackgroundLayer {
    quad: SceneRenderQuad,
    blend_mode: SceneRenderBlendMode,
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
enum Phase10PassContext<'a> {
    Base,
    Effect(&'a ScenePhase10EffectPassNode),
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct Phase10ResolvedPass<'a> {
    pass: &'a SceneMaterialPassPlan,
    context: Phase10PassContext<'a>,
}

#[cfg(target_os = "macos")]
fn phase10_visual_pass_chain<'a>(
    visual: &'a ScenePhase10VisualPlan,
) -> Vec<Phase10ResolvedPass<'a>> {
    let mut chain = visual
        .material
        .passes
        .iter()
        .map(|pass| Phase10ResolvedPass {
            pass,
            context: Phase10PassContext::Base,
        })
        .collect::<Vec<_>>();
    for effect in &visual.effect_chain {
        for effect_pass in &effect.passes {
            chain.extend(
                effect_pass
                    .material_passes
                    .iter()
                    .map(|pass| Phase10ResolvedPass {
                        pass,
                        context: Phase10PassContext::Effect(effect_pass),
                    }),
            );
        }
    }
    chain
}

#[cfg(target_os = "macos")]
fn phase10_texel_size(texture: Option<&Phase10TextureHandle>) -> [f32; 2] {
    let Some(texture) = texture else {
        return [1.0, 1.0];
    };
    texture.metrics.texel_size()
}

#[cfg(target_os = "macos")]
fn phase10_optional_texel_size(texture: Option<&Phase10TextureHandle>) -> [f32; 2] {
    let Some(texture) = texture else {
        return [0.0, 0.0];
    };
    texture.metrics.texel_size()
}

#[cfg(target_os = "macos")]
fn phase10_texture_resolution(texture: Option<&Phase10TextureHandle>) -> [f32; 4] {
    let Some(texture) = texture else {
        return [1.0, 1.0, 1.0, 1.0];
    };
    texture.metrics.resolution()
}

#[cfg(target_os = "macos")]
fn phase10_optional_texture_resolution(texture: Option<&Phase10TextureHandle>) -> [f32; 4] {
    let Some(texture) = texture else {
        return [1.0, 1.0, 0.0, 0.0];
    };
    texture.metrics.resolution()
}

#[cfg(target_os = "macos")]
fn phase10_texture_metrics_from_size(width: usize, height: usize) -> Phase10TextureMetrics {
    Phase10TextureMetrics {
        texture_size: [width.max(1) as f32, height.max(1) as f32],
        content_size: [width.max(1) as f32, height.max(1) as f32],
    }
}

#[cfg(target_os = "macos")]
fn phase10_texture_metrics_from_texture(
    texture: &ProtocolObject<dyn MTLTexture>,
) -> Phase10TextureMetrics {
    phase10_texture_metrics_from_size(texture.width(), texture.height())
}

#[cfg(target_os = "macos")]
fn ensure_phase10_render_target_in_store(
    device: &ProtocolObject<dyn MTLDevice>,
    store: &mut BTreeMap<String, Retained<ProtocolObject<dyn MTLTexture>>>,
    key: &str,
    width: usize,
    height: usize,
) -> Option<Phase10TextureHandle> {
    if let Some(texture) = store.get(key) {
        return Some(Phase10TextureHandle {
            texture: texture.clone(),
            metrics: phase10_texture_metrics_from_size(width, height),
        });
    }
    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm,
            width.max(1),
            height.max(1),
            false,
        )
    };
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
    descriptor.setStorageMode(MTLStorageMode::Private);
    let texture = device.newTextureWithDescriptor(&descriptor)?;
    store.insert(key.to_string(), texture.clone());
    Some(Phase10TextureHandle {
        texture,
        metrics: phase10_texture_metrics_from_size(width, height),
    })
}

#[cfg(target_os = "macos")]
fn build_solid_texture_image(color: SceneRenderColor, width: usize, height: usize) -> DynamicImage {
    let pixel = image::Rgba([color.red, color.green, color.blue, color.alpha]);
    DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width.max(1) as u32,
        height.max(1) as u32,
        pixel,
    ))
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneVertexUploadStrategy {
    InlineBytes,
    SharedBuffer,
}

#[cfg(target_os = "macos")]
fn scene_vertex_upload_strategy(byte_len: usize) -> SceneVertexUploadStrategy {
    if byte_len > INLINE_VERTEX_BYTES_LIMIT {
        SceneVertexUploadStrategy::SharedBuffer
    } else {
        SceneVertexUploadStrategy::InlineBytes
    }
}

#[cfg(target_os = "macos")]
fn text_texture_cache_key(item: &SceneRenderTextItem) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    item.object_id.hash(&mut hasher);
    item.behavior.hash(&mut hasher);
    item.text.hash(&mut hasher);
    item.font.cache_key.hash(&mut hasher);
    item.font.file_candidates.hash(&mut hasher);
    item.font.family_candidates.hash(&mut hasher);
    item.point_size.to_bits().hash(&mut hasher);
    item.color.red.hash(&mut hasher);
    item.color.green.hash(&mut hasher);
    item.color.blue.hash(&mut hasher);
    item.color.alpha.hash(&mut hasher);
    item.horizontal_align.hash(&mut hasher);
    item.vertical_align.hash(&mut hasher);
    item.blur_enabled.hash(&mut hasher);
    item.blur_radius.to_bits().hash(&mut hasher);
    item.max_rows.hash(&mut hasher);
    item.limit_width.hash(&mut hasher);
    item.limit_use_ellipsis.hash(&mut hasher);
    item.dynamic_input_generation.hash(&mut hasher);
    item.quad.width.to_bits().hash(&mut hasher);
    item.quad.height.to_bits().hash(&mut hasher);
    item.content_left.to_bits().hash(&mut hasher);
    item.content_top.to_bits().hash(&mut hasher);
    item.content_width.to_bits().hash(&mut hasher);
    item.content_height.to_bits().hash(&mut hasher);
    item.effect_paths.hash(&mut hasher);
    format!("text:{}:{:x}", item.object_id, hasher.finish())
}

#[cfg(target_os = "macos")]
fn sprite_particle_texture_paths(item: &SceneRenderSpriteParticleItem) -> Vec<PathBuf> {
    let mut paths = vec![item.config.texture_path.clone()];
    for child in &item.children {
        if !paths.contains(&child.config.texture_path) {
            paths.push(child.config.texture_path.clone());
        }
    }
    paths
}

#[cfg(target_os = "macos")]
fn particle_plan_signature(plan: &SceneRenderPlan) -> Option<u64> {
    if plan.particles.is_empty() && plan.rope_particles.is_empty() && plan.sprite_particles.is_empty() {
        return None;
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for item in &plan.particles {
        item.object_id.hash(&mut hasher);
        match item.particle_kind {
            crate::models::SceneParticleKind::LineTrail => 0_u8.hash(&mut hasher),
            crate::models::SceneParticleKind::PetalTrail => 1_u8.hash(&mut hasher),
        }
        item.color.red.hash(&mut hasher);
        item.color.green.hash(&mut hasher);
        item.color.blue.hash(&mut hasher);
        item.color.alpha.hash(&mut hasher);
        item.size.to_bits().hash(&mut hasher);
        item.emission_rate.to_bits().hash(&mut hasher);
        item.schedule_mode.hash(&mut hasher);
        item.spawn_origin[0].to_bits().hash(&mut hasher);
        item.spawn_origin[1].to_bits().hash(&mut hasher);
        item.max_count.hash(&mut hasher);
        item.lifetime_ms.to_bits().hash(&mut hasher);
        item.speed_range[0].to_bits().hash(&mut hasher);
        item.speed_range[1].to_bits().hash(&mut hasher);
        item.instantaneous.hash(&mut hasher);
        item.start_time_ms.to_bits().hash(&mut hasher);
        item.sign.to_bits().hash(&mut hasher);
        item.spawn_radius[0].to_bits().hash(&mut hasher);
        item.spawn_radius[1].to_bits().hash(&mut hasher);
        item.uv_scrolling[0].to_bits().hash(&mut hasher);
        item.uv_scrolling[1].to_bits().hash(&mut hasher);
        item.fade_alpha.to_bits().hash(&mut hasher);
        item.subdivision.hash(&mut hasher);
        item.rope_length.to_bits().hash(&mut hasher);
    }
    for item in &plan.sprite_particles {
        2_u8.hash(&mut hasher);
        hash_sprite_particle_item(&mut hasher, item);
    }
    for item in &plan.rope_particles {
        3_u8.hash(&mut hasher);
        item.object_id.hash(&mut hasher);
        item.object_name.hash(&mut hasher);
        item.renderer_family.hash(&mut hasher);
        item.schedule_mode.hash(&mut hasher);
        item.segment_count.hash(&mut hasher);
        item.subdivision.hash(&mut hasher);
        item.length.to_bits().hash(&mut hasher);
        item.min_length.to_bits().hash(&mut hasher);
        item.max_length.to_bits().hash(&mut hasher);
        item.width.to_bits().hash(&mut hasher);
        item.lifetime_ms.to_bits().hash(&mut hasher);
        item.color.red.hash(&mut hasher);
        item.color.green.hash(&mut hasher);
        item.color.blue.hash(&mut hasher);
        item.color.alpha.hash(&mut hasher);
        item.uv_scrolling[0].to_bits().hash(&mut hasher);
        item.uv_scrolling[1].to_bits().hash(&mut hasher);
        item.fade_alpha.to_bits().hash(&mut hasher);
        item.material_path.hash(&mut hasher);
        item.control_points.len().hash(&mut hasher);
        for control_point in &item.control_points {
            control_point.id.hash(&mut hasher);
            control_point.position[0].to_bits().hash(&mut hasher);
            control_point.position[1].to_bits().hash(&mut hasher);
            control_point.lock_to_pointer.hash(&mut hasher);
        }
    }
    Some(hasher.finish())
}

#[cfg(target_os = "macos")]
fn hash_sprite_particle_item(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    item: &SceneRenderSpriteParticleItem,
) {
    item.object_id.hash(hasher);
    item.object_name.hash(hasher);
    hash_sprite_particle_config(hasher, &item.config);
    item.schedule_mode.hash(hasher);
    item.children.len().hash(hasher);
    for child in &item.children {
        child.child_type.hash(hasher);
        child.probability.to_bits().hash(hasher);
        child.control_point_start_index.hash(hasher);
        hash_sprite_particle_config(hasher, &child.config);
    }
}

#[cfg(target_os = "macos")]
fn hash_sprite_particle_config(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    config: &crate::services::scene_render_planner_service::SceneSpriteParticleConfig,
) {
    config.texture_path.hash(hasher);
    config.blend_mode.hash(hasher);
    for value in config
        .spawn_origin
        .iter()
        .chain(config.spawn_radius.iter())
        .chain(config.size_range.iter())
        .chain(config.lifetime_ms_range.iter())
        .chain(config.speed_range.iter())
        .chain(config.rotation_range.iter())
        .chain(config.angular_velocity_range.iter())
        .chain(config.alpha_range.iter())
    {
        value.to_bits().hash(hasher);
    }
    for direction in &config.directions {
        direction[0].to_bits().hash(hasher);
        direction[1].to_bits().hash(hasher);
    }
    config.sign.to_bits().hash(hasher);
    config.orientation.to_bits().hash(hasher);
    match config.velocity_range {
        Some(range) => {
            true.hash(hasher);
            for vector in range {
                vector[0].to_bits().hash(hasher);
                vector[1].to_bits().hash(hasher);
            }
        }
        None => false.hash(hasher),
    }
    config.color_min.hash(hasher);
    config.color_max.hash(hasher);
    config.texture_frames.len().hash(hasher);
    for frame in &config.texture_frames {
        for value in frame.uv_rect {
            value.to_bits().hash(hasher);
        }
        frame.aspect_ratio.to_bits().hash(hasher);
    }
    config.turbulence.to_bits().hash(hasher);
    if let Some(size_change) = config.size_change {
        size_change[0].to_bits().hash(hasher);
        size_change[1].to_bits().hash(hasher);
    }
    hash_optional_oscillation(hasher, &config.position_oscillation);
    hash_optional_oscillation(hasher, &config.alpha_oscillation);
    hash_optional_oscillation(hasher, &config.size_oscillation);
    config.fade_in_ms.to_bits().hash(hasher);
    config.fade_out_ms.to_bits().hash(hasher);
    config.emission_rate.to_bits().hash(hasher);
    config.max_count.hash(hasher);
    config.start_time_ms.to_bits().hash(hasher);
    config.instantaneous.hash(hasher);
    config.sequence_multiplier.to_bits().hash(hasher);
}

#[cfg(target_os = "macos")]
fn hash_optional_oscillation(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    oscillation: &Option<
        crate::services::scene_render_planner_service::SceneSpriteParticleOscillationConfig,
    >,
) {
    if let Some(oscillation) = oscillation {
        true.hash(hasher);
        for value in oscillation
            .amplitude_range
            .iter()
            .chain(oscillation.frequency_range.iter())
            .chain(oscillation.phase_range.iter())
            .chain(oscillation.axis_scale.iter())
        {
            value.to_bits().hash(hasher);
        }
    } else {
        false.hash(hasher);
    }
}

#[cfg(target_os = "macos")]
fn build_pipeline_state(
    device: &ProtocolObject<dyn MTLDevice>,
    vertex_function: &ProtocolObject<dyn objc2_metal::MTLFunction>,
    fragment_function: &ProtocolObject<dyn objc2_metal::MTLFunction>,
    blend_mode: SceneRenderBlendMode,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, String> {
    let pipeline_descriptor = MTLRenderPipelineDescriptor::new();
    pipeline_descriptor.setVertexFunction(Some(vertex_function));
    pipeline_descriptor.setFragmentFunction(Some(fragment_function));

    unsafe {
        let attachment = pipeline_descriptor
            .colorAttachments()
            .objectAtIndexedSubscript(0);
        attachment.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        configure_blend_attachment(&attachment, blend_mode);
    }

    device
        .newRenderPipelineStateWithDescriptor_error(&pipeline_descriptor)
        .map_err(|error| format!("failed to create Scene Metal pipeline state: {error:?}"))
}

#[cfg(target_os = "macos")]
fn compile_scene_shader_library(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLLibrary>>, String> {
    device
        .newLibraryWithSource_options_error(ns_string!(SCENE_SHADER_SOURCE), None)
        .map_err(|error| format!("failed to compile Scene Metal shader source: {error:?}"))
}

#[cfg(target_os = "macos")]
fn build_scene_pipeline_states(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<NativeScenePipelineStates, String> {
    let library = compile_scene_shader_library(device)?;
    let vertex_function = library
        .newFunctionWithName(ns_string!("scene_vertex"))
        .ok_or_else(|| "Scene Metal vertex function was not found".to_string())?;
    let fragment_function = library
        .newFunctionWithName(ns_string!("scene_fragment"))
        .ok_or_else(|| "Scene Metal fragment function was not found".to_string())?;

    Ok(NativeScenePipelineStates {
        normal: build_pipeline_state(
            device,
            &vertex_function,
            &fragment_function,
            SceneRenderBlendMode::Normal,
        )?,
        additive: build_pipeline_state(
            device,
            &vertex_function,
            &fragment_function,
            SceneRenderBlendMode::Additive,
        )?,
        multiply: build_pipeline_state(
            device,
            &vertex_function,
            &fragment_function,
            SceneRenderBlendMode::Multiply,
        )?,
    })
}

#[cfg(target_os = "macos")]
fn configure_blend_attachment(
    attachment: &MTLRenderPipelineColorAttachmentDescriptor,
    blend_mode: SceneRenderBlendMode,
) {
    attachment.setBlendingEnabled(true);
    attachment.setRgbBlendOperation(MTLBlendOperation::Add);
    attachment.setAlphaBlendOperation(MTLBlendOperation::Add);

    match blend_mode {
        SceneRenderBlendMode::Normal => {
            attachment.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
            attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        }
        SceneRenderBlendMode::Additive => {
            attachment.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
            attachment.setDestinationRGBBlendFactor(MTLBlendFactor::One);
            attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::One);
        }
        SceneRenderBlendMode::Multiply => {
            attachment.setSourceRGBBlendFactor(MTLBlendFactor::DestinationColor);
            attachment.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            attachment.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        }
    }
}

#[cfg(target_os = "macos")]
fn create_scene_video_texture_cache(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<CFRetained<CVMetalTextureCache>, String> {
    let mut cache_ptr = std::ptr::null_mut();
    let status = unsafe {
        CVMetalTextureCache::create(None, None, device, None, NonNull::from(&mut cache_ptr))
    };
    if status != kCVReturnSuccess {
        return Err(format!(
            "CVMetalTextureCacheCreate failed for Scene video runtime with status {status}"
        ));
    }
    let cache_ptr = NonNull::new(cache_ptr)
        .ok_or_else(|| "CVMetalTextureCacheCreate returned a null cache".to_string())?;
    Ok(unsafe { CFRetained::from_raw(cache_ptr) })
}

#[cfg(target_os = "macos")]
impl NativeSceneVideoSource {
    fn new(object_id: u32, asset_path: PathBuf, paused: bool) -> Result<Self, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Scene video source must be created on the main thread".to_string())?;
        let url = file_url_for_path(&asset_path)?;
        let item = unsafe { AVPlayerItem::playerItemWithURL(&url, mtm) };
        let output_settings = scene_video_output_settings();
        let output = unsafe {
            AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&output_settings),
            )
        };
        unsafe {
            item.addOutput(output.as_ref());
        }

        let player = unsafe { AVPlayer::playerWithPlayerItem(Some(item.as_ref()), mtm) };
        unsafe {
            player.setMuted(true);
            player.setVolume(0.0);
            player.setActionAtItemEnd(AVPlayerActionAtItemEnd::None);
            if paused {
                player.pause();
            } else {
                player.play();
            }
        }

        Ok(Self {
            object_id,
            asset_path,
            paused,
            player,
            item,
            output,
            current_cv_texture: None,
            current_texture: None,
        })
    }

    fn stop(&mut self) {
        unsafe {
            self.player.pause();
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
        self.current_texture = None;
        self.current_cv_texture = None;
    }

    fn state(&self) -> SceneVideoTextureSourceState {
        SceneVideoTextureSourceState {
            asset_path: self.asset_path.clone(),
            paused: self.paused,
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        unsafe {
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    fn current_texture(
        &mut self,
        texture_cache: &CVMetalTextureCache,
        paused: bool,
    ) -> Result<Option<Retained<ProtocolObject<dyn MTLTexture>>>, String> {
        self.set_paused(paused);
        self.ensure_looping(paused);

        match unsafe { self.item.status() } {
            AVPlayerItemStatus::Failed => {
                return Err(format!(
                    "AVPlayerItem failed for object {} at {}: {:?}",
                    self.object_id,
                    self.asset_path.display(),
                    unsafe { self.item.error() }
                ));
            }
            AVPlayerItemStatus::Unknown => {
                return Ok(self.current_texture.clone());
            }
            AVPlayerItemStatus::ReadyToPlay => {}
            _ => {}
        }

        let mut item_time = unsafe { self.output.itemTimeForHostTime(CACurrentMediaTime()) };
        if !cm_time_is_numeric(item_time) {
            item_time = unsafe { self.player.currentTime() };
        }

        let needs_frame = self.current_texture.is_none()
            || unsafe { self.output.hasNewPixelBufferForItemTime(item_time) };
        if needs_frame {
            let pixel_buffer = unsafe {
                self.output
                    .copyPixelBufferForItemTime_itemTimeForDisplay(item_time, std::ptr::null_mut())
            };
            if let Some(pixel_buffer) = pixel_buffer.as_ref() {
                self.update_current_texture(texture_cache, pixel_buffer)?;
            }
        }

        Ok(self.current_texture.clone())
    }

    fn ensure_looping(&self, paused: bool) {
        let duration_seconds = unsafe { self.item.duration().seconds() };
        let current_seconds = unsafe { self.player.currentTime().seconds() };
        if !duration_seconds.is_finite()
            || !current_seconds.is_finite()
            || duration_seconds <= 0.05
            || current_seconds < duration_seconds - 0.03
        {
            return;
        }

        let zero = unsafe { CMTime::with_seconds(0.0, 600) };
        unsafe {
            self.player.seekToTime(zero);
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    fn update_current_texture(
        &mut self,
        texture_cache: &CVMetalTextureCache,
        pixel_buffer: &CVPixelBuffer,
    ) -> Result<(), String> {
        let width = CVPixelBufferGetWidth(pixel_buffer);
        let height = CVPixelBufferGetHeight(pixel_buffer);
        if width == 0 || height == 0 {
            return Err(format!(
                "Scene video source {} produced an empty pixel buffer",
                self.asset_path.display()
            ));
        }

        let mut cv_texture_ptr = std::ptr::null_mut();
        let status = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                texture_cache,
                pixel_buffer,
                None,
                MTLPixelFormat::BGRA8Unorm,
                width,
                height,
                0,
                NonNull::from(&mut cv_texture_ptr),
            )
        };
        if status != kCVReturnSuccess {
            return Err(format!(
                "CVMetalTextureCacheCreateTextureFromImage failed for {} with status {status}",
                self.asset_path.display()
            ));
        }
        let cv_texture_ptr = NonNull::new(cv_texture_ptr).ok_or_else(|| {
            format!(
                "CVMetalTextureCacheCreateTextureFromImage returned null for {}",
                self.asset_path.display()
            )
        })?;
        let cv_texture = unsafe { CFRetained::from_raw(cv_texture_ptr) };
        let texture = CVMetalTextureGetTexture(cv_texture.as_ref()).ok_or_else(|| {
            format!(
                "CVMetalTextureGetTexture returned null for {}",
                self.asset_path.display()
            )
        })?;
        self.current_cv_texture = Some(cv_texture);
        self.current_texture = Some(texture);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn scene_video_output_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    let pixel_format = NSNumber::numberWithUnsignedInt(kCVPixelFormatType_32BGRA);
    let metal_compatible = NSNumber::numberWithBool(true);
    unsafe {
        NSDictionary::from_slices(
            &[
                cfstring_key_as_nsstring(kCVPixelBufferPixelFormatTypeKey),
                cfstring_key_as_nsstring(kCVPixelBufferMetalCompatibilityKey),
            ],
            &[pixel_format.as_ref(), metal_compatible.as_ref()],
        )
    }
}

#[cfg(target_os = "macos")]
fn cfstring_key_as_nsstring(value: &'static CFString) -> &'static NSString {
    unsafe { &*(value as *const CFString as *const NSString) }
}

#[cfg(target_os = "macos")]
fn file_url_for_path(path: &Path) -> Result<Retained<NSURL>, String> {
    let Some(path_string) = path.to_str() else {
        return Err(format!(
            "path {} is not valid UTF-8 for AVFoundation",
            path.display()
        ));
    };
    let path_string = NSString::from_str(path_string);
    Ok(NSURL::fileURLWithPath(&path_string))
}

#[cfg(target_os = "macos")]
#[cfg_attr(not(test), allow(dead_code))]
fn load_phase10_texture_image(path: &Path) -> Result<DynamicImage, String> {
    load_phase10_texture_source(path).map(|decoded| decoded.image)
}

#[cfg(target_os = "macos")]
fn resolve_phase10_texture_source_path(path: &Path) -> PathBuf {
    phase10_texture_path_candidates(path)
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(target_os = "macos")]
fn phase10_texture_path_candidates(path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return vec![path.to_path_buf()];
    };
    let lower_name = file_name.to_ascii_lowercase();
    let Some(parent) = path.parent() else {
        return vec![path.to_path_buf()];
    };

    let mut push_base_candidates = |stem: &str| {
        candidates.push(parent.join(format!("{stem}.png")));
        candidates.push(parent.join(format!("{stem}.tex")));
    };

    if lower_name.ends_with(".tex-json") {
        let stem = &file_name[..file_name.len() - ".tex-json".len()];
        push_base_candidates(stem);
    } else if lower_name.ends_with(".tex.json") {
        let stem = &file_name[..file_name.len() - ".tex.json".len()];
        push_base_candidates(stem);
    }

    candidates.push(path.to_path_buf());
    candidates
}

#[cfg(target_os = "macos")]
struct Phase10DecodedTexture {
    image: DynamicImage,
    metrics: Phase10TextureMetrics,
}

#[cfg(target_os = "macos")]
fn load_phase10_texture_source(path: &Path) -> Result<Phase10DecodedTexture, String> {
    let resolved_path = resolve_phase10_texture_source_path(path);
    let loader_path = resolved_path.as_path();
    let extension = loader_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());

    if extension.as_deref() == Some("tex") {
        let image = crate::tex::load_tex_image(loader_path).map_err(|error| {
            format!(
                "unable to decode phase-10 texture {}: {error}",
                loader_path.display()
            )
        })?;
        let metrics =
            phase10_texture_metrics_from_size(image.width() as usize, image.height() as usize);
        return Ok(Phase10DecodedTexture { image, metrics });
    }

    let image = image::open(loader_path).map_err(|error| {
        format!(
            "unable to decode phase-10 texture {}: {error}",
            loader_path.display()
        )
    })?;
    let metrics =
        phase10_texture_metrics_from_size(image.width() as usize, image.height() as usize);
    Ok(Phase10DecodedTexture { image, metrics })
}

#[cfg(target_os = "macos")]
fn cm_time_is_numeric(time: CMTime) -> bool {
    unsafe { time.seconds().is_finite() }
}

#[cfg(target_os = "macos")]
fn load_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    image: DynamicImage,
) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, String> {
    let rgba = image.to_rgba8();
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    if width == 0 || height == 0 {
        return Err("decoded image is empty".to_string());
    }

    let descriptor = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::RGBA8Unorm,
            width,
            height,
            false,
        )
    };
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setUsage(MTLTextureUsage::ShaderRead);
    descriptor.setStorageMode(MTLStorageMode::Shared);

    let Some(texture) = device.newTextureWithDescriptor(&descriptor) else {
        return Err("Metal device returned no texture for the descriptor".to_string());
    };

    let region = MTLRegion {
        origin: objc2_metal::MTLOrigin { x: 0, y: 0, z: 0 },
        size: objc2_metal::MTLSize {
            width,
            height,
            depth: 1,
        },
    };
    let bytes = NonNull::new(rgba.as_raw().as_ptr() as *mut c_void)
        .ok_or_else(|| "decoded image bytes were unexpectedly null".to_string())?;
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(region, 0, bytes, width * 4);
    }

    Ok(texture)
}

#[cfg(target_os = "macos")]
fn create_sound_player(
    sound: &SceneRenderSoundItem,
    paused: bool,
    output_volume: f64,
) -> Result<NativeSceneSoundPlayer, String> {
    let Some(path_string) = sound.asset_path.to_str() else {
        return Err(format!(
            "sound path {} is not valid UTF-8 for AVFoundation",
            sound.asset_path.display()
        ));
    };
    let path_string = NSString::from_str(path_string);
    let url = NSURL::fileURLWithPath(&path_string);
    let player =
        unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
            .map_err(|error| format!("{error:?}"))?;
    unsafe {
        player.setVolume(effective_output_volume(sound.volume, output_volume) as f32);
        player.setNumberOfLoops(if sound.looped { -1 } else { 0 });
        player.setMeteringEnabled(true);
        let _ = player.prepareToPlay();
    }
    let mut sound_player = NativeSceneSoundPlayer {
        state: SceneSoundRuntimeState::from_sound_item(sound, paused),
        player,
    };
    set_sound_player_paused(&mut sound_player, paused);
    Ok(sound_player)
}

#[cfg(target_os = "macos")]
fn configure_sound_player(
    player: &mut NativeSceneSoundPlayer,
    state: &SceneSoundRuntimeState,
    output_volume: f64,
) {
    unsafe {
        player
            .player
            .setVolume(effective_output_volume(state.volume, output_volume) as f32);
        player
            .player
            .setNumberOfLoops(if state.looped { -1 } else { 0 });
    }
    player.state.looped = state.looped;
    player.state.volume = state.volume;
}

#[cfg(target_os = "macos")]
fn set_sound_player_paused(player: &mut NativeSceneSoundPlayer, paused: bool) {
    unsafe {
        if paused {
            player.player.pause();
        } else if !player.player.isPlaying() {
            let _ = player.player.play();
        }
    }
    player.state.paused = paused;
}

#[cfg(target_os = "macos")]
fn stop_sound_player(player: &NativeSceneSoundPlayer) {
    unsafe {
        player.player.stop();
    }
}

#[cfg(target_os = "macos")]
fn build_white_texture_image() -> DynamicImage {
    DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([255, 255, 255, 255]),
    ))
}

#[cfg(target_os = "macos")]
fn build_petal_texture_image() -> DynamicImage {
    let width = 64;
    let height = 44;
    let mut image = image::RgbaImage::new(width, height);
    let cx = width as f64 * 0.38;
    let cy = height as f64 * 0.42;
    let rx = width as f64 * 0.48;
    let ry = height as f64 * 0.42;
    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64 - cx) / rx;
            let dy = (y as f64 - cy) / ry;
            let distance = dx * dx + dy * dy;
            let alpha = if distance >= 1.0 {
                0.0
            } else {
                (1.0 - distance).powf(1.8)
            };
            let highlight = ((1.0
                - (((x as f64 - width as f64 * 0.22) / (width as f64 * 0.2)).powi(2)
                    + ((y as f64 - height as f64 * 0.24) / (height as f64 * 0.18)).powi(2)))
            .max(0.0))
            .powf(2.5);
            image.put_pixel(
                x,
                y,
                image::Rgba([
                    (255.0 * (0.84 + 0.16 * highlight)) as u8,
                    (255.0 * (0.84 + 0.16 * highlight)) as u8,
                    (255.0 * (0.84 + 0.16 * highlight)) as u8,
                    (255.0 * alpha) as u8,
                ]),
            );
        }
    }
    DynamicImage::ImageRgba8(image)
}

#[cfg(target_os = "macos")]
struct SceneTextRasterizedTexture {
    image: DynamicImage,
    warnings: Vec<NativeSceneWarning>,
}

#[cfg(target_os = "macos")]
struct SceneResolvedTextFont {
    font: Retained<NSFont>,
    fallback_detail: Option<SceneTextFontFallbackDetail>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneTextFontFallbackDetail {
    reason: String,
    attempted_files: Vec<String>,
    family_candidates: Vec<String>,
}

#[cfg(target_os = "macos")]
fn rasterize_text_texture(
    item: &SceneRenderTextItem,
) -> Result<SceneTextRasterizedTexture, String> {
    let width = item.quad.width.max(1.0).ceil() as usize;
    let height = item.quad.height.max(1.0).ceil() as usize;
    let bytes_per_row = width
        .checked_mul(4)
        .ok_or_else(|| "text texture row size overflowed".to_string())?;
    let total_bytes = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| "text texture buffer size overflowed".to_string())?;
    let mut pixels = vec![0_u8; total_bytes];
    let color_space = CGColorSpace::new_device_rgb()
        .ok_or_else(|| "Core Graphics RGB color space is unavailable".to_string())?;
    let bitmap_info = CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast::<c_void>(),
            width,
            height,
            8,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
        )
    }
    .ok_or_else(|| "unable to create Core Graphics bitmap context for text".to_string())?;

    let graphics_context = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, true);
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&graphics_context));
    graphics_context.setImageInterpolation(NSImageInterpolation::High);

    let color = nscolor_from_scene_color(item.color);
    let paragraph_style = paragraph_style_for_text(item);
    let string = NSString::from_str(item.text.as_str());
    let options = text_drawing_options(item);
    let mut warnings = item
        .effect_paths
        .iter()
        .filter(|path| !path.to_ascii_lowercase().contains("blur"))
        .map(|path| NativeSceneWarning::unsupported_text_effect(&item.object_name, path))
        .collect::<Vec<_>>();
    let resolved_point_size =
        resolve_scene_text_point_size(item, &string, &color, &paragraph_style, options)?;
    let resolved_font = scene_text_font_with_point_size(item, resolved_point_size)?;
    if let Some(detail) = resolved_font.fallback_detail.clone() {
        warnings.push(NativeSceneWarning::text_font_fallback(item, detail));
    }
    let attributes = build_text_attributes(&resolved_font.font, &color, &paragraph_style);
    let rect = CGRect::new(
        CGPoint::new(item.content_left, item.content_top),
        CGSize::new(item.content_width.max(1.0), item.content_height.max(1.0)),
    );
    unsafe {
        string.drawWithRect_options_attributes_context(
            rect,
            options,
            Some(&attributes),
            Some(&NSStringDrawingContext::new()),
        );
    }
    graphics_context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let mut image = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
        .ok_or_else(|| "text pixels could not be rewrapped as RGBA".to_string())?;
    image::imageops::flip_vertical_in_place(&mut image);
    unpremultiply_rgba_pixels(&mut image);
    if item.blur_enabled {
        let blur_source = image.clone();
        let mut blurred = image::imageops::blur(
            &DynamicImage::ImageRgba8(blur_source),
            item.blur_radius as f32,
        );
        for pixel in blurred.pixels_mut() {
            pixel.0[3] = ((pixel.0[3] as f32) * 0.34) as u8;
        }
        image::imageops::overlay(&mut blurred, &image, 0, 0);
        image = blurred;
    }
    Ok(SceneTextRasterizedTexture {
        image: DynamicImage::ImageRgba8(image),
        warnings,
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn rasterize_scene_text_item_snapshot(
    item: &SceneRenderTextItem,
) -> Result<DynamicImage, String> {
    rasterize_text_texture(item).map(|rasterized| rasterized.image)
}

#[cfg(target_os = "macos")]
fn unpremultiply_rgba_pixels(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = pixel.0[3];
        if alpha == 0 || alpha == 255 {
            continue;
        }
        let alpha_scale = 255.0 / alpha as f32;
        pixel.0[0] = ((pixel.0[0] as f32 * alpha_scale).round()).clamp(0.0, 255.0) as u8;
        pixel.0[1] = ((pixel.0[1] as f32 * alpha_scale).round()).clamp(0.0, 255.0) as u8;
        pixel.0[2] = ((pixel.0[2] as f32 * alpha_scale).round()).clamp(0.0, 255.0) as u8;
    }
}

#[cfg(target_os = "macos")]
fn resolve_scene_text_point_size(
    item: &SceneRenderTextItem,
    text: &NSString,
    color: &NSColor,
    paragraph_style: &NSMutableParagraphStyle,
    options: NSStringDrawingOptions,
) -> Result<f64, String> {
    let authored_fit_behavior = matches!(
        item.behavior,
        crate::models::SceneTextBehavior::Static | crate::models::SceneTextBehavior::Clock
    );
    let needs_native_metric_fit = authored_fit_behavior
        || item.text.contains('\n')
        || item.limit_width
        || item.limit_use_ellipsis
        || item.max_rows.unwrap_or_default() > 1;
    if !needs_native_metric_fit {
        return Ok(item.point_size.max(1.0));
    }

    let mut point_size = item.point_size.max(1.0);
    for _ in 0..3 {
        let font = scene_text_font_with_point_size(item, point_size)?.font;
        let attributes = build_text_attributes(&font, color, paragraph_style);
        let measured = measure_scene_text_bounds(text, &attributes, item, options);
        let scale = if authored_fit_behavior {
            scene_text_fit_scale(item, measured.size.width, measured.size.height)
        } else {
            scene_text_height_fit_scale(item, measured.size.height).min(1.0)
        };
        let next_point_size = (point_size * scale).clamp(1.0, 2048.0);
        if (next_point_size - point_size).abs() < 0.5 {
            point_size = next_point_size;
            break;
        }
        point_size = next_point_size;
    }
    Ok(point_size)
}

#[cfg(target_os = "macos")]
fn measure_scene_text_bounds(
    text: &NSString,
    attributes: &NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>,
    item: &SceneRenderTextItem,
    options: NSStringDrawingOptions,
) -> CGRect {
    let constrain_width =
        item.limit_width || item.max_rows.unwrap_or_default() > 1 || item.text.contains('\n');
    let measure_width = if constrain_width {
        item.content_width.max(1.0)
    } else {
        100_000.0
    };
    let measure_height = 100_000.0;
    unsafe {
        text.boundingRectWithSize_options_attributes_context(
            CGSize::new(measure_width, measure_height),
            options,
            Some(attributes),
            Some(&NSStringDrawingContext::new()),
        )
    }
}

#[cfg(target_os = "macos")]
fn scene_text_height_fit_scale(item: &SceneRenderTextItem, measured_height: f64) -> f64 {
    let container_height = item.content_height.max(1.0);
    (container_height / measured_height.max(1.0)).clamp(0.05, 16.0)
}

#[cfg(target_os = "macos")]
fn scene_text_fit_scale(
    item: &SceneRenderTextItem,
    measured_width: f64,
    measured_height: f64,
) -> f64 {
    let container_width = item.content_width.max(1.0);
    let container_height = item.content_height.max(1.0);
    let width_scale = container_width / measured_width.max(1.0);
    let height_scale = container_height / measured_height.max(1.0);
    let prefers_width = item.behavior == crate::models::SceneTextBehavior::Static;
    let mut scale = if prefers_width {
        width_scale
    } else {
        height_scale
    };
    if measured_width * scale > container_width || item.limit_width || item.limit_use_ellipsis {
        scale = scale.min(width_scale);
    }
    if measured_height * scale > container_height {
        scale = scale.min(height_scale);
    }
    scale.clamp(0.05, 16.0)
}

#[cfg(target_os = "macos")]
fn scene_text_font_with_point_size(
    item: &SceneRenderTextItem,
    point_size: f64,
) -> Result<SceneResolvedTextFont, String> {
    let cache_key = scene_text_font_cache_key(item, point_size);
    if let Some(cached_font) =
        SCENE_TEXT_FONT_CACHE.with(|cache| cache.borrow().get(&cache_key).cloned())
    {
        return Ok(SceneResolvedTextFont {
            font: cached_font.font,
            fallback_detail: cached_font.fallback_detail,
        });
    }

    let font = scene_text_font_uncached(item, point_size)?;
    SCENE_TEXT_FONT_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            cache_key,
            SceneCachedTextFont {
                font: font.font.clone(),
                fallback_detail: font.fallback_detail.clone(),
            },
        );
    });
    Ok(font)
}

#[cfg(target_os = "macos")]
fn scene_text_font_uncached(
    item: &SceneRenderTextItem,
    point_size: f64,
) -> Result<SceneResolvedTextFont, String> {
    for font_path in &item.font.file_candidates {
        let Some(path_string) = font_path.to_str() else {
            return Err(format!(
                "font path {} is not valid UTF-8",
                font_path.display()
            ));
        };
        let path_string = NSString::from_str(path_string);
        let url = NSURL::fileURLWithPath(&path_string);
        if let Some(descriptors) =
            unsafe { CTFontManagerCreateFontDescriptorsFromURL(url.as_ref()) }
        {
            let typed_descriptors: &CFArray<CTFontDescriptor> =
                unsafe { &*((&*descriptors) as *const _ as *const CFArray<CTFontDescriptor>) };
            if let Some(descriptor) = typed_descriptors.get(0) {
                if let Some(font) = NSFont::fontWithDescriptor_size(descriptor.as_ref(), point_size)
                {
                    return Ok(SceneResolvedTextFont {
                        font,
                        fallback_detail: None,
                    });
                }
            }
        }
    }

    for family_name in &item.font.family_candidates {
        let family_name = NSString::from_str(family_name);
        if let Some(font) = NSFont::fontWithName_size(&family_name, point_size) {
            return Ok(SceneResolvedTextFont {
                font,
                fallback_detail: None,
            });
        }
    }

    let fallback_detail = item
        .font
        .authored_reference
        .as_ref()
        .map(|authored_reference| SceneTextFontFallbackDetail {
            reason: format!(
                "Font reference {authored_reference:?} did not resolve through Scene content, external assets, builtin assets, or authored family candidates; the renderer used the system font as the final fallback."
            ),
            attempted_files: item
                .font
                .file_candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            family_candidates: item.font.family_candidates.clone(),
        });

    Ok(SceneResolvedTextFont {
        font: NSFont::systemFontOfSize(point_size),
        fallback_detail,
    })
}

#[cfg(target_os = "macos")]
fn scene_text_font_cache_key(item: &SceneRenderTextItem, point_size: f64) -> String {
    format!("{}:{}", item.font.cache_key, point_size.to_bits())
}

#[cfg(target_os = "macos")]
fn build_text_attributes(
    font: &NSFont,
    color: &NSColor,
    paragraph_style: &NSMutableParagraphStyle,
) -> Retained<NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>> {
    unsafe {
        NSDictionary::from_slices(
            &[
                NSFontAttributeName,
                NSForegroundColorAttributeName,
                NSParagraphStyleAttributeName,
            ],
            &[font, color, paragraph_style],
        )
    }
}

#[cfg(target_os = "macos")]
fn nscolor_from_scene_color(color: SceneRenderColor) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        color.red as f64 / 255.0,
        color.green as f64 / 255.0,
        color.blue as f64 / 255.0,
        color.alpha as f64 / 255.0,
    )
}

#[cfg(target_os = "macos")]
fn paragraph_style_for_text(item: &SceneRenderTextItem) -> Retained<NSMutableParagraphStyle> {
    let style = NSMutableParagraphStyle::new();
    style.setAlignment(match item.horizontal_align {
        SceneTextHorizontalAlign::Left => NSTextAlignment::Left,
        SceneTextHorizontalAlign::Right => NSTextAlignment::Right,
        SceneTextHorizontalAlign::Center => NSTextAlignment::Center,
    });
    style.setLineBreakMode(if item.limit_use_ellipsis {
        NSLineBreakMode::ByTruncatingTail
    } else if item.limit_width {
        NSLineBreakMode::ByWordWrapping
    } else {
        NSLineBreakMode::ByClipping
    });
    style
}

#[cfg(target_os = "macos")]
fn text_drawing_options(item: &SceneRenderTextItem) -> NSStringDrawingOptions {
    let mut options =
        NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading;
    if item.limit_use_ellipsis || item.max_rows.unwrap_or_default() > 1 {
        options |= NSStringDrawingOptions::TruncatesLastVisibleLine;
    }
    options
}

#[cfg(target_os = "macos")]
fn scene_projection(
    view: &MTKView,
    plan: &SceneRenderPlan,
    camera_offset: (f64, f64),
) -> SceneProjection {
    let drawable_size = view.drawableSize();
    scene_projection_for_size(
        drawable_size.width,
        drawable_size.height,
        plan,
        camera_offset,
    )
}

#[cfg(target_os = "macos")]
fn scene_projection_for_size(
    view_width: f64,
    view_height: f64,
    plan: &SceneRenderPlan,
    camera_offset: (f64, f64),
) -> SceneProjection {
    let view_width = view_width.max(1.0);
    let view_height = view_height.max(1.0);
    let cover_scale = (view_width / plan.canvas_width)
        .max(view_height / plan.canvas_height)
        .max(0.001);
    let camera_scale = cover_scale * plan.camera.zoom.max(0.001);
    SceneProjection {
        scene_origin_x: (view_width - plan.canvas_width * camera_scale) / 2.0 + camera_offset.0,
        scene_origin_y: (view_height - plan.canvas_height * camera_scale) / 2.0 + camera_offset.1,
        scene_canvas_height: plan.canvas_height,
        camera_scale,
        view_width,
        view_height,
    }
}

#[cfg(target_os = "macos")]
fn quad_primitive_from_render_quad(
    quad: SceneRenderQuad,
    color: SceneRenderColor,
) -> SceneQuadPrimitive {
    SceneQuadPrimitive {
        left: quad.left,
        top: quad.top,
        width: quad.width,
        height: quad.height,
        rotation: quad.rotation,
        opacity: quad.opacity,
        flip_x: quad.flip_x,
        flip_y: quad.flip_y,
        uv_rect: full_quad_uv_rect(),
        color,
        transform_origin_x: quad.left + quad.width / 2.0,
        transform_origin_y: quad.top + quad.height / 2.0,
    }
}

#[cfg(target_os = "macos")]
fn quad_primitive_from_particle(primitive: SceneParticlePrimitive) -> SceneQuadPrimitive {
    let uv = full_quad_uv_rect();
    let uv_rect = [
        uv[0] + primitive.uv_offset[0] as f32,
        uv[1] + primitive.uv_offset[1] as f32,
        uv[2],
        uv[3],
    ];
    SceneQuadPrimitive {
        left: primitive.left,
        top: primitive.top,
        width: primitive.width,
        height: primitive.height,
        rotation: primitive.rotation,
        opacity: primitive.opacity,
        flip_x: false,
        flip_y: false,
        uv_rect,
        color: primitive.color,
        transform_origin_x: primitive.transform_origin_x,
        transform_origin_y: primitive.transform_origin_y,
    }
}

#[cfg(target_os = "macos")]
fn quad_primitive_from_sprite_particle(
    primitive: SceneSpriteParticlePrimitive,
) -> SceneQuadPrimitive {
    SceneQuadPrimitive {
        left: primitive.left,
        top: primitive.top,
        width: primitive.width,
        height: primitive.height,
        rotation: primitive.rotation,
        opacity: primitive.opacity,
        flip_x: false,
        flip_y: false,
        uv_rect: primitive.uv_rect,
        color: primitive.color,
        transform_origin_x: primitive.transform_origin_x,
        transform_origin_y: primitive.transform_origin_y,
    }
}

#[cfg(all(target_os = "macos", test))]
fn build_scene_vertices(
    item: &SceneRenderVisualItem,
    plan: &SceneRenderPlan,
    view_width: f64,
    view_height: f64,
    camera_offset: (f64, f64),
) -> [SceneVertex; 6] {
    let projection = scene_projection_for_size(view_width, view_height, plan, camera_offset);
    build_projected_quad_vertices(
        quad_primitive_from_render_quad(item.quad, SceneRenderColor::default()),
        &projection,
    )
}

#[cfg(target_os = "macos")]
fn build_projected_quad_vertices(
    quad: SceneQuadPrimitive,
    projection: &SceneProjection,
) -> [SceneVertex; 6] {
    let center_x =
        projection.scene_origin_x + (quad.left + quad.width / 2.0) * projection.camera_scale;
    let center_y =
        projection.scene_origin_y + (quad.top + quad.height / 2.0) * projection.camera_scale;
    let origin_x = projection.scene_origin_x + quad.transform_origin_x * projection.camera_scale;
    let origin_y = projection.scene_origin_y + quad.transform_origin_y * projection.camera_scale;
    let half_width = quad.width * projection.camera_scale / 2.0;
    let half_height = quad.height * projection.camera_scale / 2.0;
    let flip_x = if quad.flip_x { -1.0 } else { 1.0 };
    let flip_y = if quad.flip_y { -1.0 } else { 1.0 };
    let (sin, cos) = quad.rotation.sin_cos();
    let color = color_to_shader(quad.color);
    let opacity = quad.opacity as f32;
    let [u0, v0, u1, v1] = quad.uv_rect;

    let corner = |x: f64, y: f64| -> [f32; 2] {
        let local_center_x = x * flip_x;
        let local_center_y = y * flip_y;
        let screen_x = center_x + local_center_x;
        let screen_y = center_y + local_center_y;
        let translated_x = screen_x - origin_x;
        let translated_y = screen_y - origin_y;
        let rotated_x = translated_x * cos - translated_y * sin;
        let rotated_y = translated_x * sin + translated_y * cos;
        let final_x = origin_x + rotated_x;
        let final_y = origin_y + rotated_y;
        [
            ((final_x / projection.view_width) * 2.0 - 1.0) as f32,
            (1.0 - (final_y / projection.view_height) * 2.0) as f32,
        ]
    };

    let top_left = corner(-half_width, -half_height);
    let top_right = corner(half_width, -half_height);
    let bottom_left = corner(-half_width, half_height);
    let bottom_right = corner(half_width, half_height);

    [
        SceneVertex {
            position: top_left,
            uv: [u0, v0],
            color,
            opacity,
        },
        SceneVertex {
            position: bottom_left,
            uv: [u0, v1],
            color,
            opacity,
        },
        SceneVertex {
            position: top_right,
            uv: [u1, v0],
            color,
            opacity,
        },
        SceneVertex {
            position: top_right,
            uv: [u1, v0],
            color,
            opacity,
        },
        SceneVertex {
            position: bottom_left,
            uv: [u0, v1],
            color,
            opacity,
        },
        SceneVertex {
            position: bottom_right,
            uv: [u1, v1],
            color,
            opacity,
        },
    ]
}

#[cfg(target_os = "macos")]
fn full_quad_uv_rect() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}

#[cfg(target_os = "macos")]
fn build_projected_puppet_mesh_vertices(
    mesh_frame: &crate::services::scene_mdl_service::SceneMdlMeshFrame,
    world_position: [f64; 3],
    world_scale: [f64; 3],
    world_angles: [f64; 3],
    opacity: f64,
    projection: &SceneProjection,
) -> Option<Vec<SceneVertex>> {
    if mesh_frame.positions.is_empty() || mesh_frame.indices.len() < 3 {
        return None;
    }

    let color = color_to_shader(SceneRenderColor::default());
    let opacity = opacity as f32;
    let transform = scene_puppet_world_transform(world_position, world_scale, world_angles);
    let vertices = mesh_frame
        .indices
        .iter()
        .filter_map(|index| {
            let index = *index as usize;
            let position = *mesh_frame.positions.get(index)?;
            let uv = mesh_frame
                .uvs
                .get(index)
                .copied()
                .unwrap_or(glam::Vec2::ZERO);
            let transformed = transform * glam::Vec4::new(position.x, position.y, position.z, 1.0);
            let scene_x = transformed.x as f64;
            let scene_y = projection.scene_canvas_height - transformed.y as f64;
            Some(SceneVertex {
                position: project_absolute_scene_point(scene_x, scene_y, projection),
                uv: [uv.x, uv.y],
                color,
                opacity,
            })
        })
        .collect::<Vec<_>>();
    (vertices.len() >= 3).then_some(vertices)
}

#[cfg(target_os = "macos")]
fn scene_puppet_world_transform(
    world_position: [f64; 3],
    world_scale: [f64; 3],
    world_angles: [f64; 3],
) -> glam::Mat4 {
    let position = glam::Vec3::new(
        world_position[0] as f32,
        world_position[1] as f32,
        world_position[2] as f32,
    );
    let scale = glam::Vec3::new(
        world_scale[0] as f32,
        world_scale[1] as f32,
        world_scale[2] as f32,
    );
    let angles = glam::Vec3::new(
        world_angles[0] as f32,
        world_angles[1] as f32,
        world_angles[2] as f32,
    );

    glam::Mat4::from_translation(position)
        * glam::Mat4::from_rotation_z(angles.z)
        * glam::Mat4::from_rotation_y(angles.y)
        * glam::Mat4::from_rotation_x(angles.x)
        * glam::Mat4::from_scale(scale)
}

#[cfg(target_os = "macos")]
fn project_absolute_scene_point(
    scene_x: f64,
    scene_y: f64,
    projection: &SceneProjection,
) -> [f32; 2] {
    let final_x = projection.scene_origin_x + scene_x * projection.camera_scale;
    let final_y = projection.scene_origin_y + scene_y * projection.camera_scale;
    [
        ((final_x / projection.view_width) * 2.0 - 1.0) as f32,
        (1.0 - (final_y / projection.view_height) * 2.0) as f32,
    ]
}

#[cfg(target_os = "macos")]
fn color_to_shader(color: SceneRenderColor) -> [f32; 4] {
    if color == SceneRenderColor::default() {
        return [1.0, 1.0, 1.0, 1.0];
    }

    [
        color.red as f32 / 255.0,
        color.green as f32 / 255.0,
        color.blue as f32 / 255.0,
        color.alpha as f32 / 255.0,
    ]
}

#[cfg(target_os = "macos")]
fn scene_soundscape_audio_levels(app: &AppHandle, count: usize) -> Option<Vec<f64>> {
    let state = app.try_state::<NativeSceneRendererServiceState>()?;
    let mtm = MainThreadMarker::new()?;
    let soundscape = state.soundscape.lock().ok()?;
    let soundscape = soundscape.as_ref()?;
    soundscape.get(mtm).reactive_levels(count)
}

#[cfg(target_os = "macos")]
fn scene_input_projection_for_view(
    app: &AppHandle,
    view: &MTKView,
    plan: &SceneRenderPlan,
) -> crate::services::scene_input_response_service::SceneInputProjection {
    let Some(window) = view.window() else {
        return project_shared_input_to_scene(
            None,
            SceneInputViewport::default(),
            SceneInputSceneBounds {
                width: plan.canvas_width,
                height: plan.canvas_height,
            },
        );
    };
    let frame = window.frame();
    let snapshot = input_service::current_input_snapshot(app).ok();
    let projection =
        scene_projection_for_size(frame.size.width, frame.size.height, plan, (0.0, 0.0));
    project_shared_input_to_scene_with_cover(
        snapshot.as_ref(),
        SceneInputViewport {
            origin_x: frame.origin.x,
            origin_y: frame.origin.y,
            width: frame.size.width,
            height: frame.size.height,
        },
        SceneInputSceneBounds {
            width: plan.canvas_width,
            height: plan.canvas_height,
        },
        projection.scene_origin_x,
        projection.scene_origin_y,
        projection.camera_scale,
        projection.camera_scale,
    )
}

#[cfg(target_os = "macos")]
fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use crate::services::scene_render_planner_service::{
        SceneClearColor, SceneRenderAudioItem, SceneRenderBlendMode, SceneRenderCamera,
        SceneRenderDrawItem, SceneRenderDrawKind, SceneRenderParticleItem, SceneRenderPlan,
        SceneRenderQuad, SceneRenderSourceKind, SceneRenderTextFontBinding, SceneRenderTextItem,
        SceneRenderVisualItem,
    };

    #[cfg(target_os = "macos")]
    use super::{
        build_scene_pipeline_states, build_scene_vertices, load_phase10_texture_image,
        load_phase10_texture_source, particle_plan_signature, phase10_texture_path_candidates,
        scene_text_font_cache_key, should_retain_visual_in_draw_plan, text_texture_cache_key,
        unpremultiply_rgba_pixels, SceneTextHorizontalAlign,
    };
    use super::{
        now_playing_runtime_warnings_for_scene, phase10_background_source_order,
        plan_native_scene_renderer_runtime, runtime_dependency_warnings_for_plan,
        video_texture_frame_warning, video_texture_source_warning, NativeSceneRendererSnapshot,
        Phase10BackgroundSourceKind, SceneDiagnosticDomain, SceneRenderColor, SceneRendererSpec,
        SceneSessionPlan, AUDIO_INPUT_UNAVAILABLE_CODE, INPUT_SNAPSHOT_UNAVAILABLE_CODE,
    };
    use crate::models::{
        SceneEvaluatedDocument, SceneManifest, SceneNowPlayingAvailability,
        SceneNowPlayingDiagnostic, SceneNowPlayingDiagnosticSeverity, SceneNowPlayingSnapshot,
        SceneNowPlayingState, SceneRuntimeDocument, SceneTextBehavior, SceneTextLayer,
    };
    #[cfg(target_os = "macos")]
    use crate::services::scene_render_graph_service::ScenePhase10InputBinding;
    #[cfg(target_os = "macos")]
    use crate::services::scene_render_planner_service::SceneTextVerticalAlign;
    use crate::services::scene_shader_material_service::{
        SceneCompatEffectKind, SceneMaterialPassPlan, SceneMaterialTextureBinding,
        SceneResolvedMaterialPlan, SceneShaderProgram, SceneShaderProgramKind,
    };
    use crate::services::scene_video_texture_service::{
        self, SceneVideoTextureLifecycleAction, SceneVideoTextureSourceSpec,
        SceneVideoTextureSourceState, VIDEO_TEXTURE_FRAME_FAILED_CODE,
        VIDEO_TEXTURE_SOURCE_FAILED_CODE,
    };
    #[cfg(target_os = "macos")]
    use image::GenericImageView;

    #[cfg(target_os = "macos")]
    use image::RgbaImage;
    #[cfg(target_os = "macos")]
    use objc2_metal::MTLCreateSystemDefaultDevice;
    #[cfg(target_os = "macos")]
    use std::fs;
    #[cfg(target_os = "macos")]
    use std::mem::{align_of, size_of};
    use std::{collections::BTreeMap, path::PathBuf};
    #[cfg(target_os = "macos")]
    use tempfile::tempdir;

    fn sample_spec(wallpaper_id: &str, labels: &[&str], item_count: usize) -> SceneRendererSpec {
        SceneRendererSpec {
            wallpaper_id: wallpaper_id.to_string(),
            render_plan: SceneRenderPlan {
                clear_color: SceneClearColor {
                    red: 10,
                    green: 20,
                    blue: 30,
                    alpha: 255,
                },
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
                draw_order: (0..item_count)
                    .map(|index| SceneRenderDrawItem {
                        object_id: (index + 1) as u32,
                        kind: SceneRenderDrawKind::Visual,
                    })
                    .collect(),
                visuals: (0..item_count)
                    .map(|index| SceneRenderVisualItem {
                        object_id: (index + 1) as u32,
                        object_name: "Hero".to_string(),
                        texture_path: format!("/tmp/asset-{item_count}.png").into(),
                        source_kind: SceneRenderSourceKind::Image,
                        quad: SceneRenderQuad {
                            left: 0.0,
                            top: 0.0,
                            width: 100.0,
                            height: 100.0,
                            rotation: 0.0,
                            opacity: 1.0,
                            flip_x: false,
                            flip_y: false,
                        },
                        blend_mode: SceneRenderBlendMode::Normal,
                    })
                    .collect(),
                texts: Vec::new(),
                audios: Vec::new(),
                particles: Vec::new(),
                rope_particles: Vec::new(),
                sprite_particles: Vec::new(),
                sounds: Vec::new(),
            },
            phase10_graph: super::ScenePhase10GraphPlan::default(),
            window_labels: labels.iter().map(|label| (*label).to_string()).collect(),
            paused: false,
        }
    }

    fn phase10_visual_plan_for_item(item: &SceneRenderVisualItem) -> super::ScenePhase10VisualPlan {
        super::ScenePhase10VisualPlan {
            object_id: item.object_id,
            object_name: item.object_name.clone(),
            quad: item.quad,
            base_color: SceneRenderColor::default(),
            base_source_kind: Some(item.source_kind),
            authored_size: [item.quad.width, item.quad.height],
            world_position: [item.quad.left, item.quad.top, 0.0],
            world_scale: [1.0, 1.0, 1.0],
            world_angles: [0.0, 0.0, item.quad.rotation],
            blend_mode: item.blend_mode,
            base_texture_path: Some(item.texture_path.clone()),
            material: SceneResolvedMaterialPlan {
                material_path: PathBuf::from("/tmp/background-order.material"),
                passes: vec![],
                material_effects: vec![],
            },
            puppet_path: None,
            animation_layers: vec![],
            effect_chain: vec![],
            submesh_count: 0,
            submeshes: vec![],
            mask_binding_count: 0,
            mask_bindings: vec![],
            attachments: vec![],
            morph_target_count: 0,
            container_kind: None,
        }
    }

    fn sample_sound_only_spec(
        wallpaper_id: &str,
        labels: &[&str],
        sound_count: usize,
    ) -> SceneRendererSpec {
        SceneRendererSpec {
            wallpaper_id: wallpaper_id.to_string(),
            render_plan: SceneRenderPlan {
                clear_color: SceneClearColor {
                    red: 10,
                    green: 20,
                    blue: 30,
                    alpha: 255,
                },
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
                draw_order: (0..sound_count)
                    .map(|index| SceneRenderDrawItem {
                        object_id: 50 + index as u32,
                        kind: SceneRenderDrawKind::Sound,
                    })
                    .collect(),
                visuals: Vec::new(),
                texts: Vec::new(),
                audios: Vec::new(),
                particles: Vec::new(),
                rope_particles: Vec::new(),
                sprite_particles: Vec::new(),
                sounds: (0..sound_count)
                    .map(|index| {
                        crate::services::scene_render_planner_service::SceneRenderSoundItem {
                            object_id: 50 + index as u32,
                            object_name: format!("Ambient-{index}"),
                            asset_path: PathBuf::from(format!("/tmp/ambient-{index}.m4a")),
                            looped: true,
                            volume: 0.65,
                        }
                    })
                    .collect(),
            },
            phase10_graph: super::ScenePhase10GraphPlan::default(),
            window_labels: labels.iter().map(|label| (*label).to_string()).collect(),
            paused: false,
        }
    }

    fn sample_runtime_warning_plan() -> SceneRenderPlan {
        SceneRenderPlan {
            clear_color: SceneClearColor::default(),
            canvas_width: 1920.0,
            canvas_height: 1080.0,
            camera: SceneRenderCamera {
                zoom: 1.0,
                center: [0.0, 0.0],
                camera_shake: false,
                camera_shake_amplitude: 0.0,
                camera_shake_speed: 0.0,
                parallax_mouse_influence: 0.4,
            },
            draw_order: vec![
                SceneRenderDrawItem {
                    object_id: 7,
                    kind: SceneRenderDrawKind::Audio,
                },
                SceneRenderDrawItem {
                    object_id: 8,
                    kind: SceneRenderDrawKind::Particle,
                },
            ],
            visuals: Vec::new(),
            texts: Vec::new(),
            audios: vec![SceneRenderAudioItem {
                object_id: 7,
                object_name: "Spectrum".to_string(),
                quad: SceneRenderQuad {
                    left: 0.0,
                    top: 0.0,
                    width: 320.0,
                    height: 100.0,
                    rotation: 0.0,
                    opacity: 1.0,
                    flip_x: false,
                    flip_y: false,
                },
                bar_count: 16,
                gap: 4.0,
                bar_width: 8.0,
                bar_radius: 4.0,
                drawable_top: 12.0,
                drawable_height: 88.0,
                min_scale: 0.1,
                normalized_lower_bound: 0.0,
                volume_factor: 1.0,
                color: SceneRenderColor::default(),
            }],
            particles: vec![SceneRenderParticleItem {
                object_id: 8,
                object_name: "Trail".to_string(),
                particle_kind: crate::models::SceneParticleKind::LineTrail,
                schedule_mode: crate::models::SceneParticleScheduleMode::InputDriven,
                spawn_origin: [0.0, 0.0],
                color: SceneRenderColor::default(),
                size: 3.0,
                emission_rate: 64.0,
                max_count: 32,
                lifetime_ms: 520.0,
                speed_range: [24.0, 48.0],
                instantaneous: false,
                start_time_ms: 0.0,
                sign: 1.0,
                spawn_radius: [0.0, 0.0],
                uv_scrolling: [0.0, 0.0],
                fade_alpha: 0.0,
                subdivision: 1,
                rope_length: 0.0,
            }],
            rope_particles: Vec::new(),
            sprite_particles: Vec::new(),
            sounds: Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    fn rgba_tex_bytes(pixel: [u8; 4], width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TEXV0005\0");
        bytes.extend_from_slice(b"TEXI0001\0");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(b"TEXB0004\0");
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&(pixel.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&pixel);
        bytes
    }

    #[cfg(target_os = "macos")]
    fn rgba_tex_bytes_with_padding(
        pixel: [u8; 4],
        texture_width: u32,
        texture_height: u32,
        content_width: u32,
        content_height: u32,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TEXV0005\0");
        bytes.extend_from_slice(b"TEXI0001\0");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&texture_width.to_le_bytes());
        bytes.extend_from_slice(&texture_height.to_le_bytes());
        bytes.extend_from_slice(&content_width.to_le_bytes());
        bytes.extend_from_slice(&content_height.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(b"TEXB0004\0");
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&texture_width.to_le_bytes());
        bytes.extend_from_slice(&texture_height.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        let payload_size = (texture_width as i32) * (texture_height as i32) * 4;
        bytes.extend_from_slice(&payload_size.to_le_bytes());
        for _ in 0..texture_width * texture_height {
            bytes.extend_from_slice(&pixel);
        }
        bytes
    }

    fn visual_draw_sequence(spec: &SceneRendererSpec) -> Vec<(u32, bool)> {
        let phase10_visuals = spec
            .phase10_graph
            .visuals
            .iter()
            .map(|visual| (visual.object_id, true))
            .collect::<BTreeMap<_, _>>();
        render_submission_sequence(spec)
            .into_iter()
            .filter_map(|(object_id, kind)| {
                (kind == SceneRenderDrawKind::Visual)
                    .then_some((object_id, phase10_visuals.contains_key(&object_id)))
            })
            .collect()
    }

    fn render_submission_sequence(spec: &SceneRendererSpec) -> Vec<(u32, SceneRenderDrawKind)> {
        let phase10_visuals = spec
            .phase10_graph
            .visuals
            .iter()
            .map(|visual| visual.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let visual_ids = spec
            .render_plan
            .visuals
            .iter()
            .map(|item| item.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let text_ids = spec
            .render_plan
            .texts
            .iter()
            .map(|item| item.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let audio_ids = spec
            .render_plan
            .audios
            .iter()
            .map(|item| item.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let particle_ids = spec
            .render_plan
            .particles
            .iter()
            .map(|item| item.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let sprite_particle_ids = spec
            .render_plan
            .sprite_particles
            .iter()
            .map(|item| item.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let sound_ids = spec
            .render_plan
            .sounds
            .iter()
            .map(|item| item.object_id)
            .collect::<std::collections::BTreeSet<_>>();
        let mut sequence = Vec::new();
        let mut drawn_phase10 = std::collections::BTreeSet::new();

        for item in &spec.render_plan.draw_order {
            match item.kind {
                SceneRenderDrawKind::Visual => {
                    if phase10_visuals.contains(&item.object_id)
                        || visual_ids.contains(&item.object_id)
                    {
                        sequence.push((item.object_id, SceneRenderDrawKind::Visual));
                        if phase10_visuals.contains(&item.object_id) {
                            drawn_phase10.insert(item.object_id);
                        }
                    }
                }
                SceneRenderDrawKind::Text if text_ids.contains(&item.object_id) => {
                    sequence.push((item.object_id, SceneRenderDrawKind::Text));
                }
                SceneRenderDrawKind::Audio if audio_ids.contains(&item.object_id) => {
                    sequence.push((item.object_id, SceneRenderDrawKind::Audio));
                }
                SceneRenderDrawKind::Particle if particle_ids.contains(&item.object_id) => {
                    sequence.push((item.object_id, SceneRenderDrawKind::Particle));
                }
                SceneRenderDrawKind::SpriteParticle
                    if sprite_particle_ids.contains(&item.object_id) =>
                {
                    sequence.push((item.object_id, SceneRenderDrawKind::SpriteParticle));
                }
                SceneRenderDrawKind::Sound if sound_ids.contains(&item.object_id) => {
                    sequence.push((item.object_id, SceneRenderDrawKind::Sound));
                }
                _ => {}
            }
        }

        for visual in &spec.phase10_graph.visuals {
            if drawn_phase10.insert(visual.object_id) {
                sequence.push((visual.object_id, SceneRenderDrawKind::Visual));
            }
        }

        sequence
    }

    fn sync_visual_draw_order(spec: &mut SceneRendererSpec) {
        spec.render_plan.draw_order = spec
            .render_plan
            .visuals
            .iter()
            .map(|item| SceneRenderDrawItem {
                object_id: item.object_id,
                kind: SceneRenderDrawKind::Visual,
            })
            .collect();
    }

    #[cfg(target_os = "macos")]
    fn sample_text_item() -> super::SceneRenderTextItem {
        super::SceneRenderTextItem {
            object_id: 7,
            object_name: "Clock".to_string(),
            behavior: crate::models::SceneTextBehavior::Clock,
            quad: SceneRenderQuad {
                left: 1962.0947,
                top: 511.6051,
                width: 479.0,
                height: 220.0,
                rotation: 0.0,
                opacity: 0.7,
                flip_x: false,
                flip_y: false,
            },
            content_left: 0.0,
            content_top: 0.0,
            content_width: 479.0,
            content_height: 220.0,
            text: "22:34:53".to_string(),
            font: SceneRenderTextFontBinding {
                authored_reference: Some("fonts/test-clock.otf".to_string()),
                reference_kind: Some(
                    crate::services::scene_resource_service::SceneTextFontReferenceKind::PathLike,
                ),
                file_candidates: vec![PathBuf::from("/tmp/test-clock.otf")],
                family_candidates: vec!["Test Clock".to_string(), "Helvetica".to_string()],
                cache_key: "font:test-clock".to_string(),
            },
            point_size: 164.0,
            color: SceneRenderColor {
                red: 255,
                green: 255,
                blue: 255,
                alpha: 179,
            },
            horizontal_align: SceneTextHorizontalAlign::Center,
            vertical_align: SceneTextVerticalAlign::Center,
            blur_enabled: false,
            blur_radius: 0.0,
            effect_paths: vec![],
            max_rows: Some(1),
            limit_width: true,
            limit_use_ellipsis: false,
            dynamic_input_generation: None,
        }
    }

    #[test]
    fn runtime_plan_replaces_scene_host_when_wallpaper_changes() {
        let desired = sample_spec("scene-b", &["player"], 1);
        let plan = plan_native_scene_renderer_runtime(
            &NativeSceneRendererSnapshot {
                spec: Some(sample_spec("scene-a", &["player"], 1)),
                labels: vec!["player".to_string()],
            },
            Some(&desired),
        );

        assert!(matches!(
            plan.session,
            SceneSessionPlan::Replace { ref spec } if spec.wallpaper_id == "scene-b"
        ));
        assert_eq!(plan.remove_labels, vec!["player".to_string()]);
        assert_eq!(plan.ensure_labels, vec!["player".to_string()]);
    }

    #[test]
    fn runtime_plan_updates_scene_when_render_plan_changes_in_place() {
        let current = sample_spec("scene-a", &["player"], 1);
        let desired = sample_spec("scene-a", &["player"], 2);
        let plan = plan_native_scene_renderer_runtime(
            &NativeSceneRendererSnapshot {
                spec: Some(current),
                labels: vec!["player".to_string()],
            },
            Some(&desired),
        );

        assert!(matches!(plan.session, SceneSessionPlan::UpdateScene { .. }));
        assert!(plan.remove_labels.is_empty());
    }

    #[test]
    fn runtime_plan_stops_when_scene_runtime_is_removed() {
        let plan = plan_native_scene_renderer_runtime(
            &NativeSceneRendererSnapshot {
                spec: Some(sample_spec("scene-a", &["player"], 1)),
                labels: vec!["player".to_string(), "player-screen-1".to_string()],
            },
            None,
        );

        assert!(matches!(plan.session, SceneSessionPlan::Stop));
        assert_eq!(
            plan.remove_labels,
            vec!["player".to_string(), "player-screen-1".to_string()]
        );
    }

    #[test]
    fn runtime_plan_keeps_session_when_spec_is_stable() {
        let current = sample_spec("scene-a", &["player"], 1);
        let plan = plan_native_scene_renderer_runtime(
            &NativeSceneRendererSnapshot {
                spec: Some(current.clone()),
                labels: vec!["player".to_string()],
            },
            Some(&current),
        );

        assert!(matches!(plan.session, SceneSessionPlan::Keep));
        assert_eq!(plan.ensure_labels, vec!["player".to_string()]);
    }

    #[test]
    fn sound_only_runtime_plan_stays_stable_and_updates_pause_state() {
        let current = sample_sound_only_spec("scene-a", &["player"], 1);
        let stable_plan = plan_native_scene_renderer_runtime(
            &NativeSceneRendererSnapshot {
                spec: Some(current.clone()),
                labels: vec!["player".to_string()],
            },
            Some(&current),
        );

        assert!(current.render_plan.has_renderable_output());
        assert!(matches!(stable_plan.session, SceneSessionPlan::Keep));

        let mut paused = current.clone();
        paused.paused = true;
        let paused_plan = plan_native_scene_renderer_runtime(
            &NativeSceneRendererSnapshot {
                spec: Some(current),
                labels: vec!["player".to_string()],
            },
            Some(&paused),
        );

        assert!(matches!(
            paused_plan.session,
            SceneSessionPlan::UpdateScene { .. }
        ));
    }

    #[test]
    fn phase_09i_runtime_dependency_warnings_cover_audio_and_input_paths() {
        let warnings =
            runtime_dependency_warnings_for_plan(&sample_runtime_warning_plan(), true, true, true);

        assert!(warnings
            .iter()
            .any(|warning| warning.code == AUDIO_INPUT_UNAVAILABLE_CODE));
        assert!(warnings
            .iter()
            .any(|warning| warning.code == INPUT_SNAPSHOT_UNAVAILABLE_CODE));
        assert!(warnings.iter().all(|warning| warning.detail.is_some()));
    }

    #[test]
    fn phase_09i_runtime_dependency_warnings_stay_empty_when_services_are_available() {
        let warnings = runtime_dependency_warnings_for_plan(
            &sample_runtime_warning_plan(),
            false,
            true,
            false,
        );

        assert!(warnings.is_empty());
    }

    #[test]
    fn phase_09i_runtime_dependency_warnings_skip_uninitialized_input_snapshot() {
        let warnings = runtime_dependency_warnings_for_plan(
            &sample_runtime_warning_plan(),
            false,
            false,
            true,
        );

        assert!(warnings
            .iter()
            .all(|warning| warning.code != INPUT_SNAPSHOT_UNAVAILABLE_CODE));
    }

    #[test]
    fn phase_09g_now_playing_provider_warnings_stay_in_text_runtime_diagnostic_layer() {
        let scene = SceneRuntimeDocument {
            runtime_owner_key: None,
            source: SceneManifest {
                text_layers: vec![SceneTextLayer {
                    id: 9,
                    name: "Media Title".to_string(),
                    dependencies: vec![],
                    parent_id: None,
                    alignment: None,
                    anchor: None,
                    horizontal_align: None,
                    vertical_align: None,
                    content: "Fallback".to_string(),
                    behavior: SceneTextBehavior::MediaTitle,
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
                    render_bounds: Some([0.0, 0.0, 300.0, 80.0]),
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
                }],
                ..SceneManifest::default()
            },
            evaluated: SceneEvaluatedDocument::default(),
            now_playing: SceneNowPlayingSnapshot {
                availability: SceneNowPlayingAvailability::Available,
                state: SceneNowPlayingState::PlayingWithoutTitle,
                title: None,
                artist: Some("Artist".to_string()),
                album: None,
                source: Some("Music".to_string()),
                generation: 3,
                updated_at: chrono::Utc::now(),
                refresh_interval_millis: 1500,
                diagnostics: vec![
                    SceneNowPlayingDiagnostic {
                        severity: SceneNowPlayingDiagnosticSeverity::Info,
                        code: "no-media".to_string(),
                        message: "No media.".to_string(),
                        detail: None,
                    },
                    SceneNowPlayingDiagnostic {
                        severity: SceneNowPlayingDiagnosticSeverity::Warning,
                        code: "title-missing".to_string(),
                        message: "Title missing.".to_string(),
                        detail: Some("source: Music".to_string()),
                    },
                ],
            },
        };

        let warnings = now_playing_runtime_warnings_for_scene(&scene);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "now-playing-title-missing");
        let detail = warnings[0].detail.as_ref().expect("detail");
        assert_eq!(detail.domain, SceneDiagnosticDomain::Text);
        assert_eq!(
            detail.runtime_stage.as_deref(),
            Some("now-playing-provider")
        );
        assert_eq!(
            detail.underlying_diagnostic.as_deref(),
            Some("now-playing/title-missing")
        );
    }

    #[test]
    fn phase_09i_video_texture_warning_uses_dedicated_code_and_runtime_stage() {
        let warning =
            video_texture_frame_warning("Loop", "pixel buffer conversion failed".to_string());

        assert_eq!(warning.code, VIDEO_TEXTURE_FRAME_FAILED_CODE);
        assert!(warning.message.contains("Loop"));
        let detail = warning.detail.as_ref().expect("video texture detail");
        assert_eq!(detail.runtime_stage.as_deref(), Some("video-frame"));
        assert_eq!(
            detail.underlying_diagnostic.as_deref(),
            Some("scene-video-texture/frame")
        );
    }

    #[test]
    fn phase_09h_video_texture_source_warning_stays_in_video_texture_domain() {
        let warning = video_texture_source_warning(
            &SceneVideoTextureSourceSpec {
                object_id: 7,
                object_name: "Loop".to_string(),
                asset_path: PathBuf::from("/tmp/loop.mp4"),
            },
            "AVFoundation rejected source".to_string(),
        );

        assert_eq!(warning.code, VIDEO_TEXTURE_SOURCE_FAILED_CODE);
        assert!(warning.message.contains("Loop"));
        let detail = warning.detail.as_ref().expect("video texture detail");
        assert_eq!(detail.domain, SceneDiagnosticDomain::VideoTexture);
        assert_eq!(detail.runtime_stage.as_deref(), Some("video-source-sync"));
        assert_eq!(
            detail.underlying_diagnostic.as_deref(),
            Some("scene-video-texture/source-sync")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scene_shader_source_compiles_and_creates_pipeline_states() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let _pipelines =
            build_scene_pipeline_states(&device).expect("Scene shader library and pipelines");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_mesh_vertices_respect_authored_visual_size() {
        let projection = super::SceneProjection {
            scene_origin_x: 0.0,
            scene_origin_y: 0.0,
            scene_canvas_height: 300.0,
            camera_scale: 1.0,
            view_width: 400.0,
            view_height: 300.0,
        };
        let mesh = crate::services::scene_mdl_service::SceneMdlMeshFrame {
            positions: vec![
                glam::Vec3::new(-1.0, -1.0, 0.0),
                glam::Vec3::new(1.0, -1.0, 0.0),
                glam::Vec3::new(-1.0, 1.0, 0.0),
            ],
            uvs: vec![
                glam::Vec2::new(0.0, 0.0),
                glam::Vec2::new(1.0, 0.0),
                glam::Vec2::new(0.0, 1.0),
            ],
            indices: vec![0, 1, 2],
        };
        let vertices = super::build_projected_puppet_mesh_vertices(
            &mesh,
            [120.0, 160.0, 0.0],
            [12.0, 9.0, 1.0],
            [0.0, 0.0, 0.0],
            1.0,
            &projection,
        )
        .expect("mesh vertices");

        assert_eq!(vertices.len(), 3);
        assert!(vertices
            .iter()
            .all(|vertex| vertex.position[0].abs() <= 1.0));
        assert!(vertices
            .iter()
            .all(|vertex| vertex.position[1].abs() <= 1.0));
        let min_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(max_x - min_x < 0.15);
        assert!(max_y - min_y < 0.15);
        assert!(vertices[2].position[1] > vertices[0].position[1]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_puppet_mesh_vertices_follow_world_transform_directly() {
        let projection = super::SceneProjection {
            scene_origin_x: 0.0,
            scene_origin_y: 0.0,
            scene_canvas_height: 300.0,
            camera_scale: 1.0,
            view_width: 400.0,
            view_height: 300.0,
        };
        let mesh = crate::services::scene_mdl_service::SceneMdlMeshFrame {
            positions: vec![
                glam::Vec3::new(-10.0, -5.0, 0.0),
                glam::Vec3::new(10.0, -5.0, 0.0),
                glam::Vec3::new(-10.0, 15.0, 0.0),
            ],
            uvs: vec![
                glam::Vec2::new(0.0, 0.0),
                glam::Vec2::new(1.0, 0.0),
                glam::Vec2::new(0.0, 1.0),
            ],
            indices: vec![0, 1, 2],
        };
        let vertices = super::build_projected_puppet_mesh_vertices(
            &mesh,
            [210.0, 90.0, 0.0],
            [1.5, 2.0, 1.0],
            [0.0, 0.0, 0.0],
            1.0,
            &projection,
        )
        .expect("mesh vertices");
        let min_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);

        let expected_min_x = (((195.0_f64) / projection.view_width) * 2.0 - 1.0) as f32;
        let expected_max_x = (((225.0_f64) / projection.view_width) * 2.0 - 1.0) as f32;
        let expected_top_y = (1.0 - (180.0_f64 / projection.view_height) * 2.0) as f32;
        let expected_bottom_y = (1.0 - (220.0_f64 / projection.view_height) * 2.0) as f32;

        assert!((min_x - expected_min_x).abs() < 0.02);
        assert!((max_x - expected_max_x).abs() < 0.02);
        assert!((max_y - expected_top_y).abs() < 0.02);
        assert!((min_y - expected_bottom_y).abs() < 0.02);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn particle_signature_stays_stable_when_only_text_changes() {
        let mut plan = sample_runtime_warning_plan();
        plan.audios.clear();
        plan.draw_order.clear();
        plan.particles = vec![
            crate::services::scene_render_planner_service::SceneRenderParticleItem {
                object_id: 9,
                object_name: "trail".to_string(),
                particle_kind: crate::models::SceneParticleKind::LineTrail,
                schedule_mode: crate::models::SceneParticleScheduleMode::InputDriven,
                spawn_origin: [0.0, 0.0],
                color: super::SceneRenderColor {
                    red: 255,
                    green: 255,
                    blue: 255,
                    alpha: 255,
                },
                size: 12.0,
                emission_rate: 48.0,
                max_count: 32,
                lifetime_ms: 520.0,
                speed_range: [24.0, 48.0],
                instantaneous: false,
                start_time_ms: 0.0,
                sign: 1.0,
                spawn_radius: [0.0, 0.0],
                uv_scrolling: [0.0, 0.0],
                fade_alpha: 0.0,
                subdivision: 1,
                rope_length: 0.0,
            },
        ];
        let signature = particle_plan_signature(&plan);

        let mut next = plan.clone();
        next.texts.push(SceneRenderTextItem {
            object_id: 99,
            object_name: "Clock".to_string(),
            behavior: crate::models::SceneTextBehavior::Static,
            quad: SceneRenderQuad {
                left: 0.0,
                top: 0.0,
                width: 100.0,
                height: 40.0,
                rotation: 0.0,
                opacity: 1.0,
                flip_x: false,
                flip_y: false,
            },
            content_left: 0.0,
            content_top: 0.0,
            content_width: 100.0,
            content_height: 40.0,
            text: "12:00".to_string(),
            font: crate::services::scene_render_planner_service::SceneRenderTextFontBinding {
                authored_reference: None,
                reference_kind: None,
                file_candidates: Vec::new(),
                family_candidates: Vec::new(),
                cache_key: "default".to_string(),
            },
            point_size: 24.0,
            color: SceneRenderColor::default(),
            horizontal_align:
                crate::services::scene_render_planner_service::SceneTextHorizontalAlign::Center,
            vertical_align:
                crate::services::scene_render_planner_service::SceneTextVerticalAlign::Center,
            blur_enabled: false,
            blur_radius: 0.0,
            effect_paths: Vec::new(),
            max_rows: None,
            limit_width: false,
            limit_use_ellipsis: false,
            dynamic_input_generation: None,
        });

        assert_eq!(signature, particle_plan_signature(&next));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scene_vertex_layout_matches_metal_constant_buffer_stride() {
        assert_eq!(size_of::<super::SceneVertex>(), 36);
        assert_eq!(align_of::<super::SceneVertex>(), 4);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unpremultiply_rgba_restores_straight_alpha_channels() {
        let mut image =
            RgbaImage::from_raw(1, 1, vec![90, 40, 20, 128]).expect("premultiplied RGBA pixel");

        unpremultiply_rgba_pixels(&mut image);

        assert_eq!(image.into_raw(), vec![179, 80, 40, 128]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_texture_cache_key_ignores_quad_position_only_changes() {
        let mut item = sample_text_item();
        let key = text_texture_cache_key(&item);

        item.quad.left += 240.0;
        item.quad.top += 80.0;

        assert_eq!(key, text_texture_cache_key(&item));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_texture_cache_key_tracks_style_and_effect_path_changes() {
        let mut item = sample_text_item();
        let key = text_texture_cache_key(&item);

        item.color.alpha = 255;
        assert_ne!(key, text_texture_cache_key(&item));

        let mut item = sample_text_item();
        item.effect_paths.push("effects/glow.json".to_string());
        assert_ne!(key, text_texture_cache_key(&item));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_texture_cache_key_tracks_dynamic_input_generation() {
        let mut item = sample_text_item();
        item.behavior = crate::models::SceneTextBehavior::MediaTitle;
        item.dynamic_input_generation = Some(1);
        let key = text_texture_cache_key(&item);

        item.dynamic_input_generation = Some(2);

        assert_ne!(key, text_texture_cache_key(&item));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_font_cache_key_tracks_font_binding_changes() {
        let mut item = sample_text_item();
        let key = scene_text_font_cache_key(&item, item.point_size);

        item.font.cache_key = "font:updated".to_string();
        assert_ne!(key, scene_text_font_cache_key(&item, item.point_size));

        let mut item = sample_text_item();
        item.font
            .family_candidates
            .push("DIN Alternate".to_string());
        assert_ne!(
            text_texture_cache_key(&sample_text_item()),
            text_texture_cache_key(&item)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_font_cache_preserves_fallback_diagnostics_for_family_only_fonts() {
        let mut item = sample_text_item();
        item.font.authored_reference = Some("__WallpaperPhase09A_MissingFamily__".to_string());
        item.font.reference_kind =
            Some(crate::services::scene_resource_service::SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["__WallpaperPhase09A_MissingFamily__".to_string()];
        item.font.cache_key = "font:phase-09a-missing-family".to_string();

        let first =
            super::scene_text_font_with_point_size(&item, 17.0).expect("first font resolution");
        let second =
            super::scene_text_font_with_point_size(&item, 17.0).expect("cached font resolution");

        assert!(first.fallback_detail.is_some());
        assert_eq!(first.fallback_detail, second.fallback_detail);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plain_text_rasterization_does_not_add_unauthored_shadow_pixels() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Static;
        item.object_name = "Plain Caption".to_string();
        item.text = "SHADOW".to_string();
        item.point_size = 72.0;
        item.content_width = 420.0;
        item.content_height = 180.0;
        item.quad.width = 420.0;
        item.quad.height = 180.0;
        item.color.alpha = 255;
        item.font.authored_reference = Some("Helvetica".to_string());
        item.font.reference_kind =
            Some(crate::services::scene_resource_service::SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["Helvetica".to_string()];
        item.font.cache_key = "font:phase-09a-plain-shadow-regression".to_string();

        let rasterized = super::rasterize_text_texture(&item).expect("plain text raster");
        let dark_shadow_pixels = rasterized
            .image
            .to_rgba8()
            .pixels()
            .filter(|pixel| pixel.0[3] > 8 && pixel.0[0] < 80 && pixel.0[1] < 80 && pixel.0[2] < 80)
            .count();

        assert_eq!(dark_shadow_pixels, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vertical_calendar_text_uses_native_font_metrics_before_rasterizing() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Date;
        item.text = "2\n9\n\nA\nP\nR\n\n2\n0\n2\n6".to_string();
        item.point_size = 140.0;
        item.content_width = 120.0;
        item.content_height = 120.0;
        item.quad.width = 140.0;
        item.quad.height = 140.0;
        item.font.authored_reference = Some("Helvetica".to_string());
        item.font.reference_kind =
            Some(crate::services::scene_resource_service::SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["Helvetica".to_string()];
        item.font.cache_key = "font:phase-09a-vertical-calendar".to_string();

        let string = super::NSString::from_str(item.text.as_str());
        let color = super::nscolor_from_scene_color(item.color);
        let paragraph_style = super::paragraph_style_for_text(&item);
        let options = super::text_drawing_options(&item);
        let resolved_point_size =
            super::resolve_scene_text_point_size(&item, &string, &color, &paragraph_style, options)
                .expect("resolved point size");
        let resolved_font = super::scene_text_font_with_point_size(&item, resolved_point_size)
            .expect("resolved font")
            .font;
        let attributes = super::build_text_attributes(&resolved_font, &color, &paragraph_style);
        let measured = super::measure_scene_text_bounds(&string, &attributes, &item, options);

        assert!(resolved_point_size < item.point_size);
        assert!(measured.size.height <= item.content_height + 1.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vertical_calendar_text_does_not_shrink_against_single_line_width() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Date;
        item.text = "2\n9\n\nA\nP\nR\n\n2\n0\n2\n6".to_string();
        item.point_size = 140.0;
        item.content_width = 72.0;
        item.content_height = 3000.0;
        item.quad.width = 48.0;
        item.quad.height = 3020.0;
        item.font.authored_reference = Some("Helvetica".to_string());
        item.font.reference_kind =
            Some(crate::services::scene_resource_service::SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["Helvetica".to_string()];
        item.font.cache_key = "font:phase-09a-vertical-calendar-narrow".to_string();

        let string = super::NSString::from_str(item.text.as_str());
        let color = super::nscolor_from_scene_color(item.color);
        let paragraph_style = super::paragraph_style_for_text(&item);
        let options = super::text_drawing_options(&item);
        let resolved_point_size =
            super::resolve_scene_text_point_size(&item, &string, &color, &paragraph_style, options)
                .expect("resolved point size");

        assert!(resolved_point_size > item.point_size * 0.5);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dynamic_text_fit_scale_clamps_to_width_when_real_glyph_bounds_overflow() {
        let item = sample_text_item();
        let scale = super::scene_text_fit_scale(&item, 620.0, 180.0);

        assert!(scale < 1.0);
        assert!((scale - (item.content_width / 620.0)).abs() < 0.0001);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn static_text_fit_scale_tracks_box_width() {
        let mut item = sample_text_item();
        item.behavior = crate::models::SceneTextBehavior::Static;
        item.content_width = 300.0;
        item.content_height = 120.0;

        let scale = super::scene_text_fit_scale(&item, 600.0, 80.0);

        assert!((scale - 0.5).abs() < 0.0001);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn audio_bar_rotation_flips_scene_angle_into_screen_space() {
        assert!((super::scene_audio_bar_rotation(-1.5708) - 1.5708).abs() < 0.0001);
        assert!((super::scene_audio_bar_rotation(0.35) + 0.35).abs() < 0.0001);
    }

    #[test]
    fn scene_video_texture_module_filters_and_syncs_scene_video_sources() {
        let current = BTreeMap::from([
            (
                7_u32,
                SceneVideoTextureSourceState {
                    asset_path: PathBuf::from("/tmp/loop-a.mp4"),
                    paused: false,
                },
            ),
            (
                8_u32,
                SceneVideoTextureSourceState {
                    asset_path: PathBuf::from("/tmp/loop-b.mp4"),
                    paused: false,
                },
            ),
        ]);
        let video = SceneRenderVisualItem {
            object_id: 7,
            object_name: "Loop".to_string(),
            texture_path: PathBuf::from("/tmp/loop-a.mp4"),
            source_kind: SceneRenderSourceKind::Video,
            quad: SceneRenderQuad {
                left: 0.0,
                top: 0.0,
                width: 100.0,
                height: 100.0,
                rotation: 0.0,
                opacity: 1.0,
                flip_x: false,
                flip_y: false,
            },
            blend_mode: SceneRenderBlendMode::Normal,
        };
        let mut image = video.clone();
        image.object_id = 9;
        image.object_name = "Poster".to_string();
        image.source_kind = SceneRenderSourceKind::Image;
        image.texture_path = PathBuf::from("/tmp/poster.png");

        let desired = scene_video_texture_service::desired_video_texture_sources(&[video, image]);
        let plan =
            scene_video_texture_service::plan_video_texture_source_sync(&current, &desired, true);

        assert_eq!(
            desired,
            vec![SceneVideoTextureSourceSpec {
                object_id: 7,
                object_name: "Loop".to_string(),
                asset_path: PathBuf::from("/tmp/loop-a.mp4"),
            }]
        );
        assert_eq!(
            plan.actions,
            vec![
                SceneVideoTextureLifecycleAction::Remove {
                    object_id: 8,
                    asset_path: PathBuf::from("/tmp/loop-b.mp4"),
                },
                SceneVideoTextureLifecycleAction::SetPaused {
                    object_id: 7,
                    paused: true,
                },
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn baseline_visual_quad_projects_evaluated_rect_directly() {
        let spec = sample_spec("scene-a", &["player"], 1);
        let item = &spec.render_plan.visuals[0];
        let vertices = build_scene_vertices(item, &spec.render_plan, 1920.0, 1080.0, (0.0, 0.0));

        assert_eq!(vertices[0].position, [-1.0, 1.0]);
        assert_eq!(vertices[1].position, [-1.0, 0.8148148]);
        assert_eq!(vertices[2].position, [-0.8958333, 1.0]);
        assert_eq!(vertices[5].position, [-0.8958333, 0.8148148]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scene_projection_keeps_baseline_layout_independent_from_camera_center_metadata() {
        let mut spec = sample_spec("scene-a", &["player"], 1);
        let item = &spec.render_plan.visuals[0];
        let baseline = build_scene_vertices(item, &spec.render_plan, 1920.0, 1080.0, (0.0, 0.0));

        spec.render_plan.camera.center = [-120.0, -45.0];
        let shifted = build_scene_vertices(item, &spec.render_plan, 1920.0, 1080.0, (0.0, 0.0));

        assert!(shifted
            .iter()
            .zip(baseline.iter())
            .all(|(shifted, baseline)| shifted.position == baseline.position));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scene_projection_uses_cover_scaling_for_mismatched_canvas_aspect_ratio() {
        let mut spec = sample_spec("scene-a", &["player"], 1);
        spec.render_plan.canvas_width = 4000.0;
        spec.render_plan.canvas_height = 2336.0;
        spec.render_plan.visuals[0].quad.left = 0.0;
        spec.render_plan.visuals[0].quad.top = 0.0;
        spec.render_plan.visuals[0].quad.width = 4000.0;
        spec.render_plan.visuals[0].quad.height = 2336.0;

        let vertices = build_scene_vertices(
            &spec.render_plan.visuals[0],
            &spec.render_plan,
            2048.0,
            1152.0,
            (0.0, 0.0),
        );

        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);

        assert!(min_y < -1.0);
        assert!(max_y > 1.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rotated_baseline_visual_quad_stays_in_expected_projection_range() {
        let mut spec = sample_spec("scene-a", &["player"], 1);
        spec.render_plan.visuals[0].quad.left = 320.0;
        spec.render_plan.visuals[0].quad.top = 180.0;
        spec.render_plan.visuals[0].quad.width = 640.0;
        spec.render_plan.visuals[0].quad.height = 360.0;
        spec.render_plan.visuals[0].quad.rotation = 0.35;
        let item = &spec.render_plan.visuals[0];
        let vertices = build_scene_vertices(item, &spec.render_plan, 1920.0, 1080.0, (0.0, 0.0));

        let min_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);

        assert!(vertices
            .iter()
            .all(|vertex| { vertex.position[0].is_finite() && vertex.position[1].is_finite() }));
        assert!(max_x - min_x < 1.05);
        assert!(max_y - min_y < 1.05);
    }

    #[test]
    fn phase10_visuals_follow_plan_visual_order() {
        let mut spec = sample_spec("scene-a", &["player"], 4);
        for (index, item) in spec.render_plan.visuals.iter_mut().enumerate() {
            item.object_id = match index {
                0 => 17,
                1 => 67,
                2 => 71,
                _ => 3228,
            };
        }
        sync_visual_draw_order(&mut spec);
        spec.phase10_graph.visuals = vec![
            super::ScenePhase10VisualPlan {
                object_id: 67,
                object_name: "衣".to_string(),
                quad: spec.render_plan.visuals[1].quad,
                base_color: SceneRenderColor::default(),
                base_source_kind: None,
                authored_size: [4500.0, 2000.0],
                world_position: [1872.60095, 870.0, 0.0],
                world_scale: [0.92, 0.92, 0.92],
                world_angles: [0.0, 0.0, 0.0],
                blend_mode: SceneRenderBlendMode::Normal,
                base_texture_path: None,
                material: SceneResolvedMaterialPlan {
                    material_path: PathBuf::from("/tmp/body.material"),
                    passes: vec![],
                    material_effects: vec![],
                },
                puppet_path: Some(PathBuf::from("/tmp/body.mdl")),
                animation_layers: vec![],
                effect_chain: vec![],
                submesh_count: 0,
                submeshes: vec![],
                mask_binding_count: 0,
                mask_bindings: vec![],
                attachments: vec![],
                morph_target_count: 0,
                container_kind: None,
            },
            super::ScenePhase10VisualPlan {
                object_id: 71,
                object_name: "头发".to_string(),
                quad: spec.render_plan.visuals[2].quad,
                base_color: SceneRenderColor::default(),
                base_source_kind: None,
                authored_size: [4500.0, 2000.0],
                world_position: [2106.4047084, 958.7946464, 0.0],
                world_scale: [0.92, 0.92, 0.92],
                world_angles: [0.0, 0.0, 0.0],
                blend_mode: SceneRenderBlendMode::Normal,
                base_texture_path: None,
                material: SceneResolvedMaterialPlan {
                    material_path: PathBuf::from("/tmp/hair.material"),
                    passes: vec![],
                    material_effects: vec![],
                },
                puppet_path: Some(PathBuf::from("/tmp/hair.mdl")),
                animation_layers: vec![],
                effect_chain: vec![],
                submesh_count: 0,
                submeshes: vec![],
                mask_binding_count: 0,
                mask_bindings: vec![],
                attachments: vec![],
                morph_target_count: 0,
                container_kind: None,
            },
        ];

        assert_eq!(
            visual_draw_sequence(&spec),
            vec![(17, false), (67, true), (71, true), (3228, false)]
        );
    }

    #[test]
    fn phase10_background_sources_follow_global_draw_order() {
        let mut spec = sample_spec("scene-a", &["player"], 3);
        spec.render_plan.visuals[0].object_id = 11;
        spec.render_plan.visuals[1].object_id = 33;
        spec.render_plan.visuals[2].object_id = 77;
        let mut text = sample_text_item();
        text.object_id = 22;
        spec.render_plan.texts = vec![text];
        spec.render_plan.draw_order = vec![
            SceneRenderDrawItem {
                object_id: 11,
                kind: SceneRenderDrawKind::Visual,
            },
            SceneRenderDrawItem {
                object_id: 22,
                kind: SceneRenderDrawKind::Text,
            },
            SceneRenderDrawItem {
                object_id: 33,
                kind: SceneRenderDrawKind::Visual,
            },
            SceneRenderDrawItem {
                object_id: 77,
                kind: SceneRenderDrawKind::Visual,
            },
        ];
        spec.phase10_graph.visuals =
            vec![phase10_visual_plan_for_item(&spec.render_plan.visuals[2])];

        let order = phase10_background_source_order(&spec.render_plan, &spec.phase10_graph)
            .into_iter()
            .map(|(item, source)| (item.object_id, source))
            .collect::<Vec<_>>();

        assert_eq!(
            order,
            vec![
                (11, Phase10BackgroundSourceKind::Visual),
                (22, Phase10BackgroundSourceKind::Text),
                (33, Phase10BackgroundSourceKind::Visual),
                (77, Phase10BackgroundSourceKind::Phase10Visual),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renderer_submission_sequence_preserves_global_order_across_types() {
        let mut spec = sample_spec("scene-a", &["player"], 3);
        spec.render_plan.visuals[0].object_id = 11;
        spec.render_plan.visuals[1].object_id = 33;
        spec.render_plan.visuals[2].object_id = 77;

        let mut text = sample_text_item();
        text.object_id = 22;
        let mut warning_plan = sample_runtime_warning_plan();
        let mut audio = warning_plan.audios.remove(0);
        audio.object_id = 66;
        let mut particle = warning_plan.particles.remove(0);
        particle.object_id = 44;

        spec.render_plan.texts = vec![text];
        spec.render_plan.particles = vec![particle];
        spec.render_plan.audios = vec![audio];
        spec.render_plan.sounds = vec![
            crate::services::scene_render_planner_service::SceneRenderSoundItem {
                object_id: 55,
                object_name: "Ambient".to_string(),
                asset_path: PathBuf::from("/tmp/ambient.m4a"),
                looped: true,
                volume: 0.8,
            },
        ];
        spec.render_plan.draw_order = vec![
            SceneRenderDrawItem {
                object_id: 11,
                kind: SceneRenderDrawKind::Visual,
            },
            SceneRenderDrawItem {
                object_id: 22,
                kind: SceneRenderDrawKind::Text,
            },
            SceneRenderDrawItem {
                object_id: 33,
                kind: SceneRenderDrawKind::Visual,
            },
            SceneRenderDrawItem {
                object_id: 44,
                kind: SceneRenderDrawKind::Particle,
            },
            SceneRenderDrawItem {
                object_id: 55,
                kind: SceneRenderDrawKind::Sound,
            },
            SceneRenderDrawItem {
                object_id: 66,
                kind: SceneRenderDrawKind::Audio,
            },
            SceneRenderDrawItem {
                object_id: 77,
                kind: SceneRenderDrawKind::Visual,
            },
        ];

        assert_eq!(
            render_submission_sequence(&spec),
            vec![
                (11, SceneRenderDrawKind::Visual),
                (22, SceneRenderDrawKind::Text),
                (33, SceneRenderDrawKind::Visual),
                (44, SceneRenderDrawKind::Particle),
                (55, SceneRenderDrawKind::Sound),
                (66, SceneRenderDrawKind::Audio),
                (77, SceneRenderDrawKind::Visual),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_consumed_visuals_stay_in_draw_plan_as_order_placeholders() {
        let mut spec = sample_spec("scene-a", &["player"], 4);
        for (index, item) in spec.render_plan.visuals.iter_mut().enumerate() {
            item.object_id = (index + 1) as u32;
        }
        sync_visual_draw_order(&mut spec);
        let consumed = [
            spec.render_plan.visuals[1].object_id,
            spec.render_plan.visuals[2].object_id,
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();

        let retained = spec
            .render_plan
            .visuals
            .iter()
            .filter_map(|item| {
                let image_loaded = item.object_id != spec.render_plan.visuals[0].object_id;
                should_retain_visual_in_draw_plan(item, &consumed, image_loaded)
                    .then_some(item.object_id)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            retained,
            vec![
                spec.render_plan.visuals[1].object_id,
                spec.render_plan.visuals[2].object_id,
                spec.render_plan.visuals[3].object_id,
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn large_phase10_mesh_vertices_use_shared_buffer_strategy() {
        let inline_len = std::mem::size_of::<super::SceneVertex>() * 6;
        let mesh_len = std::mem::size_of::<super::SceneVertex>() * 512;

        assert_eq!(
            super::scene_vertex_upload_strategy(inline_len),
            super::SceneVertexUploadStrategy::InlineBytes
        );
        assert_eq!(
            super::scene_vertex_upload_strategy(mesh_len),
            super::SceneVertexUploadStrategy::SharedBuffer
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_texture_candidates_expand_tex_json_sidecars() {
        let path = PathBuf::from("/tmp/materials/util/White.tex-json");
        assert_eq!(
            phase10_texture_path_candidates(&path),
            vec![
                PathBuf::from("/tmp/materials/util/White.png"),
                PathBuf::from("/tmp/materials/util/White.tex"),
                PathBuf::from("/tmp/materials/util/White.tex-json"),
            ]
        );

        let alternate = PathBuf::from("/tmp/materials/util/White.tex.json");
        assert_eq!(
            phase10_texture_path_candidates(&alternate),
            vec![
                PathBuf::from("/tmp/materials/util/White.png"),
                PathBuf::from("/tmp/materials/util/White.tex"),
                PathBuf::from("/tmp/materials/util/White.tex.json"),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_texture_loader_decodes_raw_tex_payloads() {
        let temp = tempdir().expect("temp dir");
        let tex_path = temp.path().join("white.tex");
        fs::write(&tex_path, rgba_tex_bytes([12, 34, 56, 78], 1, 1)).expect("write tex");

        let image = load_phase10_texture_image(&tex_path).expect("decode phase10 tex");

        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [12, 34, 56, 78]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_texture_loader_prefers_png_sidecar_for_tex_json_metadata() {
        let temp = tempdir().expect("temp dir");
        let metadata_path = temp.path().join("white.tex-json");
        let png_path = temp.path().join("white.png");
        let png = RgbaImage::from_pixel(1, 1, image::Rgba([90, 80, 70, 255]));
        png.save(&png_path).expect("save png");
        fs::write(&metadata_path, br#"{"format":"rgba8888"}"#).expect("write tex metadata");

        let image = load_phase10_texture_image(&metadata_path).expect("decode tex-json image");

        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [90, 80, 70, 255]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_texture_source_uses_uploaded_extent_for_cropped_tex_masks() {
        let temp = tempdir().expect("temp dir");
        let tex_path = temp.path().join("masked.tex");
        fs::write(
            &tex_path,
            rgba_tex_bytes_with_padding([255, 255, 255, 255], 8, 4, 4, 2),
        )
        .expect("write padded tex");

        let decoded = load_phase10_texture_source(&tex_path).expect("decode texture source");

        assert_eq!(decoded.image.dimensions(), (4, 2));
        assert_eq!(decoded.metrics.texture_size, [4.0, 2.0]);
        assert_eq!(decoded.metrics.content_size, [4.0, 2.0]);
        assert_eq!(decoded.metrics.resolution(), [4.0, 2.0, 4.0, 2.0]);
    }

    #[cfg(target_os = "macos")]
    fn compat_effect_program(kind: SceneCompatEffectKind) -> SceneShaderProgram {
        SceneShaderProgram {
            key: format!("test:{kind:?}"),
            kind: SceneShaderProgramKind::EffectCompat(kind),
            metal_source_path: PathBuf::from("/tmp/scene-effect-compat.metal"),
            vertex_entry: "phase10_effect_vertex",
            fragment_entry: "phase10_effect_fragment",
            variant_defines: BTreeMap::new(),
        }
    }

    #[cfg(target_os = "macos")]
    fn compat_effect_pass(
        kind: SceneCompatEffectKind,
        textures: Vec<SceneMaterialTextureBinding>,
        combos: BTreeMap<String, i32>,
    ) -> SceneMaterialPassPlan {
        SceneMaterialPassPlan {
            index: 0,
            shader_ref: format!("effects/{:?}", kind).to_ascii_lowercase(),
            program: compat_effect_program(kind),
            blend_mode: SceneRenderBlendMode::Normal,
            combos,
            uniforms: BTreeMap::new(),
            textures,
            effect_paths: vec![],
        }
    }

    #[cfg(target_os = "macos")]
    fn phase10_test_visual_with_passes(
        puppet_path: Option<PathBuf>,
        passes: Vec<SceneMaterialPassPlan>,
    ) -> super::ScenePhase10VisualPlan {
        super::ScenePhase10VisualPlan {
            object_id: 42,
            object_name: "Phase10Test".to_string(),
            quad: SceneRenderQuad {
                left: 100.0,
                top: 80.0,
                width: 200.0,
                height: 100.0,
                rotation: 0.0,
                opacity: 1.0,
                flip_x: false,
                flip_y: false,
            },
            base_color: SceneRenderColor::default(),
            base_source_kind: None,
            authored_size: [200.0, 100.0],
            world_position: [200.0, 170.0, 0.0],
            world_scale: [1.0, 1.0, 1.0],
            world_angles: [0.0, 0.0, 0.0],
            blend_mode: SceneRenderBlendMode::Normal,
            base_texture_path: None,
            material: SceneResolvedMaterialPlan {
                material_path: PathBuf::from("/tmp/test.material"),
                passes,
                material_effects: vec![],
            },
            puppet_path,
            animation_layers: vec![],
            effect_chain: vec![],
            submesh_count: 0,
            submeshes: vec![],
            mask_binding_count: 0,
            mask_bindings: vec![],
            attachments: vec![],
            morph_target_count: 0,
            container_kind: None,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_puppet_multi_pass_enters_offscreen_previous_chain() {
        let first_pass = compat_effect_pass(SceneCompatEffectKind::Pulse, vec![], BTreeMap::new());
        let mut second_pass =
            compat_effect_pass(SceneCompatEffectKind::Pulse, vec![], BTreeMap::new());
        second_pass.index = 1;

        let single = phase10_test_visual_with_passes(
            Some(PathBuf::from("/tmp/puppet.mdl")),
            vec![first_pass.clone()],
        );
        let multi = phase10_test_visual_with_passes(
            Some(PathBuf::from("/tmp/puppet.mdl")),
            vec![first_pass, second_pass],
        );

        assert!(!super::phase10_visual_requires_offscreen_chain(&single));
        assert!(super::phase10_visual_requires_offscreen_chain(&multi));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_puppet_offscreen_projection_maps_scene_bounds_to_local_target() {
        let visual =
            phase10_test_visual_with_passes(Some(PathBuf::from("/tmp/puppet.mdl")), vec![]);
        let projection = super::phase10_puppet_offscreen_projection(&visual, 300.0, 200, 100);
        let mesh = crate::services::scene_mdl_service::SceneMdlMeshFrame {
            positions: vec![
                glam::Vec3::new(-100.0, -50.0, 0.0),
                glam::Vec3::new(100.0, -50.0, 0.0),
                glam::Vec3::new(-100.0, 50.0, 0.0),
            ],
            uvs: vec![
                glam::Vec2::new(0.0, 0.0),
                glam::Vec2::new(1.0, 0.0),
                glam::Vec2::new(0.0, 1.0),
            ],
            indices: vec![0, 1, 2],
        };

        let vertices = super::build_projected_puppet_mesh_vertices(
            &mesh,
            visual.world_position,
            visual.world_scale,
            visual.world_angles,
            visual.quad.opacity,
            &projection,
        )
        .expect("offscreen puppet mesh vertices");

        let min_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);

        assert!((min_x + 1.0).abs() < 0.001);
        assert!((max_x - 1.0).abs() < 0.001);
        assert!((min_y + 1.0).abs() < 0.001);
        assert!((max_y - 1.0).abs() < 0.001);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_effect_texture_slot_plan_preserves_sparse_authored_ordinals() {
        let pass = super::ScenePhase10EffectPassNode {
            index: 0,
            bindings: vec![],
            target_name: None,
            copy_background: false,
            input_bindings: vec![ScenePhase10InputBinding {
                slot: 0,
                source: super::ScenePhase10InputSource::LocalCurrentVisual,
            }],
            constants: BTreeMap::new(),
            texture_overrides: vec![None, None, None, Some(PathBuf::from("/tmp/mask-b.png"))],
            material_passes: vec![],
        };
        let material_textures = vec![SceneMaterialTextureBinding {
            slot_index: 2,
            slot_name: "g_Texture2".to_string(),
            texture_name: Some("normal.png".to_string()),
            resolved_path: Some(PathBuf::from("/tmp/normal.png")),
        }];

        assert_eq!(
            super::phase10_effect_texture_slot_plan(&material_textures, &pass),
            BTreeMap::from([
                (
                    0,
                    super::Phase10EffectTextureSource::GraphInput(
                        super::ScenePhase10InputSource::LocalCurrentVisual
                    )
                ),
                (2, super::Phase10EffectTextureSource::MaterialSlot(2)),
                (3, super::Phase10EffectTextureSource::OverrideSlot(3)),
            ])
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_effect_texture_slot_plan_keeps_runtime_override_indices_sparse() {
        let pass = super::ScenePhase10EffectPassNode {
            index: 0,
            bindings: vec![],
            target_name: None,
            copy_background: false,
            input_bindings: vec![ScenePhase10InputBinding {
                slot: 0,
                source: super::ScenePhase10InputSource::LocalCurrentVisual,
            }],
            constants: BTreeMap::new(),
            texture_overrides: vec![None, None, Some(PathBuf::from("/tmp/mask.png"))],
            material_passes: vec![],
        };

        assert_eq!(
            super::phase10_effect_texture_slot_plan(&[], &pass),
            BTreeMap::from([
                (
                    0,
                    super::Phase10EffectTextureSource::GraphInput(
                        super::ScenePhase10InputSource::LocalCurrentVisual
                    )
                ),
                (2, super::Phase10EffectTextureSource::OverrideSlot(2)),
            ])
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_effect_texture_slot_plan_preserves_graph_input_source_scope() {
        let pass = super::ScenePhase10EffectPassNode {
            index: 1,
            bindings: vec![],
            target_name: None,
            copy_background: true,
            input_bindings: vec![
                ScenePhase10InputBinding {
                    slot: 0,
                    source: super::ScenePhase10InputSource::CopiedBackground,
                },
                ScenePhase10InputBinding {
                    slot: 1,
                    source: super::ScenePhase10InputSource::Background,
                },
                ScenePhase10InputBinding {
                    slot: 2,
                    source: super::ScenePhase10InputSource::NamedTarget("scratch".to_string()),
                },
            ],
            constants: BTreeMap::new(),
            texture_overrides: vec![None, None, None, Some(PathBuf::from("/tmp/mask.png"))],
            material_passes: vec![],
        };

        assert_eq!(
            super::phase10_effect_texture_slot_plan(&[], &pass),
            BTreeMap::from([
                (
                    0,
                    super::Phase10EffectTextureSource::GraphInput(
                        super::ScenePhase10InputSource::CopiedBackground
                    )
                ),
                (
                    1,
                    super::Phase10EffectTextureSource::GraphInput(
                        super::ScenePhase10InputSource::Background
                    )
                ),
                (
                    2,
                    super::Phase10EffectTextureSource::GraphInput(
                        super::ScenePhase10InputSource::NamedTarget("scratch".to_string())
                    )
                ),
                (3, super::Phase10EffectTextureSource::OverrideSlot(3)),
            ])
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_effect_shader_defines_promote_mask_and_timeoffset_from_authored_slots() {
        let pulse_pass = compat_effect_pass(
            SceneCompatEffectKind::Pulse,
            vec![SceneMaterialTextureBinding {
                slot_index: 2,
                slot_name: "g_Texture2".to_string(),
                texture_name: Some("mask.png".to_string()),
                resolved_path: Some(PathBuf::from("/tmp/mask.png")),
            }],
            BTreeMap::new(),
        );
        let pulse_runtime = super::ScenePhase10EffectPassNode {
            index: 0,
            bindings: vec![],
            target_name: None,
            copy_background: false,
            input_bindings: vec![ScenePhase10InputBinding {
                slot: 0,
                source: super::ScenePhase10InputSource::LocalCurrentVisual,
            }],
            constants: BTreeMap::new(),
            texture_overrides: vec![],
            material_passes: vec![],
        };
        let pulse_resolved = super::Phase10ResolvedPass {
            pass: &pulse_pass,
            context: super::Phase10PassContext::Effect(&pulse_runtime),
        };
        let pulse_defines = super::phase10_pass_shader_defines(&pulse_resolved);
        assert_eq!(pulse_defines.get("MASK"), Some(&1));
        assert_eq!(pulse_defines.get("BLENDMODE"), Some(&9));

        let shake_pass = compat_effect_pass(
            SceneCompatEffectKind::Shake,
            vec![],
            BTreeMap::from([("NOISE".to_string(), 1)]),
        );
        let shake_runtime = super::ScenePhase10EffectPassNode {
            index: 0,
            bindings: vec![],
            target_name: None,
            copy_background: false,
            input_bindings: vec![ScenePhase10InputBinding {
                slot: 0,
                source: super::ScenePhase10InputSource::LocalCurrentVisual,
            }],
            constants: BTreeMap::new(),
            texture_overrides: vec![None, None, Some(PathBuf::from("/tmp/timeoffset.png"))],
            material_passes: vec![],
        };
        let shake_resolved = super::Phase10ResolvedPass {
            pass: &shake_pass,
            context: super::Phase10PassContext::Effect(&shake_runtime),
        };
        let shake_defines = super::phase10_pass_shader_defines(&shake_resolved);
        assert_eq!(shake_defines.get("TIMEOFFSET"), Some(&1));
        assert_eq!(shake_defines.get("NOISE"), Some(&1));

        let waterwaves_pass =
            compat_effect_pass(SceneCompatEffectKind::WaterWaves, vec![], BTreeMap::new());
        let waterwaves_runtime = super::ScenePhase10EffectPassNode {
            index: 0,
            bindings: vec![],
            target_name: None,
            copy_background: false,
            input_bindings: vec![ScenePhase10InputBinding {
                slot: 0,
                source: super::ScenePhase10InputSource::LocalCurrentVisual,
            }],
            constants: BTreeMap::new(),
            texture_overrides: vec![None, Some(PathBuf::from("/tmp/waterwaves-mask.png"))],
            material_passes: vec![],
        };
        let waterwaves_resolved = super::Phase10ResolvedPass {
            pass: &waterwaves_pass,
            context: super::Phase10PassContext::Effect(&waterwaves_runtime),
        };
        let waterwaves_defines = super::phase10_pass_shader_defines(&waterwaves_resolved);
        assert_eq!(waterwaves_defines.get("MASK"), Some(&1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_pulse_shader_consumes_noise_and_mask_in_slot_local_uv_space() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(!shader.contains("0.5 + centered / scale"));
        assert!(!shader.contains("centered = uv - 0.5"));
        assert!(shader.contains("float2 slot2_uv;"));
        assert!(shader.contains(
            "float noise = aux_texture.sample(texture_sampler, noise_uv).r * uniforms.angle;"
        ));
        assert!(shader.contains("float mask = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r;"));
        assert!(shader.contains("sampled = mix(original, sampled, mask);"));
        assert!(shader.contains("#if PULSEALPHA"));
        assert!(shader.contains("#if PULSECOLOR"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_optional_texel_size_uses_zero_for_unbound_aux_slots() {
        assert_eq!(super::phase10_optional_texel_size(None), [0.0, 0.0]);
        assert_eq!(super::phase10_texel_size(None), [1.0, 1.0]);
        assert_eq!(
            super::phase10_optional_texture_resolution(None),
            [1.0, 1.0, 0.0, 0.0]
        );
        assert_eq!(
            super::phase10_texture_resolution(None),
            [1.0, 1.0, 1.0, 1.0]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_displaced_effects_guard_local_input_bounds() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("static float4 sample_displaced_input("));
        assert!(shader.contains("if (!uv_inside_unit(displaced_uv))"));
        assert!(shader.contains("return sample_input(input_texture, texture_sampler, base_uv);"));
        assert!(shader.contains(
            "float4 shaken = sample_displaced_input(input_texture, texture_sampler, primary_uv, texCoordOffset);"
        ));
        assert!(shader.contains("sampled = sample_displaced_input("));
        assert!(shader.contains(
            "float4 displaced = sample_displaced_input(input_texture, texture_sampler, primary_uv, offset);"
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_shake_shader_uses_flow_mask_time_offset_and_optional_mask_instead_of_camera_jitter()
    {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("texture2d<float> aux2_texture [[texture(2)]]"));
        assert!(shader.contains("texture2d<float> aux3_texture [[texture(3)]]"));
        assert!(shader.contains("float2 slot1_uv;"));
        assert!(shader.contains("float4 slot3_resolution;"));
        assert_eq!(
            shader
                .matches("constexpr float phase_scale = 6.28318530718;")
                .count(),
            2
        );
        assert!(shader.contains(
            "flow_phase = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r * 6.28318530718;"
        ));
        assert!(!shader.contains(
            "flow_phase = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r * 1.57079632679;"
        ));
        assert!(shader.contains("flow_mask = (flow_colors - float2(0.498)) * 2.0;"));
        assert!(shader.contains(
            "float2 texCoordOffset = offset * uniforms.intensity * uniforms.intensity * flow_mask;"
        ));
        assert!(shader.contains(
            "float2 mask_uv = stage_vertex.slot3_uv + phase10_offset_between_texture_spaces("
        ));
        assert!(shader.contains("sampled = mix(sampled, shaken, mask);"));
        assert!(!shader.contains("float px = uniforms.intensity * uniforms.texel_size.x;"));
        assert!(!shader.contains("sin(uniforms.time * max(uniforms.speed, 0.001) * 7.0) * px"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_waterripple_shader_distinguishes_mask_and_normal_slots() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("float2 slot1_uv;"));
        assert!(shader.contains("float2 slot2_uv;"));
        assert!(shader.contains(
            "float4 ripple_uv = float4(stage_vertex.slot2_uv, stage_vertex.slot2_uv * 1.333);"
        ));
        assert!(shader.contains("float3 n1 = aux2_texture.sample(texture_sampler, fract(ripple_uv.xy)).xyz * 2.0 - 1.0;"));
        assert!(shader.contains("float mask = 1.0;"));
        assert!(shader.contains("#if MASK"));
        assert!(!shader.contains("#elif PHASE10_EFFECT_BLUR"));
        assert!(!shader.contains("#elif PHASE10_EFFECT_SHINE"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_waterwaves_and_scroll_shaders_consume_timeoffset_and_repeat_contracts() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("float2 slot1_uv;"));
        assert!(shader.contains("float2 slot2_uv;"));
        assert!(shader.contains("time_offset = aux2_texture.sample(texture_sampler, clamp(stage_vertex.slot2_uv, float2(0.0), float2(1.0))).r * 1.57079632679;"));
        assert!(shader.contains("fract((primary_uv + signed_scroll) * repeat)"));
        assert!(shader
            .contains("sign(scroll_speed) * pow(abs(scroll_speed), float2(2.0)) * uniforms.time"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_mask_alpha_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::SceneShaderProgramKind;
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = super::SceneShaderProgram {
            key: "clippingmaskimage4".to_string(),
            kind: SceneShaderProgramKind::MaskAlpha,
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-mask-alpha.metal"),
            vertex_entry: "compat_mask_vertex",
            fragment_entry: "compat_mask_alpha_fragment",
            variant_defines: BTreeMap::new(),
        };
        let pipeline = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::new(),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("mask-alpha shader compile");
        let _ = pipeline;
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_mask_apply_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::SceneShaderProgramKind;
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = super::SceneShaderProgram {
            key: "clippingmaskimage4-apply".to_string(),
            kind: SceneShaderProgramKind::MaskApply,
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-mask-apply.metal"),
            vertex_entry: "compat_mask_apply_vertex",
            fragment_entry: "compat_mask_apply_fragment",
            variant_defines: BTreeMap::new(),
        };
        let pipeline = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::new(),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("mask-apply shader compile");
        let _ = pipeline;
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_copy_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::SceneShaderProgramKind;
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = super::SceneShaderProgram {
            key: "copy".to_string(),
            kind: SceneShaderProgramKind::Copy,
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-copy.metal"),
            vertex_entry: "compat_copy_vertex",
            fragment_entry: "compat_copy_fragment",
            variant_defines: BTreeMap::new(),
        };
        let pipeline = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::new(),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("copy shader compile");
        let _ = pipeline;
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_non_effect_shader_variant_key_includes_program_kind() {
        use crate::services::scene_shader_material_service::SceneShaderProgramKind;
        let mask_alpha = super::SceneShaderProgram {
            key: "mask-a".to_string(),
            kind: SceneShaderProgramKind::MaskAlpha,
            metal_source_path: PathBuf::from("/tmp/mask-alpha.metal"),
            vertex_entry: "vert_main",
            fragment_entry: "frag_main",
            variant_defines: BTreeMap::new(),
        };
        let mask_apply = super::SceneShaderProgram {
            key: "mask-b".to_string(),
            kind: SceneShaderProgramKind::MaskApply,
            metal_source_path: PathBuf::from("/tmp/mask-apply.metal"),
            vertex_entry: "vert_main",
            fragment_entry: "frag_main",
            variant_defines: BTreeMap::new(),
        };
        let key_a = super::phase10_shader_variant_key(
            &mask_alpha,
            &BTreeMap::new(),
            super::SceneRenderBlendMode::Normal,
        );
        let key_b = super::phase10_shader_variant_key(
            &mask_apply,
            &BTreeMap::new(),
            super::SceneRenderBlendMode::Normal,
        );
        assert!(!key_a.is_empty());
        assert!(!key_b.is_empty());
        assert_ne!(
            key_a, key_b,
            "MaskAlpha and MaskApply variant keys must be distinct"
        );
    }
}
