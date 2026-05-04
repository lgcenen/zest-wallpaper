import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  toAssetUrl: vi.fn((path?: string | null) => (path ? `asset://${path}` : null)),
  usePlayerController: vi.fn(),
}));

vi.mock("../src/tauri", () => ({
  toAssetUrl: mocks.toAssetUrl,
}));

vi.mock("../src/state/player-controller", () => ({
  usePlayerController: mocks.usePlayerController,
}));

import { PlayerAppShell } from "../src/app-shell/player-runtime";
import type { PlayerRuntimeState, WallpaperRuntimeRecord } from "../src/types";

function sceneWallpaper(): WallpaperRuntimeRecord {
  return {
    id: "synthetic-scene",
    title: "Synthetic Native Scene",
    wallpaperType: "scene",
    sourcePath: "/fixtures/source/native-scene",
    managedPath: "/fixtures/managed/native-scene",
    previewPath: "/fixtures/managed/native-scene/preview.png",
    entryPath: null,
    propertySchema: [],
    propertySections: [],
    importedAt: "2026-05-04T00:00:00.000Z",
    tags: [],
    runtime: {
      kind: "scene",
      scene: {
        manifestPath: "/fixtures/managed/native-scene/extracted/scene.json",
        evaluated: {
          canvasWidth: 1920,
          canvasHeight: 1080,
          clearColor: null,
          camera: {
            zoom: 1,
            cameraShake: false,
            cameraShakeAmplitude: 0,
            cameraShakeSpeed: 0,
            parallaxMouseInfluence: 0,
          },
          parallax: { enabled: false },
          renderList: [],
          objects: {},
        },
      },
    },
  };
}

describe("phase-11 player runtime cutover", () => {
  it("routes scene wallpapers to the native Metal host placeholder without frontend media fallback", () => {
    const state: PlayerRuntimeState = {
      active: sceneWallpaper(),
      paused: false,
      diagnostics: [],
    };
    mocks.usePlayerController.mockReturnValue(state);
    mocks.toAssetUrl.mockClear();

    const { container } = render(<PlayerAppShell />);

    expect(screen.getByText("Synthetic Native Scene 将由原生 Metal Scene 宿主加载。")).toBeTruthy();
    expect(container.querySelector(".scene-stage")).toBeNull();
    expect(container.querySelector("video")).toBeNull();
    expect(container.querySelector("audio")).toBeNull();
    expect(container.querySelector("canvas")).toBeNull();
    expect(mocks.toAssetUrl).not.toHaveBeenCalled();
  });
});
