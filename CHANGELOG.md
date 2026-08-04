# Changelog

All notable changes to Aether Tray are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Section headers use the release tag format (`## [v0.1.0]`) so the release
workflow can extract the entry for the version being published.

## [Unreleased]

### Added

- (nothing yet)

## [v0.1.0] - 2026-08-05

### Added

- Tray-only client for the [Aether](https://github.com/CluvexStudio/Aether)
  censorship-circumvention core, with no window and no system network changes
  (no TUN device, routing, DNS, system proxy, or elevation).
- One-click connect with a fully configurable profile: protocol
  (Auto / MASQUE / WireGuard / gool), scan mode, IP version, obfuscation
  profiles, quick reconnect, and MASQUE-over-HTTP/2.
- **Connection info**: while connected, the tray resolves the tunnel's exit
  IP, location, and ISP **through** the SOCKS5 proxy (ifconfig.co) and shows
  them as a tray submenu — with live uptime, one-click **Copy IP**, and
  **Refresh** without reconnecting. Opt-out via the "Connection info" toggle.
- Smart notifications for the transitions that matter (connected, connection
  lost, errors), plus launch-at-login and connect-on-launch.
- Cloudflare Zero Trust support: organization teams, email / service-token /
  access-token sign-in, and Gateway routing — credentials are scrubbed before
  any profile is persisted and never reach disk.
- Routing controls: block and direct domain/CIDR lists plus an Aether rules
  file.
- CLI remote control over a local socket: `connect`, `disconnect`, and
  `status [--json]` (status includes the resolved geo info), plus a headless
  `--no-tray` supervisor mode for servers and tray-less desktops.
- Automatic reconnect with backoff after unexpected drops, orphan reaping on
  crash, and graceful Ctrl-C shutdown that never leaves the core holding the
  SOCKS port.
- First-run migration of settings from the windowed Aether-GUI app, leaving
  the original files untouched so both apps keep working.
- Automated release pipeline building macOS (arm64/x86_64), Linux
  (x86_64/aarch64), and Windows (x86_64) artifacts.

### Fixed

- Cross-compiling the Linux aarch64 build: pkg-config is now configured for
  the arm64 sysroot (`PKG_CONFIG_ALLOW_CROSS` + arm64 `PKG_CONFIG_LIBDIR`),
  which `glib-sys`/`gtk-sys` require.

[Unreleased]: https://github.com/rendinjast/aether-tray/compare/v0.1.0...HEAD
[v0.1.0]: https://github.com/rendinjast/aether-tray/releases/tag/v0.1.0
