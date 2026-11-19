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
- 2026-11-18 | language | TypeScript first, covering JS/JSX/TSX; Python second | Owner's own code is TS/JS, strong typing helps precision, large reach; Python later stress-tests dynamic-language handling
- 2026-11-18 | architecture | Core schema is language-agnostic; each language is a `LanguagePlugin`; frameworks are separate plugins | Add languages without schema changes; adding Python must need zero core changes
- 2026-11-18 | dependencies | Zero runtime dependencies: SQLite and grammars compiled in; users never need Node, npm, Python or Rust | Cannot assume developers have any toolchain installed
- 2026-11-18 | resolution | Native resolver is the baseline; tsserver/SCIP are optional enhancers detected at runtime | scip-typescript and tsserver need Node, which breaks the no-dependency rule
- 2026-11-18 | distribution | One-line install scripts (`install.sh`, `install.ps1`) fetch prebuilt binaries from GitHub Releases, SHA-256 verified, user-level install, no admin | curl/irm downloads do not set macOS quarantine or Windows Mark-of-the-Web flags, so no paid signing is needed at launch
- 2026-11-18 | signing | macOS ad-hoc signature (`codesign -s -`) in CI; paid Apple/Windows signing deferred to Phase 12 | Apple Silicon kills unsigned binaries; paid signing only matters for enterprise or antivirus false positives
- 2026-11-18 | models | Embedding models are opt-in downloads (`codegraph models pull`), never bundled or required | Keeps the binary small and dependency-free
- 2026-11-19 | agents | Standard MCP over stdio is the only integration surface; agent-specific knowledge lives in a data-driven adapter table (`crates/agents`) and COMPAT.md | One server works with any MCP client; per-agent differences are config paths, formats and entry shapes
- 2026-11-19 | agents | `init` prefers an agent's own CLI, otherwise merges into its config with diff, confirmation, backup, atomic write and idempotence | Never clobber a user's existing MCP servers or settings
- 2026-11-19 | agents | Configs point to a stable shim path (`~/.codegraph/bin/codegraph`) | `codegraph update` and Windows PATH issues cannot break agent configs
- 2026-11-19 | agents | `init` also writes a marked instruction block into AGENTS.md, CLAUDE.md, GEMINI.md, .cursor/rules etc. | An MCP server the agent never calls saves no tokens
- 2026-11-19 | agents | Read-only tools carry `readOnlyHint`; write tools are separate | Lets clients auto-approve reads and avoid a permission prompt per call
- 2026-11-19 | agents | Every MCP tool is also a CLI subcommand | Agents with a shell tool but no MCP support can still use codegraph
- 2026-11-19 | agents | Config paths in COMPAT.md are unverified starting points; verify against each agent's docs and a real install before coding its adapter | Paths and formats change between versions and third-party summaries disagree
- 2026-11-19 | docs | Architecture diagram source is `docs/architecture.mmd`, embedded as Mermaid in README.md and rendered to `docs/architecture.svg`; CI fails if they diverge | GitHub renders Mermaid, other viewers need the SVG, and docs must not drift from the design
- 2026-11-19 | observability | Terminal views (`status`, `doctor`, `stats`) before any GUI; GUI (`codegraph ui`) deferred until the benchmark is positive | A UI is large scope and would distract from proving token savings; both views read the same query log
- 2026-11-19 | observability | Query log stores metadata only (tool, agent, size, latency), never source code or query text | Privacy: code and prompts must stay out of logs
- 2026-11-19 | observability | "Tokens saved" is only ever shown as a labeled estimate; real numbers come from the Phase 10 benchmark | We cannot observe what an agent would have spent without the tool
- 2026-11-19 | docs | Per-agent setup guides and the README agent table are generated from the adapter table | Prevents docs and `init` behavior from drifting apart

## Thresholds to fill in as phases land

- Phase 4 call-graph precision threshold: TBD
- Phase 4 call-graph recall threshold: TBD
- Phase 9 retrieval recall@5 threshold: TBD

## Log

<!-- Agent: add new decisions below this line. -->
- 2026-11-19 | agents | Config edits are surgical (jsonc-parser CST / toml_edit), never a shell-out to agent-native CLIs | Byte-restore on `agents remove` and comment/format preservation are testable only with surgical edits; native CLIs vary (codex manages global-only files, opencode has no remove subcommand), can't show a pre-write diff, and can't be asserted in CI. Native CLI names documented per agent in COMPAT.md instead
- 2026-11-19 | agents | All Tier 1 rows re-verified against official docs 2026-11-19; VS Code Copilot targets root `.mcp.json` (portable Copilot format) + `~/.copilot/mcp-config.json`, NOT `.vscode/mcp.json` (editor-managed, deprecated, removed ~Feb 2027) | Old starting points were stale; `.vscode/mcp.json` is going away
- 2026-11-19 | agents | Antigravity project scope (.agents/mcp_config.json) is implemented but flagged unreliable; user scope (~/.gemini/config/mcp_config.json) is the reliable path | Official docs disagree with behavior (upstream issue #60); CONFIGS must work today, so the warning is printed during init
- 2026-11-19 | agents | Claude Code / VS Code Copilot share root `.mcp.json`; one codegraph entry serves both (same shape; STDIO_TYPE). Doctor treats "instructions block missing" as WARN, not FAIL, because an entry can exist for an agent init never set up | Both agents officially read the portable file; a hard FAIL would be noise when Claude Code was the one configured
- 2026-11-19 | agents | Codex CLI project scope (.codex/config.toml) written surgically though `codex mcp add` exists | `codex mcp add` manages only the global file and cannot write project scope or show a diff; surgical edit preserves comments and is uniform
- 2026-11-19 | agents | Instruction block targets per-agent native file (CLAUDE.md for Claude Code, GEMINI.md for Gemini CLI, AGENTS.md for the rest); user scope writes no instruction block | AGENTS.md is not read by Gemini CLI (gemini-cli#28227 not_planned) or Claude Desktop (none); global instruction files are too invasive without explicit opt-in
- 2026-11-19 | agents | `codegraph init` creates the stable shim (~/.codegraph/bin/codegraph) by copying the current binary if missing, and refuses to overwrite it | Agent configs point at a stable path so `codegraph update` and build-directory moves never break them
- 2026-11-19 | mcp | MCP `roots` not used; repo resolution = `--project` > `$CODEGRAPH_PROJECT` > walk up from cwd to nearest `.git` | `roots` is deprecated in protocol 2026-07-28 (SEP-2577); the stdio launch cwd is the repo for every real client
- 2026-11-19 | mcp | `status` is a tool (not just CLI) and the first index runs on a background thread (updater role only); `.codegraph/serve.lock` (flock/fcntl/LockFileEx via std File::try_lock) elects one updater per repo, others are read-only | PLAN 2.5A; agents that open 3+ windows must not trip over each other; read-only servers still answer all tools
- 2026-11-19 | mcp | readOnlyHint=true on all 6 tools; write tools (record_finding, mark_cleared) will be separate when Phase 7 lands | PLAN 2.5A; most clients auto-approve read-only tools
- 2026-11-19 | observability | Tokens are estimated bytes/4 for budgeted output (already the query-layer approximation) | Cheap, documented, deterministic; no tokenizer dependency
- 2026-11-19 | docs | Per-agent setup guides (docs/agents/*.md) and generated README table are deferred to Phase 2.6 — README now says so instead of linking to nonexistent files | Rule 14 (keep docs true) forbids broken links in the same change that makes the rest true
- 2026-11-19 | release | Release base (installers, `codegraph update`, docs links) is github.com/BuildTeamRYS/codegraph | Actual git remote; github.com/ravijaanthony/codegraph 404s
- 2026-11-19 | release | Linux ARM64 built via `cross` on the free x86_64 ubuntu-latest runner | No ARM hosted runners on a private free org; docker-embedded cross toolchain needs no ARM hardware
- 2026-11-19 | release | Windows ARM64 is best-effort on windows-latest (MSVC ARM64 toolset if present); install.ps1 falls back to the x86_64 asset on ARM64 Windows | Cannot guarantee the toolset on free images; Windows emulates x86_64 natively, so no attendee is blocked
- 2026-11-19 | release | Release assets are exactly the binaries + installers + checksums.txt; LICENSE not uploaded | Owner asked for the exact 9-asset list
- 2026-11-19 | release | Linux ships static musl only; no .deb/.rpm packages at this stage | Runs on every distro incl. RPM-based; packaging deferred per PLAN Phase 12
- 2026-11-19 | install | install.sh/install.ps1 print step-by-step progress with sizes, speed and ETA (Write-Progress in PS, curl meter in sh) and a final "what was stored where" summary; `--from-dir DIR`/`-FromDir DIR` installs from a folder of assets for the offline demo bundle | Repo is private — anonymous downloads 404; bundle handout is the supported offline path
- 2026-11-19 | install | `--uninstall` is agent-aware (lists codegraph MCP configs, offers removal, prints the exact PATH rc line to delete) | Otherwise agent configs point at a deleted binary and break silently
- 2026-11-19 | store | MCP background first-index vs status poll raced on fresh SQLite files → SQLITE_BUSY (Windows CI; latent everywhere). Fix: WAL conversion pragma retried up to 100×10ms (first opener wins, others no-op), BEGIN IMMEDIATE migration with version re-check inside the write lock, busy_timeout=5000 on every connection | The race was scheduling-luck on Linux; failed deterministically on Windows CI
- 2026-11-19 | store | Fresh databases can be opened concurrently by N connections with no cross-process coordination; every entry point (open, in_memory, background index, status handler) must be atomic against each other | Mutex-by-design in SQLite, not in Rust
- 2026-11-19 | release | Repo is public; the curl|sh / irm|iex one-liners and anonymous `releases/latest` are the primary install paths; browser download + --from-dir bundle are documented fallbacks | Owner flipped the repo public on 2026-11-19; token-based flows no longer needed for demos
- 2026-11-19 | ci | install.ps1 lifecycle (from-dir → --version → -Uninstall) is smoke-tested on windows-latest in CI; install.sh harness matrix now ubuntu+macos | install.ps1 had never been executed anywhere; CI is its first and permanent test, no Windows machine required
