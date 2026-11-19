use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use codegraph_cli::{default_db_path, git_head_commit, index_repo};
use codegraph_store::Store;

fn home_dir() -> Option<PathBuf> {
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

#[derive(Parser)]
#[command(
    name = "codegraph",
    version,
    about = "Local code knowledge base for coding agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Index a repository into its local store.
    Index {
        /// Repository root (default: current directory).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Database path (default: <repo>/.codegraph/index.db).
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Serve the index to coding agents over MCP stdio (all major MCP clients).
    Serve {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Print store status: schema version, freshness, counts, gaps.
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Self-update codegraph from GitHub Releases (verifies SHA-256 first).
    Update {
        /// Only report whether an update is available; do not install it.
        #[arg(long)]
        check: bool,
    },
    /// Register codegraph as an MCP server in detected coding agents
    /// (Claude Code, Cursor, OpenCode). Shows the change first; only writes
    /// after confirmation (or --yes).
    Init {
        /// Repository root (default: current directory).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Target user-global agent configs instead of project configs.
        #[arg(long)]
        global: bool,
        /// Only consider one agent: claude | cursor | opencode.
        #[arg(long)]
        agent: Option<String>,
        /// Apply without asking for confirmation.
        #[arg(long)]
        yes: bool,
        /// Print the proposed changes and exit without writing.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Index { path, db } => {
            let db = db.unwrap_or_else(|| default_db_path(&path));
            let store = Store::open(&db)?;
            let stats = index_repo(&store, &path)?;
            println!(
                "indexed: scanned={} parsed={} skipped={} removed={} symbols={} gaps={}",
                stats.files_scanned,
                stats.files_parsed,
                stats.files_skipped,
                stats.files_removed,
                stats.symbols_written,
                stats.gaps
            );
            println!("store: {}", db.display());
        }
        Command::Serve { path, db } => {
            let db = db.unwrap_or_else(|| default_db_path(&path));
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(codegraph_mcp::run_stdio(path, db))?;
        }
        Command::Init {
            path,
            global,
            agent,
            yes,
            dry_run,
        } => {
            use std::io::{IsTerminal, Write};
            let repo = path
                .canonicalize()
                .with_context(|| format!("cannot open repo {}", path.display()))?;
            let home = home_dir().context("cannot determine home directory")?;
            let exe = std::env::current_exe().context("cannot locate current executable")?;
            let filter = match agent.as_deref() {
                None => None,
                Some(id) => Some(codegraph_cli::init::AgentKind::from_id(id).with_context(
                    || format!("unknown agent '{id}' (expected claude|cursor|opencode)"),
                )?),
            };
            let dets = codegraph_cli::init::detect(&repo, &home, global, true);
            let dets: Vec<_> = dets
                .into_iter()
                .filter(|d| filter.map(|f| d.agent == f).unwrap_or(true))
                .collect();
            if dets.is_empty() {
                println!(
                    "codegraph init: no supported agents detected in {}",
                    repo.display()
                );
                println!(
                    "supported (project): Claude Code (.mcp.json), Cursor (.cursor/mcp.json), OpenCode (opencode.json); global mode: pass --global"
                );
                println!("see README.md for manual MCP configuration");
                return Ok(());
            }
            for det in &dets {
                let state = if det.already_configured {
                    "already configured"
                } else if det.existed {
                    "file exists — will merge"
                } else {
                    "new file"
                };
                println!(
                    "detected {}: {} ({state})",
                    det.agent.name(),
                    det.config_path.display()
                );
            }
            for det in &dets {
                println!(
                    "\n--- {} ({}) ---\n{}",
                    det.agent.name(),
                    det.config_path.display(),
                    codegraph_cli::init::proposed_json(det, &exe, &repo)
                );
            }
            let pending: Vec<_> = dets.into_iter().filter(|d| !d.already_configured).collect();
            if pending.is_empty() {
                println!(
                    "\ncodegraph is already configured for every detected agent; nothing to do."
                );
                return Ok(());
            }
            if dry_run {
                println!("\ndry run: nothing written. Re-run without --dry-run to apply (it will ask first).");
                return Ok(());
            }
            let confirmed = if yes {
                true
            } else if std::io::stdin().is_terminal() {
                print!("\nApply these changes? [y/N] ");
                std::io::stdout().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
            } else {
                println!(
                    "\nstdin is not a terminal; not applying anything. Re-run with --yes to apply."
                );
                false
            };
            if !confirmed {
                println!("not applying anything.");
                return Ok(());
            }
            let outcomes = codegraph_cli::init::apply_all(&pending, &exe, &repo)?;
            for outcome in outcomes {
                match outcome {
                    codegraph_cli::init::ApplyOutcome::Written {
                        config_path,
                        backup_path,
                    } => {
                        println!("wrote {}", config_path.display());
                        if let Some(b) = backup_path {
                            println!("backup: {}", b.display());
                        }
                    }
                    codegraph_cli::init::ApplyOutcome::SkippedUnparseable { config_path } => {
                        eprintln!(
                            "skipped {}: file is not valid JSON — left untouched",
                            config_path.display()
                        );
                    }
                }
            }
        }
        Command::Update { check } => {
            let current = env!("CARGO_PKG_VERSION");
            let outcome = codegraph_cli::update::update(current, check)?;
            println!("{}", outcome.message());
        }
        Command::Status { path, db } => {
            let db = db.unwrap_or_else(|| default_db_path(&path));
            if !db.exists() {
                println!("codegraph status");
                println!(
                    "store: {} (missing — run `codegraph index` first)",
                    db.display()
                );
                return Ok(());
            }
            let store = Store::open(&db)?;
            let schema = store.schema_version()?;
            let indexed_commit = store
                .get_meta("indexed_commit")?
                .unwrap_or_else(|| "unknown".into());
            let head = git_head_commit(&path).unwrap_or_else(|| "unknown".into());
            let freshness = if indexed_commit == "unknown" || head == "unknown" {
                "unknown"
            } else if indexed_commit == head {
                "fresh"
            } else {
                "stale"
            };
            println!("codegraph status");
            println!("schema_version: {schema}");
            println!("indexed_commit: {indexed_commit}");
            println!("head: {head}");
            println!("freshness: {freshness}");
            println!("files: {}", store.file_count()?);
            println!("symbols: {}", store.symbol_count()?);
            let gaps = store.gaps()?;
            println!("gaps: {}", gaps.len());
            for (path, error) in gaps {
                println!("  gap: {path} — {error}");
            }
        }
    }
    Ok(())
}
