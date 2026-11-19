#!/bin/sh
# Local test harness for install/install.sh. Runs against a file://-backed
# fake release root; no network, no writes outside temp dirs.
#
#   sh install/test-install.sh
#
# Covers: happy-path install, checksum-tamper abort, missing checksum
# entry, install into a $HOME containing spaces, uninstall, --help.

set -eu

HERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
INSTALLER="$HERE/install.sh"
REPO_ROOT="$(CDPATH= cd -- "$HERE/.." && pwd)"

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); printf 'PASS %s\n' "$1"; }
bad()  { FAIL=$((FAIL + 1)); printf 'FAIL %s\n' "$1"; }

# --- fake release root ---------------------------------------------------------
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM

ASSET="codegraph-x86_64-unknown-linux-musl"
REAL_BINARY="${CODEGRAPH_TEST_BINARY:-}"
if [ -z "$REAL_BINARY" ]; then
    for candidate in "$REPO_ROOT/target/release/codegraph" "$REPO_ROOT/target/debug/codegraph"; do
        [ -x "$candidate" ] && REAL_BINARY="$candidate" && break
    done
fi
[ -n "$REAL_BINARY" ] || { echo "no codegraph binary found; run cargo build first" >&2; exit 2; }

REL="$WORK/repo"
mkdir -p "$REL/releases/download/v0.2.0"
cp "$REAL_BINARY" "$REL/releases/download/v0.2.0/$ASSET"
( cd "$REL/releases/download/v0.2.0" && {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$ASSET" > checksums.txt;
    else shasum -a 256 "$ASSET" > checksums.txt; fi
} )

export CODEGRAPH_RELEASE_BASE="file://$REL"
FILE_BASE_UNSUPPORTED=0

run_installer() { # env-setup via leading env on call site
    sh "$INSTALLER" "$@"
}

# --- 1. happy path -------------------------------------------------------------
T="$WORK/t1"; mkdir -p "$T"
OUT="$(CODEGRAPH_INSTALL_DIR="$T/cg" sh "$INSTALLER" --version 0.2.0 2>&1)" \
    && BIN_OK=1 || BIN_OK=0
if [ "$BIN_OK" = 1 ] && [ -x "$T/cg/bin/codegraph" ] && "$T/cg/bin/codegraph" --version >/dev/null 2>&1; then
    ok "happy-path install and binary runs"
else
    bad "happy-path install: $OUT"
fi
echo "$OUT" | grep -q "SHA-256 verified" && ok "checksum verification reported" || bad "checksum verification not reported"

# --- 2. tampered binary aborts, nothing installed ------------------------------
printf 'tampered' >> "$REL/releases/download/v0.2.0/$ASSET"
T="$WORK/t2"; mkdir -p "$T"
if CODEGRAPH_INSTALL_DIR="$T/cg" sh "$INSTALLER" --version 0.2.0 >/dev/null 2>&1; then
    bad "tampered checksum was NOT rejected"
else
    [ ! -e "$T/cg/bin/codegraph" ] && ok "tampered checksum aborts; nothing installed" \
        || bad "tampered install left a binary behind"
fi
# restore the good binary + sums
cp "$REAL_BINARY" "$REL/releases/download/v0.2.0/$ASSET"
( cd "$REL/releases/download/v0.2.0" && {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$ASSET" > checksums.txt;
    else shasum -a 256 "$ASSET" > checksums.txt; fi
} )

# --- 3. checksums.txt without the asset aborts ----------------------------------
cp "$REL/releases/download/v0.2.0/checksums.txt" "$REL/releases/download/v0.2.0/checksums.bak"
echo "deadbeef  some-other-asset" > "$REL/releases/download/v0.2.0/checksums.txt"
T="$WORK/t3"; mkdir -p "$T"
if CODEGRAPH_INSTALL_DIR="$T/cg" sh "$INSTALLER" --version 0.2.0 >/dev/null 2>&1; then
    bad "missing checksum entry was NOT rejected"
else
    [ ! -e "$T/cg/bin/codegraph" ] && ok "missing checksum entry aborts" || bad "missing checksum left a binary behind"
fi
mv "$REL/releases/download/v0.2.0/checksums.bak" "$REL/releases/download/v0.2.0/checksums.txt"

# --- 4. HOME containing spaces --------------------------------------------------
SPACED="$WORK/home with spaces"; mkdir -p "$SPACED"
T="$WORK/t4"
if HOME="$SPACED" env -u CODEGRAPH_INSTALL_DIR sh "$INSTALLER" --version 0.2.0 >/dev/null 2>&1; then
    if [ -x "$SPACED/.codegraph/bin/codegraph" ]; then
        ok "installs under a home directory with spaces"
    else
        bad "spaces-home: binary not found at expected path"
    fi
else
    bad "spaces-home: installer failed"
fi

# --- 5. uninstall -----------------------------------------------------------------
SPACED_HOME="$WORK/home with spaces"
if HOME="$SPACED_HOME" env -u CODEGRAPH_INSTALL_DIR sh "$INSTALLER" --uninstall >/dev/null 2>&1; then
    [ ! -d "$SPACED_HOME/.codegraph" ] && ok "uninstall removes ~/.codegraph" || bad "uninstall left ~/.codegraph behind"
else
    bad "uninstall failed"
fi

# --- 6. unknown flag ----------------------------------------------------------------
if sh "$INSTALLER" --bogus-flag >/dev/null 2>&1; then
    bad "unknown flag accepted"
else
    ok "unknown flag rejected"
fi

# --- 7. --from-dir installs from a local bundle ------------------------------------
BUNDLE="$WORK/bundle"; mkdir -p "$BUNDLE"
cp "$REAL_BINARY" "$BUNDLE/$ASSET"
( cd "$BUNDLE" && {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$ASSET" > checksums.txt;
    else shasum -a 256 "$ASSET" > checksums.txt; fi
} )
T="$WORK/t7"; mkdir -p "$T"
OUT="$(CODEGRAPH_INSTALL_DIR="$T/cg" sh "$INSTALLER" --from-dir "$BUNDLE" 2>&1)" \
    && FROM_DIR_OK=1 || FROM_DIR_OK=0
if [ "$FROM_DIR_OK" = 1 ] && [ -x "$T/cg/bin/codegraph" ] && "$T/cg/bin/codegraph" --version >/dev/null 2>&1; then
    ok "--from-dir bundle install works"
    echo "$OUT" | grep -q "offline mode" && ok "--from-dir reports offline mode" || bad "--from-dir offline mode not reported"
    echo "$OUT" | grep -q "SHA-256 verified" && ok "--from-dir verifies checksums.txt" || bad "--from-dir did not verify checksums.txt"
else
    bad "--from-dir bundle install: $OUT"
fi

# --- 7b. --from-dir with tampered checksum aborts -----------------------------------
printf 'tampered' >> "$BUNDLE/$ASSET"
T="$WORK/t7b"; mkdir -p "$T"
if CODEGRAPH_INSTALL_DIR="$T/cg" sh "$INSTALLER" --from-dir "$BUNDLE" >/dev/null 2>&1; then
    bad "--from-dir tampered checksum NOT rejected"
else
    [ ! -e "$T/cg/bin/codegraph" ] && ok "--from-dir tampered bundle aborts" || bad "--from-dir tampered install left a binary"
fi
cp "$REAL_BINARY" "$BUNDLE/$ASSET"
( cd "$BUNDLE" && {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$ASSET" > checksums.txt;
    else shasum -a 256 "$ASSET" > checksums.txt; fi
} )

# --- 7c. --from-dir without checksums.txt aborts unless --skip-checksum ------------
mv "$BUNDLE/checksums.txt" "$BUNDLE/checksums.bak"
T="$WORK/t7c"; mkdir -p "$T"
if CODEGRAPH_INSTALL_DIR="$T/cg" sh "$INSTALLER" --from-dir "$BUNDLE" >/dev/null 2>&1; then
    bad "--from-dir without checksums.txt accepted"
else
    ok "--from-dir without checksums.txt aborts"
fi
if CODEGRAPH_INSTALL_DIR="$T/cg2" sh "$INSTALLER" --from-dir "$BUNDLE" --skip-checksum >/dev/null 2>&1; then
    ok "--from-dir --skip-checksum installs"
else
    bad "--from-dir --skip-checksum failed"
fi

printf '\ninstall.sh tests: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
