import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const sceneRuntimeSettings = {
    externalAssetsPath: null as string | null,
    externalAssetsExists: false,
  };
  const wallpaper = {
    id: "wallpaper-aurora",
    title: "Aurora Flow",
    wallpaperType: "video" as const,
    sourcePath: "/fixtures/source/Aurora Flow",
    managedPath: "/fixtures/managed/Aurora Flow",
    previewPath: "/fixtures/managed/Aurora Flow/preview.png",
    entryPath: "/fixtures/managed/Aurora Flow/index.html",
    propertySchema: [
      {
        key: "speed",
        label: "Speed",
        markup: null,
        kind: "slider" as const,
        value: 1,
        defaultValue: 1,
        min: 0,
        max: 5,
        step: 0.5,
        condition: null,
        order: 0,
        presentation: "control" as const,
        options: [],
      },
    ],
    propertySections: [
      {
        key: "general",
        label: "General",
        order: 0,
        condition: null,
        items: [
          {
            kind: "property" as const,
            key: "speed",
            text: null,
            markup: null,
            order: 0,
            condition: null,
          },
        ],
      },
    ],
    importedAt: "2026-04-09T10:00:00.000Z",
    tags: ["focus", "north"],
    runtime: {
      kind: "video" as const,
      video: {
        entryPath: "/fixtures/managed/Aurora Flow/index.html",
        previewPath: "/fixtures/managed/Aurora Flow/preview.png",
        sourcePath: "/fixtures/source/Aurora Flow/video.mp4",
        managedPath: "/fixtures/managed/Aurora Flow/video.mp4",
      },
    },
  };
  const restoredWallpaper = {
    ...wallpaper,
    id: "wallpaper-neon",
    title: "Neon Drift",
    sourcePath: "/fixtures/source/Neon Drift",
    managedPath: "/fixtures/managed/Neon Drift",
    previewPath: "/fixtures/managed/Neon Drift/preview.png",
    entryPath: "/fixtures/managed/Neon Drift/index.html",
    importedAt: "2026-04-08T10:00:00.000Z",
    tags: ["city", "night"],
    runtime: {
      kind: "video" as const,
      video: {
        entryPath: "/fixtures/managed/Neon Drift/index.html",
        previewPath: "/fixtures/managed/Neon Drift/preview.png",
        sourcePath: "/fixtures/source/Neon Drift/video.mp4",
        managedPath: "/fixtures/managed/Neon Drift/video.mp4",
      },
    },
  };
  const fallbackWallpaper = {
    ...wallpaper,
    id: "wallpaper-fallback",
    title: "Missing Preview Atlas",
    wallpaperType: "scene" as const,
    sourcePath: "/fixtures/source/Missing Preview Atlas",
    managedPath: "/fixtures/managed/Missing Preview Atlas",
    previewPath: null,
    entryPath: "/fixtures/managed/Missing Preview Atlas/index.html",
    importedAt: "2026-04-07T10:00:00.000Z",
    tags: ["fallback", "atlas"],
    runtime: { kind: "unknown" as const },
  };

  return {
    wallpaper,
    restoredWallpaper,
    fallbackWallpaper,
    gateway: {
      listWallpapers: vi.fn(async () => [wallpaper, fallbackWallpaper, restoredWallpaper]),
      importWallpaper: vi.fn(async () => wallpaper),
      getWallpaperDetails: vi.fn(async () => wallpaper),
      applyDynamicWallpaper: vi.fn(async () => wallpaper),
      setWallpaperProperties: vi.fn(async () => wallpaper),
      pauseResumeDynamic: vi.fn(async (paused: boolean) => paused),
      removeWallpaper: vi.fn(async () => true),
      chooseImportDirectory: vi.fn(async () => null),
      chooseSceneAssetsDirectory: vi.fn(async () => null),
      getSceneRuntimeSettings: vi.fn(async () => ({ ...sceneRuntimeSettings })),
      setSceneExternalAssetsPath: vi.fn(async (path: string | null) => {
        sceneRuntimeSettings.externalAssetsPath = path;
        sceneRuntimeSettings.externalAssetsExists = Boolean(path);
        return { ...sceneRuntimeSettings };
      }),
      toAssetUrl: vi.fn((path?: string | null) => (path ? `asset://${path}` : null)),
      getPlayerState: vi.fn(async () => ({ active: restoredWallpaper, paused: false })),
      onPlayerLoad: vi.fn(async () => () => undefined),
      onPlayerPause: vi.fn(async () => () => undefined),
      onPlayerUpdate: vi.fn(async () => () => undefined),
    },
    usePlayerController: vi.fn(() => ({ active: restoredWallpaper, paused: false })),
  };
});

vi.mock("../src/gateway", () => mocks.gateway);
vi.mock("../src/state/player-controller", () => ({
  usePlayerController: mocks.usePlayerController,
}));

import WorkbenchApp from "../src/app-shell/WorkbenchApp";
import { WorkbenchSelect } from "../src/app-shell/WorkbenchSelect";
import { getWorkbenchCopy } from "../src/app-shell/workbench-copy";

async function chooseWorkbenchOption(
  user: ReturnType<typeof userEvent.setup>,
  triggerLabel: string,
  optionLabel: string,
) {
  await user.click(screen.getByLabelText(triggerLabel));
  await user.click(await screen.findByRole("option", { name: optionLabel }));
}

describe("phase-06 workbench gui", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.style.removeProperty("--wb-gui-opacity");
    mocks.gateway.listWallpapers.mockClear();
    mocks.gateway.applyDynamicWallpaper.mockClear();
    mocks.gateway.chooseSceneAssetsDirectory.mockClear();
    mocks.gateway.getSceneRuntimeSettings.mockClear();
    mocks.gateway.setSceneExternalAssetsPath.mockClear();
    mocks.gateway.getSceneRuntimeSettings.mockResolvedValue({
      externalAssetsPath: null,
      externalAssetsExists: false,
    });
    mocks.usePlayerController.mockReturnValue({
      active: mocks.restoredWallpaper,
      paused: false,
    });
  });

  it("prefers the restored active wallpaper in the detail pane on startup", async () => {
    const { container } = render(<WorkbenchApp />);

    await screen.findByText("本地壁纸库");
    await screen.findByRole("heading", { name: "Neon Drift" });

    expect(screen.getByRole("button", { name: "导入" })).toBeTruthy();
    expect(screen.getByLabelText("排序")).toBeTruthy();
    expect(screen.getByLabelText("搜索")).toBeTruthy();
    expect(screen.getByRole("button", { name: "设置" })).toBeTruthy();
    expect(container.querySelector("[data-tauri-drag-region]")).toBeNull();
    expect(container.querySelector("[data-window-drag-handle]")).toBeNull();
    expect(container.querySelector("[data-window-drag-ignore]")).toBeNull();
    expect(container.querySelector(".library-card-thumb")).toBeTruthy();
    expect(container.querySelector(".library-card-copy")).toBeNull();
    expect(container.querySelector(".detail-poster-shell")).toBeNull();

    const overlay = container.querySelector(".library-card-overlay");
    expect(overlay?.textContent).toContain("Video");
    expect(overlay?.textContent).not.toContain("当前桌面");
    expect(screen.queryByText("当前桌面")).toBeNull();

    expect(screen.queryByRole("heading", { name: "状态" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "摘要" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "路径" })).toBeNull();
  });

  it("persists theme, language, and gui opacity preferences across rerenders", async () => {
    const user = userEvent.setup();
    const firstRender = render(<WorkbenchApp />);

    await screen.findByText("本地壁纸库");

    await user.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.queryByLabelText("缩略图密度")).toBeNull();
    expect(screen.getByRole("heading", { name: "属性" })).toBeTruthy();
    fireEvent.input(screen.getByLabelText("GUI 透明度"), { target: { value: "70" } });
    await chooseWorkbenchOption(user, "语言", "English");

    await screen.findByText("Wallpaper Library");

    await chooseWorkbenchOption(user, "Appearance", "Light");

    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("light");
      expect(document.documentElement.dataset.workbenchTheme).toBe("light");
      expect(document.documentElement.style.getPropertyValue("--wb-gui-opacity")).toBe("0.70");
    });
    expect(
      getComputedStyle(firstRender.container.querySelector(".workbench-shell") as HTMLElement)
        .getPropertyValue("--wb-gui-opacity")
        .trim(),
    ).toBe("0.70");

    firstRender.unmount();

    const secondRender = render(<WorkbenchApp />);

    await screen.findByText("Wallpaper Library");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
    expect(
      getComputedStyle(secondRender.container.querySelector(".workbench-shell") as HTMLElement)
        .getPropertyValue("--wb-gui-opacity")
        .trim(),
    ).toBe("0.70");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect((screen.getByLabelText("GUI Opacity") as HTMLInputElement).value).toBe("70");
    fireEvent.input(screen.getByLabelText("GUI Opacity"), { target: { value: "100" } });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--wb-gui-opacity")).toBe("1.00");
    });
    expect(
      getComputedStyle(secondRender.container.querySelector(".workbench-shell") as HTMLElement)
        .getPropertyValue("--wb-gui-opacity")
        .trim(),
    ).toBe("1.00");
  });

  it("keeps no-preview library cards identifiable with a readable title fallback", async () => {
    const { container } = render(<WorkbenchApp />);

    await screen.findByText("本地壁纸库");

    const card = screen.getByRole("button", { name: mocks.fallbackWallpaper.title });
    expect(within(card).getByText(mocks.fallbackWallpaper.title)).toBeTruthy();
    expect(card.querySelector(".library-card-fallback-title")?.textContent).toBe(
      mocks.fallbackWallpaper.title,
    );
    expect(card.querySelector(".library-card-fallback-mark")?.textContent).toBe("S");
    expect(container.querySelector(".library-card-copy")).toBeNull();
  });

  it("closes the custom select menu when focus tabs away from the select root", async () => {
    const user = userEvent.setup();

    function SelectHarness() {
      const [value, setValue] = useState("recent");
      return (
        <>
          <WorkbenchSelect
            ariaLabel="排序"
            value={value}
            options={[{ value: "recent", label: "最近导入" }]}
            onChange={setValue}
          />
          <button type="button">下一个控件</button>
        </>
      );
    }

    render(<SelectHarness />);

    await user.click(screen.getByLabelText("排序"));
    expect(await screen.findByRole("listbox", { name: "排序" })).toBeTruthy();

    await user.tab();

    await waitFor(() => {
      expect(screen.queryByRole("listbox", { name: "排序" })).toBeNull();
    });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "下一个控件" }));
  });

  it("trims unused phase-06 shell copy fields while preserving active labels", () => {
    const zhCopy = getWorkbenchCopy("zh-CN") as Record<string, unknown>;
    const enCopy = getWorkbenchCopy("en") as Record<string, unknown>;

    expect(zhCopy).not.toHaveProperty("detailTitle");
    expect(zhCopy).not.toHaveProperty("statusHeading");
    expect(zhCopy).not.toHaveProperty("metadataHeading");
    expect(zhCopy).not.toHaveProperty("pathsHeading");
    expect(zhCopy).not.toHaveProperty("usageState");
    expect(enCopy).not.toHaveProperty("actionsHeading");
    expect(enCopy).not.toHaveProperty("feedbackLabel");
    expect(zhCopy.propertiesHeading).toBe("属性");
    expect(enCopy.propertiesHeading).toBe("Properties");
    expect(zhCopy.desktopLabel).toBe("当前桌面");
    expect(enCopy.desktopLabel).toBe("Desktop");
  });
});
