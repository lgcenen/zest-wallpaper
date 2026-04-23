import { useEffect, useState } from "react";

export type WorkbenchThemeMode = "light" | "dark" | "system";
export type WorkbenchResolvedTheme = "light" | "dark";
export type WorkbenchLanguage = "zh-CN" | "en";
export type WorkbenchSortKey = "recent" | "title";

export interface WorkbenchPreferences {
  themeMode: WorkbenchThemeMode;
  language: WorkbenchLanguage;
  sortKey: WorkbenchSortKey;
}

const STORAGE_KEY = "wallpaper-workbench.preferences";

function inferLanguage(): WorkbenchLanguage {
  if (typeof navigator === "undefined") {
    return "en";
  }
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

function defaultPreferences(): WorkbenchPreferences {
  return {
    themeMode: "system",
    language: inferLanguage(),
    sortKey: "recent",
  };
}

function isThemeMode(value: unknown): value is WorkbenchThemeMode {
  return value === "light" || value === "dark" || value === "system";
}

function isLanguage(value: unknown): value is WorkbenchLanguage {
  return value === "zh-CN" || value === "en";
}

function isSortKey(value: unknown): value is WorkbenchSortKey {
  return value === "recent" || value === "title";
}

function systemTheme(): WorkbenchResolvedTheme {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "dark";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function readStoredWorkbenchPreferences(): WorkbenchPreferences {
  const fallback = defaultPreferences();
  if (typeof window === "undefined" || typeof window.localStorage === "undefined") {
    return fallback;
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return fallback;
    }
    const parsed = JSON.parse(raw) as Partial<WorkbenchPreferences>;
    return {
      themeMode: isThemeMode(parsed.themeMode) ? parsed.themeMode : fallback.themeMode,
      language: isLanguage(parsed.language) ? parsed.language : fallback.language,
      sortKey: isSortKey(parsed.sortKey) ? parsed.sortKey : fallback.sortKey,
    };
  } catch {
    return fallback;
  }
}

function writeStoredWorkbenchPreferences(preferences: WorkbenchPreferences) {
  if (typeof window === "undefined" || typeof window.localStorage === "undefined") {
    return;
  }

  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
}

function resolveTheme(preferences: WorkbenchPreferences, detectedTheme: WorkbenchResolvedTheme) {
  return preferences.themeMode === "system" ? detectedTheme : preferences.themeMode;
}

export function applyWorkbenchDocumentPreferences(
  preferences: WorkbenchPreferences,
  resolvedTheme: WorkbenchResolvedTheme = resolveTheme(preferences, systemTheme()),
) {
  if (typeof document === "undefined") {
    return;
  }

  document.documentElement.dataset.theme = resolvedTheme;
  document.documentElement.dataset.workbenchTheme = preferences.themeMode;
  document.documentElement.lang = preferences.language;
  document.documentElement.style.colorScheme = resolvedTheme;
}

export function initializeWorkbenchDocumentPreferences() {
  const preferences = readStoredWorkbenchPreferences();
  applyWorkbenchDocumentPreferences(preferences);
}

export function useWorkbenchPreferences() {
  const [preferences, setPreferences] = useState<WorkbenchPreferences>(() =>
    readStoredWorkbenchPreferences(),
  );
  const [detectedTheme, setDetectedTheme] = useState<WorkbenchResolvedTheme>(() => systemTheme());
  const resolvedTheme = resolveTheme(preferences, detectedTheme);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const syncTheme = () => {
      setDetectedTheme(mediaQuery.matches ? "dark" : "light");
    };
    syncTheme();

    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", syncTheme);
      return () => mediaQuery.removeEventListener("change", syncTheme);
    }

    mediaQuery.addListener(syncTheme);
    return () => mediaQuery.removeListener(syncTheme);
  }, []);

  useEffect(() => {
    writeStoredWorkbenchPreferences(preferences);
  }, [preferences]);

  useEffect(() => {
    applyWorkbenchDocumentPreferences(preferences, resolvedTheme);
  }, [preferences, resolvedTheme]);

  function updatePreference<Key extends keyof WorkbenchPreferences>(
    key: Key,
    value: WorkbenchPreferences[Key],
  ) {
    setPreferences((current) => {
      if (Object.is(current[key], value)) {
        return current;
      }
      return {
        ...current,
        [key]: value,
      };
    });
  }

  return {
    preferences,
    resolvedTheme,
    setPreferences,
    updatePreference,
  };
}
