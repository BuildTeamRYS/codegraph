//! Backup + atomic write + byte-for-byte restore.
//!
//! Every file we modify gets one pristine backup (`<file>.codegraph.bak`,
//! created once and never overwritten) and a small state sidecar
//! (`<file>.codegraph.state.json`) recording the SHA-256 of the file *after*
//! our write. On `agents remove`:
//!
//! - if the file's current SHA-256 still equals the recorded post-write
//!   hash (nothing else changed it since), we restore the backup bytes
//!   exactly — or delete the file if we created it;
//! - otherwise we surgically remove just our entry and keep the user's
//!   later edits.
//!
//! This is what `codegraph agents remove <id>` means by "restores the
//! original config byte for byte" while never clobbering a hand-edited file.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    /// SHA-256 of the file after codegraph wrote it.
    pub sha256_after: String,
    /// True when codegraph created the file (no prior backup exists).
    pub created: bool,
}

pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".codegraph.bak");
    PathBuf::from(name)
}

pub fn state_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".codegraph.state.json");
    PathBuf::from(name)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Read the recorded state, if any.
pub fn read_state(file: &Path) -> Option<State> {
    let raw = fs::read_to_string(state_path(file)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Back up `file` (once) and write state after a successful write.
/// `bytes` is the content that will be written by the caller right after.
pub fn begin_write(file: &Path) -> Result<Option<PathBuf>> {
    let backup = backup_path(file);
    if file.exists() && !backup.exists() {
        fs::copy(file, &backup).with_context(|| {
            format!("cannot back up {} to {}", file.display(), backup.display())
        })?;
        return Ok(Some(backup));
    }
    Ok(None)
}

pub fn finish_write(file: &Path, bytes: &[u8], created: bool) -> Result<()> {
    let state = State {
        version: 1,
        sha256_after: sha256_hex(bytes),
        created,
    };
    let path = state_path(file);
    fs::write(&path, serde_json::to_string_pretty(&state)?)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

/// Write `bytes` to `file` atomically: temp file in the same directory,
/// fsync, rename. Creates parent directories.
pub fn atomic_write(file: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let tmp = file.with_extension(format!("codegraph.tmp-{}", std::process::id()));
    fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    if let Ok(f) = fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    fs::rename(&tmp, file).with_context(|| {
        let _ = fs::remove_file(&tmp);
        format!("cannot rename {} to {}", tmp.display(), file.display())
    })?;
    Ok(())
}

/// What happened on removal, for user-facing reporting.
#[derive(Debug, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// File was byte-identical to our post-write state; original restored
    /// (or the file was deleted because we created it).
    Restored,
    /// File had other edits since our write; only our entry was removed.
    Surgical,
    /// No backup/state found; caller must fall back to surgical edit.
    NoState,
}

/// Try to restore the file to its exact pre-codegraph bytes.
/// Returns `NoState` when the manifest cannot decide (caller falls back).
pub fn try_restore(file: &Path) -> Result<RestoreOutcome> {
    let current = fs::read(file).unwrap_or_default();
    let Some(state) = read_state(file) else {
        return Ok(RestoreOutcome::NoState);
    };
    if sha256_hex(&current) != state.sha256_after {
        return Ok(RestoreOutcome::Surgical);
    }
    if state.created {
        fs::remove_file(file).with_context(|| format!("cannot remove {}", file.display()))?;
    } else {
        let backup = backup_path(file);
        let bytes = fs::read(&backup).with_context(|| {
            format!(
                "recorded state for {} but {} is missing; edit the file to remove the codegraph entry manually",
                file.display(),
                backup.display()
            )
        })?;
        atomic_write(file, &bytes)?;
    }
    let _ = fs::remove_file(state_path(file));
    Ok(RestoreOutcome::Restored)
}
