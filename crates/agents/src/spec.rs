//! The agent adapter table: one entry per supported coding agent.
//!
//! Every row was verified against the agent's official documentation on
//! 2026-11-19 (see COMPAT.md "Source quality"). Config paths and formats
//! change between agent releases; re-verify on every major release of an
//! agent before trusting a row here.
//!
//! All paths are declared as data and expanded by one function
//! ([`crate::expand`]); nothing else in the crate hard-codes an agent.

use std::path::{Path, PathBuf};

/// Which config scope an entry applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// In the repository (usually committed / shared).
    Project,
    /// In the user's home directory (personal, all projects).
    User,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::User => "user",
        }
    }
}

/// Where a config path is anchored. Expanded against the current OS in
/// [`expand`]; all fall back to something sensible on every OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathBase {
    /// Repo root (project scope only).
    RepoRoot,
    /// `$HOME` / `%USERPROFILE%`.
    Home,
    /// XDG config home: `$XDG_CONFIG_HOME` or `~/.config`.
    ConfigHome,
    /// `$CODEX_HOME` or `~/.codex`.
    CodexHome,
    /// `%APPDATA%` on Windows; `~/.config` elsewhere.
    AppDataRoaming,
    /// `%LOCALAPPDATA%` on Windows; `~/.local/share` elsewhere.
    LocalAppData,
    /// macOS: `~/Library/Application Support`; Windows: `%APPDATA%`;
    /// Linux: `~/.config`.
    LibraryAppSupport,
}

/// A scoped config file location, as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopedPath {
    pub scope: Scope,
    pub base: PathBase,
    pub segments: &'static [&'static str],
}

/// On-disk format of the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// Strict JSON (comments not allowed by the agent; we still parse
    /// leniently for display but refuse to write files we cannot parse).
    Json,
    /// JSON with optional comments (JSONC).
    Jsonc,
    /// TOML (comment-preserving via `toml_edit`).
    Toml,
}

impl ConfigFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigFormat::Json => "json",
            ConfigFormat::Jsonc => "jsonc",
            ConfigFormat::Toml => "toml",
        }
    }
}

/// How the `codegraph` entry is shaped in the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryStyle {
    /// Nesting from the document root to the object that holds the server
    /// entries, e.g. `&["mcpServers"]` or `&["mcp", "servers"]`.
    /// For TOML this names the top-level table, e.g. `&["mcp_servers"]`.
    pub servers_key: &'static [&'static str],
    /// OpenCode style: `{ "type": "local", "command": [bin, ...args] }`.
    /// Otherwise: `{ ["type": "stdio"], "command": bin, "args": [...] }`.
    pub command_array: bool,
    /// Include a `"type": "stdio"` field in the entry (VS Code / Claude Code).
    pub include_type: bool,
}

/// Binary names used to detect the agent on PATH (any match is evidence).
pub type DetectBinaries = &'static [&'static str];

/// The complete adapter for one coding agent.
#[derive(Debug, Clone, Copy)]
pub struct AgentSpec {
    /// Stable id used on the command line (`codegraph init --agent <id>`).
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Compatibility tier (see COMPAT.md).
    pub tier: u8,
    /// Candidate config paths; for each scope, the first entry is the
    /// default write location. All are checked for detection.
    pub paths: &'static [ScopedPath],
    pub format: ConfigFormat,
    pub entry: EntryStyle,
    /// Binaries on PATH that indicate the agent is installed.
    pub detect_binaries: DetectBinaries,
    /// Extra filesystem markers (relative to repo root) that indicate the
    /// agent is configured for this repo. `~` entries are home-relative.
    pub detect_markers_repo: &'static [&'static str],
    /// Agent-specific quirk note shown during init (e.g. unreliable scope).
    pub note: Option<&'static str>,
    /// Which file gets the `<!-- codegraph:start -->` instruction block.
    /// Relative to repo root. `None`: the agent reads no instruction file.
    pub instructions: Option<&'static str>,
    /// What the binary shows after setup: restart/approve steps.
    pub restart_note: &'static str,
    /// A ready-to-paste prompt the user can give the agent to verify.
    pub verify_prompt: &'static str,
    /// Where to look in the agent's docs for MCP setup (shown on failure).
    pub docs_url: &'static str,
    /// Native CLI that can add/remove the server, for the docs. We do not
    /// shell out to it (see DECISIONS.md), but guides must mention it.
    pub native_cli: Option<&'static str>,
}

const STDIO: EntryStyle = EntryStyle {
    servers_key: &["mcpServers"],
    command_array: false,
    include_type: false,
};

const STDIO_TYPE: EntryStyle = EntryStyle {
    servers_key: &["mcpServers"],
    command_array: false,
    include_type: true,
};

const OPENCODE: EntryStyle = EntryStyle {
    servers_key: &["mcp", "servers"],
    command_array: true,
    include_type: false,
};

/// The Tier 1 agents. Keep sorted by id.
pub const AGENTS: &[AgentSpec] = &[
    AgentSpec {
        id: "antigravity",
        name: "Google Antigravity (IDE and CLI)",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".agents", "mcp_config.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::Home, segments: &[".gemini", "config", "mcp_config.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO,
        detect_binaries: &["agy"],
        detect_markers_repo: &[".agents"],
        note: Some("Project scope is documented but silently ignored by current Antigravity CLI builds (upstream issue #60). Use --scope user, or verify the server shows up before relying on it."),
        instructions: Some("AGENTS.md"),
        restart_note: "Reload MCP: CLI — run `/mcp` and reload (or start a new session); App/IDE — restart from the MCP panel (⋮ → Restart) or restart the app.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://antigravity.google/docs/mcp",
        native_cli: Some("agy mcp add <name> -- <command> [args...] (v1.1.16+, user scope)"),
    },
    AgentSpec {
        id: "claude-code",
        name: "Claude Code (CLI, desktop app, IDE extensions)",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".mcp.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::Home, segments: &[".claude.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO_TYPE,
        detect_binaries: &["claude"],
        detect_markers_repo: &[".mcp.json", ".claude"],
        note: Some("User scope writes top-level mcpServers in ~/.claude.json; `claude mcp add` defaults to a private per-project \"local\" scope stored under projects.\"<path>\".mcpServers instead."),
        instructions: Some("CLAUDE.md"),
        restart_note: "Exit and start a new Claude Code session (`claude`). Project .mcp.json servers need interactive approval on first use; check with `claude mcp list`.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://code.claude.com/docs/en/mcp",
        native_cli: Some("claude mcp add [--scope user|project] <name> -- <command> [args...]; claude mcp remove <name>"),
    },
    AgentSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::User, base: PathBase::LibraryAppSupport, segments: &["Claude", "claude_desktop_config.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO,
        detect_binaries: &[],
        detect_markers_repo: &[],
        note: Some("User-level only; there is no project scope. macOS GUI apps do not inherit your shell PATH, so the absolute shim path matters. Claude Desktop reads no instruction file."),
        instructions: None,
        restart_note: "Completely quit Claude Desktop (closing the window is not enough) and relaunch it.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop",
        native_cli: None,
    },
    AgentSpec {
        id: "codex-cli",
        name: "Codex CLI",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".codex", "config.toml"] },
            ScopedPath { scope: Scope::User, base: PathBase::CodexHome, segments: &["config.toml"] },
        ],
        format: ConfigFormat::Toml,
        entry: EntryStyle { servers_key: &["mcp_servers"], command_array: false, include_type: false },
        detect_binaries: &["codex"],
        detect_markers_repo: &[".codex"],
        note: Some("Project config is honored only in trusted folders (trust_level = \"trusted\" in user config). `codex mcp add` manages only the user/global file."),
        instructions: Some("AGENTS.md"),
        restart_note: "Start a new `codex` session and run `/mcp` (or `codex mcp list`) to confirm the server connected.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://developers.openai.com/codex/extend/mcp",
        native_cli: Some("codex mcp add <name> -- <command> [args...] (user scope only); codex mcp remove <name>"),
    },
    AgentSpec {
        id: "cursor",
        name: "Cursor",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".cursor", "mcp.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::Home, segments: &[".cursor", "mcp.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO,
        detect_binaries: &["cursor"],
        detect_markers_repo: &[".cursor"],
        note: Some("Cursor is being folded into Devin/Cognition tooling; config format re-verified 2026-11-19. Examples in the docs omit `type`; keep the entry typeless."),
        instructions: Some("AGENTS.md"),
        restart_note: "Save the file, then restart Cursor (or toggle the server off/on under Customize → MCPs).",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://cursor.com/docs/mcp",
        native_cli: Some("agent mcp add <name> -- <command> [args...] (undocumented scope flags; prefer editing the file)"),
    },
    AgentSpec {
        id: "gemini-cli",
        name: "Gemini CLI",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".gemini", "settings.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::Home, segments: &[".gemini", "settings.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO,
        detect_binaries: &["gemini"],
        detect_markers_repo: &[".gemini"],
        note: Some("Project settings (including MCP servers) load only in trusted folders; the folder must be trusted or the servers silently never connect."),
        instructions: Some("GEMINI.md"),
        restart_note: "Start a new session, or run `/mcp reload` in-session. The folder must be trusted (`/permissions trust`).",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://geminicli.com/docs/tools/mcp-server.md",
        native_cli: Some("gemini mcp add -s project|user <name> <command> [args]; gemini mcp remove -s project|user <name>"),
    },
    AgentSpec {
        id: "opencode",
        name: "OpenCode",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &["opencode.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::ConfigHome, segments: &["opencode", "opencode.json"] },
        ],
        format: ConfigFormat::Jsonc,
        entry: OPENCODE,
        detect_binaries: &["opencode"],
        detect_markers_repo: &["opencode.json", "opencode.jsonc", ".opencode"],
        note: None,
        instructions: Some("AGENTS.md"),
        restart_note: "New sessions pick the server up automatically; use `/mcps` or `opencode mcp list` to confirm it connects.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://opencode.ai/docs/mcp-servers",
        native_cli: Some("opencode mcp add [--global] <name> -- <command> [args...]"),
    },
    AgentSpec {
        id: "vscode-copilot",
        name: "VS Code with GitHub Copilot",
        tier: 1,
        paths: &[
            // The portable Copilot format (top-level `mcpServers`) is the
            // documented recommendation; the editor-managed
            // `.vscode/mcp.json` (`servers` key) is deprecated (removed ~Feb 2027).
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".mcp.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::Home, segments: &[".copilot", "mcp-config.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO_TYPE,
        detect_binaries: &["code"],
        detect_markers_repo: &[".mcp.json", ".vscode"],
        note: Some("Writes the portable Copilot config (root .mcp.json for project scope, ~/.copilot/mcp-config.json for user), not the deprecated editor-managed .vscode/mcp.json. Same file as Claude Code project scope — entries merge, they do not conflict. $COPILOT_HOME overrides the user path."),
        instructions: Some("AGENTS.md"),
        restart_note: "Reopen the folder or reload the window (Developer: Reload Window). Tool calls still ask for approval; MCP enablement is also gated by Workspace Trust.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://code.visualstudio.com/docs/agents/reference/mcp-configuration",
        native_cli: Some("code --add-mcp '<json>' (user-profile, editor-managed format only)"),
    },
    AgentSpec {
        id: "windsurf",
        name: "Windsurf (Cascade) / Devin Desktop",
        tier: 1,
        paths: &[
            ScopedPath { scope: Scope::Project, base: PathBase::RepoRoot, segments: &[".devin", "mcp_config.json"] },
            ScopedPath { scope: Scope::User, base: PathBase::ConfigHome, segments: &["devin", "mcp_config.json"] },
        ],
        format: ConfigFormat::Json,
        entry: STDIO,
        detect_binaries: &["devin", "windsurf"],
        detect_markers_repo: &[".devin", ".windsurf"],
        note: Some("Windsurf is being transitioned to Devin Desktop/CLI. Config targets the Cortex-managed file (~/.config/devin/mcp_config.json; Windows %APPDATA%\\devin\\mcp_config.json). The legacy discovery file ~/.codeium/windsurf/mcp_config.json is opt-in via chat.mcp.discovery.enabled. Legacy Cascade IDE has no project scope."),
        instructions: Some("AGENTS.md"),
        restart_note: "Press the refresh button in the MCP section (… menu), or fully quit and reopen. Devin CLI: `devin mcp list` to confirm.",
        verify_prompt: "Use the codegraph tools to find where login is handled and what calls it.",
        docs_url: "https://docs.devin.ai/cli/extensibility/mcp/configuration.md",
        native_cli: Some("devin mcp add -s user|project <name> -- <command> [args...]; devin mcp remove <name>"),
    },
];

/// Look up an agent by id.
pub fn find(id: &str) -> Option<&'static AgentSpec> {
    AGENTS.iter().find(|a| a.id == id)
}

/// Expand a [`PathBase`] against the current OS. `repo` and `home` are the
/// caller's repo root and home directory (injectable for tests).
pub fn expand(base: PathBase, segments: &[&str], repo: &Path, home: &Path) -> PathBuf {
    let base_dir: PathBuf = match base {
        PathBase::RepoRoot => repo.to_path_buf(),
        PathBase::Home => home.to_path_buf(),
        PathBase::ConfigHome => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".config")),
        PathBase::CodexHome => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".codex")),
        PathBase::AppDataRoaming => {
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            }
            #[cfg(not(windows))]
            {
                home.join(".config")
            }
        }
        PathBase::LocalAppData => {
            #[cfg(windows)]
            {
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("AppData").join("Local"))
            }
            #[cfg(not(windows))]
            {
                home.join(".local").join("share")
            }
        }
        PathBase::LibraryAppSupport => {
            #[cfg(target_os = "macos")]
            {
                home.join("Library").join("Application Support")
            }
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                home.join(".config")
            }
        }
    };
    segments.iter().fold(base_dir, |acc, seg| acc.join(seg))
}

/// Config paths for one agent in one scope, with the write default first.
/// The opencode project path prefers an existing `opencode.jsonc`.
pub fn config_paths(spec: &AgentSpec, scope: Scope, repo: &Path, home: &Path) -> Vec<PathBuf> {
    if spec.id == "opencode" && scope == Scope::Project {
        let jsonc = repo.join("opencode.jsonc");
        if jsonc.exists() {
            return vec![jsonc];
        }
    }
    spec.paths
        .iter()
        .filter(|p| p.scope == scope)
        .map(|p| expand(p.base, p.segments, repo, home))
        .collect()
}

/// The default write location for one agent and scope.
pub fn config_path(spec: &AgentSpec, scope: Scope, repo: &Path, home: &Path) -> Option<PathBuf> {
    config_paths(spec, scope, repo, home).into_iter().next()
}

/// All scopes the agent supports, in default preference order.
pub fn supported_scopes(spec: &AgentSpec) -> &'static [Scope] {
    // Project first where supported, else user; claude-desktop is user-only.
    static BOTH: [Scope; 2] = [Scope::Project, Scope::User];
    static USER_ONLY: [Scope; 1] = [Scope::User];
    if spec.paths.iter().any(|p| p.scope == Scope::Project) {
        &BOTH
    } else {
        &USER_ONLY
    }
}
