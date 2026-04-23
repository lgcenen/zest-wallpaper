import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type {
  PlayerRuntimeState,
  RuntimeDiagnostic,
  SharedAudioSnapshot,
  SharedInputSnapshot,
  WallpaperRuntimeRecord,
} from "../types";

export function getPlayerState() {
  return invoke<PlayerRuntimeState>("get_player_state");
}

export function getPlayerInputSnapshot() {
  return invoke<SharedInputSnapshot>("get_player_input_snapshot");
}

export function getPlayerAudioSnapshot() {
  return invoke<SharedAudioSnapshot>("get_player_audio_snapshot");
}

export function getPlayerDiagnostics() {
  return invoke<RuntimeDiagnostic[]>("get_player_diagnostics");
}

export function setSceneAudioInterest(active: boolean) {
  return invoke<void>("set_scene_audio_interest", { active });
}

export function onPlayerLoad(
  handler: (wallpaper: WallpaperRuntimeRecord | null) => void,
) {
  return listen<WallpaperRuntimeRecord | null>("player:load", (event) => {
    handler(event.payload);
  });
}

export function onPlayerPause(handler: (paused: boolean) => void) {
  return listen<boolean>("player:pause", (event) => {
    handler(event.payload);
  });
}

export function onPlayerUpdate(handler: (wallpaper: WallpaperRuntimeRecord) => void) {
  return listen<WallpaperRuntimeRecord>("player:update", (event) => {
    handler(event.payload);
  });
}

export function onPlayerInput(handler: (snapshot: SharedInputSnapshot) => void) {
  return listen<SharedInputSnapshot>("player:input", (event) => {
    handler(event.payload);
  });
}

export function onPlayerAudio(handler: (snapshot: SharedAudioSnapshot) => void) {
  return listen<SharedAudioSnapshot>("player:audio", (event) => {
    handler(event.payload);
  });
}

export function onPlayerDiagnostics(handler: (diagnostics: RuntimeDiagnostic[]) => void) {
  return listen<RuntimeDiagnostic[]>("player:diagnostic", (event) => {
    handler(event.payload);
  });
}
