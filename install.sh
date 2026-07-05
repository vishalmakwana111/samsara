#!/bin/sh
# samsara installer — one-click download of the latest release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/vishalmakwana111/samsara/master/install.sh | sh
#
# Env overrides:
#   SAMSARA_INSTALL_DIR   where to install (default: $HOME/.local/bin)
#   SAMSARA_VERSION       tag to install (default: latest)
set -eu

REPO="vishalmakwana111/samsara"
BIN="samsara"
INSTALL_DIR="${SAMSARA_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${SAMSARA_VERSION:-latest}"

say()  { printf '\033[38;2;150;120;232m✦\033[0m %s\n' "$1"; }
die()  { printf '\033[38;2;232;120;92m✶\033[0m %s\n' "$1" >&2; exit 1; }

os=$(uname -s)
arch=$(uname -m)
case "$os-$arch" in
  Darwin-arm64)          target="aarch64-apple-darwin" ;;
  Darwin-x86_64)         target="x86_64-apple-darwin" ;;
  Linux-x86_64|Linux-amd64) target="x86_64-unknown-linux-musl" ;;
  Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-musl" ;;
  *) die "unsupported platform: $os-$arch" ;;
esac

asset="${BIN}-${target}.tar.gz"
if [ "$VERSION" = "latest" ]; then
  url="https://github.com/${REPO}/releases/latest/download/${asset}"
else
  url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
fi

command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || die "need curl or wget"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "downloading ${asset} (${VERSION})"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$url" -o "$tmp/$asset" || die "download failed: $url"
else
  wget -qO "$tmp/$asset" "$url" || die "download failed: $url"
fi

say "extracting"
tar -xzf "$tmp/$asset" -C "$tmp" || die "extract failed"
[ -f "$tmp/$BIN" ] || die "binary '$BIN' not found in archive"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/$BIN" "$INSTALL_DIR/$BIN" 2>/dev/null \
  || { cp "$tmp/$BIN" "$INSTALL_DIR/$BIN" && chmod 0755 "$INSTALL_DIR/$BIN"; }

say "installed $BIN -> $INSTALL_DIR/$BIN"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) : ;;
  *) printf '\n\033[38;2;245;158;66m➤\033[0m add it to your PATH:\n    export PATH="%s:$PATH"\n' "$INSTALL_DIR" ;;
esac
"$INSTALL_DIR/$BIN" --version 2>/dev/null || true
