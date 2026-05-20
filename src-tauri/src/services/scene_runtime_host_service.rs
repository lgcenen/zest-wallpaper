use std::{
    collections::{btree_map::Entry, BTreeSet},
    sync::Arc,
};

use tauri::AppHandle;

use crate::{
    models::{SceneRuntimeDocument, WallpaperRuntimeRecord},
    services::{
        player_host_service,
        scene_native_renderer_service::{
            NativeSceneRendererRuntime, NativeSceneRendererSnapshot, NativeSceneRendererStateAccess,
            NativeSceneRuntimeActions, NativeSceneViewHandle, NativeSceneWarning,
            SceneRendererSpec, SceneSessionPlan,
        },
        scene_render_graph_service::build_scene_phase10_graph,
        scene_render_planner_service::build_scene_render_plan_with_resolver,
        scene_resource_service::{builtin_scene_assets_root_for_app, SceneResourceResolver},
        scene_runtime_settings_service, scene_text_script_runtime_service,
    },
};

pub(crate) struct NativeSceneRendererPlan {
    pub(crate) session: SceneSessionPlan,
    pub(crate) ensure_labels: Vec<String>,
    pub(crate) remove_labels: Vec<String>,
}

pub(crate) struct DesiredSceneRendererSpec {
    pub(crate) spec: Option<SceneRendererSpec>,
    pub(crate) warnings: Vec<NativeSceneWarning>,
}

pub(crate) fn desired_scene_renderer_spec(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
    now_playing_runtime_warnings_for_scene: fn(&SceneRuntimeDocument) -> Vec<NativeSceneWarning>,
    text_font_runtime_warnings_for_plan:
        fn(&crate::services::scene_render_planner_service::SceneRenderPlan) -> Vec<NativeSceneWarning>,
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

fn text_script_runtime_warnings_for_scene(scene: &SceneRuntimeDocument) -> Vec<NativeSceneWarning> {
    scene_text_script_runtime_service::scene_text_script_runtime_diagnostics(
        scene.runtime_owner_key.as_deref(),
    )
    .into_iter()
    .map(|diagnostic| NativeSceneWarning {
        code: diagnostic.code,
        message: diagnostic.message,
        detail: Some(crate::services::scene_diagnostics::SceneDiagnosticDetail::runtime(
            crate::services::scene_diagnostics::SceneDiagnosticDomain::Text,
            diagnostic.runtime_stage,
            diagnostic.reason,
        )),
    })
    .collect()
}

pub(crate) fn plan_native_scene_renderer_runtime(
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

pub(crate) fn prepare_runtime_actions(
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

pub(crate) fn execute_runtime_actions(
    app: &AppHandle,
    state: &impl NativeSceneRendererStateAccess,
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
            let mut runtime = state.runtime_lock()?;
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
