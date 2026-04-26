use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::AppHandle;

use crate::{
    models::WallpaperRecord,
    services::diagnostic_service,
    store::{AppState, StaticSnapshotSyncState},
};

pub const DIAGNOSTIC_SUBSYSTEM: &str = "static-snapshot-sync";
const SNAPSHOT_UNAVAILABLE_CODE: &str = "snapshot-unavailable";
const APPLY_FAILED_CODE: &str = "apply-failed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticSnapshotResolutionIssue {
    SnapshotNotRecorded,
    SnapshotPathNotAbsolute,
    SnapshotPathMissing,
    UnsupportedSnapshotFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticSnapshotResolutionError {
    pub record_id: String,
    pub issue: StaticSnapshotResolutionIssue,
    pub snapshot_path: Option<String>,
}

impl StaticSnapshotResolutionError {
    pub fn diagnostic_summary(&self) -> &'static str {
        match self.issue {
            StaticSnapshotResolutionIssue::SnapshotNotRecorded => {
                "Active wallpaper has no recorded static snapshot."
            }
            StaticSnapshotResolutionIssue::SnapshotPathNotAbsolute => {
                "Recorded static snapshot path is not absolute."
            }
            StaticSnapshotResolutionIssue::SnapshotPathMissing => {
                "Recorded static snapshot path is unavailable."
            }
            StaticSnapshotResolutionIssue::UnsupportedSnapshotFormat => {
                "Recorded static snapshot format cannot be applied as a system wallpaper."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSnapshotSyncPlan {
    ApplySnapshot {
        record_id: String,
        snapshot_path: PathBuf,
    },
    MissingSnapshot {
        error: StaticSnapshotResolutionError,
    },
    InactiveRecord {
        record_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSnapshotSyncOutcome {
    Applied {
        record_id: String,
        snapshot_path: PathBuf,
        generation: u64,
    },
    MissingSnapshot {
        error: StaticSnapshotResolutionError,
        generation: u64,
    },
    ApplyFailed {
        record_id: String,
        snapshot_path: PathBuf,
        error: String,
        generation: u64,
    },
    InactiveRecord {
        record_id: String,
        generation: u64,
    },
    Cleared {
        generation: u64,
    },
}

pub fn sync_after_active_wallpaper_change(
    app: &AppHandle,
    state: &AppState,
    record: &WallpaperRecord,
) -> Result<StaticSnapshotSyncOutcome, String> {
    let active_record_id = state
        .player
        .lock()
        .map_err(|error| error.to_string())?
        .active_id
        .clone();
    let outcome = {
        let mut sync_state = state
            .static_snapshot_sync
            .lock()
            .map_err(|error| error.to_string())?;
        sync_active_snapshot_transaction(
            &mut sync_state,
            active_record_id.as_deref(),
            record,
            apply_system_wallpaper,
        )
    };
    publish_static_snapshot_diagnostic(app, &outcome)?;
    Ok(outcome)
}

pub fn clear_active_snapshot_sync(
    app: &AppHandle,
    state: &AppState,
) -> Result<StaticSnapshotSyncOutcome, String> {
    let outcome = {
        let mut sync_state = state
            .static_snapshot_sync
            .lock()
            .map_err(|error| error.to_string())?;
        clear_active_snapshot_sync_state(&mut sync_state)
    };
    publish_static_snapshot_diagnostic(app, &outcome)?;
    Ok(outcome)
}

pub fn plan_active_snapshot_sync(
    active_record_id: Option<&str>,
    record: &WallpaperRecord,
) -> StaticSnapshotSyncPlan {
    if active_record_id != Some(record.id.as_str()) {
        return StaticSnapshotSyncPlan::InactiveRecord {
            record_id: record.id.clone(),
        };
    }

    match snapshot_for_record(record) {
        Ok(snapshot_path) => StaticSnapshotSyncPlan::ApplySnapshot {
            record_id: record.id.clone(),
            snapshot_path,
        },
        Err(error) => StaticSnapshotSyncPlan::MissingSnapshot { error },
    }
}

pub fn sync_active_snapshot_transaction<ApplySystemWallpaper>(
    sync_state: &mut StaticSnapshotSyncState,
    active_record_id: Option<&str>,
    record: &WallpaperRecord,
    apply_system_wallpaper: ApplySystemWallpaper,
) -> StaticSnapshotSyncOutcome
where
    ApplySystemWallpaper: FnOnce(&Path) -> Result<(), String>,
{
    let generation = next_generation(sync_state);
    sync_state.active_record_id = active_record_id.map(ToString::to_string);

    let plan = plan_active_snapshot_sync(active_record_id, record);
    match plan {
        StaticSnapshotSyncPlan::ApplySnapshot {
            record_id,
            snapshot_path,
        } => match apply_system_wallpaper(&snapshot_path) {
            Ok(()) => {
                sync_state.last_applied_snapshot_path = Some(snapshot_path.display().to_string());
                sync_state.last_failure_code = None;
                StaticSnapshotSyncOutcome::Applied {
                    record_id,
                    snapshot_path,
                    generation,
                }
            }
            Err(error) => {
                sync_state.last_failure_code = Some(APPLY_FAILED_CODE.to_string());
                StaticSnapshotSyncOutcome::ApplyFailed {
                    record_id,
                    snapshot_path,
                    error,
                    generation,
                }
            }
        },
        StaticSnapshotSyncPlan::MissingSnapshot { error } => {
            sync_state.last_failure_code = Some(SNAPSHOT_UNAVAILABLE_CODE.to_string());
            StaticSnapshotSyncOutcome::MissingSnapshot { error, generation }
        }
        StaticSnapshotSyncPlan::InactiveRecord { record_id } => {
            StaticSnapshotSyncOutcome::InactiveRecord {
                record_id,
                generation,
            }
        }
    }
}

pub fn clear_active_snapshot_sync_state(
    sync_state: &mut StaticSnapshotSyncState,
) -> StaticSnapshotSyncOutcome {
    let generation = next_generation(sync_state);
    sync_state.active_record_id = None;
    sync_state.last_failure_code = None;
    StaticSnapshotSyncOutcome::Cleared { generation }
}

pub fn snapshot_for_record(
    record: &WallpaperRecord,
) -> Result<PathBuf, StaticSnapshotResolutionError> {
    let Some(snapshot_path) = normalized_snapshot_path(record.last_snapshot_path.as_deref()) else {
        return Err(resolution_error(
            record,
            StaticSnapshotResolutionIssue::SnapshotNotRecorded,
            None,
        ));
    };

    let snapshot_path_text = Some(snapshot_path.display().to_string());
    if !snapshot_path.is_absolute() {
        return Err(resolution_error(
            record,
            StaticSnapshotResolutionIssue::SnapshotPathNotAbsolute,
            snapshot_path_text,
        ));
    }

    if !snapshot_path.is_file() {
        return Err(resolution_error(
            record,
            StaticSnapshotResolutionIssue::SnapshotPathMissing,
            snapshot_path_text,
        ));
    }

    if !is_supported_static_snapshot_path(&snapshot_path) {
        return Err(resolution_error(
            record,
            StaticSnapshotResolutionIssue::UnsupportedSnapshotFormat,
            snapshot_path_text,
        ));
    }

    Ok(snapshot_path)
}

fn normalized_snapshot_path(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn resolution_error(
    record: &WallpaperRecord,
    issue: StaticSnapshotResolutionIssue,
    snapshot_path: Option<String>,
) -> StaticSnapshotResolutionError {
    StaticSnapshotResolutionError {
        record_id: record.id.clone(),
        issue,
        snapshot_path,
    }
}

fn is_supported_static_snapshot_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "heic" | "tif" | "tiff" | "bmp"
            )
        })
        .unwrap_or(false)
}

fn next_generation(sync_state: &mut StaticSnapshotSyncState) -> u64 {
    sync_state.generation = sync_state.generation.saturating_add(1);
    sync_state.generation
}

fn publish_static_snapshot_diagnostic(
    app: &AppHandle,
    outcome: &StaticSnapshotSyncOutcome,
) -> Result<(), String> {
    match outcome {
        StaticSnapshotSyncOutcome::Applied { .. } | StaticSnapshotSyncOutcome::Cleared { .. } => {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, APPLY_FAILED_CODE);
            let _ = diagnostic_service::clear_diagnostic(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                SNAPSHOT_UNAVAILABLE_CODE,
            );
        }
        StaticSnapshotSyncOutcome::MissingSnapshot { error, generation } => {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, APPLY_FAILED_CODE);
            diagnostic_service::record_warning(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                SNAPSHOT_UNAVAILABLE_CODE,
                error.diagnostic_summary(),
                Some(
                    json!({
                        "recordId": error.record_id,
                        "generation": generation,
                        "issue": format!("{:?}", error.issue),
                        "snapshotPath": error.snapshot_path,
                        "behavior": "kept-current-system-wallpaper"
                    })
                    .to_string(),
                ),
            )?;
        }
        StaticSnapshotSyncOutcome::ApplyFailed {
            record_id,
            snapshot_path,
            error,
            generation,
        } => {
            let _ = diagnostic_service::clear_diagnostic(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                SNAPSHOT_UNAVAILABLE_CODE,
            );
            diagnostic_service::record_error(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                APPLY_FAILED_CODE,
                "Failed to apply the active wallpaper static snapshot.",
                Some(
                    json!({
                        "recordId": record_id,
                        "generation": generation,
                        "snapshotPath": snapshot_path,
                        "error": error,
                        "behavior": "kept-current-system-wallpaper"
                    })
                    .to_string(),
                ),
            )?;
        }
        StaticSnapshotSyncOutcome::InactiveRecord { .. } => {}
    }
    Ok(())
}

fn apply_system_wallpaper(snapshot_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let escaped_path = snapshot_path
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let script = format!(
            r#"tell application "System Events"
set picture of every desktop to "{escaped_path}"
end tell"#
        );
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|error| {
                format!(
                    "failed to launch macOS wallpaper script for {}: {error}",
                    snapshot_path.display()
                )
            })?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(if stderr.is_empty() {
                format!(
                    "macOS wallpaper script failed for {}",
                    snapshot_path.display()
                )
            } else {
                format!(
                    "macOS wallpaper script failed for {}: {stderr}",
                    snapshot_path.display()
                )
            })
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = snapshot_path;
        Err("system wallpaper sync is only implemented on macOS".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, path::PathBuf};

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::models::{WallpaperRecord, WallpaperType};
    use crate::store::StaticSnapshotSyncState;

    use super::{
        clear_active_snapshot_sync_state, plan_active_snapshot_sync, snapshot_for_record,
        sync_active_snapshot_transaction, StaticSnapshotResolutionIssue, StaticSnapshotSyncOutcome,
        StaticSnapshotSyncPlan, APPLY_FAILED_CODE, SNAPSHOT_UNAVAILABLE_CODE,
    };

    fn record_with_paths(
        snapshot_path: Option<String>,
        preview_path: Option<String>,
    ) -> WallpaperRecord {
        WallpaperRecord {
            id: "active-demo".to_string(),
            title: "Active Demo".to_string(),
            wallpaper_type: WallpaperType::Video,
            source_path: "/portable/source".to_string(),
            managed_path: "/portable/managed".to_string(),
            preview_path,
            entry_path: Some("/portable/managed/clip.mp4".to_string()),
            last_snapshot_path: snapshot_path,
            property_schema: vec![],
            property_sections: vec![],
            scene_cache: None,
            scene_manifest: None,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: vec![],
        }
    }

    #[test]
    fn snapshot_for_record_uses_recorded_cached_snapshot() {
        let temp = tempdir().expect("temp dir");
        let snapshot = temp.path().join("active-demo.png");
        let preview = temp.path().join("preview.png");
        std::fs::write(&snapshot, b"snapshot").expect("snapshot file");
        std::fs::write(&preview, b"preview").expect("preview file");

        let record = record_with_paths(
            Some(snapshot.display().to_string()),
            Some(preview.display().to_string()),
        );

        assert_eq!(snapshot_for_record(&record).expect("snapshot"), snapshot);
    }

    #[test]
    fn snapshot_for_record_never_falls_back_to_preview_path() {
        let temp = tempdir().expect("temp dir");
        let preview = temp.path().join("preview.png");
        std::fs::write(&preview, b"preview").expect("preview file");

        let record = record_with_paths(None, Some(preview.display().to_string()));
        let error = snapshot_for_record(&record).expect_err("snapshot should be missing");
        let preview_text = preview.to_string_lossy().to_string();

        assert_eq!(
            error.issue,
            StaticSnapshotResolutionIssue::SnapshotNotRecorded
        );
        assert_ne!(error.snapshot_path.as_deref(), Some(preview_text.as_str()));
    }

    #[test]
    fn missing_cached_snapshot_keeps_system_wallpaper_unchanged() {
        let temp = tempdir().expect("temp dir");
        let preview = temp.path().join("preview.png");
        let missing_snapshot = temp.path().join("active-demo.png");
        std::fs::write(&preview, b"preview").expect("preview file");

        let record = record_with_paths(
            Some(missing_snapshot.display().to_string()),
            Some(preview.display().to_string()),
        );
        let plan = plan_active_snapshot_sync(Some("active-demo"), &record);

        match plan {
            StaticSnapshotSyncPlan::MissingSnapshot { error } => {
                assert_eq!(
                    error.issue,
                    StaticSnapshotResolutionIssue::SnapshotPathMissing
                );
                assert_eq!(
                    error.diagnostic_summary(),
                    "Recorded static snapshot path is unavailable."
                );
            }
            _ => panic!("expected a missing snapshot plan"),
        }
    }

    #[test]
    fn snapshot_sync_is_scoped_to_the_current_active_wallpaper() {
        let temp = tempdir().expect("temp dir");
        let snapshot = temp.path().join("active-demo.png");
        std::fs::write(&snapshot, b"snapshot").expect("snapshot file");
        let record = record_with_paths(Some(snapshot.display().to_string()), None);

        assert_eq!(
            plan_active_snapshot_sync(Some("other-wallpaper"), &record),
            StaticSnapshotSyncPlan::InactiveRecord {
                record_id: "active-demo".to_string()
            }
        );
    }

    #[test]
    fn new_apply_success_syncs_the_active_wallpaper_snapshot() {
        let temp = tempdir().expect("temp dir");
        let snapshot = temp.path().join("active-demo.jpg");
        std::fs::write(&snapshot, b"snapshot").expect("snapshot file");
        let record = record_with_paths(Some(snapshot.display().to_string()), None);
        let applied_paths = RefCell::new(Vec::<PathBuf>::new());
        let mut sync_state = StaticSnapshotSyncState::default();

        let outcome = sync_active_snapshot_transaction(
            &mut sync_state,
            Some("active-demo"),
            &record,
            |path| {
                applied_paths.borrow_mut().push(path.to_path_buf());
                Ok(())
            },
        );

        assert_eq!(
            outcome,
            StaticSnapshotSyncOutcome::Applied {
                record_id: "active-demo".to_string(),
                snapshot_path: snapshot.clone(),
                generation: 1,
            }
        );
        assert_eq!(applied_paths.into_inner(), vec![snapshot.clone()]);
        assert_eq!(
            sync_state.last_applied_snapshot_path.as_deref(),
            Some(snapshot.to_string_lossy().as_ref())
        );
        assert!(sync_state.last_failure_code.is_none());
    }

    #[test]
    fn stale_reconcile_does_not_overwrite_newer_active_snapshot_state() {
        let temp = tempdir().expect("temp dir");
        let stale_snapshot = temp.path().join("stale.png");
        let current_snapshot = temp.path().join("current.png");
        std::fs::write(&stale_snapshot, b"stale").expect("stale snapshot");
        std::fs::write(&current_snapshot, b"current").expect("current snapshot");
        let stale_record = record_with_paths(Some(stale_snapshot.display().to_string()), None);
        let applied_paths = RefCell::new(Vec::<PathBuf>::new());
        let mut sync_state = StaticSnapshotSyncState {
            active_record_id: Some("current-wallpaper".to_string()),
            generation: 7,
            last_applied_snapshot_path: Some(current_snapshot.display().to_string()),
            last_failure_code: None,
        };

        let outcome = sync_active_snapshot_transaction(
            &mut sync_state,
            Some("current-wallpaper"),
            &stale_record,
            |path| {
                applied_paths.borrow_mut().push(path.to_path_buf());
                Ok(())
            },
        );

        assert_eq!(
            outcome,
            StaticSnapshotSyncOutcome::InactiveRecord {
                record_id: "active-demo".to_string(),
                generation: 8,
            }
        );
        assert!(applied_paths.into_inner().is_empty());
        assert_eq!(
            sync_state.last_applied_snapshot_path.as_deref(),
            Some(current_snapshot.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn startup_restore_uses_the_restored_active_wallpaper_snapshot() {
        let temp = tempdir().expect("temp dir");
        let snapshot = temp.path().join("restored.tiff");
        std::fs::write(&snapshot, b"snapshot").expect("snapshot file");
        let record = record_with_paths(Some(snapshot.display().to_string()), None);
        let mut sync_state = StaticSnapshotSyncState::default();

        let outcome =
            sync_active_snapshot_transaction(&mut sync_state, Some("active-demo"), &record, |_| {
                Ok(())
            });

        assert!(matches!(
            outcome,
            StaticSnapshotSyncOutcome::Applied {
                record_id,
                generation: 1,
                ..
            } if record_id == "active-demo"
        ));
        assert_eq!(sync_state.active_record_id.as_deref(), Some("active-demo"));
        assert_eq!(
            sync_state.last_applied_snapshot_path.as_deref(),
            Some(snapshot.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn snapshot_apply_failure_keeps_active_state_and_records_failure_boundary() {
        let temp = tempdir().expect("temp dir");
        let snapshot = temp.path().join("active-demo.heic");
        std::fs::write(&snapshot, b"snapshot").expect("snapshot file");
        let record = record_with_paths(Some(snapshot.display().to_string()), None);
        let mut sync_state = StaticSnapshotSyncState::default();

        let outcome =
            sync_active_snapshot_transaction(&mut sync_state, Some("active-demo"), &record, |_| {
                Err("simulated system wallpaper failure".to_string())
            });

        assert!(matches!(
            outcome,
            StaticSnapshotSyncOutcome::ApplyFailed {
                record_id,
                error,
                generation: 1,
                ..
            } if record_id == "active-demo" && error == "simulated system wallpaper failure"
        ));
        assert_eq!(sync_state.active_record_id.as_deref(), Some("active-demo"));
        assert!(sync_state.last_applied_snapshot_path.is_none());
        assert_eq!(
            sync_state.last_failure_code.as_deref(),
            Some(APPLY_FAILED_CODE)
        );
    }

    #[test]
    fn missing_snapshot_records_diagnostic_boundary_without_preview_fallback() {
        let temp = tempdir().expect("temp dir");
        let preview = temp.path().join("preview.png");
        std::fs::write(&preview, b"preview").expect("preview file");
        let record = record_with_paths(None, Some(preview.display().to_string()));
        let mut sync_state = StaticSnapshotSyncState::default();

        let outcome =
            sync_active_snapshot_transaction(&mut sync_state, Some("active-demo"), &record, |_| {
                panic!("missing snapshot must not apply preview")
            });

        assert!(matches!(
            outcome,
            StaticSnapshotSyncOutcome::MissingSnapshot {
                error,
                generation: 1,
            } if error.issue == StaticSnapshotResolutionIssue::SnapshotNotRecorded
        ));
        assert!(sync_state.last_applied_snapshot_path.is_none());
        assert_eq!(
            sync_state.last_failure_code.as_deref(),
            Some(SNAPSHOT_UNAVAILABLE_CODE)
        );
    }

    #[test]
    fn clearing_active_wallpaper_clears_sync_target_without_reapplying_snapshot() {
        let mut sync_state = StaticSnapshotSyncState {
            active_record_id: Some("active-demo".to_string()),
            generation: 3,
            last_applied_snapshot_path: Some("/tmp/active-demo.png".to_string()),
            last_failure_code: Some(SNAPSHOT_UNAVAILABLE_CODE.to_string()),
        };

        let outcome = clear_active_snapshot_sync_state(&mut sync_state);

        assert_eq!(
            outcome,
            StaticSnapshotSyncOutcome::Cleared { generation: 4 }
        );
        assert!(sync_state.active_record_id.is_none());
        assert!(sync_state.last_failure_code.is_none());
        assert_eq!(
            sync_state.last_applied_snapshot_path.as_deref(),
            Some("/tmp/active-demo.png")
        );
    }
}
