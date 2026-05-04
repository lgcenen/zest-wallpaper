import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import type { SceneRuntimeSettingsSnapshot } from "../types";

export function getAppVersion() {
  return getVersion();
}

export function getSceneRuntimeSettings() {
  return invoke<SceneRuntimeSettingsSnapshot>("get_scene_runtime_settings");
}

export function setSceneExternalAssetsPath(path: string | null) {
  return invoke<SceneRuntimeSettingsSnapshot>("set_scene_external_assets_path", { path });
}

export function setCacheStoragePath(path: string | null) {
  return invoke<SceneRuntimeSettingsSnapshot>("set_cache_storage_path", { path });
}

export function clearSceneCache() {
  return invoke<void>("clear_scene_cache");
}

export function getSceneCacheSize() {
  return invoke<number>("get_scene_cache_size");
}

export function fetchExternalImage(url: string) {
  return invoke<string>("fetch_external_image", { url });
}
