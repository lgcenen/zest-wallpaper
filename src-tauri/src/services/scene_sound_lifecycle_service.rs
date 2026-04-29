use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::services::scene_render_planner_service::SceneRenderSoundItem;

const VOLUME_EPSILON: f64 = 0.0005;
const SOUND_METER_FLOOR_DB: f64 = -80.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SceneSoundRuntimeState {
    pub object_id: u32,
    pub object_name: String,
    pub asset_path: PathBuf,
    pub looped: bool,
    pub volume: f64,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SceneSoundLifecycleAction {
    Start {
        state: SceneSoundRuntimeState,
    },
    Replace {
        previous: SceneSoundRuntimeState,
        state: SceneSoundRuntimeState,
    },
    Update {
        state: SceneSoundRuntimeState,
    },
    SetPaused {
        object_id: u32,
        paused: bool,
    },
    Stop {
        state: SceneSoundRuntimeState,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSoundMeterReading {
    pub level: f64,
    pub phase: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSoundPlaybackWarning {
    pub code: String,
    pub object_id: u32,
    pub object_name: String,
    pub asset_path: PathBuf,
    pub runtime_stage: String,
    pub message: String,
    pub reason: String,
}

impl SceneSoundRuntimeState {
    pub fn from_sound_item(sound: &SceneRenderSoundItem, paused: bool) -> Self {
        Self {
            object_id: sound.object_id,
            object_name: sound.object_name.clone(),
            asset_path: sound.asset_path.clone(),
            looped: sound.looped,
            volume: sound.volume.clamp(0.0, 1.0),
            paused,
        }
    }
}

pub fn plan_scene_sound_lifecycle(
    current: &BTreeMap<u32, SceneSoundRuntimeState>,
    desired: &[SceneRenderSoundItem],
    paused: bool,
) -> Vec<SceneSoundLifecycleAction> {
    let desired_states = desired
        .iter()
        .map(|sound| {
            (
                sound.object_id,
                SceneSoundRuntimeState::from_sound_item(sound, paused),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let desired_ids = desired_states.keys().copied().collect::<BTreeSet<_>>();
    let mut actions = Vec::new();

    for (object_id, state) in desired_states {
        match current.get(&object_id) {
            None => actions.push(SceneSoundLifecycleAction::Start { state }),
            Some(previous) if previous.asset_path != state.asset_path => {
                actions.push(SceneSoundLifecycleAction::Replace {
                    previous: previous.clone(),
                    state,
                });
            }
            Some(previous) => {
                if previous.looped != state.looped
                    || (previous.volume - state.volume).abs() > VOLUME_EPSILON
                {
                    actions.push(SceneSoundLifecycleAction::Update {
                        state: state.clone(),
                    });
                }
                if previous.paused != state.paused {
                    actions.push(SceneSoundLifecycleAction::SetPaused {
                        object_id,
                        paused: state.paused,
                    });
                }
            }
        }
    }

    for (object_id, state) in current {
        if !desired_ids.contains(object_id) {
            actions.push(SceneSoundLifecycleAction::Stop {
                state: state.clone(),
            });
        }
    }

    actions
}

pub fn plan_scene_sound_clear(
    current: &BTreeMap<u32, SceneSoundRuntimeState>,
) -> Vec<SceneSoundLifecycleAction> {
    current
        .values()
        .cloned()
        .map(|state| SceneSoundLifecycleAction::Stop { state })
        .collect()
}

pub fn scene_sound_playback_load_warning(
    sound: &SceneRenderSoundItem,
    reason: impl Into<String>,
) -> SceneSoundPlaybackWarning {
    SceneSoundPlaybackWarning {
        code: "sound-playback-load-failed".to_string(),
        object_id: sound.object_id,
        object_name: sound.object_name.clone(),
        asset_path: sound.asset_path.clone(),
        runtime_stage: "sound-lifecycle-sync".to_string(),
        message: format!(
            "Scene sound {} could not be prepared for native playback.",
            sound.asset_path.display()
        ),
        reason: reason.into(),
    }
}

pub fn scene_sound_meter_level(average_db: f64, peak_db: f64) -> f64 {
    let average = decibels_to_scene_sound_level(average_db);
    let peak = decibels_to_scene_sound_level(peak_db);
    clamp_f64((peak * 0.68 + average * 0.32).powf(0.7), 0.0, 1.0)
}

pub fn scene_sound_reactive_levels(
    meters: &[SceneSoundMeterReading],
    count: usize,
) -> Option<Vec<f64>> {
    if count == 0 || meters.is_empty() {
        return None;
    }

    Some(scene_sound_level_bands(meters, count))
}

fn decibels_to_scene_sound_level(power_db: f64) -> f64 {
    if !power_db.is_finite() || power_db <= SOUND_METER_FLOOR_DB {
        return 0.0;
    }
    clamp_f64(10_f64.powf(power_db / 20.0), 0.0, 1.0)
}

fn scene_sound_level_bands(meters: &[SceneSoundMeterReading], count: usize) -> Vec<f64> {
    (0..count)
        .map(|index| {
            let position = if count == 1 {
                0.5
            } else {
                index as f64 / (count - 1) as f64
            };
            let edge_envelope = (1.0 - (position * 2.0 - 1.0).abs()).powf(0.32);
            let mut level = 0.0_f64;
            for meter in meters {
                let ripple_a = ((meter.phase * 3.1) + position * 8.0).sin().abs();
                let ripple_b = ((meter.phase * 5.7) + position * 17.0).cos().abs();
                let ripple = ripple_a * 0.55 + ripple_b * 0.45;
                let shaped = meter.level * (0.48 + edge_envelope * 0.52) * (0.42 + ripple * 0.58);
                level = level.max(shaped);
            }
            clamp_f64(level, 0.0, 1.0)
        })
        .collect()
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

#[cfg(test)]
mod tests {
    use super::{
        plan_scene_sound_clear, plan_scene_sound_lifecycle, scene_sound_meter_level,
        scene_sound_playback_load_warning, scene_sound_reactive_levels, SceneSoundLifecycleAction,
        SceneSoundMeterReading, SceneSoundRuntimeState,
    };
    use crate::services::scene_render_planner_service::SceneRenderSoundItem;
    use std::{collections::BTreeMap, path::PathBuf};

    fn sound(object_id: u32, asset_path: &str, looped: bool, volume: f64) -> SceneRenderSoundItem {
        SceneRenderSoundItem {
            object_id,
            object_name: format!("Sound {object_id}"),
            asset_path: PathBuf::from(asset_path),
            looped,
            volume,
        }
    }

    fn state(
        object_id: u32,
        asset_path: &str,
        looped: bool,
        volume: f64,
        paused: bool,
    ) -> SceneSoundRuntimeState {
        SceneSoundRuntimeState::from_sound_item(
            &sound(object_id, asset_path, looped, volume),
            paused,
        )
    }

    #[test]
    fn sound_lifecycle_plans_pause_resume_switch_and_clear_without_residual_tracks() {
        let initial = vec![sound(7, "sounds/ambient-a.m4a", true, 0.8)];
        let actions = plan_scene_sound_lifecycle(&BTreeMap::new(), &initial, false);
        assert_eq!(
            actions,
            vec![SceneSoundLifecycleAction::Start {
                state: state(7, "sounds/ambient-a.m4a", true, 0.8, false)
            }]
        );

        let current = BTreeMap::from([(7, state(7, "sounds/ambient-a.m4a", true, 0.8, false))]);
        let pause_actions = plan_scene_sound_lifecycle(&current, &initial, true);
        assert_eq!(
            pause_actions,
            vec![SceneSoundLifecycleAction::SetPaused {
                object_id: 7,
                paused: true
            }]
        );

        let paused = BTreeMap::from([(7, state(7, "sounds/ambient-a.m4a", true, 0.8, true))]);
        let resume_actions = plan_scene_sound_lifecycle(&paused, &initial, false);
        assert_eq!(
            resume_actions,
            vec![SceneSoundLifecycleAction::SetPaused {
                object_id: 7,
                paused: false
            }]
        );

        let switched = vec![sound(7, "sounds/ambient-b.m4a", true, 0.8)];
        let switch_actions = plan_scene_sound_lifecycle(&current, &switched, false);
        assert_eq!(
            switch_actions,
            vec![SceneSoundLifecycleAction::Replace {
                previous: state(7, "sounds/ambient-a.m4a", true, 0.8, false),
                state: state(7, "sounds/ambient-b.m4a", true, 0.8, false)
            }]
        );

        let clear_actions = plan_scene_sound_clear(&current);
        assert_eq!(
            clear_actions,
            vec![SceneSoundLifecycleAction::Stop {
                state: state(7, "sounds/ambient-a.m4a", true, 0.8, false)
            }]
        );
    }

    #[test]
    fn sound_only_scene_starts_from_sound_items_without_visual_dependencies() {
        let desired = vec![sound(11, "sounds/only.m4a", true, 1.0)];
        let actions = plan_scene_sound_lifecycle(&BTreeMap::new(), &desired, false);

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            SceneSoundLifecycleAction::Start {
                ref state
            } if state.object_id == 11 && state.asset_path == PathBuf::from("sounds/only.m4a")
        ));
    }

    #[test]
    fn sound_lifecycle_updates_volume_and_looping_without_restarting_track() {
        let current = BTreeMap::from([(5, state(5, "sounds/tone.m4a", true, 0.3, false))]);
        let desired = vec![sound(5, "sounds/tone.m4a", false, 0.7)];

        let actions = plan_scene_sound_lifecycle(&current, &desired, false);

        assert_eq!(
            actions,
            vec![SceneSoundLifecycleAction::Update {
                state: state(5, "sounds/tone.m4a", false, 0.7, false)
            }]
        );
    }

    #[test]
    fn sound_meter_and_reactive_levels_are_available_for_audio_bars() {
        let quiet = scene_sound_meter_level(-48.0, -42.0);
        let loud = scene_sound_meter_level(-12.0, -6.0);

        assert!(quiet > 0.0);
        assert!(loud > quiet);
        assert!(loud <= 1.0);

        let levels = scene_sound_reactive_levels(
            &[SceneSoundMeterReading {
                level: 0.62,
                phase: 3.25,
            }],
            12,
        )
        .expect("sound meter levels");

        assert_eq!(levels.len(), 12);
        assert!(levels.iter().all(|value| *value > 0.05));
        assert!(levels.iter().any(|value| *value > 0.3));
        assert!(levels[5] > levels[0]);
    }

    #[test]
    fn sound_runtime_warning_keeps_object_and_stage_context() {
        let warning =
            scene_sound_playback_load_warning(&sound(21, "sounds/missing.m4a", true, 1.0), "boom");

        assert_eq!(warning.code, "sound-playback-load-failed");
        assert_eq!(warning.object_id, 21);
        assert_eq!(warning.object_name, "Sound 21");
        assert_eq!(warning.runtime_stage, "sound-lifecycle-sync");
        assert_eq!(warning.reason, "boom");
    }
}
