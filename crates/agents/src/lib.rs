//! `codegraph-agents`: a data-driven adapter table for coding agents and
//! the operations `codegraph init` / `codegraph agents` build on.
//!
//! One entry per agent lives in [`spec::AGENTS`]; everything agent-specific
//! is data there. The editing engine ([`edit`]) is format-aware
//! (JSON/JSONC/TOML), always shows a diff before writing, backs up once,
//! writes atomically, and never touches a file it cannot parse.
//! `agents remove` restores the original bytes when nothing else changed.
//!
//! Not in this crate: MCP handshake checks and agent-process spawning
//! (that lives in the CLI/`codegraph-mcp`), and query-log/stats (phase 2.6).

pub mod diff;
pub mod edit;
pub mod instructions;
pub mod manifest;
pub mod spec;

pub use diff::line_diff;
pub use manifest::{RestoreOutcome, State};
pub use spec::{
    config_path, expand, find, supported_scopes, AgentSpec, ConfigFormat, PathBase, Scope, AGENTS,
};

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// Detection result for one agent in one scope.
#[derive(Debug, Clone)]
pub struct Detection {
    pub spec: &'static AgentSpec,
    pub scope: Scope,
    pub config_path: PathBuf,
    /// Human-readable evidence: binaries on PATH, markers, config file.
    pub evidence: Vec<String>,
    pub existed: bool,
    /// A codegraph entry is already present in the config.
    pub already_configured: bool,
    /// False when the agent does not support this scope.
    pub scope_supported: bool,
}

fn binary_on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}

/// Detect agents to configure. `ids` restricts to specific agents (empty =
/// all). Detection never writes anything.
pub fn detect(repo: &Path, home: &Path, scope: Scope, ids: &[&str]) -> Vec<Detection> {
    let mut out = Vec::new();
    for spec in AGENTS {
        if !ids.is_empty() && !ids.contains(&spec.id) {
            continue;
        }
        let Some(path) = config_path(spec, scope, repo, home) else {
            // Agent does not support this scope; report it so callers can
            // fall back to the other scope with a clear message.
            out.push(Detection {
                spec,
                scope,
                config_path: PathBuf::new(),
                evidence: Vec::new(),
                existed: false,
                already_configured: false,
                scope_supported: false,
            });
            continue;
        };
        let mut evidence = Vec::new();
        for bin in spec.detect_binaries {
            if binary_on_path(bin) {
                evidence.push(format!("`{bin}` on PATH"));
            }
        }
        for marker in spec.detect_markers_repo {
            if repo.join(marker).exists() {
                evidence.push(format!("{marker} exists in repo"));
            }
        }
        let existed = path.exists();
        if existed {
            evidence.push(format!("config file {} exists", path.display()));
        }
        if evidence.is_empty() {
            continue;
        }
        let already_configured = fs::read_to_string(&path)
            .ok()
            .and_then(|t| edit::read_entry(&t, spec).ok().flatten())
            .is_some();
        if already_configured {
            evidence.push("codegraph entry already present".into());
        }
        out.push(Detection {
            spec,
            scope,
            config_path: path,
            evidence,
            existed,
            already_configured,
            scope_supported: true,
        });
    }
    out
}

/// One file change planned by [`plan`] (the config file, or the instruction file).
#[derive(Debug, Clone)]
pub struct FilePlan {
    pub path: PathBuf,
    /// None when the file does not exist and will be created.
    pub old_text: Option<String>,
    pub new_text: String,
    pub diff: String,
}

/// A planned change for one agent.
#[derive(Debug, Clone)]
pub struct Change {
    pub detection: Detection,
    pub config: FilePlan,
    /// Set when the instruction block is (or would be) updated.
    pub instructions: Option<FilePlan>,
    pub note: Option<&'static str>,
}

impl Change {
    pub fn is_noop(&self) -> bool {
        self.config.old_text.as_deref() == Some(self.config.new_text.as_str())
            && self
                .instructions
                .as_ref()
                .map(|i| i.old_text.as_deref() == Some(i.new_text.as_str()))
                .unwrap_or(true)
    }
}

/// The absolute codegraph command line agents will run. Uses the stable
/// shim path (never the build-directory binary).
pub fn shim_path(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"));
        base.join("codegraph").join("bin").join("codegraph.exe")
    }
    #[cfg(not(windows))]
    {
        home.join(".codegraph").join("bin").join("codegraph")
    }
}

/// Create the shim by copying the canonical binary, if missing.
/// Returns `(path, created)`.
pub fn ensure_shim(home: &Path, canonical: &Path) -> Result<(PathBuf, bool)> {
    let path = shim_path(home);
    if path.exists() {
        return Ok((path, false));
    }
    let parent = path.parent().context("shim path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    fs::copy(canonical, &path).with_context(|| {
        format!(
            "cannot copy {} to shim {}",
            canonical.display(),
            path.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }
    Ok((path, true))
}

/// Arguments agents run: `serve <repo>`.
pub fn serve_args(repo: &Path) -> Vec<String> {
    vec!["serve".to_string(), repo.to_string_lossy().into_owned()]
}

/// Plan the changes for one detection without writing anything.
/// `command` must be absolute (callers pass the shim path).
pub fn plan(
    repo: &Path,
    _home: &Path,
    det: &Detection,
    command: &str,
    args: &[String],
) -> Result<Change> {
    let spec = det.spec;
    if !det.scope_supported {
        bail!(
            "{} does not support the {} scope",
            spec.id,
            det.scope.as_str()
        );
    }
    let old_text = fs::read_to_string(&det.config_path).ok();
    let base_text = old_text.clone().unwrap_or_else(|| match spec.format {
        ConfigFormat::Json | ConfigFormat::Jsonc => "{}\n".to_string(),
        ConfigFormat::Toml => String::new(),
    });
    let new_text = edit::upsert_entry(&base_text, spec, command, args)
        .with_context(|| format!("cannot merge into {}", det.config_path.display()))?;
    let config = FilePlan {
        path: det.config_path.clone(),
        diff: diff::line_diff(old_text.as_deref().unwrap_or(""), &new_text),
        old_text,
        new_text,
    };

    // Instruction block: project scope only (never edit a user's global
    // instruction files), and only for agents with an instruction file.
    let instructions = match (det.scope, spec.instructions) {
        (Scope::Project, Some(rel)) => {
            let path = repo.join(rel);
            let old = fs::read_to_string(&path).ok();
            let base = old.clone().unwrap_or_default();
            let new = instructions::upsert(&base);
            let diff = diff::line_diff(old.as_deref().unwrap_or(""), &new);
            Some(FilePlan {
                path,
                old_text: old,
                new_text: new,
                diff,
            })
        }
        _ => None,
    };

    Ok(Change {
        detection: det.clone(),
        config,
        instructions,
        note: det.spec.note,
    })
}

/// Outcome for one file we attempted to write.
#[derive(Debug)]
pub enum ApplyOutcome {
    Written {
        path: PathBuf,
        backup: Option<PathBuf>,
        created: bool,
    },
    /// File was unparseable or had a conflicting non-object key; untouched.
    Skipped { path: PathBuf, reason: String },
}

/// Apply a planned change set. Callers must have shown the diffs and
/// obtained confirmation already.
pub fn apply(changes: &[Change]) -> Result<Vec<ApplyOutcome>> {
    let mut outcomes = Vec::with_capacity(changes.len() * 2);
    for change in changes {
        for file in [Some(&change.config), change.instructions.as_ref()]
            .into_iter()
            .flatten()
        {
            let old = file.old_text.as_deref();
            let created = old.is_none();
            // Re-check the file did not change since planning (TOCTOU guard).
            let current = fs::read_to_string(&file.path).ok();
            if current.as_deref() != old {
                outcomes.push(ApplyOutcome::Skipped {
                    path: file.path.clone(),
                    reason:
                        "file changed since the plan was made — left untouched; re-run to re-plan"
                            .into(),
                });
                continue;
            }
            let backup = manifest::begin_write(&file.path)?;
            manifest::atomic_write(&file.path, file.new_text.as_bytes())?;
            manifest::finish_write(&file.path, file.new_text.as_bytes(), created)?;
            outcomes.push(ApplyOutcome::Written {
                path: file.path.clone(),
                backup,
                created,
            });
        }
    }
    Ok(outcomes)
}

/// What happened to one file during removal.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveStep {
    /// Byte-for-byte restore of the pre-codegraph file (or deletion of a
    /// file we created).
    Restored,
    /// The file had later edits; only our block/entry was removed.
    Surgical,
    /// Nothing to remove.
    NotPresent,
}

/// Result of removing one agent.
#[derive(Debug)]
pub struct RemoveReport {
    pub agent_id: &'static str,
    pub scope: Scope,
    pub config: Result<RemoveStep, String>,
    pub instructions: Option<Result<RemoveStep, String>>,
}

/// Remove codegraph from one agent. Never edits files the user changed
/// since our write, except to remove our own entry surgically.
pub fn remove(repo: &Path, home: &Path, spec: &'static AgentSpec, scope: Scope) -> RemoveReport {
    let config_path = config_path(spec, scope, repo, home);
    let config = match &config_path {
        Some(path) => remove_one(path, |text| edit::remove_entry(text, spec)),
        None => {
            return RemoveReport {
                agent_id: spec.id,
                scope,
                config: Err(format!(
                    "{} does not support the {} scope",
                    spec.id,
                    scope.as_str()
                )),
                instructions: None,
            }
        }
    };
    let instructions = match (scope, spec.instructions) {
        (Scope::Project, Some(rel)) => {
            let path = repo.join(rel);
            Some(remove_one(&path, |text| match instructions::strip(text) {
                Some(stripped) => Ok(stripped),
                None => Err(edit::EditError::NotPresent),
            }))
        }
        _ => None,
    };
    RemoveReport {
        agent_id: spec.id,
        scope,
        config,
        instructions,
    }
}

/// Shared removal path: manifest-aware restore first, surgical edit second.
fn remove_one(
    path: &Path,
    strip: impl FnOnce(&str) -> Result<String, edit::EditError>,
) -> Result<RemoveStep, String> {
    if !path.exists() {
        return Ok(RemoveStep::NotPresent);
    }
    match manifest::try_restore(path) {
        Ok(RestoreOutcome::Restored) => return Ok(RemoveStep::Restored),
        Ok(RestoreOutcome::Surgical | RestoreOutcome::NoState) => {}
        Err(e) => return Err(format!("{e:#}")),
    }
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    match strip(&text) {
        Ok(new_text) => {
            manifest::atomic_write(path, new_text.as_bytes()).map_err(|e| format!("{e:#}"))?;
            let _ = fs::remove_file(manifest::state_path(path));
            Ok(RemoveStep::Surgical)
        }
        Err(edit::EditError::NotPresent) => Ok(RemoveStep::NotPresent),
        Err(e) => Err(format!("{e}")),
    }
}

/// Status of one agent for `codegraph agents list`.
#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub spec: &'static AgentSpec,
    /// True when there is evidence the agent is in use on this machine.
    pub detected: bool,
    pub evidence: Vec<String>,
    pub config_path: Option<PathBuf>,
    pub config_exists: bool,
    pub configured: bool,
    pub config_valid: bool,
    pub instructions: Option<(PathBuf, bool)>,
    pub scope: Scope,
}

/// Full status across every Tier 1 agent, for one scope.
pub fn status(repo: &Path, home: &Path, scope: Scope) -> Vec<AgentStatus> {
    AGENTS
        .iter()
        .map(|spec| {
            let det = detect(repo, home, scope, &[spec.id]).into_iter().next();
            let placeholder = det.as_ref().map(|d| !d.scope_supported).unwrap_or(false);
            let (config_path, config_exists, configured, config_valid, evidence) = match &det {
                Some(d) if d.scope_supported => {
                    let path = d.config_path.clone();
                    let text = fs::read_to_string(&path).ok();
                    let config_exists = path.exists();
                    let configured = text
                        .as_deref()
                        .and_then(|t| edit::read_entry(t, spec).ok().flatten())
                        .is_some();
                    let config_valid = match &text {
                        Some(t) if !t.trim().is_empty() => edit::read_entry(t, spec).is_ok(),
                        Some(_) => true,
                        None => false,
                    };
                    (
                        Some(path),
                        config_exists,
                        configured,
                        config_valid,
                        d.evidence.clone(),
                    )
                }
                _ if placeholder => (None, false, false, true, Vec::new()),
                _ => (
                    config_path(spec, scope, repo, home),
                    fs::metadata(config_path(spec, scope, repo, home).unwrap_or_default()).is_ok(),
                    false,
                    true,
                    Vec::new(),
                ),
            };
            let instructions = match (scope, spec.instructions) {
                (Scope::Project, Some(rel)) => {
                    let p = repo.join(rel);
                    let present = fs::read_to_string(&p)
                        .map(|t| instructions::is_present(&t))
                        .unwrap_or(false);
                    Some((p, present))
                }
                _ => None,
            };
            AgentStatus {
                spec,
                detected: !evidence.is_empty(),
                evidence,
                config_path,
                config_exists,
                configured,
                config_valid,
                instructions,
                scope,
            }
        })
        .collect()
}

/// The structured config entry for codegraph, for `mcp-config` output and
/// doctor display.
pub fn entry_json(spec: &AgentSpec, command: &str, args: &[String]) -> Value {
    match spec.entry.command_array {
        true => {
            let mut cmd = vec![Value::from(command)];
            cmd.extend(args.iter().cloned().map(Value::from));
            serde_json::json!({ "type": "local", "command": cmd })
        }
        false => {
            let mut obj = serde_json::Map::new();
            if spec.entry.include_type {
                obj.insert("type".into(), Value::from("stdio"));
            }
            obj.insert("command".into(), Value::from(command));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::from).collect()),
            );
            Value::Object(obj)
        }
    }
}
