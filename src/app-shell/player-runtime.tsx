import { toAssetUrl } from "../tauri";
import type {
  PlayerRuntimeState,
  VideoRuntimeDocument,
  WallpaperRuntimeRecord,
  WebRuntimeDocument,
} from "../types";
import { usePlayerController } from "../state/player-controller";

function videoRuntimeFor(wallpaper?: WallpaperRuntimeRecord | null): VideoRuntimeDocument | null {
  return wallpaper?.runtime.kind === "video" ? wallpaper.runtime.video : null;
}

function webRuntimeFor(wallpaper?: WallpaperRuntimeRecord | null): WebRuntimeDocument | null {
  return wallpaper?.runtime.kind === "web" ? wallpaper.runtime.web : null;
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

function MediaStageSurface({
  wallpaper,
}: {
  wallpaper?: WallpaperRuntimeRecord | null;
}) {
  if (!wallpaper) {
    return (
      <div className="stage-empty stage-empty-player">
        <span>导入壁纸后，这里会显示你的壁纸。</span>
      </div>
    );
  }

  if (wallpaper.runtime.kind === "scene") {
    return <NativeSceneStageSurface title={wallpaper.title} />;
  }

  const videoRuntime = videoRuntimeFor(wallpaper);
  const previewUrl = toAssetUrl(wallpaper.previewPath ?? videoRuntime?.previewPath ?? null);
  const entryUrl = toAssetUrl(wallpaper.entryPath ?? videoRuntime?.entryPath ?? null);

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
}: {
  wallpaper?: WallpaperRuntimeRecord | null;
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

  return <MediaStageSurface wallpaper={wallpaper} />;
}

export function PlayerAppShell() {
  const state: PlayerRuntimeState = usePlayerController();

  return (
    <main className="player-shell">
      <StageSurface wallpaper={state.active} />
    </main>
  );
}
