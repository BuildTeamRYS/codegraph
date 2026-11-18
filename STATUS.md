# STATUS

The agent rewrites this file at the end of every session so a fresh session can resume from it.

## Current phase

Phase 1 complete (acceptance checks pass). Phase 2 next: MCP server.

## What works

- Workspace with crates: `core` (types/hashing), `parse` (tree-sitter TS), `store` (SQLite + migrations), `cli` (`codegraph index|status`).
- Phase 0 acceptance: `codegraph status` prints `schema_version: 1` (verified against a live db).
- Phase 1 acceptance:
  - 3 fixtures with expected JSON (hand-verified against sources): `fixtures/tiny-api`, `fixtures/web-shop`, `fixtures/cli-tool` — fixture tests assert exact symbol+import match.
  - Re-indexing an unchanged repo parses 0 files (test + measured: 550 files re-checked in 0.007s).
  - 100k-LOC target: synthetic 98.9k LOC repo indexes fully in 0.64s (real Kapruka: 208 tracked files/53.8k LOC in <1s). Limit was 2 minutes. NOTE: 100k number is a synthetic generator, not a real repo; real-repo headroom verified only up to 54k LOC.
- Editing one file re-parses only that file; deleting a file removes its rows (integration tests).
- Broken files are indexed best-effort and reported as gaps (never silently complete).
- `.github/workflows/ci.yml`: fmt + clippy + test on push/PR.
- `.codegraph/` gitignored; index db defaults to `<repo>/.codegraph/index.db`.

## What is broken or incomplete

- `edges`/`facts`/`findings`/`verdicts`/`git_stats`/`cochange` tables exist (schema v1) but nothing writes them yet — by design (phases 4-8).
- Callers/callees are NOT implemented yet; that's phase 2.
- Gap detection found 2 best-effort files in the Kapruka repo (raw `&` in JSX text at compare-plans-accordion.tsx:33/42/59 area, credits-checkout.tsx similar). tree-sitter-tsx is strict here; flagged as gaps, indexed best-effort. May be grammar limitation, not user error — worth watching.
- Rowid-based `edges.src/dst` will dangle on file deletion until phase 3 re-computes edges (no edge writers exist yet, so harmless).

## Acceptance checks

| Phase | Name | Status |
|---|---|---|
| 0 | Skeleton | PASS (status prints schema_version, CI in place) |
| 1 | Parse and store | PASS (fixtures exact-match, 0-parse reindex, 100k LOC in 0.6s) |
| 2 | MCP server, basic tools | not started |
| 3 | Incremental updates | not started |
| 4 | Precise resolution | not started |
| 5 | Derived behavior facts | not started |
| 6 | Git and test enrichment | not started |
| 7 | Findings and verdicts | not started |
| 8 | Taint and security paths | not started |
| 9 | Semantic search and summaries | not started |
| 10 | Evaluation | not started |

## Dogfooding notes (from phase 3 onward)

Where codegraph failed or wasted tokens when used on its own repo:

- None yet (dogfooding starts in phase 3).

## Next step

Phase 2: MCP server over stdio with `find_symbol`, `outline`, `callers`/`callees` (syntactic, `inferred` confidence), `get_context(symbol, budget_tokens)`. Wire it into the agent's MCP config and capture a transcript of `find_symbol`/`get_context` on this repo. Budget enforcement via a token counter test; p95 latency target <100ms on a 100k-LOC index.

## Open questions for the owner

- Phase 2 MCP tool-budget default: PLAN says `budget_tokens` default 800 — confirm or change.
- Python support (second language) — after phase 2 or after this session's phases?
