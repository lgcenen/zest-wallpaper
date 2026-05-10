use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::services::{
    scene_diagnostics::{SceneDiagnosticDetail, SceneDiagnosticDomain},
    scene_render_planner_service::{SceneRenderSourceKind, SceneRenderVisualItem},
};

pub const VIDEO_TEXTURE_FRAME_FAILED_CODE: &str = "video-texture-frame-failed";
pub const VIDEO_TEXTURE_SOURCE_FAILED_CODE: &str = "video-texture-source-failed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneVideoTextureSourceSpec {
    pub object_id: u32,
    pub object_name: String,
    pub asset_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneVideoTextureSourceState {
    pub asset_path: PathBuf,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneVideoTextureSyncPlan {
    pub actions: Vec<SceneVideoTextureLifecycleAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneVideoTextureLifecycleAction {
    Create {
        source: SceneVideoTextureSourceSpec,
        paused: bool,
    },
    Replace {
        source: SceneVideoTextureSourceSpec,
        previous_asset_path: PathBuf,
        paused: bool,
    },
    SetPaused {
        object_id: u32,
        paused: bool,
    },
    Remove {
        object_id: u32,
        asset_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneVideoTextureWarning {
    pub code: String,
    pub message: String,
    pub detail: Option<SceneDiagnosticDetail>,
}

pub fn desired_video_texture_sources(
    visuals: &[SceneRenderVisualItem],
) -> Vec<SceneVideoTextureSourceSpec> {
    let mut desired = BTreeMap::new();
    for item in visuals {
        if item.source_kind == SceneRenderSourceKind::Video {
            desired.insert(
                item.object_id,
                SceneVideoTextureSourceSpec {
                    object_id: item.object_id,
                    object_name: item.object_name.clone(),
                    asset_path: item.texture_path.clone(),
                },
            );
        }
    }
    desired.into_values().collect()
}

pub fn plan_video_texture_source_sync(
    current: &BTreeMap<u32, SceneVideoTextureSourceState>,
    desired: &[SceneVideoTextureSourceSpec],
    paused: bool,
) -> SceneVideoTextureSyncPlan {
    let desired_by_id = desired
        .iter()
        .cloned()
        .map(|source| (source.object_id, source))
        .collect::<BTreeMap<_, _>>();
    let mut actions = Vec::new();

    for (object_id, state) in current {
        if !desired_by_id.contains_key(object_id) {
            actions.push(SceneVideoTextureLifecycleAction::Remove {
                object_id: *object_id,
                asset_path: state.asset_path.clone(),
            });
        }
    }

    for (object_id, source) in desired_by_id {
        match current.get(&object_id) {
            None => actions.push(SceneVideoTextureLifecycleAction::Create { source, paused }),
            Some(state) if state.asset_path != source.asset_path => {
                actions.push(SceneVideoTextureLifecycleAction::Replace {
                    source,
                    previous_asset_path: state.asset_path.clone(),
                    paused,
                });
            }
            Some(state) if state.paused != paused => {
                actions.push(SceneVideoTextureLifecycleAction::SetPaused { object_id, paused });
            }
            Some(_) => {}
        }
    }

    SceneVideoTextureSyncPlan { actions }
}

pub fn video_texture_frame_warning(
    object_name: &str,
    error: impl Into<String>,
) -> SceneVideoTextureWarning {
    SceneVideoTextureWarning {
        code: VIDEO_TEXTURE_FRAME_FAILED_CODE.to_string(),
        message: format!("Scene video {object_name} could not produce a native frame."),
        detail: Some(
            SceneDiagnosticDetail::runtime(
                SceneDiagnosticDomain::VideoTexture,
                "video-frame",
                error.into(),
            )
            .with_underlying_diagnostic("scene-video-texture/frame"),
        ),
    }
}

pub fn video_texture_source_warning(
    object_name: &str,
    asset_path: &Path,
    error: impl Into<String>,
) -> SceneVideoTextureWarning {
    SceneVideoTextureWarning {
        code: VIDEO_TEXTURE_SOURCE_FAILED_CODE.to_string(),
        message: format!(
            "Scene video {object_name} could not prepare native video texture source {}.",
            asset_path.display()
        ),
        detail: Some(
            SceneDiagnosticDetail::runtime(
                SceneDiagnosticDomain::VideoTexture,
                "video-source-sync",
                error.into(),
            )
            .with_underlying_diagnostic("scene-video-texture/source-sync"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::scene_render_planner_service::{
        SceneRenderBlendMode, SceneRenderQuad, SceneRenderVisualItem,
    };

    fn visual(
        object_id: u32,
        source_kind: SceneRenderSourceKind,
        path: &str,
    ) -> SceneRenderVisualItem {
        SceneRenderVisualItem {
            object_id,
            object_name: format!("Visual {object_id}"),
            texture_path: PathBuf::from(path),
            source_kind,
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
        }
    }

    #[test]
    fn desired_sources_filter_scene_video_visuals_only() {
        let sources = desired_video_texture_sources(&[
            visual(7, SceneRenderSourceKind::Video, "/tmp/loop.mp4"),
            visual(8, SceneRenderSourceKind::Image, "/tmp/poster.png"),
        ]);

        assert_eq!(
            sources,
            vec![SceneVideoTextureSourceSpec {
                object_id: 7,
                object_name: "Visual 7".to_string(),
                asset_path: PathBuf::from("/tmp/loop.mp4"),
            }]
        );
    }

    #[test]
    fn sync_plan_reuses_replaces_pauses_and_removes_sources() {
        let current = BTreeMap::from([
            (
                7,
                SceneVideoTextureSourceState {
                    asset_path: PathBuf::from("/tmp/loop-a.mp4"),
                    paused: false,
                },
            ),
            (
                8,
                SceneVideoTextureSourceState {
                    asset_path: PathBuf::from("/tmp/remove.mp4"),
                    paused: false,
                },
            ),
            (
                9,
                SceneVideoTextureSourceState {
                    asset_path: PathBuf::from("/tmp/loop-c.mp4"),
                    paused: false,
                },
            ),
        ]);
        let desired = vec![
            SceneVideoTextureSourceSpec {
                object_id: 7,
                object_name: "Reuse".to_string(),
                asset_path: PathBuf::from("/tmp/loop-a.mp4"),
            },
            SceneVideoTextureSourceSpec {
                object_id: 9,
                object_name: "Replace".to_string(),
                asset_path: PathBuf::from("/tmp/loop-d.mp4"),
            },
            SceneVideoTextureSourceSpec {
                object_id: 10,
                object_name: "Create".to_string(),
                asset_path: PathBuf::from("/tmp/loop-e.mp4"),
            },
        ];

        let plan = plan_video_texture_source_sync(&current, &desired, true);

        assert_eq!(
            plan.actions,
            vec![
                SceneVideoTextureLifecycleAction::Remove {
                    object_id: 8,
                    asset_path: PathBuf::from("/tmp/remove.mp4"),
                },
                SceneVideoTextureLifecycleAction::SetPaused {
                    object_id: 7,
                    paused: true,
                },
                SceneVideoTextureLifecycleAction::Replace {
                    source: SceneVideoTextureSourceSpec {
                        object_id: 9,
                        object_name: "Replace".to_string(),
                        asset_path: PathBuf::from("/tmp/loop-d.mp4"),
                    },
                    previous_asset_path: PathBuf::from("/tmp/loop-c.mp4"),
                    paused: true,
                },
                SceneVideoTextureLifecycleAction::Create {
                    source: SceneVideoTextureSourceSpec {
                        object_id: 10,
                        object_name: "Create".to_string(),
                        asset_path: PathBuf::from("/tmp/loop-e.mp4"),
                    },
                    paused: true,
                },
            ]
        );
    }

    #[test]
    fn video_texture_warnings_use_video_texture_runtime_domain() {
        let frame = video_texture_frame_warning("Loop", "pixel buffer conversion failed");
        assert_eq!(frame.code, VIDEO_TEXTURE_FRAME_FAILED_CODE);
        assert!(frame.message.contains("Loop"));
        let detail = frame.detail.expect("frame detail");
        assert_eq!(detail.domain, SceneDiagnosticDomain::VideoTexture);
        assert_eq!(detail.runtime_stage.as_deref(), Some("video-frame"));
        assert_eq!(
            detail.underlying_diagnostic.as_deref(),
            Some("scene-video-texture/frame")
        );

        let source = video_texture_source_warning("Loop", Path::new("/tmp/loop.mp4"), "rejected");
        assert_eq!(source.code, VIDEO_TEXTURE_SOURCE_FAILED_CODE);
        let detail = source.detail.expect("source detail");
        assert_eq!(detail.domain, SceneDiagnosticDomain::VideoTexture);
        assert_eq!(detail.runtime_stage.as_deref(), Some("video-source-sync"));
    }
}
