#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::models::WallpaperRecord;

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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tempfile::tempdir;

    use crate::models::{WallpaperRecord, WallpaperType};

    use super::{
        plan_active_snapshot_sync, snapshot_for_record, StaticSnapshotResolutionIssue,
        StaticSnapshotSyncPlan,
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
}
