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
    <img alt="platform" src="https://img.shields.io/badge/platform-macOS%2015%2B-111111?style=flat-square&logo=apple" />
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

- QQ group: `867740762`

## Scope

| Capability | Status |
| --- | --- |
| Scene wallpapers | Partially supported |
| Video wallpapers | Supported |
| Web wallpapers | Supported |
| Audio input | Supported |
| Multi-display | Untested |
| Workshop wallpaper download | Not supported |

## Platform and Stack

- macOS `15.0+` only
- Tauri 2
- React + Vite
- Rust

## Development

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

## Assets and Legal Boundaries

This repository does not ship third-party proprietary wallpaper assets.

Important constraints:

- imported wallpaper content is expected to come from the user
- external `assets` directories may be mounted locally to improve Scene compatibility
- the app must remain runnable even without external assets
- this project does not copy source code, resources, or symbol naming from third-party reference apps

If you use local external assets, keep them outside this repository.

## Special Thanks

Thanks to these projects for public tooling and ecosystem reference value:

- [`repkg`](https://github.com/notscuffed/repkg)
- [`linux-wallpaperengine`](https://github.com/Almamu/linux-wallpaperengine)

## License

Apache License 2.0. See [LICENSE](./LICENSE).

## Disclaimer

`Wallpaper Engine` is a third-party product. This project is an independent macOS player and is not affiliated with, endorsed by, or distributed with proprietary Wallpaper Engine assets.
