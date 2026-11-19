#!/bin/sh
# codegraph installer — macOS and Linux.
#
#   curl -fsSL https://github.com/BuildTeamRYS/codegraph/releases/latest/download/install.sh | sh
#
# The repo is public: the one-liner above installs the latest release with no
# token. Offline/demo bundles work via --from-dir.
#
# What it does, always printed as it happens:
#   1. detects OS and CPU architecture
#   2. resolves which release to install (--version, --from-dir, else latest)
#   3. downloads the static musl binary + checksums.txt (or finds them in DIR)
#   4. verifies the SHA-256 — aborts on mismatch, installs nothing
#   5. installs to ~/.codegraph/bin (or $CODEGRAPH_INSTALL_DIR) — no admin rights
#   6. offers to add that directory to PATH; prints exactly what went where
#
# Flags:
#   --version X.Y.Z    install a specific version instead of latest
#   --from-dir DIR     install from a local folder of release assets (offline)
#   --skip-checksum    from-dir only: install even without a checksums.txt
#   --uninstall        remove ~/.codegraph (or $CODEGRAPH_INSTALL_DIR); prints
#                      PATH line to delete and offers to undo agent configs
#   --help             this help
#
# Environment overrides:
#   CODEGRAPH_RELEASE_BASE   base URL of releases (for mirrors/tests)
#   CODEGRAPH_INSTALL_DIR    install directory (default ~/.codegraph)

set -eu

BASE="${CODEGRAPH_RELEASE_BASE:-https://github.com/BuildTeamRYS/codegraph}"
INSTALL_ROOT="${CODEGRAPH_INSTALL_DIR:-$HOME/.codegraph}"
BIN_DIR="$INSTALL_ROOT/bin"
BIN_PATH="$BIN_DIR/codegraph"
PINNED_VERSION=""
FROM_DIR=""
SKIP_CHECKSUM=0
DO_UNINSTALL=0

say() { printf '%s\n' "codegraph installer: $*"; }
fail() { printf '%s\n' "codegraph installer: ERROR: $*" >&2; exit 1; }

usage() {
    cat <<'EOF'
usage: install.sh [--version X.Y.Z] [--from-dir DIR] [--skip-checksum] [--uninstall] [--help]

  --version X.Y.Z   install a specific version instead of latest
  --from-dir DIR    install from a local folder of release assets (offline mode)
  --skip-checksum   from-dir only: install even without a checksums.txt
  --uninstall       remove ~/.codegraph (or $CODEGRAPH_INSTALL_DIR), print the
                    PATH line to delete, offer to undo agent configs
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
        --from-dir) [ $# -ge 2 ] || fail "--from-dir needs a directory"; FROM_DIR="$2"; shift 2 ;;
        --from-dir=*) FROM_DIR="${1#*=}"; shift ;;
        --skip-checksum) SKIP_CHECKSUM=1; shift ;;
        --uninstall) DO_UNINSTALL=1; shift ;;
        --help|-h) usage ;;
        *) fail "unknown argument: $1 (try --help)" ;;
    esac
done

file_size() {
    # bytes of a file (POSIX-ish; falls back to 0)
    wc -c < "$1" 2>/dev/null | tr -d ' ' || printf 0
}

# --- summary helpers -----------------------------------------------------------
final_summary() {
    INSTALLED_VERSION="$("$BIN_PATH" --version 2>/dev/null | awk '{print $2}' || true)"
    printf '\n'
    say "done."
    say "  installed (uninstall with this script --uninstall):"
    say "    $BIN_PATH${INSTALLED_VERSION:+ (codegraph $INSTALLED_VERSION)}"
    say "  agent configs created by 'codegraph init' point at this exact path"
    say "  undo everything:"
    say "    1) codegraph agents remove <id>   # per agent you configured (or 'codegraph agents list')"
    say "    2) $0 --uninstall"
    say "  try it: codegraph --version && codegraph index /path/to/repo"
    say "  diagnose a setup: codegraph doctor"
}

if [ "$DO_UNINSTALL" = "1" ]; then
    printf '\n'
    say "[1/3] agent configs"
    if [ -x "$BIN_PATH" ]; then
        printf -- "--- codegraph agents list (current repo) ---\n"
        "$BIN_PATH" agents list || true
        if [ -t 0 ]; then
            printf 'remove codegraph MCP entries from configured agents now? [y/N] '
            read -r ANSWER || ANSWER=""
            case "$ANSWER" in
                y|Y|yes|YES)
                    for id in claude-code cursor vscode-copilot codex-cli gemini-cli antigravity opencode windsurf claude-desktop; do
                        "$BIN_PATH" agents remove "$id" || true
                    done
                    ;;
                *) say "skipping agent removal — do it later with: codegraph agents remove <id>" ;;
            esac
        else
            say "not a tty; leaving agent configs alone. Remove them with:"
            say "  codegraph agents remove <id>   # claude-code | cursor | vscode-copilot | codex-cli | gemini-cli | antigravity | opencode | windsurf | claude-desktop"
        fi
    else
        say "$BIN_PATH not present — nothing agent-side to do"
    fi
    printf '\n'
    say "[2/3] remove install root"
    if [ -d "$INSTALL_ROOT" ]; then
        say "removing $INSTALL_ROOT"
        rm -rf "$INSTALL_ROOT"
        say "removed."
    else
        say "nothing to uninstall ($INSTALL_ROOT does not exist)"
    fi
    printf '\n'
    say "[3/3] PATH cleanup (if this directory is in a shell rc file)"
    FOUND=0
    for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile" "$HOME/.config/fish/config.fish"; do
        if [ -f "$rc" ] && grep -nF '.codegraph/bin' "$rc" 2>/dev/null; then
            FOUND=1
            say "  delete the line(s) above from $rc"
        fi
    done
    if [ "$FOUND" = "0" ]; then
        say "  $BIN_DIR is not referenced in common rc files — nothing to clean"
    fi
    exit 0
fi

# --- 1. OS and architecture ---------------------------------------------------
say "[1/6] detecting platform"
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
    Darwin) PLATFORM="apple-darwin" ;;
    Linux)  PLATFORM="unknown-linux-musl" ;;   # static musl: runs everywhere incl. RPM-based distros
    *) fail "unsupported OS: $OS (Windows: use install.ps1)" ;;
esac
case "$ARCH" in
    x86_64|amd64)   CPU="x86_64" ;;
    arm64|aarch64)  CPU="aarch64" ;;
    *) fail "unsupported architecture: $ARCH" ;;
esac
TARGET="$CPU-$PLATFORM"
ASSET="codegraph-$TARGET"
say "  $OS $ARCH -> target $TARGET (asset: $ASSET)"

# --- 2. Which release ----------------------------------------------------------
say "[2/6] resolving release"
if [ -n "$FROM_DIR" ]; then
    [ -d "$FROM_DIR" ] || fail "--from-dir: not a directory: $FROM_DIR"
    say "  offline mode: reading assets from $FROM_DIR"
elif [ -n "$PINNED_VERSION" ]; then
    TAG="v${PINNED_VERSION#v}"
    say "  pinned version -> tag $TAG"
else
    say "  resolving latest release from $BASE"
    LOCATION="$(curl -fsSI -o /dev/null -w '%{redirect_url}' "$BASE/releases/latest" 2>/dev/null)" \
        || LOCATION="$(wget --spider -S "$BASE/releases/latest" 2>&1 | sed -n 's/^ *Location: //p' | tail -1 | tr -d '\r')" \
        || LOCATION=""
    case "$LOCATION" in
        *tag/*) TAG="${LOCATION##*/tag/}" ;;
        *) fail "could not resolve latest release from $BASE — check your connection and that a release exists (https://github.com/BuildTeamRYS/codegraph/releases); --from-dir installs from a local folder" ;;
    esac
    TAG="$(printf '%s' "$TAG" | tr -d '\r')"
    say "  latest release: $TAG"
fi

if [ -n "$FROM_DIR" ]; then
    SRC_BIN="$FROM_DIR/$ASSET"
    SRC_SUMS="$FROM_DIR/checksums.txt"
    [ -f "$SRC_BIN" ] || fail "--from-dir: missing asset $SRC_BIN"
    say "  found binary: $SRC_BIN ($(file_size "$SRC_BIN") bytes)"
    if [ -f "$SRC_SUMS" ]; then
        say "  found checksums.txt — verifying"
    elif [ "$SKIP_CHECKSUM" = "1" ]; then
        say "  no checksums.txt present and --skip-checksum given — installing unverified"
    else
        fail "--from-dir: no checksums.txt in $FROM_DIR (pass --skip-checksum to install anyway)"
    fi
    TMPDIR_CG="$(mktemp -d 2>/dev/null || mktemp -d -t codegraph)" || fail "mktemp failed"
    trap 'rm -rf "$TMPDIR_CG"' EXIT INT TERM
    TMP_BIN="$TMPDIR_CG/$ASSET"
    cp "$SRC_BIN" "$TMP_BIN" || fail "cannot stage $SRC_BIN"
    if [ -f "$SRC_SUMS" ]; then
        cp "$SRC_SUMS" "$TMPDIR_CG/checksums.txt" || fail "cannot stage checksums.txt"
    fi
    say "[3/6] staged (offline) — no network used"
    DOWNLOAD_FROM="$SRC_BIN (offline)"
else
    DOWNLOAD="$BASE/releases/download/$TAG"
    TMPDIR_CG="$(mktemp -d 2>/dev/null || mktemp -d -t codegraph)" || fail "cannot create temp dir"
    case "$TMPDIR_CG" in
        "") fail "mktemp failed" ;;
    esac
    TMP_BIN="$TMPDIR_CG/$ASSET"
    TMP_SUMS="$TMPDIR_CG/checksums.txt"
    trap 'rm -rf "$TMPDIR_CG"' EXIT INT TERM

    say "[3/6] downloading"
    say "  $DOWNLOAD/$ASSET"
    T0="$(date +%s)"
    curl -fL --progress-bar "$DOWNLOAD/$ASSET" -o "$TMP_BIN" || wget --progress=bar:force "$DOWNLOAD/$ASSET" -O "$TMP_BIN" \
        || fail "download failed: $DOWNLOAD/$ASSET"
    T1="$(date +%s)"
    say "  downloaded $(file_size "$TMP_BIN") bytes in $((T1 - T0))s"
    say "  downloading checksums.txt"
    curl -fsSL "$DOWNLOAD/checksums.txt" -o "$TMP_SUMS" || wget -q "$DOWNLOAD/checksums.txt" -O "$TMP_SUMS" \
        || fail "download failed: $DOWNLOAD/checksums.txt"
    DOWNLOAD_FROM="$DOWNLOAD ($TAG)"
fi

# --- 4. Verify ------------------------------------------------------------------
say "[4/6] verifying SHA-256"
SRC_SUMS="${TMPDIR_CG}/checksums.txt"
EXPECTED="$(grep -E "(^|[[:space:]])\*?$(basename "$TMP_BIN")\$" "$SRC_SUMS" 2>/dev/null | awk '{print $1}' | head -1)"
if [ -z "$EXPECTED" ]; then
    if [ -n "$FROM_DIR" ] && [ "$SKIP_CHECKSUM" = "1" ]; then
        say "  no checksum entry — skipping verification (--skip-checksum)"
        EXPECTED=""
    else
        fail "checksums.txt has no entry for $ASSET — aborting, nothing was installed"
    fi
fi
if [ -n "$EXPECTED" ]; then
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
    say "  SHA-256 verified ($ACTUAL)"
fi

# --- 5. Install -------------------------------------------------------------------
say "[5/6] installing"
[ -d "$BIN_DIR" ] || mkdir -p "$BIN_DIR" || fail "cannot create $BIN_DIR"
cp "$TMP_BIN" "$BIN_PATH.new" || fail "cannot write $BIN_PATH.new"
chmod 0755 "$BIN_PATH.new" || fail "cannot chmod $BIN_PATH.new"
mv "$BIN_PATH.new" "$BIN_PATH" || fail "cannot move $BIN_PATH.new into place"
say "  installed $BIN_PATH (static musl/darwin — no system dependencies)"
say "  source: $DOWNLOAD_FROM"

# --- 6. PATH ---------------------------------------------------------------------
say "[6/6] PATH"
case ":$PATH:" in
    *":$BIN_DIR:"*) say "  $BIN_DIR is already on PATH" ;;
    *)
        RC=""
        case "${SHELL:-}" in
            */zsh)  RC="$HOME/.zshrc" ;;
            */bash) RC="$HOME/.bashrc" ;;
            */fish) RC="$HOME/.config/fish/config.fish" ;;
        esac
        if [ -t 0 ] && [ -n "$RC" ]; then
            printf 'add %s to PATH in %s now? [y/N] ' "$BIN_DIR" "$RC"
            read -r ANSWER || ANSWER=""
            case "$ANSWER" in
                y|Y|yes|YES)
                    if [ "$RC" = "$HOME/.config/fish/config.fish" ]; then
                        mkdir -p "$HOME/.config/fish" && printf '\nfish_add_path %s\n' "$BIN_DIR" >> "$RC"
                    else
                        printf '\nexport PATH="%s:$PATH"\n' "$BIN_DIR" >> "$RC"
                    fi
                    say "  added to PATH in $RC — restart your shell or: export PATH=\"$BIN_DIR:\$PATH\""
                    ;;
                *) say "  skipped. Add this to your shell rc by hand: export PATH=\"$BIN_DIR:\$PATH\"" ;;
            esac
        else
            say "  $BIN_DIR is not on PATH. Add this to your shell rc (e.g. ~/.zshrc):"
            say "    export PATH=\"$BIN_DIR:\$PATH\""
        fi
        ;;
esac
final_summary
