import { invoke } from "@tauri-apps/api/core";
import type { WallpaperRuntimeRecord } from "../types";

export function listWallpapers() {
  return invoke<WallpaperRuntimeRecord[]>("list_wallpapers");
}

export function importWallpaper(path: string) {
  return invoke<WallpaperRuntimeRecord>("import_wallpaper", { path });
}

export function getWallpaperDetails(id: string) {
  return invoke<WallpaperRuntimeRecord>("get_wallpaper_details", { id });
}

export function applyDynamicWallpaper(id: string) {
  return invoke<WallpaperRuntimeRecord>("apply_dynamic_wallpaper", { id });
}

export function setWallpaperProperties(id: string, values: Record<string, unknown>) {
  return invoke<WallpaperRuntimeRecord>("set_wallpaper_properties", { id, values });
}

export function pauseResumeDynamic(paused: boolean) {
  return invoke<boolean>("pause_resume_dynamic", { paused });
}

export function removeWallpaper(id: string) {
  return invoke<boolean>("remove_wallpaper", { id });
}
