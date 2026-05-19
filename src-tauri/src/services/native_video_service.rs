use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    path::Path,
    sync::Mutex,
};

use tauri::{AppHandle, Manager};

use crate::{
    models::{WallpaperRuntime, WallpaperRuntimeRecord},
    services::{
        diagnostic_service, runtime_audio_settings_service::normalize_output_volume_percent,
        window_service,
    },
};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::{MainThreadMarker, MainThreadOnly};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
#[cfg(target_os = "macos")]
use objc2_av_foundation::{
    AVLayerVideoGravityResizeAspectFill, AVPlayerItem, AVPlayerLooper, AVQueuePlayer,
};
#[cfg(target_os = "macos")]
use objc2_av_kit::{AVPlayerView, AVPlayerViewControlsStyle};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSSize, NSURL};

const DIAGNOSTIC_SUBSYSTEM: &str = "native-video";
const MISSING_SOURCE_CODE: &str = "missing-source";
const SYNC_FAILED_CODE: &str = "sync-failed";

pub struct NativeVideoServiceState {
    runtime: Mutex<NativeVideoRuntime>,
    output_volume: Mutex<f64>,
}

impl Default for NativeVideoServiceState {
    fn default() -> Self {
        Self {
            runtime: Mutex::new(NativeVideoRuntime::default()),
            output_volume: Mutex::new(1.0),
        }
    }
}

#[derive(Default)]
struct NativeVideoRuntime {
    session: Option<NativeVideoSessionHandle>,
    views: BTreeMap<String, NativeVideoViewHandle>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NativeVideoRuntimeSnapshot {
    source_path: Option<String>,
    paused: bool,
    looping_enabled: bool,
    labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VideoPlaybackSpec {
    source_path: String,
    paused: bool,
    looping_enabled: bool,
    window_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeVideoRuntimePlan {
    session: SessionPlan,
    ensure_labels: Vec<String>,
    remove_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionPlan {
    Keep,
    Stop,
    Start {
        source_path: String,
        paused: bool,
        looping_enabled: bool,
    },
    Replace {
        source_path: String,
        paused: bool,
        looping_enabled: bool,
    },
    UpdatePause {
        paused: bool,
    },
}

pub fn sync_native_video_playback(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<(), String> {
    let result = (|| {
        let Some(state) = app.try_state::<NativeVideoServiceState>() else {
            return Ok(());
        };

        let spec = desired_video_playback_spec(app, runtime_record, paused)?;
        let output_volume = *state
            .output_volume
            .lock()
            .map_err(|error| error.to_string())?;
        let mut runtime = state.runtime.lock().map_err(|error| error.to_string())?;
        let plan = plan_native_video_runtime(&runtime.snapshot(), spec.as_ref());
        apply_runtime_plan(&mut runtime, app, plan, output_volume)
    })();
    match &result {
        Ok(()) => {
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, SYNC_FAILED_CODE);
        }
        Err(error) => {
            let _ = diagnostic_service::record_error(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                SYNC_FAILED_CODE,
                "Native video runtime failed to sync.",
                Some(error.clone()),
            );
        }
    }
    result
}

pub fn set_native_video_output_volume(app: &AppHandle, volume: f64) -> Result<(), String> {
    let normalized = normalize_output_volume_percent(volume);
    let Some(state) = app.try_state::<NativeVideoServiceState>() else {
        return Ok(());
    };

    {
        let mut current = state
            .output_volume
            .lock()
            .map_err(|error| error.to_string())?;
        *current = normalized;
    }

    let runtime = state.runtime.lock().map_err(|error| error.to_string())?;
    if let Some(session) = runtime.session.as_ref() {
        session.set_output_volume(normalized);
    }

    Ok(())
}

fn desired_video_playback_spec(
    app: &AppHandle,
    runtime_record: Option<&WallpaperRuntimeRecord>,
    paused: bool,
) -> Result<Option<VideoPlaybackSpec>, String> {
    let Some(_) = runtime_record else {
        clear_video_diagnostics(app);
        return Ok(None);
    };
    let source_path = match resolve_active_video_source_path(runtime_record) {
        Ok(Some(source_path)) => source_path,
        Ok(None) => {
            clear_video_diagnostics(app);
            return Ok(None);
        }
        Err(error) => {
            let detail = error_detail(&error);
            let _ = diagnostic_service::record_error(
                app,
                DIAGNOSTIC_SUBSYSTEM,
                MISSING_SOURCE_CODE,
                "Native video runtime is missing its media file path.",
                detail,
            );
            let _ =
                diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, SYNC_FAILED_CODE);
            return Err(error);
        }
    };
    let _ = diagnostic_service::clear_diagnostic(app, DIAGNOSTIC_SUBSYSTEM, MISSING_SOURCE_CODE);

    let labels = window_service::player_window_label_set(app)
        .into_iter()
        .collect::<Vec<_>>();
    if labels.is_empty() {
        return Ok(None);
    }

    Ok(Some(VideoPlaybackSpec {
        source_path,
        paused,
        looping_enabled: true,
        window_labels: labels,
    }))
}

fn clear_video_diagnostics(app: &AppHandle) {
    let _ = diagnostic_service::clear_subsystem(app, DIAGNOSTIC_SUBSYSTEM);
}

fn resolve_active_video_source_path(
    runtime_record: Option<&WallpaperRuntimeRecord>,
) -> Result<Option<String>, String> {
    let Some(record) = runtime_record else {
        return Ok(None);
    };
    let video = match &record.runtime {
        WallpaperRuntime::Video { video } => video,
        _ => return Ok(None),
    };

    let source_path = video
        .entry_path
        .as_ref()
        .or(record.entry_path.as_ref())
        .cloned()
        .ok_or_else(|| "native video runtime is missing its media file path".to_string())?;
    if !Path::new(&source_path).is_file() {
        return Err(format!(
            "native video runtime source file is missing: {source_path}"
        ));
    }

    Ok(Some(source_path))
}

fn error_detail(error: &str) -> Option<String> {
    let detail = error.trim();
    if detail.is_empty() {
        None
    } else {
        Some(detail.to_string())
    }
}

fn plan_native_video_runtime(
    current: &NativeVideoRuntimeSnapshot,
    desired: Option<&VideoPlaybackSpec>,
) -> NativeVideoRuntimePlan {
    let current_labels = current.labels.iter().cloned().collect::<BTreeSet<_>>();
    let desired_labels = desired
        .map(|spec| spec.window_labels.iter().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();

    let remove_labels = current_labels
        .difference(&desired_labels)
        .cloned()
        .collect::<Vec<_>>();
    let ensure_labels = desired_labels.iter().cloned().collect::<Vec<_>>();

    let session = match desired {
        None => {
            if current.source_path.is_some() || !current.labels.is_empty() {
                SessionPlan::Stop
            } else {
                SessionPlan::Keep
            }
        }
        Some(spec) => match current.source_path.as_deref() {
            None => SessionPlan::Start {
                source_path: spec.source_path.clone(),
                paused: spec.paused,
                looping_enabled: spec.looping_enabled,
            },
            Some(current_source)
                if current_source != spec.source_path
                    || current.looping_enabled != spec.looping_enabled =>
            {
                SessionPlan::Replace {
                    source_path: spec.source_path.clone(),
                    paused: spec.paused,
                    looping_enabled: spec.looping_enabled,
                }
            }
            Some(_) if current.paused != spec.paused => SessionPlan::UpdatePause {
                paused: spec.paused,
            },
            Some(_) => SessionPlan::Keep,
        },
    };

    NativeVideoRuntimePlan {
        session,
        ensure_labels,
        remove_labels,
    }
}

fn apply_runtime_plan(
    runtime: &mut NativeVideoRuntime,
    app: &AppHandle,
    plan: NativeVideoRuntimePlan,
    output_volume: f64,
) -> Result<(), String> {
    for label in &plan.remove_labels {
        runtime.remove_view(label);
    }

    match plan.session {
        SessionPlan::Keep => {}
        SessionPlan::Stop => {
            for label in runtime.views.keys().cloned().collect::<Vec<_>>() {
                runtime.remove_view(&label);
            }
            runtime.clear_session();
            return Ok(());
        }
        SessionPlan::Start {
            source_path,
            paused,
            looping_enabled,
        }
        | SessionPlan::Replace {
            source_path,
            paused,
            looping_enabled,
        } => {
            runtime.clear_session();
            runtime.session = Some(NativeVideoSessionHandle::create(
                &source_path,
                paused,
                looping_enabled,
                output_volume,
            )?);
        }
        SessionPlan::UpdatePause { paused } => {
            if let Some(session) = runtime.session.as_mut() {
                session.set_paused(paused);
            }
        }
    }

    let (Some(session), views) = (runtime.session.as_ref(), &mut runtime.views) else {
        return Ok(());
    };
    session.set_output_volume(output_volume);
    for label in &plan.ensure_labels {
        ensure_view_attached(views, app, label, session)?;
    }

    Ok(())
}

fn ensure_view_attached(
    views: &mut BTreeMap<String, NativeVideoViewHandle>,
    app: &AppHandle,
    label: &str,
    session: &NativeVideoSessionHandle,
) -> Result<(), String> {
    match views.entry(label.to_string()) {
        Entry::Occupied(entry) => entry.get().attach(app, label, session),
        Entry::Vacant(entry) => {
            let view = NativeVideoViewHandle::create()?;
            view.attach(app, label, session)?;
            entry.insert(view);
            Ok(())
        }
    }
}

impl NativeVideoRuntime {
    fn snapshot(&self) -> NativeVideoRuntimeSnapshot {
        NativeVideoRuntimeSnapshot {
            source_path: self
                .session
                .as_ref()
                .map(|session| session.source_path.clone()),
            paused: self
                .session
                .as_ref()
                .map(|session| session.paused)
                .unwrap_or(false),
            looping_enabled: self
                .session
                .as_ref()
                .map(|session| session.looping_enabled)
                .unwrap_or(false),
            labels: self.views.keys().cloned().collect(),
        }
    }

    fn clear_session(&mut self) {
        if let Some(session) = self.session.take() {
            session.teardown();
        }
    }

    fn remove_view(&mut self, label: &str) {
        if let Some(view) = self.views.remove(label) {
            view.teardown();
        }
    }
}

struct NativeVideoSessionHandle {
    source_path: String,
    paused: bool,
    looping_enabled: bool,
    #[cfg(target_os = "macos")]
    host: MainThreadBound<NativeVideoSessionHost>,
}

impl NativeVideoSessionHandle {
    fn create(
        source_path: &str,
        paused: bool,
        looping_enabled: bool,
        output_volume: f64,
    ) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let source_path_string = source_path.to_string();
            let host = run_on_main(move |mtm| {
                NativeVideoSessionHost::create(&source_path_string, paused, output_volume, mtm)
                    .map(|host| MainThreadBound::new(host, mtm))
            })?;

            return Ok(Self {
                source_path: source_path.to_string(),
                paused,
                looping_enabled,
                host,
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self {
                source_path: source_path.to_string(),
                paused,
                looping_enabled,
            })
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused = paused;

        #[cfg(target_os = "macos")]
        run_on_main(|_mtm| {
            let host = self.host.get(unsafe { MainThreadMarker::new_unchecked() });
            host.set_paused(paused);
        });
    }

    fn set_output_volume(&self, output_volume: f64) {
        #[cfg(target_os = "macos")]
        run_on_main(|_mtm| {
            let host = self.host.get(unsafe { MainThreadMarker::new_unchecked() });
            host.set_output_volume(output_volume);
        });

        #[cfg(not(target_os = "macos"))]
        {
            let _ = output_volume;
        }
    }

    fn teardown(self) {
        #[cfg(target_os = "macos")]
        run_on_main(|_mtm| {
            let host = self.host.get(unsafe { MainThreadMarker::new_unchecked() });
            host.prepare_for_detach();
        });
    }
}

struct NativeVideoViewHandle {
    #[cfg(target_os = "macos")]
    host: MainThreadBound<NativeVideoViewHost>,
}

impl NativeVideoViewHandle {
    fn create() -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let host = run_on_main(|mtm| {
                Ok::<_, String>(MainThreadBound::new(NativeVideoViewHost::create(mtm), mtm))
            })?;
            return Ok(Self { host });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self {})
        }
    }

    fn attach(
        &self,
        app: &AppHandle,
        label: &str,
        session: &NativeVideoSessionHandle,
    ) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let app = app.clone();
            let label = label.to_string();
            return run_on_main(move |_mtm| {
                let window = window_service::player_window(&app, &label)?;
                let session_host = session
                    .host
                    .get(unsafe { MainThreadMarker::new_unchecked() });
                let view_host = self.host.get(unsafe { MainThreadMarker::new_unchecked() });
                view_host.attach(&window, session_host)
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, label, session);
            Ok(())
        }
    }

    fn teardown(self) {
        #[cfg(target_os = "macos")]
        run_on_main(|_mtm| {
            let host = self.host.get(unsafe { MainThreadMarker::new_unchecked() });
            host.detach();
        });
    }
}

#[cfg(target_os = "macos")]
struct NativeVideoSessionHost {
    player: Retained<AVQueuePlayer>,
    looper: Retained<AVPlayerLooper>,
}

#[cfg(target_os = "macos")]
impl NativeVideoSessionHost {
    fn create(
        source_path: &str,
        paused: bool,
        output_volume: f64,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let url = NSURL::from_file_path(source_path)
            .ok_or_else(|| format!("native video runtime rejected invalid path: {source_path}"))?;
        let item = unsafe { AVPlayerItem::playerItemWithURL(&url, mtm) };
        let player = unsafe { AVQueuePlayer::new(MainThreadMarker::new_unchecked()) };
        unsafe {
            player.setMuted(output_volume <= 0.001);
            player.setVolume(output_volume as f32);
            player.setPreventsDisplaySleepDuringVideoPlayback(false);
        }
        let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &item) };
        let host = Self { player, looper };
        host.set_paused(paused);
        Ok(host)
    }

    fn set_paused(&self, paused: bool) {
        unsafe {
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    fn set_output_volume(&self, output_volume: f64) {
        let normalized = output_volume.clamp(0.0, 1.0);
        unsafe {
            self.player.setMuted(normalized <= 0.001);
            self.player.setVolume(normalized as f32);
        }
    }

    fn prepare_for_detach(&self) {
        unsafe {
            self.player.pause();
            self.looper.disableLooping();
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeVideoViewHost {
    view: Retained<AVPlayerView>,
}

#[cfg(target_os = "macos")]
impl NativeVideoViewHost {
    fn create(mtm: MainThreadMarker) -> Self {
        let view = unsafe {
            AVPlayerView::initWithFrame(
                AVPlayerView::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
            )
        };

        unsafe {
            view.setControlsStyle(AVPlayerViewControlsStyle::None);
            if let Some(gravity) = AVLayerVideoGravityResizeAspectFill {
                view.setVideoGravity(gravity);
            }
            view.setShowsFullScreenToggleButton(false);
            view.setShowsSharingServiceButton(false);
            view.setUpdatesNowPlayingInfoCenter(false);
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
        }

        Self { view }
    }

    fn attach(
        &self,
        window: &tauri::WebviewWindow,
        session: &NativeVideoSessionHost,
    ) -> Result<(), String> {
        let container_ptr = window.ns_view().map_err(|error| error.to_string())?;
        let container = unsafe { &*(container_ptr.cast::<NSView>()) };

        unsafe {
            self.view.setPlayer(Some(&session.player));
            self.view.removeFromSuperview();
            self.view.setFrame(container.bounds());
            container.addSubview(&self.view);
        }

        Ok(())
    }

    fn detach(&self) {
        unsafe {
            self.view.setPlayer(None);
            self.view.removeFromSuperview();
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::models::{
        VideoRuntimeDocument, WallpaperRuntime, WallpaperRuntimeRecord, WallpaperType,
    };

    use super::{
        plan_native_video_runtime, resolve_active_video_source_path, NativeVideoRuntimeSnapshot,
        SessionPlan, VideoPlaybackSpec,
    };

    fn playback_spec(source_path: &str, paused: bool, window_labels: &[&str]) -> VideoPlaybackSpec {
        VideoPlaybackSpec {
            source_path: source_path.to_string(),
            paused,
            looping_enabled: true,
            window_labels: window_labels
                .iter()
                .map(|label| (*label).to_string())
                .collect(),
        }
    }

    fn runtime_record(
        runtime: WallpaperRuntime,
        entry_path: Option<&str>,
    ) -> WallpaperRuntimeRecord {
        let wallpaper_type = match &runtime {
            WallpaperRuntime::Scene { .. } => WallpaperType::Scene,
            WallpaperRuntime::Video { .. } => WallpaperType::Video,
            WallpaperRuntime::Web { .. } => WallpaperType::Web,
            WallpaperRuntime::Application => WallpaperType::Application,
            WallpaperRuntime::Unknown => WallpaperType::Unknown,
        };

        WallpaperRuntimeRecord {
            id: "demo".to_string(),
            title: "Demo".to_string(),
            wallpaper_type,
            source_path: "/tmp/source".to_string(),
            managed_path: "/tmp/managed".to_string(),
            preview_path: None,
            entry_path: entry_path.map(str::to_string),
            last_snapshot_path: None,
            property_schema: vec![],
            property_sections: vec![],
            imported_at: Utc::now(),
            tags: vec![],
            runtime,
        }
    }

    #[test]
    fn player_startup_and_shutdown_create_and_remove_native_video_runtime() {
        let start = plan_native_video_runtime(
            &NativeVideoRuntimeSnapshot::default(),
            Some(&playback_spec(
                "/tmp/video-a.mp4",
                false,
                &["player", "player-screen-1"],
            )),
        );
        assert!(matches!(
            start.session,
            SessionPlan::Start {
                ref source_path,
                paused: false,
                looping_enabled: true,
            } if source_path == "/tmp/video-a.mp4"
        ));
        assert_eq!(start.ensure_labels, vec!["player", "player-screen-1"]);
        assert!(start.remove_labels.is_empty());

        let stop = plan_native_video_runtime(
            &NativeVideoRuntimeSnapshot {
                source_path: Some("/tmp/video-a.mp4".to_string()),
                paused: false,
                looping_enabled: true,
                labels: vec!["player".to_string(), "player-screen-1".to_string()],
            },
            None,
        );
        assert!(matches!(stop.session, SessionPlan::Stop));
        assert_eq!(stop.remove_labels, vec!["player", "player-screen-1"]);
        assert!(stop.ensure_labels.is_empty());
    }

    #[test]
    fn pause_and_resume_only_update_player_state_for_same_video() {
        let pause = plan_native_video_runtime(
            &NativeVideoRuntimeSnapshot {
                source_path: Some("/tmp/video-a.mp4".to_string()),
                paused: false,
                looping_enabled: true,
                labels: vec!["player".to_string()],
            },
            Some(&playback_spec("/tmp/video-a.mp4", true, &["player"])),
        );
        assert!(matches!(
            pause.session,
            SessionPlan::UpdatePause { paused: true }
        ));
        assert_eq!(pause.ensure_labels, vec!["player"]);
        assert!(pause.remove_labels.is_empty());

        let resume = plan_native_video_runtime(
            &NativeVideoRuntimeSnapshot {
                source_path: Some("/tmp/video-a.mp4".to_string()),
                paused: true,
                looping_enabled: true,
                labels: vec!["player".to_string()],
            },
            Some(&playback_spec("/tmp/video-a.mp4", false, &["player"])),
        );
        assert!(matches!(
            resume.session,
            SessionPlan::UpdatePause { paused: false }
        ));
        assert_eq!(resume.ensure_labels, vec!["player"]);
        assert!(resume.remove_labels.is_empty());
    }

    #[test]
    fn switching_video_wallpaper_rebuilds_the_native_session() {
        let plan = plan_native_video_runtime(
            &NativeVideoRuntimeSnapshot {
                source_path: Some("/tmp/video-a.mp4".to_string()),
                paused: false,
                looping_enabled: true,
                labels: vec!["player".to_string()],
            },
            Some(&playback_spec("/tmp/video-b.mp4", false, &["player"])),
        );

        assert!(matches!(
            plan.session,
            SessionPlan::Replace {
                ref source_path,
                paused: false,
                looping_enabled: true,
            } if source_path == "/tmp/video-b.mp4"
        ));
        assert_eq!(plan.ensure_labels, vec!["player"]);
        assert!(plan.remove_labels.is_empty());
    }

    #[test]
    fn looped_playback_intent_stays_enabled_for_native_video_sessions() {
        let plan = plan_native_video_runtime(
            &NativeVideoRuntimeSnapshot {
                source_path: Some("/tmp/video-a.mp4".to_string()),
                paused: false,
                looping_enabled: false,
                labels: vec!["player".to_string()],
            },
            Some(&playback_spec("/tmp/video-a.mp4", false, &["player"])),
        );

        assert!(matches!(
            plan.session,
            SessionPlan::Replace {
                looping_enabled: true,
                ..
            }
        ));
        assert_eq!(plan.ensure_labels, vec!["player"]);
        assert!(plan.remove_labels.is_empty());
    }

    #[test]
    fn missing_video_source_path_returns_error() {
        let record = runtime_record(
            WallpaperRuntime::Video {
                video: VideoRuntimeDocument::default(),
            },
            None,
        );

        let error =
            resolve_active_video_source_path(Some(&record)).expect_err("missing path error");
        assert!(error.contains("missing its media file path"));
    }

    #[test]
    fn missing_video_source_file_returns_error() {
        let record = runtime_record(
            WallpaperRuntime::Video {
                video: VideoRuntimeDocument {
                    entry_path: Some("/tmp/definitely-missing-video.mp4".to_string()),
                    ..VideoRuntimeDocument::default()
                },
            },
            None,
        );

        let error =
            resolve_active_video_source_path(Some(&record)).expect_err("missing file error");
        assert!(error.contains("/tmp/definitely-missing-video.mp4"));
    }
}
