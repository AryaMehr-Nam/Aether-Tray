#!/usr/bin/env bash
# Builds an AppImage into dist/.
#
# Reads as more ceremony than the macOS bundle because a Linux tray has two
# hard runtime requirements that a bare binary cannot express: GTK3 with
# libayatana-appindicator present, and a StatusNotifierItem host running in
# the session. The launcher script below checks the second and falls back to
# --no-tray rather than exiting silently, which is what happens on stock
# GNOME without an AppIndicator extension.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
APPDIR="$ROOT/dist/AppDir"

cd "$ROOT"
cargo build --release --locked --target "$TARGET"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "target/$TARGET/release/aether-tray" "$APPDIR/usr/bin/aether-tray"
[[ -f "$ROOT/binaries/aether" ]] && cp "$ROOT/binaries/aether" "$APPDIR/usr/bin/aether" \
  || echo "warning: binaries/aether not found — run binaries/fetch-aether.sh first" >&2

cat > "$APPDIR/aether-tray.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Aether
Comment=Tray-only client for the Aether censorship-circumvention tool
Exec=aether-tray
Icon=aether-tray
Categories=Network;
Terminal=false
DESKTOP
cp "$APPDIR/aether-tray.desktop" "$APPDIR/usr/share/applications/"

# A 256x256 PNG is required by the AppImage spec. Generated rather than
# vendored so there is no binary blob in the repo.
python3 - "$APPDIR/usr/share/icons/hicolor/256x256/apps/aether-tray.png" <<'PY'
import struct, sys, zlib

size = 256
cx = cy = (size - 1) / 2
radius = size / 2 - 8
rows = []
for y in range(size):
    row = bytearray([0])  # PNG filter byte: none
    for x in range(size):
        d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
        a = max(0.0, min(1.0, radius - d))
        row += bytes((0x2e, 0xb8, 0x67, int(a * 255)))
    rows.append(bytes(row))

def chunk(tag, data):
    return (struct.pack('>I', len(data)) + tag + data
            + struct.pack('>I', zlib.crc32(tag + data) & 0xffffffff))

png = (b'\x89PNG\r\n\x1a\n'
       + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
       + chunk(b'IDAT', zlib.compress(b''.join(rows), 9))
       + chunk(b'IEND', b''))
open(sys.argv[1], 'wb').write(png)
PY
cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/aether-tray.png" "$APPDIR/aether-tray.png"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="$HERE/usr/bin:$PATH"

# A StatusNotifierItem host must be on the session bus, or the tray icon is
# created and then silently never shown. Stock GNOME has none without an
# AppIndicator extension, so degrade to the headless supervisor instead of
# appearing to do nothing at all.
if [ -z "$AETHER_FORCE_TRAY" ] && [ $# -eq 0 ] && command -v gdbus >/dev/null 2>&1; then
  if ! gdbus call --session --dest org.freedesktop.DBus \
        --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.ListNames 2>/dev/null \
        | grep -q StatusNotifierWatcher; then
    echo "aether-tray: no system tray found on this desktop." >&2
    echo "  GNOME needs an AppIndicator extension:" >&2
    echo "  https://extensions.gnome.org/extension/615/appindicator-support/" >&2
    echo "  Running headless instead; pass AETHER_FORCE_TRAY=1 to override." >&2
    exec "$HERE/usr/bin/aether-tray" --no-tray
  fi
fi

exec "$HERE/usr/bin/aether-tray" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

if ! command -v appimagetool >/dev/null 2>&1; then
  echo "appimagetool not found; AppDir is ready at $APPDIR but was not packed" >&2
  exit 0
fi

mkdir -p "$ROOT/dist"
ARCH="${APPIMAGE_ARCH:-x86_64}" appimagetool "$APPDIR" \
  "$ROOT/dist/Aether-${VERSION}-${APPIMAGE_ARCH:-x86_64}.AppImage"
echo "Built dist/Aether-${VERSION}-${APPIMAGE_ARCH:-x86_64}.AppImage"
