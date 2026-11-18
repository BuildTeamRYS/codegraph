//! Syntactic call-graph resolution (phase 2). Turns raw `call_sites` into
//! `edges(kind='calls', confidence='inferred', source_tool='tree-sitter')`.
//!
//! Resolution is deliberately conservative; anything ambiguous stays
//! unresolved rather than being guessed (rule: never invent facts).
//! Full re-resolution after every index pass; the incremental
//! re-resolution optimization lands with phase 3.

use std::collections::{HashMap, HashSet};

use crate::{Store, StoreError};

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolveStats {
    pub resolved: u64,
    pub unresolved: u64,
    /// Call sites outside any symbol (module top level). Counted, no edge.
    pub unattributed: u64,
}

#[derive(Debug, Clone)]
struct SymbolEntry {
    id: i64,
    file_id: i64,
    name: String,
    qualified_name: String,
    kind: String,
    start: i64,
    end: i64,
}

fn normalize_join(dir: &str, source: &str) -> String {
    let joined = if dir.is_empty() {
        source.to_string()
    } else {
        format!("{dir}/{source}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for comp in joined.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    parts.join("/")
}

fn module_candidates(base: &str) -> Vec<String> {
    let exts = ["ts", "tsx", "mts", "cts"];
    let mut out = Vec::new();
    if base.rsplit('/').next().is_some_and(|f| f.contains('.')) {
        out.push(base.to_string());
        return out;
    }
    for e in exts {
        out.push(format!("{base}.{e}"));
    }
    for e in exts {
        out.push(format!("{base}/index.{e}"));
    }
    out
}

struct CallSiteRow {
    file_id: i64,
    file_path: String,
    caller_symbol_id: Option<i64>,
    callee_name: String,
    qualifier: Option<String>,
    is_construct: bool,
    line: i64,
    valid_hash: String,
}

struct Binding {
    kind: String,
    local_name: Option<String>,
    imported_name: Option<String>,
    source: String,
}

impl Store {
    /// Rebuild all `calls` edges from the current call sites and import
    /// bindings. Safe to call repeatedly; runs inside one transaction.
    pub fn resolve_calls(&self) -> Result<ResolveStats, StoreError> {
        let tx = self.conn.unchecked_transaction()?;

        let file_paths: HashMap<i64, String> = {
            let mut stmt = tx.prepare("SELECT id, path FROM files")?;
            let rows: Vec<(i64, String)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().collect::<HashMap<_, _>>()
        };
        let known_paths: HashSet<String> = file_paths.values().cloned().collect();

        let symbols: Vec<SymbolEntry> = {
            let mut stmt = tx.prepare(
                "SELECT id, file_id, name, qualified_name, kind, start_line, end_line FROM symbols",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(SymbolEntry {
                    id: r.get(0)?,
                    file_id: r.get(1)?,
                    name: r.get(2)?,
                    qualified_name: r.get(3)?,
                    kind: r.get(4)?,
                    start: r.get(5)?,
                    end: r.get(6)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut by_name: HashMap<&str, Vec<&SymbolEntry>> = HashMap::new();
        let mut by_file: HashMap<i64, Vec<&SymbolEntry>> = HashMap::new();
        for s in &symbols {
            by_name.entry(s.name.as_str()).or_default().push(s);
            by_file.entry(s.file_id).or_default().push(s);
        }

        let bindings: HashMap<i64, Vec<Binding>> = {
            let mut stmt =
                tx.prepare("SELECT file_id, kind, local_name, imported_name, source FROM imports")?;
            let mut map: HashMap<i64, Vec<Binding>> = HashMap::new();
            for row in stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Binding {
                        kind: r.get(1)?,
                        local_name: r.get(2)?,
                        imported_name: r.get(3)?,
                        source: r.get(4)?,
                    },
                ))
            })? {
                let (file_id, b) = row?;
                map.entry(file_id).or_default().push(b);
            }
            map
        };

        let call_sites: Vec<CallSiteRow> = {
            let mut stmt = tx.prepare(
                "SELECT cs.file_id, f.path, cs.caller_symbol_id, cs.callee_name, cs.qualifier,
                        cs.is_construct, cs.line, cs.valid_hash
                 FROM call_sites cs JOIN files f ON f.id = cs.file_id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(CallSiteRow {
                    file_id: r.get(0)?,
                    file_path: r.get(1)?,
                    caller_symbol_id: r.get(2)?,
                    callee_name: r.get(3)?,
                    qualifier: r.get(4)?,
                    is_construct: r.get::<_, i64>(5)? != 0,
                    line: r.get(6)?,
                    valid_hash: r.get(7)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // Resolve a module specifier relative to an importing file to a
        // known file path in the index.
        let resolve_module = |importer_path: &str, source: &str| -> Option<String> {
            if !source.starts_with('.') {
                return None; // bare package: not an indexed file
            }
            let dir = importer_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let base = normalize_join(dir, source);
            module_candidates(&base)
                .into_iter()
                .find(|c| known_paths.contains(c))
        };

        let kind_ok = |sym: &SymbolEntry, is_construct: bool| -> bool {
            if is_construct {
                sym.kind == "class"
            } else {
                true
            }
        };

        tx.execute("DELETE FROM edges WHERE kind = 'calls'", [])?;

        let mut stats = ResolveStats::default();
        let insert =
            |caller: i64, callee: i64, line: i64, valid_hash: &str| -> Result<(), StoreError> {
                tx.execute(
                    "INSERT INTO edges(src, dst, kind, confidence, source_tool, valid_hash, line)
                 VALUES (?1, ?2, 'calls', 'inferred', 'tree-sitter', ?3, ?4)",
                    rusqlite::params![caller, callee, valid_hash, line],
                )?;
                Ok(())
            };

        for cs in &call_sites {
            let Some(caller_id) = cs.caller_symbol_id else {
                stats.unattributed += 1;
                continue;
            };
            let bindings_here = bindings.get(&cs.file_id);
            let mut resolved_id: Option<i64> = None;

            // Rule 1: `this.foo(...)` → method `foo` of the class enclosing the call line.
            if cs.qualifier.as_deref() == Some("this") {
                let class = by_file.get(&cs.file_id).map(|syms| {
                    syms.iter()
                        .filter(|s| s.kind == "class" && s.start <= cs.line && cs.line <= s.end)
                        .min_by_key(|s| s.end - s.start)
                        .map(|c| c.name.clone())
                });
                if let Some(Some(class_name)) = class {
                    resolved_id = by_file
                        .get(&cs.file_id)
                        .and_then(|syms| {
                            syms.iter().find(|s| {
                                s.kind == "method"
                                    && s.name == cs.callee_name
                                    && s.qualified_name
                                        == format!("{class_name}.{}", cs.callee_name)
                            })
                        })
                        .map(|s| s.id);
                }
            }
            // Rule 2: member call on a namespace / named-import binding.
            else if let Some(q) = cs.qualifier.as_deref() {
                if let Some(bs) = bindings_here {
                    if let Some(binding) = bs.iter().find(|b| b.local_name.as_deref() == Some(q)) {
                        let target_path = resolve_module(&cs.file_path, &binding.source);
                        if let Some(target_path) = target_path {
                            let target_file_id = file_paths
                                .iter()
                                .find(|(_, p)| **p == target_path)
                                .map(|(id, _)| *id);
                            if let Some(tid) = target_file_id {
                                if binding.kind == "namespace" {
                                    resolved_id = by_file.get(&tid).and_then(|syms| {
                                        syms.iter()
                                            .find(|s| {
                                                s.name == cs.callee_name
                                                    && kind_ok(s, cs.is_construct)
                                            })
                                            .map(|s| s.id)
                                    });
                                }
                            }
                        }
                    }
                }
            }
            // Rule 3: plain call matching an imported binding (`import { hash_pw }`).
            if resolved_id.is_none() && cs.qualifier.is_none() {
                if let Some(bs) = bindings_here {
                    if let Some(binding) = bs
                        .iter()
                        .find(|b| b.local_name.as_deref() == Some(cs.callee_name.as_str()))
                    {
                        if let Some(target_path) = resolve_module(&cs.file_path, &binding.source) {
                            let wanted = binding
                                .imported_name
                                .as_deref()
                                .unwrap_or(cs.callee_name.as_str());
                            let target_file_id = file_paths
                                .iter()
                                .find(|(_, p)| **p == target_path)
                                .map(|(id, _)| *id);
                            if let Some(tid) = target_file_id {
                                resolved_id = by_file.get(&tid).and_then(|syms| {
                                    syms.iter()
                                        .find(|s| s.name == wanted && kind_ok(s, cs.is_construct))
                                        .map(|s| s.id)
                                });
                            }
                        }
                    }
                }
            }
            // Rule 4: same file, unique simple name match.
            if resolved_id.is_none() {
                let same_file: Vec<&SymbolEntry> = by_file
                    .get(&cs.file_id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| s.name == cs.callee_name && kind_ok(s, cs.is_construct))
                    .collect();
                resolved_id = match same_file.as_slice() {
                    [one] => Some(one.id),
                    many if many.len() > 1 => many
                        .iter()
                        .find(|s| s.start <= cs.line && cs.line <= s.end)
                        .map(|s| s.id),
                    _ => None,
                };
            }
            // Rule 5: globally unique simple name.
            if resolved_id.is_none() {
                let global: Vec<&SymbolEntry> = by_name
                    .get(cs.callee_name.as_str())
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| kind_ok(s, cs.is_construct))
                    .collect();
                if global.len() == 1 {
                    resolved_id = Some(global[0].id);
                }
            }

            match resolved_id {
                Some(callee_id) => {
                    insert(caller_id, callee_id, cs.line, &cs.valid_hash)?;
                    stats.resolved += 1;
                }
                None => stats.unresolved += 1,
            }
        }

        tx.execute(
            "INSERT INTO meta(key, value) VALUES ('calls_resolved', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![stats.resolved.to_string()],
        )?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES ('calls_unresolved', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![stats.unresolved.to_string()],
        )?;
        tx.commit()?;
        Ok(stats)
    }
}
