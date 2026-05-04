import { useEffect, useState } from "react";
import type {
  PlayerRuntimePlaybackState,
  PlayerRuntimeState,
  RuntimeDiagnostic,
  WallpaperRuntimeRecord,
} from "../types";
import {
  getPlayerDiagnostics,
  getPlayerState,
  onPlayerDiagnostics,
  onPlayerLoad,
  onPlayerPause,
  onPlayerUpdate,
} from "../gateway";

const initialPlayerState: PlayerRuntimeState = {
  active: null,
  paused: false,
  diagnostics: [],
};

function mergePlaybackState(
  current: PlayerRuntimeState,
  playback: PlayerRuntimePlaybackState,
): PlayerRuntimeState {
  return {
    ...current,
    active: playback.active ?? null,
    paused: playback.paused,
  };
}

export function usePlayerController() {
  const [state, setState] = useState<PlayerRuntimeState>(initialPlayerState);

  useEffect(() => {
    let unlistenLoad: (() => void) | undefined;
    let unlistenPause: (() => void) | undefined;
    let unlistenUpdate: (() => void) | undefined;
    let unlistenDiagnostics: (() => void) | undefined;

    getPlayerState()
      .then((playback) => {
        setState((current) => mergePlaybackState(current, playback));
      })
      .catch(() => undefined);

    getPlayerDiagnostics()
      .then((diagnostics: RuntimeDiagnostic[]) => {
        setState((current) => ({
          ...current,
          diagnostics,
        }));
      })
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

    void onPlayerDiagnostics((diagnostics) => {
      setState((current) => ({
        ...current,
        diagnostics,
      }));
    }).then((callback) => {
      unlistenDiagnostics = callback;
    });

    return () => {
      unlistenLoad?.();
      unlistenPause?.();
      unlistenUpdate?.();
      unlistenDiagnostics?.();
    };
  }, []);

  return state;
}
