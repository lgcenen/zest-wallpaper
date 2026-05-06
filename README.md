<div align="center">
  <p>
    <a href="./README.md"><strong>English</strong></a>
    ·
    <a href="./README.zh-CN.md"><strong>简体中文</strong></a>
  </p>

  <img src="./src-tauri/icons/icon.png" width="160" alt="Zest Wallpaper icon" />

  <h1>Zest Wallpaper</h1>

  <p><strong>macOS-only wallpaper player for importing and playing Wallpaper Engine projects.</strong></p>

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

## Status

This project is under active development.

## Community

- QQ group: [`867740762`](https://qm.qq.com/q/984wIUELGE)

## Scope

| Capability | Status |
| --- | --- |
| Scene wallpapers | Partially supported |
| Video wallpapers | Supported |
| Web wallpapers | Supported |
| Audio input | Supported |
| Fullscreen auto-pause | Supported |
| Multi-display | Not yet tested |
| Workshop wallpaper download | Not supported |

## Platform and Stack

- macOS `14.0+` only
- current packaged builds support Apple Silicon (`arm64`) only
- Intel Mac builds are not supported at this time
- Tauri 2
- React + Vite
- Rust

## Installation

### 1. Download from Releases

Download the latest test build from GitHub Releases.

If macOS blocks the app from opening, try this first:

- Go to `System Settings -> Privacy & Security` and click `Open Anyway`

You can also run this in Terminal:

```bash
xattr -dr com.apple.quarantine "/Applications/Zest Wallpaper.app"
```

Then open the app again.

### 2. Build it yourself

Prerequisites:

- Node.js
- Rust toolchain
- Xcode Command Line Tools

Install dependencies:

```bash
npm install
```

Run the frontend:

```bash
npm run dev
```

Run the full Tauri app in development:

```bash
npm run tauri:dev -- --no-watch
```

Build the frontend:

```bash
npm run build
```

Build the app bundle:

```bash
npm run tauri:build
```

Run tests:

```bash
npm test
cargo test -j 1 --manifest-path src-tauri/Cargo.toml
cargo check -j 1 --manifest-path src-tauri/Cargo.toml
```

## Special Thanks

Thanks to these projects for public tooling and ecosystem reference value:

- [`repkg`](https://github.com/notscuffed/repkg)
- [`linux-wallpaperengine`](https://github.com/Almamu/linux-wallpaperengine)

## License

Apache License 2.0. See [LICENSE](./LICENSE).

## Disclaimer

`Wallpaper Engine` is a third-party product. This project is an independent macOS player. It is not affiliated with, endorsed by, authorized by, or distributed together with Wallpaper Engine or any proprietary Wallpaper Engine assets.

Additional boundaries:

- this repository does not ship third-party proprietary wallpaper assets
- imported wallpaper content is expected to come from the user
- external `assets` directories may be mounted locally to improve Scene compatibility
- the app is expected to remain runnable even without external assets
- if you use local external assets, keep them outside this repository

This repository is intended to provide an independent player implementation, not a redistribution channel for third-party commercial content.
