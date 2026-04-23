#[cfg(target_os = "macos")]
use std::ptr::NonNull;
use std::{
    collections::BTreeSet,
    sync::{Condvar, Mutex, RwLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rustfft::{num_complex::Complex32, FftPlanner};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::services::{diagnostic_service, native_web_service, window_service};

#[cfg(target_os = "macos")]
use dispatch2::{DispatchQueue as NativeDispatchQueue, DispatchRetained};
#[cfg(target_os = "macos")]
use screencapturekit::dispatch_queue::{
    DispatchQoS as CaptureDispatchQoS, DispatchQueue as CaptureDispatchQueue,
};
#[cfg(target_os = "macos")]
use screencapturekit::prelude::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutputType,
};

const AUDIO_EVENT_NAME: &str = "player:audio";
const AUDIO_OUTPUT_BANDS: usize = 128;
const AUDIO_FFT_SIZE: usize = 2048;
const AUDIO_HOP_SIZE: usize = 1024;
const AUDIO_SAMPLE_RATE: i32 = 48_000;
const AUDIO_CHANNEL_COUNT: i32 = 2;
const AUDIO_ATTACK: f32 = 0.52;
const AUDIO_DECAY: f32 = 0.18;
const AUDIO_IDLE_INTERVAL: Duration = Duration::from_millis(700);
const AUDIO_ACTIVE_INTERVAL: Duration = Duration::from_millis(250);
const AUDIO_ERROR_BACKOFF: Duration = Duration::from_secs(3);
const AUDIO_SILENCE_THRESHOLD: f32 = 0.003;
#[cfg(target_os = "macos")]
const AUDIO_CAPTURE_QUEUE_LABEL: &str = "wallpaper.shared-audio.capture";
const DIAGNOSTIC_SUBSYSTEM: &str = "shared-audio";
const CAPTURE_UNAVAILABLE_CODE: &str = "capture-unavailable";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioSnapshot {
    pub timestamp_ms: u64,
    pub bands: Vec<f32>,
    pub smoothed_bands: Vec<f32>,
    pub peak: f32,
    pub rms: f32,
    pub muted: bool,
    pub active: bool,
}

impl Default for AudioSnapshot {
    fn default() -> Self {
        Self::silent(timestamp_ms())
    }
}

impl AudioSnapshot {
    pub fn silent(timestamp_ms: u64) -> Self {
        Self {
            timestamp_ms,
            bands: vec![0.0; AUDIO_OUTPUT_BANDS],
            smoothed_bands: vec![0.0; AUDIO_OUTPUT_BANDS],
            peak: 0.0,
            rms: 0.0,
            muted: true,
            active: false,
        }
    }
}

#[derive(Debug, Default)]
struct AudioWorkerSignal {
    refresh_requested: bool,
}

pub struct SharedAudioServiceState {
    snapshot: RwLock<AudioSnapshot>,
    scene_consumers: Mutex<BTreeSet<String>>,
    worker_signal: Mutex<AudioWorkerSignal>,
    worker_changed: Condvar,
}

impl Default for SharedAudioServiceState {
    fn default() -> Self {
        Self {
            snapshot: RwLock::new(AudioSnapshot::default()),
            scene_consumers: Mutex::new(BTreeSet::new()),
            worker_signal: Mutex::new(AudioWorkerSignal::default()),
            worker_changed: Condvar::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AudioDemand {
    scene: bool,
    web: bool,
}

impl AudioDemand {
    fn active(self) -> bool {
        self.scene || self.web
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SceneConsumerStatus {
    changed: bool,
    active: bool,
}

pub fn start_audio_worker(app: AppHandle) {
    thread::spawn(move || {
        let Some(state) = app.try_state::<SharedAudioServiceState>() else {
            return;
        };

        let mut session: Option<AudioCaptureSession> = None;
        let mut last_error_at: Option<Instant> = None;
        let mut last_demand = AudioDemand::default();

        loop {
            let demand = desired_audio_demand(&app).unwrap_or_default();

            if demand != last_demand {
                last_demand = demand;
            }

            if demand.active() {
                if session.is_none() {
                    match AudioCaptureSession::start(app.clone()) {
                        Ok(capture) => {
                            session = Some(capture);
                            last_error_at = None;
                            let _ = diagnostic_service::clear_diagnostic(
                                &app,
                                DIAGNOSTIC_SUBSYSTEM,
                                CAPTURE_UNAVAILABLE_CODE,
                            );
                        }
                        Err(error) => {
                            let now = Instant::now();
                            if last_error_at
                                .map(|previous| now.duration_since(previous) >= AUDIO_ERROR_BACKOFF)
                                .unwrap_or(true)
                            {
                                let _ = diagnostic_service::record_warning(
                                    &app,
                                    DIAGNOSTIC_SUBSYSTEM,
                                    CAPTURE_UNAVAILABLE_CODE,
                                    "Shared audio capture is unavailable. Check Screen Recording permission and active displays.",
                                    Some(error.clone()),
                                );
                                last_error_at = Some(now);
                            }
                            let _ =
                                publish_audio_snapshot(&app, AudioSnapshot::silent(timestamp_ms()));
                        }
                    }
                }
            } else if let Some(capture) = session.take() {
                capture.stop();
                let _ = publish_audio_snapshot(&app, AudioSnapshot::silent(timestamp_ms()));
                let _ = diagnostic_service::clear_diagnostic(
                    &app,
                    DIAGNOSTIC_SUBSYSTEM,
                    CAPTURE_UNAVAILABLE_CODE,
                );
            } else {
                let _ = diagnostic_service::clear_diagnostic(
                    &app,
                    DIAGNOSTIC_SUBSYSTEM,
                    CAPTURE_UNAVAILABLE_CODE,
                );
            }

            let wait_for = if demand.active() {
                AUDIO_ACTIVE_INTERVAL
            } else {
                AUDIO_IDLE_INTERVAL
            };
            wait_for_refresh(&state, wait_for);
        }
    });
}

pub fn current_audio_snapshot(app: &AppHandle) -> Result<AudioSnapshot, String> {
    app.try_state::<SharedAudioServiceState>()
        .map(|state| state.snapshot())
        .unwrap_or_else(|| Ok(AudioSnapshot::default()))
}

pub fn set_scene_audio_interest(
    app: &AppHandle,
    window_label: &str,
    interested: bool,
) -> Result<(), String> {
    let Some(state) = app.try_state::<SharedAudioServiceState>() else {
        return Ok(());
    };

    state.set_scene_consumer_interest(window_label, interested)?;

    request_policy_refresh(app);
    Ok(())
}

pub fn prune_scene_audio_interest(app: &AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<SharedAudioServiceState>() else {
        return Ok(false);
    };

    let live_window_labels = live_player_window_labels(app);
    let changed = state.prune_scene_consumers(&live_window_labels)?;
    if changed {
        request_policy_refresh(app);
    }
    Ok(changed)
}

pub fn request_policy_refresh(app: &AppHandle) {
    let Some(state) = app.try_state::<SharedAudioServiceState>() else {
        return;
    };

    {
        let Ok(mut signal) = state.worker_signal.lock() else {
            return;
        };
        signal.refresh_requested = true;
    }
    state.worker_changed.notify_one();
}

fn desired_audio_demand(app: &AppHandle) -> Result<AudioDemand, String> {
    let live_window_labels = live_player_window_labels(app);
    let scene = app
        .try_state::<SharedAudioServiceState>()
        .map(|state| state.scene_consumers_active_for_player_windows(&live_window_labels))
        .transpose()?
        .unwrap_or(false);
    let web = native_web_service::audio_consumers_active(app)?;

    Ok(AudioDemand { scene, web })
}

fn live_player_window_labels(app: &AppHandle) -> BTreeSet<String> {
    window_service::player_window_labels(app)
        .into_iter()
        .collect()
}

fn wait_for_refresh(state: &SharedAudioServiceState, timeout: Duration) {
    let Ok(signal) = state.worker_signal.lock() else {
        thread::sleep(timeout);
        return;
    };

    if signal.refresh_requested {
        drop(signal);
        if let Ok(mut signal) = state.worker_signal.lock() {
            signal.refresh_requested = false;
        }
        return;
    }

    let Ok((mut signal, _)) = state.worker_changed.wait_timeout(signal, timeout) else {
        return;
    };
    signal.refresh_requested = false;
}

fn publish_audio_snapshot(app: &AppHandle, snapshot: AudioSnapshot) -> Result<(), String> {
    if let Some(state) = app.try_state::<SharedAudioServiceState>() {
        state.replace_snapshot(snapshot.clone())?;
    }
    app.emit(AUDIO_EVENT_NAME, snapshot.clone())
        .map_err(|error| error.to_string())?;
    native_web_service::dispatch_shared_audio(app, &snapshot)?;
    Ok(())
}

impl SharedAudioServiceState {
    fn replace_snapshot(&self, snapshot: AudioSnapshot) -> Result<(), String> {
        let mut stored = self.snapshot.write().map_err(|error| error.to_string())?;
        *stored = snapshot;
        Ok(())
    }

    fn snapshot(&self) -> Result<AudioSnapshot, String> {
        self.snapshot
            .read()
            .map(|snapshot| snapshot.clone())
            .map_err(|error| error.to_string())
    }

    fn set_scene_consumer_interest(
        &self,
        window_label: &str,
        interested: bool,
    ) -> Result<(), String> {
        let mut consumers = self
            .scene_consumers
            .lock()
            .map_err(|error| error.to_string())?;
        if interested {
            consumers.insert(window_label.to_string());
        } else {
            consumers.remove(window_label);
        }
        Ok(())
    }

    fn prune_scene_consumers(&self, live_window_labels: &BTreeSet<String>) -> Result<bool, String> {
        Ok(self.scene_consumer_status(live_window_labels)?.changed)
    }

    fn scene_consumers_active_for_player_windows(
        &self,
        live_window_labels: &BTreeSet<String>,
    ) -> Result<bool, String> {
        Ok(self.scene_consumer_status(live_window_labels)?.active)
    }

    fn scene_consumer_status(
        &self,
        live_window_labels: &BTreeSet<String>,
    ) -> Result<SceneConsumerStatus, String> {
        let mut consumers = self
            .scene_consumers
            .lock()
            .map_err(|error| error.to_string())?;
        let before_len = consumers.len();
        consumers.retain(|label| live_window_labels.contains(label));
        Ok(SceneConsumerStatus {
            changed: consumers.len() != before_len,
            active: !consumers.is_empty(),
        })
    }
}

struct AudioCaptureSession {
    #[cfg(target_os = "macos")]
    stream: SCStream,
    #[cfg(target_os = "macos")]
    audio_output_id: usize,
    #[cfg(target_os = "macos")]
    _callback_queue: CaptureDispatchQueue,
    #[cfg(target_os = "macos")]
    drain_queue: DispatchRetained<NativeDispatchQueue>,
}

impl AudioCaptureSession {
    fn start(app: AppHandle) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            let content = SCShareableContent::get().map_err(|error| error.to_string())?;
            let display = content
                .displays()
                .into_iter()
                .next()
                .ok_or_else(|| "shared audio capture requires an active display".to_string())?;
            let filter = SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&[])
                .build();
            let configuration = SCStreamConfiguration::new()
                .with_captures_audio(true)
                .with_sample_rate(AUDIO_SAMPLE_RATE)
                .with_channel_count(AUDIO_CHANNEL_COUNT)
                .with_excludes_current_process_audio(true);

            let mut stream = SCStream::new(&filter, &configuration);
            let callback_queue = CaptureDispatchQueue::new(
                AUDIO_CAPTURE_QUEUE_LABEL,
                CaptureDispatchQoS::UserInitiated,
            );
            let drain_queue = retain_native_dispatch_queue(&callback_queue)?;
            let analyzer = Mutex::new(AudioAnalyzer::new(
                AUDIO_FFT_SIZE,
                AUDIO_HOP_SIZE,
                AUDIO_OUTPUT_BANDS,
                AUDIO_SAMPLE_RATE as f32,
            ));
            let capture_app = app.clone();

            let audio_output_id = stream
                .add_output_handler_with_queue(
                    move |sample, of_type| {
                        if of_type != SCStreamOutputType::Audio {
                            return;
                        }

                        let Some(decoded) = decode_audio_samples(&sample) else {
                            return;
                        };

                        let Ok(mut analyzer) = analyzer.lock() else {
                            return;
                        };
                        if let Some(snapshot) =
                            analyzer.push_samples(&decoded, sample_timestamp_ms(&sample))
                        {
                            let _ = publish_audio_snapshot(&capture_app, snapshot);
                        }
                    },
                    SCStreamOutputType::Audio,
                    Some(&callback_queue),
                )
                .ok_or_else(|| {
                    "shared audio capture failed to register output handler".to_string()
                })?;

            stream.start_capture().map_err(|error| error.to_string())?;
            return Ok(Self {
                stream,
                audio_output_id,
                _callback_queue: callback_queue,
                drain_queue,
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            Err("shared audio capture is only implemented on macOS".to_string())
        }
    }

    fn stop(mut self) {
        #[cfg(target_os = "macos")]
        {
            let _ = self.stream.stop_capture();
            // ScreenCaptureKit may still have audio callbacks queued after
            // `stop_capture` completes; drain the serial callback queue before
            // removing the handler and releasing the stream context.
            self.drain_queue.exec_sync(|| {});
            let _ = self
                .stream
                .remove_output_handler(self.audio_output_id, SCStreamOutputType::Audio);
        }
    }
}

#[cfg(target_os = "macos")]
fn retain_native_dispatch_queue(
    queue: &CaptureDispatchQueue,
) -> Result<DispatchRetained<NativeDispatchQueue>, String> {
    let ptr = NonNull::new(queue.as_ptr().cast_mut().cast::<NativeDispatchQueue>())
        .ok_or_else(|| "shared audio capture callback queue was null".to_string())?;
    // SAFETY: `CaptureDispatchQueue` wraps a live `dispatch_queue_t`, and
    // `dispatch2::DispatchQueue` is the same opaque dispatch queue type.
    Ok(unsafe { DispatchRetained::<NativeDispatchQueue>::retain(ptr) })
}

#[cfg(target_os = "macos")]
fn decode_audio_samples(sample: &screencapturekit::cm::CMSampleBuffer) -> Option<Vec<f32>> {
    let format = sample.format_description()?;
    let buffers = sample.audio_buffer_list()?;
    let bits_per_channel = format.audio_bits_per_channel().unwrap_or(32) as usize;
    let is_float = format.audio_is_float();
    let big_endian = format.audio_is_big_endian();
    let declared_channels = format.audio_channel_count().unwrap_or(1).max(1) as usize;

    if buffers.num_buffers() == 1 {
        let buffer = buffers.get(0)?;
        let channels = buffer.number_channels.max(1) as usize;
        return decode_interleaved_pcm(
            buffer.data(),
            bits_per_channel,
            is_float,
            big_endian,
            channels.max(declared_channels),
        );
    }

    let mut per_channel = Vec::new();
    for buffer in buffers.iter() {
        let decoded = decode_interleaved_pcm(
            buffer.data(),
            bits_per_channel,
            is_float,
            big_endian,
            buffer.number_channels.max(1) as usize,
        )?;
        per_channel.push(decoded);
    }
    mix_channels(&per_channel)
}

#[cfg(target_os = "macos")]
fn decode_interleaved_pcm(
    bytes: &[u8],
    bits_per_channel: usize,
    is_float: bool,
    big_endian: bool,
    channels: usize,
) -> Option<Vec<f32>> {
    let bytes_per_sample = bits_per_channel / 8;
    if bytes_per_sample == 0 || channels == 0 {
        return None;
    }
    let frame_size = bytes_per_sample * channels;
    if frame_size == 0 || bytes.len() < frame_size {
        return None;
    }

    let frames = bytes.len() / frame_size;
    let mut mono = Vec::with_capacity(frames);

    for frame_index in 0..frames {
        let mut sum = 0.0;
        for channel_index in 0..channels {
            let offset = frame_index * frame_size + channel_index * bytes_per_sample;
            let sample = decode_pcm_sample(
                &bytes[offset..offset + bytes_per_sample],
                bits_per_channel,
                is_float,
                big_endian,
            )?;
            sum += sample;
        }
        mono.push(sum / channels as f32);
    }

    Some(mono)
}

#[cfg(target_os = "macos")]
fn decode_pcm_sample(
    bytes: &[u8],
    bits_per_channel: usize,
    is_float: bool,
    big_endian: bool,
) -> Option<f32> {
    match (bits_per_channel, is_float, big_endian) {
        (32, true, false) => Some(f32::from_le_bytes(bytes.try_into().ok()?)),
        (32, true, true) => Some(f32::from_be_bytes(bytes.try_into().ok()?)),
        (16, false, false) => {
            Some(i16::from_le_bytes(bytes.try_into().ok()?) as f32 / i16::MAX as f32)
        }
        (16, false, true) => {
            Some(i16::from_be_bytes(bytes.try_into().ok()?) as f32 / i16::MAX as f32)
        }
        (32, false, false) => {
            Some(i32::from_le_bytes(bytes.try_into().ok()?) as f32 / i32::MAX as f32)
        }
        (32, false, true) => {
            Some(i32::from_be_bytes(bytes.try_into().ok()?) as f32 / i32::MAX as f32)
        }
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn mix_channels(per_channel: &[Vec<f32>]) -> Option<Vec<f32>> {
    if per_channel.is_empty() {
        return None;
    }
    let min_length = per_channel.iter().map(Vec::len).min().unwrap_or_default();
    if min_length == 0 {
        return None;
    }

    let mut mixed = vec![0.0; min_length];
    for channel in per_channel {
        for (index, sample) in channel.iter().take(min_length).enumerate() {
            mixed[index] += *sample;
        }
    }
    let divisor = per_channel.len() as f32;
    for sample in &mut mixed {
        *sample /= divisor;
    }
    Some(mixed)
}

#[cfg(target_os = "macos")]
fn sample_timestamp_ms(sample: &screencapturekit::cm::CMSampleBuffer) -> u64 {
    let timestamp = sample.presentation_timestamp();
    if timestamp.timescale > 0 {
        let seconds = timestamp.value as f64 / timestamp.timescale as f64;
        if seconds.is_finite() && seconds >= 0.0 {
            return (seconds * 1000.0).round() as u64;
        }
    }
    timestamp_ms()
}

struct AudioAnalyzer {
    fft_size: usize,
    hop_size: usize,
    band_ranges: Vec<(usize, usize)>,
    smoothed_bands: Vec<f32>,
    pending_samples: Vec<f32>,
    window: Vec<f32>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
}

impl AudioAnalyzer {
    fn new(fft_size: usize, hop_size: usize, band_count: usize, sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        Self {
            fft_size,
            hop_size,
            band_ranges: build_band_ranges(fft_size, band_count, sample_rate),
            smoothed_bands: vec![0.0; band_count],
            pending_samples: Vec::with_capacity(fft_size * 2),
            window: hann_window(fft_size),
            fft,
        }
    }

    fn push_samples(&mut self, samples: &[f32], timestamp_ms: u64) -> Option<AudioSnapshot> {
        if samples.is_empty() {
            return None;
        }

        self.pending_samples.extend_from_slice(samples);
        if self.pending_samples.len() < self.fft_size {
            return None;
        }

        let mut latest = None;
        while self.pending_samples.len() >= self.fft_size {
            let frame = self.pending_samples[..self.fft_size].to_vec();
            latest = Some(self.analyze_frame(&frame, timestamp_ms));

            let drain = self.hop_size.min(self.pending_samples.len());
            self.pending_samples.drain(..drain);
        }

        latest
    }

    fn analyze_frame(&mut self, frame: &[f32], timestamp_ms: u64) -> AudioSnapshot {
        let rms =
            (frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32).sqrt();
        let peak = frame
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);

        let mut spectrum = frame
            .iter()
            .zip(&self.window)
            .map(|(sample, window)| Complex32::new(sample * window, 0.0))
            .collect::<Vec<_>>();
        self.fft.process(&mut spectrum);

        let magnitudes = spectrum[..self.fft_size / 2]
            .iter()
            .map(|value| value.norm() / self.fft_size as f32)
            .collect::<Vec<_>>();

        let bands = self
            .band_ranges
            .iter()
            .map(|(start, end)| {
                if *start >= *end || *end > magnitudes.len() {
                    return 0.0;
                }
                let slice = &magnitudes[*start..*end];
                let average = slice.iter().copied().sum::<f32>() / slice.len() as f32;
                normalize_band_magnitude(average)
            })
            .collect::<Vec<_>>();

        for (index, target) in bands.iter().copied().enumerate() {
            let current = self.smoothed_bands[index];
            let factor = if target >= current {
                AUDIO_ATTACK
            } else {
                AUDIO_DECAY
            };
            self.smoothed_bands[index] = current + (target - current) * factor;
        }

        let active = peak >= AUDIO_SILENCE_THRESHOLD || rms >= AUDIO_SILENCE_THRESHOLD;
        if !active {
            for value in &mut self.smoothed_bands {
                *value *= 0.92;
            }
        }

        AudioSnapshot {
            timestamp_ms,
            bands,
            smoothed_bands: self.smoothed_bands.clone(),
            peak,
            rms,
            muted: !active,
            active,
        }
    }
}

fn build_band_ranges(fft_size: usize, band_count: usize, sample_rate: f32) -> Vec<(usize, usize)> {
    let nyquist = sample_rate / 2.0;
    let min_frequency = 20.0_f32;
    let max_frequency = nyquist.max(min_frequency + 1.0);
    let bin_hz = sample_rate / fft_size as f32;
    let mut ranges = Vec::with_capacity(band_count);

    for band in 0..band_count {
        let start_ratio = band as f32 / band_count as f32;
        let end_ratio = (band + 1) as f32 / band_count as f32;
        let start_hz = if band == 0 {
            0.0
        } else {
            min_frequency * (max_frequency / min_frequency).powf(start_ratio)
        };
        let end_hz = min_frequency * (max_frequency / min_frequency).powf(end_ratio);
        let start = ((start_hz / bin_hz).floor() as usize).min(fft_size / 2 - 1);
        let mut end = ((end_hz / bin_hz).ceil() as usize).min(fft_size / 2);
        if end <= start {
            end = (start + 1).min(fft_size / 2);
        }
        ranges.push((start, end));
    }

    ranges
}

fn hann_window(size: usize) -> Vec<f32> {
    if size <= 1 {
        return vec![1.0; size];
    }

    (0..size)
        .map(|index| {
            let phase = (2.0 * std::f32::consts::PI * index as f32) / (size - 1) as f32;
            0.5 - 0.5 * phase.cos()
        })
        .collect()
}

fn normalize_band_magnitude(value: f32) -> f32 {
    let scaled = (value * 96.0).max(0.0);
    (scaled.ln_1p() / 4.0).clamp(0.0, 1.0)
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        build_band_ranges, hann_window, normalize_band_magnitude, AudioAnalyzer, AudioDemand,
        AudioSnapshot, SharedAudioServiceState, AUDIO_OUTPUT_BANDS,
    };

    fn live_labels(labels: &[&str]) -> BTreeSet<String> {
        labels.iter().map(|label| (*label).to_string()).collect()
    }

    #[test]
    fn shared_audio_snapshot_state_round_trips_latest_frame() {
        let state = SharedAudioServiceState::default();
        let snapshot = AudioSnapshot {
            timestamp_ms: 42,
            bands: vec![0.1; AUDIO_OUTPUT_BANDS],
            smoothed_bands: vec![0.2; AUDIO_OUTPUT_BANDS],
            peak: 0.7,
            rms: 0.4,
            muted: false,
            active: true,
        };

        state
            .replace_snapshot(snapshot.clone())
            .expect("store snapshot");

        assert_eq!(state.snapshot().expect("read snapshot"), snapshot);
    }

    #[test]
    fn analyzer_turns_pcm_into_non_silent_snapshot() {
        let mut analyzer = AudioAnalyzer::new(2048, 1024, AUDIO_OUTPUT_BANDS, 48_000.0);
        let samples = (0..4096)
            .map(|index| {
                let phase = index as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU;
                phase.sin() * 0.65
            })
            .collect::<Vec<_>>();

        let snapshot = analyzer
            .push_samples(&samples, 128)
            .expect("snapshot produced");

        assert_eq!(snapshot.bands.len(), AUDIO_OUTPUT_BANDS);
        assert_eq!(snapshot.smoothed_bands.len(), AUDIO_OUTPUT_BANDS);
        assert!(snapshot.active);
        assert!(!snapshot.muted);
        assert!(snapshot.peak > 0.2);
        assert!(snapshot.rms > 0.1);
        assert!(
            snapshot
                .smoothed_bands
                .iter()
                .copied()
                .fold(0.0_f32, f32::max)
                > 0.0
        );
    }

    #[test]
    fn silent_snapshot_is_stable_and_predictable() {
        let snapshot = AudioSnapshot::silent(256);

        assert_eq!(snapshot.timestamp_ms, 256);
        assert!(snapshot.muted);
        assert!(!snapshot.active);
        assert!(snapshot.bands.iter().all(|value| *value == 0.0));
        assert!(snapshot.smoothed_bands.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn scene_and_web_demands_share_one_policy_decision() {
        let none = AudioDemand {
            scene: false,
            web: false,
        };
        let scene = AudioDemand {
            scene: true,
            web: false,
        };
        let web = AudioDemand {
            scene: false,
            web: true,
        };
        let both = AudioDemand {
            scene: true,
            web: true,
        };

        assert!(!none.active());
        assert!(scene.active());
        assert!(web.active());
        assert!(both.active());
    }

    #[test]
    fn repeated_scene_interest_does_not_duplicate_consumers() {
        let state = SharedAudioServiceState::default();
        state
            .set_scene_consumer_interest("player", true)
            .expect("register player");
        state
            .set_scene_consumer_interest("player", true)
            .expect("register duplicate player");

        assert!(state
            .scene_consumers_active_for_player_windows(&live_labels(&["player"]))
            .expect("scene consumers active"));
        assert_eq!(
            state.scene_consumers.lock().expect("lock consumers").len(),
            1
        );
    }

    #[test]
    fn stale_scene_consumer_label_is_pruned_to_live_player_windows() {
        let state = SharedAudioServiceState::default();
        state
            .set_scene_consumer_interest("player", true)
            .expect("register live scene consumer");
        state
            .set_scene_consumer_interest("player-screen-1", true)
            .expect("register stale scene consumer");

        let changed = state
            .prune_scene_consumers(&live_labels(&["player"]))
            .expect("prune scene consumers");

        assert!(changed);
        assert_eq!(
            state
                .scene_consumers
                .lock()
                .expect("lock consumers")
                .clone(),
            live_labels(&["player"])
        );
        assert!(state
            .scene_consumers_active_for_player_windows(&live_labels(&["player"]))
            .expect("scene consumers active"));
    }

    #[test]
    fn scene_audio_demand_goes_inactive_when_only_stale_labels_remain() {
        let state = SharedAudioServiceState::default();
        state
            .set_scene_consumer_interest("player-screen-1", true)
            .expect("register stale scene consumer");

        let active = state
            .scene_consumers_active_for_player_windows(&live_labels(&[]))
            .expect("compute scene consumer demand");

        assert!(!active);
        assert!(state
            .scene_consumers
            .lock()
            .expect("lock consumers")
            .is_empty());
    }

    #[test]
    fn rebuilt_window_labels_do_not_leave_stale_scene_audio_consumers_behind() {
        let state = SharedAudioServiceState::default();
        state
            .set_scene_consumer_interest("player-screen-1", true)
            .expect("register old window label");

        let active_after_rebuild = state
            .scene_consumers_active_for_player_windows(&live_labels(&["player-screen-2"]))
            .expect("prune old window label");

        assert!(!active_after_rebuild);
        assert!(state
            .scene_consumers
            .lock()
            .expect("lock consumers")
            .is_empty());

        state
            .set_scene_consumer_interest("player-screen-2", true)
            .expect("register rebuilt window label");

        assert_eq!(
            state
                .scene_consumers
                .lock()
                .expect("lock consumers")
                .clone(),
            live_labels(&["player-screen-2"])
        );
        assert!(state
            .scene_consumers_active_for_player_windows(&live_labels(&["player-screen-2"]))
            .expect("recompute scene consumer demand"));
    }

    #[test]
    fn spectral_helpers_build_expected_shapes() {
        let ranges = build_band_ranges(2048, 64, 48_000.0);
        let window = hann_window(64);

        assert_eq!(ranges.len(), 64);
        assert!(ranges.iter().all(|(start, end)| end > start));
        assert_eq!(window.len(), 64);
        assert!(normalize_band_magnitude(0.0) >= 0.0);
        assert!(normalize_band_magnitude(1.0) <= 1.0);
    }
}
