#!/usr/bin/env bash
# Packages an existing .app bundle into a drag-and-drop DMG.
#
# The DMG contains the app plus an "Applications" symlink, so the user opens
# it and drags the app onto the folder — the classic macOS install flow.
# Only tools that ship with macOS are used (ditto, hdiutil), so CI needs
# nothing extra.
set -euo pipefail

APP="${1:?usage: macos-dmg.sh <path/to/Aether.app> <output.dmg>}"
DMG="${2:?usage: macos-dmg.sh <path/to/Aether.app> <output.dmg>}"

[[ -d "$APP" ]] || { echo "error: $APP is not a directory" >&2; exit 1; }

APP_NAME="$(basename "$APP")"
VOLNAME="${APP_NAME%.app}"
STAGING="$(mktemp -d "${TMPDIR:-/tmp}/aether-dmg.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT

# ditto preserves symlinks and metadata inside the bundle, unlike cp -R.
ditto "$APP" "$STAGING/$APP_NAME"
# The drag-and-drop target.
ln -s /Applications "$STAGING/Applications"

mkdir -p "$(dirname "$DMG")"
rm -f "$DMG"
hdiutil create \
  -volname "$VOLNAME" \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  -imagekey zlib-level=9 \
  "$DMG" >/dev/null

echo "Built $DMG"
