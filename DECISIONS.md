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

## Thresholds to fill in as phases land

- Phase 4 call-graph precision threshold: TBD
- Phase 4 call-graph recall threshold: TBD
- Phase 9 retrieval recall@5 threshold: TBD

## Log

<!-- Agent: add new decisions below this line. -->
- 2026-11-19 | repo layout | `crates/lang-ts` extracted from `crates/parse`; parse now owns the `LanguagePlugin` trait and the generic walker | New PLAN requires a language-agnostic core; grammar types must not leak into the walker/driver
- 2026-11-19 | parse | JS/JSX/MJS/CJS parsed with the TypeScript grammar family: LANGUAGE_TSX for .tsx/.jsx, LANGUAGE_TYPESCRIPT for the rest | JS is a syntactic subset of TS; JSX needs the TSX dialect; no separate JS grammar required
- 2026-11-19 | store | Module-resolution extension probing extended with js/jsx/mjs/cjs (TS probed first) | JS imports must resolve like TS imports
- 2026-11-19 | tests | crlf-repo fixture pinned with `.gitattributes` (`* -text`); expected JSON hand-verified | Git would normalize CRLF; fixture proves Windows/Linux-safe line handling
- 2026-11-19 | tests | Schema language-scan test matches exact identifiers, not substrings (`facts` contains `cts`) | Substring ban false-positived on legitimate tables
- 2026-11-19 | distribution | install.sh installs the static musl asset on all Linux; no glibc asset published for now | musl-static runs on any Linux; one asset per arch keeps checksums.txt simple; glibc users unaffected
- 2026-11-19 | distribution | Release base defaults to `https://github.com/ravijaanthony/codegraph`, overridable via `CODEGRAPH_RELEASE_BASE` | Repo has no GitHub remote yet (FUTURE_PLAN item 8); URL goes live when the owner pushes; the env var also covers mirrors/tests
- 2026-11-19 | dependencies | `ureq` 3.5.2 with default-features off + rustls only, and pure-Rust `sha2` for verification | Binary must stay self-contained; native-tls would drag system OpenSSL onto musl targets
- 2026-11-19 | distribution | `codegraph update` uses `/releases/latest` redirect to resolve the tag; aborts on any checksum mismatch; binary untouched on failure | Never install unverified code; PLAN phase 0.5 rules
- 2026-11-19 | ci | Windows dynamic-dep check byte-scans the PE for VCRUNTIME/MSVCP strings; Linux uses an ldd allowlist; macOS requires /usr/lib|/System paths only | PLAN requires CI proof that only OS-provided libs are linked; no VS tooling dependency wanted
- 2026-11-19 | ci | `windows-11-arm` and `ubuntu-24.04-arm` runners used for arm64 targets | GitHub-hosted arm64 runners; unverified until the first release run
- 2026-11-19 | install | install.sh never installs without a verified SHA-256; no sudo path; PATH changes only via interactive prompt on a tty | Integrity + least surprise; piped installs print manual PATH instructions
- 2026-11-19 | init | Agent MCP configs written only after confirmation (or `--yes`); unparseable files never clobbered; `.codegraph.bak` before every rewrite | Losing a hand-edited MCP config is worse than not configuring
- 2026-11-19 | init | Project mode default; `--global` targets user configs (`~/.claude.json`, `~/.cursor/mcp.json`); PATH lookup only in interactive use, tests use filesystem markers only | Deterministic tests; sensible defaults
- 2026-11-19 | resolve | Call resolution stays in `codegraph-store::resolve` operating on generic rows (symbols/imports/call_sites); the `LanguagePlugin` trait does not yet carry a resolve method | Resolution needs nothing TS-specific in the schema; native resolver lands in phase 4 (PLAN trait method "resolve references" deferred there)
- 2026-11-19 | process | Owner's 2026-11-19 working-tree edit truncated this file's detailed 2026-11-18 log; old entries recoverable via git | Keeping this line because the file format demands the edit be visible to future sessions
- 2026-11-19 | perf | Synthetic 100k-LOC TS repo (3449 files / 17245 symbols): full index ~1.0s, no-op re-index 0.05s | Phase 1 acceptance (<2 min) met with 60x headroom; real-repo proof needs Kapruka (owner approval pending)
- 2026-11-19 | version | Workspace bumped 0.1.0 → 0.2.0 | New plugin architecture + JS support + update/init; public release will tag from this line
