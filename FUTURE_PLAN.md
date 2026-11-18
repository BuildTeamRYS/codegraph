# CodeGraph — Future Plan (Phase 2 → Phase 10) + Owner Decisions

Scope started from: Phase 0+1 DONE (see STATUS.md, commit on `main`).
This file lists, per phase: what gets built, acceptance checks, and exactly what is needed from the owner.
Once approved, this file should live at the repo root as `FUTURE_PLAN.md`.

---

## Standing owner inputs (needed across phases)

1. **Which MCP clients to wire up** (phase 2): Claude Code? Cursor? this OpenCode harness? — I can register the server in one config by default (suggest: OpenCode global config for this machine + a `.mcp.json` example for the repo) and leave others documented.
2. **MCP transport/SDK choice** (phase 2): official Rust MCP SDK (`rmcp`) vs hand-rolled stdio JSON-RPC. Recommend `rmcp` (PLAN permits it if available) with direct JSON-RPC fallback if the SDK fights us. Not expensive to reverse either way.
3. **`budget_tokens` default** (phase 2): PLAN says 800. Confirm or override (e.g. 600 for tighter budgets).
4. **Benchmark data** (phase 10, usable from phase 2): 30–50 real bug-fix tasks from your repos — needs: repo list, starting commit per task, failing test or issue text, and permission to run your real agents head-to-head with/without codegraph. This is the single most expensive/important input; without it phase 10 is hollow.
5. **External analyzer choice** (phase 7): which tools you actually want wired in — TypeScript type checker (tsc via LSP or `tsc --noEmit`), ESLint, Semgrep (security rules), `npm audit` (dependency), gitleaks or trufflehog (secrets). All run locally, offline, no telemetry. Default set if not specified: tsc + Semgrep + npm audit + gitleaks.
6. **Python support** (post phase 2 or as scheduled): tree-sitter-python is already in mind; needs your confirmation that Python is next after TypeScript support is stable.
7. **SCIP indexer for phase 4**: recommend `scip-typescript` (needs node available — it is on this machine). Confirm, and confirm network access is OK for `npm install -g @sourcegraph/scip-typescript` once, or vendored binary. No code leaves the machine; indexer emits a local .scip file.
8. **GitHub remote?** (optional, anytime): repo is local-only; CI is written but never runs until pushed somewhere. Decide if/when a remote exists. Not blocking.
9. **Kapruka as 100k+ real testbed**: it's your private repo; I only read it for timing/dogfood spot-checks. Confirm that's fine to continue (nothing is committed/index outside `/tmp` or its own `.codegraph/`).
10. **Git identity for this repo**: commits currently use `-c user.name/-c user.email` per command. Decide whether to set a repo-local identity (suggest yes).

---

## Phase 2 — MCP server, basic tools

**Build:** `crates/mcp` (rmcp or JSON-RPC stdio) + `crates/query` (budgeting, formatting). Tools:
- `find_symbol(name)` — exact + prefix match, ranked, capped (default 20 hits / budget 800)
- `outline(file)` — symbols with file:line ranges
- `callers(symbol)`, `callees(symbol)` — syntactic for now, confidence=`inferred`: resolve call expressions to same-file or imported-module symbols by name; unresolved → `unknown`, never guessed
- `get_context(symbol, budget_tokens)` — signature, docstring, range, callers/callees with confidence tags; ends with exact `file:start-end` ranges
- Every response: header `index@<commit> fresh|stale`, per-edge `[certain|inferred|unknown]`, budget-aware with "N more available, narrow by X" hint instead of overflow.

**Tests:** budget-enforcement test with a token counter (e.g. chars/4 heuristic or real tokenizer — record in DECISIONS.md); end-to-end MCP handshake test (initialize → tools/list → tools/call) in-process; fixture-backed correctness tests.

**Accept:** (1) works from a real MCP client — needs owner item 1 + a transcript; (2) every response ≤ budget (test); (3) p95 < 100ms on 100k-LOC index (bench on synthetic 550-file repo).

**Owner needed:** items 1, 2, 3. Estimated reversibility: high (all three are config/default changes).

---

## Phase 3 — Incremental updates + dogfooding starts

**Build:** file watcher (notify crate, debounced) off by default plus explicit `codegraph index --incremental` and `codegraph sync` (git diff --name-only <indexed_commit>..HEAD) that re-parses only changed files and recomputes edges touching changed symbols. Server compares stored `indexed_commit` vs HEAD and self-reports `stale`, with opt-in `refresh_before_answer` behavior.

**Tests:** property test — random edit sequences; incremental result must equal full re-index (compare files+symbols+imports+edges dumps). Accept: one-file edit → correct index in <1s (already ~0; assert via test).

**Dogfooding begins:** index THIS repo with codegraph; I use it for navigation; failures logged in STATUS.md "Dogfooding notes".

**Accept:** <1s single-file edit reindex; property test passes; `status` shows fresh after sync.

**Owner needed:** none (git already in use locally).

---

## Phase 4 — Precise resolution (SCIP)

**Build:** `crates/resolve` ingesting a `.scip` file (protobuf, via `protobuf-codegen` or hand-rolled decode — decide, record). Upgrade `imports`/call/ref edges to `certain` where SCIP resolves; anything unresolved stays `unknown` and is **never silently dropped**. Call-graph precision/recall measured on fixtures vs hand-labeled ground truth.

**Thresholds to set in DECISIONS.md** (currently TBD): suggest precision ≥ 0.95, recall ≥ 0.80 on fixtures; dynamic dispatch (string-keyed, DI containers) labeled `unknown` deliberately.

**Owner needed:** item 7 (scip-typescript install OK? vendored vs PATH); confirm thresholds above or override.

---

## Phase 5 — Derived behavior facts

**Build:** `crates/analyze` framework-aware heuristics (no execution): side effects per symbol — DB (ORM calls, raw queries), network (fetch/axios/http), filesystem (fs.*), env (`process.env.X` → `reads_env:X`), subprocess (`child_process.*`); exceptions raised/caught (`throw new X`); entry points (exported route handlers given express/fastify/next patterns, CLI commands); wiring (decorators, DI via constructor injection, middleware order). `explain(symbol, budget)` MCP tool. Fixtures with known side effects, measured false-positive rate logged in STATUS.md.

**Owner needed:** which frameworks actually appear in your codebases (express? Next.js? NestJS?) so the heuristics aren't guesswork; confirm heuristics over AST-match approach (no TypeORM-specific magic unless you use TypeORM).

---

## Phase 6 — Git and test enrichment

**Build:** `git_stats` (commits_90d, authors, last_change, bugfix_commits via `git log --grep` for fix/bug/hotfix), `cochange` (files co-appearing in commits, score = normalized lift; deterministic from fixed history → reproducible tests on a tiny synthetic git repo fixture), test-to-symbol mapping (coverage data if you already collect it; else import+naming heuristic: `foo.test.ts` / `import { bar }` in test files). Tools: `recent_changes(path)`, `tests_for(symbol)`, `risk_rank(scope)`.

**Owner needed:** (1) confirm heuristic `tested_by` is acceptable until real coverage exists, or point me at your coverage output format (e.g. istanbul lcov) if you have it; (2) okay that `risk_rank` v1 is a documented heuristic formula, not a learned model.

---

## Phase 7 — Findings and verdicts

**Build:** analyzer adapters behind a common trait: tsc (types), ESLint (lint/style), Semgrep (security), npm audit (deps), gitleaks (secrets). Each finding stored keyed to symbol with source/confidence/valid_hash; diff across runs → `open | fixed | suppressed | regressed`. Verdicts written with `check_set`, `valid_hash`, `dep_fingerprint` (hash of lockfile + resolved dep versions; bump ⇒ fingerprint changes ⇒ dependent verdicts expire). **Explicitly tested**: editing a cleared function expires it + impacted callers; unrelated verdicts survive.

MCP tools: `what_to_check(task)` (open findings + expired verdicts + unchecked symbols + skip list), `record_finding` / `mark_cleared` (agent write-back, stored `inferred` until verified), `trace_error(stack_trace)`, `impact_of(symbol)`.

Noisy rules down-ranked automatically: track per-rule precision (confirmed vs dismissed), auto-lower rank under threshold — record formula in DECISIONS.md.

**Owner needed:** item 5 (analyzer set, install OK via npm/pip/cargo?); whether Semgrep may use its registry rules or strictly local offline rules (privacy — suggest strict local); whether you want findings mirrored into GitHub code scanning later (no unless you say yes).

---

## Phase 8 — Taint and security paths

**Build:** source/sink/sanitizer tables per framework (request input: `req.body`, `req.query`, `req.params`; sinks: raw SQL, `child_process.exec`, `fs` path joins, `eval`/`Function`); path query over the certain-edges graph; sanitizers break paths. Tool: `taint_paths(scope)`.

Fixtures: known-vulnerable (SQL string concat from req.query) and known-safe (parameterized query) flows — each must give the right verdict.

**Owner needed:** none beyond phase 5 frameworks list; confirm scope = TS/Node only this round (Python taint deferred with Python support).

---

## Phase 9 — Semantic search (optional, last)

**Build:** local ONNX embedding model per symbol; vectors in SQLite (or flat file + brute force — records as decision); summaries cached by content_hash. Tool: `search_semantic(query)`.

Model choice needs network once to download (e.g. all-MiniLM-L6-v2 ONNX ~90MB) — **owner must explicitly enable this download** (PLAN: no network without owner action). Threshold for recall@5 to be recorded in DECISIONS.md after measuring on a small labeled set from your repos.

**Owner needed:** explicit yes/no to model download + which model; labeled query set (I draft from your repos, you sanity-check 20 queries).

---

## Phase 10 — Evaluation (continuous from phase 2)

**Build:** `bench/` harness: task = {repo, start_commit, failing test or issue text}; runner executes agent with and without codegraph MCP; records tokens, tool calls, files read, success (test passes), wall time. Report table per run; regressions block phase sign-off.

**Owner needed (most expensive):** item 4 — the 30–50 tasks themselves. Suggest: I mine 20 candidates from your repos (git log for revert/fix commits touching the same test), you prune to the set you like. Also need: time budget per task, model to benchmark with, and a per-task success oracle (usually: the previously-failing test now passes).

---

## Cross-cutting / recurring owner asks

- After each phase: review the tests (they are the spec), then approve the phase commit.
- Push cadence: local-only vs add remote (item 8).
- Corrections to heuristics (phases 5–8) are cheap early, expensive late — expect a review round after phase 5 fixtures land.
- Nothing leaves your machine without an explicit "yes" (phase 7 Semgrep registry, phase 9 model download).

## Ordering & risk notes

- Phase 3 dogfooding will likely reshape phase 4–7 priorities; treat this plan as editable, not contractual.
- Phase 2 transcript acceptance is the first external blocker: without a real MCP client session, phase 2 can't be marked done.
- Phase 10 tasks are the long pole; start sourcing them during phase 3, not at phase 10.
