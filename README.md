# codegraph

A local, fully offline, machine-oriented knowledge base for your codebase. Coding
agents query it over MCP (stdio) and read the right 30 lines instead of 30 files.

Required Notice: Copyright 2026 ravijaanthony

Not a human UI. Not cloud. Not a replacement for reading code.

## Status

Early. Phases 0–2 done: TypeScript parse/store, MCP server, syntactic TS call
graph. See [PLAN.md](PLAN.md), [FUTURE_PLAN.md](FUTURE_PLAN.md), [STATUS.md](STATUS.md).
Marked done only when its acceptance checks pass (`cargo test --workspace`).

## Quick start

```sh
cargo build --release

# index a TypeScript repo
codegraph index /path/to/repo            # or: cd repo && codegraph index

# check the store
codegraph status
```

The index lives at `<repo>/.codegraph/index.db` (override with `--db`).
Indexing is hash-incremental: unchanged files are never re-parsed.

## Use it from a coding agent (MCP)

`codegraph serve <repo>` speaks the MCP stdio transport — the portable way to
plug into MCP clients. Tool names: `find_symbol`, `outline`, `callers`,
`callees`, `get_context`.

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

- tree-sitter parses `.ts/.tsx` only (for now) → SQLite store, schema versioned
  via `meta(schema_version)`, integer IDs, no custom binary format.
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
crates/core    value types, hashing, tiny shared helpers
crates/parse   tree-sitter extraction (symbols, imports, call sites)
crates/store   SQLite store, schema migrations v1→v2, resolve pass
crates/query   budgeted, terse query output formatting
crates/mcp     stdio MCP server (rmcp)
crates/cli     the `codegraph` binary (index / status / serve)
fixtures/      3 sample TS repos with hand-verified expected JSON
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
