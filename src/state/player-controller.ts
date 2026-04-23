import { useEffect, useState } from "react";
import type { PlayerRuntimeState, WallpaperRuntimeRecord } from "../types";
import {
  getPlayerState,
  onPlayerLoad,
  onPlayerPause,
  onPlayerUpdate,
} from "../gateway";

export function usePlayerController() {
  const [state, setState] = useState<PlayerRuntimeState>({ active: null, paused: false });

  useEffect(() => {
    let unlistenLoad: (() => void) | undefined;
    let unlistenPause: (() => void) | undefined;
    let unlistenUpdate: (() => void) | undefined;

    getPlayerState()
      .then(setState)
      .catch(() => undefined);

    void onPlayerLoad((wallpaper: WallpaperRuntimeRecord | null) => {
      setState((current) => ({
        ...current,
        active: wallpaper,
      }));
    }).then((callback) => {
      unlistenLoad = callback;
    });

    void onPlayerPause((paused) => {
      setState((current) => ({
        ...current,
        paused,
      }));
    }).then((callback) => {
      unlistenPause = callback;
    });

    void onPlayerUpdate((wallpaper) => {
      setState((current) => ({
        ...current,
        active: wallpaper,
      }));
    }).then((callback) => {
      unlistenUpdate = callback;
    });

    return () => {
      unlistenLoad?.();
      unlistenPause?.();
      unlistenUpdate?.();
    };
  }, []);

  return state;
}
