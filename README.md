# codegraph

A local, fully offline, machine-oriented knowledge base for your codebase. Coding
agents query it over MCP (stdio) and read the right 30 lines instead of 30 files.

Required Notice: Copyright 2026 ravijaanthony

Not a human UI. Not cloud. Not a replacement for reading code.

## Status

Early. Phases 0–2 done: TypeScript/JavaScript parse/store, MCP server,
syntactic call graph; Phase 0.5 (one-line install, `codegraph update`,
release CI) implemented, awaiting a GitHub remote for end-to-end proof.
See [PLAN.md](PLAN.md), [FUTURE_PLAN.md](FUTURE_PLAN.md), [STATUS.md](STATUS.md).
Marked done only when its acceptance checks pass (`cargo test --workspace`).

## Quick start

### Install (one line, no toolchain required)

macOS / Linux:

```sh
curl -fsSL https://github.com/ravijaanthony/codegraph/releases/latest/download/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/ravijaanthony/codegraph/releases/latest/download/install.ps1 | iex
```

The installer downloads the right prebuilt binary for your OS/CPU, verifies
its SHA-256, installs to `~/.codegraph/bin` (macOS/Linux) or
`%LOCALAPPDATA%\codegraph\bin` (Windows), and offers to add it to your PATH.
No admin rights, no Node/Python/Rust required — the binary is
self-contained. Uninstall with `install.sh --uninstall` (same script) or
`install.ps1 -Uninstall`; remove `~/.codegraph` and its PATH entry by hand.

> **Note:** the GitHub repo and its releases don't exist yet (repo is
> local-only). Until a maintainer pushes tags and the release workflow runs,
> install locally from source: `cargo build --release` →
> `target/release/codegraph`.

### Verify

```sh
codegraph --version       # prints the version
codegraph update          # self-update to the latest release (SHA-256 verified)
codegraph update --check  # just report whether an update exists
```

`codegraph update` replaces the binary in place after verifying the
checksum; on any mismatch it aborts and leaves your binary untouched.

### Index a repo

```sh
codegraph index /path/to/repo            # or: cd repo && codegraph index
codegraph status /path/to/repo           # schema version, freshness, gaps
```

The index lives at `<repo>/.codegraph/index.db` (override with `--db`).
Indexing is hash-incremental: unchanged files are never re-parsed.

### Build from source

```sh
cargo build --release
```

## Use it from a coding agent (MCP)

`codegraph init` detects supported agents (Claude Code, Cursor, OpenCode),
shows the exact MCP config change, and applies it only after confirmation
(`--yes` skips the prompt; `--dry-run` prints without writing; `--global`
targets user configs instead of project configs; existing files are merged,
never clobbered, with a `.codegraph.bak` backup).

Prefer to do it by hand? `codegraph serve <repo>` speaks the MCP stdio
transport. Tool names: `find_symbol`, `outline`, `callers`, `callees`,
`get_context`.

OpenCode (verified working, this repo) — `opencode.json` at the project root:

```json
{
  "mcp": {
    "servers": {
      "codegraph": {
        "type": "local",
        "command": ["/abs/path/to/codegraph", "serve", "."]
      }
    }
  }
}
```

Claude Code (`.mcp.json` at the project root) — same shape; this config has NOT
been tested by the author yet:

```json
{
  "mcpServers": {
    "codegraph": {
      "command": "/abs/path/to/codegraph",
      "args": ["serve", "."]
    }
  }
}
```

## The tools

| Tool | What it does |
|---|---|
| `find_symbol` | Find symbols by name, ranked exact > prefix > substring |
| `outline` | Symbols in a file (or unique path suffix), source order |
| `callers` / `callees` | Syntactic call edges with `[inferred]` confidence |
| `get_context` | Signature, docstring, callers/callees, ending in `file:start-end` ranges |

Every response: `index@<commit> fresh|stale` header, `[certain|inferred|unknown]`
per edge, hard size cap (`budget_tokens`, default 800, overflow prints
`N more available, narrow by X`), and exact `file:start-end` ranges as an
escape hatch.

## How it works

- Language-agnostic core (`codegraph-parse`: walker + `LanguagePlugin`
  trait; `codegraph-lang-ts`: TypeScript/JavaScript plugin) → SQLite store,
  schema versioned via `meta(schema_version)`, integer IDs, no custom binary
  format. Handles `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`,
  `.cjs`.
- Symbols carry a content hash (covers docstring + declaration); facts/edges are
  valid only against the hash they were computed against.
- Resolution is deliberately conservative: named imports, namespace imports,
  `this.x`, unique same-file and unique global names are resolved and tagged
  `[inferred]`; dynamic dispatch and unresolved externals are **not guessed** —
  they stay `unknown`/unresolved.
- TypeScript fixtures in `fixtures/` carry hand-verified expected JSON; tests
  assert exact matches.

## Repo layout

```
crates/core     value types, hashing, tiny shared helpers
crates/parse    language-agnostic walker + LanguagePlugin trait
crates/lang-ts  TypeScript/JavaScript plugin
crates/store    SQLite store, schema migrations v1→v2, resolve pass
crates/query    budgeted, terse query output formatting
crates/mcp      stdio MCP server (rmcp)
crates/cli      the `codegraph` binary (index / status / serve / update / init)
install/        install.sh, install.ps1 + local test harness
fixtures/       sample TS/JS repos with hand-verified expected JSON
```

## Development

```sh
cargo test --workspace                      # all tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## Design invariants

1. Never invent facts: every edge/fact carries source, confidence, and the
   content hash it was computed against.
2. Every query response is size-capped and ends with exact `file:start-end` ranges.
3. Stale is not clean: a hash mismatch marks findings/verdicts expired, never OK.
4. Offline by default; no network calls unless the owner enables an analyzer.
5. Boring tech: Rust, tree-sitter, SQLite.

## License

**PolyForm Shield 1.0.0** — see [LICENSE](LICENSE). In short: free to use, modify
and redistribute for any permitted purpose, but not to provide a product that
competes with codegraph. Contributions that improve the project are welcome
under the same terms.
