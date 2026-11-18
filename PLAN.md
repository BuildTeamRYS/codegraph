# CodeGraph: Agent Build Plan

Give this whole file to your coding agent. Put it in the repo root (as `PLAN.md`, or reference it from `CLAUDE.md`/`AGENTS.md`) so every session starts with it.

---

## 1. Master prompt (paste this first)

```
You are building "codegraph": a local, machine-oriented code knowledge base that
coding agents query through an MCP server so they read the right 30 lines instead
of 30 files.

Read PLAN.md fully before writing code. Work ONE phase at a time, in order.

Rules:
1. Do not start phase N+1 until phase N's acceptance checks pass. Run them and show output.
2. Write tests first or alongside. No phase is done without passing tests.
3. Keep a living file DECISIONS.md: one line per non-obvious choice, with the reason.
4. Keep a living file STATUS.md: current phase, what works, what is broken, next step.
   Update it at the end of every session so a fresh session can resume from it.
5. Never invent facts in the index. Every edge/finding carries: source (which
   tool produced it), confidence (certain | inferred | unknown), and the
   content hash it was computed against.
6. Every query result must be size-capped and end with exact file:line ranges.
7. Prefer boring, proven tech: Rust, tree-sitter, SQLite. No custom binary format.
8. From phase 3 onward, index THIS repo with codegraph and use it to navigate
   the repo yourself (dogfooding). Note where it fails you in STATUS.md.
9. If a requirement is ambiguous, pick the simplest option, record it in
   DECISIONS.md, and continue. Ask me only for decisions that are expensive to reverse.
```

---

## 2. Project definition

**Goal:** an indexer plus query server that runs on the developer's laptop, fully offline, and gives coding agents a compact, accurate, token-budgeted model of a codebase: structure, relationships, behavior facts, findings, and verdicts.

**Not goals:** a human UI, replacing code reading entirely, cloud hosting.

**Stack:**
- Rust (indexer, store, server), single static binary
- tree-sitter for parsing
- SCIP indexers or LSP for precise symbol resolution (phase 4)
- SQLite (via `rusqlite`) as the store; zstd for large blobs
- MCP server over stdio (use the official Rust MCP SDK if available, else implement JSON-RPC directly)
- Start with ONE language (Python or TypeScript; pick whichever the owner uses most)

**Repo layout:**
```
codegraph/
  crates/
    core/        # schema, ids, hashing, confidence types
    parse/       # tree-sitter extraction
    resolve/     # SCIP/LSP ingestion
    analyze/     # derived facts + external analyzer adapters
    store/       # SQLite access, migrations, invalidation
    query/       # ranking, budgeting, output formatting
    mcp/         # MCP server
    cli/         # `codegraph index|serve|status|query`
  fixtures/      # small sample repos with KNOWN answers (used by tests)
  bench/         # evaluation tasks and harness
  PLAN.md  DECISIONS.md  STATUS.md
```

---

## 3. Core data model (build in phase 1, extend later)

Use integer IDs everywhere, with a string-intern table. Store the commit hash an index was built against.

**Tables (minimum):**
- `files(id, path, lang, content_hash, size, indexed_at, is_generated, is_vendor)`
- `symbols(id, file_id, kind, name, qualified_name, sig, start_line, end_line, content_hash)`
- `edges(src, dst, kind, confidence, source_tool)` where kind is one of
  `calls | imports | defines | inherits | reads | writes | tested_by | handles_route | depends_on`
- `facts(symbol_id, kind, value, confidence, source_tool, valid_hash)` for derived behavior
  (e.g. `db_write`, `network_call`, `raises:TimeoutError`, `reads_env:API_KEY`)
- `findings(id, symbol_id, kind, severity, confidence, source_tool, rule_id, line_start, line_end, status, first_seen, last_seen)`
  where status is `open | fixed | suppressed | regressed`
- `verdicts(symbol_id, check_set, result, by_tool, valid_hash, dep_fingerprint, at)`
- `git_stats(symbol_id or file_id, commits_90d, authors, last_change, bugfix_commits)`
- `cochange(file_a, file_b, score)`
- `meta(key, value)` including indexed commit hash and schema version

**Invalidation rule (most important invariant):**
A fact, finding, or verdict is valid only while `valid_hash` equals the symbol's current
content hash AND `dep_fingerprint` (hash of the versions/hashes of what it depends on) is
unchanged. Otherwise it is treated as expired, never as clean.

---

## 4. Phases with acceptance checks

### Phase 0: Skeleton
Cargo workspace, CLI with `index`, `status`, CI running `cargo test` and `clippy`.
**Accept:** `codegraph status` runs and prints schema version.

### Phase 1: Parse and store (one language)
Walk the repo (respect `.gitignore`, flag generated/vendor), parse with tree-sitter, extract files,
symbols (functions, classes, methods), ranges, signatures, docstrings, imports. Write to SQLite.
**Accept:**
- On `fixtures/` repos, extracted symbols match a hand-written expected JSON exactly.
- Re-indexing an unchanged repo does 0 parses (hash check).
- Indexing a 100k-LOC real repo completes in under 2 minutes.

### Phase 2: MCP server, basic tools
Tools: `find_symbol(name)`, `outline(file)`, `callers(symbol)`/`callees(symbol)` (syntactic for now,
confidence=`inferred`), `get_context(symbol, budget_tokens)`.
Output format: dense and terse, for example
```
login@auth/login.py:42-71 (user,pw)->Session
 calls: hash_pw@crypto.py:10 [inferred]
 callers: api_login@routes.py:15 [inferred]
```
**Accept:**
- Works from a real coding agent via MCP config.
- Every response is under its budget (test with a counter).
- p95 latency under 100 ms on a 100k-LOC index.

### Phase 3: Incremental updates
File watcher plus `git diff` driven re-index. Only changed files are re-parsed. Edges from/to changed
symbols are recomputed. Store indexed commit; the server compares it to HEAD and reports `stale` or
refreshes before answering.
**Accept:**
- Edit one file, the index is correct again in under 1 s.
- Property test: random edit sequences, then the incremental index equals a full re-index.
- **Start dogfooding here.**

### Phase 4: Precise resolution
Ingest SCIP (or LSP) data for definitions and references. Upgrade matching edges to `certain`; leave
unresolved edges as `unknown`. Never silently drop uncertain edges.
**Accept:** on fixtures, call-graph precision/recall against ground truth is reported and stays above
thresholds recorded in `DECISIONS.md`. Dynamic dispatch cases are labeled `unknown`, not guessed.

### Phase 5: Derived behavior facts
Extract per symbol: side effects (DB, network, filesystem, env, subprocess), exceptions raised and
caught, entry points (routes, CLI commands, jobs, handlers), and framework wiring (decorators, DI,
middleware order). Add tool `explain(symbol, budget)`.
**Accept:** fixtures with known side effects are detected correctly; false-positive rate measured and
logged.

### Phase 6: Git and test enrichment
Churn, authorship, bug-fix commit counts (commit messages and linked fixes), co-change coupling, and
test-to-symbol mapping (coverage data if available, naming/import heuristics otherwise). Tools:
`recent_changes(path)`, `tests_for(symbol)`, `risk_rank(scope)`.
**Accept:** `tests_for` correct on fixtures; co-change scores reproducible from a fixed git history.

### Phase 7: Findings and verdicts
Adapters that run external analyzers and map output to symbols: type checker, linter, Semgrep
(security), dependency audit, secret scanner. Findings diffed across runs (new / still open / fixed /
regressed). Verdicts written with `check_set`, `valid_hash`, `dep_fingerprint`.
Tools:
- `what_to_check(task)` returns open findings, expired verdicts, unchecked symbols, and a skip list
- `record_finding(symbol, kind, evidence)` and `mark_cleared(symbol, checks)` for agent write-back
  (stored with lower confidence than analyzer output until verified)
- `trace_error(stack_trace)` maps frames to symbols, then returns callers/callees, recent changes, and open findings
- `impact_of(symbol)`
**Accept:**
- Editing a cleared function expires its verdict and those of impacted callers; unrelated verdicts
  survive. Test this explicitly.
- A dependency version bump expires verdicts that depend on it.
- Analyzer precision is tracked; noisy rules are down-ranked automatically.

### Phase 8: Taint and security paths
Mark sources (request input), sinks (SQL, shell, file paths, eval), and sanitizers. Tool:
`taint_paths(scope)` returns source-to-sink paths with no sanitizer between them.
**Accept:** fixtures containing known-vulnerable and known-safe flows give the correct result for each.

### Phase 9: Semantic search and summaries (optional, last)
Local embedding model (ONNX) per symbol for fuzzy queries ("where is auth handled?"). One-line summaries
cached by content hash, so the LLM cost is paid once per change. Tool: `search_semantic(query)`.
**Accept:** retrieval recall@5 on a labeled query set meets the threshold in `DECISIONS.md`.

### Phase 10: Evaluation (run continuously from phase 2 onward)
`bench/` holds 30-50 real bug-fix tasks from the owner's repos (starting commit plus the failing test
or issue text). Harness runs the agent with and without codegraph and records: tokens used, number of
tool calls, files read, success/failure, wall time.
**Accept:** reports a table per run; regressions in success rate or token use block a phase from being
marked done.

---

## 5. Cross-cutting requirements

- **Budgeting:** every tool takes `budget_tokens` (default 800) and returns ranked results, with a
  "N more available, narrow by X" hint instead of overflowing.
- **Confidence everywhere:** responses show `[certain|inferred|unknown]` per edge or fact.
- **Escape hatch:** every result ends with exact `file:start-end` ranges so the agent can verify cheaply.
- **Staleness:** every response header includes `index@<commit> fresh|stale`.
- **Performance targets:** query p95 under 100 ms, incremental update under 1 s, first full index of
  1M LOC in under 10 min.
- **Privacy:** no network calls except when the owner explicitly enables an analyzer or model download.
  Code never leaves the machine.
- **Failure behavior:** if a parser or analyzer fails on a file, record it in `meta` as an indexing gap
  and surface it in `status`. Never return partial results as if they were complete.
- **Schema migrations:** versioned from day one.

---

## 6. How to run the agent session by session

Every session, give the agent exactly this:

```
Read PLAN.md, STATUS.md, DECISIONS.md. Continue from the current phase.
Do the next unfinished step, run the acceptance checks for it, update STATUS.md
and DECISIONS.md, then stop and summarize what changed and what you need from me.
```

Tips:
- Review each phase's tests before accepting the phase. The tests are your real specification.
- Keep sessions small (one step to one phase). Long sessions drift.
- Commit after each passing phase so you can roll back.
- When the agent proposes a change to the schema or invalidation rules, require it to update `PLAN.md`
  section 3 first.

---

## 7. First 3 tasks to give the agent (concrete start)

1. "Do Phase 0 and Phase 1 for Python. Create fixtures with 3 small sample repos and hand-written
   expected symbol JSON. Show me the passing tests."
2. "Do Phase 2. Wire the MCP server into my coding agent's config and show a transcript of the agent
   using `find_symbol` and `get_context` on this repo."
3. "Do Phase 3 and start the benchmark harness skeleton in `bench/`."
