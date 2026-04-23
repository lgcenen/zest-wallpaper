import { useEffect } from "react";
import type { WorkbenchBannerState } from "../app-shell/workbench-copy";
import type { WallpaperRuntimeRecord } from "../types";
import { listWallpapers } from "../gateway";

interface UseWorkbenchControllerOptions {
  enabled: boolean;
  setWallpapers: (
    updater:
      | WallpaperRuntimeRecord[]
      | ((current: WallpaperRuntimeRecord[]) => WallpaperRuntimeRecord[]),
  ) => void;
  setBanner: (banner: WorkbenchBannerState) => void;
}

export function useWorkbenchController({
  enabled,
  setWallpapers,
  setBanner,
}: UseWorkbenchControllerOptions) {
  useEffect(() => {
    if (!enabled) {
      return;
    }

    void listWallpapers()
      .then((records) => {
        setWallpapers(records);
      })
      .catch((error) => {
        setBanner({
          tone: "warning",
          key: "libraryReadFailed",
          values: {
            error: String(error),
          },
        });
      });
  }, [enabled, setBanner, setWallpapers]);
}
