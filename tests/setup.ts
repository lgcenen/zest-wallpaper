import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

const storage = (() => {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
    clear: () => {
      values.clear();
    },
    key: (index: number) => Array.from(values.keys())[index] ?? null,
    get length() {
      return values.size;
    },
  };
})();

beforeEach(() => {
  window.__WALLPAPER_PLAYER__ = false;
  Object.defineProperty(window.navigator, "language", {
    configurable: true,
    value: "zh-CN",
  });
  Object.defineProperty(window, "localStorage", {
    writable: true,
    value: storage,
  });
  Object.defineProperty(globalThis, "localStorage", {
    writable: true,
    value: storage,
  });
  storage.clear();

  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query.includes("dark"),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });

  Object.defineProperty(window, "requestAnimationFrame", {
    writable: true,
    value: (callback: FrameRequestCallback) => window.setTimeout(() => callback(0), 0),
  });

  Object.defineProperty(window, "cancelAnimationFrame", {
    writable: true,
    value: (handle: number) => window.clearTimeout(handle),
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  storage.clear();
  delete document.documentElement.dataset.theme;
  delete document.documentElement.dataset.workbenchTheme;
  document.documentElement.style.colorScheme = "";
});
