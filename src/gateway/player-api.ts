import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type {
  PlayerRuntimeState,
  RuntimeDiagnostic,
  WallpaperRuntimeRecord,
} from "../types";

export function getPlayerState() {
  return invoke<PlayerRuntimeState>("get_player_state");
}

export function getPlayerDiagnostics() {
  return invoke<RuntimeDiagnostic[]>("get_player_diagnostics");
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

export function onPlayerDiagnostics(handler: (diagnostics: RuntimeDiagnostic[]) => void) {
  return listen<RuntimeDiagnostic[]>("player:diagnostic", (event) => {
    handler(event.payload);
  });
}
