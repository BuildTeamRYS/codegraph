//! Orchestration shared by the `codegraph` binary and its integration tests.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use codegraph_parse::{scan, FileOutcome};
use codegraph_store::{IndexStats, Store};

/// Default index location: `<repo>/.codegraph/index.db` (gitignored).
pub fn default_db_path(repo: &Path) -> PathBuf {
    repo.join(".codegraph").join("index.db")
}

pub fn git_head_commit(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn now_rfc3339() -> String {
    // No chrono dependency in phase 0; seconds precision is enough for meta.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

/// Index `root` into `store`. Only files whose content hash changed are
/// re-parsed; everything else is skipped. Returns per-run stats.
pub fn index_repo(store: &Store, root: &Path) -> Result<IndexStats> {
    let mut stats = IndexStats::default();
    let existing = store.existing_files()?;
    let scan = scan(root, &existing).context("failed to scan repository")?;
    stats.files_scanned = scan.scanned;

    for outcome in &scan.outcomes {
        match outcome {
            FileOutcome::Unchanged { .. } => stats.files_skipped += 1,
            FileOutcome::Parsed(p) => {
                stats.files_parsed += 1;
                let written = store.replace_file(
                    &p.path,
                    &p.lang,
                    &p.content_hash,
                    p.size,
                    &now_rfc3339(),
                    p.is_generated,
                    p.is_vendor,
                    &p.symbols,
                    &p.imports,
                )?;
                stats.symbols_written += written;
                if p.had_errors {
                    store.record_gap(
                        &p.path,
                        "tree-sitter reported syntax errors; file indexed best-effort",
                    )?;
                    stats.gaps += 1;
                } else {
                    store.clear_gap(&p.path)?;
                }
            }
            FileOutcome::Failed { path, error } => {
                store.record_gap(path, error)?;
                stats.gaps += 1;
            }
        }
    }

    let seen: HashSet<String> = scan.seen_paths;
    stats.files_removed = store.remove_files_not_in(&seen)?;

    match git_head_commit(root) {
        Some(c) => store.set_meta("indexed_commit", &c)?,
        None => store.set_meta("indexed_commit", "unknown")?,
    }
    store.set_meta("indexed_at", &now_rfc3339())?;
    Ok(stats)
}
