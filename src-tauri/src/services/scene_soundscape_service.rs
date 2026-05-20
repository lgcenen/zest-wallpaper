#[cfg(target_os = "macos")]
use std::{cell::RefCell, collections::BTreeMap};

#[cfg(target_os = "macos")]
use dispatch2::{run_on_main, MainThreadBound};
#[cfg(target_os = "macos")]
use objc2::{rc::Retained, AnyThread, MainThreadMarker};
#[cfg(target_os = "macos")]
use objc2_avf_audio::AVAudioPlayer;
use tauri::AppHandle;

use crate::{
    services::{
        runtime_audio_settings_service::effective_output_volume,
        scene_render_planner_service::SceneRenderSoundItem,
        scene_sound_lifecycle_service::{
            plan_scene_sound_clear, plan_scene_sound_lifecycle, scene_sound_meter_level,
            scene_sound_playback_load_warning, scene_sound_reactive_levels,
            SceneSoundLifecycleAction, SceneSoundMeterReading, SceneSoundPlaybackWarning,
            SceneSoundRuntimeState,
        },
    },
};

#[cfg(target_os = "macos")]
use objc2_foundation::{NSString, NSURL};

pub(crate) type SceneSoundPlaybackWarningMapper<W> = fn(SceneSoundPlaybackWarning) -> W;

#[cfg(target_os = "macos")]
pub(crate) struct NativeSceneSoundscape {
    players: RefCell<BTreeMap<u32, NativeSceneSoundPlayer>>,
    output_volume: RefCell<f64>,
}

#[cfg(target_os = "macos")]
impl Default for NativeSceneSoundscape {
    fn default() -> Self {
        Self {
            players: RefCell::new(BTreeMap::new()),
            output_volume: RefCell::new(1.0),
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeSceneSoundPlayer {
    state: SceneSoundRuntimeState,
    player: Retained<AVAudioPlayer>,
}

#[cfg(target_os = "macos")]
impl NativeSceneSoundscape {
    pub(crate) fn sync<W>(
        &self,
        sounds: &[SceneRenderSoundItem],
        paused: bool,
        output_volume: f64,
        warning_mapper: SceneSoundPlaybackWarningMapper<W>,
    ) -> Vec<W> {
        self.set_output_volume(output_volume);
        let current_states = self.playback_states();
        let actions = plan_scene_sound_lifecycle(&current_states, sounds, paused);
        let sounds_by_id = sounds
            .iter()
            .map(|sound| (sound.object_id, sound))
            .collect::<BTreeMap<_, _>>();
        let mut players = self.players.borrow_mut();
        let mut warnings = Vec::new();

        for action in actions {
            match action {
                SceneSoundLifecycleAction::Start { state } => {
                    if let Some(sound) = sounds_by_id.get(&state.object_id) {
                        match create_sound_player(sound, state.paused, self.current_output_volume())
                        {
                            Ok(player) => {
                                players.insert(state.object_id, player);
                            }
                            Err(error) => warnings.push(warning_mapper(
                                scene_sound_playback_load_warning(sound, error),
                            )),
                        }
                    }
                }
                SceneSoundLifecycleAction::Replace { previous: _, state } => {
                    if let Some(existing) = players.remove(&state.object_id) {
                        stop_sound_player(&existing);
                    }
                    if let Some(sound) = sounds_by_id.get(&state.object_id) {
                        match create_sound_player(sound, state.paused, self.current_output_volume())
                        {
                            Ok(player) => {
                                players.insert(state.object_id, player);
                            }
                            Err(error) => warnings.push(warning_mapper(
                                scene_sound_playback_load_warning(sound, error),
                            )),
                        }
                    }
                }
                SceneSoundLifecycleAction::Update { state } => {
                    if let Some(player) = players.get_mut(&state.object_id) {
                        configure_sound_player(player, &state, self.current_output_volume());
                    }
                }
                SceneSoundLifecycleAction::SetPaused { object_id, paused } => {
                    if let Some(player) = players.get_mut(&object_id) {
                        player.state.paused = paused;
                        set_sound_player_paused(player, paused);
                    }
                }
                SceneSoundLifecycleAction::Stop { state } => {
                    if let Some(player) = players.remove(&state.object_id) {
                        stop_sound_player(&player);
                    }
                }
            }
        }

        warnings
    }

    pub(crate) fn clear(&self) {
        let actions = plan_scene_sound_clear(&self.playback_states());
        let mut players = self.players.borrow_mut();
        for action in actions {
            if let SceneSoundLifecycleAction::Stop { state } = action {
                if let Some(player) = players.remove(&state.object_id) {
                    stop_sound_player(&player);
                }
            }
        }
    }

    pub(crate) fn reactive_levels(&self, count: usize) -> Option<Vec<f64>> {
        if count == 0 {
            return None;
        }

        let current = self.players.borrow();
        let mut meters = Vec::new();
        for player in current.values() {
            if !unsafe { player.player.isPlaying() } {
                continue;
            }

            unsafe {
                player.player.updateMeters();
            }
            let channel_count = unsafe { player.player.numberOfChannels() }.max(1) as usize;
            let mut level = 0.0_f64;
            for channel in 0..channel_count {
                let average = unsafe { player.player.averagePowerForChannel(channel) } as f64;
                let peak = unsafe { player.player.peakPowerForChannel(channel) } as f64;
                level = level.max(scene_sound_meter_level(average, peak));
            }
            if level <= 0.001 {
                continue;
            }

            let phase = unsafe { player.player.currentTime() } as f64;
            meters.push(SceneSoundMeterReading { level, phase });
        }

        scene_sound_reactive_levels(&meters, count)
    }

    pub(crate) fn set_output_volume(&self, output_volume: f64) {
        let normalized = output_volume.clamp(0.0, 1.0);
        *self.output_volume.borrow_mut() = normalized;
        for player in self.players.borrow_mut().values_mut() {
            unsafe {
                player
                    .player
                    .setVolume(effective_output_volume(player.state.volume, normalized) as f32);
            }
        }
    }

    fn current_output_volume(&self) -> f64 {
        *self.output_volume.borrow()
    }

    fn playback_states(&self) -> BTreeMap<u32, SceneSoundRuntimeState> {
        self.players
            .borrow()
            .iter()
            .map(|(object_id, player)| (*object_id, player.state.clone()))
            .collect()
    }
}

#[cfg(target_os = "macos")]
fn create_sound_player(
    sound: &SceneRenderSoundItem,
    paused: bool,
    output_volume: f64,
) -> Result<NativeSceneSoundPlayer, String> {
    let Some(path_string) = sound.asset_path.to_str() else {
        return Err(format!(
            "sound path {} is not valid UTF-8 for AVFoundation",
            sound.asset_path.display()
        ));
    };
    let path_string = NSString::from_str(path_string);
    let url = NSURL::fileURLWithPath(&path_string);
    let player =
        unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
            .map_err(|error| format!("{error:?}"))?;
    unsafe {
        player.setVolume(effective_output_volume(sound.volume, output_volume) as f32);
        player.setNumberOfLoops(if sound.looped { -1 } else { 0 });
        player.setMeteringEnabled(true);
        let _ = player.prepareToPlay();
    }
    let mut sound_player = NativeSceneSoundPlayer {
        state: SceneSoundRuntimeState::from_sound_item(sound, paused),
        player,
    };
    set_sound_player_paused(&mut sound_player, paused);
    Ok(sound_player)
}

#[cfg(target_os = "macos")]
fn configure_sound_player(
    player: &mut NativeSceneSoundPlayer,
    state: &SceneSoundRuntimeState,
    output_volume: f64,
) {
    unsafe {
        player
            .player
            .setVolume(effective_output_volume(state.volume, output_volume) as f32);
        player
            .player
            .setNumberOfLoops(if state.looped { -1 } else { 0 });
    }
    player.state.looped = state.looped;
    player.state.volume = state.volume;
}

#[cfg(target_os = "macos")]
fn set_sound_player_paused(player: &mut NativeSceneSoundPlayer, paused: bool) {
    unsafe {
        if paused {
            player.player.pause();
        } else if !player.player.isPlaying() {
            let _ = player.player.play();
        }
    }
    player.state.paused = paused;
}

#[cfg(target_os = "macos")]
fn stop_sound_player(player: &NativeSceneSoundPlayer) {
    unsafe {
        player.player.stop();
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn sync_scene_soundscape<W: Send>(
    slot: &std::sync::Mutex<Option<MainThreadBound<NativeSceneSoundscape>>>,
    output_volume: f64,
    sounds: Option<(&[SceneRenderSoundItem], bool)>,
    warning_mapper: SceneSoundPlaybackWarningMapper<W>,
) -> Result<Vec<W>, String> {
    run_on_main(|mtm| {
        let mut slot = slot.lock().map_err(|error| error.to_string())?;
        if slot.is_none() {
            slot.replace(MainThreadBound::new(NativeSceneSoundscape::default(), mtm));
        }
        let soundscape = slot
            .as_ref()
            .expect("soundscape should exist after initialization")
            .get(mtm);
        Ok(match sounds {
            Some((sounds, paused)) => {
                soundscape.sync(sounds, paused, output_volume, warning_mapper)
            }
            None => {
                soundscape.clear();
                Vec::new()
            }
        })
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn set_scene_soundscape_output_volume(
    slot: &std::sync::Mutex<Option<MainThreadBound<NativeSceneSoundscape>>>,
    output_volume: f64,
) -> Result<(), String> {
    run_on_main(|mtm| {
        let slot = slot.lock().map_err(|error| error.to_string())?;
        if let Some(soundscape) = slot.as_ref() {
            soundscape.get(mtm).set_output_volume(output_volume);
        }
        Ok(())
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn scene_soundscape_audio_levels(
    app: &AppHandle,
    slot: &std::sync::Mutex<Option<MainThreadBound<NativeSceneSoundscape>>>,
    count: usize,
) -> Option<Vec<f64>> {
    let _ = app;
    let mtm = MainThreadMarker::new()?;
    let soundscape = slot.lock().ok()?;
    let soundscape = soundscape.as_ref()?;
    soundscape.get(mtm).reactive_levels(count)
}
