# Static Snapshot Sync Contract

Zest Wallpaper is a dynamic wallpaper player. Static snapshots are a companion capability for the current active dynamic wallpaper, not a separate product mode.

## Product Contract

- The active dynamic wallpaper remains the source of truth for the desktop experience.
- A playable `Scene`, `Video`, or `Web` wallpaper may have one cached static snapshot recorded by `last_snapshot_path`.
- When the active dynamic wallpaper changes, the system wallpaper sync target is the static snapshot associated with that same active record.
- The independent static wallpaper mode is forbidden: the product must not expose an arbitrary local image picker, a static wallpaper library, or a manual “apply static wallpaper” action detached from the active dynamic wallpaper.
- Static snapshots may support preview-adjacent workflows such as color sampling, theme extraction, and active-wallpaper status display, but those workflows must stay attached to a wallpaper record.

## Data Boundaries

- `WallpaperRecord` is durable library metadata. It owns import paths, runtime metadata, `preview_path`, and the optional `last_snapshot_path` for the cached static snapshot associated with that record.
- `WallpaperRuntimeRecord` is the UI/runtime transport shape for a playable wallpaper. It may expose top-level `lastSnapshotPath` as record metadata, but `Scene`, `Video`, and `Web` runtime documents must not treat snapshots as render inputs.
- `preview_path` is for Workbench thumbnails, detail presentation, and non-system-wallpaper auxiliary analysis.
- `snapshot_for_record` resolves only `last_snapshot_path`; it MUST NOT fall back to `preview_path`, `entry_path`, or arbitrary image files.
- System wallpaper application state belongs to the native services path and must be keyed by the active wallpaper record, never by a standalone static asset.

## Allowed GUI Entrypoints

- Show the active wallpaper and whether static snapshot sync has a recorded snapshot.
- Show diagnostics when the active wallpaper has no usable static snapshot.
- Offer future current-active-wallpaper retry or regeneration controls only if they stay scoped to the active dynamic wallpaper.

## Forbidden GUI Entrypoints

- “Apply this preview as system wallpaper.”
- “Choose any local image as wallpaper.”
- A separate static wallpaper collection, queue, or mode.
- Applying a snapshot from an inactive wallpaper record.

## Missing Snapshot Behavior

- If `last_snapshot_path` points to an existing supported image, system wallpaper sync may apply it for the current active wallpaper.
- If `last_snapshot_path` is empty, relative, missing on disk, or an unsupported image format, the sync path records a diagnostic and leaves the current macOS system wallpaper unchanged.
- `preview_path` is never a replacement for a missing snapshot, even when it points to a valid image.
