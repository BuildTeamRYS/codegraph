//! MCP server over stdio exposing codegraph queries to coding agents.
//!
//! Stdio is deliberate: every major MCP client (Claude Code/Desktop, Cursor,
//! Windsurf, Zed, Continue, OpenCode) can launch and speak to this binary,
//! so one server works everywhere. See DECISIONS.md.

use std::path::PathBuf;

use anyhow::Result;
use codegraph_core::git_head_commit;
use codegraph_query as query;
use codegraph_store::Store;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router, ServiceExt};
use serde::Deserialize;

fn freshness(repo: &std::path::Path, store: &Store) -> query::Freshness {
    let indexed = store.get_meta("indexed_commit").ok().flatten();
    let head = git_head_commit(repo);
    match (indexed.as_deref(), head.as_deref()) {
        (Some(i), Some(h)) if i == h => query::Freshness::Fresh(h.to_string()),
        (Some(i), Some(h)) => query::Freshness::Stale {
            indexed: i.to_string(),
            head: h.to_string(),
        },
        _ => query::Freshness::Unknown,
    }
}

#[derive(Clone)]
pub struct CodegraphServer {
    repo: PathBuf,
    db: PathBuf,
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

#[tool_router(server_handler)]
impl CodegraphServer {
    pub fn new(repo: PathBuf, db: PathBuf) -> Self {
        CodegraphServer { repo, db }
    }

    fn with_store(
        &self,
        f: impl FnOnce(&Store, query::Freshness) -> Result<String, query::QueryError>,
    ) -> String {
        let store = match Store::open(&self.db) {
            Ok(s) => s,
            Err(e) => return format!("error: cannot open store {}: {e}", self.db.display()),
        };
        let fresh = freshness(&self.repo, &store);
        match f(&store, fresh) {
            Ok(text) => text,
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(
        description = "Find symbols by name. Dense output: one line per match with file:start-end ranges. Use instead of opening files to locate a symbol."
    )]
    fn find_symbol(&self, Parameters(p): Parameters<FindSymbolParams>) -> String {
        self.with_store(|s, f| query::find_symbol(s, &f, &p.name, p.budget_tokens))
    }

    #[tool(
        description = "List the symbols in a file (path or unique path suffix), in source order, with file:start-end ranges."
    )]
    fn outline(&self, Parameters(p): Parameters<OutlineParams>) -> String {
        self.with_store(|s, f| query::outline(s, &f, &p.file, p.budget_tokens))
    }

    #[tool(
        description = "List syntactic callers of a symbol with [inferred] confidence and file:start-end ranges. Unresolved dynamic dispatch is never guessed."
    )]
    fn callers(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        self.with_store(|s, f| query::callers(s, &f, &p.symbol, p.budget_tokens))
    }

    #[tool(
        description = "List syntactic callees of a symbol with [inferred] confidence and file:start-end ranges. Unresolved dynamic dispatch is never guessed."
    )]
    fn callees(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        self.with_store(|s, f| query::callees(s, &f, &p.symbol, p.budget_tokens))
    }

    #[tool(
        description = "Compact context for one symbol: signature, docstring, callers and callees with confidence tags, ending in exact file:start-end ranges. Read this before opening the file."
    )]
    fn get_context(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        self.with_store(|s, f| query::get_context(s, &f, &p.symbol, p.budget_tokens))
    }
}

/// Serve over stdio until the client disconnects.
pub async fn run_stdio(repo: PathBuf, db: PathBuf) -> Result<()> {
    let server = CodegraphServer::new(repo, db);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
