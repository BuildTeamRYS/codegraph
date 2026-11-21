# codegraph

A local code knowledge graph for coding agents. It indexes your repository on your own machine and lets any MCP-capable agent look up where things are, what calls what, what could break, and what has already been checked, so the agent reads the right 30 lines instead of 30 files.

> **Status: planning.** This README describes the target design, and not everything below is built yet. See [STATUS.md](STATUS.md) for what works today and [PLAN.md](PLAN.md) for the build plan.

It is built for agents, not humans: results are dense, token-budgeted text with exact `file:line` ranges and a confidence tag (`certain`, `inferred`, `unknown`) on every relationship.

## How it works

```mermaid
flowchart LR
  subgraph INPUTS["Inputs"]
    A1["Source files"]
    A2["Git history"]
    A3["Tests and coverage"]
    A4["Optional analyzers<br/>type checker, linter, Semgrep"]
  end

  subgraph IDX["Indexer (one Rust binary)"]
    B1["Parse<br/>tree-sitter + LanguagePlugin"]
    B2["Resolve<br/>native resolver<br/>(tsserver or SCIP if present)"]
    B3["Derive facts<br/>side effects, entry points"]
    B4["Enrich<br/>churn, co-change, test links"]
  end

  C[("Knowledge base<br/>SQLite in .codegraph/<br/>symbols, edges, facts,<br/>findings, verdicts")]
  U["Updater<br/>watcher + git diff<br/>incremental re-index"]

  subgraph Q["Query layer"]
    D3["Rank + token budget"]
    D1["MCP server (stdio)"]
    D2["CLI: codegraph query"]
  end

  AG["Any MCP-capable coding agent<br/>Claude Code, Cursor, Codex CLI, Gemini CLI,<br/>Antigravity, OpenCode, Windsurf, ..."]
  L["Query log<br/>status, doctor, stats, later UI"]
  S["codegraph init<br/>writes agent config + instructions"]

  A1 --> B1 --> B2 --> B3 --> C
  A2 --> B4
  A3 --> B4
  B4 --> C
  A4 --> C
  U --> B1
  U -. "expires stale verdicts" .-> C
  C --> D3
  D3 --> D1
  D3 --> D2
  D1 <--> AG
  D2 --> AG
  D1 --> L
  S -. "sets up" .-> AG
```

If your viewer does not render Mermaid, see [architecture.svg](architecture.svg).

1. The indexer parses your code (tree-sitter), resolves names, derives facts such as side effects and entry points, and adds git and test information.
2. Everything is stored in a local SQLite file in `.codegraph/`. Each fact is tied to the hash of the code it describes, so when the code changes, stale facts expire instead of misleading the agent.
3. The updater watches for changes and re-indexes only what changed.
4. Agents query it through a standard MCP server (or the CLI) and get ranked, size-capped answers.
5. Nothing leaves your machine.

## Get started

- Installing the binaries or connecting an agent? See [SETUP.md](SETUP.md).
- Building from source or preparing a dev machine from scratch? See [CONFIG.md](CONFIG.md).

## Agent compatibility

The codegraph MCP server works with any MCP-capable client — that is the only integration surface. Installation and per-agent setup commands are documented in [SETUP.md](SETUP.md); compatibility details are tracked in [COMPAT.md](COMPAT.md).

| Agent                                          | Status                                                                        |
| ---------------------------------------------- | ----------------------------------------------------------------------------- |
| Claude Code (CLI, desktop app, IDE extensions) | verified 2026-11-19 (pending interactive approval on first project-scope run) |
| Cursor                                         | unverified                                                                    |
| VS Code with GitHub Copilot                    | unverified                                                                    |
| Codex CLI                                      | unverified                                                                    |
| Gemini CLI                                     | unverified                                                                    |
| Google Antigravity (IDE and CLI)               | unverified                                                                    |
| OpenCode                                       | verified 2026-11-19 (`opencode mcp list` → connected)                         |
| Windsurf / Devin Desktop                       | unverified                                                                    |
| Claude Desktop                                 | unverified                                                                    |

## Commands

This table is a reference for what the binary can do. Setup and configuration steps live in [SETUP.md](SETUP.md).

| Command                         | What it does                                                                                      |
| ------------------------------- | ------------------------------------------------------------------------------------------------- |
| `codegraph init`                | Index the repo, detect agents, and set them up (asks first)                                       |
| `codegraph index`               | Build or refresh the index                                                                        |
| `codegraph serve`               | Run the MCP server on stdio (agents launch this)                                                  |
| `codegraph status`              | Index health: files and symbols, indexed commit vs HEAD, gaps, active enhancers, connected agents |
| `codegraph doctor`              | Diagnose setup problems per agent                                                                 |
| `codegraph stats`               | Queries served per agent, latency, and tokens returned                                            |
| `codegraph query <tool> <args>` | Call a tool from the command line                                                                 |
| `codegraph agents list`         | List Tier 1 agents and whether they are detected/configured, per scope                            |
| `codegraph agents remove <id>`  | Undo setup: restores the pre-install config bytes; removes the instruction block                  |
| `codegraph agents doctor`       | Diagnose setup problems per agent, each finding with a fix (alias: `codegraph doctor`)            |
| `codegraph mcp-config`          | Print a config snippet for any MCP client                                                         |
| `codegraph update`              | Update the binary                                                                                 |

## Privacy

codegraph runs on your machine. It makes no network calls except updating the binary and downloads you explicitly enable (optional analyzers, embedding models). The query log records metadata only (tool, agent, result size, latency), never your source code or query text, and it stays local.

## Project documents

- [SETUP.md](SETUP.md): install and configure the binaries on target devices
- [CONFIG.md](CONFIG.md): dev machine prerequisites and release build instructions
- [PLAN.md](PLAN.md): build plan and acceptance checks
- [STATUS.md](STATUS.md): current state
- [DECISIONS.md](DECISIONS.md): why things are the way they are
- [COMPAT.md](COMPAT.md): agent compatibility matrix
