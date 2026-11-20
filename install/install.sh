#!/usr/bin/env sh
set -eu

REPO="BuildTeamRYS/codegraph"
BIN_NAME="codegraph"
INSTALL_DIR="${HOME}/.local/bin"
CARGO_BIN="${HOME}/.cargo/bin"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64|amd64|aarch64|arm64) ;;
      *) echo "Unsupported Linux architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      arm64|x86_64) ;;
      *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# 1. Ensure a downloader and extractor are available
require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required tool: $1" >&2
    exit 1
  fi
}

require_cmd tar

download_file() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$out" "$url"
  else
    echo "Neither curl nor wget is installed. Please install curl and try again." >&2
    exit 1
  fi
}

# 2. Install Rust via rustup if cargo is not present
if ! command -v cargo >/dev/null 2>&1; then
  if [ -x "${CARGO_BIN}/cargo" ]; then
    export PATH="${CARGO_BIN}:$PATH"
  else
    echo "Rust not found. Installing Rust toolchain via rustup..."
    RUSTUP_SCRIPT="$(mktemp)"
    download_file "https://sh.rustup.rs" "$RUSTUP_SCRIPT"
    sh "$RUSTUP_SCRIPT" -y --profile minimal --no-modify-path
    rm -f "$RUSTUP_SCRIPT"
    export PATH="${CARGO_BIN}:$PATH"
  fi
fi

# 3. Download source archive and build in a temporary directory
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

SRC_ARCHIVE="$TMP_DIR/src.tar.gz"
SRC_URL="https://github.com/${REPO}/archive/refs/heads/main.tar.gz"

echo "Downloading source from $SRC_URL..."
download_file "$SRC_URL" "$SRC_ARCHIVE"
tar -xzf "$SRC_ARCHIVE" -C "$TMP_DIR"

SRC_DIR="$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
if [ -z "$SRC_DIR" ] || [ ! -d "$SRC_DIR" ]; then
  echo "Failed to locate extracted source directory." >&2
  exit 1
fi

echo "Compiling $BIN_NAME (this may take a couple of minutes)..."
(
  cd "$SRC_DIR"
  cargo build --release --locked
)

BUILT_BIN="${SRC_DIR}/target/release/${BIN_NAME}"
if [ ! -x "$BUILT_BIN" ]; then
  # Fallback if Cargo package/binary name differs from BIN_NAME
  FOUND_BIN="$(find "${SRC_DIR}/target/release" -maxdepth 1 -type f -perm -u+x ! -name '*.d' ! -name '*.so' ! -name '*.dylib' | head -n 1)"
  if [ -n "$FOUND_BIN" ]; then
    BUILT_BIN="$FOUND_BIN"
  else
    echo "Build succeeded, but no compiled binary was found in target/release." >&2
    exit 1
  fi
fi

# 4. Install binary
mkdir -p "$INSTALL_DIR"
mv "$BUILT_BIN" "${INSTALL_DIR}/${BIN_NAME}"
chmod +x "${INSTALL_DIR}/${BIN_NAME}"

if [ "$OS" = "Darwin" ]; then
  xattr -dr com.apple.quarantine "${INSTALL_DIR}/${BIN_NAME}" 2>/dev/null || true
fi

echo "Installed $BIN_NAME to ${INSTALL_DIR}/${BIN_NAME}"
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "Add ${INSTALL_DIR} to your PATH, e.g.:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac
