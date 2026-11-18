# STATUS

The agent rewrites this file at the end of every session so a fresh session can resume from it.

## Current phase

Phase 2 complete (acceptance checks pass). Phase 3 next: incremental updates.
`opencode.json` at repo root registers `codegraph` as an MCP server in this project;
the `codegraph_*` tools are live in the OpenCode session that built this phase.

## What works

- Phase 0+1 as before (see git log).
- Phase 2:
  - `codegraph-query`: dense, terse output; `find_symbol`, `outline`, `callers`/`callees`,
    `get_context`; every response size-capped with "N more available, narrow by X" hint;
    header `index@<commit> fresh|stale|unknown`; escape hatch `escape: path:start-end` footer;
    per-edge `[inferred]` confidence tags.
  - Syntactic call graph resolved at index time into `edges(kind='calls')` from raw
    `call_sites` + import bindings (`this.x` → enclosing class; named imports into resolved
    module; unique same-file; unique global; else unresolved — never guessed).
    Fixture calls: `login → hash_pw/verify_pw`, `verify_pw → hash_pw`, `App → Cart (new)`,
    `App → Cart.add/Cart.total`. Local-variable member calls stay unresolved (no type info).
  - `codegraph-mcp`: stdio MCP server (rmcp 3.11), 5 tools, dense text results.
  - `codegraph serve <path> [--db]` subcommand.
  - Phase 2 acceptance: **transcript captured in this session** — all 5 tools called live
    over MCP from the OpenCode agent (find_symbol/outline/callers/callees/get_context on
    the repo index); **budget test** (13 query tests incl. truncation hint) pass;
    **p95 latency** on synthetic 100k-LOC index: find_symbol 0.59ms, outline 0.13ms,
    get_context 0.11ms, callees 0.10ms (target <100ms).
  - MCP e2e test (crates/cli/tests/mcp.rs): initialize → tools/list → tools/call over stdio.

## What is broken or incomplete

- RSVP is a Rust repo, so dogfooding via this index sees only the fixture `.ts` files;
  real agent value on THIS repo starts when codegraph itself is TypeScript (out of scope)
  or when Python support lands. Indexing another TS repo (e.g. Kapruka with `--db /tmp/...`)
  is the interim dogfooding path.
- `edges` still rebuilt from scratch on every index pass (fine ≤100k LOC; phase 3 makes it incremental).
- Perf verified only on synthetic 100k LOC (real Kapruka: 53.8k LOC, <1s).

## Acceptance checks

| Phase | Name | Status |
|---|---|---|
| 0 | Skeleton | PASS |
| 1 | Parse and store | PASS |
| 2 | MCP server, basic tools | PASS |
| 3 | Incremental updates | not started |
| 4 | Precise resolution | not started |
| 5 | Derived behavior facts | not started |
| 6 | Git and test enrichment | not started |
| 7 | Findings and verdicts | not started |
| 8 | Taint and security paths | not started |
| 9 | Semantic search and summaries | not started |
| 10 | Evaluation | not started |

## Housekeeping

- LICENSE: PolyForm Shield 1.0.0 (source-available, non-competing). Copyright holder currently "ravijaanthony" — owner must confirm full legal name/company before release.
- README.md: minimal dev-facing; OpenCode MCP config marked verified, Claude Code marked untested.

## Dogfooding notes (from phase 3 onward)

Where codegraph failed or wasted tokens when used on its own repo:

- 2026-11-18 (phase 2, pre-dogfood): repo index covers only fixtures (Rust codebase).
  No failures observed in tool correctness; results were accurate and terse.

## Next step

Phase 3: file watcher + `git diff`-driven re-index. Only changed files re-parsed (already true via
content hash); incremental re-resolution of edges touching changed symbols (currently full pass);
server compares stored `indexed_commit` vs HEAD and reports `stale` / self-refreshes before answering.
Property test: random edit sequences; incremental index equals full re-index.
Then start dogfooding properly.

## Open questions for the owner

- Phase 4 thresholds to lock in DECISIONS.md: precision ≥ 0.95 / recall ≥ 0.80 on fixtures — confirm?
- Phase 10 benchmark tasks: ok to mine ~20 candidates from your repos next (git log fix/revert commits)?
