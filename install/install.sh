#!/bin/sh
# codegraph installer — macOS and Linux.
#
# One-liner:
#   curl -fsSL https://github.com/ravijaanthony/codegraph/releases/latest/download/install.sh | sh
#   (or the pinned one-liner from the README)
#
# What it does, always printed as it happens:
#   1. detects OS and CPU architecture
#   2. resolves which release to install (--version, else latest)
#   3. downloads the static musl binary + checksums.txt to a temp dir
#   4. verifies the SHA-256 — aborts on mismatch, installs nothing
#   5. installs to ~/.codegraph/bin (or $CODEGRAPH_INSTALL_DIR) — no admin rights
#   6. offers to add that directory to PATH (interactive; skipped when piped)
#
# Flags:
#   --version X.Y.Z    install a specific version instead of latest
#   --uninstall        remove ~/.codegraph (or $CODEGRAPH_INSTALL_DIR) entirely
#   --help             this help
#
# Environment overrides:
#   CODEGRAPH_RELEASE_BASE   base URL of releases (for mirrors/tests)
#   CODEGRAPH_INSTALL_DIR    install directory (default ~/.codegraph)
#
# Targets: <arch>-apple-darwin on macOS, <arch>-unknown-linux-musl on Linux
# (static — runs on any Linux regardless of glibc). macOS binaries are
# ad-hoc signed in CI (Apple Silicon refuses unsigned executables).

set -eu

BASE="${CODEGRAPH_RELEASE_BASE:-https://github.com/ravijaanthony/codegraph}"
INSTALL_ROOT="${CODEGRAPH_INSTALL_DIR:-$HOME/.codegraph}"
BIN_DIR="$INSTALL_ROOT/bin"
BIN_PATH="$BIN_DIR/codegraph"
PINNED_VERSION=""
DO_UNINSTALL=0

say() { printf '%s\n' "codegraph installer: $*"; }
fail() { printf '%s\n' "codegraph installer: ERROR: $*" >&2; exit 1; }

usage() {
    cat <<'EOF'
usage: install.sh [--version X.Y.Z] [--uninstall] [--help]

  --version X.Y.Z   install a specific version instead of latest
  --uninstall       remove ~/.codegraph (or $CODEGRAPH_INSTALL_DIR)
  --help            this help

Environment:
  CODEGRAPH_RELEASE_BASE   base URL of the releases (mirrors/tests)
  CODEGRAPH_INSTALL_DIR    install directory (default: ~/.codegraph)
EOF
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) [ $# -ge 2 ] || fail "--version needs a value"; PINNED_VERSION="$2"; shift 2 ;;
        --version=*) PINNED_VERSION="${1#*=}"; shift ;;
        --uninstall) DO_UNINSTALL=1; shift ;;
        --help|-h) usage ;;
        *) fail "unknown argument: $1 (try --help)" ;;
    esac
done

if [ "$DO_UNINSTALL" = "1" ]; then
    if [ -d "$INSTALL_ROOT" ]; then
        say "removing $INSTALL_ROOT"
        rm -rf "$INSTALL_ROOT"
        say "done. If you added $BIN_DIR to PATH in your shell rc file, remove that line by hand."
    else
        say "nothing to uninstall ($INSTALL_ROOT does not exist)"
    fi
    exit 0
fi

# --- 1. OS and architecture ---------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
    Darwin) PLATFORM="apple-darwin" ;;
    Linux)  PLATFORM="unknown-linux-musl" ;;   # static musl: runs everywhere
    *) fail "unsupported OS: $OS (Windows: use install.ps1)" ;;
esac
case "$ARCH" in
    x86_64|amd64)   CPU="x86_64" ;;
    arm64|aarch64)  CPU="aarch64" ;;
    *) fail "unsupported architecture: $ARCH" ;;
esac
TARGET="$CPU-$PLATFORM"
ASSET="codegraph-$TARGET"
say "detected $OS $ARCH -> target $TARGET"

# --- 2. Which release ----------------------------------------------------------
if [ -n "$PINNED_VERSION" ]; then
    TAG="v${PINNED_VERSION#v}"
    say "installing pinned version $TAG"
else
    say "resolving latest release from $BASE"
    # GitHub serves /releases/latest as a 302 to /releases/tag/<tag>.
    # Do NOT follow it (-L would wipe redirect_url); we want the Location.
    LOCATION="$(curl -fsSI -o /dev/null -w '%{redirect_url}' "$BASE/releases/latest" 2>/dev/null)" \
        || LOCATION="$(wget --spider -S "$BASE/releases/latest" 2>&1 | sed -n 's/^ *Location: //p' | tail -1 | tr -d '\r')" \
        || LOCATION=""
    case "$LOCATION" in
        *tag/*) TAG="${LOCATION##*/tag/}" ;;
        *) fail "could not resolve latest release from $BASE (offline? no releases yet?)" ;;
    esac
    TAG="$(printf '%s' "$TAG" | tr -d '\r')"
    say "latest release is $TAG"
fi

DOWNLOAD="$BASE/releases/download/$TAG"

# --- 3+4. Download and verify ---------------------------------------------------
TMPDIR_CG="$(mktemp -d 2>/dev/null || mktemp -d -t codegraph)" || fail "cannot create temp dir"
case "$TMPDIR_CG" in
    "") fail "mktemp failed" ;;
esac
# Quoted everywhere; mktemp output can contain spaces on some systems.
TMP_BIN="$TMPDIR_CG/$ASSET"
TMP_SUMS="$TMPDIR_CG/checksums.txt"
trap 'rm -rf "$TMPDIR_CG"' EXIT INT TERM

say "downloading $DOWNLOAD/$ASSET"
curl -fsSL "$DOWNLOAD/$ASSET" -o "$TMP_BIN" || wget -q "$DOWNLOAD/$ASSET" -O "$TMP_BIN" \
    || fail "download failed: $DOWNLOAD/$ASSET"
say "downloading checksums.txt"
curl -fsSL "$DOWNLOAD/checksums.txt" -o "$TMP_SUMS" || wget -q "$DOWNLOAD/checksums.txt" -O "$TMP_SUMS" \
    || fail "download failed: $DOWNLOAD/checksums.txt"

EXPECTED="$(grep -E "(^|[[:space:]])\*?$ASSET\$" "$TMP_SUMS" | awk '{print $1}' | head -1)"
[ -n "$EXPECTED" ] || fail "checksums.txt has no entry for $ASSET — aborting, nothing was installed"
if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP_BIN" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "$TMP_BIN" | awk '{print $1}')"
elif command -v openssl >/dev/null 2>&1; then
    ACTUAL="$(openssl dgst -sha256 "$TMP_BIN" | awk '{print $NF}')"
else
    fail "no SHA-256 tool found (sha256sum, shasum or openssl) — refusing to install an unverified binary"
fi
EXPECTED="$(printf '%s' "$EXPECTED" | tr 'A-F' 'a-f')"
ACTUAL="$(printf '%s' "$ACTUAL" | tr 'A-F' 'a-f')"
if [ "$ACTUAL" != "$EXPECTED" ]; then
    fail "checksum mismatch for $ASSET (expected $EXPECTED, got $ACTUAL) — refusing to install; nothing was installed"
fi
say "SHA-256 verified ($ACTUAL)"

# --- 5. Install -------------------------------------------------------------------
say "installing to $BIN_DIR"
[ -d "$BIN_DIR" ] || mkdir -p "$BIN_DIR" || fail "cannot create $BIN_DIR"
# Copy to a temp name in the same dir, then rename: atomic on POSIX.
cp "$TMP_BIN" "$BIN_PATH.new" || fail "cannot write $BIN_PATH.new"
chmod 0755 "$BIN_PATH.new" || fail "cannot chmod $BIN_PATH.new"
mv "$BIN_PATH.new" "$BIN_PATH" || fail "cannot move $BIN_PATH.new into place"
say "installed $BIN_PATH"

# --- 6. PATH ---------------------------------------------------------------------
case ":$PATH:" in
    *":$BIN_DIR:"*) say "$BIN_DIR is already on PATH" ;;
    *)
        # Only touch an rc file with explicit confirmation; when the script
        # is piped (no tty on stdin), just print instructions.
        RC=""
        case "${SHELL:-}" in
            */zsh)  RC="$HOME/.zshrc" ;;
            */bash) RC="$HOME/.bashrc" ;;
            */fish) RC="$HOME/.config/fish/config.fish" ;;
        esac
        if [ -t 0 ] && [ -n "$RC" ]; then
            printf 'codegraph installer: add %s to PATH in %s now? [y/N] ' "$BIN_DIR" "$RC"
            read -r ANSWER || ANSWER=""
            case "$ANSWER" in
                y|Y|yes|YES)
                    if [ "$RC" = "$HOME/.config/fish/config.fish" ]; then
                        mkdir -p "$HOME/.config/fish" && printf '\nfish_add_path %s\n' "$BIN_DIR" >> "$RC"
                    else
                        printf '\nexport PATH="%s:$PATH"\n' "$BIN_DIR" >> "$RC"
                    fi
                    say "added to PATH in $RC — restart your shell or: export PATH=\"$BIN_DIR:\$PATH\""
                    ;;
                *) say "skipped. Add this to your shell rc by hand: export PATH=\"$BIN_DIR:\$PATH\"" ;;
            esac
        else
            say "$BIN_DIR is not on PATH. Add this to your shell rc (e.g. ~/.zshrc):"
            say "  export PATH=\"$BIN_DIR:\$PATH\""
        fi
        ;;
esac

say "done. Try: codegraph --version && codegraph index /path/to/repo"
