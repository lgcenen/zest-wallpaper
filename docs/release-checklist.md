# 发布检查清单

## 发布门槛

以下项目全部满足后，才能把当前基线视为可发布：

1. `cargo test -j 1 --manifest-path src-tauri/Cargo.toml`
2. `cargo check -j 1 --manifest-path src-tauri/Cargo.toml`
3. `npm run build`
4. `docs/known-limitations.md` 已更新到当前实现
5. `Scene / Video / Web` 可移植自动回归至少各通过一组
6. player lifecycle 回归至少通过一组

## 自动回归

发布前至少验证以下可移植路径各一组：

### Scene

- 导入 synthetic 或 repo fixture 的 `Scene` 样例后，manifest 能正常生成。
- 导入或刷新后能生成 `snapshot.png`，并登记到 `WallpaperRecord.last_snapshot_path`。
- 属性、文本层、音频层、系统纹理层仍能出现在运行时文档中。
- Scene snapshot 只作为当前活动壁纸的系统壁纸同步输入；不复用旧前端 `SceneStageSurface`，不 fallback 到 `preview_path`。
- 不支持的 Scene 深层能力必须返回明确失败或诊断，不允许 silent success。

### Video

- 导入 synthetic 或 repo fixture 的 `Video` 样例后，entry 文件能被识别。
- 导入或刷新后能生成 `snapshot.png`，并登记到 `WallpaperRecord.last_snapshot_path`。
- 应用该壁纸后，原生 `AVPlayer` 宿主可以启动、暂停、恢复、切换和销毁。
- 主窗口隐藏、`Reopen Workbench`、多屏窗口重建不会让视频壁纸异常退出。

### Web

- 导入 synthetic 或 repo fixture 的 `Web` 样例后，entry HTML 能被识别，真实 runtime URL 可以访问。
- 导入或刷新后能生成 `snapshot.png`，并登记到 `WallpaperRecord.last_snapshot_path`。
- 页面内 bridge 注入、property bootstrap、paused 更新、cursor 更新和 audio listener readiness 正常工作。
- 切换 Web 壁纸、修改属性、手动暂停 / 自动暂停 / 恢复不会把 GUI 或桌面 player 一起卡死。

### 静态快照同步

- `snapshot_for_record` 只接受 `last_snapshot_path`，缺失、相对路径、文件不存在或格式不支持时不得 fallback 到 `preview_path`。
- 新 apply 成功后只同步当前 active record 的 snapshot；stale reconcile / stale scene update 不得覆盖新 active snapshot 状态。
- 启动恢复时，恢复的 active wallpaper 与 `static_snapshot_sync.active_record_id`、`last_applied_snapshot_path` 保持一致。
- 系统壁纸应用失败时，保留当前 macOS 系统壁纸，保留 active player state，并记录 `static-snapshot-sync/apply-failed`。
- 快照缺失或损坏时，保留当前 macOS 系统壁纸，并记录 `static-snapshot-sync/snapshot-unavailable`。

## 手工 smoke

- 如果当前开发机上恰好有额外本机样例，可以补做 `Scene / Video / Web` 手工 smoke。
- 本机个人目录、外接磁盘路径、私有 managed library 只能作为辅助手工验证，不能作为仓库自动测试、phase 验收或发布 gate。
- 切换 `Video / Web / Scene` 当前活动壁纸后，确认动态 player 启动，macOS 系统壁纸同步到该记录的 `snapshot.png`。
- 对一个已删除或手动破坏的 `last_snapshot_path` 做 smoke：切换时系统壁纸保持旧值，诊断面出现 `static-snapshot-sync/snapshot-unavailable`。
- 对一个系统应用失败的环境做 smoke：切换不应污染 active player state，诊断面出现 `static-snapshot-sync/apply-failed`。
- 若当前壁纸已存在 player 窗口，切换时菜单栏背景刷新可以 best-effort；失败只允许记录 `static-snapshot-sync/menu-bar-refresh-degraded`。

## 生命周期回归

- 主窗口关闭只隐藏 Workbench，不退出进程。
- 托盘 `Open Workbench / Pause Player / Quit` 行为正常。
- `Reopen` 只影响 Workbench，不打断当前 player。
- 多屏 player 在显示器拓扑变化后能重建并重新绑定原生宿主。

## 权限与环境检查

- macOS `Screen Recording` 权限状态已确认。
- 共享音频服务不可用时，诊断列表会出现 `shared-audio/capture-unavailable`，且不会把播放器主链拖挂。
- Web 样例依赖的本地资源、相对路径和注入 HTML 都能通过真实 runtime URL 访问。

## 诊断检查

- `native-video`、`native-web`、`shared-audio` 的异常会进入统一诊断面。
- `static-snapshot-sync/snapshot-unavailable` 表示当前 active wallpaper 没有可用 `last_snapshot_path`；用户可见语义是“动态壁纸继续，系统壁纸保持原值”。
- `static-snapshot-sync/apply-failed` 表示 snapshot 文件存在但 macOS 拒绝应用；用户可见语义是“动态壁纸继续，系统壁纸保持原值”。
- `static-snapshot-sync/menu-bar-refresh-degraded` 表示系统壁纸已应用，但 player window 顶部 tint 刷新失败；该诊断不能阻断 apply。
- 当前主 runtime 的启动失败会回传错误；非当前宿主的清理失败只能记为诊断，不得阻断本次切换。
- 发布前应确认没有稳定复现的未清理错误诊断。
