import type { RuntimeDiagnostic, WallpaperType } from "../types";
import type {
  WorkbenchLanguage,
  WorkbenchResolvedTheme,
  WorkbenchSortKey,
  WorkbenchThemeMode,
} from "../state/workbench-preferences";

export type WorkbenchBannerTone = "neutral" | "success" | "warning";
export type WorkbenchBannerKey =
  | "dropHint"
  | "libraryReadFailed"
  | "sceneAssetsReadFailed"
  | "dropWithoutPath"
  | "importing"
  | "importSuccess"
  | "importFailed"
  | "applySuccess"
  | "applyFailed"
  | "propertySaved"
  | "propertySaveFailed"
  | "playerPaused"
  | "playerResumed"
  | "playerToggleFailed"
  | "sceneAssetsMounted"
  | "sceneAssetsCleared"
  | "sceneAssetsUpdateFailed"
  | "cacheCleared"
  | "cacheClearFailed"
  | "cachePathSetFailed"
  | "removeSuccess"
  | "removeFailed";

export interface WorkbenchBannerState {
  tone: WorkbenchBannerTone;
  key: WorkbenchBannerKey;
  values?: Record<string, string | number>;
}

export interface WorkbenchCopy {
  toolbarEyebrow: string;
  toolbarTitle: string;
  toolbarSummary: (visible: number, total: number, filtering: boolean) => string;
  importAction: string;
  sortAction: string;
  filterTypeAction: string;
  filterTagAction: string;
  filterAllTypes: string;
  filterAllTags: string;
  searchAction: string;
  searchPlaceholder: string;
  settingsAction: string;
  settingsTabGeneral: string;
  settingsTabAbout: string;
  aboutHeading: string;
  copyrightLabel: string;
  versionLabel: string;
  qqGroupLabel: string;
  cacheLabel: string;
  cacheBrowseAction: string;
  cacheClearAction: string;
  cachePathUnset: string;
  cachePathMounted: string;
  cachePathMissing: string;
  cacheStorageHint: string;
  cacheStorageLoading: string;
  cacheStorageSaving: string;
  cacheClearing: string;
  emptyLibraryTitle: string;
  emptyLibraryBody: string;
  emptySearchTitle: string;
  emptySearchBody: string;
  detailEyebrow: string;
  detailWaitingTitle: string;
  detailWaitingBody: string;
  propertiesHeading: string;
  propertiesFallbackSection: string;
  propertiesSummary: (count: number) => string;
  propertiesEmpty: string;
  propertiesHidden: string;
  pauseAction: string;
  resumeAction: string;
  removeAction: string;
  desktopLabel: string;
  applying: string;
  applyReady: string;
  applyLive: string;
  applyFailed: string;
  runtimeDiagnosticLabel: string;
  runtimeDiagnosticMessage: (diagnostic: RuntimeDiagnostic) => string;
  sceneAssetsLabel: string;
  sceneAssetsBrowseAction: string;
  sceneAssetsClearAction: string;
  sceneAssetsUnset: string;
  sceneAssetsMounted: string;
  sceneAssetsMissing: string;
  sceneAssetsHint: string;
  sceneAssetsLoading: string;
  sceneAssetsSaving: string;
  guiOpacityLabel: string;
  themeModeLabel: string;
  languageLabel: string;
  defaultSortLabel: string;
  guiOpacityValue: (value: number) => string;
  themeModeName: (mode: WorkbenchThemeMode) => string;
  languageName: (value: WorkbenchLanguage) => string;
  sortName: (value: WorkbenchSortKey) => string;
  themeResolved: (mode: WorkbenchThemeMode, resolved: WorkbenchResolvedTheme) => string;
  typeLabel: (type: WallpaperType) => string;
  objectCount: (count: number) => string;
  bannerMessage: (banner: WorkbenchBannerState) => string;
}

const zhCnCopy: WorkbenchCopy = {
  toolbarEyebrow: "Workbench",
  toolbarTitle: "Zest Wallpaper",
  toolbarSummary: (visible: number, total: number, filtering: boolean) =>
    filtering ? `显示 ${visible} / ${total} 个壁纸` : `${total} 个本地壁纸`,
  importAction: "导入",
  sortAction: "排序",
  filterTypeAction: "筛选类型",
  filterTagAction: "筛选标签",
  filterAllTypes: "全部类型",
  filterAllTags: "全部标签",
  searchAction: "搜索",
  searchPlaceholder: "搜索标题、标签或类型",
  settingsAction: "设置",
  settingsTabGeneral: "常规",
  settingsTabAbout: "关于",
  aboutHeading: "关于 Zest Wallpaper",
  copyrightLabel: "Copyright",
  versionLabel: "版本",
  qqGroupLabel: "QQ 群",
  cacheLabel: "Scene 缓存",
  cacheBrowseAction: "选择目录",
  cacheClearAction: "清理缓存",
  cachePathUnset: "使用默认位置",
  cachePathMounted: "已配置",
  cachePathMissing: "路径不可用",
  cacheStorageHint: "自定义缓存存储目录。留空则使用管理的库路径下的默认位置。",
  cacheStorageLoading: "读取中…",
  cacheStorageSaving: "保存中…",
  cacheClearing: "清理中…",
  emptyLibraryTitle: "还没有本地壁纸",
  emptyLibraryBody: "导入目录后，这里会显示你的 Workbench 壁纸库。",
  emptySearchTitle: "没有匹配的壁纸",
  emptySearchBody: "换个关键词，或清空搜索后再试。",
  detailEyebrow: "详细信息",
  detailWaitingTitle: "等待选择壁纸",
  detailWaitingBody: "左侧单击缩略图会立即应用到桌面，并在这里展开状态与属性。",
  propertiesHeading: "属性",
  propertiesFallbackSection: "常规",
  propertiesSummary: (count: number) => `${count} 个当前可编辑项`,
  propertiesEmpty: "这个壁纸没有可编辑的 `project.json` 属性。",
  propertiesHidden: "当前没有处于生效条件内的可编辑属性。",
  pauseAction: "暂停播放",
  resumeAction: "继续播放",
  removeAction: "移除",
  desktopLabel: "当前桌面",
  applying: "应用中",
  applyReady: "等待应用",
  applyLive: "动态播放已在桌面生效；静态快照同步只跟随当前活动壁纸。",
  applyFailed: "应用失败",
  runtimeDiagnosticLabel: "运行时诊断",
  runtimeDiagnosticMessage: (diagnostic: RuntimeDiagnostic) =>
    `运行时诊断（${diagnostic.subsystem}/${diagnostic.code}）：${diagnostic.summary}`,
  sceneAssetsLabel: "Scene 外部 assets",
  sceneAssetsBrowseAction: "挂载目录",
  sceneAssetsClearAction: "清除",
  sceneAssetsUnset: "未配置",
  sceneAssetsMounted: "已挂载",
  sceneAssetsMissing: "路径不可用",
  sceneAssetsHint: "这是可选的高兼容 Scene 资源根。未挂载时应用仍可启动、导入和浏览，只对缺失资源给出诊断。",
  sceneAssetsLoading: "读取中…",
  sceneAssetsSaving: "保存中…",
  guiOpacityLabel: "GUI 透明度",
  themeModeLabel: "外观",
  languageLabel: "语言",
  defaultSortLabel: "默认排序",
  guiOpacityValue: (value: number) => `${value}%`,
  themeModeName: (mode: WorkbenchThemeMode) => {
    switch (mode) {
      case "light":
        return "白天";
      case "dark":
        return "夜晚";
      case "system":
      default:
        return "跟随系统";
    }
  },
  languageName: (value: WorkbenchLanguage) => (value === "zh-CN" ? "简体中文" : "English"),
  sortName: (value: WorkbenchSortKey) => (value === "title" ? "名称" : "最近导入"),
  themeResolved: (mode: WorkbenchThemeMode, resolved: WorkbenchResolvedTheme) =>
    mode === "system"
      ? `当前跟随系统，实际为${resolved === "dark" ? "夜晚" : "白天"}。`
      : `当前固定为${mode === "dark" ? "夜晚" : "白天"}。`,
  typeLabel: (type: WallpaperType) => {
    switch (type) {
      case "scene":
        return "Scene";
      case "video":
        return "Video";
      case "web":
        return "Web";
      case "application":
        return "Application";
      default:
        return "Unknown";
    }
  },
  objectCount: (count: number) => `${count} 个对象`,
  bannerMessage: (banner: WorkbenchBannerState) => {
    switch (banner.key) {
      case "dropHint":
        return "把 Windows 上的壁纸目录拖进来，或点击导入。";
      case "libraryReadFailed":
        return `读取本地库失败：${String(banner.values?.error ?? "")}`;
      case "sceneAssetsReadFailed":
        return `读取 Scene assets 设置失败：${String(banner.values?.error ?? "")}`;
      case "dropWithoutPath":
        return "拖拽没有暴露本地路径，改用导入按钮会更稳。";
      case "importing":
        return "正在逆向导入并建立本地素材索引…";
      case "importSuccess":
        return `${String(banner.values?.title ?? "")} 已导入。`;
      case "importFailed":
        return `导入失败：${String(banner.values?.error ?? "")}`;
      case "applySuccess":
        return `${String(banner.values?.title ?? "")} 已应用到桌面。`;
      case "applyFailed":
        return `动态应用失败：${String(banner.values?.error ?? "")}`;
      case "propertySaved":
        return `${String(banner.values?.label ?? "")} 已写入本地配置。`;
      case "propertySaveFailed":
        return `属性保存失败：${String(banner.values?.error ?? "")}`;
      case "playerPaused":
        return "桌面层播放器已暂停。";
      case "playerResumed":
        return "桌面层播放器继续播放。";
      case "playerToggleFailed":
        return `无法切换播放器状态：${String(banner.values?.error ?? "")}`;
      case "sceneAssetsMounted":
        return `Scene 外部 assets 已挂载到 ${String(banner.values?.path ?? "")}。`;
      case "sceneAssetsCleared":
        return "Scene 外部 assets 挂载已清除。";
      case "sceneAssetsUpdateFailed":
        return `更新 Scene assets 设置失败：${String(banner.values?.error ?? "")}`;
      case "removeSuccess":
        return `${String(banner.values?.title ?? "")} 已从本地工作台移除。`;
      case "removeFailed":
        return `删除失败：${String(banner.values?.error ?? "")}`;
      case "cacheCleared":
        return "Scene 缓存已清理。";
      case "cacheClearFailed":
        return `清理缓存失败：${String(banner.values?.error ?? "")}`;
      case "cachePathSetFailed":
        return `设置缓存目录失败：${String(banner.values?.error ?? "")}`;
      default:
        return "";
    }
  },
};

const englishCopy: WorkbenchCopy = {
  toolbarEyebrow: "Workbench",
  toolbarTitle: "Zest Wallpaper",
  toolbarSummary: (visible: number, total: number, filtering: boolean) =>
    filtering ? `Showing ${visible} of ${total} wallpapers` : `${total} wallpapers`,
  importAction: "Import",
  sortAction: "Sort",
  filterTypeAction: "Filter by Type",
  filterTagAction: "Filter by Tag",
  filterAllTypes: "All Types",
  filterAllTags: "All Tags",
  searchAction: "Search",
  searchPlaceholder: "Search title, tag, or type",
  settingsAction: "Settings",
  settingsTabGeneral: "General",
  settingsTabAbout: "About",
  aboutHeading: "About Zest Wallpaper",
  copyrightLabel: "Copyright",
  versionLabel: "Version",
  qqGroupLabel: "QQ Group",
  cacheLabel: "Scene Cache",
  cacheBrowseAction: "Choose Folder",
  cacheClearAction: "Clear Cache",
  cachePathUnset: "Default location",
  cachePathMounted: "Configured",
  cachePathMissing: "Path unavailable",
  cacheStorageHint: "Custom cache storage directory. Defaults to the managed library path when empty.",
  cacheStorageLoading: "Loading…",
  cacheStorageSaving: "Saving…",
  cacheClearing: "Clearing…",
  emptyLibraryTitle: "No wallpapers yet",
  emptyLibraryBody: "Import a directory and your Workbench library will appear here.",
  emptySearchTitle: "No wallpapers match this search",
  emptySearchBody: "Try another keyword or clear the search.",
  detailEyebrow: "Inspector",
  detailWaitingTitle: "Select a wallpaper",
  detailWaitingBody:
    "Click a thumbnail to apply it immediately and inspect its state and properties here.",
  propertiesHeading: "Properties",
  propertiesFallbackSection: "General",
  propertiesSummary: (count: number) => `${count} editable right now`,
  propertiesEmpty: "This wallpaper does not expose editable `project.json` properties.",
  propertiesHidden: "No editable properties are active under the current conditions.",
  pauseAction: "Pause Playback",
  resumeAction: "Resume Playback",
  removeAction: "Remove",
  desktopLabel: "Desktop",
  applying: "Applying",
  applyReady: "Waiting to apply",
  applyLive: "Dynamic playback is live; static snapshot sync follows only the active wallpaper.",
  applyFailed: "Apply failed",
  runtimeDiagnosticLabel: "Runtime diagnostic",
  runtimeDiagnosticMessage: (diagnostic: RuntimeDiagnostic) =>
    `Runtime diagnostic (${diagnostic.subsystem}/${diagnostic.code}): ${diagnostic.summary}`,
  sceneAssetsLabel: "Scene External Assets",
  sceneAssetsBrowseAction: "Mount Folder",
  sceneAssetsClearAction: "Clear",
  sceneAssetsUnset: "Not configured",
  sceneAssetsMounted: "Mounted",
  sceneAssetsMissing: "Path unavailable",
  sceneAssetsHint:
    "This is an optional high-compatibility Scene resource root. The app still starts, imports, and browses without it; missing resources surface as diagnostics instead.",
  sceneAssetsLoading: "Loading…",
  sceneAssetsSaving: "Saving…",
  guiOpacityLabel: "GUI Opacity",
  themeModeLabel: "Appearance",
  languageLabel: "Language",
  defaultSortLabel: "Default Sort",
  guiOpacityValue: (value: number) => `${value}%`,
  themeModeName: (mode: WorkbenchThemeMode) => {
    switch (mode) {
      case "light":
        return "Light";
      case "dark":
        return "Dark";
      case "system":
      default:
        return "System";
    }
  },
  languageName: (value: WorkbenchLanguage) => (value === "zh-CN" ? "简体中文" : "English"),
  sortName: (value: WorkbenchSortKey) => (value === "title" ? "Title" : "Recently Imported"),
  themeResolved: (mode: WorkbenchThemeMode, resolved: WorkbenchResolvedTheme) =>
    mode === "system"
      ? `Following system, currently resolved to ${resolved}.`
      : `Pinned to ${mode}.`,
  typeLabel: (type: WallpaperType) => {
    switch (type) {
      case "scene":
        return "Scene";
      case "video":
        return "Video";
      case "web":
        return "Web";
      case "application":
        return "Application";
      default:
        return "Unknown";
    }
  },
  objectCount: (count: number) => `${count} objects`,
  bannerMessage: (banner: WorkbenchBannerState) => {
    switch (banner.key) {
      case "dropHint":
        return "Drag a Windows wallpaper directory here, or use import.";
      case "libraryReadFailed":
        return `Failed to read the local library: ${String(banner.values?.error ?? "")}`;
      case "sceneAssetsReadFailed":
        return `Failed to read Scene assets settings: ${String(banner.values?.error ?? "")}`;
      case "dropWithoutPath":
        return "The drag event did not expose a local path. The import button is more reliable.";
      case "importing":
        return "Importing and indexing local assets…";
      case "importSuccess":
        return `${String(banner.values?.title ?? "")} imported successfully.`;
      case "importFailed":
        return `Import failed: ${String(banner.values?.error ?? "")}`;
      case "applySuccess":
        return `${String(banner.values?.title ?? "")} is now on the desktop.`;
      case "applyFailed":
        return `Failed to apply wallpaper: ${String(banner.values?.error ?? "")}`;
      case "propertySaved":
        return `${String(banner.values?.label ?? "")} saved to local config.`;
      case "propertySaveFailed":
        return `Failed to save property: ${String(banner.values?.error ?? "")}`;
      case "playerPaused":
        return "Desktop playback paused.";
      case "playerResumed":
        return "Desktop playback resumed.";
      case "playerToggleFailed":
        return `Unable to toggle playback: ${String(banner.values?.error ?? "")}`;
      case "sceneAssetsMounted":
        return `Scene external assets mounted at ${String(banner.values?.path ?? "")}.`;
      case "sceneAssetsCleared":
        return "Scene external assets mount cleared.";
      case "sceneAssetsUpdateFailed":
        return `Failed to update Scene assets settings: ${String(banner.values?.error ?? "")}`;
      case "removeSuccess":
        return `${String(banner.values?.title ?? "")} removed from the local library.`;
      case "removeFailed":
        return `Remove failed: ${String(banner.values?.error ?? "")}`;
      case "cacheCleared":
        return "Scene cache cleared.";
      case "cacheClearFailed":
        return `Failed to clear cache: ${String(banner.values?.error ?? "")}`;
      case "cachePathSetFailed":
        return `Failed to set cache directory: ${String(banner.values?.error ?? "")}`;
      default:
        return "";
    }
  },
};

export function getWorkbenchCopy(language: WorkbenchLanguage): WorkbenchCopy {
  return language === "zh-CN" ? zhCnCopy : englishCopy;
}
