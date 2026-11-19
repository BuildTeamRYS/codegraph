# STATUS

The agent rewrites this file at the end of every session so a fresh session can resume from it.

Note: the 2026-11-19 owner edit of the planning docs reset this file to "nothing built"; the code on disk (and git history before the reset) showed phases 0, 1, 2 as complete. This file now reflects reality again, and keeps the new phase structure (2.5 / 2.6 / 13).

## Current phase

Phase 2.5 (universal agent support) — implemented and green; real-install verification done for OpenCode and Claude Code; the rest of Tier 1 needs a real install each. Next: Phase 2.6 (observability: query log, status/stats/doctor polish, generated per-agent docs) or Phase 3 (incremental updates). Phase 0.5 remains blocked on a GitHub remote + release tag.

## What works

- Phase 0: cargo workspace, CI (fmt+clippy+test on ubuntu/macos/windows; test matrix + install-script harness).
- Phase 1: TypeScript/JavaScript plugin behind the `LanguagePlugin` trait (`crates/parse` = trait + walker, `crates/lang-ts` = TS/JS plugin). `.ts/.tsx/.mts/.cts/.js/.jsx/.mjs/.cjs`. Fixtures: `tiny-api`, `web-shop`, `cli-tool`, `js-tool` (hand-verified expected JSON), `crlf-repo` (CRLF pinned via `.gitattributes`). No-op re-index = 0 parses. Synthetic 100k-LOC TS repo: full index ~1.0s. Schema TS-scan test passes. **Phase 1 acceptance passes.**
- Phase 2: MCP server (rmcp, stdio) with find_symbol/outline/callers/callees/get_context/status; budgeted output; MCP e2e test. **Phase 2 acceptance passes.** p95 ≈0.6ms on synthetic index.
- Phase 0.5 (partial): `codegraph update` (SHA-256 verify, atomic replace, tamper abort, integration-tested vs fake HTTP server). `install/install.sh` (+ 7/7 harness pass) and `install.ps1` (untested — no Windows here). `.github/workflows/release.yml` written, never run. Blocked on: GitHub remote + `v*` tag + clean machines.
- Phase 2.5 (new): `crates/agents` — data-driven Tier 1 adapter table, all 9 agents, config paths/formats re-verified against official docs on 2026-11-19. Surgical JSON/JSONC/TOML editing (comments, formatting, key order, other servers survive); backup-once + atomic write; byte-for-byte restore on `agents remove`; `.codegraph.bak` never overwritten; TOCTOU guard re-checks file before writing. Instruction block (`<!-- codegraph:start -->`) written idempotently into AGENTS.md/CLAUDE.md/GEMINI.md. `codegraph init` indexes the repo, detects agents (PATH + markers), prints diffs, applies only with confirmation (`--yes`), `--dry-run`, `--scope user|project`, `--agent` (repeatable), creates the stable shim at `~/.codegraph/bin/codegraph`. New CLI: `agents list|remove|doctor`, `mcp-config --format json|toml|yaml`, `query <tool>` (identical output to MCP). MCP: `readOnlyHint` on all 6 tools, new `status` tool (role/indexing/freshness/counts), `.codegraph/serve.lock` elects one updater per repo (others read-only), first index runs in the background. Conformance tests in CI (3 OSes): handshake, tool list, every tool call, malformed input survives, stdout carries only JSON-RPC frames, exactly one updater among 3 concurrent serves, background first-index works.
- Real installs (this machine, 2026-11-19): OpenCode v2.0.27 — config points at shim, `opencode mcp list` → `✓ codegraph connected`, `codegraph doctor --agent opencode` all PASS. Claude Code 2.1.201 — `claude mcp list` finds codegraph in `.mcp.json`, reports `⏸ Pending approval` (expected for project scope until interactive approval).

## What is broken or incomplete

- Phase 0.5 cannot finish without a GitHub remote: pushing `main` + tagging `v0.2.0` is required to exercise release CI, real one-line installs, Gatekeeper and the Windows installer. Owner must decide (will ask).
- `install/install.ps1` untested (no Windows/pwsh here); runner labels `windows-11-arm` / `ubuntu-24.04-arm` assumed correct.
- CommonJS `require()` calls are recorded as call sites but not mapped to import bindings (CJS interop deferred; ESM named/default/namespace resolve).
- MCP `roots` NOT used: deprecated in protocol 2026-07-28 (SEP-2577). Repo resolution = `--project` > `$CODEGRAPH_PROJECT` > walk up from cwd to nearest `.git`.
- `init` does not shell out to agent-native CLIs (`claude mcp add` etc.); configs are edited surgically instead (see DECISIONS.md).
- Doctor's handshake spawns each configured agent's server; on a slow machine the 5s timeout may false-FAIL (set up is otherwise fine).
- Tier 1 verification status: OpenCode and Claude Code verified on a real install here; Cursor, VS Code Copilot, Codex CLI, Gemini CLI, Antigravity, Windsurf, Claude Desktop remain unverified end-to-end (config formats and paths verified against official docs only). See COMPAT.md.
- README's per-agent guide links (`docs/agents/*.md`) do not exist yet — generation lands in Phase 2.6; README now says so.

## Acceptance checks

| Phase | Name | Status |
|---|---|---|
| 0 | Skeleton | PASS |
| 0.5 | Distribution (CI builds, install scripts) | PARTIAL — implemented + locally verified; clean-machine/release-run proof pending GitHub remote + tag |
| 1 | Parse and store | PASS |
| 2 | MCP server, basic tools | PASS |
| 2.5 | Universal agent support | PASS for the parts that need no foreign machines: conformance suite green, merge/restore/idempotency tests, doctor self-check, OpenCode + Claude Code verified on real installs. Tier 1 real-install checks for the other 7 agents pending (need installs; COMPAT.md has per-agent verification log) |
| 2.6 | Observability, setup guidance, generated docs | not started |
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
| 13 | Local UI (only after Phase 10 is positive) | not started |

## Dogfooding notes (from phase 3 onward)

Where codegraph failed or wasted tokens when used on its own repo:

- The repo is Rust; the TS/JS index only sees `fixtures/` (15 files). Dogfooding this repo for real starts with Phase 3+ or Python support.
- `status` tool output on this repo: `index@db595ea2bb79 fresh` after re-index; files=15 (fixtures), symbols=26.
- The new `codegraph query` CLI works on this repo (find_symbol login → fixtures/tiny-api/src/login.ts:9-12).

## Next step

1. Phase 2.6: query log, `status`/`stats`/`doctor` polish, post-install guidance, generated per-agent docs + README table from the adapter table, architecture.mmd/SVG sync (CI `mmdc` drift check).
2. Or Phase 3 first (file watcher + git-diff incremental re-index + property test; dogfooding proper). The plan's task list does 2.6 first (task 4) then 3.
3. Owner action to finish Phase 0.5: GitHub remote + tag `v0.2.0`; then install one-liners on clean Windows/macOS/Linux machines.

## Open questions for the owner

1. GitHub remote: repo is local-only. Create it + push + tag `v0.2.0` to finish Phase 0.5? (Will not push without a go-ahead.)
2. `.mcp.json`, `AGENTS.md`, `CLAUDE.md` created in this repo by the 2026-11-19 verification runs (OpenCode + Claude Code). Keep or remove? Keeping is consistent with `opencode.json` (already tracked).
3. Release base URL defaults to `https://github.com/ravijaanthony/codegraph` — correct?
4. LICENSE copyright line still says "ravijaanthony" (git handle) — confirm or correct before any public release.
