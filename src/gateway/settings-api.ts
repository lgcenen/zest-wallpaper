import { invoke } from "@tauri-apps/api/core";
import type { SceneRuntimeSettingsSnapshot } from "../types";

export function getSceneRuntimeSettings() {
  return invoke<SceneRuntimeSettingsSnapshot>("get_scene_runtime_settings");
}

export function setSceneExternalAssetsPath(path: string | null) {
  return invoke<SceneRuntimeSettingsSnapshot>("set_scene_external_assets_path", { path });
}
