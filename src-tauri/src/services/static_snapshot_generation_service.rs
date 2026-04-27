use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, ProtocolObject},
    DeclaredClass, MainThreadMarker, MainThreadOnly,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSBackingStoreType, NSColor, NSImage, NSWindow, NSWindowStyleMask};
#[cfg(target_os = "macos")]
use objc2_foundation::{
    NSDate, NSError, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURLRequest,
    NSURL,
};
#[cfg(target_os = "macos")]
use objc2_web_kit::{
    WKAudiovisualMediaTypes, WKNavigation, WKNavigationDelegate, WKSnapshotConfiguration,
    WKWebView, WKWebViewConfiguration,
};

use crate::{
    models::{WallpaperRecord, WallpaperType},
    services::{static_snapshot_service, web_runtime_service},
};

pub const STATIC_SNAPSHOT_FILE_NAME: &str = "snapshot.png";
const STATIC_SNAPSHOT_TEMP_FILE_NAME: &str = ".snapshot.png.tmp";
#[cfg(target_os = "macos")]
const WEB_SNAPSHOT_VIEW_WIDTH: f64 = 1920.0;
#[cfg(target_os = "macos")]
const WEB_SNAPSHOT_VIEW_HEIGHT: f64 = 1080.0;
#[cfg(target_os = "macos")]
const WEB_SNAPSHOT_LOAD_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(target_os = "macos")]
const WEB_SNAPSHOT_CAPTURE_TIMEOUT: Duration = Duration::from_secs(4);
#[cfg(target_os = "macos")]
const WEB_SNAPSHOT_SETTLE_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSnapshotGenerationOutcome {
    Generated { snapshot_path: PathBuf },
    Existing { snapshot_path: PathBuf },
    Unsupported { wallpaper_type: WallpaperType },
    MissingVideoSource { reason: String },
    MissingWebEntry { reason: String },
    Failed { reason: String },
}

pub fn ensure_static_snapshot_for_record(
    record: &mut WallpaperRecord,
) -> StaticSnapshotGenerationOutcome {
    generate_static_snapshot_for_record_with(
        record,
        false,
        render_video_snapshot_file,
        render_web_snapshot_file,
    )
}

pub fn regenerate_static_snapshot_for_record(
    record: &mut WallpaperRecord,
) -> StaticSnapshotGenerationOutcome {
    generate_static_snapshot_for_record_with(
        record,
        true,
        render_video_snapshot_file,
        render_web_snapshot_file,
    )
}

#[cfg(test)]
pub fn ensure_static_snapshot_for_record_with<RenderVideoSnapshot>(
    record: &mut WallpaperRecord,
    render_video_snapshot: RenderVideoSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    generate_static_snapshot_for_record_with(
        record,
        false,
        render_video_snapshot,
        |_entry, _output| {
            Err("web static snapshot generation test renderer was not provided".to_string())
        },
    )
}

#[cfg(test)]
pub fn regenerate_static_snapshot_for_record_with<RenderVideoSnapshot>(
    record: &mut WallpaperRecord,
    render_video_snapshot: RenderVideoSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    generate_static_snapshot_for_record_with(
        record,
        true,
        render_video_snapshot,
        |_entry, _output| {
            Err("web static snapshot generation test renderer was not provided".to_string())
        },
    )
}

#[cfg(test)]
pub fn ensure_static_snapshot_for_record_with_renderers<RenderVideoSnapshot, RenderWebSnapshot>(
    record: &mut WallpaperRecord,
    render_video_snapshot: RenderVideoSnapshot,
    render_web_snapshot: RenderWebSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
    RenderWebSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    generate_static_snapshot_for_record_with(
        record,
        false,
        render_video_snapshot,
        render_web_snapshot,
    )
}

#[cfg(test)]
pub fn regenerate_static_snapshot_for_record_with_renderers<
    RenderVideoSnapshot,
    RenderWebSnapshot,
>(
    record: &mut WallpaperRecord,
    render_video_snapshot: RenderVideoSnapshot,
    render_web_snapshot: RenderWebSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
    RenderWebSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    generate_static_snapshot_for_record_with(
        record,
        true,
        render_video_snapshot,
        render_web_snapshot,
    )
}

fn generate_static_snapshot_for_record_with<RenderVideoSnapshot, RenderWebSnapshot>(
    record: &mut WallpaperRecord,
    refresh_existing: bool,
    render_video_snapshot: RenderVideoSnapshot,
    render_web_snapshot: RenderWebSnapshot,
) -> StaticSnapshotGenerationOutcome
where
    RenderVideoSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
    RenderWebSnapshot: FnOnce(&Path, &Path) -> Result<(), String>,
{
    if !refresh_existing {
        if let Ok(snapshot_path) = static_snapshot_service::snapshot_for_record(record) {
            return StaticSnapshotGenerationOutcome::Existing { snapshot_path };
        }
    }

    record.last_snapshot_path = None;

    let source = match snapshot_source_for_record(record) {
        StaticSnapshotSource::Video(source_path) => SnapshotRenderPlan {
            source_path,
            render: Box::new(render_video_snapshot),
        },
        StaticSnapshotSource::Web(entry_path) => SnapshotRenderPlan {
            source_path: entry_path,
            render: Box::new(render_web_snapshot),
        },
        StaticSnapshotSource::MissingVideoSource { reason } => {
            return StaticSnapshotGenerationOutcome::MissingVideoSource { reason };
        }
        StaticSnapshotSource::MissingWebEntry { reason } => {
            return StaticSnapshotGenerationOutcome::MissingWebEntry { reason };
        }
        StaticSnapshotSource::Unsupported { wallpaper_type } => {
            return StaticSnapshotGenerationOutcome::Unsupported { wallpaper_type };
        }
    };

    let snapshot_path = static_snapshot_path_for_record(record);
    let temp_path = static_snapshot_temp_path_for_record(record);
    if let Err(error) = prepare_snapshot_output_path(&snapshot_path, &temp_path) {
        return StaticSnapshotGenerationOutcome::Failed { reason: error };
    }

    match (source.render)(&source.source_path, &temp_path)
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

struct SnapshotRenderPlan<'a> {
    source_path: PathBuf,
    render: Box<dyn FnOnce(&Path, &Path) -> Result<(), String> + 'a>,
}

enum StaticSnapshotSource {
    Video(PathBuf),
    Web(PathBuf),
    MissingVideoSource { reason: String },
    MissingWebEntry { reason: String },
    Unsupported { wallpaper_type: WallpaperType },
}

fn snapshot_source_for_record(record: &WallpaperRecord) -> StaticSnapshotSource {
    match record.wallpaper_type {
        WallpaperType::Video => match normalized_video_source_path(record.entry_path.as_deref()) {
            Some(source_path) => StaticSnapshotSource::Video(source_path),
            None => StaticSnapshotSource::MissingVideoSource {
                reason: "video snapshot generation requires an existing entry_path file"
                    .to_string(),
            },
        },
        WallpaperType::Web => match normalized_web_entry_path(record.entry_path.as_deref()) {
            Ok(entry_path) => StaticSnapshotSource::Web(entry_path),
            Err(reason) => StaticSnapshotSource::MissingWebEntry { reason },
        },
        _ => StaticSnapshotSource::Unsupported {
            wallpaper_type: record.wallpaper_type.clone(),
        },
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

fn normalized_web_entry_path(entry_path: Option<&str>) -> Result<PathBuf, String> {
    let path = entry_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "web snapshot generation requires an entry_path HTML file".to_string())?;
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(format!(
            "web snapshot generation entry HTML file is missing: {}",
            path.display()
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if !matches!(extension.as_deref(), Some("html") | Some("htm")) {
        return Err(format!(
            "web snapshot generation only supports HTML entry files: {}",
            path.display()
        ));
    }
    Ok(path)
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
            "static snapshot renderer did not create {}",
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

#[cfg(target_os = "macos")]
fn render_web_snapshot_file(entry_path: &Path, output_path: &Path) -> Result<(), String> {
    let entry = entry_path.to_str().ok_or_else(|| {
        format!(
            "web entry path is not valid UTF-8 for snapshot generation: {}",
            entry_path.display()
        )
    })?;
    let output = output_path.to_str().ok_or_else(|| {
        format!(
            "snapshot output path is not valid UTF-8 for web snapshot generation: {}",
            output_path.display()
        )
    })?;
    let runtime_url = web_runtime_service::get_web_runtime_url(entry)?;
    let (load_sender, load_receiver) = mpsc::channel();
    let host = create_web_snapshot_capture_host(runtime_url.clone(), load_sender)?;

    let result = (|| {
        wait_for_web_snapshot_result(
            load_receiver,
            WEB_SNAPSHOT_LOAD_TIMEOUT,
            "WKWebView navigation finish",
        )?;
        thread::sleep(WEB_SNAPSHOT_SETTLE_DELAY);

        let (capture_sender, capture_receiver) = mpsc::channel();
        capture_web_snapshot(&host, output.to_string(), capture_sender)?;
        wait_for_web_snapshot_result(
            capture_receiver,
            WEB_SNAPSHOT_CAPTURE_TIMEOUT,
            "WKWebView snapshot capture",
        )
    })();

    teardown_web_snapshot_capture_host(&host);
    result.map_err(|error| format!("web snapshot generation failed for {runtime_url}: {error}"))
}

#[cfg(not(target_os = "macos"))]
fn render_web_snapshot_file(entry_path: &Path, output_path: &Path) -> Result<(), String> {
    let _ = (entry_path, output_path);
    Err("web static snapshot generation is only implemented on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn wait_for_web_snapshot_result(
    receiver: Receiver<Result<(), String>>,
    timeout: Duration,
    stage: &str,
) -> Result<(), String> {
    if MainThreadMarker::new().is_some() {
        return wait_for_web_snapshot_result_on_main_thread(receiver, timeout, stage);
    }

    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("timed out waiting for {stage} after {timeout:?}"))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{stage} channel disconnected before completion"))
        }
    }
}

#[cfg(target_os = "macos")]
fn wait_for_web_snapshot_result_on_main_thread(
    receiver: Receiver<Result<(), String>>,
    timeout: Duration,
    stage: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match receiver.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(format!("{stage} channel disconnected before completion"));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!("timed out waiting for {stage} after {timeout:?}"));
        }

        let step = (deadline - now).min(Duration::from_millis(50));
        let until = NSDate::dateWithTimeIntervalSinceNow(step.as_secs_f64());
        NSRunLoop::currentRunLoop().runUntilDate(&until);
    }
}

#[cfg(target_os = "macos")]
struct WebSnapshotCaptureHost {
    host: MainThreadBound<WebSnapshotCaptureHostInner>,
}

#[cfg(target_os = "macos")]
fn create_web_snapshot_capture_host(
    runtime_url: String,
    load_sender: Sender<Result<(), String>>,
) -> Result<WebSnapshotCaptureHost, String> {
    if let Some(mtm) = MainThreadMarker::new() {
        let host = WebSnapshotCaptureHostInner::create(&runtime_url, load_sender, mtm)?;
        return Ok(WebSnapshotCaptureHost {
            host: MainThreadBound::new(host, mtm),
        });
    }

    let host = run_on_main(move |mtm| {
        WebSnapshotCaptureHostInner::create(&runtime_url, load_sender, mtm)
            .map(|host| MainThreadBound::new(host, mtm))
    })?;
    Ok(WebSnapshotCaptureHost { host })
}

#[cfg(target_os = "macos")]
fn capture_web_snapshot(
    host: &WebSnapshotCaptureHost,
    output_path: String,
    capture_sender: Sender<Result<(), String>>,
) -> Result<(), String> {
    if let Some(mtm) = MainThreadMarker::new() {
        let host = host.host.get(mtm);
        return host.capture_to_path(output_path, capture_sender, mtm);
    }

    run_on_main(move |mtm| {
        let host = host.host.get(mtm);
        host.capture_to_path(output_path, capture_sender, mtm)
    })
}

#[cfg(target_os = "macos")]
fn teardown_web_snapshot_capture_host(host: &WebSnapshotCaptureHost) {
    if let Some(mtm) = MainThreadMarker::new() {
        let host = host.host.get(mtm);
        host.teardown();
        return;
    }

    run_on_main(|mtm| {
        let host = host.host.get(mtm);
        host.teardown();
    });
}

#[cfg(target_os = "macos")]
struct WebSnapshotCaptureHostInner {
    window: Retained<NSWindow>,
    view: Retained<WKWebView>,
    delegate: Retained<WebSnapshotNavigationDelegate>,
}

#[cfg(target_os = "macos")]
struct WebSnapshotNavigationDelegateIvars {
    sender: Sender<Result<(), String>>,
}

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = WebSnapshotNavigationDelegateIvars]
    struct WebSnapshotNavigationDelegate;

    unsafe impl NSObjectProtocol for WebSnapshotNavigationDelegate {}

    unsafe impl WKNavigationDelegate for WebSnapshotNavigationDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn web_view_did_finish_navigation(
            this: &WebSnapshotNavigationDelegate,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
        ) {
            let _ = this.ivars().sender.send(Ok(()));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn web_view_did_fail_provisional_navigation(
            this: &WebSnapshotNavigationDelegate,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            let _ = this.ivars().sender.send(Err(format!(
                "WKWebView provisional navigation failed: {}",
                error.localizedDescription()
            )));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn web_view_did_fail_navigation(
            this: &WebSnapshotNavigationDelegate,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            let _ = this.ivars().sender.send(Err(format!(
                "WKWebView navigation failed: {}",
                error.localizedDescription()
            )));
        }
    }
);

#[cfg(target_os = "macos")]
impl WebSnapshotNavigationDelegate {
    fn new(sender: Sender<Result<(), String>>, mtm: MainThreadMarker) -> Retained<Self> {
        let delegate = mtm
            .alloc::<WebSnapshotNavigationDelegate>()
            .set_ivars(WebSnapshotNavigationDelegateIvars { sender });

        unsafe { msg_send![super(delegate), init] }
    }
}

#[cfg(target_os = "macos")]
impl WebSnapshotCaptureHostInner {
    fn create(
        runtime_url: &str,
        load_sender: Sender<Result<(), String>>,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe {
            configuration.setUpgradeKnownHostsToHTTPS(false);
            configuration.setAllowsAirPlayForMediaPlayback(false);
            configuration
                .setMediaTypesRequiringUserActionForPlayback(WKAudiovisualMediaTypes::None);
        }

        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(WEB_SNAPSHOT_VIEW_WIDTH, WEB_SNAPSHOT_VIEW_HEIGHT),
        );
        let view = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        unsafe {
            view.setUnderPageBackgroundColor(Some(&NSColor::clearColor()));
        }

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(
                    NSPoint::new(-100_000.0, -100_000.0),
                    NSSize::new(WEB_SNAPSHOT_VIEW_WIDTH, WEB_SNAPSHOT_VIEW_HEIGHT),
                ),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setReleasedWhenClosed(false);
            window.setOpaque(false);
            window.setBackgroundColor(Some(&NSColor::clearColor()));
            window.setIgnoresMouseEvents(true);
        }
        window.setContentView(Some(&view));
        window.orderBack(None);

        let delegate = WebSnapshotNavigationDelegate::new(load_sender, mtm);
        let delegate_proto = ProtocolObject::from_ref(&*delegate);
        unsafe {
            view.setNavigationDelegate(Some(delegate_proto));
        }

        let url_string = NSString::from_str(runtime_url);
        let url = NSURL::URLWithString(&url_string)
            .ok_or_else(|| format!("WKWebView rejected invalid web snapshot URL: {runtime_url}"))?;
        let request = NSURLRequest::requestWithURL(&url);
        unsafe {
            view.loadRequest(&request);
        }

        Ok(Self {
            window,
            view,
            delegate,
        })
    }

    fn capture_to_path(
        &self,
        output_path: String,
        capture_sender: Sender<Result<(), String>>,
        mtm: MainThreadMarker,
    ) -> Result<(), String> {
        let configuration = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            configuration.setRect(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(WEB_SNAPSHOT_VIEW_WIDTH, WEB_SNAPSHOT_VIEW_HEIGHT),
            ));
            configuration.setAfterScreenUpdates(true);
        }

        let block = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let result = encode_web_snapshot_image_to_png(image, error, &output_path);
            let _ = capture_sender.send(result);
        });
        unsafe {
            self.view
                .takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &block);
        }
        Ok(())
    }

    fn teardown(&self) {
        unsafe {
            self.view.stopLoading();
            self.view.setNavigationDelegate(None);
        }
        self.window.orderOut(None);
        self.window.setContentView(None);
        self.view.removeFromSuperview();
        let _ = &self.delegate;
    }
}

#[cfg(target_os = "macos")]
fn encode_web_snapshot_image_to_png(
    image: *mut NSImage,
    error: *mut NSError,
    output_path: &str,
) -> Result<(), String> {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey};
    use objc2_foundation::NSDictionary;

    unsafe {
        if !error.is_null() {
            return Err(format!(
                "WKWebView snapshot capture failed: {}",
                (*error).localizedDescription()
            ));
        }
        if image.is_null() {
            return Err("WKWebView snapshot capture returned no image".to_string());
        }

        let image = &*image;
        let tiff_data = image
            .TIFFRepresentation()
            .ok_or_else(|| "WKWebView snapshot image could not be encoded as TIFF".to_string())?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff_data)
            .ok_or_else(|| "WKWebView snapshot TIFF could not be decoded".to_string())?;
        let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
        let png_data = bitmap
            .representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
            .ok_or_else(|| "WKWebView snapshot image could not be encoded as PNG".to_string())?;
        let output = NSString::from_str(output_path);
        if png_data.writeToFile_atomically(&output, true) {
            Ok(())
        } else {
            Err(format!(
                "failed to write web static snapshot to {output_path}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::tempdir;

    use crate::models::{WallpaperRecord, WallpaperType};

    use super::{
        ensure_static_snapshot_for_record_with, ensure_static_snapshot_for_record_with_renderers,
        regenerate_static_snapshot_for_record_with,
        regenerate_static_snapshot_for_record_with_renderers, static_snapshot_path_for_record,
        StaticSnapshotGenerationOutcome, STATIC_SNAPSHOT_FILE_NAME, STATIC_SNAPSHOT_TEMP_FILE_NAME,
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
    fn web_generation_writes_managed_snapshot_and_registers_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let entry = managed.join("index.html");
        fs::write(&entry, "<html><body>web</body></html>").expect("web entry");
        let preview = managed.join("preview.png");
        fs::write(&preview, b"preview").expect("preview");
        let mut record = record(
            WallpaperType::Web,
            managed.display().to_string(),
            Some(entry.display().to_string()),
            Some(preview.display().to_string()),
        );

        let outcome = ensure_static_snapshot_for_record_with_renderers(
            &mut record,
            |_source_path, _output| {
                panic!("web snapshot generation must not invoke the video renderer")
            },
            |entry_path, output| {
                assert_eq!(entry_path, entry.as_path());
                assert_eq!(
                    output.file_name().and_then(|value| value.to_str()),
                    Some(STATIC_SNAPSHOT_TEMP_FILE_NAME)
                );
                fs::write(output, b"web-snapshot").map_err(|error| error.to_string())
            },
        );

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
        assert_ne!(
            record.last_snapshot_path.as_deref(),
            record.preview_path.as_deref()
        );
        assert_eq!(
            fs::read(&expected_snapshot).expect("snapshot"),
            b"web-snapshot"
        );
        assert!(!managed.join(STATIC_SNAPSHOT_TEMP_FILE_NAME).exists());
    }

    #[test]
    fn repeated_web_generation_overwrites_stable_snapshot_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let entry = managed.join("index.html");
        fs::write(&entry, "<html><body>web</body></html>").expect("web entry");
        let snapshot = managed.join(STATIC_SNAPSHOT_FILE_NAME);
        fs::write(&snapshot, b"old").expect("old snapshot");
        let mut record = record(
            WallpaperType::Web,
            managed.display().to_string(),
            Some(entry.display().to_string()),
            None,
        );
        record.last_snapshot_path = Some(snapshot.display().to_string());

        let outcome = regenerate_static_snapshot_for_record_with_renderers(
            &mut record,
            |_source_path, _output| {
                panic!("web snapshot generation must not invoke the video renderer")
            },
            |_entry_path, output| fs::write(output, b"new").map_err(|error| error.to_string()),
        );

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
    fn web_generation_failure_does_not_register_or_commit_snapshot_path() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let entry = managed.join("index.html");
        fs::write(&entry, "<html><body>web</body></html>").expect("web entry");
        let preview = managed.join("preview.png");
        fs::write(&preview, b"preview").expect("preview");
        let mut record = record(
            WallpaperType::Web,
            managed.display().to_string(),
            Some(entry.display().to_string()),
            Some(preview.display().to_string()),
        );

        let outcome = ensure_static_snapshot_for_record_with_renderers(
            &mut record,
            |_source_path, _output| {
                panic!("web snapshot generation must not invoke the video renderer")
            },
            |_entry_path, output| {
                fs::write(output, b"partial").map_err(|error| error.to_string())?;
                Err("simulated web generation failure".to_string())
            },
        );

        assert_eq!(
            outcome,
            StaticSnapshotGenerationOutcome::Failed {
                reason: "simulated web generation failure".to_string(),
            }
        );
        assert!(record.last_snapshot_path.is_none());
        assert_ne!(
            record.last_snapshot_path.as_deref(),
            record.preview_path.as_deref()
        );
        assert!(!static_snapshot_path_for_record(&record).exists());
        assert!(!managed.join(STATIC_SNAPSHOT_TEMP_FILE_NAME).exists());
    }

    #[test]
    fn missing_web_entry_does_not_register_preview_as_snapshot() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let preview = managed.join("preview.png");
        fs::write(&preview, b"preview").expect("preview");
        let mut record = record(
            WallpaperType::Web,
            managed.display().to_string(),
            Some(managed.join("missing.html").display().to_string()),
            Some(preview.display().to_string()),
        );

        let outcome = ensure_static_snapshot_for_record_with_renderers(
            &mut record,
            |_source_path, _output| panic!("missing web entry must not invoke the video renderer"),
            |_entry_path, _output| panic!("missing web entry must not invoke the web renderer"),
        );

        assert!(matches!(
            outcome,
            StaticSnapshotGenerationOutcome::MissingWebEntry { .. }
        ));
        assert!(record.last_snapshot_path.is_none());
        assert_ne!(
            record.last_snapshot_path.as_deref(),
            record.preview_path.as_deref()
        );
    }

    #[test]
    fn scene_generation_remains_explicitly_unsupported_without_preview_fallback() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let preview = managed.join("preview.png");
        fs::write(&preview, b"preview").expect("preview");

        let mut record = record(
            WallpaperType::Scene,
            managed.display().to_string(),
            None,
            Some(preview.display().to_string()),
        );

        let outcome = ensure_static_snapshot_for_record_with_renderers(
            &mut record,
            |_source, _output| {
                panic!("scene snapshot generation must not invoke video snapshot generation")
            },
            |_entry, _output| {
                panic!("scene snapshot generation must not invoke web snapshot generation")
            },
        );

        assert_eq!(
            outcome,
            StaticSnapshotGenerationOutcome::Unsupported {
                wallpaper_type: WallpaperType::Scene
            }
        );
        assert!(record.last_snapshot_path.is_none());
        assert_ne!(
            record.last_snapshot_path.as_deref(),
            record.preview_path.as_deref()
        );
    }
}
