#!/usr/bin/env bash
# Downloads the pinned Aether release binary for the current platform into
# binaries/, verified against its published SHA256SUMS.txt.
# Run this before `cargo run` / `cargo build --release`.
#
# Works on Windows too, under Git Bash or the MSYS shell that ships with the
# GitHub Actions windows runners — the release publishes a .zip there rather
# than a tarball, so the extract step differs.
set -euo pipefail

AETHER_VERSION="v1.5.0"
REPO="CluvexStudio/Aether"
DEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)         ASSET="aether-linux-x86_64.tar.gz" ;;
  Linux-aarch64)        ASSET="aether-linux-arm64.tar.gz" ;;
  Darwin-x86_64)        ASSET="aether-macos-x86_64.tar.gz" ;;
  Darwin-arm64)         ASSET="aether-macos-arm64.tar.gz" ;;
  # Git Bash reports MINGW64_NT-*, MSYS reports MSYS_NT-*, Cygwin CYGWIN_NT-*.
  MINGW*|MSYS*|CYGWIN*)
    case "$(uname -m)" in
      x86_64)  ASSET="aether-windows-x86_64.zip" ;;
      aarch64|ARM64) ASSET="aether-windows-aarch64.zip" ;;
      *) echo "Unsupported Windows arch: $(uname -m)" >&2; exit 1 ;;
    esac
    ;;
  *) echo "Unsupported platform: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

# CI override: when cross-building (e.g. the Intel-mac build on an arm64
# runner), the bundled core must match the target arch, not the host's.
ASSET="${AETHER_ASSET:-$ASSET}"

URL="https://github.com/${REPO}/releases/download/${AETHER_VERSION}/${ASSET}"
SUMS_URL="https://github.com/${REPO}/releases/download/${AETHER_VERSION}/SHA256SUMS.txt"

cd "$DEST_DIR"
curl -fsSL -o "$ASSET" "$URL"
curl -fsSL -o SHA256SUMS.txt "$SUMS_URL"

# macOS ships shasum rather than GNU sha256sum.
if command -v sha256sum >/dev/null 2>&1; then
  VERIFY=(sha256sum -c -)
else
  VERIFY=(shasum -a 256 -c -)
fi

# `grep "$ASSET"` alone also matches the "<asset>.sha256" lines the release
# publishes, which name files we never downloaded and would fail the check.
if ! grep -E "[ *]${ASSET}\$" SHA256SUMS.txt | "${VERIFY[@]}"; then
  echo "Checksum verification failed for $ASSET" >&2
  rm -f "$ASSET" SHA256SUMS.txt
  exit 1
fi

case "$ASSET" in
  *.zip) unzip -o -q "$ASSET" ;;
  *)     tar xzf "$ASSET" ;;
esac

# The Windows asset extracts aether.exe; every other target extracts aether.
if [[ -f aether ]]; then
  chmod +x aether
  BINARY="aether"
else
  BINARY="aether.exe"
fi

rm -f "$ASSET" SHA256SUMS.txt
echo "Aether binary ready at $DEST_DIR/$BINARY"
