use std::collections::{BTreeMap, BTreeSet};

use crate::services::audio_input_service::AudioSnapshot;

const MIN_ACTIVE_LEVEL: f64 = 0.0005;
const MIN_FRAME_DELTA_MS: u64 = 16;
const ATTACK_RESPONSE_RATE: f64 = 11.0;
const DECAY_RESPONSE_RATE: f64 = 5.5;
const IDLE_RELEASE_RATE: f64 = 7.0;

#[derive(Debug, Default)]
pub struct SceneAudioCoordinator {
    states: BTreeMap<usize, SceneAudioBandState>,
}

#[derive(Debug, Default)]
struct SceneAudioBandState {
    smoothed: Vec<f64>,
    last_updated_ms: Option<u64>,
}

impl SceneAudioCoordinator {
    pub fn levels_for_count(
        &mut self,
        snapshot: Option<&AudioSnapshot>,
        sound_levels: Option<&[f64]>,
        count: usize,
        now_ms: u64,
    ) -> Vec<f64> {
        if count == 0 {
            return Vec::new();
        }

        let target_levels = merge_scene_audio_levels(
            &derive_scene_audio_levels(snapshot, count),
            sound_levels,
            count,
        );
        let state = self.states.entry(count).or_default();
        let reinitialized = state.smoothed.len() != count || state.last_updated_ms.is_none();
        if reinitialized {
            state.smoothed = target_levels.clone();
            state.last_updated_ms = Some(now_ms);
            return state.smoothed.clone();
        }

        let delta_ms = state
            .last_updated_ms
            .map(|previous| now_ms.saturating_sub(previous).max(MIN_FRAME_DELTA_MS))
            .unwrap_or(MIN_FRAME_DELTA_MS);
        state.last_updated_ms = Some(now_ms);

        for (index, current) in state.smoothed.iter_mut().enumerate() {
            let target = target_levels.get(index).copied().unwrap_or_default();
            let response_rate = if target > *current {
                ATTACK_RESPONSE_RATE
            } else if target > MIN_ACTIVE_LEVEL {
                DECAY_RESPONSE_RATE
            } else {
                IDLE_RELEASE_RATE
            };
            let factor = smoothing_factor(delta_ms, response_rate);
            *current += (target - *current) * factor;
            if target <= MIN_ACTIVE_LEVEL {
                *current *= 1.0 - factor * 0.42;
            }
            *current = current.clamp(0.0, 1.0);
        }

        state.smoothed.clone()
    }

    pub fn retain_counts(&mut self, active_counts: &BTreeSet<usize>) {
        self.states.retain(|count, _| active_counts.contains(count));
    }

    pub fn reset(&mut self) {
        self.states.clear();
    }
}

pub fn derive_scene_audio_levels(snapshot: Option<&AudioSnapshot>, count: usize) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }

    let Some(snapshot) = snapshot else {
        return vec![0.0; count];
    };
    if !snapshot.active || snapshot.smoothed_bands.is_empty() {
        return vec![0.0; count];
    }

    (0..count)
        .map(|index| {
            let (start, end) = scene_audio_window(index, count, snapshot.smoothed_bands.len());
            let mut sum = 0.0_f64;
            let mut sum_squares = 0.0_f64;
            let mut peak = 0.0_f64;
            let mut samples = 0.0_f64;

            for band in start..end {
                let value = snapshot
                    .smoothed_bands
                    .get(band)
                    .copied()
                    .unwrap_or_default() as f64;
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
            let shaped = peak * 0.48 + rms * 0.32 + average * 0.20;
            let response = scene_audio_response_curve(shaped);
            let position = if count == 1 {
                0.5
            } else {
                index as f64 / (count - 1) as f64
            };
            (response * scene_audio_band_envelope(position)).clamp(0.0, 1.0)
        })
        .collect()
}

pub fn merge_scene_audio_levels(
    shared_levels: &[f64],
    sound_levels: Option<&[f64]>,
    count: usize,
) -> Vec<f64> {
    (0..count)
        .map(|index| {
            let shared = shared_levels.get(index).copied().unwrap_or_default();
            let sound = sound_levels
                .and_then(|levels| levels.get(index).copied())
                .unwrap_or_default();
            shared.max(sound).clamp(0.0, 1.0)
        })
        .collect()
}

fn scene_audio_window(index: usize, count: usize, source_len: usize) -> (usize, usize) {
    let start = (index * source_len) / count.max(1);
    let mut end = ((index + 1) * source_len) / count.max(1);
    if end <= start {
        end = (start + 1).min(source_len);
    }
    (start.min(source_len), end.min(source_len))
}

fn scene_audio_response_curve(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    let lifted = value.powf(0.82);
    let floor = if value > MIN_ACTIVE_LEVEL { 0.03 } else { 0.0 };
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::services::audio_input_service::AudioSnapshot;

    use super::{derive_scene_audio_levels, SceneAudioCoordinator};

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
