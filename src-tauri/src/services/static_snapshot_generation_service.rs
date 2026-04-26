use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    models::{WallpaperRecord, WallpaperType},
    services::static_snapshot_service,
};

pub const STATIC_SNAPSHOT_FILE_NAME: &str = "snapshot.png";
const STATIC_SNAPSHOT_TEMP_FILE_NAME: &str = ".snapshot.png.tmp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSnapshotGenerationOutcome {
    Generated { snapshot_path: PathBuf },
    Existing { snapshot_path: PathBuf },
    Unsupported { wallpaper_type: WallpaperType },
    MissingVideoSource { reason: String },
    Failed { reason: String },
}

pub fn ensure_static_snapshot_for_record(
    record: &mut WallpaperRecord,
) -> StaticSnapshotGenerationOutcome {
    generate_static_snapshot_for_record_with(record, false, render_video_snapshot_file)
}

pub fn regenerate_static_snapshot_for_record(
    record: &mut WallpaperRecord,
) -> StaticSnapshotGenerationOutcome {
    generate_static_snapshot_for_record_with(record, true, render_video_snapshot_file)
}

#[cfg(test)]
pub fn ensure_static_snapshot_for_record_with<RenderVideoSnapshot>(
    record: &mut WallpaperRecord,
    render_video_snapshot: RenderVideoSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    generate_static_snapshot_for_record_with(record, false, render_video_snapshot)
}

#[cfg(test)]
pub fn regenerate_static_snapshot_for_record_with<RenderVideoSnapshot>(
    record: &mut WallpaperRecord,
    render_video_snapshot: RenderVideoSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    generate_static_snapshot_for_record_with(record, true, render_video_snapshot)
}

fn generate_static_snapshot_for_record_with<RenderVideoSnapshot>(
    record: &mut WallpaperRecord,
    refresh_existing: bool,
    render_video_snapshot: RenderVideoSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    if !matches!(record.wallpaper_type, WallpaperType::Video) {
        return StaticSnapshotGenerationOutcome::Unsupported {
            wallpaper_type: record.wallpaper_type.clone(),
        };
    }

    if !refresh_existing {
        if let Ok(snapshot_path) = static_snapshot_service::snapshot_for_record(record) {
            return StaticSnapshotGenerationOutcome::Existing { snapshot_path };
        }
    }

    record.last_snapshot_path = None;

    let Some(source_path) = normalized_video_source_path(record.entry_path.as_deref()) else {
        return StaticSnapshotGenerationOutcome::MissingVideoSource {
            reason: "video snapshot generation requires an existing entry_path file".to_string(),
        };
    };

    let snapshot_path = static_snapshot_path_for_record(record);
    let temp_path = static_snapshot_temp_path_for_record(record);
    if let Err(error) = prepare_snapshot_output_path(&snapshot_path, &temp_path) {
        return StaticSnapshotGenerationOutcome::Failed { reason: error };
    }

    match render_video_snapshot(&source_path, &temp_path)
        .and_then(|()| commit_generated_snapshot(&temp_path, &snapshot_path))
    {
        Ok(()) => {
            record.last_snapshot_path = Some(snapshot_path.display().to_string());
            StaticSnapshotGenerationOutcome::Generated { snapshot_path }
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            StaticSnapshotGenerationOutcome::Failed { reason: error }
        }
    }
}

pub fn static_snapshot_path_for_record(record: &WallpaperRecord) -> PathBuf {
    Path::new(&record.managed_path).join(STATIC_SNAPSHOT_FILE_NAME)
}

fn static_snapshot_temp_path_for_record(record: &WallpaperRecord) -> PathBuf {
    Path::new(&record.managed_path).join(STATIC_SNAPSHOT_TEMP_FILE_NAME)
}

fn normalized_video_source_path(entry_path: Option<&str>) -> Option<PathBuf> {
    let path = entry_path
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

fn prepare_snapshot_output_path(snapshot_path: &Path, temp_path: &Path) -> Result<(), String> {
    let parent = snapshot_path.parent().ok_or_else(|| {
        format!(
            "static snapshot output path has no parent: {}",
            snapshot_path.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create static snapshot directory {}: {error}",
            parent.display()
        )
    })?;
    if temp_path.exists() {
        fs::remove_file(temp_path).map_err(|error| {
            format!(
                "failed to remove stale static snapshot temp file {}: {error}",
                temp_path.display()
            )
        })?;
    }
    Ok(())
}

fn commit_generated_snapshot(temp_path: &Path, snapshot_path: &Path) -> Result<(), String> {
    if !temp_path.is_file() {
        return Err(format!(
            "video snapshot renderer did not create {}",
            temp_path.display()
        ));
    }
    fs::rename(temp_path, snapshot_path).map_err(|error| {
        format!(
            "failed to commit static snapshot {} from {}: {error}",
            snapshot_path.display(),
            temp_path.display()
        )
    })
}

#[cfg(target_os = "macos")]
fn render_video_snapshot_file(source_path: &Path, output_path: &Path) -> Result<(), String> {
    use std::ptr;

    use objc2::{runtime::AnyObject, AnyThread};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey};
    use objc2_av_foundation::{AVAsset, AVAssetImageGenerator};
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSDictionary, NSString, NSURL};

    let source = source_path.to_str().ok_or_else(|| {
        format!(
            "video source path is not valid UTF-8 for snapshot generation: {}",
            source_path.display()
        )
    })?;
    let output = output_path.to_str().ok_or_else(|| {
        format!(
            "snapshot output path is not valid UTF-8 for snapshot generation: {}",
            output_path.display()
        )
    })?;

    let url = NSURL::from_file_path(source)
        .ok_or_else(|| format!("AVFoundation rejected video source path: {source}"))?;
    let asset = unsafe { AVAsset::assetWithURL(&url) };
    let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
    unsafe {
        generator.setAppliesPreferredTrackTransform(true);
        generator.setRequestedTimeToleranceBefore(CMTime::new(0, 600));
        generator.setRequestedTimeToleranceAfter(CMTime::new(0, 600));
    }

    #[allow(deprecated)]
    let image = unsafe {
        generator.copyCGImageAtTime_actualTime_error(CMTime::new(0, 600), ptr::null_mut())
    }
    .map_err(|error| {
        format!(
            "AVAssetImageGenerator failed to extract a static snapshot from {}: {}",
            source_path.display(),
            error.localizedDescription()
        )
    })?;

    let bitmap = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or_else(|| {
        format!(
            "failed to encode video static snapshot as PNG for {}",
            source_path.display()
        )
    })?;
    let output = NSString::from_str(output);
    if data.writeToFile_atomically(&output, true) {
        Ok(())
    } else {
        Err(format!(
            "failed to write video static snapshot to {}",
            output_path.display()
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn render_video_snapshot_file(source_path: &Path, output_path: &Path) -> Result<(), String> {
    let _ = (source_path, output_path);
    Err("video static snapshot generation is only implemented on macOS".to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::models::{WallpaperRecord, WallpaperType};

    use super::{
        ensure_static_snapshot_for_record_with, regenerate_static_snapshot_for_record_with,
        static_snapshot_path_for_record, StaticSnapshotGenerationOutcome,
        STATIC_SNAPSHOT_FILE_NAME, STATIC_SNAPSHOT_TEMP_FILE_NAME,
    };

    fn record(
        wallpaper_type: WallpaperType,
        managed_path: String,
        entry_path: Option<String>,
        preview_path: Option<String>,
    ) -> WallpaperRecord {
        WallpaperRecord {
            id: "snapshot-demo".to_string(),
            title: "Snapshot Demo".to_string(),
            wallpaper_type,
            source_path: managed_path.clone(),
            managed_path,
            preview_path,
            entry_path,
            last_snapshot_path: None,
            property_schema: Vec::new(),
            property_sections: Vec::new(),
            scene_cache: None,
            scene_manifest: None,
            scene_manifest_version: None,
            scene_manifest_dirty: false,
            imported_at: Utc::now(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn video_generation_writes_managed_snapshot_and_registers_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let source = managed.join("source.mp4");
        fs::write(&source, b"video").expect("source video");
        let mut record = record(
            WallpaperType::Video,
            managed.display().to_string(),
            Some(source.display().to_string()),
            Some(managed.join("preview.png").display().to_string()),
        );

        let outcome = ensure_static_snapshot_for_record_with(&mut record, |source_path, output| {
            assert_eq!(source_path, source.as_path());
            assert_eq!(
                output.file_name().and_then(|value| value.to_str()),
                Some(STATIC_SNAPSHOT_TEMP_FILE_NAME)
            );
            fs::write(output, b"snapshot").map_err(|error| error.to_string())
        });

        let expected_snapshot = managed.join(STATIC_SNAPSHOT_FILE_NAME);
        assert_eq!(
            outcome,
            StaticSnapshotGenerationOutcome::Generated {
                snapshot_path: expected_snapshot.clone(),
            }
        );
        assert_eq!(
            record.last_snapshot_path.as_deref(),
            Some(expected_snapshot.to_string_lossy().as_ref())
        );
        assert_eq!(fs::read(&expected_snapshot).expect("snapshot"), b"snapshot");
        assert!(!managed.join(STATIC_SNAPSHOT_TEMP_FILE_NAME).exists());
    }

    #[test]
    fn repeated_video_generation_overwrites_stable_snapshot_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let source = managed.join("source.mp4");
        fs::write(&source, b"video").expect("source video");
        let snapshot = managed.join(STATIC_SNAPSHOT_FILE_NAME);
        fs::write(&snapshot, b"old").expect("old snapshot");
        let mut record = record(
            WallpaperType::Video,
            managed.display().to_string(),
            Some(source.display().to_string()),
            None,
        );
        record.last_snapshot_path = Some(snapshot.display().to_string());

        let outcome =
            regenerate_static_snapshot_for_record_with(&mut record, |_source_path, output| {
                fs::write(output, b"new").map_err(|error| error.to_string())
            });

        assert_eq!(
            outcome,
            StaticSnapshotGenerationOutcome::Generated {
                snapshot_path: snapshot.clone(),
            }
        );
        assert_eq!(fs::read(&snapshot).expect("snapshot"), b"new");
        assert_eq!(
            record.last_snapshot_path.as_deref(),
            Some(snapshot.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn generation_failure_does_not_register_or_commit_snapshot_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let source = managed.join("source.mp4");
        fs::write(&source, b"video").expect("source video");
        let mut record = record(
            WallpaperType::Video,
            managed.display().to_string(),
            Some(source.display().to_string()),
            None,
        );

        let outcome =
            ensure_static_snapshot_for_record_with(&mut record, |_source_path, output| {
                fs::write(output, b"partial").map_err(|error| error.to_string())?;
                Err("simulated generation failure".to_string())
            });

        assert_eq!(
            outcome,
            StaticSnapshotGenerationOutcome::Failed {
                reason: "simulated generation failure".to_string(),
            }
        );
        assert!(record.last_snapshot_path.is_none());
        assert!(!static_snapshot_path_for_record(&record).exists());
        assert!(!managed.join(STATIC_SNAPSHOT_TEMP_FILE_NAME).exists());
    }

    #[test]
    fn web_and_scene_generation_are_explicitly_unsupported_without_preview_fallback() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let preview = managed.join("preview.png");
        fs::write(&preview, b"preview").expect("preview");

        for wallpaper_type in [WallpaperType::Web, WallpaperType::Scene] {
            let mut record = record(
                wallpaper_type.clone(),
                managed.display().to_string(),
                None,
                Some(preview.display().to_string()),
            );

            let outcome =
                ensure_static_snapshot_for_record_with(&mut record, |_source, _output| {
                    panic!("unsupported wallpaper types must not invoke video snapshot generation")
                });

            assert_eq!(
                outcome,
                StaticSnapshotGenerationOutcome::Unsupported { wallpaper_type }
            );
            assert!(record.last_snapshot_path.is_none());
            assert_ne!(
                record.last_snapshot_path.as_deref(),
                record.preview_path.as_deref()
            );
        }
    }
}
