//! Orchestration shared by the `codegraph` binary and its integration tests.

pub mod setup;
pub mod update;

use std::path::{Path, PathBuf};

pub use codegraph_core::git_head_commit;
pub use codegraph_index::{default_db_path, index_repo};

/// Re-exported for CLI/tests that construct freshness headers.
pub use codegraph_query::{freshness, Freshness};

/// Resolve the repo root for `path`:
/// 1. explicit argument wins;
/// 2. `CODEGRAPH_PROJECT` environment variable;
/// 3. walk up from cwd to the nearest `.git` (monorepo-friendly);
/// 4. the cwd itself.
pub fn resolve_repo(path: Option<&Path>) -> PathBuf {
    if let Some(p) = path {
        return p.to_path_buf();
    }
    if let Some(p) = std::env::var_os("CODEGRAPH_PROJECT").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return cwd,
        }
    }
}

pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                // HOMEDRIVE + HOMEPATH is legacy belt-and-braces; USERPROFILE
                // is virtually always set on real Windows. OsString::push
                // concatenates raw bytes — OsString has no Display impl and
                // paths may not be UTF-8, so no format!().
                let mut buf = std::ffi::OsString::from(std::env::var_os("HOMEDRIVE")?);
                buf.push(std::env::var_os("HOMEPATH")?);
                Some(PathBuf::from(buf))
            })
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// The stdio repo for agent configs: the canonical repo path.
///
/// On Windows, `canonicalize()` returns extended-length paths prefixed
/// with `\\?\` (or `\\?\UNC\` for UNC shares). That prefix is understood by
/// the Rust/Windows APIs but confuses some agent tool stacks and JSON
/// tooling, and it leaks into config files and prompt fill contexts, so we
/// strip it back to a plain absolute path.
pub fn canonical_repo(repo: &Path) -> PathBuf {
    let canon = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    #[cfg(windows)]
    {
        let s = canon.to_string_lossy().into_owned();
        if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        canon
    }
    #[cfg(not(windows))]
    canon
}
