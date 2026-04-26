export type WallpaperType = "scene" | "video" | "web" | "application" | "unknown";

export type PropertyKind =
  | "bool"
  | "slider"
  | "color"
  | "combo"
  | "textinput"
  | "text"
  | "group"
  | "unknown";

export type PropertyPresentation = "control" | "group" | "decoration";
export type PropertySectionItemKind = "property" | "description" | "separator";

export interface WallpaperOption {
  label: string;
  value: string;
}

export interface WallpaperProperty {
  key: string;
  label: string;
  markup?: string | null;
  kind: PropertyKind;
  value: unknown;
  defaultValue: unknown;
  min?: number | null;
  max?: number | null;
  step?: number | null;
  condition?: string | null;
  order?: number | null;
  presentation: PropertyPresentation;
  options: WallpaperOption[];
}

export interface PropertySectionItem {
  kind: PropertySectionItemKind;
  key?: string | null;
  text?: string | null;
  markup?: string | null;
  order?: number | null;
  condition?: string | null;
}

export interface PropertySection {
  key: string;
  label: string;
  order?: number | null;
  condition?: string | null;
  items: PropertySectionItem[];
}

export interface SceneBinding {
  propertyKey: string;
  condition?: string | null;
}

export interface SceneNodeState {
  id: number;
  name: string;
  dependencies: number[];
  parentId?: number | null;
  visible: boolean;
  visibilityBinding?: SceneBinding | null;
  position: [number, number, number];
  positionBindings?: SceneAxisBindings | null;
  scale: [number, number, number];
  angles?: [number, number, number] | null;
  rotation?: number | null;
}

export interface SceneAnimationLayer {
  id: number;
  rate: number;
  visible: boolean;
  visibilityBinding?: SceneBinding | null;
  blend: string;
  animation: string;
}

export type SceneTextBehavior =
  | "static"
  | "script"
  | "clock"
  | "date"
  | "weekday"
  | "dayPeriod"
  | "fps"
  | "mediaTitle";
export type SceneAssetKind = "image" | "video" | "system" | "unsupported";
export type SceneParticleKind = "lineTrail" | "petalTrail";
export type SceneLogicNodeKind = "container" | "visual" | "text" | "audio" | "particle" | "sound";
export type SceneRenderNodeKind =
  | "container"
  | "sprite"
  | "video"
  | "text"
  | "audioReactive"
  | "particle"
  | "sound"
  | "systemTexture"
  | "unsupported";

export interface SceneParallax {
  enabled: boolean;
  amount?: number | null;
  delay?: number | null;
}

export interface SceneAxisBindings {
  x?: string | null;
  y?: string | null;
}

export interface SceneCamera {
  zoom: number;
  cameraShake: boolean;
  cameraShakeAmplitude: number;
  cameraShakeSpeed: number;
  parallaxMouseInfluence: number;
  zoomBinding?: string | null;
  cameraShakeBinding?: string | null;
  parallaxMouseInfluenceBinding?: string | null;
}

export interface SceneVisualLayer {
  id: number;
  dependencies: number[];
  parentId?: number | null;
  name: string;
  alignment?: string | null;
  visible: boolean;
  visibilityBinding?: SceneBinding | null;
  position: [number, number, number];
  positionBindings?: SceneAxisBindings | null;
  scale: [number, number, number];
  scaleBinding?: string | null;
  angles?: [number, number, number] | null;
  size?: [number, number] | null;
  intrinsicSize?: [number, number] | null;
  parallaxDepth?: [number, number] | null;
  parallaxDepthBinding?: string | null;
  rotation?: number | null;
  opacity?: number | null;
  opacityBinding?: string | null;
  color?: string | null;
  colorBinding?: string | null;
  brightness?: number | null;
  brightnessBinding?: string | null;
  colorBlendMode?: number | null;
  renderBounds?: [number, number, number, number] | null;
  fullscreen: boolean;
  autosize: boolean;
  solidLayer: boolean;
  passthrough: boolean;
  noPadding: boolean;
  modelWidth?: number | null;
  modelHeight?: number | null;
  puppetPath?: string | null;
  animationLayers: SceneAnimationLayer[];
  modelPath?: string | null;
  materialPath?: string | null;
  shaderPath?: string | null;
  textureNames: string[];
  assetKind: SceneAssetKind;
  assetPath?: string | null;
  systemTextureKey?: string | null;
  blendMode?: string | null;
}

export interface SceneTextLayer {
  id: number;
  name: string;
  dependencies: number[];
  parentId?: number | null;
  alignment?: string | null;
  horizontalAlign?: string | null;
  verticalAlign?: string | null;
  content: string;
  behavior: SceneTextBehavior;
  delimiter?: string | null;
  monthFormat?: string | null;
  dayFormat?: string | null;
  showDay?: boolean | null;
  alignVertical?: boolean | null;
  useDelimiter?: boolean | null;
  showSeconds?: boolean | null;
  use24hFormat?: boolean | null;
  visible: boolean;
  visibilityBinding?: SceneBinding | null;
  textBinding?: string | null;
  position: [number, number, number];
  positionBindings?: SceneAxisBindings | null;
  scale: [number, number, number];
  size?: [number, number] | null;
  renderBounds?: [number, number, number, number] | null;
  parallaxDepth?: [number, number] | null;
  color?: string | null;
  colorBinding?: string | null;
  alpha?: number | null;
  alphaBinding?: string | null;
  pointSize?: number | null;
  pointSizeBinding?: string | null;
  fontPath?: string | null;
  effectPaths: string[];
  scriptText?: string | null;
  padding?: number | null;
  maxRows?: number | null;
  maxWidth?: number | null;
  limitWidth?: boolean | null;
  limitUseEllipsis?: boolean | null;
  blockAlign?: boolean | null;
}

export interface SceneAudioLayer {
  id: number;
  name: string;
  dependencies: number[];
  parentId?: number | null;
  alignment?: string | null;
  visible: boolean;
  visibilityBinding?: SceneBinding | null;
  position: [number, number, number];
  positionBindings?: SceneAxisBindings | null;
  scale: [number, number, number];
  size?: [number, number] | null;
  renderBounds?: [number, number, number, number] | null;
  angle?: number | null;
  barCount: number;
  color?: string | null;
  barSpacing?: number | null;
  barBounds?: [number, number] | null;
  minimumHeight?: number | null;
  radius?: number | null;
  volumeFactor?: number | null;
  opacity?: number | null;
}

export interface SceneSoundTrack {
  id: number;
  name: string;
  assetPath: string;
  looped: boolean;
  volume: number;
  volumeBinding?: string | null;
}

export interface SceneParticleLayer {
  id: number;
  name: string;
  dependencies: number[];
  parentId?: number | null;
  visible: boolean;
  visibilityBinding?: SceneBinding | null;
  kind: SceneParticleKind;
  particlePath: string;
  color?: string | null;
  colorBinding?: string | null;
  size: number;
  sizeBinding?: string | null;
  emissionRate: number;
}

export interface SceneLogicNode {
  id: number;
  name: string;
  parentId?: number | null;
  kind: SceneLogicNodeKind;
  visible: boolean;
  condition?: string | null;
  bindings: string[];
}

export interface SceneRenderNode {
  id: number;
  name: string;
  parentId?: number | null;
  kind: SceneRenderNodeKind;
  visible: boolean;
  assetPath?: string | null;
  materialPath?: string | null;
}

export interface SceneMaterialPass {
  ownerId: number;
  materialPath?: string | null;
  shaderPath?: string | null;
  blendMode?: string | null;
  textureNames: string[];
  systemTextureKey?: string | null;
}

export interface SceneAudioSource {
  id: number;
  name: string;
  sourceType: string;
  assetPath?: string | null;
  reactive: boolean;
}

export interface SceneManifest {
  canvasWidth?: number | null;
  canvasHeight?: number | null;
  clearColor?: string | null;
  camera: SceneCamera;
  parallax: SceneParallax;
  nodes: SceneNodeState[];
  primaryVisual?: SceneVisualLayer | null;
  visualLayers: SceneVisualLayer[];
  textLayers: SceneTextLayer[];
  audioLayers: SceneAudioLayer[];
  soundTracks: SceneSoundTrack[];
  particleLayers: SceneParticleLayer[];
  logicGraph: SceneLogicNode[];
  renderGraph: SceneRenderNode[];
  materialPasses: SceneMaterialPass[];
  audioSources: SceneAudioSource[];
  objectCount: number;
}

export interface EvaluatedSceneTransform {
  position: [number, number, number];
  scale: [number, number, number];
  rotation: number;
  renderBounds?: [number, number, number, number] | null;
}

export interface EvaluatedSceneCamera {
  zoom: number;
  cameraShake: boolean;
  cameraShakeAmplitude: number;
  cameraShakeSpeed: number;
  parallaxMouseInfluence: number;
}

export interface EvaluatedTextStyle {
  color?: string | null;
  alpha: number;
  pointSize: number;
  fontPath?: string | null;
  effectPaths: string[];
  horizontalAlign?: string | null;
  verticalAlign?: string | null;
  padding?: number | null;
  maxRows?: number | null;
  maxWidth?: number | null;
  limitWidth?: boolean | null;
  limitUseEllipsis?: boolean | null;
  blockAlign?: boolean | null;
}

export interface EvaluatedTextLayout {
  size?: [number, number] | null;
  renderBounds?: [number, number, number, number] | null;
  contentBounds?: [number, number, number, number] | null;
  scaledPointSize: number;
  scaledPadding: number;
  worldScale: [number, number, number];
}

export interface EvaluatedTextState {
  value: string;
  style: EvaluatedTextStyle;
  layout: EvaluatedTextLayout;
}

export interface EvaluatedAudioState {
  barCount: number;
  color?: string | null;
  barSpacing?: number | null;
  barBounds?: [number, number] | null;
  minimumHeight?: number | null;
  radius?: number | null;
  volumeFactor?: number | null;
  opacity?: number | null;
}

export interface EvaluatedSceneObjectBase {
  id: number;
  name: string;
  dependencies: number[];
  parentId?: number | null;
  visible: boolean;
  alignment?: string | null;
  opacity: number;
  transform: EvaluatedSceneTransform;
}

export type EvaluatedSceneObject =
  | ({
      kind: "container";
    } & EvaluatedSceneObjectBase)
  | ({
      kind: "visual";
      assetKind: SceneAssetKind;
      assetPath?: string | null;
      systemTextureKey?: string | null;
      textureNames: string[];
      blendMode?: string | null;
      color?: string | null;
      brightness?: number | null;
      colorBlendMode?: number | null;
      parallaxDepth?: [number, number] | null;
      angles?: [number, number, number] | null;
      fullscreen?: boolean;
      autosize?: boolean;
      solidLayer?: boolean;
      passthrough?: boolean;
      noPadding?: boolean;
      puppetPath?: string | null;
      animationLayers: SceneAnimationLayer[];
      primary?: boolean;
      backgroundCandidate?: boolean;
    } & EvaluatedSceneObjectBase)
  | ({
      kind: "text";
      behavior: SceneTextBehavior;
      text: EvaluatedTextState;
    } & EvaluatedSceneObjectBase)
  | ({
      kind: "audio";
      audio: EvaluatedAudioState;
    } & EvaluatedSceneObjectBase)
  | ({
      kind: "particle";
      particlePath: string;
      particleKind: SceneParticleKind;
      color?: string | null;
      size: number;
      emissionRate: number;
    } & EvaluatedSceneObjectBase)
  | ({
      kind: "sound";
      assetPath: string;
      looped: boolean;
      volume: number;
    } & EvaluatedSceneObjectBase);

export interface SceneEvaluatedDocument {
  canvasWidth: number;
  canvasHeight: number;
  clearColor?: string | null;
  camera: EvaluatedSceneCamera;
  parallax: SceneParallax;
  objects: Record<string, EvaluatedSceneObject>;
  renderList: number[];
  evaluatedAt: string;
}

export interface SceneRuntimeDocument {
  runtimeOwnerKey?: string | null;
  source: SceneManifest;
  evaluated: SceneEvaluatedDocument;
}

export interface VideoRuntimeDocument {
  entryPath?: string | null;
  previewPath?: string | null;
  sourcePath: string;
  managedPath: string;
}

export interface WebRuntimeDocument {
  entryPath?: string | null;
  previewPath?: string | null;
  sourcePath: string;
  managedPath: string;
}

export type WallpaperRuntime =
  | { kind: "scene"; scene: SceneRuntimeDocument }
  | { kind: "video"; video: VideoRuntimeDocument }
  | { kind: "web"; web: WebRuntimeDocument }
  | { kind: "application" }
  | { kind: "unknown" };

export interface WallpaperRuntimeRecord {
  id: string;
  title: string;
  wallpaperType: WallpaperType;
  sourcePath: string;
  managedPath: string;
  previewPath?: string | null;
  entryPath?: string | null;
  propertySchema: WallpaperProperty[];
  propertySections: PropertySection[];
  importedAt: string;
  tags: string[];
  runtime: WallpaperRuntime;
}

export interface SceneRuntimeSettingsSnapshot {
  externalAssetsPath?: string | null;
  externalAssetsExists: boolean;
}

export interface PlayerRuntimeState {
  active?: WallpaperRuntimeRecord | null;
  paused: boolean;
}

export interface SharedInputModifiers {
  shift: boolean;
  control: boolean;
  option: boolean;
  command: boolean;
}

export interface SharedInputSnapshot {
  globalX: number;
  globalY: number;
  systemX: number;
  systemY: number;
  desktopWidth: number;
  desktopHeight: number;
  timestampMs: number;
  active: boolean;
  modifiers: SharedInputModifiers;
  scrollDeltaX: number;
  scrollDeltaY: number;
}

export interface SharedAudioSnapshot {
  timestampMs: number;
  bands: number[];
  smoothedBands: number[];
  peak: number;
  rms: number;
  muted: boolean;
  active: boolean;
}

export type RuntimeDiagnosticSeverity = "warning" | "error";

export interface RuntimeDiagnostic {
  timestampMs: number;
  subsystem: string;
  code: string;
  severity: RuntimeDiagnosticSeverity;
  summary: string;
  detail?: string | null;
}
