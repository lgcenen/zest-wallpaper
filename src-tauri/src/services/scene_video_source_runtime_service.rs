use super::*;

#[cfg(target_os = "macos")]
pub(super) fn sync_video_sources(
    renderer: &mut NativeSceneMetalRenderer,
    visuals: &[SceneRenderVisualItem],
    paused: bool,
) -> Vec<NativeSceneWarning> {
    let current_paths = renderer
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
                if let Some(mut source) = renderer.video_sources.remove(&object_id) {
                    source.stop();
                }
            }
            SceneVideoTextureLifecycleAction::SetPaused { object_id, paused } => {
                if let Some(source) = renderer.video_sources.get_mut(&object_id) {
                    source.set_paused(paused);
                }
            }
            SceneVideoTextureLifecycleAction::Replace { source, paused, .. } => {
                if let Some(existing) = renderer.video_sources.get_mut(&source.object_id) {
                    existing.stop();
                }
                match NativeSceneVideoSource::new(
                    source.object_id,
                    source.asset_path.clone(),
                    paused,
                ) {
                    Ok(next_source) => {
                        renderer.video_sources.insert(source.object_id, next_source);
                    }
                    Err(error) => {
                        renderer.video_sources.remove(&source.object_id);
                        warnings.push(video_texture_source_warning(&source, error));
                    }
                }
            }
            SceneVideoTextureLifecycleAction::Create { source, paused } => {
                match NativeSceneVideoSource::new(source.object_id, source.asset_path.clone(), paused)
                {
                    Ok(next_source) => {
                        renderer.video_sources.insert(source.object_id, next_source);
                    }
                    Err(error) => warnings.push(video_texture_source_warning(&source, error)),
                }
            }
        }
    }

    warnings
}

#[cfg(target_os = "macos")]
pub(super) fn clear_video_sources(renderer: &mut NativeSceneMetalRenderer) {
    for source in renderer.video_sources.values_mut() {
        source.stop();
    }
    renderer.video_sources.clear();
    renderer.video_texture_cache.flush(0);
}

#[cfg(target_os = "macos")]
pub(super) fn video_texture_for_item(
    renderer: &mut NativeSceneMetalRenderer,
    item: &SceneRenderVisualItem,
) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let source = renderer.video_sources.get_mut(&item.object_id)?;
    match source.current_texture(&renderer.video_texture_cache, renderer.paused) {
        Ok(texture) => texture,
        Err(error) => {
            let detail = video_texture_frame_warning(&item.object_name, error);
            let _ = diagnostic_service::record_warning(
                &renderer.app,
                DIAGNOSTIC_SUBSYSTEM,
                &detail.code,
                detail.message.clone(),
                detail.detail_json(),
            );
            None
        }
    }
}
