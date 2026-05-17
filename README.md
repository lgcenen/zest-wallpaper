<div align="center">
  <p>
    <a href="./README.en.md"><strong>English</strong></a>
    ·
    <a href="./README.md"><strong>简体中文</strong></a>
  </p>

  <img src="./src-tauri/icons/icon.png" width="160" alt="Zest Wallpaper 图标" />

  <h1>Zest Wallpaper</h1>

  <p><strong>面向 macOS 的动态壁纸播放器，用于导入并播放 Wallpaper Engine 项目。</strong></p>

  <p>
    <img alt="platform" src="https://img.shields.io/badge/platform-macOS%2014%2B-111111?style=flat-square&logo=apple" />
    <img alt="license" src="https://img.shields.io/badge/license-Apache%202.0-0F766E?style=flat-square" />
    <img alt="runtime" src="https://img.shields.io/badge/runtime-Scene%20%7C%20Video%20%7C%20Web-2563EB?style=flat-square" />
    <img alt="status" src="https://img.shields.io/badge/status-active%20development-F59E0B?style=flat-square" />
  </p>

  <p>
    <a href="https://github.com/lgcenen/zest-wallpaper/stargazers"><img alt="stars" src="https://img.shields.io/github/stars/lgcenen/zest-wallpaper?style=flat-square&label=stars" /></a>
    <a href="https://github.com/lgcenen/zest-wallpaper/forks"><img alt="forks" src="https://img.shields.io/github/forks/lgcenen/zest-wallpaper?style=flat-square&label=forks" /></a>
    <a href="https://github.com/lgcenen/zest-wallpaper/issues"><img alt="issues" src="https://img.shields.io/github/issues/lgcenen/zest-wallpaper?style=flat-square&label=issues" /></a>
  </p>
</div>

## 当前状态

本项目仍处于持续开发中。

## 社区交流

- 项目 QQ 交流群：[`867740762`](https://qm.qq.com/q/984wIUELGE)

## 支持范围

| 能力 | 状态 |
| --- | --- |
| 场景壁纸 | 部分支持 |
| 视频壁纸 | 支持 |
| 网页壁纸 | 支持 |
| 音频输入 | 支持 |
| 全屏自动暂停 | 支持 |
| 多显示器 | 尚未测试 |
| 创意工坊壁纸下载 | 未支持 |

## 平台与技术栈

- 仅限 macOS `14.0+` 使用
- 当前打包产物仅支持 Apple Silicon (`arm64`)
- 目前暂不支持 Intel 芯片 Mac
- Tauri 2
- React + Vite
- Rust

## 安装

### 1. 从 Releases 下载

从 GitHub Releases 下载最新测试版。

如果 macOS 阻止应用打开，你可以先尝试：

- 前往“系统设置 -> 隐私与安全性”，点击“仍要打开”

也可以在终端执行：

```bash
xattr -dr com.apple.quarantine "/Applications/Zest Wallpaper.app"
```

然后重新打开应用。

### 2. 自行构建

前置依赖：

- Node.js
- Rust toolchain
- Xcode Command Line Tools

安装依赖：

```bash
npm install
```

启动前端开发服务器：

```bash
npm run dev
```

启动完整 Tauri 开发应用：

```bash
npm run tauri:dev -- --no-watch
```

构建前端：

```bash
npm run build
```

构建应用包：

```bash
npm run tauri:build
```

运行测试：

```bash
npm test
cargo test -j 1 --manifest-path src-tauri/Cargo.toml
cargo check -j 1 --manifest-path src-tauri/Cargo.toml
```

## 特别鸣谢

感谢以下项目提供的公开工具与生态参考价值：

- [`repkg`](https://github.com/notscuffed/repkg)
- [`linux-wallpaperengine`](https://github.com/Almamu/linux-wallpaperengine)

## 许可证

Apache License 2.0。参见 [LICENSE](./LICENSE)。

## 声明

`Wallpaper Engine` 是第三方产品。本项目是一个独立的 macOS 播放器，不隶属于、不受其官方认可、不代表其官方立场，也不与 Wallpaper Engine 或其专有资源一同分发。

还需要明确以下边界：

- 本仓库不分发第三方专有壁纸资源
- 导入的壁纸内容应由用户自行提供
- 可以通过本地挂载外部 `assets` 目录来提升 Scene 兼容性
- 即使没有外部 `assets`，应用本身也必须保持可运行
- 如果你使用本地外部 `assets`，请将它们保存在仓库之外

本仓库的目标是提供独立播放器实现，而不是作为任何第三方商业内容的再分发渠道。
