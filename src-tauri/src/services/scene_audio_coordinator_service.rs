use std::collections::{BTreeMap, BTreeSet};

use crate::services::audio_input_service::AudioSnapshot;

const MIN_ACTIVE_LEVEL: f64 = 0.0005;
const MIN_FRAME_DELTA_MS: u64 = 16;
const ATTACK_RESPONSE_RATE: f64 = 11.0;
const DECAY_RESPONSE_RATE: f64 = 5.5;
const IDLE_RELEASE_RATE: f64 = 7.0;
const RESPONSE_CURVE_EXPONENT: f64 = 0.78;
const RESPONSE_CURVE_FLOOR: f64 = 0.025;
const BAND_WINDOW_EXPONENT: f64 = 1.72;
const SHARED_AUDIO_MISSING_CODE: &str = "shared-audio-snapshot-missing";
const SHARED_AUDIO_IDLE_CODE: &str = "shared-audio-idle";
const SHARED_AUDIO_BANDS_MISSING_CODE: &str = "shared-audio-bands-missing";

#[derive(Debug)]
pub struct SceneAudioCoordinator {
    states: BTreeMap<usize, SceneAudioBandState>,
    config: SceneAudioCoordinatorConfig,
}

#[derive(Debug, Default)]
struct SceneAudioBandState {
    smoothed: Vec<f64>,
    last_updated_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneAudioCoordinatorConfig {
    pub min_active_level: f64,
    pub attack_response_rate: f64,
    pub decay_response_rate: f64,
    pub idle_release_rate: f64,
    pub response_curve_exponent: f64,
    pub response_curve_floor: f64,
    pub band_window_exponent: f64,
    pub peak_weight: f64,
    pub rms_weight: f64,
    pub average_weight: f64,
    pub local_sound_weight: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneAudioCoordinatorFrame {
    pub levels: Vec<f64>,
    pub diagnostics: Vec<SceneAudioCoordinatorDiagnostic>,
    pub shared_audio_active: bool,
    pub local_sound_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneAudioCoordinatorDiagnostic {
    pub severity: SceneAudioCoordinatorDiagnosticSeverity,
    pub code: String,
    pub runtime_stage: String,
    pub message: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneAudioCoordinatorDiagnosticSeverity {
    Info,
    Warning,
}

impl Default for SceneAudioCoordinator {
    fn default() -> Self {
        Self {
            states: BTreeMap::new(),
            config: SceneAudioCoordinatorConfig::default(),
        }
    }
}

impl Default for SceneAudioCoordinatorConfig {
    fn default() -> Self {
        Self {
            min_active_level: MIN_ACTIVE_LEVEL,
            attack_response_rate: ATTACK_RESPONSE_RATE,
            decay_response_rate: DECAY_RESPONSE_RATE,
            idle_release_rate: IDLE_RELEASE_RATE,
            response_curve_exponent: RESPONSE_CURVE_EXPONENT,
            response_curve_floor: RESPONSE_CURVE_FLOOR,
            band_window_exponent: BAND_WINDOW_EXPONENT,
            peak_weight: 0.50,
            rms_weight: 0.32,
            average_weight: 0.18,
            local_sound_weight: 0.92,
        }
    }
}

impl SceneAudioCoordinator {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_config(config: SceneAudioCoordinatorConfig) -> Self {
        Self {
            states: BTreeMap::new(),
            config,
        }
    }

    pub fn levels_for_count(
        &mut self,
        snapshot: Option<&AudioSnapshot>,
        sound_levels: Option<&[f64]>,
        count: usize,
        now_ms: u64,
    ) -> Vec<f64> {
        self.frame_for_count(snapshot, sound_levels, count, now_ms)
            .levels
    }

    pub fn frame_for_count(
        &mut self,
        snapshot: Option<&AudioSnapshot>,
        sound_levels: Option<&[f64]>,
        count: usize,
        now_ms: u64,
    ) -> SceneAudioCoordinatorFrame {
        if count == 0 {
            return SceneAudioCoordinatorFrame {
                levels: Vec::new(),
                diagnostics: scene_audio_input_diagnostics(snapshot, sound_levels),
                shared_audio_active: shared_audio_snapshot_active(snapshot),
                local_sound_active: local_sound_levels_active(sound_levels),
            };
        }

        let shared_levels = derive_scene_audio_levels_with_config(snapshot, count, self.config);
        let local_levels = derive_scene_local_sound_levels(sound_levels, count, self.config);
        let target_levels = merge_scene_audio_levels_with_config(
            &shared_levels,
            Some(&local_levels),
            count,
            self.config,
        );
        let diagnostics = scene_audio_input_diagnostics(snapshot, sound_levels);
        let shared_audio_active = shared_audio_snapshot_active(snapshot);
        let local_sound_active = local_sound_levels_active(sound_levels);
        let state = self.states.entry(count).or_default();
        let reinitialized = state.smoothed.len() != count || state.last_updated_ms.is_none();
        if reinitialized {
            state.smoothed = target_levels.clone();
            state.last_updated_ms = Some(now_ms);
            return SceneAudioCoordinatorFrame {
                levels: state.smoothed.clone(),
                diagnostics,
                shared_audio_active,
                local_sound_active,
            };
        }

        let delta_ms = state
            .last_updated_ms
            .map(|previous| now_ms.saturating_sub(previous).max(MIN_FRAME_DELTA_MS))
            .unwrap_or(MIN_FRAME_DELTA_MS);
        state.last_updated_ms = Some(now_ms);

        for (index, current) in state.smoothed.iter_mut().enumerate() {
            let target = target_levels.get(index).copied().unwrap_or_default();
            let response_rate = if target > *current {
                self.config.attack_response_rate
            } else if target > self.config.min_active_level {
                self.config.decay_response_rate
            } else {
                self.config.idle_release_rate
            };
            let factor = smoothing_factor(delta_ms, response_rate);
            *current += (target - *current) * factor;
            if target <= self.config.min_active_level {
                *current *= 1.0 - factor * 0.42;
            }
            *current = current.clamp(0.0, 1.0);
        }

        SceneAudioCoordinatorFrame {
            levels: state.smoothed.clone(),
            diagnostics,
            shared_audio_active,
            local_sound_active,
        }
    }

    pub fn retain_counts(&mut self, active_counts: &BTreeSet<usize>) {
        self.states.retain(|count, _| active_counts.contains(count));
    }

    pub fn reset(&mut self) {
        self.states.clear();
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn derive_scene_audio_levels(snapshot: Option<&AudioSnapshot>, count: usize) -> Vec<f64> {
    derive_scene_audio_levels_with_config(snapshot, count, SceneAudioCoordinatorConfig::default())
}

fn derive_scene_audio_levels_with_config(
    snapshot: Option<&AudioSnapshot>,
    count: usize,
    config: SceneAudioCoordinatorConfig,
) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }

    let Some(snapshot) = snapshot else {
        return vec![0.0; count];
    };
    if !shared_audio_snapshot_active(Some(snapshot)) {
        return vec![0.0; count];
    }

    (0..count)
        .map(|index| {
            let (start, end) =
                scene_audio_window_with_config(index, count, snapshot.smoothed_bands.len(), config);
            let shaped = shaped_audio_band_value(
                snapshot.smoothed_bands.iter().map(|value| *value as f64),
                start,
                end,
                config,
            );
            let response = scene_audio_response_curve_with_config(shaped, config);
            let position = if count == 1 {
                0.5
            } else {
                index as f64 / (count - 1) as f64
            };
            (response * scene_audio_band_envelope(position)).clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn merge_scene_audio_levels(
    shared_levels: &[f64],
    sound_levels: Option<&[f64]>,
    count: usize,
) -> Vec<f64> {
    merge_scene_audio_levels_with_config(
        shared_levels,
        sound_levels,
        count,
        SceneAudioCoordinatorConfig::default(),
    )
}

fn merge_scene_audio_levels_with_config(
    shared_levels: &[f64],
    sound_levels: Option<&[f64]>,
    count: usize,
    config: SceneAudioCoordinatorConfig,
) -> Vec<f64> {
    (0..count)
        .map(|index| {
            let shared = shared_levels.get(index).copied().unwrap_or_default();
            let sound = sound_levels
                .and_then(|levels| levels.get(index).copied())
                .unwrap_or_default()
                * config.local_sound_weight;
            combine_audio_energy(shared, sound).clamp(0.0, 1.0)
        })
        .collect()
}

fn derive_scene_local_sound_levels(
    sound_levels: Option<&[f64]>,
    count: usize,
    config: SceneAudioCoordinatorConfig,
) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }
    let Some(sound_levels) = sound_levels.filter(|levels| !levels.is_empty()) else {
        return vec![0.0; count];
    };
    (0..count)
        .map(|index| {
            let (start, end) = linear_audio_window(index, count, sound_levels.len().max(1));
            let shaped = shaped_audio_band_value(sound_levels.iter().copied(), start, end, config);
            scene_audio_response_curve_with_config(shaped, config)
        })
        .collect()
}

fn scene_audio_window_with_config(
    index: usize,
    count: usize,
    source_len: usize,
    config: SceneAudioCoordinatorConfig,
) -> (usize, usize) {
    if source_len == 0 || count == 0 {
        return (0, 0);
    }
    let start_ratio = (index as f64 / count as f64).powf(config.band_window_exponent);
    let end_ratio = ((index + 1) as f64 / count as f64).powf(config.band_window_exponent);
    let start = (start_ratio * source_len as f64).floor() as usize;
    let mut end = (end_ratio * source_len as f64).ceil() as usize;
    if end <= start {
        end = (start + 1).min(source_len);
    }
    (start.min(source_len), end.min(source_len))
}

fn linear_audio_window(index: usize, count: usize, source_len: usize) -> (usize, usize) {
    if source_len == 0 || count == 0 {
        return (0, 0);
    }
    let start = (index * source_len) / count;
    let mut end = ((index + 1) * source_len) / count;
    if end <= start {
        end = (start + 1).min(source_len);
    }
    (start.min(source_len), end.min(source_len))
}

fn shaped_audio_band_value(
    values: impl Iterator<Item = f64>,
    start: usize,
    end: usize,
    config: SceneAudioCoordinatorConfig,
) -> f64 {
    let mut sum = 0.0_f64;
    let mut sum_squares = 0.0_f64;
    let mut peak = 0.0_f64;
    let mut samples = 0.0_f64;

    for value in values.skip(start).take(end.saturating_sub(start)) {
        let value = value.clamp(0.0, 1.0);
        sum += value;
        sum_squares += value * value;
        peak = peak.max(value);
        samples += 1.0;
    }

    if samples <= 0.0 {
        return 0.0;
    }

    let average = sum / samples;
    let rms = (sum_squares / samples).sqrt();
    let total_weight = (config.peak_weight + config.rms_weight + config.average_weight).max(0.001);
    (peak * config.peak_weight + rms * config.rms_weight + average * config.average_weight)
        / total_weight
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn scene_audio_response_curve(value: f64) -> f64 {
    scene_audio_response_curve_with_config(value, SceneAudioCoordinatorConfig::default())
}

fn scene_audio_response_curve_with_config(value: f64, config: SceneAudioCoordinatorConfig) -> f64 {
    let value = value.clamp(0.0, 1.0);
    let lifted = value.powf(config.response_curve_exponent);
    let floor = if value > config.min_active_level {
        config.response_curve_floor
    } else {
        0.0
    };
    (lifted * 0.97 + floor).clamp(0.0, 1.0)
}

fn scene_audio_band_envelope(position: f64) -> f64 {
    let center_weight = (1.0 - (position * 2.0 - 1.0).abs()).powf(0.45);
    (0.74 + center_weight * 0.26).clamp(0.0, 1.0)
}

fn smoothing_factor(delta_ms: u64, response_rate: f64) -> f64 {
    let delta_seconds = (delta_ms as f64 / 1000.0).clamp(1.0 / 240.0, 0.25);
    (1.0 - (-response_rate * delta_seconds).exp()).clamp(0.0, 1.0)
}

fn combine_audio_energy(shared: f64, sound: f64) -> f64 {
    let shared = shared.clamp(0.0, 1.0);
    let sound = sound.clamp(0.0, 1.0);
    1.0 - (1.0 - shared) * (1.0 - sound)
}

pub fn scene_audio_input_diagnostics(
    snapshot: Option<&AudioSnapshot>,
    sound_levels: Option<&[f64]>,
) -> Vec<SceneAudioCoordinatorDiagnostic> {
    if local_sound_levels_active(sound_levels) {
        return Vec::new();
    }

    match snapshot {
        None => vec![SceneAudioCoordinatorDiagnostic {
            severity: SceneAudioCoordinatorDiagnosticSeverity::Warning,
            code: SHARED_AUDIO_MISSING_CODE.to_string(),
            runtime_stage: "shared-audio".to_string(),
            message: "Scene audio response has no shared audio snapshot.".to_string(),
            reason:
                "No shared audio frame was available and no local Scene sound meter was active."
                    .to_string(),
        }],
        Some(snapshot) if snapshot.smoothed_bands.is_empty() => {
            vec![SceneAudioCoordinatorDiagnostic {
                severity: SceneAudioCoordinatorDiagnosticSeverity::Warning,
                code: SHARED_AUDIO_BANDS_MISSING_CODE.to_string(),
                runtime_stage: "shared-audio".to_string(),
                message: "Scene audio response received a shared audio snapshot without bands."
                    .to_string(),
                reason: "The shared audio frame was active but did not expose spectrum bands."
                    .to_string(),
            }]
        }
        Some(snapshot) if !snapshot.active || snapshot.muted => {
            vec![SceneAudioCoordinatorDiagnostic {
                severity: SceneAudioCoordinatorDiagnosticSeverity::Info,
                code: SHARED_AUDIO_IDLE_CODE.to_string(),
                runtime_stage: "shared-audio".to_string(),
                message: "Scene audio response is using silence because shared audio is idle."
                    .to_string(),
                reason: "Shared audio reported an inactive or muted frame.".to_string(),
            }]
        }
        Some(_) => Vec::new(),
    }
}

fn shared_audio_snapshot_active(snapshot: Option<&AudioSnapshot>) -> bool {
    snapshot
        .map(|snapshot| {
            snapshot.active
                && !snapshot.muted
                && !snapshot.smoothed_bands.is_empty()
                && snapshot
                    .smoothed_bands
                    .iter()
                    .any(|value| *value as f64 > MIN_ACTIVE_LEVEL)
        })
        .unwrap_or(false)
}

fn local_sound_levels_active(sound_levels: Option<&[f64]>) -> bool {
    sound_levels
        .map(|levels| levels.iter().any(|value| *value > MIN_ACTIVE_LEVEL))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::services::audio_input_service::AudioSnapshot;

    use super::{
        derive_scene_audio_levels, merge_scene_audio_levels, scene_audio_input_diagnostics,
        scene_audio_response_curve, SceneAudioCoordinator, SceneAudioCoordinatorConfig,
        SceneAudioCoordinatorDiagnosticSeverity,
    };

    fn snapshot_with_bands(bands: &[f32]) -> AudioSnapshot {
        AudioSnapshot {
            timestamp_ms: 42,
            bands: bands.to_vec(),
            smoothed_bands: bands.to_vec(),
            peak: 0.8,
            rms: 0.5,
            muted: false,
            active: true,
        }
    }

    #[test]
    fn derive_scene_audio_levels_shapes_shared_bands_into_requested_count() {
        let mut bands = vec![0.0_f32; 64];
        for (index, value) in bands.iter_mut().enumerate() {
            let position = index as f32 / 63.0;
            *value = (1.0 - (position * 2.0 - 1.0).abs()).powf(0.5);
        }
        let levels = derive_scene_audio_levels(Some(&snapshot_with_bands(&bands)), 12);

        assert_eq!(levels.len(), 12);
        assert!(levels.iter().all(|value| value.is_finite()));
        assert!(levels[5] > levels[0]);
        assert!(levels[6] > levels[11]);
    }

    #[test]
    fn scene_audio_response_curve_lifts_quiet_signal_without_overdriving_peak() {
        let quiet = scene_audio_response_curve(0.04);
        let medium = scene_audio_response_curve(0.35);
        let peak = scene_audio_response_curve(1.0);

        assert!(quiet > 0.04);
        assert!(medium > quiet);
        assert!(peak > 0.99);
        assert!(peak <= 1.0);
    }

    #[test]
    fn derive_scene_audio_levels_uses_perceptual_low_band_resolution() {
        let mut low_bands = vec![0.0_f32; 64];
        low_bands[0] = 1.0;
        low_bands[1] = 0.85;

        let levels = derive_scene_audio_levels(Some(&snapshot_with_bands(&low_bands)), 12);

        assert_eq!(levels.len(), 12);
        assert!(levels[0] > 0.5);
        assert!(levels[0] > levels[6]);
    }

    #[test]
    fn merge_scene_audio_levels_combines_shared_and_local_energy_consistently() {
        let merged = merge_scene_audio_levels(&[0.3, 0.0, 0.4], Some(&[0.0, 0.6, 0.5]), 3);

        assert!(merged[0] >= 0.3);
        assert!(merged[1] > 0.5);
        assert!(merged[2] > 0.6);
        assert!(merged.iter().all(|value| *value <= 1.0));
    }

    #[test]
    fn scene_audio_coordinator_merges_local_sound_levels_when_shared_audio_is_idle() {
        let mut coordinator = SceneAudioCoordinator::default();
        let levels = coordinator.levels_for_count(None, Some(&[0.2, 0.5, 0.1]), 3, 1000);

        assert_eq!(levels.len(), 3);
        assert!(levels[1] > levels[0]);
        assert!(levels[1] > 0.1);
    }

    #[test]
    fn scene_audio_coordinator_smooths_transitions_instead_of_jumping() {
        let mut coordinator = SceneAudioCoordinator::default();
        let hot = snapshot_with_bands(&vec![1.0; 64]);
        let first = coordinator.levels_for_count(Some(&hot), None, 8, 1000);
        let second =
            coordinator.levels_for_count(Some(&AudioSnapshot::silent(1100)), None, 8, 1100);

        assert!(first.iter().all(|value| *value > 0.0));
        assert!(second.iter().all(|value| *value > 0.0));
        assert!(second[3] < first[3]);
        assert!(second[3] > 0.1);
    }

    #[test]
    fn scene_audio_coordinator_uses_configured_attack_and_release_curves() {
        let mut coordinator = SceneAudioCoordinator::with_config(SceneAudioCoordinatorConfig {
            attack_response_rate: 18.0,
            decay_response_rate: 2.0,
            idle_release_rate: 1.0,
            ..SceneAudioCoordinatorConfig::default()
        });
        let quiet = snapshot_with_bands(&vec![0.05; 64]);
        let hot = snapshot_with_bands(&vec![1.0; 64]);

        let first = coordinator.levels_for_count(Some(&quiet), None, 4, 1000);
        let attacked = coordinator.levels_for_count(Some(&hot), None, 4, 1016);
        let released =
            coordinator.levels_for_count(Some(&AudioSnapshot::silent(1032)), None, 4, 1032);

        assert!(attacked[1] > first[1]);
        assert!(released[1] < attacked[1]);
        assert!(released[1] > 0.0);
    }

    #[test]
    fn scene_audio_coordinator_frame_reports_audio_input_boundaries() {
        let mut coordinator = SceneAudioCoordinator::default();
        let missing = coordinator.frame_for_count(None, None, 4, 1000);

        assert_eq!(missing.levels, vec![0.0; 4]);
        assert_eq!(
            missing.diagnostics[0].severity,
            SceneAudioCoordinatorDiagnosticSeverity::Warning
        );
        assert_eq!(missing.diagnostics[0].code, "shared-audio-snapshot-missing");

        let local = coordinator.frame_for_count(None, Some(&[0.6, 0.1, 0.0, 0.0]), 4, 1016);
        assert!(local.diagnostics.is_empty());
        assert!(local.local_sound_active);
    }

    #[test]
    fn scene_audio_input_diagnostics_distinguish_idle_from_malformed_frames() {
        let idle = scene_audio_input_diagnostics(Some(&AudioSnapshot::silent(1000)), None);
        assert_eq!(idle.len(), 1);
        assert_eq!(
            idle[0].severity,
            SceneAudioCoordinatorDiagnosticSeverity::Info
        );
        assert_eq!(idle[0].code, "shared-audio-idle");

        let malformed = AudioSnapshot {
            active: true,
            muted: false,
            smoothed_bands: Vec::new(),
            bands: Vec::new(),
            timestamp_ms: 1000,
            peak: 0.0,
            rms: 0.0,
        };
        let diagnostics = scene_audio_input_diagnostics(Some(&malformed), None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "shared-audio-bands-missing");
    }

    #[test]
    fn retain_counts_prunes_unused_audio_state_buckets() {
        let mut coordinator = SceneAudioCoordinator::default();
        let snapshot = snapshot_with_bands(&vec![0.5; 64]);
        let _ = coordinator.levels_for_count(Some(&snapshot), None, 8, 1000);
        let _ = coordinator.levels_for_count(Some(&snapshot), None, 16, 1000);

        coordinator.retain_counts(&BTreeSet::from([16]));

        let levels = coordinator.levels_for_count(Some(&snapshot), None, 16, 1020);
        assert_eq!(levels.len(), 16);
        let reinitialized = coordinator.levels_for_count(Some(&snapshot), None, 8, 1020);
        assert_eq!(reinitialized.len(), 8);
    }
}
