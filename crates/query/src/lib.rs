//! Dense, terse output for coding agents. Every query returns a
//! size-capped string ending in exact `file:start-end` ranges.
//!
//! Budgeting: responses are assembled greedily from a ranked line list;
//! when the budget runs out we print a "N more available, narrow by X"
//! hint instead of overflowing. Token estimate is bytes/4, a boring
//! approximation (recorded in DECISIONS.md).

use codegraph_store::{EdgeEndpoint, Store, StoreError, SymbolRow};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

/// Token estimate: ceil(bytes / 4). Deliberately simple; see DECISIONS.md.
pub fn estimate_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

#[derive(Debug, Clone)]
pub enum Freshness {
    Fresh(String),
    Stale { indexed: String, head: String },
    Unknown,
}

const DEFAULT_BUDGET: usize = 800;

struct Budgeter {
    budget: usize,
    used: usize,
    lines: Vec<String>,
    omitted: usize,
}

impl Budgeter {
    fn new(budget: usize) -> Self {
        Budgeter {
            budget: budget.max(16),
            used: 0,
            lines: Vec::new(),
            omitted: 0,
        }
    }
    fn push(&mut self, line: String) -> bool {
        let cost = estimate_tokens(&line) + 1;
        if self.used + cost > self.budget {
            self.omitted += 1;
            return false;
        }
        self.used += cost;
        self.lines.push(line);
        true
    }
    /// Like push, but silent overflow: header/footer are mandatory.
    fn push_mandatory(&mut self, line: String) {
        self.used += estimate_tokens(&line) + 1;
        self.lines.push(line);
    }
    fn finish(mut self, narrow_hint: &str) -> String {
        if self.omitted > 0 {
            self.lines.push(format!(
                "… {} more available, narrow by {narrow_hint}",
                self.omitted
            ));
        }
        self.lines
            .push(format!("tokens: ~{} (budget {})", self.used, self.budget));
        self.lines.join("\n")
    }
}

fn symbol_line(s: &SymbolRow) -> String {
    format!(
        "{}@{}:{}-{} [{}] {}",
        s.qualified_name, s.path, s.start_line, s.end_line, s.kind, s.signature
    )
}

fn endpoint_line(e: &EdgeEndpoint) -> String {
    format!(
        "  {}@{}:{}-{} [{}] line {}",
        e.symbol.qualified_name,
        e.symbol.path,
        e.symbol.start_line,
        e.symbol.end_line,
        e.confidence,
        e.line
    )
}

/// Exact + prefix + substring match, ranked; capped by budget.
pub fn find_symbol(
    store: &Store,
    freshness: &Freshness,
    query: &str,
    budget_tokens: Option<usize>,
) -> Result<String, QueryError> {
    let budget = budget_tokens.unwrap_or(DEFAULT_BUDGET);
    let mut b = Budgeter::new(budget);
    b.push_mandatory(freshness.header());
    let (rows, total) = store.find_symbols(query, 200)?;
    if rows.is_empty() {
        b.push_mandatory(format!("find_symbol {query:?}: 0 matches"));
        return Ok(b.finish("qualified name (e.g. Class.method)"));
    }
    b.push_mandatory(format!(
        "find_symbol {query:?}: {} of {} match(es)",
        rows.len().min(200),
        total
    ));
    for row in rows {
        if !b.push(symbol_line(&row)) {
            break;
        }
    }
    Ok(b.finish("qualified name (e.g. Class.method)"))
}

/// Symbols in a file, in source order. `file` may be a suffix of the path.
pub fn outline(
    store: &Store,
    freshness: &Freshness,
    file: &str,
    budget_tokens: Option<usize>,
) -> Result<String, QueryError> {
    let budget = budget_tokens.unwrap_or(DEFAULT_BUDGET);
    let mut b = Budgeter::new(budget);
    b.push_mandatory(freshness.header());
    let rows = store.symbols_in_file(file)?;
    if rows.is_empty() {
        b.push_mandatory(format!(
            "outline {file:?}: 0 indexed symbols (wrong path suffix?)"
        ));
        return Ok(b.finish("longer path suffix"));
    }
    let path = &rows[0].path;
    b.push_mandatory(format!("outline {path}: {} symbol(s)", rows.len()));
    for row in &rows {
        if !b.push(symbol_line(row)) {
            break;
        }
    }
    Ok(b.finish("path suffix + symbol kind"))
}

fn resolve_or_hint(
    store: &Store,
    b: &mut Budgeter,
    name: &str,
) -> Result<Vec<SymbolRow>, QueryError> {
    let rows = store.resolve_symbol(name)?;
    if rows.is_empty() {
        b.push_mandatory(format!("{name:?}: 0 indexed symbols (try find_symbol)"));
        return Ok(vec![]);
    }
    if rows.len() > 8 {
        b.push_mandatory(format!(
            "{name:?}: {} matches; narrowing to first 8 (pass qualified_name):",
            rows.len()
        ));
        return Ok(rows.into_iter().take(8).collect());
    }
    Ok(rows)
}

pub fn callers(
    store: &Store,
    freshness: &Freshness,
    symbol: &str,
    budget_tokens: Option<usize>,
) -> Result<String, QueryError> {
    endpoints(store, freshness, symbol, true, budget_tokens)
}

pub fn callees(
    store: &Store,
    freshness: &Freshness,
    symbol: &str,
    budget_tokens: Option<usize>,
) -> Result<String, QueryError> {
    endpoints(store, freshness, symbol, false, budget_tokens)
}

fn endpoints(
    store: &Store,
    freshness: &Freshness,
    symbol: &str,
    want_callers: bool,
    budget_tokens: Option<usize>,
) -> Result<String, QueryError> {
    let budget = budget_tokens.unwrap_or(DEFAULT_BUDGET);
    let mut b = Budgeter::new(budget);
    b.push_mandatory(freshness.header());
    let direction = if want_callers { "callers" } else { "callees" };
    let rows = resolve_or_hint(store, &mut b, symbol)?;
    for row in rows {
        let eps = if want_callers {
            store.callers(row.id)?
        } else {
            store.callees(row.id)?
        };
        if !b.push(symbol_line(&row)) {
            continue;
        }
        if eps.is_empty() && !b.push(format!("  (no known {direction})")) {
            continue;
        }
        for e in eps {
            if !b.push(endpoint_line(&e)) {
                break;
            }
        }
    }
    Ok(b.finish("qualified_name, or a single file (path suffix)"))
}

/// Compact context for one symbol: signature, docstring, range, and its
/// immediate callers/callees with confidence tags.
pub fn get_context(
    store: &Store,
    freshness: &Freshness,
    symbol: &str,
    budget_tokens: Option<usize>,
) -> Result<String, QueryError> {
    let budget = budget_tokens.unwrap_or(DEFAULT_BUDGET);
    let mut b = Budgeter::new(budget);
    b.push_mandatory(freshness.header());
    let rows = resolve_or_hint(store, &mut b, symbol)?;
    for row in rows {
        b.push_mandatory(symbol_line(&row));
        if let Some(doc) = &row.docstring {
            let _ = b.push(format!("doc: {doc}"));
        }
        let calls = store.callees(row.id)?;
        if !calls.is_empty() {
            let _ = b.push("calls:".to_string());
            for e in calls {
                if !b.push(endpoint_line(&e)) {
                    break;
                }
            }
        }
        let users = store.callers(row.id)?;
        if !users.is_empty() {
            let _ = b.push("callers:".to_string());
            for e in users {
                if !b.push(endpoint_line(&e)) {
                    break;
                }
            }
        }
        b.push_mandatory(format!(
            "escape: {}:{}-{}",
            row.path, row.start_line, row.end_line
        ));
    }
    Ok(b.finish("get_context on one symbol name at a time"))
}

fn short_commit(c: &str) -> String {
    c.chars().take(12).collect()
}

/// Compute freshness from stored meta + HEAD (shared by MCP and CLI).
pub fn freshness(indexed: Option<&str>, head: Option<&str>) -> Freshness {
    match (indexed, head) {
        (Some(i), Some(h)) if i == h => Freshness::Fresh(i.to_string()),
        (Some(i), Some(h)) => Freshness::Stale {
            indexed: i.to_string(),
            head: h.to_string(),
        },
        _ => Freshness::Unknown,
    }
}

impl Freshness {
    /// Dense one-line header, e.g. `index@a1b2c3 fresh`.
    pub fn header(&self) -> String {
        match self {
            Freshness::Fresh(c) => format!("index@{} fresh", short_commit(c)),
            Freshness::Stale { indexed, head } => {
                format!(
                    "index@{} stale (head={})",
                    short_commit(indexed),
                    short_commit(head)
                )
            }
            Freshness::Unknown => "index@unknown".to_string(),
        }
    }
}
