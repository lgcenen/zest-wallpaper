# 已知限制

## 平台边界

- 本项目是 `macOS only`，不提供 Windows 或 Linux 运行时。
- 当前发布基线只覆盖 `Scene`、原生 `AVPlayer` 视频路径和原生 `WKWebView` 网页路径。
- 静态快照与系统壁纸同步现在属于产品链的一部分；但它应始终跟随当前活动动态壁纸，不应演变成独立静态壁纸模式。

## 生命周期与窗口

- 主窗口关闭行为是“隐藏 Workbench”，不是退出应用；player 会继续后台运行。
- `Reopen` 只恢复 Workbench，可见 player 不会因此被重建或暂停。
- 多屏 player 依赖当前显示器拓扑；显示器插拔或重排后会由生命周期 worker 重建窗口宿主。
- 如果持久化的当前壁纸在启动恢复时已经损坏或缺少 `Video/Web` 入口文件，应用会清掉该 active wallpaper、记录诊断并继续启动；不会在后续重启里反复卡死在同一个坏 `active_id`。

## 静态快照同步

- 静态快照同步只服务于当前活动动态壁纸；它不是独立静态壁纸模式，也不会提供任意本地图片选择器。
- `Video / Web / Scene` 的静态快照生成成功后会写入 managed 目录下的 `snapshot.png`，并登记到 `WallpaperRecord.last_snapshot_path`。
- 系统壁纸同步只读取 `last_snapshot_path`。即使 `preview_path` 指向可用图片或 GIF，也不会被当成系统壁纸快照 fallback。
- 当 `last_snapshot_path` 缺失、为空、不是绝对路径、文件不存在或格式不支持时，动态壁纸仍可继续运行；系统壁纸保持当前值，并记录 `static-snapshot-sync/snapshot-unavailable`。
- 当 snapshot 文件存在但 macOS 拒绝应用为系统壁纸时，动态壁纸状态保持当前 active record；系统壁纸保持当前值，并记录 `static-snapshot-sync/apply-failed`。
- 菜单栏颜色刷新依赖 macOS 对系统壁纸和桌面层的刷新时机；如果系统壁纸已应用但 player window 顶部 tint 刷新失败，只记录 `static-snapshot-sync/menu-bar-refresh-degraded`，不阻断壁纸切换。
- 启动阶段不会因为旧库中大量缺失 snapshot 而批量生成所有快照；快照生成发生在导入、元数据刷新和当前 active apply/restore 链路中。

## Scene

- `Scene` player 主路径已经切到原生 `Metal` 宿主，不再以旧前端 DOM/CSS `SceneStageSurface` 作为产品主渲染链。
- 旧前端 `SceneVisualNode`、`SceneTextNode`、`SceneAudioNode`、`SceneSoundscape`、`SceneParticleOverlay` 等组件仍然残留在仓库中，作为 `phase-11` cutover 前的待删除旧链；它们不是正式产品 fallback。
- 当前路线允许用户在设置中挂载从 Windows 获取的外部 `assets` 目录提升复杂 `Scene` 兼容性；未挂载时应用仍可启动、导入、浏览和运行基础支持内容，但复杂资源依赖会以明确诊断收口。
- `Scene` 的鼠标和音频输入已经切到共享服务；如果共享服务不可用，`Scene` 会退化为无输入或静音输入，而不是直接中断整个 player。
- `Scene` 的基础 2D 布局、文本、音频响应、声音轨、粒子和视频纹理已经进入原生路径，但更高层的粒子系统层级、字体绑定完整性、Now Playing 输入层和更细的诊断仍未完全对齐参考实现。
- `MDL/puppet`、shader/material/effect 文件解析和 render graph 虽已进入仓库实现，但产品完成度仍未收口；复杂 `Scene` 仍可能出现部分渲染错误、warning-only best-effort 进入，或在明确诊断下失败。

## Video

- `Video` player 主路径完全依赖 macOS `AVQueuePlayer + AVPlayerLooper + AVPlayerView`。
- 当前原生视频宿主统一静音播放，不把壁纸音频输出到系统。
- 视频源文件缺失时不会自动回退到前端 `<video>`；当前主 runtime 会直接失败并回传错误，同时记录诊断。

## Web

- `Web` player 主路径完全依赖 macOS `WKWebView`，继续使用真实 runtime URL，不回退到 `blob:`、`srcdoc:` 或前端 `iframe`。
- 页面内只保留 Wallpaper Engine 风格最小 shim；如果网页自身的 `applyUserProperties`、脚本执行或渲染线程阻塞，原生宿主不会替它做额外前端补偿。
- `wallpaper:properties`、`wallpaper:paused`、`wallpaper:cursor`、`wallpaper:audio` 由原生侧统一调度；如果 bridge readiness 没有完成，状态消息会重试而不是改走 React 宿主。
- Web 入口 HTML 缺失时不会静默跳过；当前主 runtime 会直接失败并回传错误，同时记录诊断。

## 权限与系统集成

- 共享音频服务依赖 `ScreenCaptureKit`。如果 macOS 没有授予 `Screen Recording` 权限，`Scene/Web` 的音频输入会保持静默，并记录 `shared-audio/capture-unavailable` 诊断。
- 鼠标输入来自共享输入服务；如果系统事件读取失败，当前会退化为“本帧没有输入”，不会阻断壁纸切换。

## 诊断

- Phase-05 新增了统一运行时诊断面，当前暴露的是内存态最近诊断列表和 `player:diagnostic` 事件。
- 诊断覆盖运行时宿主、共享输入/音频链和静态快照同步链，不替代崩溃报告、性能分析或网页内部脚本调试。
- 静态快照同步诊断的用户可见语义固定为“动态壁纸继续，系统壁纸保持原值或 best-effort 刷新”；诊断不会把 `preview_path` 升级为系统壁纸输入。

## 测试与 smoke

- 仓库自动回归必须保持可移植，不能依赖本机 `/Users/...`、`/Volumes/...`、私有 `Application Support` 或外接磁盘样例路径。
- 如果开发机上存在额外本机壁纸样例，它们只能作为手工 smoke 辅助，不是 phase 验收或 release gate 的正式组成部分。
