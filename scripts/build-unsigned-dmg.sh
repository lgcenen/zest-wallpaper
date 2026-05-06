#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="${APP_NAME:-Zest Wallpaper.app}"
APP_PATH="${APP_PATH:-$ROOT_DIR/src-tauri/target/release/bundle/macos/$APP_NAME}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/src-tauri/target/release/bundle/dmg-manual}"
DMG_NAME="${DMG_NAME:-Zest Wallpaper_0.1.0_aarch64.dmg}"
VOLUME_NAME="${VOLUME_NAME:-Zest Wallpaper}"
STAGING_DIR="${STAGING_DIR:-${TMPDIR:-/tmp}/zest-wallpaper-dmg}"

if [[ ! -d "$APP_PATH" ]]; then
  echo "Missing app bundle: $APP_PATH" >&2
  exit 1
fi

rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR" "$OUTPUT_DIR"

cp -R "$APP_PATH" "$STAGING_DIR/$APP_NAME"
ln -s /Applications "$STAGING_DIR/Applications"

hdiutil create \
  -volname "$VOLUME_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$OUTPUT_DIR/$DMG_NAME"

echo "Created DMG: $OUTPUT_DIR/$DMG_NAME"
