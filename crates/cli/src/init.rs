//! `codegraph init`: register codegraph as an MCP server in detected coding
//! agents' configs — after showing the change and only with confirmation.
//!
//! Detection is by filesystem markers (and, interactively, by PATH lookup).
//! We never edit anything until the caller confirms (or `--yes` is passed),
//! never touch files we can't parse, and always leave a `.codegraph.bak`
//! backup next to any file we rewrite.
//!
//! Supported (project mode, repo root):
//!   Claude Code  `.mcp.json`                (mcpServers.codegraph)
//!   Cursor       `.cursor/mcp.json`         (mcpServers.codegraph)
//!   OpenCode     `opencode.json`            (mcp.servers.codegraph, type local)
//! Global mode: `~/.claude.json` and `~/.cursor/mcp.json` instead of project files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Cursor,
    OpenCode,
}

impl AgentKind {
    pub fn name(&self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude Code",
            AgentKind::Cursor => "Cursor",
            AgentKind::OpenCode => "OpenCode",
        }
    }
    pub fn id(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Cursor => "cursor",
            AgentKind::OpenCode => "opencode",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(AgentKind::Claude),
            "cursor" => Some(AgentKind::Cursor),
            "opencode" => Some(AgentKind::OpenCode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub agent: AgentKind,
    /// Config file we would write.
    pub config_path: PathBuf,
    /// True when the file already exists.
    pub existed: bool,
    /// True when a codegraph entry is already present.
    pub already_configured: bool,
}

/// Detect candidates. `path_lookup` additionally treats a resolvable CLI
/// (`claude`, `cursor`, `opencode`) as evidence the agent is used; tests
/// pass `false` for determinism.
pub fn detect(repo: &Path, home: &Path, global: bool, path_lookup: bool) -> Vec<Detection> {
    let mut out = Vec::new();
    for agent in [AgentKind::Claude, AgentKind::Cursor, AgentKind::OpenCode] {
        let config_path = config_path(agent, repo, home, global);
        let existed = config_path.exists();
        let marker =
            marker_found(agent, repo, home, global) || (path_lookup && on_path(agent.id()));
        if !marker {
            continue;
        }
        let already_configured = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|v| codegraph_entry(&v, agent).is_some())
            .unwrap_or(false);
        out.push(Detection {
            agent,
            config_path,
            existed,
            already_configured,
        });
    }
    out
}

fn config_path(agent: AgentKind, repo: &Path, home: &Path, global: bool) -> PathBuf {
    match (agent, global) {
        (AgentKind::Claude, false) => repo.join(".mcp.json"),
        (AgentKind::Cursor, false) => repo.join(".cursor").join("mcp.json"),
        (AgentKind::OpenCode, false) => repo.join("opencode.json"),
        (AgentKind::Claude, true) => home.join(".claude.json"),
        (AgentKind::Cursor, true) => home.join(".cursor").join("mcp.json"),
        (AgentKind::OpenCode, true) => home.join(".config").join("opencode").join("opencode.json"),
    }
}

fn marker_found(agent: AgentKind, repo: &Path, home: &Path, global: bool) -> bool {
    if global {
        return config_path(agent, repo, home, true).exists();
    }
    match agent {
        AgentKind::Claude => repo.join(".mcp.json").exists() || repo.join(".claude").is_dir(),
        AgentKind::Cursor => {
            repo.join(".cursor").is_dir() || repo.join(".cursor").join("mcp.json").exists()
        }
        AgentKind::OpenCode => {
            repo.join("opencode.json").exists() || repo.join(".opencode").is_dir()
        }
    }
}

fn on_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(binary);
        candidate.is_file() || (cfg!(windows) && dir.join(format!("{binary}.exe")).is_file())
    })
}

/// The codegraph entry for this agent, merging into an existing document.
fn merged_document(existing: Option<Value>, agent: AgentKind, exe: &str, repo: &str) -> Value {
    let mut doc = existing.unwrap_or_else(|| json!({}));
    if !doc.is_object() {
        doc = json!({});
    }
    let map = doc.as_object_mut().unwrap();
    match agent {
        AgentKind::Claude | AgentKind::Cursor => {
            let servers = map.entry("mcpServers").or_insert_with(|| json!({}));
            if !servers.is_object() {
                *servers = json!({});
            }
            servers["codegraph"] = json!({ "command": exe, "args": ["serve", repo] });
        }
        AgentKind::OpenCode => {
            let mcp = map.entry("mcp").or_insert_with(|| json!({}));
            if !mcp.is_object() {
                *mcp = json!({});
            }
            let mcp = mcp.as_object_mut().unwrap();
            let servers = mcp.entry("servers").or_insert_with(|| json!({}));
            if !servers.is_object() {
                *servers = json!({});
            }
            servers["codegraph"] = json!({ "type": "local", "command": [exe, "serve", repo] });
        }
    }
    doc
}

fn codegraph_entry(doc: &Value, agent: AgentKind) -> Option<&Value> {
    match agent {
        AgentKind::Claude | AgentKind::Cursor => doc.get("mcpServers")?.get("codegraph"),
        AgentKind::OpenCode => doc.get("mcp")?.get("servers")?.get("codegraph"),
    }
}

/// The pretty-printed JSON we would write for this detection (for display).
pub fn proposed_json(det: &Detection, exe: &Path, repo: &Path) -> String {
    let existing = read_existing(&det.config_path);
    let doc = merged_document(
        existing,
        det.agent,
        &exe.to_string_lossy(),
        &repo.to_string_lossy(),
    );
    serde_json::to_string_pretty(&doc).unwrap()
}

fn read_existing(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub enum ApplyOutcome {
    Written {
        config_path: PathBuf,
        backup_path: Option<PathBuf>,
    },
    SkippedUnparseable {
        config_path: PathBuf,
    },
}

/// Write the config for each detection. Callers must have confirmed first.
/// Never clobbers an unparseable file; backs up everything it rewrites.
pub fn apply_all(dets: &[Detection], exe: &Path, repo: &Path) -> Result<Vec<ApplyOutcome>> {
    let mut outcomes = Vec::new();
    for det in dets {
        let raw = fs::read_to_string(&det.config_path);
        let existing = match &raw {
            Ok(s) => match serde_json::from_str::<Value>(s) {
                Ok(v) => Some(v),
                Err(_) => {
                    outcomes.push(ApplyOutcome::SkippedUnparseable {
                        config_path: det.config_path.clone(),
                    });
                    continue;
                }
            },
            Err(_) => None,
        };
        if let Some(parent) = det.config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let backup_path = if det.config_path.exists() {
            let mut name = det.config_path.clone().into_os_string();
            name.push(".codegraph.bak");
            let backup = PathBuf::from(name);
            fs::copy(&det.config_path, &backup)
                .with_context(|| format!("cannot back up {}", det.config_path.display()))?;
            Some(backup)
        } else {
            None
        };
        let doc = merged_document(
            existing,
            det.agent,
            &exe.to_string_lossy(),
            &repo.to_string_lossy(),
        );
        let pretty = serde_json::to_string_pretty(&doc)?;
        fs::write(&det.config_path, pretty + "\n")
            .with_context(|| format!("cannot write {}", det.config_path.display()))?;
        outcomes.push(ApplyOutcome::Written {
            config_path: det.config_path.clone(),
            backup_path,
        });
    }
    Ok(outcomes)
}
