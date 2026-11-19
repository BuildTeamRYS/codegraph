# COMPAT: agent compatibility matrix

Status values: `unverified` | `verified (date, agent version, OS)`.

Configuration paths and formats below were re-verified against each agent's official documentation on **2026-11-19** (see "Source quality"). Agents change their config surface between releases; re-verify on every major agent release and update `crates/agents/src/spec.rs` to match. Rows are verified end-to-end (real install + `codegraph init` + tool call) only when marked `verified`.

Note on naming: Windsurf is being transitioned to **Devin Desktop/CLI** (Cognition); Google Antigravity is `agy`/IDE from Google DeepMind. "Claude Code" covers the CLI, desktop app and IDE extensions (shared config).

## Tier 1 (must work before release)

| Agent | Config location (verified 2026-11-19) | Format and key | Source quality | Status |
|---|---|---|---|---|
| Claude Code (CLI, desktop app, IDE extensions) | project `.mcp.json`; user `~/.claude.json` (top-level `mcpServers`; `claude mcp add` CLI default is a private `local` scope under `projects."<path>".mcpServers`); local `.mcp.json` wins over user | JSON, `mcpServers`, entry `{type:"stdio", command, args[], env?}` (`type` omitted ⇒ stdio) | Official docs (code.claude.com/docs/en/mcp) | verified (2026-11-19, Claude Code 2.1.201, Linux — config accepted by `claude mcp list`, `⏸ Pending approval` as expected for project scope; tools not yet listed pending interactive approval) |
| Cursor | project `.cursor/mcp.json`; user `~/.cursor/mcp.json` (Windows path not officially printed; presumably `%USERPROFILE%\.cursor\mcp.json`) | JSON, `mcpServers`, entry `{command, args[], env?, envFile?}` (docs omit `type`; keep typeless) | Official docs (cursor.com/docs/mcp) | unverified |
| VS Code (GitHub Copilot) | project **root `.mcp.json`** (portable Copilot format, recommended); user `$COPILOT_HOME/mcp-config.json` else `~/.copilot/mcp-config.json`. **Not** `.vscode/mcp.json` — editor-managed, deprecated, removed ~Feb 2027 | JSON, `mcpServers`, entry `{type:"stdio", command, args[], tools?}` (portable format) — same file & shape as Claude Code project scope | Official docs (code.visualstudio.com — MCP configuration reference) | unverified |
| Codex CLI | user `~/.codex/config.toml` (or `$CODEX_HOME/config.toml`); project `<repo>/.codex/config.toml` (**trusted folders only**) | TOML, `[mcp_servers.<name>]` `command = …, args = […]`, `env`, `cwd` | Official docs + openai/codex source | unverified |
| Gemini CLI | project `.gemini/settings.json`; user `~/.gemini/settings.json` (all OSes) | JSON, `mcpServers`, entry `{command, args[], env?, timeout?, trust?}`; project settings load only in **trusted** folders | Official docs (geminicli.com / github google-gemini/gemini-cli) | unverified |
| Google Antigravity (IDE and CLI) | user `~/.gemini/config/mcp_config.json`; project `<workspace>/.agents/mcp_config.json` — documented, but **silently ignored by current CLI builds** (upstream issue #60, platform-dependent); user scope is the reliable choice | JSON, `mcpServers`, entry `{command, args[], env?, disabled?}`; `agy mcp add` (v1.1.16+, undocumented in CLI reference) | Official docs (antigravity.google/docs/mcp) + upstream issue tracker | unverified |
| OpenCode | project `opencode.json` or `opencode.jsonc`; user `~/.config/opencode/opencode.json` (or `$XDG_CONFIG_HOME/opencode/opencode.json`) | JSON(C), `mcp.servers.<name>`, entry `{type:"local", command:[bin, …args]}` — **command is an array**; `disabled` not `enabled` | Official docs (opencode.ai/docs/mcp-servers) | **verified (2026-11-19, OpenCode v2.0.27, Linux — `opencode mcp list` → `✓ codegraph connected`, `codegraph doctor --agent opencode` all PASS)** |
| Windsurf (Cascade) / Devin Desktop | Devin CLI/Desktop local: user `$XDG_CONFIG_HOME/devin/mcp_config.json` else `~/.config/devin/mcp_config.json` (Windows `%AppData%\devin\mcp_config.json`); project `.devin/mcp_config.json` (+ gitignored `.local.json` variant); legacy discovery file `~/.codeium/windsurf/mcp_config.json` (opt-in via `chat.mcp.discovery.enabled`) | JSON, `mcpServers`, entry `{command, args[], env?, disabled?}` | Official docs (docs.devin.ai — Cascade/MCP configuration) | unverified |
| Claude Desktop | macOS `~/Library/Application Support/Claude/claude_desktop_config.json`; Windows `%APPDATA%\Claude\claude_desktop_config.json`; Linux (beta) `~/.config/Claude/claude_desktop_config.json`; **user-scope only, no project scope**; reads no instruction file; fully quit & relaunch after edits | JSON, `mcpServers`, entry `{command, args[], env?}` | Official docs (modelcontextprotocol.io quickstart; support.claude.com has no raw-JSON page) | unverified |

## Tier 2 (best effort, add as users ask)

Cline, Roo Code, Zed, Continue, Goose, GitHub Copilot CLI, Augment, JetBrains assistants. Config locations not yet researched; the adapter table (`crates/agents`) is where new entries go once researched.

## Tier 3

Any other MCP client: use `codegraph mcp-config --format json|toml|yaml` to print a snippet, or call the CLI subcommands (`codegraph query <tool> …`) from a shell tool.

## Adapter behavior (all Tier 1 agents)

`codegraph init` writes configs **surgically**: comments, key order, formatting and other servers survive byte-identically (JSON/JSONC via `jsonc-parser` CST; TOML via `toml_edit`). The original is backed up once (`<file>.codegraph.bak`, never overwritten) and `agents remove` restores the pre-install bytes exactly when nothing else changed. `init` prints the diff and only writes with confirmation (`--yes` to skip), prefers project scope where the agent supports it (`--scope user|project`), writes the agent's instruction file (`CLAUDE.md` for Claude Code; `GEMINI.md` for Gemini CLI; `.github/copilot-instructions.md`-compatible `AGENTS.md` elsewhere; Claude Desktop reads none), and points configs at the stable shim `~/.codegraph/bin/codegraph` (Windows `%LOCALAPPDATA%\codegraph\bin\codegraph.exe`). Env vars: none are written by codegraph (no secrets needed); agents that filter env (Gemini redacts `*TOKEN*`-like vars) are unaffected.

## Quirks to test on every agent

- Do environment variables from the config reach the server?
- Does the command resolve on Windows (we write an absolute shim path, so it should)?
- Total tool-count limits and combined server+tool name length limits.
- Output-size caps and per-call timeouts.
- Permission prompts: are read-only tools auto-approved via `readOnlyHint`, or does the user get prompted on every call?
- Which instruction file does the agent read (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/`, other)?
- Antigravity: does project `.agents/mcp_config.json` actually load in your version (issue #60 says it may not)?
- Gemini CLI: is the folder trusted? Project `mcpServers` never connect in untrusted folders.
- Codex CLI: is the project trusted (`trust_level = "trusted"`)? Project `.codex/config.toml` is otherwise ignored entirely.

## Verification log

### 2026-11-19 — OpenCode v2.0.27 (Linux, x86_64)

```
agent: OpenCode
version: v2.0.27
os: Linux
date: 2026-11-19
config path actually used: <repo>/opencode.json (project scope)
install result (init, run twice, remove): init --yes re-pointed the existing entry at the
  stable shim (~/.codegraph/bin/codegraph) + wrote AGENTS.md instruction block; idempotent;
  remove path tested in integration tests (restores original bytes)
tools listed (count): 6 (find_symbol, outline, callers, callees, get_context, status)
find_symbol / get_context transcript: opencode mcp list → ✓ codegraph connected;
  codegraph doctor --agent opencode → all PASS
quirks found: `opencode mcp list` is the health check; restart not required for new
  sessions; project config wins over user config for same-named servers
source: official docs https://opencode.ai/docs/mcp-servers (fetched 2026-11-19)
```

### 2026-11-19 — Claude Code 2.1.201 (Linux, x86_64)

```
agent: Claude Code (CLI)
version: 2.1.201
os: Linux
date: 2026-11-19
config path actually used: <repo>/.mcp.json (project scope, created by codegraph init)
install result (init): wrote .mcp.json (new file) + CLAUDE.md instruction block;
  claude mcp list finds it and reports "codegraph: … - ⏸ Pending approval (run `claude` to approve)"
tools listed (count): not listed yet — project-scope servers need interactive approval first.
  Headless check blocked too: `claude -p …` answers "Not logged in · Please run /login"
  on this machine (no credentials). Config acceptance itself is verified; the tool call
  needs one interactive `claude` run + approval. doctor does not depend on approval.
find_symbol / get_context transcript: pending — tool call blocked on approval/login;
  MCP conformance itself covered by codegraph's rmcp-based e2e + doctor handshake (all PASS)
quirks found: `claude mcp add` defaults to a private `local` scope (~/.claude.json under
  projects."<path>".mcpServers), not user scope; project .mcp.json entries are otherwise
  auto-approved in non-interactive runs (claude -p)
source: official docs https://code.claude.com/docs/en/mcp (fetched 2026-11-19)
```

### Verification backlog (need a real install)

- Cursor (CLI health: no documented headless check; IDE/GUI required)
- VS Code Copilot (headless: `code` CLI absent on the test machine)
- Codex CLI / Gemini CLI / Antigravity / Windsurf / Claude Desktop (not installed on the test machine)
