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
10. NO RUNTIME DEPENDENCIES. The shipped product is one self-contained binary. It must never
    require the user to have Node, npm, Python, Rust, Docker or anything else installed. External
    tools (tsserver, Semgrep, embedding models) may only be OPTIONAL enhancers detected at
    runtime; if absent, features degrade gracefully and nothing breaks.
11. LANGUAGE-AGNOSTIC CORE. The core schema contains no TypeScript-specific (or any single
    language) fields. Language-specific data goes through the `LanguagePlugin` trait and
    free-form `kind`/`attrs` fields.
12. Every phase must build and run on Windows, macOS and Linux. CI proves it. Never assume
    Unix paths or LF line endings.
13. AGENT-AGNOSTIC. The MCP server uses only the standard protocol. Agent-specific knowledge
    (config paths, formats, instruction files) lives only in `crates/agents` as data and in
    COMPAT.md. Never hard-code one agent into the core or the tools.
14. KEEP DOCS TRUE. README.md and docs/architecture.mmd must match reality. Any phase that adds
    or changes a component, command or supported agent updates them in the same change.
    README claims about unbuilt features must be marked as planned.
```

---

## 2. Project definition

**Goal:** an indexer plus query server that runs on the developer's laptop, fully offline, and gives coding agents a compact, accurate, token-budgeted model of a codebase: structure, relationships, behavior facts, findings, and verdicts.

**Not goals:** a human UI, replacing code reading entirely, cloud hosting.

**Stack:**
- Rust (indexer, store, server), single static binary. Rust is a build-time tool only; end users never need it
- tree-sitter for parsing, with all grammars compiled into the binary
- Native module/name resolver for precise symbols (phase 4). SCIP or LSP only as an OPTIONAL enhancer when already installed on the user's machine
- SQLite (via `rusqlite` with the `bundled` feature, compiled into the binary) as the store; zstd for large blobs
- MCP server over stdio (use the official Rust MCP SDK if available, else implement JSON-RPC directly)
- First language: TypeScript, covering JS/JSX/TSX (decided, see DECISIONS.md). Second: Python. The core stays language-agnostic through a `LanguagePlugin` trait
- Target platforms: Windows (x64, arm64), macOS (arm64, x64), Linux (x64, arm64, static musl)
- Distribution: one-line install scripts (`install.sh`, `install.ps1`) that fetch a prebuilt binary from GitHub Releases. No package manager required

**Repo layout:**
```
codegraph/
  crates/
    core/        # schema, ids, hashing, confidence types
    parse/       # tree-sitter extraction + LanguagePlugin trait
    lang-ts/     # TypeScript/JS plugin (first); later lang-py/, lang-go/, ...
    frameworks/  # optional framework plugins (Express, Next.js, NestJS, React, ...)
    resolve/     # native resolver; optional SCIP/LSP ingestion
    analyze/     # derived facts + external analyzer adapters
    store/       # SQLite access, migrations, invalidation
    query/       # ranking, budgeting, output formatting
    mcp/         # MCP server (standard protocol only)
    agents/      # per-agent adapters: detect, install, uninstall, doctor (data-driven)
    cli/         # `codegraph index|serve|status|query|init|agents|doctor|mcp-config`
  install/       # install.sh, install.ps1, checksum and uninstall logic
  fixtures/      # small sample repos with KNOWN answers (used by tests)
  bench/         # evaluation tasks and harness
  README.md  PLAN.md  DECISIONS.md  STATUS.md  COMPAT.md
  docs/          # architecture.mmd (diagram source), architecture.svg, agents/<agent>.md (generated)
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

### Phase 0.5: Distribution (do this early, not last)
Goal: a developer installs with one command and needs nothing else on their machine.
- CI matrix builds release binaries for Windows (x64, arm64), macOS (arm64, x64) and Linux (x64, arm64, static musl) and publishes them with SHA-256 checksums to GitHub Releases.
- SQLite and all tree-sitter grammars are compiled into the binary.
- macOS builds get a free ad-hoc signature (`codesign -s -`) in CI. Apple Silicon kills unsigned binaries without it. No paid signing yet.
- `install/install.sh` (macOS/Linux) and `install/install.ps1` (Windows): detect OS and CPU, download the right binary, verify the SHA-256 (abort on mismatch), install to a user folder (`~/.codegraph/bin` or `%LOCALAPPDATA%\codegraph\bin`) with no admin rights, offer to add it to PATH, print every action it takes, support `--uninstall` and a pinned `--version`.
- `codegraph update` self-updates the binary and keeps the install path stable. After installing, the script prints the next step: `codegraph init`.
- Docs show the OS-appropriate one-liner: `curl -fsSL <url>/install.sh | sh` and `irm <url>/install.ps1 | iex`. Do NOT point users at browser downloads (those get quarantine/SmartScreen flags).
**Accept:**
- On clean Windows, macOS and Linux machines or VMs with no Rust, Node or Python, the one-line install followed by `codegraph --version` and `codegraph index fixtures/<repo>` succeeds.
- The binary's dynamic dependencies are only OS-provided libraries (`ldd` / `otool -L` / `dumpbin`).
- A macOS arm64 binary installed by the script runs with no Gatekeeper prompt.
- A tampered binary (wrong checksum) makes the installer abort. Test this explicitly.
- The installer works when the home directory path contains spaces.

### Phase 1: Parse and store (TypeScript first)
Walk the repo (respect `.gitignore`, flag generated/vendor), parse with tree-sitter, extract files,
symbols (functions, classes, methods, arrow functions, exported consts), ranges, signatures, docstrings, imports/exports. Write to SQLite. Handle `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs`. Define the `LanguagePlugin` trait (extract symbols, extract imports, resolve references, detect framework entry points) and implement it for TypeScript only; the core must not know about TypeScript.
**Accept:**
- On `fixtures/` repos, extracted symbols match a hand-written expected JSON exactly.
- Re-indexing an unchanged repo does 0 parses (hash check).
- Indexing a 100k-LOC real repo completes in under 2 minutes.
- The core schema contains no TypeScript-specific columns (a test scans the schema).
- Indexing works with Windows paths and CRLF line endings (fixture with CRLF files).

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
- Stdout carries only protocol messages (tested). Setting up agents is Phase 2.5.
- Every response is under its budget (test with a counter).
- p95 latency under 100 ms on a 100k-LOC index.

### Phase 2.5: Universal agent support
Goal: any coding agent that can attach an MCP server can use codegraph, set up with one command and with no per-agent code in the core. The MCP server is the only integration surface. Everything agent-specific lives in `crates/agents` as data plus small adapters, and in `COMPAT.md`.

**A. Standards-compliant MCP server**
- stdio transport is primary. Optional streamable HTTP is localhost-only (127.0.0.1, random port, auth token) for clients that need it.
- stdout carries ONLY protocol messages. Logs go to stderr. No ANSI colors, banners or progress bars. A stray byte on stdout breaks clients.
- Negotiate the protocol version gracefully and ignore unknown capabilities. Use MCP `roots` when the client provides them; otherwise `--project <path>`; otherwise walk up from the working directory to the nearest `.git`.
- Read-only tools carry `readOnlyHint: true` so clients can auto-approve them. Write tools (`record_finding`, `mark_cleared`) are separate and clearly marked.
- Keep the tool list small, tool names short and descriptions brief, because some clients cap total tool count and combined server+tool name length. Use plain JSON-schema inputs only (no unions or exotic features), because some clients reject them.
- Respect client timeouts: no tool call blocks for more than a few seconds. The first index runs in the background and a `status` tool reports progress.

**B. Agent adapter registry (`crates/agents`)**
A data-driven table with one entry per agent: id, how to detect it (binary on PATH, app or config folder exists), config path per OS and scope (user or project), format (JSON, JSONC, TOML, YAML), the key that holds servers, the exact entry shape, an optional native add command (for example `claude mcp add`, `codex mcp add`), and where its instructions go (section C). Operations: `detect`, `install`, `uninstall`, `status`, `doctor`.
Rules for changing a user's config:
- Prefer the agent's own CLI when it has one. Otherwise edit the file.
- Merge, never overwrite. Preserve other servers, comments and formatting (use JSONC- and TOML-aware editing). Back up the original once. Write atomically. Be idempotent.
- Show a diff and ask first. `--yes` skips the prompt, `--dry-run` previews. Never touch the config of an agent the user did not select.
- Write an absolute path to a stable shim (`~/.codegraph/bin/codegraph`) into every config, so `codegraph update` never breaks configs and Windows PATH issues do not matter.
- `--scope user|project`, defaulting to project where the agent supports it.
- Entry shapes differ between agents (for example OpenCode takes `command` as an array, others take `command` plus `args`, Codex uses TOML). That is why this is a table and not a template.

**C. Agent instructions (so the agent actually uses the tools)**
An MCP server the agent ignores saves nothing. `init` also writes a short marked block (`<!-- codegraph:start --> ... <!-- codegraph:end -->`, replaced idempotently) into the agent's instruction file: `AGENTS.md` (read by many agents), `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/`, and so on. Content, 5 to 8 lines: query codegraph first for locations, callers and impact; read code only for the line ranges it returns; how to read `[certain|inferred|unknown]`; fall back to normal search when the index is stale or a result is `unknown`. Where an agent supports it, also offer a permission allow-list snippet for the read-only tools.

**D. Fallbacks for agents we do not know**
- `codegraph mcp-config --format json|toml|yaml` prints a ready-to-paste snippet with the right command path.
- Every MCP tool is also a CLI subcommand (`codegraph query find_symbol Foo`) with identical terse output, so any agent with a shell tool can use codegraph even without MCP.

**E. Several agents and windows at once**
- The index lives in `.codegraph/` in the repo (offer to add it to `.gitignore`), SQLite in WAL mode.
- Several `serve` processes can run on one repo. A lock file elects one updater/watcher per repo and the rest are read-only.
- Monorepos and multi-root workspaces get one index per root, and the server picks by path.

**F. Compatibility matrix (`COMPAT.md`)**
- Tier 1 (must work before release): Claude Code (CLI, desktop app, IDE extensions), Cursor, VS Code with GitHub Copilot, Codex CLI, Gemini CLI, Google Antigravity (IDE and CLI), OpenCode, Windsurf, Claude Desktop.
- Tier 2 (add as users ask): Cline, Roo Code, Zed, Continue, Goose, GitHub Copilot CLI, Augment, JetBrains assistants.
- Tier 3: anything else through section D.
- For each agent record: tested version, OS, the config path actually used, quirks, and a transcript of the agent calling `find_symbol` and `get_context`.
- Config paths and formats in `COMPAT.md` are starting points. Verify each against the agent's own docs and a real install before writing its adapter. Never hard-code from memory or third-party summaries.

**Accept:**
- A scripted MCP client in CI, on all three OSes, passes a conformance suite: handshake, tool list, every tool call, malformed input, cancellation, and a stdout check that finds nothing but JSON-RPC.
- For every Tier 1 agent: `codegraph init --agent <id>` makes the agent list and successfully call a tool on a real install (manual run recorded in `COMPAT.md`). Running it twice changes nothing. `codegraph agents remove <id>` restores the original config byte for byte.
- Config merge tests: existing servers, comments and formatting survive for JSON, JSONC and TOML fixtures.
- With 3 concurrent clients on one repo, exactly one updater runs and no client reads corrupted data.
- `codegraph doctor` connects to its own server as an MCP client and reports per-agent status.

### Phase 2.6: Observability, setup guidance and docs
Goal: a developer can see that codegraph works, set it up in their own agent without guessing, and trust the documentation.

**A. Query log**
Table `query_log(ts, tool, agent, project, tokens_returned, latency_ms, result_count, truncated)`. The agent name comes from the MCP client info. It records metadata only: never source code and never query text. It is local, can be disabled, and `codegraph stats --reset` clears it.

**B. Terminal views (no GUI yet)**
- `codegraph status`: files and symbols indexed, indexed commit versus HEAD (fresh or stale), indexing gaps, which optional enhancers are active, which agents are configured.
- `codegraph stats`: queries per agent, p50/p95 latency, tokens returned. Any "tokens saved" figure is shown ONLY as a labeled estimate (for example, size of the files avoided); real savings come from the Phase 10 benchmark.
- `codegraph doctor`: connects to its own server as an MCP client and diagnoses specific faults per agent: config not found, config invalid, wrong binary path, server crashes on start (run it and capture stderr), handshake fails, tool list empty or hidden by approval settings, instruction block missing, index stale. Every finding says what to do next.

**C. Post-install guidance**
After `init`, print for each configured agent: whether to restart or reload it, whether to approve the new server, a ready-to-paste verification prompt, and where to look if the tools do not appear (`codegraph doctor`).

**D. Per-agent docs, generated**
Generate `docs/agents/<agent>.md` and the supported-agents table in `README.md` from the adapter table in `crates/agents`, so code and docs cannot drift. Each page covers: automatic setup, manual setup, restart/approve steps, how to verify, troubleshooting, and uninstall. An agent gets a "verified" badge only with a tested version and date recorded in `COMPAT.md`.

**E. Architecture diagram**
- Source: `docs/architecture.mmd`. It is embedded as a Mermaid block in `README.md` (GitHub renders it) and rendered to `docs/architecture.svg` for viewers that do not render Mermaid.
- CI renders the diagram with `mmdc` and fails if the README block or the SVG does not match `docs/architecture.mmd`.
- Master prompt rule 14 applies: any change to a component, command or supported agent updates the diagram and README in the same change.

**Accept:**
- `query_log` is filled for every tool call with the correct agent name, and a test proves it contains no source code or query text.
- `status` and `stats` output is correct on fixtures. `doctor` detects each injected fault (broken config, wrong path, crashing server, missing instruction block, stale index) with a specific message.
- The README diagram renders on GitHub (manual check recorded), and CI fails when `docs/architecture.mmd`, the README block and the SVG diverge.
- Generated guides exist for every Tier 1 agent, and a developer following only that guide gets a working setup (result recorded in `COMPAT.md`).

### Phase 3: Incremental updates
File watcher plus `git diff` driven re-index. Only changed files are re-parsed. Edges from/to changed
symbols are recomputed. Store indexed commit; the server compares it to HEAD and reports `stale` or
refreshes before answering.
**Accept:**
- Edit one file, the index is correct again in under 1 s.
- Property test: random edit sequences, then the incremental index equals a full re-index.
- **Start dogfooding here.**

### Phase 4: Precise resolution (no required dependencies)
Three tiers:
- **Tier A (baseline, always works):** native resolver for TypeScript/JS: relative imports, tsconfig `paths`/`baseUrl`, `node_modules` lookup, package `exports`, re-exports and barrel files. An edge is `certain` only when resolution is unambiguous, otherwise `inferred`.
- **Tier B (optional):** if Node/tsserver or a SCIP index is already on the machine, detect it and ingest definitions and references to upgrade edges to `certain`. Absence must never cause an error.
- **Tier C:** nothing else is required.
Leave unresolved edges as `unknown`. Never silently drop uncertain edges.
**Accept:** on fixtures, call-graph precision/recall against ground truth is reported and stays above
thresholds recorded in `DECISIONS.md`. Dynamic dispatch cases are labeled `unknown`, not guessed.
The suite also runs with Tier B disabled and must still meet the Tier A thresholds.

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
Local embedding model (ONNX), downloaded on demand with `codegraph models pull` and never bundled or required, per symbol for fuzzy queries ("where is auth handled?"). One-line summaries
cached by content hash, so the LLM cost is paid once per change. Tool: `search_semantic(query)`.
**Accept:** retrieval recall@5 on a labeled query set meets the threshold in `DECISIONS.md`.

### Phase 10: Evaluation (run continuously from phase 2 onward)
`bench/` holds 30-50 real bug-fix tasks from the owner's repos (starting commit plus the failing test
or issue text). Harness runs each Tier 1 agent that supports headless runs (start with Claude Code, Codex CLI and OpenCode) with and without codegraph, so savings are measured per agent, and records: tokens used, number of
tool calls, files read, success/failure, wall time.
**Accept:** reports a table per run; regressions in success rate or token use block a phase from being
marked done.

---

### Phase 11: Language expansion and framework plugins
Add Python as the second language, then Go and Java. Adding Python must need ZERO changes to the core schema; if it does, the abstraction was wrong. Fix the core and record it in `DECISIONS.md`.
Framework plugins are separate from language plugins. For TypeScript start with Express, Next.js, NestJS and React: each contributes entry points (routes, handlers, components), side-effect rules and wiring facts.
**Accept:** Python fixtures pass the same suite as TypeScript with no core schema change; each framework plugin has fixtures with known routes and side effects.

### Phase 12: Signing and enterprise distribution (only when needed)
Trigger: antivirus false positives, enterprise requests, or users hitting Gatekeeper/SmartScreen. Add Apple Developer ID signing plus notarization and a Windows code-signing certificate. Add Homebrew, Scoop and winget packages.
**Accept:** signed binaries pass Gatekeeper and SmartScreen on clean machines, including when downloaded through a browser.

### Phase 13: Local UI (only after Phase 10 shows a positive result)
`codegraph ui` serves a read-only page on 127.0.0.1, with assets embedded in the binary (no extra install). It shows a graph explorer, findings and verdicts, per-agent usage, and tokens returned over time, so a developer can see why an agent received a particular answer. Savings appear only as labeled estimates. It reads the same data as `status` and `stats`.
**Accept:** works offline on all three OSes, binds only to localhost, is read-only, and every number matches the terminal commands.

---

## 5. Cross-cutting requirements

- **Budgeting:** every tool takes `budget_tokens` (default 800) and returns ranked results, with a
  "N more available, narrow by X" hint instead of overflowing.
- **Confidence everywhere:** responses show `[certain|inferred|unknown]` per edge or fact.
- **Escape hatch:** every result ends with exact `file:start-end` ranges so the agent can verify cheaply.
- **Staleness:** every response header includes `index@<commit> fresh|stale`.
- **Performance targets:** query p95 under 100 ms, incremental update under 1 s, first full index of
  1M LOC in under 10 min.
- **Privacy:** no network calls except installing/updating the binary and when the owner explicitly enables an analyzer or model download.
  Code never leaves the machine.
- **Failure behavior:** if a parser or analyzer fails on a file, record it in `meta` as an indexing gap
  and surface it in `status`. Never return partial results as if they were complete.
- **Schema migrations:** versioned from day one.
- **Zero runtime dependencies:** SQLite and all tree-sitter grammars are compiled in. CI checks the binary's dynamic dependencies contain only OS-provided libraries.
- **Cross-platform:** Windows, macOS and Linux are all first-class. Tests cover Windows paths, CRLF line endings, symlinks and case-insensitive filesystems.
- **Graceful degradation:** every optional enhancer (tsserver, SCIP, Semgrep, embeddings) is detected at runtime, and `codegraph status` lists which are active.

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

1. "Do Phase 0, Phase 0.5 and Phase 1 for TypeScript (covering JS/JSX/TSX). Create fixtures with 3 small sample repos and hand-written
   expected symbol JSON. Show me the passing tests."
2. "Do Phase 2. Wire the MCP server into my coding agent's config and show a transcript of the agent
   using `find_symbol` and `get_context` on this repo."
3. "Do Phase 2.5. Start with Claude Code, Cursor and Codex CLI, then the rest of Tier 1. Verify each agent's config format against its own docs and a real install, and record results in COMPAT.md."
4. "Do Phase 2.6. Build the query log, status, stats and doctor, the post-install guidance, and generate the per-agent docs and README table. Keep docs/architecture.mmd and the README diagram in sync."
5. "Do Phase 3 and start the benchmark harness skeleton in `bench/`."
