(() => {
  if (typeof window.__wallpaperApplyRuntimeMessage === "function") {
    return;
  }

  const NATIVE_BRIDGE_HANDLER = "wallpaperBridge";

  const state = {
    properties: Object.create(null),
    paused: false,
    audioSamples: Array.from({ length: 128 }, () => 0),
    lastCursorTarget: null,
  };
  const audioListeners = new Set();
  let nativeReadyPosted = false;

  function normalizeProperties(input) {
    const next = Object.create(null);
    if (!input || typeof input !== "object") {
      return next;
    }
    for (const [key, rawValue] of Object.entries(input)) {
      if (
        rawValue &&
        typeof rawValue === "object" &&
        Object.prototype.hasOwnProperty.call(rawValue, "value")
      ) {
        next[key] = rawValue;
      } else {
        next[key] = { value: rawValue };
      }
    }
    return next;
  }

  function emitChanges(changes) {
    if (!changes.length || typeof currentListener.onChanged !== "function") {
      return;
    }
    try {
      currentListener.onChanged(changes);
    } catch {}
  }

  function applyProperties(input) {
    const normalized = normalizeProperties(input);
    const changes = [];
    for (const [key, property] of Object.entries(normalized)) {
      state.properties[key] = property;
      changes.push({
        name: key,
        value:
          property && typeof property === "object" ? property.value : property,
      });
    }
    try {
      currentListener.applyUserProperties?.(normalized);
    } catch {}
    emitChanges(changes);
  }

  function pauseMedia(paused) {
    state.paused = Boolean(paused);
    const elements = document.querySelectorAll("video, audio");
    elements.forEach((element) => {
      try {
        if (state.paused) {
          element.pause();
        } else {
          const playPromise = element.play?.();
          if (playPromise && typeof playPromise.catch === "function") {
            playPromise.catch(() => {});
          }
        }
      } catch {}
    });
  }

  function createMousePayload(type, cursor, relatedTarget) {
    return {
      clientX: cursor.x,
      clientY: cursor.y,
      screenX: cursor.x,
      screenY: cursor.y,
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      relatedTarget: relatedTarget ?? null,
    };
  }

  function dispatchMouseEvent(target, type, cursor, relatedTarget) {
    if (!target) {
      return;
    }
    try {
      target.dispatchEvent(
        new MouseEvent(type, createMousePayload(type, cursor, relatedTarget)),
      );
    } catch {}
    if (typeof PointerEvent === "function" && type === "mousemove") {
      try {
        target.dispatchEvent(
          new PointerEvent("pointermove", {
            ...createMousePayload(type, cursor, relatedTarget),
            pointerId: 1,
            pointerType: "mouse",
            isPrimary: true,
          }),
        );
      } catch {}
    }
  }

  function resolveCursorTarget(cursor) {
    const x = Math.max(0, Number(cursor.x) || 0);
    const y = Math.max(0, Number(cursor.y) || 0);
    return (
      document.elementFromPoint(x, y) ||
      document.querySelector("canvas") ||
      document.body ||
      document.documentElement
    );
  }

  function dispatchCursor(cursor) {
    if (!cursor || typeof cursor.x !== "number" || typeof cursor.y !== "number") {
      return;
    }

    const target = resolveCursorTarget(cursor);
    const previousTarget = state.lastCursorTarget;

    if (previousTarget && previousTarget !== target) {
      dispatchMouseEvent(previousTarget, "mouseout", cursor, target);
      dispatchMouseEvent(previousTarget, "mouseleave", cursor, target);
    }

    if (target && previousTarget !== target) {
      dispatchMouseEvent(target, "mouseover", cursor, previousTarget);
      dispatchMouseEvent(target, "mouseenter", cursor, previousTarget);
    }

    dispatchMouseEvent(target, "mousemove", cursor, previousTarget);
    dispatchMouseEvent(document, "mousemove", cursor, previousTarget);
    dispatchMouseEvent(window, "mousemove", cursor, previousTarget);

    state.lastCursorTarget = target;
  }

  function dispatchAudioSamples(samples) {
    if (!Array.isArray(samples)) {
      return;
    }
    state.audioSamples = samples.map((value) => {
      const numeric = Number(value);
      if (!Number.isFinite(numeric)) {
        return 0;
      }
      return Math.max(0, numeric);
    });

    audioListeners.forEach((listener) => {
      try {
        listener(state.audioSamples);
      } catch {}
    });
  }

  function postNativeBridgeMessage(payload) {
    const handler =
      window.webkit &&
      window.webkit.messageHandlers &&
      window.webkit.messageHandlers[NATIVE_BRIDGE_HANDLER];
    if (!handler || typeof handler.postMessage !== "function") {
      return false;
    }
    try {
      handler.postMessage(JSON.stringify(payload));
      return true;
    } catch {
      return false;
    }
  }

  function notifyNativeReady() {
    if (nativeReadyPosted) {
      return true;
    }
    nativeReadyPosted = postNativeBridgeMessage({
      type: "wallpaper:bridge-ready",
      href: window.location.href,
    });
    return nativeReadyPosted;
  }

  function syncNativeAudioInterest() {
    postNativeBridgeMessage({
      type: "wallpaper:audio-listener",
      href: window.location.href,
      active: audioListeners.size > 0,
    });
  }

  function wrapListener(candidate) {
    const source = candidate && typeof candidate === "object" ? candidate : {};
    const wrapped = { ...source };
    wrapped.setProperty = function setProperty(key, value) {
      state.properties[key] = { value };
      try {
        source.setProperty?.(key, value);
      } catch {}
      applyProperties({ [key]: { value } });
    };
    wrapped.getPropertyValue = function getPropertyValue(key) {
      return state.properties[key] ? state.properties[key].value : null;
    };
    return wrapped;
  }

  let currentListener = wrapListener(window.wallpaperPropertyListener);

  Object.defineProperty(window, "wallpaperPropertyListener", {
    configurable: true,
    enumerable: true,
    get() {
      return currentListener;
    },
    set(value) {
      currentListener = wrapListener(value);
      queueMicrotask(() => applyProperties(state.properties));
    },
  });

  window.wallpaperInitAPI = function wallpaperInitAPI() {
    applyProperties(state.properties);
    return true;
  };

  window.wallpaperRegisterAudioListener = function wallpaperRegisterAudioListener(listener) {
    if (typeof listener !== "function") {
      return false;
    }
    const wasEmpty = audioListeners.size === 0;
    audioListeners.add(listener);
    try {
      listener(state.audioSamples);
    } catch {}
    if (wasEmpty) {
      syncNativeAudioInterest();
    }
    return true;
  };

  window.wallpaperUnregisterAudioListener = function wallpaperUnregisterAudioListener(listener) {
    const deleted = audioListeners.delete(listener);
    if (deleted && audioListeners.size === 0) {
      syncNativeAudioInterest();
    }
    return true;
  };

  function applyBridgeMessage(payload) {
    if (!payload || typeof payload !== "object") {
      return false;
    }
    if (payload.type === "wallpaper:properties") {
      applyProperties(payload.properties);
      return true;
    }
    if (payload.type === "wallpaper:paused") {
      pauseMedia(payload.paused);
      return true;
    }
    if (payload.type === "wallpaper:cursor") {
      dispatchCursor(payload.cursor);
      return true;
    }
    if (payload.type === "wallpaper:audio") {
      dispatchAudioSamples(payload.samples);
      return true;
    }
    return false;
  }

  window.__wallpaperApplyRuntimeMessage = applyBridgeMessage;

  if (document.readyState === "loading") {
    document.addEventListener(
      "DOMContentLoaded",
      () => {
        applyProperties(state.properties);
        notifyNativeReady();
        syncNativeAudioInterest();
      },
      { once: true },
    );
  } else {
    queueMicrotask(() => {
      applyProperties(state.properties);
      notifyNativeReady();
      syncNativeAudioInterest();
    });
  }

  window.addEventListener(
    "load",
    () => {
      notifyNativeReady();
    },
    { once: true },
  );
  window.addEventListener("pageshow", () => {
    nativeReadyPosted = false;
    notifyNativeReady();
    syncNativeAudioInterest();
  });
})();
