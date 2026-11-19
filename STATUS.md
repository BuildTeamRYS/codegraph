# STATUS

The agent rewrites this file at the end of every session so a fresh session can resume from it.

## Current phase

Phases 0, 1, 2 code-complete and green (see below); **Phase 0.5 (distribution) implemented and locally verified; pending proof on clean machines, which needs a GitHub remote + release tag push**. Next: Phase 3 (incremental updates) — dogfooding starts there.

Note: the working-tree edit that reset this file (2026-11-19) discarded the previous status log; the real history is in git (`git log`, and `git show HEAD:STATUS.md` before the reset). This file was rewritten 2026-11-19 to reflect reality.

## What works

- Phase 0: cargo workspace, CI (fmt+clippy+test on ubuntu/macos/windows; test matrix + install-script harness).
- Phase 1: TypeScript/JavaScript plugin behind the `LanguagePlugin` trait (`crates/parse` = trait + walker, `crates/lang-ts` = TS/JS plugin). `.ts/.tsx/.mts/.cts/.js/.jsx/.mjs/.cjs`. Fixtures: `tiny-api`, `web-shop`, `cli-tool`, `js-tool` (hand-verified expected JSON), `crlf-repo` (CRLF pinned via `.gitattributes`). No-op re-index = 0 parses. Synthetic 100k-LOC TS repo (3449 files, 17245 symbols): full index **~1.0s**, no-op re-index **0.05s** (acceptance: <2 min). Kapruka (53.8k LOC real repo) was <1s in an earlier session. Schema TS-scan test passes. **All Phase 1 acceptance checks pass.**
- Phase 2: MCP server (rmcp, stdio) with find_symbol/outline/callers/callees/get_context; budgeted output; p95 ≈0.6ms on synthetic 100k-LOC; MCP e2e test. **`codegraph init` added this session**: detects Claude Code/Cursor/OpenCode by filesystem markers, prints merged config, applies only after confirmation (`--yes`), `--dry-run`, `--global`, `--agent`, merges (never clobbers) existing configs, `.codegraph.bak` backups, idempotent; 9 integration tests.
- Phase 0.5: `codegraph update` (ureq+rustls, SHA-256 verify, atomic replace, tamper abort — integration-tested against a local fake HTTP server). `install/install.sh` (OS/arch detect, pinned/latest, checksum-verified, `~/.codegraph/bin`, PATH prompt on tty, `--uninstall`) + `install/test-install.sh` (**7/7 pass**: happy path, tamper abort, missing checksum, spaces-in-HOME, uninstall, bad flag) + full piped one-liner install verified end-to-end against a local fake release server. `install/install.ps1` written (Get-FileHash verify, User PATH opt-in, `-Uninstall`, `-Version`, `-ReleaseBase`) — **not yet executed anywhere** (no Windows/pwsh on this machine). `.github/workflows/release.yml`: tag `v*` → 6 targets (linux x64/arm musl, macOS x64/arm64 ad-hoc signed, Windows x64/arm64) → `checksums.txt` → GitHub Release; dynamic-dep checks (ldd allowlist, otool, PE byte-scan for CRT). CI now tests on ubuntu+macos+windows.

## What is broken or incomplete

- **Phase 0.5 cannot be fully "done" without a GitHub remote**: pushing a `v*` tag is what exercises the release workflow, the real one-line installs, Gatekeeper behavior on macOS, and the Windows installer. Nothing in the release pipeline has run on GitHub yet.
- `install/install.ps1` untested (no Windows here); CI only covers install.sh.
- `.github/workflows/release.yml` untested end-to-end; `windows-11-arm` and `ubuntu-24.04-arm` runner labels assumed correct (GA in 2026); musl-tools apt install assumed available.
- CommonJS `require()` calls are recorded as call sites but **not** mapped to import bindings (CJS interop resolution deferred; ESM named/default/namespace imports resolve). Recorded as known gap.
- DECISIONS.md log was truncated by the owner's 2026-11-19 working-tree edit; prior rationale entries (schema v2 `line` column, imports-table decision, edge `valid_hash`, etc.) are recoverable via git. Not restored — see open questions.
- p95 verified on synthetic index only; no transcript yet of a real agent driving `codegraph init` + MCP end-to-end on this repo (owner to wire it, or phase 3 dogfooding).

## Acceptance checks

| Phase | Name | Status |
|---|---|---|
| 0 | Skeleton | PASS |
| 0.5 | Distribution (CI builds, install scripts) | PARTIAL — implemented + locally verified (install.sh 7/7, update e2e, fake-release one-liner e2e); clean-machine/release-run proof pending GitHub remote + tag |
| 1 | Parse and store | PASS (all listed checks incl. CRLF + schema scan + 100k-LOC perf on synthetic; Kapruka re-check pending) |
| 2 | MCP server, basic tools | PASS (+ `codegraph init` acceptance added and tested) |
| 3 | Incremental updates | not started |
| 4 | Precise resolution | not started |
| 5 | Derived behavior facts | not started |
| 6 | Git and test enrichment | not started |
| 7 | Findings and verdicts | not started |
| 8 | Taint and security paths | not started |
| 9 | Semantic search and summaries | not started |
| 10 | Evaluation | not started |
| 11 | Language expansion and framework plugins | not started |
| 12 | Signing and enterprise distribution (only when needed) | not started |

## Dogfooding notes (from phase 3 onward)

Where codegraph failed or wasted tokens when used on its own repo:

- None yet. (This repo is Rust; this index only sees fixtures + any TS/JS files — the honest dogfooding path for the Rust repo itself is phase 3+ with Kapruka or after Python support lands.)

## Next step

1. Phase 3: file watcher + `git diff` incremental re-index; property test (incremental == full re-index); `codegraph sync`; start dogfooding; bench/ skeleton.
2. Owner actions needed to finish Phase 0.5: create the GitHub remote, push `main`, tag `v0.2.0`; then run the release workflow and test install one-liners on clean Windows/macOS/Linux machines.

## Open questions for the owner

1. **GitHub remote**: repo is local-only. Creating it + pushing + tagging `v0.2.0` is required to complete Phase 0.5 acceptance (release CI, one-line install, Gatekeeper check). Want me to prepare it? I won't push without your go-ahead.
2. **DECISIONS.md truncation**: your 2026-11-19 edit dropped the detailed 2026-11-18 log. Want it restored (appended back under a "recovered" section), or intentional curation? It's recoverable from git either way.
3. **Kapruka 100k-LOC re-check**: may I re-run the perf smoke test against Kapruka (read-only, index into /tmp only) to confirm the Phase 1 "<2 min" acceptance on a real repo?
4. Release base URL defaults to `https://github.com/ravijaanthony/codegraph` (inferred from LICENSE handle). Correct?
5. LICENSE copyright line still says "ravijaanthony" (git handle, not legal name) — confirm or correct before any public release.
