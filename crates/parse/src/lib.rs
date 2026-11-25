//! Language-agnostic parsing driver and the `LanguagePlugin` trait.
//!
//! The core schema (files/symbols/imports/call_sites/...) carries no
//! language-specific columns; each language is a plugin behind this trait
//! (see `codegraph-lang-ts` for TypeScript/JavaScript). PLAN.md section 2 and
//! DECISIONS.md (2026-11-18 | architecture) forbid TS-specific knowledge in
//! this crate beyond the shared helpers below.
//!
//! What this crate owns:
//! - the walk (`ignore` crate: respects .gitignore/.git/global ignores, keeps
//!   hidden files, never follows symlinks),
//! - hash-based incremental skips,
//! - vendor/generated classification helpers shared by plugins,
//! - the `FileOutcome`/`ScanOutput` driver types.
//!
//! What plugins own: grammar selection, extraction rules, docstrings,
//! signatures, call-site recording.
//!
//! Files whose parser reports errors are still returned as `Parsed` with
//! `had_errors = true`; the caller records an indexing gap (never silently
//! complete). Files that cannot be read are `Failed`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use codegraph_core::{CallSite, ImportRecord, SymbolRecord};
use ignore::WalkBuilder;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unreadable file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{0}")]
    Parser(String),
}

/// One parsed file, ready for the store.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub path: String,
    pub lang: String,
    pub content_hash: String,
    pub size: i64,
    pub is_generated: bool,
    pub is_vendor: bool,
    pub symbols: Vec<SymbolRecord>,
    pub imports: Vec<ImportRecord>,
    pub calls: Vec<CallSite>,
    /// True if the parser reported syntax errors; file indexed best-effort.
    pub had_errors: bool,
}

#[derive(Debug)]
pub enum FileOutcome {
    /// Content hash matched the index; no parsing was done.
    Unchanged {
        path: String,
    },
    Parsed(ParsedFile),
    /// File could not be read at all; caller records a gap.
    Failed {
        path: String,
        error: String,
    },
}

#[derive(Debug, Default)]
pub struct ScanOutput {
    pub outcomes: Vec<FileOutcome>,
    pub seen_paths: HashSet<String>,
    pub scanned: u64,
}

/// A framework entry point detected in a file (route handler, CLI command,
/// job). Framework plugins contribute these from phase 11 on; the built-in
/// language plugins detect nothing and return an empty vec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    /// e.g. `http_route`, `cli_command`, `job`.
    pub kind: String,
    /// Qualified name of the handler symbol.
    pub symbol: String,
    /// 1-based line of the handler definition.
    pub line: u32,
}

/// Vendor directory heuristic shared by plugins. Paths use forward slashes.
pub fn is_vendor_path(rel_path: &str) -> bool {
    const VENDOR_DIRS: &[&str] = &[
        "node_modules",
        "vendor",
        "third_party",
        "dist",
        "build",
        "out",
    ];
    rel_path.split('/').any(|c| VENDOR_DIRS.contains(&c))
}

/// Returns true if this path should be completely skipped during indexing.
/// This prevents massive bloat and parser errors from auto-generated,
/// third-party code, or secrets that the agent doesn't need to reason about.
pub fn should_skip_indexing(rel_path: &str) -> bool {
    // 1. Explicit vendor, build, and cache directories
    const SKIP_DIRS: &[&str] = &[
        "node_modules",
        "vendor",
        "third_party",
        "dist",
        "build",
        "out",
        ".cache",
        ".next",
        ".nuxt",
        ".svelte-kit",
        "coverage",
        ".git",
        ".svn",
        ".idea",
        ".vscode-test",
    ];

    if rel_path.split('/').any(|c| SKIP_DIRS.contains(&c)) {
        return true;
    }

    // 2. Secret files and lockfiles (no need to index these for a code graph)
    let name = std::path::Path::new(rel_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if name == ".env"
        || name.starts_with(".env.")
        || name.starts_with(".env_")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".crt")
        || name == "yarn.lock"
        || name == "package-lock.json"
        || name == "pnpm-lock.yaml"
        || name == "bun.lockb"
        || name == "bun.lock"
    {
        return true;
    }

    // 3. Auto-generated files
    if name.ends_with(".d.ts")
        || name.contains(".generated.")
        || name.contains(".g.ts")
        || name.contains(".min.js")
        || name.contains(".min.ts")
    {
        return true;
    }

    // 4. Hidden files/directories (except well-known config dirs that agents might need)
    let first_component = rel_path.split('/').next().unwrap_or("");
    if first_component.starts_with('.')
        && first_component != ".github"
        && first_component != ".vscode"
        && first_component != ".cursor"
        && first_component != ".claude"
        && first_component != ".codegraph"
    {
        return true;
    }

    false
}

/// One language (or one JS family dialect) behind a single trait.
///
/// Implementations must be cheap, deterministic and offline. The core never
/// sees grammar types.
pub trait LanguagePlugin {
    /// Stable lowercase language id stored in `files.lang` for `rel_path`
    /// (e.g. `typescript`, `tsx`, `javascript`, `jsx`), or None for paths
    /// this plugin does not handle. Repos use forward slashes.
    fn language_for(&self, rel_path: &str) -> Option<&'static str>;

    /// `(is_generated, is_vendor)` for a repo-relative path. Defaults to the
    /// shared vendor-dir heuristic; language plugins extend it (e.g. the TS
    /// plugin flags `.d.ts` and `*.min.*` as generated).
    fn classify(&self, rel_path: &str) -> (bool, bool) {
        (false, is_vendor_path(rel_path))
    }

    /// Extract one file. `rel_path` uses forward slashes relative to the
    /// scanned root; `bytes` is the raw file content.
    fn extract(&self, rel_path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError>;

    /// Framework entry points in this file (phase 11). Default: none.
    fn detect_entry_points(&self, _parsed: &ParsedFile) -> Vec<EntryPoint> {
        Vec::new()
    }
}

/// Walk `root`, skipping unchanged files (same content hash in `existing`),
/// parsing everything else with the first matching plugin (by path order in
/// `plugins`). Returns outcomes plus the set of seen repo-relative paths.
pub fn scan(
    root: &Path,
    existing: &HashMap<String, (i64, String)>,
    plugins: &[&dyn LanguagePlugin],
) -> Result<ScanOutput, ParseError> {
    let root = root.canonicalize().map_err(|e| ParseError::Io {
        path: root.display().to_string(),
        source: e,
    })?;
    let mut output = ScanOutput::default();

    let mut builder = WalkBuilder::new(&root);
    builder
        .standard_filters(true) // respects .gitignore and global ignores
        .hidden(false) // hidden config files are still source; .git is always skipped
        .follow_links(false)
        .sort_by_file_path(|a, b| a.cmp(b));

    // Explicitly respect .dockerignore if it exists in the repo root
    let dockerignore = root.join(".dockerignore");
    if dockerignore.exists() {
        builder.add_ignore(dockerignore);
    }

    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                // Silently ignore walk errors (e.g., permission denied on a system folder)
                continue;
            }
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = repo_relative(&root, path);

        // HARD SKIP: Prevent indexing of vendor, cache, secrets, and generated files.
        // This acts as a safety net even if .gitignore is missing or misconfigured.
        if should_skip_indexing(&rel) {
            continue;
        }

        let plugin = plugins
            .iter()
            .find_map(|p| p.language_for(&rel).map(|_| *p));
        let Some(plugin) = plugin else { continue };

        output.scanned += 1;
        output.seen_paths.insert(rel.clone());

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(source) => {
                output.outcomes.push(FileOutcome::Failed {
                    path: rel,
                    error: source.to_string(),
                });
                continue;
            }
        };
        let content_hash = codegraph_core::hash_bytes(&bytes);
        if let Some((_id, existing_hash)) = existing.get(&rel) {
            if *existing_hash == content_hash {
                output.outcomes.push(FileOutcome::Unchanged { path: rel });
                continue;
            }
        }
        match plugin.extract(&rel, &bytes) {
            Ok(parsed) => output.outcomes.push(FileOutcome::Parsed(parsed)),
            Err(e) => output.outcomes.push(FileOutcome::Failed {
                path: rel,
                error: e.to_string(),
            }),
        }
    }
    Ok(output)
}

/// Repo-relative path with forward slashes, stable across platforms.
fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
