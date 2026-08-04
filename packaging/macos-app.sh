#!/usr/bin/env bash
# Builds Aether.app for the current (or $TARGET) architecture into dist/.
#
# LSUIElement is the point of this bundle: it makes macOS treat the app as an
# accessory, so there is no Dock icon and no Cmd-Tab entry for something that
# only ever draws a menu-bar item. The binary also sets the equivalent
# activation policy at runtime for the un-bundled case.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
APP="$ROOT/dist/Aether.app"

cd "$ROOT"
cargo build --release --locked --target "$TARGET"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "target/$TARGET/release/aether-tray" "$APP/Contents/MacOS/aether-tray"

# The core is looked up next to the executable first, so bundling it here
# means a user never has to fetch it separately.
if [[ -f "$ROOT/binaries/aether" ]]; then
  cp "$ROOT/binaries/aether" "$APP/Contents/MacOS/aether"
  chmod +x "$APP/Contents/MacOS/aether"
else
  echo "warning: binaries/aether not found — run binaries/fetch-aether.sh first" >&2
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Aether</string>
    <key>CFBundleDisplayName</key><string>Aether</string>
    <key>CFBundleIdentifier</key><string>com.cluvexstudio.aether-tray</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>aether-tray</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <!-- Tray-only: no Dock icon, no Cmd-Tab entry. -->
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc signature. Without any signature at all, Gatekeeper refuses to run
# the bundle on Apple Silicon; a real release needs a Developer ID and
# notarisation, which cannot happen in this script.
codesign --force --deep --sign - "$APP" 2>/dev/null \
  || echo "warning: ad-hoc codesign failed; the bundle may not launch" >&2

echo "Built $APP ($TARGET, v$VERSION)"
