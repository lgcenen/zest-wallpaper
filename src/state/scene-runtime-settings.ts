import { useEffect, useState } from "react";
import {
  clearSceneCache,
  getSceneCacheSize,
  getSceneRuntimeSettings,
  setCacheStoragePath,
  setSceneExternalAssetsPath,
} from "../gateway";
import type { SceneRuntimeSettingsSnapshot } from "../types";

const DEFAULT_SETTINGS: SceneRuntimeSettingsSnapshot = {
  externalAssetsPath: null,
  externalAssetsExists: false,
  cacheStoragePath: null,
  cacheStorageExists: false,
};

export function useSceneRuntimeSettings() {
  const [settings, setSettings] =
    useState<SceneRuntimeSettingsSnapshot>(DEFAULT_SETTINGS);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  const [cacheSize, setCacheSize] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);

    void getSceneRuntimeSettings()
      .then((next) => {
        if (!active) {
          return;
        }
        setSettings(next);
      })
      .catch((nextError) => {
        if (!active) {
          return;
        }
        setError(String(nextError));
      })
      .finally(() => {
        if (!active) {
          return;
        }
        setLoading(false);
      });

    void getSceneCacheSize()
      .then((size) => {
        if (active) setCacheSize(size);
      })
      .catch(() => {
        if (active) setCacheSize(0);
      });

    return () => {
      active = false;
    };
  }, []);

  async function refreshCacheSize() {
    try {
      const size = await getSceneCacheSize();
      setCacheSize(size);
    } catch {
      setCacheSize(null);
    }
  }

  async function updateExternalAssetsPath(path: string | null) {
    setSaving(true);
    try {
      const next = await setSceneExternalAssetsPath(path);
      setSettings(next);
      return next;
    } catch (nextError) {
      throw nextError;
    } finally {
      setSaving(false);
    }
  }

  async function updateCacheStoragePath(path: string | null) {
    setSaving(true);
    try {
      const next = await setCacheStoragePath(path);
      setSettings(next);
      await refreshCacheSize();
      return next;
    } catch (nextError) {
      throw nextError;
    } finally {
      setSaving(false);
    }
  }

  async function clearCache() {
    setClearing(true);
    try {
      await clearSceneCache();
      await refreshCacheSize();
    } catch (nextError) {
      throw nextError;
    } finally {
      setClearing(false);
    }
  }

  return {
    settings,
    loading,
    saving,
    error,
    clearing,
    cacheSize,
    updateExternalAssetsPath,
    updateCacheStoragePath,
    clearCache,
    refreshCacheSize,
  };
}
