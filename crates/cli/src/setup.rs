//! Command implementations for `codegraph init`, `codegraph agents …`,
//! `codegraph mcp-config`, `codegraph query` and `codegraph doctor`.
//!
//! Everything agent-specific lives in `codegraph-agents` as data; this file
//! only resolves the environment (repo root, home, shim) and prints.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use codegraph_agents::{self as agents, Scope};
use codegraph_index::index_repo;
use codegraph_store::Store;

use crate::{canonical_repo, default_db_path, git_head_commit, home_dir, resolve_repo};

/// `codegraph init` options (mirrors the CLI flags).
#[derive(Debug, Default)]
pub struct InitOptions {
    /// Positional repo path; None → resolve_repo().
    pub path: Option<PathBuf>,
    /// Explicit `--scope`.
    pub scope: Option<Scope>,
    /// `--global` convenience alias for `--scope user`.
    pub global: bool,
    /// `--agent` ids (repeatable). Empty → every detected Tier 1 agent.
    pub agents: Vec<String>,
    pub yes: bool,
    pub dry_run: bool,
    pub skip_index: bool,
}

fn scope_or_default(explicit: Option<Scope>, global: bool) -> Scope {
    explicit.unwrap_or(if global { Scope::User } else { Scope::Project })
}

/// Expand `--agent` ids, validating against the registry.
fn validate_agent_ids(ids: &[String]) -> Result<Vec<&'static agents::AgentSpec>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        match agents::find(id) {
            Some(spec) => out.push(spec),
            None => bail!(
                "unknown agent '{id}' — known agents: {}",
                agents::AGENTS
                    .iter()
                    .map(|a| a.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    Ok(out)
}

/// The command agents will run, i.e. the stable shim path, plus the
/// `serve <repo>` arguments. Prints what it does about the shim.
fn shim_command(
    home: &Path,
    repo: &Path,
    dry_run: bool,
    actions: &mut Vec<String>,
) -> Result<(String, Vec<String>)> {
    let shim = agents::shim_path(home);
    if shim.exists() {
        return Ok((
            shim.to_string_lossy().into_owned(),
            agents::serve_args(repo),
        ));
    }
    if dry_run {
        actions.push(format!(
            "would create shim {} (copy of the current binary)",
            shim.display()
        ));
        return Ok((
            shim.to_string_lossy().into_owned(),
            agents::serve_args(repo),
        ));
    }
    let canonical = std::env::current_exe().context("cannot locate current executable")?;
    let (path, created) = agents::ensure_shim(home, &canonical)?;
    if created {
        actions.push(format!(
            "created shim {} (copied from {})",
            path.display(),
            canonical.display()
        ));
    }
    Ok((
        path.to_string_lossy().into_owned(),
        agents::serve_args(repo),
    ))
}

pub struct InitReport {
    /// Agents that were already configured everywhere (nothing to do).
    pub already: Vec<&'static str>,
    /// Agents written.
    pub written: Vec<&'static str>,
    /// Agents skipped with a reason (unparseable config etc.).
    pub skipped: Vec<(&'static str, String)>,
    pub dry_run: bool,
    pub gitignore_appended: bool,
}

/// Run `codegraph init`. Prints everything it does.
pub fn init(opts: &InitOptions) -> Result<InitReport> {
    let repo = canonical_repo(&resolve_repo(opts.path.as_deref()));
    let home = home_dir().context("cannot determine home directory")?;
    let scope = scope_or_default(opts.scope, opts.global);
    let mut actions: Vec<String> = Vec::new();

    // 1. Index first (README: `codegraph init` indexes the repo).
    if opts.skip_index {
        println!("skipping index (--no-index)");
    } else if opts.dry_run {
        println!(
            "dry run: would index {} now (use --no-index to skip)",
            repo.display()
        );
    } else {
        let db = default_db_path(&repo);
        let store = Store::open(&db)?;
        let stats = index_repo(&store, &repo)?;
        println!(
            "indexed: scanned={} parsed={} skipped={} removed={} symbols={} gaps={}",
            stats.files_scanned,
            stats.files_parsed,
            stats.files_skipped,
            stats.files_removed,
            stats.symbols_written,
            stats.gaps
        );
    }

    // 2. Decide which agents to configure.
    let only = validate_agent_ids(&opts.agents)?;
    let ids: Vec<&str> = if only.is_empty() {
        Vec::new() // all
    } else {
        only.iter().map(|s| s.id).collect()
    };
    let det_scope = scope;
    let detections = agents::detect(&repo, &home, det_scope, &ids);
    let mut usable: Vec<agents::Detection> = Vec::new();
    for det in detections {
        if !det.scope_supported {
            // Agent does not support this scope: fall back to the other
            // (claude-desktop is user-scope only), but only with real
            // evidence (an existing config file) or an explicit --agent
            // request — never create a config for an app we can't see.
            let other = if det.scope == Scope::Project {
                Scope::User
            } else {
                Scope::Project
            };
            let spec = det.spec;
            if !only.is_empty() {
                // Explicit ask: push a synthetic detection for the other scope.
                if let Some(path) = agents::config_path(spec, other, &repo, &home) {
                    println!(
                        "note: {} does not support the {} scope; using {} scope",
                        spec.id,
                        det.scope.as_str(),
                        other.as_str()
                    );
                    usable.push(agents::Detection {
                        spec,
                        scope: other,
                        config_path: path,
                        evidence: vec!["requested explicitly (--agent)".into()],
                        existed: false,
                        already_configured: false,
                        scope_supported: true,
                    });
                }
                continue;
            }
            let mut fallback = agents::detect(&repo, &home, other, &[spec.id]);
            match fallback.pop() {
                Some(mut f) if !f.evidence.is_empty() => {
                    println!(
                        "note: {} does not support the {} scope; using {} scope",
                        spec.id,
                        det.scope.as_str(),
                        other.as_str()
                    );
                    f.scope_supported = true;
                    usable.push(f);
                }
                _ => {
                    println!(
                        "skipping {}: no {} marker found (use --agent {} to set it up anyway)",
                        spec.id,
                        other.as_str(),
                        spec.id
                    );
                }
            }
            continue;
        }
        if only.is_empty() && det.evidence.is_empty() {
            continue;
        }
        usable.push(det);
    }
    // Explicit --agent that produced no evidence (e.g. fresh machine):
    // still configure it — the user asked by name.
    if !only.is_empty() {
        for spec in only {
            if usable.iter().any(|d| d.spec.id == spec.id) {
                continue;
            }
            let other_scope_ok = spec.paths.iter().any(|p| p.scope == det_scope);
            let effective = if other_scope_ok {
                det_scope
            } else {
                Scope::User
            };
            if let Some(mut det) = agents::detect(&repo, &home, effective, &[spec.id]).pop() {
                det.scope_supported = true;
                usable.push(det);
            } else if let Some(path) = agents::config_path(spec, effective, &repo, &home) {
                usable.push(agents::Detection {
                    spec,
                    scope: effective,
                    config_path: path,
                    evidence: vec!["requested explicitly (--agent)".into()],
                    existed: false,
                    already_configured: false,
                    scope_supported: true,
                });
            }
        }
    }
    if usable.is_empty() {
        println!(
            "codegraph init: no supported agents detected in {}",
            repo.display()
        );
        println!("detected: none. Explicit example: codegraph init --agent claude-code");
        println!(
            "supported: {}",
            agents::AGENTS
                .iter()
                .map(|a| a.id)
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("see docs: https://github.com/BuildTeamRYS/codegraph#connect-your-coding-agent");
        return Ok(InitReport {
            already: Vec::new(),
            written: Vec::new(),
            skipped: Vec::new(),
            dry_run: opts.dry_run,
            gitignore_appended: false,
        });
    }

    // 3. Shim (skipped for dry-run; prints actions).
    let (command, args) = shim_command(&home, &repo, opts.dry_run, &mut actions)?;

    // 4. Announce detection, plan changes, print diffs.
    println!();
    for det in &usable {
        let state = if det.already_configured {
            "already configured"
        } else if det.existed {
            "config exists — will merge"
        } else {
            "new file"
        };
        println!("detected {}: {}", det.spec.name, det.config_path.display());
        for ev in &det.evidence {
            println!("    {ev}");
        }
        println!("    scope: {} ({state})", det.scope.as_str());
        if let Some(note) = det.spec.note {
            println!("    note: {note}");
        }
    }
    let plans: Vec<agents::Change> = usable
        .iter()
        .map(|det| agents::plan(&repo, &home, det, &command, &args))
        .collect::<Result<Vec<_>>>()?;
    println!();
    let mut pending: Vec<&agents::Change> = Vec::new();
    for change in &plans {
        let id = change.detection.spec.id;
        if change.is_noop() {
            println!("{id}: already configured — nothing to do");
            continue;
        }
        println!("--- {id}: {} ---", change.config.path.display());
        print!("{}", change.config.diff.trim_end_matches('\n'));
        if let Some(inst) = &change.instructions {
            if inst.old_text.as_deref() != Some(inst.new_text.as_str()) {
                println!(
                    "--- {}: {} (instruction block) ---",
                    id,
                    inst.path.display()
                );
                print!("{}", inst.diff.trim_end_matches('\n'));
            }
        }
        println!();
        pending.push(change);
    }

    // 5. .gitignore: offer for project scope.
    let gitignore_path = repo.join(".gitignore");
    let gitignore_offer = matches!(scope, Scope::Project)
        && gitignore_path.exists()
        && !fs::read_to_string(&gitignore_path)
            .map(|t| {
                t.lines().any(|l| {
                    l.trim() == ".codegraph/"
                        || l.trim() == ".codegraph"
                        || l.trim() == "/.codegraph/"
                })
            })
            .unwrap_or(true);
    if gitignore_offer {
        println!(".gitignore: would append '.codegraph/' so the local index is not committed");
    }

    if opts.dry_run {
        println!(
            "dry run: nothing written. Re-run without --dry-run to apply (it will ask first)."
        );
        return Ok(InitReport {
            already: Vec::new(),
            written: Vec::new(),
            skipped: Vec::new(),
            dry_run: true,
            gitignore_appended: false,
        });
    }

    if pending.is_empty() && !gitignore_offer {
        println!("codegraph is already configured for every detected agent; nothing to do.");
        return Ok(InitReport {
            already: usable.iter().map(|d| d.spec.id).collect(),
            written: Vec::new(),
            skipped: Vec::new(),
            dry_run: false,
            gitignore_appended: false,
        });
    }

    // 6. Confirm.
    let confirmed = if opts.yes {
        true
    } else if std::io::stdin().is_terminal() {
        print!("Apply these changes? [y/N] ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    } else {
        println!("stdin is not a terminal; not applying anything. Re-run with --yes to apply.");
        false
    };
    if !confirmed {
        println!("not applying anything.");
        return Ok(InitReport {
            already: Vec::new(),
            written: Vec::new(),
            skipped: Vec::new(),
            dry_run: false,
            gitignore_appended: false,
        });
    }

    // 7. Apply.
    for action in &actions {
        println!("{action}");
    }
    let owned: Vec<agents::Change> = pending.into_iter().cloned().collect();
    let outcomes = agents::apply(&owned)?;
    let mut report = InitReport {
        already: Vec::new(),
        written: Vec::new(),
        skipped: Vec::new(),
        dry_run: false,
        gitignore_appended: false,
    };
    for outcome in outcomes {
        match outcome {
            agents::ApplyOutcome::Written {
                path,
                backup,
                created,
            } => {
                println!(
                    "wrote {}{}",
                    path.display(),
                    if created { " (new file)" } else { "" }
                );
                if let Some(b) = backup {
                    println!("backup: {}", b.display());
                }
            }
            agents::ApplyOutcome::Skipped { path, reason } => {
                eprintln!("skipped {}: {reason}", path.display());
            }
        }
    }
    if gitignore_offer {
        let mut f = fs::OpenOptions::new().append(true).open(&gitignore_path)?;
        writeln!(f, ".codegraph/")?;
        report.gitignore_appended = true;
        println!("wrote .gitignore: appended '.codegraph/'");
    }

    // 8. Post-install guidance.
    for change in &owned {
        let spec = change.detection.spec;
        report.written.push(spec.id);
        println!();
        println!("--- next steps for {} ---", spec.name);
        println!("1. {}", spec.restart_note);
        println!("2. Verify with `codegraph doctor`, or ask the agent:");
        println!("     {}", spec.verify_prompt);
        println!("3. To undo: codegraph agents remove {}", spec.id);
        println!("   Agent docs: {}", spec.docs_url);
    }
    Ok(report)
}

/// `codegraph agents list`.
pub fn agents_list(scope: Option<Scope>, path: Option<&Path>) -> Result<()> {
    let repo = canonical_repo(&resolve_repo(path));
    let home = home_dir().context("cannot determine home directory")?;
    let scopes: Vec<Scope> = match scope {
        Some(s) => vec![s],
        None => vec![Scope::Project, Scope::User],
    };
    for scope in scopes {
        println!("== {} scope ==", scope.as_str());
        for st in agents::status(&repo, &home, scope) {
            let spec = st.spec;
            if spec.paths.iter().all(|p| p.scope != scope) {
                println!("{:<16} —        n/a (other scope only)", spec.id);
                continue;
            }
            let cfg = st
                .config_path
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "-".into());
            let detected = if st.detected { "detected" } else { "-" };
            let configured = if st.configured { "configured" } else { "-" };
            // "does not parse" only when the file exists but is broken —
            // a missing file is just "not set up", not an error worth alarming.
            let valid = if st.config_exists && !st.config_valid {
                " (config does not parse!)"
            } else {
                ""
            };
            let inst = st
                .instructions
                .map(|(p, present)| {
                    format!(
                        "instructions: {} ({})",
                        p.display(),
                        if present { "present" } else { "missing" }
                    )
                })
                .unwrap_or_else(|| "instructions: n/a".into());
            println!(
                "{:<16} {detected:<9} {configured:<11} {cfg}{valid}",
                spec.id
            );
            println!("{:16}          {inst}", "");
        }
        println!();
    }
    println!("setup: codegraph init [--agent <id>] [--scope project|user]; docs: https://github.com/BuildTeamRYS/codegraph");
    Ok(())
}

/// `codegraph agents remove`.
pub fn agents_remove(id: &str, scope: Option<Scope>, path: Option<&Path>) -> Result<()> {
    let spec = agents::find(id).with_context(|| {
        format!(
            "unknown agent '{id}' — known: {}",
            agents::AGENTS
                .iter()
                .map(|a| a.id)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let repo = canonical_repo(&resolve_repo(path));
    let home = home_dir().context("cannot determine home directory")?;
    let scope = match scope {
        Some(s) => s,
        None => {
            if spec.paths.iter().any(|p| p.scope == Scope::Project) {
                Scope::Project
            } else {
                Scope::User
            }
        }
    };
    // Binary-repeat for symmetry with install: backups, atomic writes, etc.
    let report = agents::remove(&repo, &home, spec, scope);
    println!(
        "removed codegraph from {} ({} scope):",
        spec.id,
        scope.as_str()
    );
    match report.config {
        Ok(agents::RemoveStep::Restored) => {
            println!("  config: restored to the exact pre-install bytes")
        }
        Ok(agents::RemoveStep::Surgical) => {
            println!("  config: removed only the codegraph entry (your other edits kept)")
        }
        Ok(agents::RemoveStep::NotPresent) => println!("  config: nothing to remove"),
        Err(e) => println!("  config: FAILED — {e}"),
    }
    match report.instructions {
        Some(Ok(agents::RemoveStep::Restored)) => {
            println!("  instructions: restored to the exact pre-install bytes")
        }
        Some(Ok(agents::RemoveStep::Surgical)) => {
            println!("  instructions: stripped the codegraph block (your other edits kept)")
        }
        Some(Ok(agents::RemoveStep::NotPresent)) => println!("  instructions: nothing to remove"),
        Some(Err(e)) => println!("  instructions: FAILED — {e}"),
        None => {}
    }
    Ok(())
}

/// Render an MCP config snippet for any client (`--format json|toml|yaml`).
pub fn mcp_config(format: &str, path: Option<&Path>) -> Result<()> {
    let repo = canonical_repo(&resolve_repo(path));
    let home = home_dir().context("cannot determine home directory")?;
    let shim = agents::shim_path(&home);
    let command = shim.to_string_lossy().into_owned();
    let args = agents::serve_args(&repo);
    match format {
        "json" => {
            let snippet = serde_json::json!({
                "mcpServers": {
                    "codegraph": {
                        "type": "stdio",
                        "command": command,
                        "args": args,
                    }
                }
            });
            println!("{}", serde_json::to_string_pretty(&snippet)?);
        }
        "toml" => {
            println!("[mcp_servers.codegraph]");
            println!("command = {:?}", command);
            print!("args = [");
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                print!("{a:?}");
            }
            println!("]");
        }
        "yaml" => {
            println!("mcpServers:");
            println!("  codegraph:");
            println!("    type: stdio");
            println!("    command: {command}");
            println!("    args:");
            for a in &args {
                println!("      - {a}");
            }
        }
        other => bail!("unknown --format '{other}' (expected json|toml|yaml)"),
    }
    println!("# codegraph: the path above is an absolute path to the shim; run `codegraph init` to be configured automatically.");
    Ok(())
}

/// Outcome of looking for a configured codegraph entry in one agent's file.
enum Configured {
    /// Entry found: config path, command, args.
    Yes(PathBuf, String, Vec<String>),
    /// File exists but cannot be parsed — report that, not "not configured".
    Broken(PathBuf, String),
    Absent,
}

/// Find the configured command + args for an agent (any candidate path).
fn configured_command(
    repo: &Path,
    home: &Path,
    spec: &'static agents::AgentSpec,
    scope: Scope,
) -> Configured {
    for path in agents::spec::config_paths(spec, scope, repo, home) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        match agents::edit::read_entry(&text, spec) {
            Ok(Some(entry)) => {
                let (command, args) = match &entry {
                    serde_json::Value::Object(obj) => match obj.get("command") {
                        Some(serde_json::Value::Array(items)) => {
                            let mut it =
                                items.iter().filter_map(|v| v.as_str().map(str::to_string));
                            let cmd = it.next().unwrap_or_default();
                            (cmd, it.collect())
                        }
                        Some(serde_json::Value::String(s)) => {
                            let args = obj
                                .get("args")
                                .and_then(|a| a.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str().map(str::to_string))
                                        .collect()
                                })
                                .unwrap_or_default();
                            (s.clone(), args)
                        }
                        _ => {
                            return Configured::Broken(
                                path,
                                "command entry has an unexpected shape".into(),
                            )
                        }
                    },
                    _ => {
                        return Configured::Broken(path, "codegraph entry is not an object".into())
                    }
                };
                return Configured::Yes(path, command, args);
            }
            Ok(None) => continue, // parses, entry absent
            Err(e) => return Configured::Broken(path, e.to_string()),
        }
    }
    Configured::Absent
}

/// `codegraph doctor`: per agent, diagnose setup problems, each with a fix.
pub fn doctor(ids: &[String], scope: Option<Scope>, path: Option<&Path>) -> Result<i32> {
    let repo = canonical_repo(&resolve_repo(path));
    let home = home_dir().context("cannot determine home directory")?;
    let only = validate_agent_ids(ids)?;
    let scopes: Vec<Scope> = match scope {
        Some(s) => vec![s],
        None => vec![Scope::Project, Scope::User],
    };

    let mut any_fail = false;
    let mut any_warn = false;
    let mut any_checked = false;
    let mut configured_ids: Vec<&'static str> = Vec::new();
    for scope in scopes {
        for spec in agents::AGENTS {
            if !only.is_empty() && !only.iter().any(|s| s.id == spec.id) {
                continue;
            }
            let check = configured_command(&repo, &home, spec, scope);
            match &check {
                Configured::Yes(_, _, _) | Configured::Broken(_, _) => {
                    configured_ids.push(spec.id);
                }
                Configured::Absent => {}
            }
            let (config_path, command, args) = match check {
                Configured::Yes(path, command, args) => (path, command, args),
                Configured::Broken(path, reason) => {
                    println!("--- {} ({} scope) ---", spec.id, scope.as_str());
                    println!("FAIL config does not parse: {reason}");
                    println!(
                        "     fix: fix the syntax in {} (a pre-install backup may exist as .codegraph.bak)",
                        path.display()
                    );
                    any_fail = true;
                    any_checked = true;
                    continue;
                }
                Configured::Absent => {
                    // A scope the user never set up is not a failure — a
                    // FAIL here would be wrong for e.g. opencode, which is
                    // configured in project scope only. An explicitly
                    // requested agent configured in *no* scope is reported
                    // after the loops instead.
                    if !only.is_empty() {
                        println!("--- {} ({} scope) ---", spec.id, scope.as_str());
                        println!("skip: not configured in this scope");
                    }
                    continue;
                }
            };
            any_checked = true;
            println!("--- {} ({} scope) ---", spec.id, scope.as_str());
            let mut ok = true;

            // 1. Config parses.
            match fs::read_to_string(&config_path) {
                Ok(text) => {
                    match agents::edit::read_entry(&text, spec) {
                        Ok(_) => println!("PASS config parses: {}", config_path.display()),
                        Err(e) => {
                            println!("FAIL config does not parse: {e}");
                            println!("     fix: fix the syntax in {} (a backup may exist as .codegraph.bak)", config_path.display());
                            ok = false;
                        }
                    }
                }
                Err(e) => {
                    println!("FAIL config unreadable: {e}");
                    println!("     fix: check permissions on {}", config_path.display());
                    ok = false;
                }
            }

            // 2. Configured command exists and is executable.
            //
            // "Path-like" = contains a separator or drive letter, in either
            // spelling — MCP clients and agent docs use both `C:\x\y.exe` and
            // `C:/x/y.exe`-style spellings, and on Windows `/path` is a valid
            // (drive-relative) path. Bare names (`npx`, `codegraph`) are PATH
            // lookups and get the warning branch instead.
            let cmd_path = Path::new(&command);
            let path_like = command.contains('/')
                || command.contains('\\')
                || (command.len() > 1 && command.as_bytes()[1] == b':');
            if path_like || cmd_path.is_absolute() {
                if cmd_path.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = fs::metadata(cmd_path)
                            .map(|m| m.permissions().mode())
                            .unwrap_or(0);
                        if mode & 0o111 == 0 {
                            println!("FAIL server binary is not executable: {command}");
                            println!("     fix: chmod +x {command}  (or replace it with an executable copy)");
                            ok = false;
                        } else {
                            println!("PASS server command exists: {command}");
                        }
                    }
                    #[cfg(not(unix))]
                    println!("PASS server command exists: {command}");
                } else {
                    println!("FAIL server binary not found: {command}");
                    println!("     fix: restore the binary at that path, or run `codegraph init` to rewrite the shim path");
                    ok = false;
                }
            } else {
                println!("WARN server command '{command}' is not absolute — PATH resolution depends on the agent's environment");
                println!(
                    "     fix: point the config at the absolute shim path ({} or `codegraph init`)",
                    agents::shim_path(&home).display()
                );
            }

            // 3. MCP handshake + tool list.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let timeout = Duration::from_secs(5);
            match rt.block_on(codegraph_mcp::ping_server(&command, &args, &repo, timeout)) {
                Ok(report) => {
                    let names: Vec<&str> = report.tools.iter().map(|t| t.name.as_str()).collect();
                    for expected in [
                        "find_symbol",
                        "outline",
                        "callers",
                        "callees",
                        "get_context",
                        "status",
                    ] {
                        if names.contains(&expected) {
                            println!("PASS tool {expected} registered");
                        } else {
                            println!("FAIL tool {expected} missing from server");
                            println!(
                                "     fix: update codegraph and re-register (codegraph update)"
                            );
                            ok = false;
                        }
                    }
                    let non_readonly: Vec<&str> = report
                        .tools
                        .iter()
                        .filter(|t| t.read_only_hint != Some(true))
                        .map(|t| t.name.as_str())
                        .collect();
                    if non_readonly.is_empty() {
                        println!("PASS all tools carry readOnlyHint");
                    } else {
                        println!(
                            "FAIL tools missing readOnlyHint: {}",
                            non_readonly.join(", ")
                        );
                        println!("     fix: update codegraph (codegraph update)");
                        ok = false;
                    }
                }
                Err(e) => {
                    println!("FAIL server did not answer MCP handshake: {e:#}");
                    println!("     fix: run `{command} {}` by hand and read stderr; then check `codegraph status`", args.join(" "));
                    ok = false;
                }
            }

            // 4. Instruction block (project scope only).
            if let (Scope::Project, Some(rel)) = (scope, spec.instructions) {
                let inst = repo.join(rel);
                let present = fs::read_to_string(&inst)
                    .map(|t| codegraph_agents::instructions::is_present(&t))
                    .unwrap_or(false);
                if present {
                    println!("PASS instructions: {}", inst.display());
                } else {
                    // Not necessarily a user choice: VS Code Copilot and
                    // Claude Code share root .mcp.json, so an entry can
                    // exist for an agent init never set up.
                    println!(
                        "WARN instructions missing codegraph block: {}",
                        inst.display()
                    );
                    println!(
                        "     fix: codegraph init --agent {} (re-applies the block)",
                        spec.id
                    );
                    any_warn = true;
                }
            }

            // 5. Index freshness.
            let db = default_db_path(&repo);
            match Store::open(&db) {
                Ok(store) => {
                    let indexed = store.get_meta("indexed_commit").ok().flatten();
                    let head = git_head_commit(&repo);
                    let fresh = codegraph_query::freshness(indexed.as_deref(), head.as_deref());
                    println!("PASS index: {}", fresh.header());
                }
                Err(e) => {
                    println!("WARN index not readable: {e}");
                    println!("     fix: codegraph index {}", repo.display());
                }
            }

            if !ok {
                any_fail = true;
            }
            println!();
        }
    }
    // An explicitly requested agent configured in no scope → actionable FAIL.
    for spec in &only {
        if !configured_ids.contains(&spec.id) {
            println!("--- {} (any scope) ---", spec.id);
            println!(
                "FAIL: not configured — run `codegraph init --agent {}`",
                spec.id
            );
            any_fail = true;
            any_checked = true;
        }
    }
    if !any_checked {
        println!("no configured agents found (project and user scope).");
        println!("run `codegraph init` to set up; `codegraph agents list` shows detection across scopes.");
    } else if any_fail {
        println!("doctor: problems found (see FAIL lines above).");
        return Ok(1);
    } else if any_warn {
        println!("doctor: all hard checks passed (warnings above are advisory).");
    } else {
        println!("doctor: all checks passed.");
    }
    Ok(0)
}

/// Tool parameter bundle for `codegraph query` (mirrors the MCP schemas).
#[derive(Debug, Default)]
pub struct QueryArgs {
    pub name: Option<String>,
    pub file: Option<String>,
    pub symbol: Option<String>,
    pub budget_tokens: Option<usize>,
}

/// `codegraph query <tool> [args...]` — the same text the MCP tools return.
/// Positional form: the first bare argument fills the primary parameter.
pub fn query(
    tool: &str,
    positional: Vec<String>,
    mut args: QueryArgs,
    path: Option<&Path>,
) -> Result<()> {
    let repo = canonical_repo(&resolve_repo(path));
    let db = default_db_path(&repo);
    // Positional fills the primary parameter of this tool.
    fn fill(field: &mut Option<String>, positional: &[String]) {
        if field.is_none() && !positional.is_empty() {
            *field = Some(positional.join(" "));
        }
    }
    match tool {
        "find_symbol" => fill(&mut args.name, &positional),
        "outline" => fill(&mut args.file, &positional),
        "callers" | "callees" | "get_context" => fill(&mut args.symbol, &positional),
        _ => {}
    }
    let store = Store::open(&db).with_context(|| format!("cannot open index {}", db.display()))?;
    let fresh = codegraph_query::freshness(
        store.get_meta("indexed_commit")?.as_deref(),
        git_head_commit(&repo).as_deref(),
    );
    let text = match tool {
        "find_symbol" => {
            let name = args.name.context("find_symbol needs --name (or a positional argument)")?;
            codegraph_query::find_symbol(&store, &fresh, &name, args.budget_tokens)?
        }
        "outline" => {
            let file = args.file.context("outline needs --file (or a positional argument)")?;
            codegraph_query::outline(&store, &fresh, &file, args.budget_tokens)?
        }
        "callers" => {
            let s = args.symbol.context("callers needs --symbol (or a positional argument)")?;
            codegraph_query::callers(&store, &fresh, &s, args.budget_tokens)?
        }
        "callees" => {
            let s = args.symbol.context("callees needs --symbol (or a positional argument)")?;
            codegraph_query::callees(&store, &fresh, &s, args.budget_tokens)?
        }
        "get_context" => {
            let s = args.symbol.context("get_context needs --symbol (or a positional argument)")?;
            codegraph_query::get_context(&store, &fresh, &s, args.budget_tokens)?
        }
        "status" => {
            let files = store.file_count()?;
            let symbols = store.symbol_count()?;
            let gaps = store.gaps()?.len();
            format!(
                "codegraph status\nfreshness: {}\nfiles: {files}  symbols: {symbols}  gaps: {gaps}\nindex: {}",
                fresh.header(),
                db.display()
            )
        }
        other => bail!(
            "unknown tool '{other}' — known tools: find_symbol, outline, callers, callees, get_context, status"
        ),
    };
    println!("{text}");
    Ok(())
}
