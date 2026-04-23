import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

async function chooseDirectory(title: string) {
  const selected = await open({
    directory: true,
    multiple: false,
    title,
  });
  if (!selected || Array.isArray(selected)) {
    return null;
  }
  return selected;
}

export function chooseImportDirectory(title?: string) {
  return chooseDirectory(title ?? "选择 Windows 壁纸目录");
}

export function chooseSceneAssetsDirectory(title?: string) {
  return chooseDirectory(title ?? "选择 Scene 外部 assets 目录");
}

export function toAssetUrl(path?: string | null) {
  if (!path) {
    return null;
  }
  return convertFileSrc(path);
}
