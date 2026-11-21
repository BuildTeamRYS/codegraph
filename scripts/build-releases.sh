#!/usr/bin/env bash
# Build all distributable codegraph artifacts into ./dist/.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: ./scripts/build-releases.sh [OPTIONS]

Builds these artifacts into ./dist/:
  - codegraph-linux-x64.tar.gz
  - codegraph-<version>-1.x86_64.rpm
  - codegraph-windows-x64.zip
  - codegraph-macos-arm64.tar.gz
  - SHA256SUMS

Options:
  -h, --help    Show this help and exit

One-time prerequisites:
  rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu aarch64-apple-darwin
  Fedora/RHEL host tools:
    sudo dnf install -y mingw64-gcc mingw64-binutils zip clang llvm lld cmake make \
      libxml2-devel openssl-devel xz-devel bzip2-devel
  cargo install cargo-generate-rpm
  osxcross with an Apple SDK installed at ~/osxcross:
    git clone https://github.com/tpoechtrager/osxcross.git ~/osxcross
    # put MacOSX*.sdk.tar.xz in ~/osxcross/tarballs/
    cd ~/osxcross && ./build.sh
  The SDK must come from Apple Xcode / Command Line Tools and comply with its license.

Note: OSX_AR is resolved with /bin/ls because interactive 'ls' may be aliased to lsd.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
export PATH="$HOME/osxcross/target/bin:$PATH"

preflight() {
  local missing=()
  local cmd
  for cmd in cargo rustup strip zip sha256sum file \
             x86_64-w64-mingw32-gcc x86_64-w64-mingw32-ar x86_64-w64-mingw32-strip; do
    command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
  done
  cargo generate-rpm --version >/dev/null 2>&1 || missing+=("cargo-generate-rpm")
  command -v oa64-clang >/dev/null 2>&1 || missing+=("oa64-clang (~/osxcross/target/bin)")
  compgen -G "$HOME/osxcross/target/bin/aarch64-apple-*-ar" >/dev/null 2>&1 || \
    missing+=("aarch64-apple-*-ar wrapper (~/osxcross/target/bin)")
  for target in x86_64-unknown-linux-gnu x86_64-pc-windows-gnu aarch64-apple-darwin; do
    rustup target list --installed 2>/dev/null | grep -qx "$target" || \
      missing+=("rust target $target")
  done
  if (( ${#missing[@]} > 0 )); then
    printf 'Missing prerequisites:\n' >&2
    printf '  - %s\n' "${missing[@]}" >&2
    printf '\nRun ./scripts/build-releases.sh --help for setup details.\n' >&2
    exit 1
  fi
}

preflight

VER=$(awk -F'"' '/^version[[:space:]]*=/ { print $2; exit }' Cargo.toml)
: "${VER:?could not read workspace version from Cargo.toml}"

mkdir -p dist
rm -f dist/SHA256SUMS

# Linux x86_64 archive + RPM.
# RPM metadata lives in crates/cli/Cargo.toml ([package.metadata.generate-rpm]).
LINUX_TARGET=x86_64-unknown-linux-gnu
cargo build --release -p codegraph-cli --target "$LINUX_TARGET"
strip -s "target/$LINUX_TARGET/release/codegraph"
tar -czf dist/codegraph-linux-x64.tar.gz \
  -C "target/$LINUX_TARGET/release" codegraph
cargo generate-rpm -p crates/cli --target "$LINUX_TARGET" \
  -o "dist/codegraph-$VER-1.x86_64.rpm"

# Windows x64 (GNU toolchain, not MSVC).
WINDOWS_TARGET=x86_64-pc-windows-gnu
CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR=x86_64-w64-mingw32-ar \
cargo build --release -p codegraph-cli --target "$WINDOWS_TARGET"
x86_64-w64-mingw32-strip -s "target/$WINDOWS_TARGET/release/codegraph.exe"
rm -f dist/codegraph-windows-x64.zip
(
  cd "target/$WINDOWS_TARGET/release"
  zip -9 -q ../../../dist/codegraph-windows-x64.zip codegraph.exe
)

# macOS Apple Silicon via osxcross.
# Use /bin/ls: interactive 'ls' may be aliased to lsd, which annotates symlinks.
OSX_AR=$(/bin/ls "$HOME/osxcross/target/bin"/aarch64-apple-*-ar 2>/dev/null | head -1)
if [[ -z "$OSX_AR" ]]; then
  echo "osxcross aarch64 ar wrapper not found; run ~/osxcross/build.sh first" >&2
  exit 1
fi

MACOSX_TARGET=aarch64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=11.0 \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=oa64-clang \
CARGO_TARGET_AARCH64_APPLE_DARWIN_AR="$OSX_AR" \
CC_aarch64_apple_darwin=oa64-clang \
CXX_aarch64_apple_darwin=oa64-clang++ \
AR_aarch64_apple_darwin="$OSX_AR" \
cargo build --release -p codegraph-cli --target "$MACOSX_TARGET"
tar -czf dist/codegraph-macos-arm64.tar.gz \
  -C "target/$MACOSX_TARGET/release" codegraph

(
  cd dist
  sha256sum * > SHA256SUMS
)

file "target/$LINUX_TARGET/release/codegraph" \
     "target/$WINDOWS_TARGET/release/codegraph.exe" \
     "target/$MACOSX_TARGET/release/codegraph"
