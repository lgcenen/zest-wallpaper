use std::collections::BTreeSet;

use tauri::{AppHandle, Emitter};

use crate::store::{AppState, DynamicPlayerState};

use super::{lifecycle_service, scene_update};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PauseTransition {
    pub(super) effective_paused: bool,
    pub(super) effective_changed: bool,
    pub(super) manual_changed: bool,
    pub(super) manual_paused: bool,
}

pub fn pause_resume_dynamic(
    paused: bool,
    app: &AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    set_player_paused(paused, app, state)
}

pub(crate) fn set_player_paused(
    paused: bool,
    app: &AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    let transition = {
        let mut player = state.player.lock().map_err(|error| error.to_string())?;
        let transition = apply_pause_change(&mut player, Some(paused), None);
        if transition.manual_changed {
            super::apply_flow::persist_player_state(&player);
        }
        transition
    };

    lifecycle_service::sync_pause_menu_state(app, transition.manual_paused);
    if transition.effective_changed {
        scene_update::sync_native_runtime_for_active_wallpaper(app, state)?;
        app.emit("player:pause", transition.effective_paused)
            .map_err(|error| error.to_string())?;
    }
    Ok(transition.effective_paused)
}

pub(crate) fn set_player_auto_pause_screen_labels(
    labels: BTreeSet<String>,
    app: &AppHandle,
    state: &AppState,
) -> Result<bool, String> {
    let transition = {
        let mut player = state.player.lock().map_err(|error| error.to_string())?;
        apply_pause_change(&mut player, None, Some(labels))
    };

    if transition.effective_changed {
        scene_update::sync_native_runtime_for_active_wallpaper(app, state)?;
        app.emit("player:pause", transition.effective_paused)
            .map_err(|error| error.to_string())?;
    }
    Ok(transition.effective_paused)
}

pub(super) fn apply_pause_change(
    player: &mut DynamicPlayerState,
    manual: Option<bool>,
    auto_pause_screen_labels: Option<BTreeSet<String>>,
) -> PauseTransition {
    let previous_manual = player.manually_paused;
    let previous_effective = player.effective_paused();

    if player.active_id.is_none() {
        player.manually_paused = false;
        player.auto_pause_screen_labels.clear();
    } else {
        if let Some(value) = manual {
            player.manually_paused = value;
        }
        if let Some(labels) = auto_pause_screen_labels {
            player.auto_pause_screen_labels = labels;
        }
    }

    PauseTransition {
        effective_paused: player.effective_paused(),
        effective_changed: previous_effective != player.effective_paused(),
        manual_changed: previous_manual != player.manually_paused,
        manual_paused: player.manually_paused,
    }
}
