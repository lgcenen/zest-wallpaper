import { useEffect, useState } from "react";
import {
  getSceneRuntimeSettings,
  setSceneExternalAssetsPath,
} from "../gateway";
import type { SceneRuntimeSettingsSnapshot } from "../types";

const DEFAULT_SETTINGS: SceneRuntimeSettingsSnapshot = {
  externalAssetsPath: null,
  externalAssetsExists: false,
};

export function useSceneRuntimeSettings() {
  const [settings, setSettings] =
    useState<SceneRuntimeSettingsSnapshot>(DEFAULT_SETTINGS);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

    return () => {
      active = false;
    };
  }, []);

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

  return {
    settings,
    loading,
    saving,
    error,
    updateExternalAssetsPath,
  };
}
