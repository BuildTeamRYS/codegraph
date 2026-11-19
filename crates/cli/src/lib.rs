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
                Some(PathBuf::from(format!(
                    "{}{}",
                    std::env::var_os("HOMEDRIVE")?,
                    std::env::var_os("HOMEPATH")?
                )))
            })
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// The stdio repo for agent configs: the canonical repo path.
pub fn canonical_repo(repo: &Path) -> PathBuf {
    repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf())
}
