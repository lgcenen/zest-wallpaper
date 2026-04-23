import { useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  getPlayerAudioSnapshot,
  getPlayerInputSnapshot,
  onPlayerAudio,
  onPlayerInput,
  setSceneAudioInterest,
  toAssetUrl,
} from "../tauri";
import type {
  EvaluatedAudioState,
  EvaluatedSceneObject,
  EvaluatedTextState,
  PlayerRuntimeState,
  SceneRuntimeDocument,
  SharedAudioSnapshot,
  SharedInputSnapshot,
  VideoRuntimeDocument,
  WallpaperRuntimeRecord,
  WebRuntimeDocument,
} from "../types";
import { usePlayerController } from "../state/player-controller";

interface MediaSize {
  width: number;
  height: number;
}

interface SceneCursorState {
  x: number;
  y: number;
  active: boolean;
  timestamp: number;
}

interface TrailPoint {
  id: number;
  x: number;
  y: number;
  size: number;
  createdAt: number;
  color: string;
}

interface PetalParticle {
  id: number;
  x: number;
  y: number;
  vx: number;
  vy: number;
  rotation: number;
  spin: number;
  size: number;
  bornAt: number;
  lifeMs: number;
  color: string;
}

const DEFAULT_ASPECT_RATIO = 16 / 9;
const sceneFontFamilyCache = new Map<string, string>();
const sceneFontLoadCache = new Map<string, Promise<string | null>>();

function sceneRuntimeFor(wallpaper?: WallpaperRuntimeRecord | null): SceneRuntimeDocument | null {
  return wallpaper?.runtime.kind === "scene" ? wallpaper.runtime.scene : null;
}

function videoRuntimeFor(wallpaper?: WallpaperRuntimeRecord | null): VideoRuntimeDocument | null {
  return wallpaper?.runtime.kind === "video" ? wallpaper.runtime.video : null;
}

function webRuntimeFor(wallpaper?: WallpaperRuntimeRecord | null): WebRuntimeDocument | null {
  return wallpaper?.runtime.kind === "web" ? wallpaper.runtime.web : null;
}

function fontFamilyKey(fontPath: string) {
  let hash = 2166136261;
  for (let index = 0; index < fontPath.length; index += 1) {
    hash ^= fontPath.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return `SceneFont-${(hash >>> 0).toString(36)}`;
}

async function loadSceneFontFamily(fontPath: string) {
  const existing = sceneFontFamilyCache.get(fontPath);
  if (existing) {
    return existing;
  }

  const loading = sceneFontLoadCache.get(fontPath);
  if (loading) {
    return loading;
  }

  const task = (async () => {
    if (typeof window === "undefined" || !("FontFace" in window) || !document.fonts) {
      return null;
    }
    const source = toAssetUrl(fontPath);
    if (!source) {
      return null;
    }
    const family = fontFamilyKey(fontPath);
    const face = new FontFace(family, `url("${source}")`);
    await face.load();
    document.fonts.add(face);
    sceneFontFamilyCache.set(fontPath, family);
    return family;
  })()
    .catch(() => null)
    .finally(() => {
      sceneFontLoadCache.delete(fontPath);
    });

  sceneFontLoadCache.set(fontPath, task);
  return task;
}

function useSceneFontFamily(fontPath: string | null | undefined) {
  const [fontFamily, setFontFamily] = useState<string | null>(
    fontPath ? sceneFontFamilyCache.get(fontPath) ?? null : null,
  );

  useEffect(() => {
    if (!fontPath) {
      setFontFamily(null);
      return;
    }
    let cancelled = false;
    void loadSceneFontFamily(fontPath).then((family) => {
      if (!cancelled) {
        setFontFamily(family);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [fontPath]);

  return fontFamily;
}

function sceneCursorFromSharedInputSnapshot(
  snapshot: SharedInputSnapshot,
  sceneWidth: number,
  sceneHeight: number,
) {
  const viewportWidth = Math.max(window.innerWidth || 0, 1);
  const viewportHeight = Math.max(window.innerHeight || 0, 1);
  const localX = clamp(snapshot.globalX - window.screenX, 0, viewportWidth);
  const localY = clamp(snapshot.globalY - window.screenY, 0, viewportHeight);
  const normalizedX = clamp(localX / viewportWidth, 0, 1);
  const normalizedTopY = clamp(localY / viewportHeight, 0, 1);
  const normalizedBottomY = clamp(1 - normalizedTopY, 0, 1);

  return {
    cursor: {
      x: normalizedX * sceneWidth,
      y: normalizedBottomY * sceneHeight,
      active: snapshot.active,
      timestamp: 0,
    },
    motion: {
      x: (normalizedX - 0.5) * 2,
      y: (normalizedTopY - 0.5) * 2,
    },
  };
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function vectorColorToCss(value?: string | null, alpha = 1) {
  if (!value) {
    return `rgba(255, 255, 255, ${alpha})`;
  }
  const parts = value
    .split(/[\s,]+/)
    .map((segment) => Number.parseFloat(segment))
    .filter((segment) => Number.isFinite(segment));
  if (parts.length >= 3) {
    const [r, g, b] = parts;
    const color = [r, g, b].map((channel) =>
      channel > 1 ? clamp(channel, 0, 255) : clamp(channel * 255, 0, 255),
    );
    return `rgba(${color.map((channel) => channel.toFixed(0)).join(", ")}, ${alpha})`;
  }
  return value;
}

function blendModeToCss(blendMode?: string | null) {
  const normalized = blendMode?.trim().toLowerCase();
  switch (normalized) {
    case "translucent":
      return "normal";
    case "additive":
      return "plus-lighter";
    case "multiply":
      return "multiply";
    case "normal":
    case "opaque":
    default:
      return "normal";
  }
}

function alignmentToJustify(alignment: string | null | undefined) {
  const normalized = alignment?.toLowerCase() ?? "";
  if (normalized.includes("left")) {
    return "flex-start";
  }
  if (normalized.includes("right")) {
    return "flex-end";
  }
  return "center";
}

function alignmentToItems(alignment: string | null | undefined) {
  const normalized = alignment?.toLowerCase() ?? "";
  if (normalized.includes("top")) {
    return "flex-start";
  }
  if (normalized.includes("bottom")) {
    return "flex-end";
  }
  return "center";
}

function cursorDistance(left: SceneCursorState | null, right: SceneCursorState | null) {
  if (!left || !right) {
    return Number.POSITIVE_INFINITY;
  }
  return Math.hypot(left.x - right.x, left.y - right.y);
}

function applySceneMotionVariables(node: HTMLElement | null, motion: { x: number; y: number }) {
  if (!node) {
    return;
  }
  node.style.setProperty("--scene-motion-x-10", `${(motion.x * 10).toFixed(2)}px`);
  node.style.setProperty("--scene-motion-x-6", `${(motion.x * 6).toFixed(2)}px`);
  node.style.setProperty("--scene-motion-y-8", `${(-motion.y * 8).toFixed(2)}px`);
  node.style.setProperty("--scene-motion-y-6", `${(-motion.y * 6).toFixed(2)}px`);
  node.style.setProperty("--scene-motion-y-5", `${(-motion.y * 5).toFixed(2)}px`);
}

function pointsToSvgPath(points: TrailPoint[], sceneHeight: number) {
  if (points.length === 0) {
    return null;
  }
  return points
    .map((point, index) => {
      const x = point.x.toFixed(2);
      const y = (sceneHeight - point.y).toFixed(2);
      return `${index === 0 ? "M" : "L"}${x} ${y}`;
    })
    .join(" ");
}

function stableAudioKey(objects: EvaluatedSceneObject[]) {
  return objects
    .filter((object): object is Extract<EvaluatedSceneObject, { kind: "sound" }> => object.kind === "sound")
    .map((object) => `${object.id}:${object.assetPath}:${object.looped}:${object.volume.toFixed(4)}`)
    .join("|");
}

function deriveSceneAudioLevels(
  snapshot: SharedAudioSnapshot | null,
  count: number,
) {
  if (!snapshot?.active || count <= 0 || snapshot.smoothedBands.length === 0) {
    return Array.from({ length: count }, () => 0);
  }

  const source = snapshot.smoothedBands;
  return Array.from({ length: count }, (_, index) => {
    const start = Math.floor((index * source.length) / count);
    const end = Math.max(start + 1, Math.ceil(((index + 1) * source.length) / count));
    let sum = 0;
    let peak = 0;
    let samples = 0;

    for (let bandIndex = start; bandIndex < end && bandIndex < source.length; bandIndex += 1) {
      const value = clamp(source[bandIndex] ?? 0, 0, 1);
      sum += value;
      peak = Math.max(peak, value);
      samples += 1;
    }

    if (samples === 0) {
      return 0;
    }

    const average = sum / samples;
    return clamp(peak * 0.62 + average * 0.38, 0, 1);
  });
}

function useElementSize<T extends HTMLElement>(ref: RefObject<T>) {
  const [size, setSize] = useState({ width: 0, height: 0 });

  useEffect(() => {
    const node = ref.current;
    if (!node) {
      return;
    }

    const update = () => {
      const next = node.getBoundingClientRect();
      setSize({ width: next.width, height: next.height });
    };

    update();
    const observer = new ResizeObserver(() => update());
    observer.observe(node);
    window.addEventListener("resize", update);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
    };
  }, [ref]);

  return size;
}

function useViewportSize() {
  const [size, setSize] = useState<MediaSize>(() => ({
    width: typeof window !== "undefined" ? window.innerWidth : 0,
    height: typeof window !== "undefined" ? window.innerHeight : 0,
  }));

  useEffect(() => {
    const update = () => {
      setSize({
        width: window.innerWidth,
        height: window.innerHeight,
      });
    };

    update();
    window.addEventListener("resize", update);

    return () => {
      window.removeEventListener("resize", update);
    };
  }, []);

  return size;
}

function evaluatedObjects(scene: SceneRuntimeDocument) {
  return scene.evaluated.renderList
    .map((id) => scene.evaluated.objects[String(id)] ?? null)
    .filter((object): object is EvaluatedSceneObject => Boolean(object && object.visible));
}

function NativeVideoStageSurface({
  previewUrl,
  title,
  hasEntry,
}: {
  previewUrl?: string | null;
  title: string;
  hasEntry: boolean;
}) {
  if (previewUrl) {
    return <img className="stage-media stage-media-player" src={previewUrl} alt={title} />;
  }

  return (
    <div className="stage-empty stage-empty-player">
      <span>
        {hasEntry
          ? "原生视频播放器正在准备中。"
          : "这个视频壁纸当前没有可直接播放的入口文件。"}
      </span>
    </div>
  );
}

function NativeWebStageSurface({
  title,
  hasEntry,
}: {
  title: string;
  hasEntry: boolean;
}) {
  return (
    <div className="stage-empty stage-empty-player">
      <span>
        {hasEntry ? `${title} 将由原生 WKWebView 宿主加载。` : "这个网页壁纸当前没有可加载的 HTML 入口。"}
      </span>
    </div>
  );
}

function NativeSceneStageSurface({ title }: { title: string }) {
  return (
    <div className="stage-empty stage-empty-player">
      <span>{title} 将由原生 Metal Scene 宿主加载。</span>
    </div>
  );
}

function SceneVisualNode({
  object,
  paused,
}: {
  object: Extract<EvaluatedSceneObject, { kind: "visual" }>;
  paused?: boolean;
}) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const bounds = object.transform.renderBounds;
  const textureUrl = toAssetUrl(object.assetPath);
  const flipX = object.transform.scale[0] < 0 ? -1 : 1;
  const flipY = object.transform.scale[1] < 0 ? -1 : 1;
  const translateX = object.primary ? "var(--scene-motion-x-10, 0px)" : "var(--scene-motion-x-6, 0px)";
  const translateY = object.primary ? "var(--scene-motion-y-8, 0px)" : "var(--scene-motion-y-5, 0px)";

  useEffect(() => {
    const element = videoRef.current;
    if (!element || object.assetKind !== "video") {
      return;
    }
    if (paused) {
      element.pause();
      return;
    }
    void element.play().catch(() => undefined);
  }, [object.assetKind, object.assetPath, paused]);

  if (!bounds || object.assetKind === "unsupported") {
    return null;
  }

  return (
    <div
      className={`scene-visual-layer scene-visual-player ${
        object.backgroundCandidate ? "scene-visual-player-background" : ""
      }`}
      style={{
        left: `${object.backgroundCandidate ? 0 : bounds[0]}px`,
        top: `${object.backgroundCandidate ? 0 : bounds[1]}px`,
        width: `${object.backgroundCandidate ? "100%" : `${bounds[2]}px`}`,
        height: `${object.backgroundCandidate ? "100%" : `${bounds[3]}px`}`,
        transform: `translate(${translateX}, ${translateY}) scale(${flipX}, ${flipY}) rotate(${object.transform.rotation.toFixed(4)}rad)`,
        opacity: clamp(object.opacity, 0, 1),
        mixBlendMode: blendModeToCss(object.blendMode),
      }}
    >
      {textureUrl && object.assetKind === "video" ? (
        <video
          ref={videoRef}
          className="scene-visual-image"
          src={textureUrl}
          autoPlay
          loop
          muted
          playsInline
        />
      ) : null}
      {textureUrl && (object.assetKind === "image" || object.assetKind === "system") ? (
        <img className="scene-visual-image" src={textureUrl} alt={object.name} />
      ) : null}
    </div>
  );
}

function SceneTextNode({
  object,
}: {
  object: Extract<EvaluatedSceneObject, { kind: "text" }>;
}) {
  const fontFamily = useSceneFontFamily(object.text.style.fontPath);
  const bounds = object.text.layout.renderBounds ?? object.transform.renderBounds;

  if (!bounds) {
    return null;
  }

  const contentBounds = object.text.layout.contentBounds ?? bounds;
  const color = vectorColorToCss(object.text.style.color, object.text.style.alpha);
  const blurEffect = object.text.style.effectPaths.some((path) => /blur/i.test(path));
  const pointSize = clamp(object.text.layout.scaledPointSize || object.text.style.pointSize || 24, 6, 512);
  const justifyContent = alignmentToJustify(object.text.style.horizontalAlign ?? object.alignment ?? "center");
  const alignItems = alignmentToItems(object.text.style.verticalAlign ?? "center");
  const textShadow = blurEffect
    ? "0 0 0 rgba(0,0,0,0)"
    : "0 10px 40px rgba(0, 0, 0, 0.56), 0 2px 14px rgba(0, 0, 0, 0.32)";
  const blurRadius = Math.max(4, pointSize * 0.22);
  const translateX = "var(--scene-motion-x-6, 0px)";
  const translateY = "var(--scene-motion-y-6, 0px)";
  const contentLeft = Math.max(0, contentBounds[0] - bounds[0]);
  const contentTop = Math.max(0, contentBounds[1] - bounds[1]);
  const contentWidth = Math.max(1, contentBounds[2]);
  const contentHeight = Math.max(1, contentBounds[3]);
  const textAlign =
    justifyContent === "flex-start"
      ? "left"
      : justifyContent === "flex-end"
        ? "right"
        : "center";

  return (
    <div
      className={`scene-text-layer scene-text-${object.behavior}`}
      style={{
        left: `${bounds[0]}px`,
        top: `${bounds[1]}px`,
        width: `${Math.max(1, bounds[2])}px`,
        height: `${Math.max(1, bounds[3])}px`,
        position: "absolute",
        transform: `translate(${translateX}, ${translateY}) rotate(${object.transform.rotation.toFixed(4)}rad) scale(${object.transform.scale[0] < 0 ? -1 : 1}, ${object.transform.scale[1] < 0 ? -1 : 1})`,
        fontSize: `${pointSize}px`,
        fontFamily: fontFamily ?? `"SF Pro Text", "PingFang SC", sans-serif`,
        color,
        opacity: clamp(object.opacity, 0, 1),
        textShadow,
        overflow: "visible",
      }}
    >
      <div
        style={{
          position: "absolute",
          left: `${contentLeft}px`,
          top: `${contentTop}px`,
          width: `${contentWidth}px`,
          minHeight: `${contentHeight}px`,
          display: "flex",
          justifyContent,
          alignItems,
          textAlign,
          overflow: "visible",
        }}
      >
        {blurEffect ? (
          <span
            style={{
              position: "absolute",
              inset: 0,
              color,
              opacity: 0.34,
              filter: `blur(${blurRadius.toFixed(2)}px)`,
              pointerEvents: "none",
              whiteSpace: "pre-line",
            }}
          >
            {object.text.value}
          </span>
        ) : null}
        <span
          style={{
            position: "relative",
            display:
              object.text.style.maxRows && object.text.style.maxRows > 1 ? "-webkit-box" : "block",
            WebkitLineClamp:
              object.text.style.maxRows && object.text.style.maxRows > 1
                ? String(object.text.style.maxRows)
                : undefined,
            WebkitBoxOrient:
              object.text.style.maxRows && object.text.style.maxRows > 1 ? "vertical" : undefined,
            whiteSpace:
              object.text.style.maxRows === 1
                ? "pre"
                : object.text.style.limitWidth
                  ? "pre-wrap"
                  : "pre-line",
            overflow: object.text.style.limitWidth ? "hidden" : "visible",
            textOverflow: object.text.style.limitUseEllipsis ? "ellipsis" : "clip",
            width: "100%",
            maxWidth: "100%",
          }}
        >
          {object.text.value}
        </span>
      </div>
    </div>
  );
}

function SceneAudioNode({
  object,
  snapshot,
}: {
  object: Extract<EvaluatedSceneObject, { kind: "audio" }>;
  snapshot: SharedAudioSnapshot | null;
}) {
  const barRefs = useRef<Array<HTMLSpanElement | null>>([]);
  const bounds = object.transform.renderBounds;
  const audio: EvaluatedAudioState = object.audio;

  if (!bounds) {
    return null;
  }

  const count = clamp(audio.barCount, 8, 72);
  const width = Math.max(1, bounds[2]);
  const height = Math.max(1, bounds[3]);
  const gap = Math.max(1, (width / count) * clamp(audio.barSpacing ?? 0.42, 0.05, 2.5) * 0.22);
  const barWidth = Math.max(2, (width - gap * (count - 1)) / count);
  const radius = Math.max(2, barWidth * clamp(audio.radius ?? 0.6, 0, 2.5));
  const accent = vectorColorToCss(audio.color, 0.96);
  const minimumHeight = clamp(audio.minimumHeight ?? 0.12, 0, 3);
  const [rawLowerBound, rawUpperBound] = audio.barBounds ?? [0, 0.58];
  const upperBound = clamp(rawUpperBound, 0.01, 1);
  const lowerBound = clamp(rawLowerBound, 0, upperBound);
  const volumeFactor = clamp(audio.volumeFactor ?? 1, 0.1, 4);
  const drawableHeight = Math.max(1, height * upperBound);
  const drawableTop = height - drawableHeight;
  const normalizedLowerBound = clamp(lowerBound / upperBound, 0, 1);
  const minScale = clamp(
    Math.max((barWidth * minimumHeight) / drawableHeight, normalizedLowerBound, 0.02),
    0.02,
    1,
  );
  const levels = useMemo(() => deriveSceneAudioLevels(snapshot, count), [snapshot, count]);

  useEffect(() => {
    for (let index = 0; index < count; index += 1) {
      const bar = barRefs.current[index];
      if (!bar) {
        continue;
      }
      const level = clamp((levels[index] ?? 0) * volumeFactor, 0, 1);
      const bounded = normalizedLowerBound + (1 - normalizedLowerBound) * level;
      const scaleY = clamp(Math.max(minScale, bounded), minScale, 1);
      bar.style.transform = `scaleY(${scaleY.toFixed(4)})`;
      bar.style.opacity = clamp(0.35 + scaleY * 0.65, 0.35, 1).toFixed(4);
    }
  }, [count, levels, minScale, normalizedLowerBound, volumeFactor]);

  return (
    <div
      className="scene-audio-layer"
      style={{
        left: `${bounds[0]}px`,
        top: `${bounds[1]}px`,
        width: `${width}px`,
        height: `${height}px`,
        color: accent,
        gap: `${gap}px`,
        opacity: clamp(audio.opacity ?? object.opacity, 0.05, 1),
        transform: `rotate(${object.transform.rotation.toFixed(4)}rad) scale(${object.transform.scale[0] < 0 ? -1 : 1}, ${object.transform.scale[1] < 0 ? -1 : 1})`,
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          top: `${drawableTop}px`,
          height: `${drawableHeight}px`,
          display: "flex",
          alignItems: "end",
          justifyContent: "stretch",
          gap: `${gap}px`,
          overflow: "hidden",
        }}
      >
        {Array.from({ length: count }).map((_, index) => (
          <span
            key={`${object.id}-${index}`}
            ref={(node) => {
              barRefs.current[index] = node;
            }}
            className="scene-audio-bar"
            style={{
              width: `${barWidth}px`,
              height: `${drawableHeight}px`,
              borderRadius: `${radius}px`,
              transform: `scaleY(${minScale.toFixed(4)})`,
              opacity: 0.35,
              animation: "none",
            }}
          />
        ))}
      </div>
    </div>
  );
}

function SceneSoundscape({
  objects,
  paused,
}: {
  objects: EvaluatedSceneObject[];
  paused?: boolean;
}) {
  const soundObjects = useMemo(
    () =>
      objects.filter(
        (object): object is Extract<EvaluatedSceneObject, { kind: "sound" }> =>
          object.kind === "sound" && object.visible,
      ),
    [objects],
  );
  const audioRefs = useRef<Record<number, HTMLAudioElement | null>>({});
  const trackSignature = useMemo(() => stableAudioKey(soundObjects), [soundObjects]);

  useEffect(() => {
    return () => {
      Object.values(audioRefs.current).forEach((element) => {
        element?.pause();
      });
    };
  }, []);

  useEffect(() => {
    const activeIds = new Set(soundObjects.map((track) => track.id));
    for (const [id, element] of Object.entries(audioRefs.current)) {
      if (!activeIds.has(Number(id))) {
        element?.pause();
      }
    }

    soundObjects.forEach((track) => {
      const element = audioRefs.current[track.id];
      if (!element) {
        return;
      }
      element.loop = track.looped;
      element.volume = clamp(track.volume, 0, 1);
      if (paused) {
        element.pause();
        return;
      }
      if (element.paused) {
        void element.play().catch(() => undefined);
      }
    });
  }, [paused, soundObjects, trackSignature]);

  if (soundObjects.length === 0) {
    return null;
  }

  return (
    <div className="scene-soundscape" aria-hidden>
      {soundObjects.map((track) => {
        const src = toAssetUrl(track.assetPath);
        if (!src) {
          return null;
        }
        return (
          <audio
            key={track.id}
            ref={(node) => {
              audioRefs.current[track.id] = node;
            }}
            src={src}
            preload="auto"
            loop={track.looped}
          />
        );
      })}
    </div>
  );
}

function SceneParticleOverlay({
  objects,
  sceneWidth,
  sceneHeight,
  cursorRef,
}: {
  objects: EvaluatedSceneObject[];
  sceneWidth: number;
  sceneHeight: number;
  cursorRef: { current: SceneCursorState };
}) {
  const [cursor, setCursor] = useState<SceneCursorState>(cursorRef.current);
  const [linePoints, setLinePoints] = useState<TrailPoint[]>([]);
  const [petals, setPetals] = useState<PetalParticle[]>([]);
  const nextIdRef = useRef(1);
  const lastCursorRef = useRef<SceneCursorState | null>(null);

  const particleObjects = useMemo(
    () =>
      objects.filter(
        (object): object is Extract<EvaluatedSceneObject, { kind: "particle" }> =>
          object.kind === "particle" && object.visible,
      ),
    [objects],
  );
  const lineObject = particleObjects.find((object) => object.particleKind === "lineTrail") ?? null;
  const petalObject =
    particleObjects.find((object) => object.particleKind === "petalTrail") ?? null;

  useEffect(() => {
    if (!lineObject && !petalObject) {
      setLinePoints([]);
      setPetals([]);
      lastCursorRef.current = null;
    }
  }, [lineObject, petalObject]);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      const next = cursorRef.current;
      setCursor((current) => {
        if (
          current.x === next.x &&
          current.y === next.y &&
          current.active === next.active &&
          current.timestamp === next.timestamp
        ) {
          return current;
        }
        return next;
      });
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frame);
  }, [cursorRef]);

  useEffect(() => {
    const now = cursor.timestamp;
    if (!cursor.active) {
      lastCursorRef.current = null;
      return;
    }

    const previous = lastCursorRef.current;
    const moved = cursorDistance(previous, cursor);
    lastCursorRef.current = cursor;

    if (lineObject && (!previous || moved >= clamp(lineObject.size * 1.2, 8, 28))) {
      setLinePoints((current) =>
        [
          ...current.filter((point) => now - point.createdAt < 520),
          {
            id: nextIdRef.current++,
            x: cursor.x,
            y: cursor.y,
            size: lineObject.size,
            createdAt: now,
            color: vectorColorToCss(lineObject.color, 0.92),
          },
        ].slice(-22),
      );
    }

    if (petalObject && (!previous || moved >= clamp(petalObject.size * 6, 10, 34))) {
      const burst = clamp(Math.round((petalObject.emissionRate || 80) / 45), 1, 4);
      const color = vectorColorToCss(petalObject.color, 0.88);
      setPetals((current) => {
        const next = current.filter((particle) => now - particle.bornAt < particle.lifeMs);
        for (let index = 0; index < burst; index += 1) {
          const spread = (index - (burst - 1) / 2) * petalObject.size * 4.2;
          next.push({
            id: nextIdRef.current++,
            x: cursor.x + spread,
            y: cursor.y + (Math.random() - 0.5) * 8,
            vx: (Math.random() - 0.5) * 0.9,
            vy: -0.4 - Math.random() * 0.7,
            rotation: Math.random() * 360,
            spin: (Math.random() - 0.5) * 1.8,
            size: 9 + petalObject.size * (10 + Math.random() * 8),
            bornAt: now,
            lifeMs: 1100 + Math.random() * 600,
            color,
          });
        }
        return next.slice(-84);
      });
    }
  }, [cursor, lineObject, petalObject]);

  useEffect(() => {
    if ((!lineObject && !petalObject) || (linePoints.length === 0 && petals.length === 0)) {
      return;
    }

    let frame = 0;
    const tick = () => {
      const now = performance.now();
      setLinePoints((current) => current.filter((point) => now - point.createdAt < 520));
      setPetals((current) =>
        current
          .map((particle) => ({
            ...particle,
            x: particle.x + particle.vx,
            y: particle.y + particle.vy,
            rotation: particle.rotation + particle.spin,
            vy: particle.vy - 0.015,
          }))
          .filter((particle) => now - particle.bornAt < particle.lifeMs),
      );
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frame);
  }, [lineObject, linePoints.length, petalObject, petals.length]);

  const linePath = pointsToSvgPath(linePoints, sceneHeight);
  const renderNow = performance.now();

  return (
    <div className="scene-particle-overlay">
      {lineObject && linePath ? (
        <svg
          className="scene-particle-svg"
          viewBox={`0 0 ${sceneWidth} ${sceneHeight}`}
          preserveAspectRatio="none"
        >
          <path
            className="scene-particle-path glow"
            d={linePath}
            style={{
              stroke: linePoints[0]?.color ?? vectorColorToCss(lineObject.color, 0.92),
              strokeWidth: `${lineObject.size * 3.4}px`,
            }}
          />
          <path
            className="scene-particle-path"
            d={linePath}
            style={{
              stroke: linePoints[0]?.color ?? vectorColorToCss(lineObject.color, 0.92),
              strokeWidth: `${lineObject.size * 1.6}px`,
            }}
          />
        </svg>
      ) : null}

      {petalObject
        ? petals.map((particle) => {
            const age = clamp((renderNow - particle.bornAt) / particle.lifeMs, 0, 1);
            return (
              <span
                key={particle.id}
                className="scene-petal-particle"
                style={{
                  left: `${particle.x}px`,
                  top: `${sceneHeight - particle.y}px`,
                  width: `${particle.size}px`,
                  height: `${particle.size * 0.68}px`,
                  opacity: 1 - age,
                  background: `radial-gradient(circle at 30% 30%, rgba(255,255,255,0.92), ${particle.color})`,
                  transform: `translate(-50%, -50%) rotate(${particle.rotation.toFixed(2)}deg)`,
                }}
              />
            );
          })
        : null}
    </div>
  );
}

function SceneStageSurface({
  wallpaper,
  scene,
  paused,
}: {
  wallpaper: WallpaperRuntimeRecord;
  scene: SceneRuntimeDocument;
  paused?: boolean;
}) {
  const stageRef = useRef<HTMLDivElement | null>(null);
  const cameraRef = useRef<HTMLDivElement | null>(null);
  const stageSize = useElementSize(stageRef);
  const windowViewportSize = useViewportSize();
  const playerMotionTargetRef = useRef({ x: 0, y: 0 });
  const playerMotionRef = useRef({ x: 0, y: 0 });
  const [audioSnapshot, setAudioSnapshot] = useState<SharedAudioSnapshot | null>(null);
  const cursorRef = useRef<SceneCursorState>({
    x: scene.evaluated.canvasWidth / 2,
    y: scene.evaluated.canvasHeight / 2,
      active: false,
      timestamp: performance.now(),
  });

  const sceneWidth = scene.evaluated.canvasWidth || 3840;
  const sceneHeight = scene.evaluated.canvasHeight || 2160;
  const renderObjects = useMemo(() => evaluatedObjects(scene), [scene]);
  const soundObjects = useMemo(
    () => renderObjects.filter((object) => object.kind === "sound"),
    [renderObjects],
  );
  const particleObjects = useMemo(
    () => renderObjects.filter((object) => object.kind === "particle"),
    [renderObjects],
  );
  const hasAudioObjects = useMemo(
    () => renderObjects.some((object) => object.kind === "audio" && object.visible),
    [renderObjects],
  );
  const displayObjects = useMemo(
    () => renderObjects.filter((object) => object.kind !== "sound" && object.kind !== "particle" && object.kind !== "container"),
    [renderObjects],
  );

  const effectiveStageWidth =
    stageSize.width > 0 ? stageSize.width : windowViewportSize.width;
  const effectiveStageHeight =
    stageSize.height > 0 ? stageSize.height : windowViewportSize.height;
  const sceneScale =
    effectiveStageWidth > 0 && effectiveStageHeight > 0
      ? Math.min(effectiveStageWidth / sceneWidth, effectiveStageHeight / sceneHeight)
      : 1;
  const camera = scene.evaluated.camera;
  const parallaxEnabled = scene.evaluated.parallax.enabled;

  useEffect(() => {
    playerMotionTargetRef.current = { x: 0, y: 0 };
    playerMotionRef.current = { x: 0, y: 0 };
    cursorRef.current = {
      x: sceneWidth / 2,
      y: sceneHeight / 2,
      active: false,
      timestamp: performance.now(),
    };
    applySceneMotionVariables(stageRef.current, playerMotionRef.current);
  }, [sceneHeight, sceneWidth, wallpaper.id]);

  useEffect(() => {
    let frame = 0;
    let previousTime = performance.now();
    const tick = () => {
      const now = performance.now();
      const deltaMs = Math.max(1, now - previousTime);
      const smoothing = 1 - Math.exp(-deltaMs / 42);
      const current = playerMotionRef.current;
      const target = playerMotionTargetRef.current;
      playerMotionRef.current = {
        x: current.x + (target.x - current.x) * smoothing,
        y: current.y + (target.y - current.y) * smoothing,
      };
      applySceneMotionVariables(stageRef.current, playerMotionRef.current);
      const shakePhase = now / 1000;
      const shakeX =
        camera.cameraShake
          ? Math.sin(shakePhase * (camera.cameraShakeSpeed + 0.35)) *
            (camera.cameraShakeAmplitude * 3.2)
          : 0;
      const shakeY =
        camera.cameraShake
          ? Math.cos(shakePhase * (camera.cameraShakeSpeed + 0.18)) *
            (camera.cameraShakeAmplitude * 2.4)
          : 0;
      const motion = playerMotionRef.current;
      const cameraOffsetX =
        motion.x * camera.parallaxMouseInfluence * (sceneWidth * 0.012) + shakeX;
      const cameraOffsetY =
        motion.y * camera.parallaxMouseInfluence * (sceneHeight * 0.012) + shakeY;
      if (cameraRef.current) {
        cameraRef.current.style.transform = `translate(-50%, -50%) translate(${cameraOffsetX.toFixed(
          2,
        )}px, ${cameraOffsetY.toFixed(2)}px) scale(${(sceneScale * camera.zoom).toFixed(5)})`;
      }
      previousTime = now;
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frame);
  }, [
    camera.cameraShake,
    camera.cameraShakeAmplitude,
    camera.cameraShakeSpeed,
    camera.parallaxMouseInfluence,
    camera.zoom,
    sceneHeight,
    sceneScale,
    sceneWidth,
  ]);

  useEffect(() => {
    if (!hasAudioObjects) {
      setAudioSnapshot(null);
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void getPlayerAudioSnapshot()
      .then((snapshot) => {
        if (!cancelled) {
          setAudioSnapshot(snapshot);
        }
      })
      .catch(() => undefined);

    void onPlayerAudio((snapshot) => {
      if (!cancelled) {
        setAudioSnapshot(snapshot);
      }
    }).then((callback) => {
      if (cancelled) {
        callback();
        return;
      }
      unlisten = callback;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [hasAudioObjects]);

  useEffect(() => {
    if (!hasAudioObjects) {
      return;
    }

    void setSceneAudioInterest(!paused).catch(() => undefined);

    return () => {
      void setSceneAudioInterest(false).catch(() => undefined);
    };
  }, [hasAudioObjects, paused, wallpaper.id]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    const applySharedInputSnapshot = (snapshot: SharedInputSnapshot) => {
      if (cancelled) {
        return;
      }
      const projected = sceneCursorFromSharedInputSnapshot(snapshot, sceneWidth, sceneHeight);
      cursorRef.current = {
        ...projected.cursor,
        timestamp: performance.now(),
      };
      if (parallaxEnabled && projected.cursor.active) {
        playerMotionTargetRef.current = projected.motion;
      } else {
        playerMotionTargetRef.current = { x: 0, y: 0 };
      }
    };

    void getPlayerInputSnapshot()
      .then(applySharedInputSnapshot)
      .catch(() => undefined);

    void onPlayerInput(applySharedInputSnapshot).then((callback) => {
      if (cancelled) {
        callback();
        return;
      }
      unlisten = callback;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [parallaxEnabled, sceneHeight, sceneWidth]);

  const clearColor = vectorColorToCss(scene.evaluated.clearColor, 0.14);

  return (
    <div
      ref={stageRef}
      className="scene-stage scene-stage-player"
      style={{
        background: `radial-gradient(circle at 20% 18%, rgba(255,255,255,0.05), transparent 34%), linear-gradient(180deg, rgba(2, 2, 3, 0.88) 0%, rgba(0, 0, 0, 1) 100%), ${clearColor}`,
      }}
    >
      <SceneSoundscape objects={soundObjects} paused={paused} />
      <div
        ref={cameraRef}
        className="scene-camera"
        style={{
          width: `${sceneWidth}px`,
          height: `${sceneHeight}px`,
          transform: `translate(-50%, -50%) scale(${(sceneScale * camera.zoom).toFixed(5)})`,
        }}
      >
        <div className="scene-field">
          {displayObjects.map((object) => {
            if (object.kind === "visual") {
              return (
                <SceneVisualNode
                  key={object.id}
                  object={object}
                  paused={paused}
                />
              );
            }
            if (object.kind === "text") {
              return <SceneTextNode key={object.id} object={object} />;
            }
            if (object.kind === "audio") {
              return <SceneAudioNode key={object.id} object={object} snapshot={audioSnapshot} />;
            }
            return null;
          })}
          {particleObjects.length > 0 ? (
            <SceneParticleOverlay
              objects={particleObjects}
              sceneWidth={sceneWidth}
              sceneHeight={sceneHeight}
              cursorRef={cursorRef}
            />
          ) : null}
        </div>
      </div>
    </div>
  );
}

function MediaStageSurface({
  wallpaper,
  paused,
}: {
  wallpaper?: WallpaperRuntimeRecord | null;
  paused?: boolean;
}) {
  const videoRuntime = videoRuntimeFor(wallpaper);
  const previewUrl = toAssetUrl(wallpaper?.previewPath ?? videoRuntime?.previewPath ?? null);
  const entryUrl = toAssetUrl(wallpaper?.entryPath ?? videoRuntime?.entryPath ?? null);

  if (!wallpaper) {
    return (
      <div className="stage-empty stage-empty-player">
        <span>导入一个 Wallpaper Engine 目录后，这里会成为你的桌面舞台。</span>
      </div>
    );
  }

  if (wallpaper.runtime.kind === "scene") {
    return <NativeSceneStageSurface title={wallpaper.title} />;
  }

  if (wallpaper.runtime.kind === "video") {
    return (
      <NativeVideoStageSurface
        previewUrl={previewUrl}
        title={wallpaper.title}
        hasEntry={Boolean(entryUrl)}
      />
    );
  }

  if (previewUrl) {
    return (
      <img className="stage-media stage-media-player" src={previewUrl} alt={wallpaper.title} />
    );
  }

  return (
    <div className="stage-empty stage-empty-player">
      <span>这个壁纸已经导入，但当前没有可直接显示的入口素材。</span>
    </div>
  );
}

function StageSurface({
  wallpaper,
  paused,
}: {
  wallpaper?: WallpaperRuntimeRecord | null;
  paused?: boolean;
}) {
  if (wallpaper?.runtime.kind === "web") {
    const webRuntime = webRuntimeFor(wallpaper);
    const hasEntry = Boolean(webRuntime?.entryPath ?? wallpaper.entryPath);

    return (
      <NativeWebStageSurface
        title={wallpaper.title}
        hasEntry={hasEntry}
      />
    );
  }

  return (
    <MediaStageSurface
      wallpaper={wallpaper}
      paused={paused}
    />
  );
}

export function PlayerAppShell() {
  const state: PlayerRuntimeState = usePlayerController();

  return (
    <main className="player-shell">
      <StageSurface wallpaper={state.active} paused={state.paused} />
    </main>
  );
}
