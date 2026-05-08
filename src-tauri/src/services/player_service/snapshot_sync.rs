use tauri::AppHandle;

use crate::{
    models::WallpaperRecord,
    services::static_snapshot_service,
    store::AppState,
};

pub(super) fn sync_static_snapshot_for_active_runtime(
    app: &AppHandle,
    state: &AppState,
    record: &WallpaperRecord,
) -> Result<(), String> {
    static_snapshot_service::sync_after_active_wallpaper_change(app, state, record)?;
    Ok(())
}

pub(super) fn supports_apply_time_static_snapshot_generation(record: &WallpaperRecord) -> bool {
    matches!(
        record.wallpaper_type,
        crate::models::WallpaperType::Scene
            | crate::models::WallpaperType::Video
            | crate::models::WallpaperType::Web
    )
}

pub(super) fn snapshot_generation_inputs_match(
    current: &WallpaperRecord,
    candidate: &WallpaperRecord,
) -> bool {
    current.id == candidate.id
        && current.wallpaper_type == candidate.wallpaper_type
        && current.managed_path == candidate.managed_path
        && current.entry_path == candidate.entry_path
}
