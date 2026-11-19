//! MCP server over stdio exposing codegraph queries to coding agents.
//!
//! Stdio is deliberate: every major MCP client can launch and speak to
//! this binary, so one server works everywhere. See COMPAT.md.
//!
//! Phase 2.5 hardening:
//! - read-only tools carry `readOnlyHint: true` so clients can auto-approve;
//! - a `status` tool reports index freshness, gaps, the updater role and
//!   background first-index progress;
//! - several `serve` processes on one repo elect a single updater via an
//!   advisory file lock (`.codegraph/serve.lock`); the rest are read-only;
//! - the first index runs on a background thread — the server answers
//!   immediately and `status` shows progress;
//! - MCP `roots` are not used: the mechanism is deprecated in protocol
//!   2026-07-28 (SEP-2577). The repo is chosen by `--project`, then
//!   `CODEGRAPH_PROJECT`, then by walking up from the cwd to the nearest
//!   `.git`. See DECISIONS.md.
//!
//! stdout carries only protocol messages; all diagnostics go to stderr.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use codegraph_core::git_head_commit;
use codegraph_query as query;
use codegraph_store::Store;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router, ServiceExt};
use serde::Deserialize;

fn freshness_of(repo: &std::path::Path, store: &Store) -> query::Freshness {
    let indexed = store.get_meta("indexed_commit").ok().flatten();
    let head = git_head_commit(repo);
    query::freshness(indexed.as_deref(), head.as_deref())
}

/// Which role this server runs in on the repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRole {
    /// Holds `.codegraph/serve.lock`; may run a background first-index.
    Updater,
    /// Another live server holds the lock; this one only reads.
    ReadOnly,
}

impl ServerRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerRole::Updater => "updater",
            ServerRole::ReadOnly => "read-only",
        }
    }
}

/// Lifecycle of the background first index (reported by `status`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IndexingState {
    #[default]
    Idle,
    Running,
    Done,
    Failed(String),
}

impl IndexingState {
    fn as_str(&self) -> String {
        match self {
            IndexingState::Idle => "idle".to_string(),
            IndexingState::Running => "running".to_string(),
            IndexingState::Done => "done".to_string(),
            IndexingState::Failed(e) => format!("failed: {e}"),
        }
    }
}

#[derive(Clone)]
pub struct CodegraphServer {
    repo: PathBuf,
    db: PathBuf,
    role: ServerRole,
    indexing: Arc<Mutex<IndexingState>>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FindSymbolParams {
    /// Symbol name to search for (exact match ranked first, then prefix, then substring).
    pub name: String,
    /// Token budget for the response (default 800).
    pub budget_tokens: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct OutlineParams {
    /// Repo-relative path of the file, or a unique path suffix (e.g. `cart.ts`).
    pub file: String,
    /// Token budget for the response (default 800).
    pub budget_tokens: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SymbolParams {
    /// Symbol name (or qualified name like `Cart.total`). Unqualified names that
    /// match several symbols print up to 8 candidates.
    pub symbol: String,
    /// Token budget for the response (default 800).
    pub budget_tokens: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct StatusParams {
    /// Token budget for the response (default 800).
    pub budget_tokens: Option<usize>,
}

#[tool_router(server_handler)]
impl CodegraphServer {
    pub fn new(
        repo: PathBuf,
        db: PathBuf,
        role: ServerRole,
        indexing: Arc<Mutex<IndexingState>>,
    ) -> Self {
        CodegraphServer {
            repo,
            db,
            role,
            indexing,
        }
    }

    fn with_store(
        &self,
        f: impl FnOnce(&Store, query::Freshness) -> Result<String, query::QueryError>,
    ) -> String {
        let store = match Store::open(&self.db) {
            Ok(s) => s,
            Err(e) => return format!("error: cannot open store {}: {e}", self.db.display()),
        };
        let fresh = freshness_of(&self.repo, &store);
        match f(&store, fresh) {
            Ok(text) => text,
            Err(e) => format!("error: {e}"),
        }
    }

    /// True when no usable index exists yet (fresh install).
    fn needs_first_index(&self) -> bool {
        if !self.db.exists() {
            return true;
        }
        Store::open(&self.db)
            .and_then(|s| s.get_meta("indexed_commit"))
            .map(|v| v.is_none())
            .unwrap_or(false)
    }

    /// Spawn the background first index, if we hold the updater role and
    /// the repo has no index yet. Never fails the server.
    fn maybe_spawn_background_index(&self) {
        if self.role != ServerRole::Updater || !self.needs_first_index() {
            return;
        }
        if matches!(
            *self.indexing.lock().unwrap_or_else(|e| e.into_inner()),
            IndexingState::Running
        ) {
            return; // already started
        }
        let repo = self.repo.clone();
        let db = self.db.clone();
        let indexing = Arc::clone(&self.indexing);
        std::thread::spawn(move || {
            {
                let mut guard = indexing.lock().unwrap_or_else(|e| e.into_inner());
                *guard = IndexingState::Running;
            }
            let outcome = (|| -> Result<()> {
                let store = Store::open(&db)?;
                codegraph_index::index_repo(&store, &repo)?;
                Ok(())
            })();
            let mut guard = indexing.lock().unwrap_or_else(|e| e.into_inner());
            *guard = match outcome {
                Ok(()) => IndexingState::Done,
                Err(e) => {
                    eprintln!("codegraph: background first index failed: {e:#}");
                    IndexingState::Failed(format!("{e:#}"))
                }
            };
        });
    }

    #[tool(
        description = "Find symbols by name. Dense output: one line per match with file:start-end ranges. Use instead of opening files to locate a symbol.",
        annotations(read_only_hint = true)
    )]
    fn find_symbol(&self, Parameters(p): Parameters<FindSymbolParams>) -> String {
        self.with_store(|s, f| query::find_symbol(s, &f, &p.name, p.budget_tokens))
    }

    #[tool(
        description = "List the symbols in a file (path or unique path suffix), in source order, with file:start-end ranges.",
        annotations(read_only_hint = true)
    )]
    fn outline(&self, Parameters(p): Parameters<OutlineParams>) -> String {
        self.with_store(|s, f| query::outline(s, &f, &p.file, p.budget_tokens))
    }

    #[tool(
        description = "List syntactic callers of a symbol with [inferred] confidence and file:start-end ranges. Unresolved dynamic dispatch is never guessed.",
        annotations(read_only_hint = true)
    )]
    fn callers(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        self.with_store(|s, f| query::callers(s, &f, &p.symbol, p.budget_tokens))
    }

    #[tool(
        description = "List syntactic callees of a symbol with [inferred] confidence and file:start-end ranges. Unresolved dynamic dispatch is never guessed.",
        annotations(read_only_hint = true)
    )]
    fn callees(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        self.with_store(|s, f| query::callees(s, &f, &p.symbol, p.budget_tokens))
    }

    #[tool(
        description = "Compact context for one symbol: signature, docstring, callers and callees with confidence tags, ending in exact file:start-end ranges. Read this before opening the file.",
        annotations(read_only_hint = true)
    )]
    fn get_context(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        self.with_store(|s, f| query::get_context(s, &f, &p.symbol, p.budget_tokens))
    }

    #[tool(
        description = "Status of the codegraph index on this repo: freshness vs HEAD, file/symbol counts, indexing gaps, role (updater or read-only), and background first-index progress.",
        annotations(read_only_hint = true)
    )]
    fn status(&self, Parameters(_p): Parameters<StatusParams>) -> String {
        let role = self.role.as_str();
        let indexing = self
            .indexing
            .lock()
            .map(|g| g.as_str())
            .unwrap_or_else(|_| "unknown".to_string());
        let store = match Store::open(&self.db) {
            Ok(s) => s,
            Err(e) => {
                return format!(
                    "codegraph status\nerror: cannot open store {}: {e}\nrole: {role}\nindexing: {indexing}",
                    self.db.display()
                )
            }
        };
        let fresh = freshness_of(&self.repo, &store);
        let files = store.file_count().unwrap_or(0);
        let symbols = store.symbol_count().unwrap_or(0);
        let gaps = store.gaps().map(|g| g.len()).unwrap_or(0);
        let indexed_at = store
            .get_meta("indexed_at")
            .ok()
            .flatten()
            .unwrap_or_else(|| "never".into());
        let mut out = format!(
            "codegraph status\nrole: {role}\nindexing: {indexing}\nfreshness: {}\nfiles: {files}  symbols: {symbols}  gaps: {gaps}\nindexed_at: {indexed_at}",
            fresh.header()
        );
        if gaps > 0 {
            out.push_str("\ngaps (files not fully indexed): see `codegraph status` CLI output");
        }
        out
    }
}

/// Advisory, cross-platform updater election for one repo.
/// Returns the lock file (held for the caller's lifetime) and our role.
fn elect_updater(repo: &std::path::Path) -> Result<(std::fs::File, ServerRole)> {
    let dir = repo.join(".codegraph");
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let lock_path = dir.join("serve.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("cannot open {}", lock_path.display()))?;
    let role = match file.try_lock() {
        Ok(()) => ServerRole::Updater,
        Err(_) => ServerRole::ReadOnly,
    };
    Ok((file, role))
}

/// Serve over stdio until the client disconnects.
pub async fn run_stdio(repo: PathBuf, db: PathBuf) -> Result<()> {
    eprintln!("codegraph mcp: stdio transport on {}", repo.display());
    let (_lock, role) = elect_updater(&repo)?;
    let indexing = Arc::new(Mutex::new(IndexingState::Idle));
    let server = CodegraphServer::new(repo, db, role, indexing);
    server.maybe_spawn_background_index();
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// One tool as reported by `tools/list`, for `codegraph doctor`.
#[derive(Debug, Clone)]
pub struct PingedTool {
    pub name: String,
    pub read_only_hint: Option<bool>,
}

/// Outcome of pinging a server over stdio (handshake + tools/list).
#[derive(Debug)]
pub struct PingReport {
    pub tools: Vec<PingedTool>,
}

/// Spawn `command args...` in `cwd` and speak MCP to it. Used by
/// `codegraph doctor` to verify an agent's configured server actually
/// answers the protocol. Child stderr is inherited so crash output
/// reaches the terminal.
pub async fn ping_server(
    command: &str,
    args: &[String],
    cwd: &std::path::Path,
    timeout: std::time::Duration,
) -> Result<PingReport> {
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args).current_dir(cwd);
    let child = rmcp::transport::TokioChildProcess::new(cmd)
        .with_context(|| format!("cannot spawn `{command}`"))?;
    let service = tokio::time::timeout(timeout, ().serve(child))
        .await
        .context("MCP initialize timed out")?
        .context("MCP initialize failed")?;
    let tools = tokio::time::timeout(timeout, service.list_all_tools())
        .await
        .context("tools/list timed out")?
        .context("tools/list failed")?;
    let tools = tools
        .into_iter()
        .map(|t| PingedTool {
            name: t.name.to_string(),
            read_only_hint: t.annotations.and_then(|a| a.read_only_hint),
        })
        .collect();
    let _ = service.cancel().await;
    Ok(PingReport { tools })
}
