//! SQLite-backed store for codegraph. Schema versioning lives in the `meta`
//! table; `schema_version` is the single source of truth (see PLAN.md §3).
//!
//! Rule 5 of the build plan: every edge/fact/finding carries its source,
//! confidence and the content hash it was computed against — hence the
//! `valid_hash` columns on edges/facts/findings (the plan lists edges without
//! one, but rule 5 wins).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use codegraph_core::{CallSite, ContentHash, ImportRecord, SymbolRecord};
pub use rusqlite;
use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = codegraph_core::SCHEMA_VERSION;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration failed at schema version {0}: {1}")]
    Migration(u32, String),
}

pub mod resolve;

pub use resolve::ResolveStats;

pub struct Store {
    conn: Connection,
    pub path: PathBuf,
}

/// Per-run counters. `files_parsed == 0` on a no-op re-index is an
/// acceptance criterion of phase 1, so this is asserted in tests.
#[derive(Debug, Default, Clone)]
pub struct IndexStats {
    pub files_scanned: u64,
    pub files_parsed: u64,
    pub files_skipped: u64,
    pub files_removed: u64,
    pub symbols_written: u64,
    pub gaps: u64,
    pub calls_resolved: u64,
    pub calls_unresolved: u64,
    pub calls_unattributed: u64,
}

const MIGRATION_2: &str = r#"
ALTER TABLE imports ADD COLUMN kind TEXT NOT NULL DEFAULT 'named';
ALTER TABLE imports ADD COLUMN local_name TEXT;
ALTER TABLE imports ADD COLUMN imported_name TEXT;
CREATE TABLE IF NOT EXISTS call_sites (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    caller_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    callee_name TEXT NOT NULL,
    qualifier TEXT,
    is_construct INTEGER NOT NULL DEFAULT 0,
    line INTEGER NOT NULL,
    valid_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_call_sites_file ON call_sites(file_id);
CREATE INDEX IF NOT EXISTS idx_call_sites_name ON call_sites(callee_name);
ALTER TABLE edges ADD COLUMN line INTEGER;
"#;

const MIGRATION_1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    lang TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    size INTEGER NOT NULL,
    indexed_at TEXT NOT NULL,
    is_generated INTEGER NOT NULL DEFAULT 0,
    is_vendor INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    sig TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    docstring TEXT
);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    line INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_id);
CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY,
    src INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    dst INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('calls','imports','defines','inherits','reads','writes','tested_by','handles_route','depends_on')),
    confidence TEXT NOT NULL CHECK (confidence IN ('certain','inferred','unknown')),
    source_tool TEXT NOT NULL,
    valid_hash TEXT
);
CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src, kind);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(dst, kind);
CREATE TABLE IF NOT EXISTS facts (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    confidence TEXT NOT NULL CHECK (confidence IN ('certain','inferred','unknown')),
    source_tool TEXT NOT NULL,
    valid_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_facts_symbol ON facts(symbol_id);
CREATE TABLE IF NOT EXISTS findings (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    severity TEXT NOT NULL,
    confidence TEXT NOT NULL CHECK (confidence IN ('certain','inferred','unknown')),
    source_tool TEXT NOT NULL,
    rule_id TEXT,
    line_start INTEGER,
    line_end INTEGER,
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','fixed','suppressed','regressed')),
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    valid_hash TEXT
);
CREATE INDEX IF NOT EXISTS idx_findings_symbol ON findings(symbol_id);
CREATE TABLE IF NOT EXISTS verdicts (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    check_set TEXT NOT NULL,
    result TEXT NOT NULL,
    by_tool TEXT NOT NULL,
    valid_hash TEXT NOT NULL,
    dep_fingerprint TEXT,
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_verdicts_symbol ON verdicts(symbol_id);
CREATE TABLE IF NOT EXISTS git_stats (
    id INTEGER PRIMARY KEY,
    file_id INTEGER REFERENCES files(id) ON DELETE CASCADE,
    symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    commits_90d INTEGER NOT NULL DEFAULT 0,
    authors INTEGER NOT NULL DEFAULT 0,
    last_change TEXT,
    bugfix_commits INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS cochange (
    file_a INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    file_b INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    score REAL NOT NULL,
    PRIMARY KEY (file_a, file_b)
);
"#;

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                StoreError::Migration(0, format!("cannot create {}: {e}", parent.display()))
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Store {
            conn,
            path: path.to_path_buf(),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Store {
            conn,
            path: PathBuf::from(":memory:"),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let meta_exists: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |r| Ok(r.get::<_, i64>(0)? > 0),
        )?;
        let current: u32 = if meta_exists {
            self.conn
                .query_row(
                    "SELECT value FROM meta WHERE key = 'schema_version'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        } else {
            0
        };
        if current >= SCHEMA_VERSION {
            return Ok(());
        }
        // Migrations are append-only and applied in order inside one
        // transaction; failure rolls everything back so the db is never
        // half-migrated.
        let tx = self.conn.unchecked_transaction()?;
        if current < 1 {
            tx.execute_batch(MIGRATION_1)
                .map_err(|e| StoreError::Migration(1, e.to_string()))?;
        }
        if current < 2 {
            tx.execute_batch(MIGRATION_2)
                .map_err(|e| StoreError::Migration(2, e.to_string()))?;
        }
        tx.execute(
            "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32, StoreError> {
        let v: String = self.conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )?;
        Ok(v.parse().unwrap_or(0))
    }

    /// Raw DDL of every table and index in the store (from sqlite_master).
    /// Used by the schema-hygiene test that scans for language-specific
    /// columns, and by `codegraph status` for diagnostics.
    pub fn schema_ddl(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// path -> (file_id, content_hash) for every indexed file. The indexer
    /// uses this to skip parsing unchanged files.
    pub fn existing_files(&self) -> Result<HashMap<String, (i64, ContentHash)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, id, content_hash FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)?, r.get::<_, String>(2)?),
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (path, v) = row?;
            map.insert(path, v);
        }
        Ok(map)
    }

    /// Replace a file's row and its symbols/imports/call-sites atomically.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_file(
        &self,
        path: &str,
        lang: &str,
        content_hash: &str,
        size: i64,
        indexed_at: &str,
        is_generated: bool,
        is_vendor: bool,
        symbols: &[SymbolRecord],
        imports: &[ImportRecord],
        calls: &[CallSite],
    ) -> Result<u64, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO files(path, lang, content_hash, size, indexed_at, is_generated, is_vendor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(path) DO UPDATE SET
                lang = excluded.lang,
                content_hash = excluded.content_hash,
                size = excluded.size,
                indexed_at = excluded.indexed_at,
                is_generated = excluded.is_generated,
                is_vendor = excluded.is_vendor",
            params![
                path,
                lang,
                content_hash,
                size,
                indexed_at,
                is_generated as i64,
                is_vendor as i64
            ],
        )?;
        let file_id: i64 =
            tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                r.get(0)
            })?;
        tx.execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id])?;
        tx.execute("DELETE FROM imports WHERE file_id = ?1", params![file_id])?;
        tx.execute(
            "DELETE FROM call_sites WHERE file_id = ?1",
            params![file_id],
        )?;
        let mut written = 0u64;
        // (id, start, end, content_hash) per inserted symbol, for call-site attribution.
        let mut inserted: Vec<(i64, u32, u32, String)> = Vec::with_capacity(symbols.len());
        for s in symbols {
            tx.execute(
                "INSERT INTO symbols(file_id, kind, name, qualified_name, sig, start_line, end_line, content_hash, docstring)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![file_id, s.kind.as_str(), s.name, s.qualified_name, s.signature,
                        s.start_line as i64, s.end_line as i64, s.content_hash, s.docstring],
            )?;
            let id = tx.last_insert_rowid();
            inserted.push((id, s.start_line, s.end_line, s.content_hash.clone()));
            written += 1;
        }
        for imp in imports {
            tx.execute(
                "INSERT INTO imports(file_id, source, kind, local_name, imported_name, line) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![file_id, imp.source, imp.kind, imp.local_name, imp.imported_name, imp.line as i64],
            )?;
        }
        // Attribute each call site to its innermost enclosing symbol. Calls
        // at module top level stay unattributed (no edge; counted as
        // unattributed by the resolve pass) — the file's hash is the best
        // validity anchor for them.
        for call in calls {
            let mut best: Option<(i64, u64, &String)> = None;
            for (id, start, end, hash) in &inserted {
                if *start <= call.line && call.line <= *end {
                    let span = (*end - *start) as u64;
                    if best.as_ref().map(|(_, bs, _)| span < *bs).unwrap_or(true) {
                        best = Some((*id, span, hash));
                    }
                }
            }
            let (caller_id, valid_hash) = match &best {
                Some((id, _, hash)) => (Some(*id), hash.to_string()),
                None => (None, content_hash.to_string()),
            };
            tx.execute(
                "INSERT INTO call_sites(file_id, caller_symbol_id, callee_name, qualifier, is_construct, line, valid_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![file_id, caller_id, call.callee_name, call.qualifier,
                        call.is_construct as i64, call.line as i64, valid_hash],
            )?;
        }
        tx.commit()?;
        Ok(written)
    }

    /// Remove files under `seen_paths` absent from the index (deletions).
    pub fn remove_files_not_in(
        &self,
        seen_paths: &std::collections::HashSet<String>,
    ) -> Result<u64, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let existing: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM files")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut removed = 0u64;
        for p in existing {
            if !seen_paths.contains(&p) {
                tx.execute("DELETE FROM files WHERE path = ?1", params![p])?;
                removed += 1;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn record_gap(&self, path: &str, error: &str) -> Result<(), StoreError> {
        self.set_meta(&format!("gap:{path}"), error)
    }

    pub fn gaps(&self) -> Result<Vec<(String, String)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM meta WHERE key LIKE 'gap:%' ORDER BY key")?;
        let rows = stmt.query_map([], |r| {
            let k: String = r.get(0)?;
            Ok((
                k.trim_start_matches("gap:").to_string(),
                r.get::<_, String>(1)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn clear_gap(&self, path: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM meta WHERE key = ?1",
            params![format!("gap:{path}")],
        )?;
        Ok(())
    }

    pub fn symbol_count(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?)
    }

    pub fn file_count(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?)
    }
}

/// Row shapes for query consumers (phase 2). Paths are repo-relative.
#[derive(Debug, Clone)]
pub struct SymbolRow {
    pub id: i64,
    pub file_id: i64,
    pub path: String,
    pub kind: String,
    pub name: String,
    pub qualified_name: String,
    pub signature: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content_hash: String,
    pub docstring: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EdgeEndpoint {
    pub symbol: SymbolRow,
    pub confidence: String,
    pub line: i64,
}

impl Store {
    pub fn find_symbols(
        &self,
        needle: &str,
        limit: usize,
    ) -> Result<(Vec<SymbolRow>, usize), StoreError> {
        // Ranked: exact (0) > prefix (1) > substring (2), then path, then position.
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.file_id, f.path, s.kind, s.name, s.qualified_name, s.sig,
                    s.start_line, s.end_line, s.content_hash, s.docstring
             FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.name LIKE '%' || ?1 || '%' OR s.qualified_name LIKE '%' || ?1 || '%'
             ORDER BY LENGTH(s.name), f.path, s.start_line",
        )?;
        let rows = stmt
            .query_map(params![needle], map_symbol_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let total = rows.len();
        let needle_l = needle.to_lowercase();
        let mut ranked: Vec<(u8, SymbolRow)> = rows
            .into_iter()
            .map(|r| {
                let n = r.name.to_lowercase();
                let q = r.qualified_name.to_lowercase();
                let tier = if n == needle_l || q == needle_l {
                    0
                } else if n.starts_with(&needle_l) || q.starts_with(&needle_l) {
                    1
                } else {
                    2
                };
                (tier, r)
            })
            .collect();
        ranked.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.path.cmp(&b.1.path))
                .then(a.1.start_line.cmp(&b.1.start_line))
        });
        let out: Vec<SymbolRow> = ranked.into_iter().take(limit).map(|(_, r)| r).collect();
        Ok((out, total))
    }

    /// Suffix match on path, longest match wins first.
    pub fn symbols_in_file(&self, path_suffix: &str) -> Result<Vec<SymbolRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.file_id, f.path, s.kind, s.name, s.qualified_name, s.sig,
                    s.start_line, s.end_line, s.content_hash, s.docstring
             FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE f.path = ?1 OR f.path LIKE '%/' || ?1
             ORDER BY s.start_line",
        )?;
        let rows = stmt
            .query_map(params![path_suffix], map_symbol_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn resolve_symbol(&self, name: &str) -> Result<Vec<SymbolRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.file_id, f.path, s.kind, s.name, s.qualified_name, s.sig,
                    s.start_line, s.end_line, s.content_hash, s.docstring
             FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.name = ?1 OR s.qualified_name = ?1
             ORDER BY f.path, s.start_line",
        )?;
        let rows = stmt
            .query_map(params![name], map_symbol_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn callees(&self, symbol_id: i64) -> Result<Vec<EdgeEndpoint>, StoreError> {
        self.edge_endpoints(
            "SELECT dst AS other, confidence, line FROM edges WHERE src = ?1 AND kind = 'calls' ORDER BY line",
            symbol_id,
        )
    }

    pub fn callers(&self, symbol_id: i64) -> Result<Vec<EdgeEndpoint>, StoreError> {
        self.edge_endpoints(
            "SELECT src AS other, confidence, line FROM edges WHERE dst = ?1 AND kind = 'calls' ORDER BY line",
            symbol_id,
        )
    }

    fn edge_endpoints(&self, sql: &str, symbol_id: i64) -> Result<Vec<EdgeEndpoint>, StoreError> {
        let mut stmt = self.conn.prepare(sql)?;
        let edges: Vec<(i64, String, i64)> = stmt
            .query_map(params![symbol_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(edges.len());
        for (other, confidence, line) in edges {
            let row = self.conn.query_row(
                "SELECT s.id, s.file_id, f.path, s.kind, s.name, s.qualified_name, s.sig,
                        s.start_line, s.end_line, s.content_hash, s.docstring
                 FROM symbols s JOIN files f ON f.id = s.file_id WHERE s.id = ?1",
                params![other],
                map_symbol_row,
            )?;
            out.push(EdgeEndpoint {
                symbol: row,
                confidence,
                line,
            });
        }
        out.sort_by(|a, b| {
            a.symbol
                .path
                .cmp(&b.symbol.path)
                .then(a.symbol.start_line.cmp(&b.symbol.start_line))
        });
        Ok(out)
    }

    pub fn symbol_by_id(&self, id: i64) -> Result<Option<SymbolRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT s.id, s.file_id, f.path, s.kind, s.name, s.qualified_name, s.sig,
                        s.start_line, s.end_line, s.content_hash, s.docstring
                 FROM symbols s JOIN files f ON f.id = s.file_id WHERE s.id = ?1",
                params![id],
                map_symbol_row,
            )
            .optional()?)
    }
}

fn map_symbol_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolRow> {
    Ok(SymbolRow {
        id: r.get(0)?,
        file_id: r.get(1)?,
        path: r.get(2)?,
        kind: r.get(3)?,
        name: r.get(4)?,
        qualified_name: r.get(5)?,
        signature: r.get(6)?,
        start_line: r.get::<_, i64>(7)? as u32,
        end_line: r.get::<_, i64>(8)? as u32,
        content_hash: r.get(9)?,
        docstring: r.get(10)?,
    })
}
