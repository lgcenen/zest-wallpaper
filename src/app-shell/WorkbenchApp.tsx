import { startTransition, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  applyDynamicWallpaper,
  chooseCacheDirectory,
  chooseImportDirectory,
  chooseSceneAssetsDirectory,
  fetchExternalImage,
  getAppVersion,
  importWallpaper,
  listRuntimeAudioOutputDevices,
  listWallpapers,
  openExternalUrl,
  pauseResumeDynamic,
  removeWallpaper,
  setRuntimeAudioOutputDevice,
  setRuntimeAudioOutputVolume,
  setWallpaperProperties,
  toAssetUrl,
} from "../gateway";
import type {
  PlayerRuntimeState,
  PropertySection,
  RuntimeAudioOutputDevice,
  RuntimeDiagnostic,
  SceneRuntimeSettingsSnapshot,
  WallpaperProperty,
  WallpaperRuntimeRecord,
  WallpaperType,
} from "../types";
import {
  getWorkbenchCopy,
  type WorkbenchBannerState,
  type WorkbenchCopy,
} from "./workbench-copy";
import { WorkbenchSelect, type WorkbenchSelectOption } from "./WorkbenchSelect";
import { usePlayerController } from "../state/player-controller";
import {
  applyDraftsToWallpaper,
  buildPropertyMapByKey,
  deriveInspectorProperties,
  propertyMap,
  propertyVisible,
  sectionScopedLabel,
  sectionVisible,
  serializePropertyValue,
  truthy,
  valuesEqual,
  visibleSectionControlCount,
  type DraftPropertyValues,
} from "../state/property-drafts";
import {
  WORKBENCH_GUI_OPACITY_MAX,
  WORKBENCH_GUI_OPACITY_MIN,
  WORKBENCH_GUI_OPACITY_STEP,
  WORKBENCH_AUDIO_OUTPUT_VOLUME_MAX,
  WORKBENCH_AUDIO_OUTPUT_VOLUME_MIN,
  WORKBENCH_AUDIO_OUTPUT_VOLUME_STEP,
  useWorkbenchPreferences,
  type WorkbenchAudioOutputDevice,
  type WorkbenchLanguage,
  type WorkbenchSortKey,
  type WorkbenchThemeMode,
} from "../state/workbench-preferences";
import { useWorkbenchController } from "../state/workbench-controller";
import { useSceneRuntimeSettings } from "../state/scene-runtime-settings";

interface ApplyErrorState {
  id: string;
  error: string;
}

interface ScrollRevealOptions {
  topPadding?: number;
  bottomPadding?: number;
  behavior?: ScrollBehavior;
}

export const APPLY_WALLPAPER_GUI_TIMEOUT_MS = 45_000;

function applyTimeoutMessage(timeoutMs: number) {
  return `Wallpaper apply did not finish within ${Math.round(
    timeoutMs / 1000,
  )}s. The native apply chain is still pending; check [wallpaper-apply] stage logs for the last completed phase.`;
}

function applyDynamicWallpaperWithGuiTimeout(id: string, timeoutMs = APPLY_WALLPAPER_GUI_TIMEOUT_MS) {
  let timeoutId: ReturnType<typeof window.setTimeout> | undefined;
  const timeout = new Promise<WallpaperRuntimeRecord>((_, reject) => {
    timeoutId = window.setTimeout(() => {
      reject(new Error(applyTimeoutMessage(timeoutMs)));
    }, timeoutMs);
  });

  return Promise.race([applyDynamicWallpaper(id), timeout]).finally(() => {
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
    }
  });
}

function valueToColor(value: unknown) {
  if (typeof value !== "string") {
    return "#ffffff";
  }
  const parts = value
    .trim()
    .split(/\s+/)
    .map((segment) => Number.parseFloat(segment));
  if (parts.length < 3 || parts.some((segment) => Number.isNaN(segment))) {
    return "#ffffff";
  }
  const [r, g, b] = parts.map((segment) =>
    Math.max(0, Math.min(255, Math.round(segment * 255))),
  );
  return `#${[r, g, b]
    .map((segment) => segment.toString(16).padStart(2, "0"))
    .join("")}`;
}

function colorToWallpaperValue(hex: string) {
  const normalized = hex.replace("#", "");
  if (normalized.length !== 6) {
    return "1 1 1";
  }
  const channels = normalized.match(/.{2}/g) ?? ["ff", "ff", "ff"];
  return channels
    .map((channel) => (Number.parseInt(channel, 16) / 255).toFixed(4))
    .join(" ");
}

function revealWithinScrollContainer(
  container: HTMLElement | null,
  target: HTMLElement | null,
  options: ScrollRevealOptions = {},
) {
  if (!container || !target) {
    return;
  }

  const topPadding = options.topPadding ?? 12;
  const bottomPadding = options.bottomPadding ?? 96;
  const containerRect = container.getBoundingClientRect();
  const targetRect = target.getBoundingClientRect();
  const currentTop = container.scrollTop;
  let nextTop = currentTop;

  const topLimit = containerRect.top + topPadding;
  const bottomLimit = containerRect.bottom - bottomPadding;

  if (targetRect.top < topLimit) {
    nextTop += targetRect.top - topLimit;
  } else if (targetRect.bottom > bottomLimit) {
    nextTop += targetRect.bottom - bottomLimit;
  }

  if (Math.abs(nextTop - currentTop) < 1) {
    return;
  }

  const nextScrollTop = Math.max(0, nextTop);
  if (typeof container.scrollTo === "function") {
    container.scrollTo({
      top: nextScrollTop,
      behavior: options.behavior ?? "smooth",
    });
  } else {
    container.scrollTop = nextScrollTop;
  }
}

function extractMarkupImageSource(markup?: string | null) {
  if (!markup) {
    return null;
  }
  const match = markup.match(/<img[^>]+src=["']([^"']+)["']/i);
  return match?.[1] ?? null;
}

function extractMarkupHref(markup?: string | null) {
  if (!markup) {
    return null;
  }
  const match = markup.match(/<a[^>]+href=["']([^"']+)["']/i);
  return match?.[1] ?? null;
}

function fileDirectory(path?: string | null) {
  if (!path) {
    return null;
  }
  const normalized = path.replace(/\\/g, "/");
  const index = normalized.lastIndexOf("/");
  return index >= 0 ? normalized.slice(0, index + 1) : null;
}

function resolveAssetReference(reference?: string | null, baseFilePath?: string | null) {
  if (!reference) {
    return null;
  }
  if (/^(https?:|data:|blob:|asset:|tauri:)/i.test(reference)) {
    return reference;
  }

  const sourcePath = reference.startsWith("/")
    ? reference
    : (() => {
        const baseDirectory = fileDirectory(baseFilePath);
        if (!baseDirectory) {
          return null;
        }
        try {
          return decodeURIComponent(new URL(reference, `file://${encodeURI(baseDirectory)}`).pathname);
        } catch {
          return null;
        }
      })();

  return sourcePath ? toAssetUrl(sourcePath) : null;
}

function isExternalHttpUrl(reference?: string | null) {
  return /^(https?:)/i.test(reference ?? "");
}

function ExternalImage({
  src,
  alt,
  className,
  loading,
}: {
  src: string;
  alt: string;
  className: string;
  loading?: "lazy";
}) {
  const [dataUrl, setDataUrl] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDataUrl(null);
    fetchExternalImage(src)
      .then((url) => {
        if (!cancelled) setDataUrl(url);
      })
      .catch(() => {
        if (!cancelled) setDataUrl(null);
      });
    return () => {
      cancelled = true;
    };
  }, [src]);

  if (!dataUrl) return null;
  return (
    <img
      className={className}
      src={dataUrl}
      alt={alt}
      loading={loading}
    />
  );
}

function feedbackToneClass(tone: WorkbenchBannerState["tone"]) {
  switch (tone) {
    case "success":
      return "success";
    case "warning":
      return "warning";
    case "neutral":
    default:
      return "neutral";
  }
}

function latestRuntimeDiagnostic(
  diagnostics: RuntimeDiagnostic[],
): RuntimeDiagnostic | null {
  return diagnostics.reduce<RuntimeDiagnostic | null>((latest, diagnostic) => {
    if (!latest || diagnostic.timestampMs >= latest.timestampMs) {
      return diagnostic;
    }
    return latest;
  }, null);
}

function SelectField({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <label className="settings-field">
      <span>{label}</span>
      <WorkbenchSelect ariaLabel={label} value={value} options={options} onChange={onChange} align="end" />
    </label>
  );
}

function SettingsPopover({
  appVersion,
  themeMode,
  resolvedTheme,
  language,
  sortKey,
  guiOpacity,
  audioOutputVolume,
  audioOutputDevice,
  audioOutputDeviceOptions,
  sceneRuntimeSettings,
  sceneRuntimeSettingsLoading,
  sceneRuntimeSettingsSaving,
  sceneCacheClearing,
  copy,
  onThemeModeChange,
  onLanguageChange,
  onSortKeyChange,
  onGuiOpacityChange,
  onAudioOutputVolumeChange,
  onAudioOutputDeviceChange,
  onChooseSceneAssets,
  onClearSceneAssets,
  onChooseCacheDir,
  onClearCache,
}: {
  appVersion: string;
  themeMode: WorkbenchThemeMode;
  resolvedTheme: "light" | "dark";
  language: WorkbenchLanguage;
  sortKey: WorkbenchSortKey;
  guiOpacity: number;
  audioOutputVolume: number;
  audioOutputDevice: WorkbenchAudioOutputDevice;
  audioOutputDeviceOptions: WorkbenchSelectOption[];
  sceneRuntimeSettings: SceneRuntimeSettingsSnapshot;
  sceneRuntimeSettingsLoading: boolean;
  sceneRuntimeSettingsSaving: boolean;
  sceneCacheClearing: boolean;
  copy: WorkbenchCopy;
  onThemeModeChange: (value: WorkbenchThemeMode) => void;
  onLanguageChange: (value: WorkbenchLanguage) => void;
  onSortKeyChange: (value: WorkbenchSortKey) => void;
  onGuiOpacityChange: (value: number) => void;
  onAudioOutputVolumeChange: (value: number) => void;
  onAudioOutputDeviceChange: (value: WorkbenchAudioOutputDevice) => void;
  onChooseSceneAssets: () => void;
  onClearSceneAssets: () => void;
  onChooseCacheDir: () => void;
  onClearCache: () => void;
}) {
  const [tab, setTab] = useState<"general" | "about">("general");
  const githubUrl = "https://github.com/lgcenen/zest-wallpaper";

  const externalAssetsPath = sceneRuntimeSettings.externalAssetsPath?.trim() ?? "";
  const sceneAssetsStatus = !externalAssetsPath
    ? copy.sceneAssetsUnset
    : sceneRuntimeSettings.externalAssetsExists
      ? copy.sceneAssetsMounted
      : copy.sceneAssetsMissing;
  const sceneAssetsStateClass = !externalAssetsPath
    ? "idle"
    : sceneRuntimeSettings.externalAssetsExists
      ? "ready"
      : "warning";

  const cachePath = sceneRuntimeSettings.cacheStoragePath?.trim() ?? "";
  const cacheStatus = !cachePath
    ? copy.cachePathUnset
    : sceneRuntimeSettings.cacheStorageExists
      ? copy.cachePathMounted
      : copy.cachePathMissing;
  const cacheStateClass = !cachePath
    ? "idle"
    : sceneRuntimeSettings.cacheStorageExists
      ? "ready"
      : "warning";

  return (
    <div className="settings-popover" role="dialog" aria-label={copy.settingsAction}>
      <div className="settings-popover-header">
        <div className="settings-popover-copy">
          <strong>{copy.settingsAction}</strong>
        </div>
      </div>

      <div className="settings-tabs">
        <button
          type="button"
          className={`settings-tab ${tab === "general" ? "active" : ""}`}
          onClick={() => setTab("general")}
        >
          {copy.settingsTabGeneral}
        </button>
        <button
          type="button"
          className={`settings-tab ${tab === "about" ? "active" : ""}`}
          onClick={() => setTab("about")}
        >
          {copy.settingsTabAbout}
        </button>
      </div>

      {tab === "general" ? (
        <div className="settings-grid">
          <SelectField
            label={copy.themeModeLabel}
            value={themeMode}
            options={[
              { value: "system", label: copy.themeModeName("system") },
              { value: "dark", label: copy.themeModeName("dark") },
              { value: "light", label: copy.themeModeName("light") },
            ]}
            onChange={(value) => onThemeModeChange(value as WorkbenchThemeMode)}
          />
          <SelectField
            label={copy.languageLabel}
            value={language}
            options={[
              { value: "zh-CN", label: copy.languageName("zh-CN") },
              { value: "en", label: copy.languageName("en") },
            ]}
            onChange={(value) => onLanguageChange(value as WorkbenchLanguage)}
          />
          <label className="settings-field">
            <span>{copy.guiOpacityLabel}</span>
            <div className="settings-range-row">
              <input
                className="property-range settings-range"
                type="range"
                min={WORKBENCH_GUI_OPACITY_MIN}
                max={WORKBENCH_GUI_OPACITY_MAX}
                step={WORKBENCH_GUI_OPACITY_STEP}
                value={guiOpacity}
                aria-label={copy.guiOpacityLabel}
                onInput={(event) =>
                  onGuiOpacityChange(Number((event.target as HTMLInputElement).value))
                }
                onChange={(event) => onGuiOpacityChange(Number(event.target.value))}
              />
              <strong className="settings-range-value">{copy.guiOpacityValue(guiOpacity)}</strong>
            </div>
          </label>

          <section className="settings-audio" aria-label={copy.audioSettingsLabel}>
            <div className="settings-section-heading">
              <strong>{copy.audioSettingsLabel}</strong>
            </div>

            <label className="settings-field">
              <span>{copy.audioOutputVolumeLabel}</span>
              <div className="settings-range-row">
                <input
                  className="property-range settings-range"
                  type="range"
                  min={WORKBENCH_AUDIO_OUTPUT_VOLUME_MIN}
                  max={WORKBENCH_AUDIO_OUTPUT_VOLUME_MAX}
                  step={WORKBENCH_AUDIO_OUTPUT_VOLUME_STEP}
                  value={audioOutputVolume}
                  aria-label={copy.audioOutputVolumeLabel}
                  onInput={(event) =>
                    onAudioOutputVolumeChange(Number((event.target as HTMLInputElement).value))
                  }
                  onChange={(event) => onAudioOutputVolumeChange(Number(event.target.value))}
                />
                <strong className="settings-range-value">
                  {copy.audioOutputVolumeValue(audioOutputVolume)}
                </strong>
              </div>
            </label>

            <SelectField
              label={copy.audioOutputDeviceLabel}
              value={audioOutputDevice}
              options={audioOutputDeviceOptions}
              onChange={(value) => onAudioOutputDeviceChange(value)}
            />
          </section>

          <div className="settings-runtime">
            <label className="settings-field">
              <span>{copy.sceneAssetsLabel}</span>
              <div className="settings-path-card">
                <div className="settings-path-copy">
                  <strong
                    className={`settings-path-state ${sceneAssetsStateClass}`}
                  >
                    {sceneRuntimeSettingsLoading ? copy.sceneAssetsLoading : sceneAssetsStatus}
                  </strong>
                  <code>{externalAssetsPath || copy.sceneAssetsUnset}</code>
                  <p>{copy.sceneAssetsHint}</p>
                </div>

                <div className="settings-path-actions">
                  <button
                    className="ghost-button"
                    type="button"
                    disabled={sceneRuntimeSettingsLoading || sceneRuntimeSettingsSaving}
                    onClick={onChooseSceneAssets}
                  >
                    {sceneRuntimeSettingsSaving ? copy.sceneAssetsSaving : copy.sceneAssetsBrowseAction}
                  </button>
                  <button
                    className="ghost-button"
                    type="button"
                    disabled={
                      sceneRuntimeSettingsLoading
                      || sceneRuntimeSettingsSaving
                      || !externalAssetsPath
                    }
                    onClick={onClearSceneAssets}
                  >
                    {copy.sceneAssetsClearAction}
                  </button>
                </div>
              </div>
            </label>

            <label className="settings-field">
              <span>{copy.cacheLabel}</span>
              <div className="settings-path-card">
                <div className="settings-path-copy">
                  <strong
                    className={`settings-path-state ${cacheStateClass}`}
                  >
                    {sceneRuntimeSettingsLoading ? copy.cacheStorageLoading : cacheStatus}
                  </strong>
                  <code>{cachePath || copy.cachePathUnset}</code>
                  <p>{copy.cacheStorageHint}</p>
                </div>

                <div className="settings-path-actions">
                  <button
                    className="ghost-button"
                    type="button"
                    disabled={sceneRuntimeSettingsLoading || sceneRuntimeSettingsSaving}
                    onClick={onChooseCacheDir}
                  >
                    {sceneRuntimeSettingsSaving ? copy.cacheStorageSaving : copy.cacheBrowseAction}
                  </button>
                  <button
                    className="ghost-button"
                    type="button"
                    disabled={sceneCacheClearing}
                    onClick={onClearCache}
                  >
                    {sceneCacheClearing ? copy.cacheClearing : copy.cacheClearAction}
                  </button>
                </div>
              </div>
            </label>
          </div>
        </div>
      ) : (
        <div className="settings-about">
          <strong className="settings-about-heading">{copy.aboutHeading}</strong>
          <div className="settings-about-grid">
            <span className="settings-about-label">{copy.appLabel}</span>
            <span className="settings-about-value">Zest Wallpaper</span>

            <span className="settings-about-label">{copy.versionLabel}</span>
            <span className="settings-about-value">{appVersion}</span>

            <span className="settings-about-label">{copy.licenseLabel}</span>
            <span className="settings-about-value">Apache-2.0</span>

            <span className="settings-about-label">{copy.githubLabel}</span>
            <a
              className="settings-about-link"
              href={githubUrl}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => {
                event.preventDefault();
                void openExternalUrl(githubUrl);
              }}
            >
              {githubUrl}
            </a>

            <span className="settings-about-label">{copy.qqGroupLabel}</span>
            <span className="settings-about-value">867740762</span>
          </div>
        </div>
      )}
    </div>
  );
}

function PropertyField({
  property,
  label,
  onPreview,
  onCommit,
}: {
  property: WallpaperProperty;
  label?: string;
  onPreview: (value: unknown) => void;
  onCommit: (value: unknown) => void;
}) {
  const displayLabel = label ?? property.label;

  if (property.kind === "group" || property.kind === "text") {
    return (
      <div className="property property-copy">
        <span title={displayLabel}>{displayLabel}</span>
      </div>
    );
  }

  if (property.kind === "bool") {
    return (
      <label className="property property-row">
        <span className="property-label" title={displayLabel}>
          {displayLabel}
        </span>
        <span className="property-control property-control-compact">
          <input
            className="toggle-switch"
            type="checkbox"
            checked={truthy(property.value)}
            onChange={(event) => {
              const next = event.target.checked;
              onPreview(next);
              onCommit(next);
            }}
          />
        </span>
      </label>
    );
  }

  if (property.kind === "slider") {
    const currentValue =
      typeof property.value === "number" ? property.value : Number(property.value ?? 0);
    const displayValue = Number.isFinite(currentValue)
      ? currentValue.toFixed(2).replace(/\.00$/, "")
      : "0";
    return (
      <label className="property property-row">
        <span className="property-label" title={displayLabel}>
          {displayLabel}
        </span>
        <span className="property-control property-control-slider">
          <strong className="property-value">{displayValue}</strong>
          <input
            className="property-range"
            type="range"
            min={property.min ?? 0}
            max={property.max ?? 100}
            step={property.step ?? 1}
            value={Number.isFinite(currentValue) ? currentValue : 0}
            onInput={(event) => onPreview(Number((event.target as HTMLInputElement).value))}
            onChange={(event) => onPreview(Number(event.target.value))}
            onPointerUp={(event) => onCommit(Number((event.currentTarget as HTMLInputElement).value))}
            onKeyUp={(event) => onCommit(Number((event.currentTarget as HTMLInputElement).value))}
            onBlur={(event) => onCommit(Number(event.currentTarget.value))}
          />
        </span>
      </label>
    );
  }

  if (property.kind === "color") {
    return (
      <label className="property property-row">
        <span className="property-label" title={displayLabel}>
          {displayLabel}
        </span>
        <span className="property-control property-control-compact">
          <input
            className="property-color"
            type="color"
            value={valueToColor(property.value)}
            onInput={(event) => onPreview(colorToWallpaperValue((event.target as HTMLInputElement).value))}
            onChange={(event) => onCommit(colorToWallpaperValue(event.target.value))}
          />
        </span>
      </label>
    );
  }

  if (property.kind === "combo") {
    return (
      <label className="property property-row">
        <span className="property-label" title={displayLabel}>
          {displayLabel}
        </span>
        <span className="property-control">
          <WorkbenchSelect
            ariaLabel={displayLabel}
            value={String(property.value ?? "")}
            options={property.options.map(
              (option) =>
                ({
                  value: option.value,
                  label: option.label,
                }) satisfies WorkbenchSelectOption,
            )}
            onChange={(nextValue) => {
              onPreview(nextValue);
              onCommit(nextValue);
            }}
            align="end"
          />
        </span>
      </label>
    );
  }

  return (
    <label className="property property-row">
      <span className="property-label" title={displayLabel}>
        {displayLabel}
      </span>
      <span className="property-control">
        <input
          type="text"
          value={String(property.value ?? "")}
          onChange={(event) => onPreview(event.target.value)}
          onBlur={(event) => onCommit(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              onCommit((event.currentTarget as HTMLInputElement).value);
            }
          }}
        />
      </span>
    </label>
  );
}

function PropertiesPanel({
  wallpaper,
  properties,
  sections,
  copy,
  onPreview,
  onCommit,
  scrollContainerRef,
}: {
  wallpaper: WallpaperRuntimeRecord;
  properties: WallpaperProperty[];
  sections: PropertySection[];
  copy: WorkbenchCopy;
  onPreview: (property: WallpaperProperty, value: unknown) => void;
  onCommit: (property: WallpaperProperty, value: unknown) => void;
  scrollContainerRef: RefObject<HTMLDivElement | null>;
}) {
  const [openSections, setOpenSections] = useState<Record<string, boolean>>({});
  const propertiesByKey = useMemo(() => buildPropertyMapByKey(properties), [properties]);
  const effectivePropertyValues = useMemo(() => propertyMap(wallpaper), [wallpaper]);
  const sectionKeysSignature = useMemo(
    () => sections.map((section) => section.key).join("|"),
    [sections],
  );
  const previousWallpaperIdRef = useRef(wallpaper.id);

  useEffect(() => {
    const wallpaperChanged = previousWallpaperIdRef.current !== wallpaper.id;
    previousWallpaperIdRef.current = wallpaper.id;

    setOpenSections((current) => {
      const next: Record<string, boolean> = {};
      sections.forEach((section, index) => {
        next[section.key] =
          wallpaperChanged
            ? index === 0
            : current[section.key] ?? index === 0;
      });

      const currentKeys = Object.keys(current);
      const nextKeys = Object.keys(next);
      if (
        currentKeys.length === nextKeys.length
        && nextKeys.every((key) => current[key] === next[key])
      ) {
        return current;
      }

      return next;
    });
  }, [sectionKeysSignature, sections, wallpaper.id]);

  function revealProperty(target: EventTarget | null, behavior: ScrollBehavior) {
    if (!(target instanceof HTMLElement)) {
      return;
    }

    const row = target.closest(".property") as HTMLElement | null;
    revealWithinScrollContainer(scrollContainerRef.current, row, {
      topPadding: 12,
      bottomPadding: 144,
      behavior,
    });
  }

  function toggleSection(section: PropertySection) {
    setOpenSections((current) => {
      const nextOpen = !current[section.key];
      if (nextOpen) {
        window.requestAnimationFrame(() => {
          const container = scrollContainerRef.current;
          const sectionNode = container?.querySelector<HTMLElement>(
            `[data-property-section="${section.key}"]`,
          );
          revealWithinScrollContainer(container ?? null, sectionNode ?? null, {
            topPadding: 12,
            bottomPadding: 144,
            behavior: "smooth",
          });
        });
      }
      return {
        ...current,
        [section.key]: nextOpen,
      };
    });
  }

  const visibleSections = sections.filter((section) =>
    sectionVisible(section, propertiesByKey, effectivePropertyValues),
  );

  return (
    <div
      className="property-list"
      onFocusCapture={(event) => revealProperty(event.target, "smooth")}
      onPointerDownCapture={(event) => revealProperty(event.target, "auto")}
    >
      {visibleSections.map((section) => (
        <section
          key={section.key}
          className={`property-group ${openSections[section.key] ? "open" : ""}`}
          data-property-section={section.key}
        >
          <button
            type="button"
            className="property-group-trigger"
            onClick={() => toggleSection(section)}
          >
            <span className="property-group-title" title={section.label}>
              {section.label}
            </span>
            <span className={`property-group-chevron ${openSections[section.key] ? "open" : ""}`}>
              ⌃
            </span>
          </button>
          {openSections[section.key] ? (
            <div className="property-group-body">
              {section.items.map((item, index) => {
                if (item.kind === "separator") {
                  return <div key={`${section.key}-separator-${index}`} className="property-separator" />;
                }

                if (item.kind === "description") {
                  const rawImageSrc = extractMarkupImageSource(item.markup);
                  const imageSource = resolveAssetReference(
                    rawImageSrc,
                    wallpaper.entryPath,
                  );
                  const isImageExternal = isExternalHttpUrl(rawImageSrc);
                  const rawLinkTarget = extractMarkupHref(item.markup);
                  const linkTarget =
                    resolveAssetReference(rawLinkTarget, wallpaper.entryPath) ?? rawLinkTarget;
                  const externalLinkTarget = isExternalHttpUrl(linkTarget) ? linkTarget : null;
                  if (!item.text?.trim() && !imageSource) {
                    return null;
                  }
                  return (
                    <div key={`${section.key}-description-${index}`} className="property property-description">
                      {item.text?.trim() ? (
                        linkTarget ? (
                          <a
                            className="property-description-link"
                            href={linkTarget}
                            target={externalLinkTarget ? "_blank" : undefined}
                            rel={externalLinkTarget ? "noreferrer" : undefined}
                            onClick={
                              externalLinkTarget
                                ? (event) => {
                                    event.preventDefault();
                                    void openExternalUrl(externalLinkTarget);
                                  }
                                : undefined
                            }
                            title={item.text}
                          >
                            {item.text}
                          </a>
                        ) : (
                          <span title={item.text}>{item.text}</span>
                        )
                      ) : null}
                      {imageSource ? (
                        isImageExternal ? (
                          <ExternalImage
                            className="property-description-image"
                            src={imageSource}
                            alt={item.text ?? section.label}
                            loading="lazy"
                          />
                        ) : (
                          <img
                            className="property-description-image"
                            src={imageSource}
                            alt={item.text ?? section.label}
                            loading="lazy"
                          />
                        )
                      ) : null}
                    </div>
                  );
                }

                const property = item.key ? propertiesByKey.get(item.key) : null;
                if (!property || !propertyVisible(property, effectivePropertyValues)) {
                  return null;
                }

                return (
                  <PropertyField
                    key={property.key}
                    property={property}
                    label={sectionScopedLabel(section, property)}
                    onPreview={(value) => onPreview(property, value)}
                    onCommit={(value) => onCommit(property, value)}
                  />
                );
              })}
            </div>
          ) : null}
        </section>
      ))}

      {wallpaper.propertySchema.length === 0 ? (
        <div className="property property-copy">
          <span>{copy.propertiesEmpty}</span>
        </div>
      ) : null}
      {wallpaper.propertySchema.length > 0 && visibleSections.length === 0 ? (
        <div className="property property-copy">
          <span>{copy.propertiesHidden}</span>
        </div>
      ) : null}
    </div>
  );
}

function LibraryPane({
  appVersion,
  wallpapers,
  totalCount,
  selectedId,
  activeWallpaperId,
  isApplyingWallpaperId,
  searchQuery,
  typeFilter,
  tagFilter,
  availableTags,
  isImporting,
  copy,
  resolvedTheme,
  preferences,
  audioOutputDeviceOptions,
  sceneRuntimeSettings,
  sceneRuntimeSettingsLoading,
  sceneRuntimeSettingsSaving,
  sceneCacheClearing,
  onSearchChange,
  onTypeFilterChange,
  onTagFilterChange,
  onSortChange,
  onThemeModeChange,
  onLanguageChange,
  onGuiOpacityChange,
  onAudioOutputVolumeChange,
  onAudioOutputDeviceChange,
  onChooseSceneAssets,
  onClearSceneAssets,
  onChooseCacheDir,
  onClearCache,
  onSelect,
  onOpenImport,
}: {
  appVersion: string;
  wallpapers: WallpaperRuntimeRecord[];
  totalCount: number;
  selectedId: string | null;
  activeWallpaperId: string | null;
  isApplyingWallpaperId: string | null;
  searchQuery: string;
  typeFilter: WallpaperType | "all";
  tagFilter: string;
  availableTags: string[];
  isImporting: boolean;
  copy: WorkbenchCopy;
  resolvedTheme: "light" | "dark";
  preferences: {
    themeMode: WorkbenchThemeMode;
    language: WorkbenchLanguage;
    sortKey: WorkbenchSortKey;
    guiOpacity: number;
    audioOutputVolume: number;
    audioOutputDevice: WorkbenchAudioOutputDevice;
  };
  audioOutputDeviceOptions: WorkbenchSelectOption[];
  sceneRuntimeSettings: SceneRuntimeSettingsSnapshot;
  sceneRuntimeSettingsLoading: boolean;
  sceneRuntimeSettingsSaving: boolean;
  sceneCacheClearing: boolean;
  onSearchChange: (value: string) => void;
  onTypeFilterChange: (value: WallpaperType | "all") => void;
  onTagFilterChange: (value: string) => void;
  onSortChange: (value: WorkbenchSortKey) => void;
  onThemeModeChange: (value: WorkbenchThemeMode) => void;
  onLanguageChange: (value: WorkbenchLanguage) => void;
  onGuiOpacityChange: (value: number) => void;
  onAudioOutputVolumeChange: (value: number) => void;
  onAudioOutputDeviceChange: (value: WorkbenchAudioOutputDevice) => void;
  onChooseSceneAssets: () => void;
  onClearSceneAssets: () => void;
  onChooseCacheDir: () => void;
  onClearCache: () => void;
  onSelect: (record: WallpaperRuntimeRecord) => void;
  onOpenImport: () => void;
}) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const settingsRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!settingsOpen) {
      return;
    }

    const onPointerDown = (event: PointerEvent) => {
      if (settingsRef.current?.contains(event.target as Node)) {
        return;
      }
      setSettingsOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSettingsOpen(false);
      }
    };

    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [settingsOpen]);

  return (
    <section className="library-pane">
      <header className="workbench-toolbar">
        <div className="workbench-toolbar-copy">
          <h1>{copy.toolbarTitle}</h1>
          <span className="workbench-toolbar-meta">
            {copy.toolbarSummary(wallpapers.length, totalCount, searchQuery.trim().length > 0)}
          </span>
        </div>

        <div className="workbench-toolbar-actions">
          <button
            className="primary-button"
            type="button"
            disabled={isImporting}
            onClick={onOpenImport}
          >
            {copy.importAction}
          </button>

          <label className="toolbar-search">
            <span className="sr-only">{copy.searchAction}</span>
            <input
              type="search"
              value={searchQuery}
              aria-label={copy.searchAction}
              placeholder={copy.searchPlaceholder}
              onChange={(event) => onSearchChange(event.target.value)}
            />
          </label>

          <label className="toolbar-select">
            <span className="sr-only">{copy.sortAction}</span>
            <WorkbenchSelect
              ariaLabel={copy.sortAction}
              value={preferences.sortKey}
              options={[
                { value: "recent", label: copy.sortName("recent") },
                { value: "title", label: copy.sortName("title") },
              ]}
              onChange={(value) => onSortChange(value as WorkbenchSortKey)}
              align="end"
            />
          </label>

          <label className="toolbar-select">
            <span className="sr-only">{copy.filterTypeAction}</span>
            <WorkbenchSelect
              ariaLabel={copy.filterTypeAction}
              value={typeFilter}
              options={[
                { value: "all", label: copy.filterAllTypes },
                { value: "scene", label: copy.typeLabel("scene") },
                { value: "video", label: copy.typeLabel("video") },
                { value: "web", label: copy.typeLabel("web") },
              ]}
              onChange={(value) => onTypeFilterChange(value as WallpaperType | "all")}
            />
          </label>

          <label className="toolbar-select">
            <span className="sr-only">{copy.filterTagAction}</span>
            <WorkbenchSelect
              ariaLabel={copy.filterTagAction}
              value={tagFilter}
              options={[
                { value: "all", label: copy.filterAllTags },
                ...availableTags.map((tag) => ({ value: tag, label: tag })),
              ]}
              onChange={(value) => onTagFilterChange(value)}
            />
          </label>

          <div ref={settingsRef} className="toolbar-settings">
            <button
              className="ghost-button"
              type="button"
              aria-haspopup="dialog"
              aria-expanded={settingsOpen}
              onClick={() => setSettingsOpen((current) => !current)}
            >
              {copy.settingsAction}
            </button>
            {settingsOpen ? (
              <SettingsPopover
                appVersion={appVersion}
                themeMode={preferences.themeMode}
                resolvedTheme={resolvedTheme}
                language={preferences.language}
                sortKey={preferences.sortKey}
                guiOpacity={preferences.guiOpacity}
                audioOutputVolume={preferences.audioOutputVolume}
                audioOutputDevice={preferences.audioOutputDevice}
                audioOutputDeviceOptions={audioOutputDeviceOptions}
                sceneRuntimeSettings={sceneRuntimeSettings}
                sceneRuntimeSettingsLoading={sceneRuntimeSettingsLoading}
                sceneRuntimeSettingsSaving={sceneRuntimeSettingsSaving}
                sceneCacheClearing={sceneCacheClearing}
                copy={copy}
                onThemeModeChange={onThemeModeChange}
                onLanguageChange={onLanguageChange}
                onSortKeyChange={onSortChange}
                onGuiOpacityChange={onGuiOpacityChange}
                onAudioOutputVolumeChange={onAudioOutputVolumeChange}
                onAudioOutputDeviceChange={onAudioOutputDeviceChange}
                onChooseSceneAssets={onChooseSceneAssets}
                onClearSceneAssets={onClearSceneAssets}
                onChooseCacheDir={onChooseCacheDir}
                onClearCache={onClearCache}
              />
            ) : null}
          </div>
        </div>
      </header>

      <div className="library-grid-scroll">
        <div className="library-grid">
          {wallpapers.length === 0 ? (
            <div className="empty-library">
              <strong>{searchQuery.trim() ? copy.emptySearchTitle : copy.emptyLibraryTitle}</strong>
              <span>{searchQuery.trim() ? copy.emptySearchBody : copy.emptyLibraryBody}</span>
            </div>
          ) : null}

          {wallpapers.map((record) => {
            const thumbnail = toAssetUrl(record.previewPath);
            const isSelected = selectedId === record.id;
            const isActive = activeWallpaperId === record.id;
            const isApplying = isApplyingWallpaperId === record.id;
            const overlayTone = isActive ? "active" : isApplying ? "applying" : "idle";

            return (
              <button
                key={record.id}
                type="button"
                className={`library-card ${isSelected ? "active" : ""} ${isActive ? "desktop-active" : ""} ${isApplying ? "applying" : ""}`}
                onClick={() => onSelect(record)}
                aria-label={record.title}
                title={record.title}
              >
                <div className="library-card-thumb">
                  {thumbnail ? (
                    <img src={thumbnail} alt="" />
                  ) : (
                    <span className="library-card-fallback">
                      <span className="library-card-fallback-mark" aria-hidden="true">
                        {copy.typeLabel(record.wallpaperType).slice(0, 1)}
                      </span>
                      <span className="library-card-fallback-title" title={record.title}>
                        {record.title}
                      </span>
                    </span>
                  )}
                  <div className={`library-card-overlay ${overlayTone}`}>
                    <strong>{copy.typeLabel(record.wallpaperType)}</strong>
                  </div>
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function WallpaperDetailPane({
  wallpaper,
  activeWallpaper,
  isApplying,
  lastApplyError,
  diagnostics,
  banner,
  paused,
  copy,
  visiblePropertyCount,
  inspectorProperties,
  inspectorSections,
  detailScrollRef,
  onPauseToggle,
  onRemove,
  onPreview,
  onCommit,
}: {
  wallpaper?: WallpaperRuntimeRecord | null;
  activeWallpaper?: WallpaperRuntimeRecord | null;
  isApplying: boolean;
  lastApplyError?: string | null;
  diagnostics: RuntimeDiagnostic[];
  banner: WorkbenchBannerState;
  paused: boolean;
  copy: WorkbenchCopy;
  visiblePropertyCount: number;
  inspectorProperties: WallpaperProperty[];
  inspectorSections: PropertySection[];
  detailScrollRef: RefObject<HTMLDivElement | null>;
  onPauseToggle: () => void;
  onRemove: () => void;
  onPreview: (property: WallpaperProperty, value: unknown) => void;
  onCommit: (property: WallpaperProperty, value: unknown) => void;
}) {
  if (!wallpaper) {
    return (
      <article className="detail-surface detail-empty-state">
        <p className="eyebrow">{copy.detailEyebrow}</p>
        <h2>{copy.detailWaitingTitle}</h2>
        <p>{copy.detailWaitingBody}</p>
      </article>
    );
  }

  const isActive = activeWallpaper?.id === wallpaper.id;
  const latestDiagnostic = latestRuntimeDiagnostic(diagnostics);
  const diagnosticFeedback = latestDiagnostic
    ? copy.runtimeDiagnosticMessage(latestDiagnostic)
    : null;
  const fallbackFeedbackTone: WorkbenchBannerState["tone"] = lastApplyError
    ? "warning"
    : latestDiagnostic
      ? "warning"
      : isApplying
        ? "neutral"
        : isActive
          ? "success"
          : "neutral";
  const fallbackFeedback = lastApplyError
    ? copy.applyFailed
    : diagnosticFeedback
      ? diagnosticFeedback
      : isApplying
        ? copy.applying
        : isActive
          ? copy.applyLive
          : activeWallpaper
            ? `${copy.desktopLabel} · ${activeWallpaper.title}`
            : copy.applyReady;
  const feedbackMessage = banner.key === "dropHint" ? fallbackFeedback : copy.bannerMessage(banner);
  const feedbackTone = banner.key === "dropHint" ? fallbackFeedbackTone : banner.tone;

  return (
    <article className="detail-surface">
      <section className="detail-section detail-hero">
        <div className="detail-hero-layout">
          <div className="detail-hero-copy">
            <div className="detail-hero-drag">
              <div className="detail-heading">
                <p className="eyebrow">{copy.detailEyebrow}</p>
                <h2 className="detail-title" title={wallpaper.title}>
                  {wallpaper.title}
                </h2>
              </div>

              <div className="detail-badge-row">
                <span className="toolbar-chip">{copy.typeLabel(wallpaper.wallpaperType)}</span>
                {wallpaper.runtime.kind === "scene" ? (
                  <span className="toolbar-chip">
                    {copy.objectCount(wallpaper.runtime.scene.source.objectCount)}
                  </span>
                ) : null}
                {isApplying ? <span className="toolbar-chip warning">{copy.applying}</span> : null}
                {lastApplyError ? <span className="toolbar-chip warning">{copy.applyFailed}</span> : null}
                {latestDiagnostic ? (
                  <span className="toolbar-chip warning">{copy.runtimeDiagnosticLabel}</span>
                ) : null}
              </div>

              {wallpaper.tags.length > 0 ? (
                <div className="token-wrap">
                  {wallpaper.tags.map((tag) => (
                    <span key={tag} className="token-chip">
                      {tag}
                    </span>
                  ))}
                </div>
              ) : null}

              <div className={`detail-inline-feedback ${feedbackToneClass(feedbackTone)}`}>
                <span className="detail-inline-feedback-copy">{feedbackMessage}</span>
              </div>
            </div>

            <div className="detail-actions">
              <button className="ghost-button" type="button" onClick={onPauseToggle}>
                {paused ? copy.resumeAction : copy.pauseAction}
              </button>
              <button className="ghost-button danger" type="button" onClick={onRemove}>
                {copy.removeAction}
              </button>
            </div>
          </div>
        </div>
      </section>

      <section className="detail-section">
        <div className="detail-section-header">
          <h3>{copy.propertiesHeading}</h3>
          <span>{copy.propertiesSummary(visiblePropertyCount)}</span>
        </div>
        <PropertiesPanel
          wallpaper={wallpaper}
          properties={inspectorProperties}
          sections={inspectorSections}
          copy={copy}
          onPreview={onPreview}
          onCommit={onCommit}
          scrollContainerRef={detailScrollRef}
        />
      </section>
    </article>
  );
}

export default function WorkbenchApp() {
  const playerState: PlayerRuntimeState = usePlayerController();
  const { preferences, resolvedTheme, updatePreference } = useWorkbenchPreferences();
  const {
    settings: sceneRuntimeSettings,
    loading: sceneRuntimeSettingsLoading,
    saving: sceneRuntimeSettingsSaving,
    error: sceneRuntimeSettingsError,
    clearing: sceneCacheClearing,
    updateExternalAssetsPath,
    updateCacheStoragePath,
    clearCache,
  } = useSceneRuntimeSettings();
  const copy = useMemo(() => getWorkbenchCopy(preferences.language), [preferences.language]);
  const [audioOutputDevices, setAudioOutputDevices] = useState<RuntimeAudioOutputDevice[]>([
    { id: "system-default", name: "System Default Output", isSystemDefault: true },
  ]);
  const [wallpapers, setWallpapers] = useState<WallpaperRuntimeRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectionMode, setSelectionMode] = useState<"auto" | "manual">("auto");
  const [draftValues, setDraftValues] = useState<DraftPropertyValues>({});
  const [banner, setBanner] = useState<WorkbenchBannerState>({
    tone: "neutral",
    key: "dropHint",
  });
  const [isImporting, setIsImporting] = useState(false);
  const [appVersion, setAppVersion] = useState("0.1.0");
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearchQuery = useDeferredValue(searchQuery);
  const [typeFilter, setTypeFilter] = useState<WallpaperType | "all">("all");
  const [tagFilter, setTagFilter] = useState<string>("all");
  const [isApplyingWallpaperId, setIsApplyingWallpaperId] = useState<string | null>(null);
  const [lastApplyError, setLastApplyError] = useState<ApplyErrorState | null>(null);
  const detailScrollRef = useRef<HTMLDivElement | null>(null);
  const inFlightPropertySignatures = useRef(new Set<string>());
  const applyRequestRef = useRef(0);
  const activeWallpaper = playerState.active ?? null;
  const activeWallpaperId = activeWallpaper?.id ?? null;
  const paused = playerState.paused;
  const audioOutputDeviceOptions = useMemo(() => {
    const options = audioOutputDevices.map((device) => ({
      value: device.id,
      label: device.isSystemDefault ? copy.audioOutputSystemDefaultName : device.name,
    }));
    if (
      preferences.audioOutputDevice &&
      !options.some((option) => option.value === preferences.audioOutputDevice)
    ) {
      options.push({
        value: preferences.audioOutputDevice,
        label: preferences.audioOutputDevice,
      });
    }
    return options;
  }, [audioOutputDevices, copy.audioOutputSystemDefaultName, preferences.audioOutputDevice]);

  useWorkbenchController({
    enabled: true,
    setWallpapers,
    setBanner,
  });

  useEffect(() => {
    let cancelled = false;

    getAppVersion()
      .then((version) => {
        if (!cancelled) {
          setAppVersion(version);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setAppVersion("0.1.0");
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    listRuntimeAudioOutputDevices()
      .then((devices) => {
        if (!cancelled && devices.length > 0) {
          setAudioOutputDevices(devices);
        }
      })
      .catch((error) => {
        console.warn("Failed to list runtime audio output devices", error);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!sceneRuntimeSettingsError) {
      return;
    }
    setBanner({
      tone: "warning",
      key: "sceneAssetsReadFailed",
      values: {
        error: sceneRuntimeSettingsError,
      },
    });
  }, [sceneRuntimeSettingsError, setBanner]);

  useEffect(() => {
    void setRuntimeAudioOutputVolume(preferences.audioOutputVolume).catch((error) => {
      console.warn("Failed to update runtime audio output volume", error);
    });
  }, [preferences.audioOutputVolume]);

  useEffect(() => {
    const deviceId =
      preferences.audioOutputDevice === "system-default" ? null : preferences.audioOutputDevice;
    void setRuntimeAudioOutputDevice(deviceId).catch((error) => {
      console.warn("Failed to update runtime audio output device", error);
    });
  }, [preferences.audioOutputDevice]);

  const orderedWallpapers = useMemo(() => {
    const next = [...wallpapers];
    switch (preferences.sortKey) {
      case "title":
        next.sort((left, right) =>
          left.title.localeCompare(
            right.title,
            preferences.language === "zh-CN" ? "zh-Hans-CN" : "en",
          ),
        );
        break;
      case "recent":
      default:
        next.sort((left, right) => right.importedAt.localeCompare(left.importedAt));
        break;
    }
    return next;
  }, [preferences.language, preferences.sortKey, wallpapers]);

  const availableTags = useMemo(() => {
    const tags = new Set<string>();
    for (const record of wallpapers) {
      for (const tag of record.tags ?? []) {
        tags.add(tag);
      }
    }
    return [...tags].sort((a, b) => a.localeCompare(b));
  }, [wallpapers]);

  const visibleWallpapers = useMemo(() => {
    let result = orderedWallpapers;

    if (typeFilter !== "all") {
      result = result.filter((record) => record.wallpaperType === typeFilter);
    }

    if (tagFilter !== "all") {
      result = result.filter((record) => (record.tags ?? []).includes(tagFilter));
    }

    const keyword = deferredSearchQuery.trim().toLocaleLowerCase();
    if (keyword) {
      result = result.filter((record) => {
        const haystacks = [
          record.title,
          record.wallpaperType,
          copy.typeLabel(record.wallpaperType),
          ...(record.tags ?? []),
        ];
        return haystacks.some((value) => value.toLocaleLowerCase().includes(keyword));
      });
    }

    return result;
  }, [copy, deferredSearchQuery, orderedWallpapers, typeFilter, tagFilter]);

  const selected = useMemo(
    () => wallpapers.find((record) => record.id === selectedId) ?? null,
    [selectedId, wallpapers],
  );

  const previewWallpaper = useMemo(
    () => applyDraftsToWallpaper(selected, draftValues),
    [draftValues, selected],
  );
  const inspectorProperties = useMemo(
    () => deriveInspectorProperties(previewWallpaper?.propertySchema ?? []),
    [previewWallpaper?.propertySchema],
  );
  const previewPropertyMap = useMemo(() => propertyMap(previewWallpaper), [previewWallpaper]);
  const inspectorPropertyMap = useMemo(
    () => buildPropertyMapByKey(inspectorProperties),
    [inspectorProperties],
  );
  const inspectorSections = useMemo(() => {
    if (!previewWallpaper) {
      return [];
    }
    if (previewWallpaper.propertySections.length > 0) {
      return previewWallpaper.propertySections;
    }
    return [
      {
        key: "general",
        label: copy.propertiesFallbackSection,
        order: 0,
        condition: null,
        items: inspectorProperties
          .filter((property) => property.presentation !== "decoration")
          .map((property) => ({
            kind: "property" as const,
            key: property.key,
            text: null,
            order: property.order ?? null,
            condition: property.condition ?? null,
          })),
      },
    ];
  }, [copy.propertiesFallbackSection, inspectorProperties, previewWallpaper]);
  const visiblePropertyCount = useMemo(() => {
    if (!previewWallpaper) {
      return 0;
    }
    return visibleSectionControlCount(
      inspectorSections,
      inspectorPropertyMap,
      previewPropertyMap,
    );
  }, [inspectorPropertyMap, inspectorSections, previewPropertyMap, previewWallpaper]);

  useEffect(() => {
    setDraftValues({});
    inFlightPropertySignatures.current.clear();
  }, [selectedId]);

  useEffect(() => {
    if (wallpapers.length === 0) {
      if (selectedId !== null) {
        setSelectedId(null);
      }
      if (selectionMode !== "auto") {
        setSelectionMode("auto");
      }
      return;
    }

    const hasSelectedWallpaper = selectedId
      ? wallpapers.some((record) => record.id === selectedId)
      : false;
    const hasActiveWallpaper = activeWallpaperId
      ? wallpapers.some((record) => record.id === activeWallpaperId)
      : false;
    const fallbackSelectionId = hasActiveWallpaper
      ? activeWallpaperId
      : wallpapers[0]?.id ?? null;

    if (!hasSelectedWallpaper) {
      if (selectedId !== fallbackSelectionId) {
        setSelectedId(fallbackSelectionId);
      }
      if (selectionMode !== "auto") {
        setSelectionMode("auto");
      }
      return;
    }

    if (selectionMode === "auto" && hasActiveWallpaper && selectedId !== activeWallpaperId) {
      setSelectedId(activeWallpaperId);
    }
  }, [activeWallpaperId, selectedId, selectionMode, wallpapers]);

  useEffect(() => {
    if (activeWallpaperId && isApplyingWallpaperId === activeWallpaperId) {
      setIsApplyingWallpaperId(null);
      setLastApplyError((current) => (current?.id === activeWallpaperId ? null : current));
    }
  }, [activeWallpaperId, isApplyingWallpaperId]);

  useEffect(() => {
    if (window.__WALLPAPER_PLAYER__) {
      return;
    }

    const onDrop = async (event: DragEvent) => {
      event.preventDefault();
      const file = event.dataTransfer?.files?.[0] as File & { path?: string };
      if (!file?.path) {
        setBanner({
          tone: "warning",
          key: "dropWithoutPath",
        });
        return;
      }
      await handleImport(file.path);
    };

    const onDragOver = (event: DragEvent) => {
      event.preventDefault();
    };

    window.addEventListener("drop", onDrop);
    window.addEventListener("dragover", onDragOver);

    return () => {
      window.removeEventListener("drop", onDrop);
      window.removeEventListener("dragover", onDragOver);
    };
  }, []);

  async function handleWallpaperCardClick(record: WallpaperRuntimeRecord) {
    setSelectionMode("manual");
    setSelectedId(record.id);
    setLastApplyError((current) => (current?.id === record.id ? null : current));

    if (record.id === activeWallpaperId || record.id === isApplyingWallpaperId) {
      return;
    }

    const requestId = applyRequestRef.current + 1;
    applyRequestRef.current = requestId;
    setIsApplyingWallpaperId(record.id);
    setLastApplyError(null);
    const applyStartedAt = window.performance.now();

    try {
      const updated = await applyDynamicWallpaperWithGuiTimeout(record.id);
      if (applyRequestRef.current !== requestId) {
        return;
      }

      setWallpapers((current) =>
        current.map((item) => (item.id === updated.id ? updated : item)),
      );
      setBanner({
        tone: "success",
        key: "applySuccess",
        values: {
          title: updated.title,
        },
      });
    } catch (error) {
      if (applyRequestRef.current !== requestId) {
        return;
      }

      const nextError = String(error);
      setLastApplyError({
        id: record.id,
        error: nextError,
      });
      setBanner({
        tone: "warning",
        key: "applyFailed",
        values: {
          error: nextError,
        },
      });
    } finally {
      if (applyRequestRef.current === requestId) {
        setIsApplyingWallpaperId((current) => (current === record.id ? null : current));
        window.console.info(
          `[wallpaper-apply] stage=gui_loading_cleared status=done id=${record.id} elapsed_ms=${Math.round(
            window.performance.now() - applyStartedAt,
          )}`,
        );
      }
    }
  }

  async function handleImport(path: string) {
    setIsImporting(true);
    setBanner({
      tone: "neutral",
      key: "importing",
    });
    try {
      const record = await importWallpaper(path);
      const refreshed = await listWallpapers();
      setWallpapers(refreshed);
      setSelectionMode("manual");
      setSelectedId(record.id);
      setLastApplyError(null);
      setBanner({
        tone: "success",
        key: "importSuccess",
        values: {
          title: record.title,
        },
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "importFailed",
        values: {
          error: String(error),
        },
      });
    } finally {
      setIsImporting(false);
    }
  }

  async function handleOpenImport() {
    const path = await chooseImportDirectory(
      preferences.language === "zh-CN"
        ? "选择 Windows 壁纸目录"
        : "Choose a Windows wallpaper directory",
    );
    if (!path) {
      return;
    }
    await handleImport(path);
  }

  async function handleChooseSceneAssets() {
    const path = await chooseSceneAssetsDirectory(
      preferences.language === "zh-CN"
        ? "选择 Scene 外部 assets 目录"
        : "Choose a Scene external assets directory",
    );
    if (!path) {
      return;
    }

    try {
      const next = await updateExternalAssetsPath(path);
      setBanner({
        tone: "success",
        key: "sceneAssetsMounted",
        values: {
          path: next.externalAssetsPath ?? path,
        },
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "sceneAssetsUpdateFailed",
        values: {
          error: String(error),
        },
      });
    }
  }

  async function handleClearSceneAssets() {
    try {
      await updateExternalAssetsPath(null);
      setBanner({
        tone: "neutral",
        key: "sceneAssetsCleared",
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "sceneAssetsUpdateFailed",
        values: {
          error: String(error),
        },
      });
    }
  }

  async function handleChooseCacheDir() {
    const path = await chooseCacheDirectory(
      preferences.language === "zh-CN"
        ? "选择缓存存储目录"
        : "Choose a cache storage directory",
    );
    if (!path) {
      return;
    }

    try {
      await updateCacheStoragePath(path);
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "cachePathSetFailed",
        values: {
          error: String(error),
        },
      });
    }
  }

  async function handleClearCache() {
    try {
      await clearCache();
      setBanner({
        tone: "neutral",
        key: "cacheCleared",
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "cacheClearFailed",
        values: {
          error: String(error),
        },
      });
    }
  }

  async function handlePropertyCommit(property: WallpaperProperty, value: unknown) {
    if (!selected) {
      return;
    }
    const persisted = selected.propertySchema.find((item) => item.key === property.key);
    if (persisted && valuesEqual(persisted.value, value)) {
      setDraftValues((current) => {
        if (!Object.prototype.hasOwnProperty.call(current, property.key)) {
          return current;
        }
        const next = { ...current };
        delete next[property.key];
        return next;
      });
      return;
    }

    const signature = `${selected.id}:${property.key}:${serializePropertyValue(value)}`;
    if (inFlightPropertySignatures.current.has(signature)) {
      return;
    }

    inFlightPropertySignatures.current.add(signature);
    try {
      const updated = await setWallpaperProperties(selected.id, {
        [property.key]: value,
      });
      startTransition(() => {
        setWallpapers((current) =>
          current.map((record) => (record.id === updated.id ? updated : record)),
        );
        setDraftValues((current) => {
          if (!Object.prototype.hasOwnProperty.call(current, property.key)) {
            return current;
          }
          const next = { ...current };
          delete next[property.key];
          return next;
        });
      });
      setBanner({
        tone: "success",
        key: "propertySaved",
        values: {
          label: property.label,
        },
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "propertySaveFailed",
        values: {
          error: String(error),
        },
      });
    } finally {
      inFlightPropertySignatures.current.delete(signature);
    }
  }

  function handlePropertyPreview(property: WallpaperProperty, value: unknown) {
    setDraftValues((current) => {
      if (valuesEqual(current[property.key], value)) {
        return current;
      }
      return {
        ...current,
        [property.key]: value,
      };
    });
  }

  async function handlePauseToggle() {
    try {
      const next = await pauseResumeDynamic(!paused);
      setBanner({
        tone: "neutral",
        key: next ? "playerPaused" : "playerResumed",
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "playerToggleFailed",
        values: {
          error: String(error),
        },
      });
    }
  }

  async function handleRemove() {
    if (!selected) {
      return;
    }
    const selectedIndex = visibleWallpapers.findIndex((record) => record.id === selected.id);
    const adjacentSelectionId =
      (selectedIndex >= 0
        ? (visibleWallpapers[selectedIndex + 1] ?? visibleWallpapers[selectedIndex - 1])?.id
        : null) ?? null;
    try {
      await removeWallpaper(selected.id);
      const refreshed = await listWallpapers();
      setWallpapers(refreshed);
      setSelectionMode("manual");
      setSelectedId(
        adjacentSelectionId && refreshed.some((record) => record.id === adjacentSelectionId)
          ? adjacentSelectionId
          : refreshed[0]?.id ?? null,
      );
      setDraftValues({});
      setLastApplyError(null);
      setIsApplyingWallpaperId(null);
      setBanner({
        tone: "neutral",
        key: "removeSuccess",
        values: {
          title: selected.title,
        },
      });
    } catch (error) {
      setBanner({
        tone: "warning",
        key: "removeFailed",
        values: {
          error: String(error),
        },
      });
    }
  }

  return (
    <main className="app-shell workbench-shell">
      <LibraryPane
        appVersion={appVersion}
        wallpapers={visibleWallpapers}
        totalCount={orderedWallpapers.length}
        selectedId={selectedId}
        activeWallpaperId={activeWallpaperId}
        isApplyingWallpaperId={isApplyingWallpaperId}
        searchQuery={searchQuery}
        typeFilter={typeFilter}
        tagFilter={tagFilter}
        availableTags={availableTags}
        isImporting={isImporting}
        copy={copy}
        resolvedTheme={resolvedTheme}
        preferences={preferences}
        audioOutputDeviceOptions={audioOutputDeviceOptions}
        sceneRuntimeSettings={sceneRuntimeSettings}
        sceneRuntimeSettingsLoading={sceneRuntimeSettingsLoading}
        sceneRuntimeSettingsSaving={sceneRuntimeSettingsSaving}
        sceneCacheClearing={sceneCacheClearing}
        onSearchChange={setSearchQuery}
        onTypeFilterChange={setTypeFilter}
        onTagFilterChange={setTagFilter}
        onSortChange={(value) => updatePreference("sortKey", value)}
        onThemeModeChange={(value) => updatePreference("themeMode", value)}
        onLanguageChange={(value) => updatePreference("language", value)}
        onGuiOpacityChange={(value) => updatePreference("guiOpacity", value)}
        onAudioOutputVolumeChange={(value) => updatePreference("audioOutputVolume", value)}
        onAudioOutputDeviceChange={(value) => updatePreference("audioOutputDevice", value)}
        onChooseSceneAssets={() => void handleChooseSceneAssets()}
        onClearSceneAssets={() => void handleClearSceneAssets()}
        onChooseCacheDir={() => void handleChooseCacheDir()}
        onClearCache={() => void handleClearCache()}
        onSelect={(record) => void handleWallpaperCardClick(record)}
        onOpenImport={() => void handleOpenImport()}
      />

      <aside className="detail-panel">
        <div ref={detailScrollRef} className="detail-scroll">
          <WallpaperDetailPane
            wallpaper={previewWallpaper}
            activeWallpaper={activeWallpaper}
            isApplying={isApplyingWallpaperId === selected?.id}
            lastApplyError={lastApplyError && lastApplyError.id === selected?.id ? lastApplyError.error : null}
            diagnostics={playerState.diagnostics ?? []}
            banner={banner}
            paused={paused}
            copy={copy}
            visiblePropertyCount={visiblePropertyCount}
            inspectorProperties={inspectorProperties}
            inspectorSections={inspectorSections}
            detailScrollRef={detailScrollRef}
            onPauseToggle={() => void handlePauseToggle()}
            onRemove={() => void handleRemove()}
            onPreview={(property, value) => handlePropertyPreview(property, value)}
            onCommit={(property, value) => void handlePropertyCommit(property, value)}
          />
        </div>
      </aside>
    </main>
  );
}
