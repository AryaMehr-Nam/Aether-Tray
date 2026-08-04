# aether-tray

A tray-only native client for [Aether](https://github.com/CluvexStudio/Aether), a
Cloudflare WARP censorship-circumvention tool. It launches the `aether` core with
the right flags, watches it, and tells you whether the local SOCKS5 proxy is up.

There is no application window. Everything lives in the system tray, with native
OS dialogs for the handful of genuinely free-text settings.

## What it does

The core exposes a local SOCKS5 proxy (default `127.0.0.1:1819`) and you point
applications at it. Nothing touches the system network stack: no TUN device, no
routing-table changes, no DNS reconfiguration, no system proxy, and no privilege
elevation anywhere.

```
curl --socks5 127.0.0.1:1819 https://cloudflare.com/cdn-cgi/trace
```

## Getting the core binary

The `aether` core is a separate executable, not vendored here:

```
./binaries/fetch-aether.sh
```

It is looked up in this order — `<exe_dir>/aether`, then `core_path` in
`config.toml`, then `$PATH`.

## Usage

```
aether-tray                    launch the tray (default)
aether-tray --no-tray          headless supervisor, logs to stdout
aether-tray connect [--protocol masque --scan-mode turbo --ip-version both --bind 0.0.0.0:1819]
aether-tray disconnect
aether-tray status [--json]
```

The subcommands talk to a running tray over a local socket. If nothing is
listening, `connect` starts a headless supervisor in-process rather than failing.

## Platform notes

**Linux tray requires an StatusNotifierItem host.** This is the real
caveat, not a footnote: on stock GNOME there is no tray at all, and you must
install an AppIndicator extension (e.g. [AppIndicator and KStatusNotifierItem
Support](https://extensions.gnome.org/extension/615/appindicator-support/)).
Without one the tray icon silently does not appear — run `--no-tray` instead.
GTK3 and libayatana-appindicator are also required at build and run time.

**Wayland** gives no cursor position, so menu placement is compositor-controlled.

**Linux dialogs** need `zenity` or `kdialog`. With neither installed, the
free-text menu items are greyed out and "Edit config.toml…" is the way to change
those settings; everything expressible as a radio group or checkbox still works
from the menu.

## Configuration

`config.toml` in the platform config directory:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/aether-tray/` |
| Windows | `%APPDATA%\aether-tray\` |
| Linux | `~/.config/aether-tray/` |

The profile is persisted only on reaching `Connected`, never on a mere attempt,
so a bad guess cannot poison future one-click connects. All four Cloudflare
Access credentials are scrubbed before writing — they stay in process memory for
the session and are asked for again next launch.

On first run, settings are migrated from the Tauri stores of the windowed
Aether-GUI app (`com.cluvexstudio.aethergui`). The originals are left untouched,
so the old app keeps working. The core's working directory deliberately stays
pointed at the old data directory when it exists: the core writes its
provisioned WARP identity (`aether-masque.toml` / `aether.toml`) there, and
moving it would silently re-provision every existing user's identity.

## Development

```
cargo test
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

`src/core/` is a near-verbatim port of the windowed app's `src-tauri/src/aether/`.
Several behaviours there encode bugs someone already hit in production and must
not be "cleaned up" — they are marked with comments explaining why. In
particular: prompt headers match by **suffix** (exact matching caused an infinite
Protocol/Scan-mode loop), re-seeing a header **clears** the answered flag, a bare
header as the partial buffer must **not** be answered, and "connected" means the
SOCKS5 port accepts a TCP connect rather than any log text.

Two directories that look interchangeable are not. The core's working directory
is shared with the windowed app so the provisioned WARP identity persists;
our own state (the pid file) lives in a separate directory, because orphan
reaping must never read a pid belonging to another process.

## Packaging

```
./packaging/macos-app.sh            # dist/Aether.app
./packaging/macos-dmg.sh <app> <dmg>  # drag-and-drop DMG from an existing bundle
./packaging/linux-appimage.sh       # dist/Aether-<version>-x86_64.AppImage
pwsh -File packaging/windows-zip.ps1  # dist/Aether-<version>-x86_64-windows.zip
```

Each script bundles the fetched core next to the executable, where the binary
looks first. `release.yml` packages macOS arm64/x86_64 (zip + drag-and-drop
DMG), Linux x86_64/aarch64 (AppImage) and Windows x86_64 (zip) and opens a
release — either from a `v*` tag or automatically from a push to `main`.

The macOS bundle sets `LSUIElement` and the binary requests
`NSApplicationActivationPolicyAccessory`, so there is no Dock icon or Cmd-Tab
entry either way. Bundles are **ad-hoc signed only** — a real release needs a
Developer ID and notarisation, so on first launch users must right-click → Open.

## Licence

AGPL-3.0-or-later, inherited from Aether-GUI. See [LICENSE](LICENSE).
