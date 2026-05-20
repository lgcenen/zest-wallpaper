use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    ffi::c_void,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[path = "scene_native_view_host.rs"]
mod scene_native_view_host;
#[path = "scene_metal_renderer.rs"]
mod scene_metal_renderer;

use scene_metal_renderer::{
    NativeSceneMetalRenderer, NativeScenePipelineStates, SceneProjection, SceneQuadPrimitive,
    SceneVertex,
};
#[cfg(all(target_os = "macos", test))]
use scene_metal_renderer::{
    Phase10EffectTextureSource, phase10_effect_texture_slot_plan, phase10_pass_shader_defines,
    phase10_perspective_corner_uniforms, phase10_puppet_offscreen_projection,
    phase10_shader_variant_key, phase10_skew_controls, phase10_spin_controls,
    phase10_transform_controls, phase10_visual_requires_offscreen_chain,
    scene_audio_bar_rotation, should_retain_visual_in_draw_plan,
};
#[cfg(all(target_os = "macos", test))]
use scene_metal_renderer::compile_scene_shader_program_pipeline;
use scene_native_view_host::NativeSceneViewHandle;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{
    models::{
        SceneNowPlayingDiagnostic, SceneNowPlayingDiagnosticSeverity, SceneRuntimeDocument,
        WallpaperRuntimeRecord,
    },
    services::{
        audio_input_service, diagnostic_service, input_service,
        player_host_service,
        runtime_audio_settings_service::{
            normalize_output_volume_percent,
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
            SceneRopeParticlePrimitive, SceneRopeParticleScheduler,
        },
        scene_render_graph_service::{
            build_scene_phase10_graph, ScenePhase10EffectPassNode, ScenePhase10GraphPlan,
            ScenePhase10InputSource, ScenePhase10VisualPlan,
        },
        scene_render_planner_service::{
            build_scene_render_plan_with_resolver, build_scene_render_text_update_with_resolver,
            SceneClearColor, SceneRenderAudioItem, SceneRenderBlendMode, SceneRenderColor,
            SceneRenderDrawItem, SceneRenderDrawKind, SceneRenderIssue, SceneRenderParticleItem,
            SceneRenderPlan, SceneRenderQuad, SceneRenderRopeParticleItem, SceneRenderSourceKind,
            SceneRenderSpriteParticleItem, SceneRenderTextItem, SceneRenderVisualItem,
        },
        scene_resource_service::{builtin_scene_assets_root_for_app, SceneResourceResolver},
        scene_runtime_settings_service,
        scene_shader_material_service::{
            load_shader_program_source, merged_shader_defines, phase10b_effect_contract_for_kind,
            SceneCompatEffectKind, SceneMaterialPassPlan, SceneMaterialTextureBinding,
            SceneMaterialUniformValue, SceneShaderProgram, SceneShaderProgramKind,
        },
        scene_sound_lifecycle_service::{
            SceneSoundPlaybackWarning,
        },
        scene_soundscape_service::{
            scene_soundscape_audio_levels, set_scene_soundscape_output_volume,
            sync_scene_soundscape as sync_native_scene_soundscape, NativeSceneSoundscape,
        },
        scene_sprite_particle_scheduler_service::{
            SceneSpriteParticlePrimitive, SceneSpriteParticleScheduler,
        },
        scene_text_raster_service::{
            scene_text_font_with_point_size, text_texture_cache_key, SceneTextFontFallbackDetail,
        },
        scene_text_script_runtime_service,
        scene_video_texture_service::{
            self, SceneVideoTextureLifecycleAction, SceneVideoTextureSourceSpec,
            SceneVideoTextureSourceState, SceneVideoTextureWarning,
        },
    },
};

#[cfg(target_os = "macos")]
use std::{ptr::NonNull, time::Instant};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use image::DynamicImage;
#[cfg(target_os = "macos")]
use objc2::{
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    AnyThread, MainThreadMarker,
};
#[cfg(target_os = "macos")]
use objc2_av_foundation::{
    AVPlayer, AVPlayerActionAtItemEnd, AVPlayerItem, AVPlayerItemStatus, AVPlayerItemVideoOutput,
};
#[cfg(target_os = "macos")]
use objc2_core_foundation::{CFRetained, CFString};
#[cfg(target_os = "macos")]
use objc2_core_media::CMTime;
#[cfg(target_os = "macos")]
use objc2_core_video::{
    kCVPixelBufferMetalCompatibilityKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess, CVMetalTexture, CVMetalTextureCache,
    CVMetalTextureGetTexture, CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSObjectProtocol, NSString, NSURL};
#[cfg(target_os = "macos")]
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLDevice, MTLLibrary, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLRegion,
    MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineColorAttachmentDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLStorageMode,
    MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};
#[cfg(target_os = "macos")]
use objc2_metal_kit::MTKView;
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
        set_scene_soundscape_output_volume(&state.soundscape, normalized)?;
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

    let labels = player_host_service::live_player_host_label_set(app)
        .into_iter()
        .collect::<Vec<_>>();
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

    warnings.sort_by(|left: &NativeSceneWarning, right: &NativeSceneWarning| {
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
            scene_text_font_with_point_size(item, item.point_size.max(1.0))
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

    for label in player_host_service::live_player_host_label_set(app) {
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
    let output_volume = *state
        .audio_output_volume
        .lock()
        .map_err(|error| error.to_string())?;
    sync_native_scene_soundscape(
        &state.soundscape,
        output_volume,
        spec.map(|spec| (spec.render_plan.sounds.as_slice(), spec.paused)),
        NativeSceneWarning::from_sound_playback_warning,
    )
}

#[cfg(not(target_os = "macos"))]
fn sync_scene_soundscape(
    _app: &AppHandle,
    _state: &NativeSceneRendererServiceState,
    _spec: Option<&SceneRendererSpec>,
) -> Result<Vec<NativeSceneWarning>, String> {
    Ok(Vec::new())
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
    slot4_resolution: [f32; 4],
    slot5_resolution: [f32; 4],
    slot6_resolution: [f32; 4],
    slot7_resolution: [f32; 4],
    texel_size: [f32; 2],
    aux_texel_size: [f32; 2],
    aux2_texel_size: [f32; 2],
    aux3_texel_size: [f32; 2],
    aux4_texel_size: [f32; 2],
    aux5_texel_size: [f32; 2],
    aux6_texel_size: [f32; 2],
    aux7_texel_size: [f32; 2],
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
    uv_rect: [f32; 4],
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
    if plan.particles.is_empty()
        && plan.rope_particles.is_empty()
        && plan.sprite_particles.is_empty()
    {
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
        item.emission_rate.to_bits().hash(&mut hasher);
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
    config.gravity[0].to_bits().hash(hasher);
    config.gravity[1].to_bits().hash(hasher);
    config.drag.to_bits().hash(hasher);
    config.fade_in_ms.to_bits().hash(hasher);
    config.fade_out_ms.to_bits().hash(hasher);
    config.emission_rate.to_bits().hash(hasher);
    config.max_count.hash(hasher);
    config.start_time_ms.to_bits().hash(hasher);
    config.instantaneous.hash(hasher);
    config.sequence_multiplier.to_bits().hash(hasher);
    config.control_points.len().hash(hasher);
    for control_point in &config.control_points {
        control_point.id.hash(hasher);
        control_point.position[0].to_bits().hash(hasher);
        control_point.position[1].to_bits().hash(hasher);
        control_point.lock_to_pointer.hash(hasher);
    }
    match config.sequence_control_point {
        Some(sequence) => {
            true.hash(hasher);
            sequence.count.hash(hasher);
            for vector in sequence.speed_range {
                vector[0].to_bits().hash(hasher);
                vector[1].to_bits().hash(hasher);
            }
        }
        None => false.hash(hasher),
    }
    config.attractors.len().hash(hasher);
    for attractor in &config.attractors {
        attractor.origin_offset[0].to_bits().hash(hasher);
        attractor.origin_offset[1].to_bits().hash(hasher);
        attractor.scale.to_bits().hash(hasher);
        attractor.threshold.to_bits().hash(hasher);
    }
    config.vortexes.len().hash(hasher);
    for vortex in &config.vortexes {
        vortex.origin_offset[0].to_bits().hash(hasher);
        vortex.origin_offset[1].to_bits().hash(hasher);
        vortex.distance_inner.to_bits().hash(hasher);
        vortex.distance_outer.to_bits().hash(hasher);
        vortex.speed_inner.to_bits().hash(hasher);
        vortex.speed_outer.to_bits().hash(hasher);
    }
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
    let image = crate::services::scene_resource_service::load_scene_texture_image(loader_path)
        .map_err(|error| {
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
    item: &SceneRenderVisualItem,
    color: SceneRenderColor,
) -> SceneQuadPrimitive {
    SceneQuadPrimitive {
        left: item.quad.left,
        top: item.quad.top,
        width: item.quad.width,
        height: item.quad.height,
        rotation: item.quad.rotation,
        opacity: item.quad.opacity,
        flip_x: item.quad.flip_x,
        flip_y: item.quad.flip_y,
        uv_rect: item.uv_rect,
        color,
        transform_origin_x: item.quad.left + item.quad.width / 2.0,
        transform_origin_y: item.quad.top + item.quad.height / 2.0,
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
fn quad_primitive_from_rope_particle(
    primitive: SceneRopeParticlePrimitive,
    canvas_height: f64,
) -> SceneQuadPrimitive {
    let dx = primitive.end[0] - primitive.start[0];
    let dy = primitive.end[1] - primitive.start[1];
    let length = (dx * dx + dy * dy).sqrt().max(1.0);
    let center_x = (primitive.start[0] + primitive.end[0]) / 2.0;
    let center_y = (primitive.start[1] + primitive.end[1]) / 2.0;
    let uv_rect = [
        primitive.uv_offset[0] as f32,
        primitive.uv_offset[1] as f32,
        1.0,
        1.0,
    ];
    SceneQuadPrimitive {
        left: center_x - length / 2.0,
        top: canvas_height - center_y - primitive.width / 2.0,
        width: length,
        height: primitive.width.max(1.0),
        rotation: -dy.atan2(dx),
        opacity: primitive.opacity,
        flip_x: false,
        flip_y: false,
        uv_rect,
        color: primitive.color,
        transform_origin_x: center_x,
        transform_origin_y: center_y,
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
        quad_primitive_from_render_quad(item, SceneRenderColor::default()),
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
        SceneRenderQuad, SceneRenderRopeControlPointItem, SceneRenderRopeParticleItem,
        SceneRenderSourceKind, SceneRenderTextFontBinding, SceneRenderTextItem,
        SceneRenderVisualItem,
    };

    #[cfg(target_os = "macos")]
    use super::{
        build_scene_pipeline_states, build_scene_vertices, load_phase10_texture_image,
        load_phase10_texture_source, particle_plan_signature, phase10_texture_path_candidates,
        quad_primitive_from_rope_particle, should_retain_visual_in_draw_plan,
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
    use crate::services::scene_render_planner_service::{
        SceneTextHorizontalAlign, SceneTextVerticalAlign,
    };
    use crate::services::scene_shader_material_service::{
        SceneCompatEffectKind, SceneMaterialPassPlan, SceneMaterialTextureBinding,
        SceneMaterialUniformValue, SceneResolvedMaterialPlan, SceneShaderProgram,
        SceneShaderProgramKind,
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
                        uv_rect: [0.0, 0.0, 1.0, 1.0],
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

    fn sample_rope_particle_item() -> SceneRenderRopeParticleItem {
        SceneRenderRopeParticleItem {
            object_id: 41,
            object_name: "Rope".to_string(),
            renderer_family: crate::models::SceneParticleRendererFamily::Rope,
            schedule_mode: crate::models::SceneParticleScheduleMode::InputDriven,
            control_points: vec![
                SceneRenderRopeControlPointItem {
                    id: 0,
                    position: [10.0, 20.0],
                    lock_to_pointer: false,
                },
                SceneRenderRopeControlPointItem {
                    id: 1,
                    position: [90.0, 60.0],
                    lock_to_pointer: false,
                },
            ],
            emission_rate: 0.0,
            segment_count: 8,
            subdivision: 2,
            length: 120.0,
            min_length: 40.0,
            max_length: 180.0,
            width: 6.0,
            lifetime_ms: 1500.0,
            color: SceneRenderColor {
                red: 200,
                green: 240,
                blue: 255,
                alpha: 255,
            },
            material_path: Some("materials/rope.material".to_string()),
            texture_path: Some(PathBuf::from("/tmp/rope.png")),
            blend_mode: SceneRenderBlendMode::Additive,
            uv_scrolling: [0.25, -0.1],
            fade_alpha: 0.15,
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
        let rope_particle_ids = spec
            .render_plan
            .rope_particles
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
                SceneRenderDrawKind::RopeParticle
                    if rope_particle_ids.contains(&item.object_id) =>
                {
                    sequence.push((item.object_id, SceneRenderDrawKind::RopeParticle));
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

    #[test]
    fn runtime_submission_sequence_includes_rope_particles() {
        let mut spec = sample_spec("scene-rope", &["player"], 0);
        spec.render_plan.draw_order = vec![SceneRenderDrawItem {
            object_id: 41,
            kind: SceneRenderDrawKind::RopeParticle,
        }];
        spec.render_plan.visuals.clear();
        spec.render_plan.rope_particles = vec![sample_rope_particle_item()];

        assert_eq!(
            render_submission_sequence(&spec),
            vec![(41, SceneRenderDrawKind::RopeParticle)]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rope_segment_quad_uses_continuous_segment_geometry() {
        let primitive = super::SceneRopeParticlePrimitive {
            start: [10.0, 20.0],
            end: [70.0, 50.0],
            width: 6.0,
            opacity: 0.75,
            color: SceneRenderColor {
                red: 200,
                green: 240,
                blue: 255,
                alpha: 255,
            },
            uv_offset: [0.25, -0.1],
        };
        let quad = quad_primitive_from_rope_particle(primitive, 100.0);

        assert!(
            quad.width > 60.0,
            "rope quad should span full segment length"
        );
        assert!((quad.height - 6.0).abs() < 0.001);
        assert!((quad.transform_origin_x - 40.0).abs() < 0.001);
        assert!((quad.transform_origin_y - 35.0).abs() < 0.001);
        assert!((quad.top - 62.0).abs() < 0.001);
        assert!(
            quad.rotation.abs() > 0.1,
            "rope quad should rotate with segment direction"
        );
        assert_eq!(quad.uv_rect[0], 0.25);
        assert_eq!(quad.uv_rect[1], -0.1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn scene_vertex_layout_matches_metal_constant_buffer_stride() {
        assert_eq!(size_of::<super::SceneVertex>(), 36);
        assert_eq!(align_of::<super::SceneVertex>(), 4);
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
            uv_rect: [0.0, 0.0, 1.0, 1.0],
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
    fn phase10_perspective_shader_defines_forward_repeat_combo_to_variant() {
        let default_pass =
            compat_effect_pass(SceneCompatEffectKind::Perspective, vec![], BTreeMap::new());
        let runtime = super::ScenePhase10EffectPassNode {
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
        let default_resolved = super::Phase10ResolvedPass {
            pass: &default_pass,
            context: super::Phase10PassContext::Effect(&runtime),
        };

        assert_eq!(
            super::phase10_pass_shader_defines(&default_resolved).get("REPEAT"),
            Some(&0)
        );

        let repeat_pass = compat_effect_pass(
            SceneCompatEffectKind::Perspective,
            vec![],
            BTreeMap::from([("REPEAT".to_string(), 1)]),
        );
        let repeat_resolved = super::Phase10ResolvedPass {
            pass: &repeat_pass,
            context: super::Phase10PassContext::Effect(&runtime),
        };

        assert_eq!(
            super::phase10_pass_shader_defines(&repeat_resolved).get("REPEAT"),
            Some(&1)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_skew_uniforms_pack_skew_axes_and_anchor_controls() {
        let user0 = super::phase10_skew_controls(&BTreeMap::from([
            (
                "skewx".to_string(),
                SceneMaterialUniformValue::Float(0.18f32.to_bits()),
            ),
            (
                "skewy".to_string(),
                SceneMaterialUniformValue::Float((-0.12f32).to_bits()),
            ),
            (
                "anchor".to_string(),
                SceneMaterialUniformValue::Float2([0.35f32.to_bits(), 0.6f32.to_bits()]),
            ),
        ]));

        assert_eq!(user0, [0.18, -0.12, 0.35, 0.6]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_transform_uniforms_pack_offset_scale_anchor_and_rotation() {
        let (user0, user1, angle) = super::phase10_transform_controls(&BTreeMap::from([
            (
                "offset".to_string(),
                SceneMaterialUniformValue::Float2([0.08f32.to_bits(), (-0.06f32).to_bits()]),
            ),
            (
                "scale".to_string(),
                SceneMaterialUniformValue::Float2([1.15f32.to_bits(), 0.85f32.to_bits()]),
            ),
            (
                "anchor".to_string(),
                SceneMaterialUniformValue::Float2([0.4f32.to_bits(), 0.55f32.to_bits()]),
            ),
            (
                "rotation".to_string(),
                SceneMaterialUniformValue::Float(0.35f32.to_bits()),
            ),
        ]));

        assert_eq!(user0, [0.08, -0.06, 1.15, 0.85]);
        assert_eq!(user1, [0.4, 0.55, 0.0, 0.0]);
        assert_eq!(angle, 0.35);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_transform_uniforms_accept_aliases_and_defaults() {
        let (user0, user1, angle) = super::phase10_transform_controls(&BTreeMap::from([
            (
                "translate".to_string(),
                SceneMaterialUniformValue::Float2([0.03f32.to_bits(), 0.04f32.to_bits()]),
            ),
            (
                "center".to_string(),
                SceneMaterialUniformValue::Float2([0.25f32.to_bits(), 0.75f32.to_bits()]),
            ),
            (
                "angle".to_string(),
                SceneMaterialUniformValue::Float((-0.2f32).to_bits()),
            ),
        ]));

        assert_eq!(user0, [0.03, 0.04, 1.0, 1.0]);
        assert_eq!(user1, [0.25, 0.75, 0.0, 0.0]);
        assert_eq!(angle, -0.2);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_skew_uniforms_derive_axes_from_edge_offsets() {
        let user0 = super::phase10_skew_controls(&BTreeMap::from([
            (
                "top".to_string(),
                SceneMaterialUniformValue::Float((-0.09f32).to_bits()),
            ),
            (
                "bottom".to_string(),
                SceneMaterialUniformValue::Float(0.09f32.to_bits()),
            ),
            (
                "left".to_string(),
                SceneMaterialUniformValue::Float(0.06f32.to_bits()),
            ),
            (
                "right".to_string(),
                SceneMaterialUniformValue::Float((-0.14f32).to_bits()),
            ),
        ]));

        assert!((user0[0] - 0.18).abs() < 0.0001);
        assert!((user0[1] + 0.20).abs() < 0.0001);
        assert!((user0[2] - 0.3).abs() < 0.0001);
        assert!((user0[3] - 0.5).abs() < 0.0001);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_spin_uniforms_pack_amount_speed_and_center_controls() {
        let (angle, speed, user0) = super::phase10_spin_controls(&BTreeMap::from([
            (
                "amount".to_string(),
                SceneMaterialUniformValue::Float(0.35f32.to_bits()),
            ),
            (
                "speed".to_string(),
                SceneMaterialUniformValue::Float(2.4f32.to_bits()),
            ),
            (
                "center".to_string(),
                SceneMaterialUniformValue::Float2([0.3f32.to_bits(), 0.7f32.to_bits()]),
            ),
            (
                "size".to_string(),
                SceneMaterialUniformValue::Float(0.18f32.to_bits()),
            ),
            (
                "feather".to_string(),
                SceneMaterialUniformValue::Float(0.04f32.to_bits()),
            ),
        ]));

        assert_eq!(angle, 0.35);
        assert_eq!(speed, 2.4);
        assert_eq!(user0, [0.3, 0.7, 0.18, 0.04]);
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
        assert!(shader.contains("texture2d<float> aux7_texture [[texture(7)]]"));
        assert!(shader.contains("float2 slot1_uv;"));
        assert!(shader.contains("float4 slot3_resolution;"));
        assert!(shader.contains("float4 slot7_resolution;"));
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
    fn phase10_perspective_uniforms_pack_corner_controls_into_effect_uniforms() {
        let (user0, user1) = super::phase10_perspective_corner_uniforms(&BTreeMap::from([
            (
                "point0".to_string(),
                SceneMaterialUniformValue::Float2([0.1f32.to_bits(), 0.2f32.to_bits()]),
            ),
            (
                "point1".to_string(),
                SceneMaterialUniformValue::Float2([0.9f32.to_bits(), 0.15f32.to_bits()]),
            ),
            (
                "point2".to_string(),
                SceneMaterialUniformValue::Float2([0.85f32.to_bits(), 0.95f32.to_bits()]),
            ),
            (
                "point3".to_string(),
                SceneMaterialUniformValue::Float2([0.05f32.to_bits(), 0.9f32.to_bits()]),
            ),
        ]));

        assert_eq!(user0, [0.1, 0.2, 0.9, 0.15]);
        assert_eq!(user1, [0.85, 0.95, 0.05, 0.9]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_perspective_uniforms_default_to_unit_quad_when_corners_missing() {
        let (user0, user1) = super::phase10_perspective_corner_uniforms(&BTreeMap::new());

        assert_eq!(user0, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(user1, [1.0, 1.0, 0.0, 1.0]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_perspective_shader_uses_corner_mapping_instead_of_raw_primary_uv() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("static float2 inverse_bilinear_uv("));
        assert!(shader.contains("float2 p0 = uniforms.user0.xy;"));
        assert!(shader.contains("float2 p3 = uniforms.user1.zw;"));
        assert!(shader.contains(
            "float2 perspective_uv = inverse_bilinear_uv(primary_uv, p0, p1, p2, p3);"
        ));
        assert!(shader.contains("if (!uv_inside_unit(perspective_uv))"));
        assert!(!shader.contains(
            "#elif PHASE10_EFFECT_PERSPECTIVE\n    float mask = step(0.0, stage_vertex.position.w);\n    float2 perspective_uv = primary_uv;"
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_transform_shader_uses_inverse_affine_controls() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("float2 offset = uniforms.user0.xy;"));
        assert!(shader.contains("float2 scale = uniforms.user0.zw;"));
        assert!(shader.contains("float2 anchor = uniforms.user1.xy;"));
        assert!(shader.contains("float2 local = primary_uv - anchor - offset;"));
        assert!(shader.contains("local = rotate2d(local, -uniforms.angle);"));
        assert!(shader.contains("float2 transform_uv = float2(local.x / scale_x, local.y / scale_y) + anchor;"));
        assert!(shader.contains("transform_uv = clamp(transform_uv, float2(0.0), float2(1.0));"));
        assert!(!shader.contains("#elif PHASE10_EFFECT_TRANSFORM\n    sampled = sample_input(input_texture, texture_sampler, fract(primary_uv));"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_transform_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::{
            SceneShaderProgram, SceneShaderProgramKind,
        };
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = SceneShaderProgram {
            key: "test:transform".to_string(),
            kind: SceneShaderProgramKind::EffectCompat(SceneCompatEffectKind::Transform),
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal"),
            vertex_entry: "phase10_effect_vertex",
            fragment_entry: "phase10_effect_fragment",
            variant_defines: BTreeMap::from([("PHASE10_EFFECT_TRANSFORM".to_string(), 1)]),
        };

        let clamp1 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_TRANSFORM".to_string(), 1),
                ("CLAMP".to_string(), 1),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("transform clamp=1 compile");
        let clamp0 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_TRANSFORM".to_string(), 1),
                ("CLAMP".to_string(), 0),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("transform clamp=0 compile");
        let _ = (clamp1, clamp0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_skew_shader_uses_inverse_shear_with_anchor_controls() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("float skew_x = uniforms.user0.x;"));
        assert!(shader.contains("float skew_y = uniforms.user0.y;"));
        assert!(shader.contains("float2 anchor = uniforms.user0.zw;"));
        assert!(shader.contains("float determinant = 1.0 - skew_x * skew_y;"));
        assert!(shader.contains("float2 local = primary_uv - anchor;"));
        assert!(shader.contains("skew_uv = float2("));
        assert!(shader.contains("skew_uv = clamp(skew_uv, float2(0.0), float2(1.0));"));
        assert!(!shader.contains("#elif PHASE10_EFFECT_SKEW\n    float2 skew_uv = primary_uv;\n#if REPEAT\n    skew_uv = fract(skew_uv);\n#endif\n    sampled = sample_input(input_texture, texture_sampler, skew_uv);"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_skew_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::{
            SceneShaderProgram, SceneShaderProgramKind,
        };
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = SceneShaderProgram {
            key: "test:skew".to_string(),
            kind: SceneShaderProgramKind::EffectCompat(SceneCompatEffectKind::Skew),
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal"),
            vertex_entry: "phase10_effect_vertex",
            fragment_entry: "phase10_effect_fragment",
            variant_defines: BTreeMap::from([("PHASE10_EFFECT_SKEW".to_string(), 1)]),
        };

        let repeat0 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_SKEW".to_string(), 1),
                ("REPEAT".to_string(), 0),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("skew repeat=0 compile");
        let repeat1 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_SKEW".to_string(), 1),
                ("REPEAT".to_string(), 1),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("skew repeat=1 compile");
        let _ = (repeat0, repeat1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_spin_shader_rotates_uv_with_amount_and_time() {
        let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal");
        let shader = fs::read_to_string(shader_path).expect("effect compat shader");

        assert!(shader.contains("float anim = uniforms.angle * sin(uniforms.time * max(uniforms.speed, 0.001));"));
        assert!(shader.contains("tex_coord -= center;"));
        assert!(shader.contains("tex_coord = rotate2d(tex_coord, anim);"));
        assert!(shader.contains("tex_coord += center;"));
        assert!(shader.contains("float4 rotated = sample_input(input_texture, texture_sampler, tex_coord);"));
        assert!(!shader.contains("float4 rotated = sample_input(input_texture, texture_sampler, primary_uv);"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_spin_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::{
            SceneShaderProgram, SceneShaderProgramKind,
        };
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = SceneShaderProgram {
            key: "test:spin".to_string(),
            kind: SceneShaderProgramKind::EffectCompat(SceneCompatEffectKind::Spin),
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal"),
            vertex_entry: "phase10_effect_vertex",
            fragment_entry: "phase10_effect_fragment",
            variant_defines: BTreeMap::from([("PHASE10_EFFECT_SPIN".to_string(), 1)]),
        };

        let repeat0 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_SPIN".to_string(), 1),
                ("MASK".to_string(), 0),
                ("REPEAT".to_string(), 0),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("spin repeat=0 compile");
        let repeat1_mask1 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_SPIN".to_string(), 1),
                ("MASK".to_string(), 1),
                ("REPEAT".to_string(), 1),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("spin repeat=1 mask=1 compile");
        let _ = (repeat0, repeat1_mask1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn phase10_perspective_shader_compiles_to_pipeline_state() {
        use crate::services::scene_shader_material_service::{
            SceneShaderProgram, SceneShaderProgramKind,
        };
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let program = SceneShaderProgram {
            key: "test:perspective".to_string(),
            kind: SceneShaderProgramKind::EffectCompat(SceneCompatEffectKind::Perspective),
            metal_source_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources/scene/assets/shaders/compat/scene-effect-compat.metal"),
            vertex_entry: "phase10_effect_vertex",
            fragment_entry: "phase10_effect_fragment",
            variant_defines: BTreeMap::from([("PHASE10_EFFECT_PERSPECTIVE".to_string(), 1)]),
        };

        let repeat0 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_PERSPECTIVE".to_string(), 1),
                ("REPEAT".to_string(), 0),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("perspective repeat=0 compile");
        let repeat1 = super::compile_scene_shader_program_pipeline(
            &device,
            &program,
            &BTreeMap::from([
                ("PHASE10_EFFECT_PERSPECTIVE".to_string(), 1),
                ("REPEAT".to_string(), 1),
            ]),
            super::SceneRenderBlendMode::Normal,
        )
        .expect("perspective repeat=1 compile");
        let _ = (repeat0, repeat1);
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
