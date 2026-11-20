#!/usr/bin/env sh
set -eu

REPO="BuildTeamRYS/codegraph"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64|amd64) ASSET="codegraph-linux-x64.tar.gz" ;;
      aarch64|arm64) ASSET="codegraph-linux-arm64.tar.gz" ;;
      *) echo "Unsupported Linux architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      arm64) ASSET="codegraph-macos-arm64.tar.gz" ;;
      x86_64) ASSET="codegraph-macos-x64.tar.gz" ;;
      *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
INSTALL_DIR="${HOME}/.local/bin"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$INSTALL_DIR"
echo "Downloading $URL..."
curl -fsSL "$URL" -o "$TMP_DIR/$ASSET"
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

mv "$TMP_DIR/codegraph" "$INSTALL_DIR/codegraph"
chmod +x "$INSTALL_DIR/codegraph"

# On macOS, clear quarantine attribute if needed
if [ "$OS" = "Darwin" ]; then
  xattr -dr com.apple.quarantine "$INSTALL_DIR/codegraph" 2>/dev/null || true
fi

echo "Installed codegraph to $INSTALL_DIR/codegraph"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Add $INSTALL_DIR to your PATH (e.g. export PATH=\"\$HOME/.local/bin:\$PATH\")" ;;
esac
