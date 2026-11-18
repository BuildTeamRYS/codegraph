# DECISIONS

One line per non-obvious choice, with the reason. Append only; never rewrite history.
If a decision is reversed, add a new line that says so and links the old one.

Format: `YYYY-MM-DD | area | decision | reason`

## Seed decisions (from PLAN.md)

- Start | language | Rust for indexer, store and server | Single static binary, fast, memory-safe, first-class tree-sitter support
- Start | store | SQLite with integer IDs and an interned string table | Portable single file, fast indexed queries, no custom binary format needed
- Start | parsing | tree-sitter first, SCIP/LSP in phase 4 | Tree-sitter gives syntax quickly; precise resolution comes later
- Start | invariant | Facts, findings and verdicts are valid only while valid_hash and dep_fingerprint match | A stale "clean" verdict must never mislead an agent
- Start | output | Dense terse text with confidence tags and file:line ranges | Models read tokens, not bytes; agents need a cheap way to verify

## Thresholds to fill in as phases land

- Phase 4 call-graph precision threshold: TBD
- Phase 4 call-graph recall threshold: TBD
- Phase 9 retrieval recall@5 threshold: TBD

## Log

<!-- Agent: add new decisions below this line. -->
- 2026-11-18 | language | TypeScript ships first (.ts/.tsx/.mts/.cts), Python next | Owner decision; Kapruka (53.8k LOC) is the standing real-repo testbed
- 2026-11-18 | repo layout | The RSVP directory is the codegraph repo root; PLAN.md/DECISIONS.md/STATUS.md already live here | Nothing sensible to move; crates/ and fixtures/ were added alongside
- 2026-11-18 | store | Index lives at `<repo>/.codegraph/index.db` by default, `--db` overrides | Keeps the target repo clean; the directory is gitignored
- 2026-11-18 | schema | edges and findings carry `valid_hash` (PLAN §3 lists edges without one) | Rule 5 wins over the column list: every edge/finding needs the content hash it was computed against
- 2026-11-18 | schema | Imports go into a dedicated `imports` table, not `edges` | Import sources don't resolve to symbol row ids until phase 4; faking dst=0 would poison the graph
- 2026-11-18 | schema | Migrations are code-applied, append-only, transactional, keyed off `meta.schema_version` | Simple, boring, rolls back cleanly on failure
- 2026-11-18 | parse | `const x = () => ...` / `const x = function ...` indexed as `function` symbols | Idiomatic TS modules bind their exported handlers this way; skipping them would hide half the JS/TS surface
- 2026-11-18 | parse | Symbol content_hash covers doc comment + declaration text | Docstring-only edits must still invalidate derived facts (rule 5)
- 2026-11-18 | parse | `.d.ts` is flagged `is_generated`, not excluded | Declaration files carry real type info; flagging preserves signal without polluting rankings later
- 2026-11-18 | parse | Files with tree-sitter ERROR nodes are indexed best-effort AND recorded as gaps | Never return partial results as if complete; status surfaces the gaps
- 2026-11-18 | walk | Hidden files are walked (`.git` is always excluded by the ignore crate); `.gitignore` respected | Hidden TS config (`.eslintrc.ts` etc.) is still source
- 2026-11-18 | walk | vendor dirs (node_modules/vendor/third_party/dist/build/out) are flagged `is_vendor`, still indexed | PLAN says flag, not skip
- 2026-11-18 | tests | `expected.symbols.json` excludes `content_hash` | Hashes can't be hand-verified; covered by dedicated hash tests instead
- 2026-11-18 | timestamps | meta timestamps are unix seconds; no chrono dependency yet | Phase 0 needs opaque freshness markers only
- 2026-11-18 | mcp | Stdio MCP is the universal transport; one binary serves Claude Code/Desktop, Cursor, Windsurf, Zed, Continue and OpenCode — no per-client code | Avoids N client integrations; MCP config is per-client by design
- 2026-11-18 | mcp | rmcp 3.11 SDK (owner choice); hand-rolled JSON-RPC fallback not needed | SDK works, has server+client; protocol kept to tools only
- 2026-11-18 | mcp | MCP tools return dense text (not JSON), errors as helpful zero-match text inside the tool result | Agents read text best; protocol stays simple
- 2026-11-18 | query | Token budget estimate = ceil(bytes/4), greedy fill from ranked lines, overflow prints "N more available, narrow by X" | Boring, adequate, testable; real tokenizer added only if agents complain
- 2026-11-18 | query | Budget default 800 (owner) | PLAN default, owner confirmed
- 2026-11-18 | store | edges carry a `line` column (v2 migration) | Agents need the call-site location, not just the callee's range
- 2026-11-18 | parse | One import record per binding (named/default/namespace/side-effect/reexport) with local_name→imported_name map | Required to resolve named-import calls across modules
- 2026-11-18 | resolve | Call graph is syntactic, full re-resolution after every index pass; incremental edge recompute deferred to phase 3 | Correctness first; 100k-LOC full resolve is already <1s
- 2026-11-18 | resolve | Resolution order: `this.x` enclosing class → namespace/named import into resolved module → unique same-file → globally unique name; everything else `unresolved` | Conservative wins; never guess (dynamic dispatch stays unlabeled)
- 2026-11-18 | resolve | Member calls through local variables (`cart.add`) and nested member exprs (`this.items.get`) stay unresolved until phase 4 SCIP | No type tracking; guessing would violate "never invent facts"
- 2026-11-18 | resolve | Calls at module top level are counted (calls_unattributed) but create no edge | edges.src is NOT NULL; no module-level symbol exists to anchor
- 2026-11-18 | resolve | Re-exports (`export ... from`) recorded as imports kind=reexport, never used for resolution | Re-export graphs resolve transitively at phase 4 with SCIP
- 2026-11-18 | bench | p95 measured in-process on the synthetic 100k-LOC index: find_symbol 0.59ms, outline 0.13ms, get_context 0.11ms, callees 0.10ms | Far under the 100ms target; subprocess e2e overhead excluded as unrepresentative
- 2026-11-18 | config | opencode.json (project) registers the release binary as `codegraph` project-locally with `serve .` | Project-local keeps machine-specific paths out of global config; release path noted, rebuild on schema/query changes
