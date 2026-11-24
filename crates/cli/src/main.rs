use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use codegraph_agents::Scope;
use codegraph_cli::{default_db_path, git_head_commit, index_repo, resolve_repo};
use codegraph_store::Store;
use std::io::{self};

/// This function creates a friendly, step-by-step menu for non-technical users.
fn run_interactive_menu() -> anyhow::Result<()> {
    use inquire::{Select, Text};

    loop {
        let options = vec![
            "📁 Initialize a new project (Index & Setup)",
            "📊 Check project status",
            "🩺 Run health check (Doctor)",
            "❓ Query the code graph",
            "🔄 Update CodeGraph",
            "❌ Exit",
        ];

        let ans = match Select::new("Welcome to CodeGraph! What would you like to do?", options)
            .prompt()
        {
            Ok(val) => val,
            Err(_) => {
                println!("Goodbye!");
                break;
            }
        };

        match ans {
            "📁 Initialize a new project (Index & Setup)" => run_command(&["init"]),
            "📊 Check project status" => run_command(&["status"]),
            "🩺 Run health check (Doctor)" => run_command(&["doctor"]),
            "❓ Query the code graph" => {
                let tool = Text::new("Enter the action (e.g., find_symbol, callers):")
                    .prompt()
                    .unwrap_or_default();
                let args = Text::new("Enter details (e.g., function name):")
                    .prompt()
                    .unwrap_or_default();

                let mut final_args = vec!["query", &tool];
                if !args.is_empty() {
                    let args_vec: Vec<&str> = args.split_whitespace().collect();
                    final_args.extend(args_vec);
                }
                run_command(&final_args);
            }
            "🔄 Update CodeGraph" => run_command(&["update"]),
            "❌ Exit" => {
                println!("Goodbye!");
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

/// This helper runs the actual tool commands in the background,
/// so we don't have to rewrite any existing logic.
fn run_command(args: &[&str]) {
    println!("\n--- Running: codegraph {} ---", args.join(" "));

    // Use the full path here to avoid naming collisions with clap's Command enum
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(args);

    let status = cmd.status().expect("Failed to execute command");

    if !status.success() {
        eprintln!("Command completed with issues. Please check the output above.");
    }

    println!("\n--- Press Enter to return to the main menu ---");
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

#[derive(Clone, Copy, ValueEnum)]
enum ScopeArg {
    Project,
    User,
}

impl ScopeArg {
    fn into(self) -> Scope {
        match self {
            ScopeArg::Project => Scope::Project,
            ScopeArg::User => Scope::User,
        }
    }
}

impl From<Scope> for ScopeArg {
    fn from(s: Scope) -> Self {
        match s {
            Scope::Project => ScopeArg::Project,
            Scope::User => ScopeArg::User,
        }
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
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
        /// Database path (default: <repo>/.codegraph/index.db).
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Serve the index to coding agents over MCP stdio (all major MCP clients).
    ///
    /// The repo is chosen by --project, else $CODEGRAPH_PROJECT, else the
    /// nearest ancestor of the working directory containing .git. Several
    /// clients may serve one repo at once; the rest become read-only.
    Serve {
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
        /// Same as the positional argument, for scripts.
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Print store status: schema version, freshness, counts, gaps.
    Status {
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Self-update codegraph from GitHub Releases (verifies SHA-256 first).
    Update {
        /// Only report whether an update is available; do not install it.
        #[arg(long)]
        check: bool,
    },
    /// Index the repo, then register codegraph as an MCP server in detected
    /// coding agents. Shows the change first; only writes after confirmation
    /// (or --yes). Undo with `codegraph agents remove <id>`.
    Init {
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
        /// Config scope to write. Default: project (agents that support it).
        #[arg(long)]
        scope: Option<ScopeArg>,
        /// Shorthand for --scope user.
        #[arg(long)]
        global: bool,
        /// Only consider these agents (repeatable). Default: every detected
        /// Tier 1 agent. Known ids: claude-code, cursor, vscode-copilot,
        /// codex-cli, gemini-cli, antigravity, opencode, windsurf, claude-desktop.
        #[arg(long)]
        agent: Vec<String>,
        /// Apply without asking for confirmation.
        #[arg(long)]
        yes: bool,
        /// Print the proposed changes and exit without writing.
        #[arg(long)]
        dry_run: bool,
        /// Do not (re)build the index.
        #[arg(long)]
        no_index: bool,
    },
    /// Manage the codegraph MCP registration in coding agents.
    Agents {
        #[command(subcommand)]
        command: AgentsCommand,
    },
    /// Diagnose codegraph's setup for each configured agent (config parses,
    /// server answers MCP, tools carry readOnlyHint, instructions present,
    /// index fresh). Every finding says what to do next.
    Doctor {
        /// Only check these agents (repeatable).
        #[arg(long)]
        agent: Vec<String>,
        /// Check only this scope. Default: both.
        #[arg(long)]
        scope: Option<ScopeArg>,
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
    },
    /// Print a ready-to-paste MCP config snippet for any MCP client.
    McpConfig {
        /// Snippet format.
        #[arg(long, default_value = "json", value_parser = ["json", "toml", "yaml"])]
        format: String,
        /// Repository root the example commands will point at.
        path: Option<PathBuf>,
    },
    /// Call an MCP tool from the command line, with the exact same terse
    /// output the MCP server returns. For agents without MCP support.
    Query {
        /// Tool to call: find_symbol | outline | callers | callees | get_context | status.
        tool: String,
        /// Positional primary argument (e.g. the symbol name).
        #[arg(trailing_var_arg = false)]
        args: Vec<String>,
        /// find_symbol: the name to search.
        #[arg(long)]
        name: Option<String>,
        /// outline: repo-relative path or unique path suffix.
        #[arg(long)]
        file: Option<String>,
        /// callers | callees | get_context: symbol name.
        #[arg(long)]
        symbol: Option<String>,
        /// Token budget for the response (default 800).
        #[arg(long)]
        budget_tokens: Option<usize>,
        /// Repository root (default: nearest ancestor of cwd containing .git).
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AgentsCommand {
    /// List Tier 1 agents and whether they are detected/configured.
    List {
        /// Scope to inspect. Default: both.
        #[arg(long)]
        scope: Option<ScopeArg>,
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
    },
    /// Remove codegraph from an agent: config entry and instruction block.
    /// Restores the original file bytes when nothing else changed them.
    Remove {
        /// Agent id (e.g. claude-code).
        id: String,
        /// Scope to edit. Default: project where supported.
        #[arg(long)]
        scope: Option<ScopeArg>,
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
    },
    /// Diagnose codegraph's setup for each configured agent.
    Doctor {
        /// Only check these agents (repeatable).
        #[arg(long)]
        agent: Vec<String>,
        /// Scope to inspect. Default: both.
        #[arg(long)]
        scope: Option<ScopeArg>,
        /// Repository root (default: nearest ancestor of cwd containing .git).
        path: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    // Well-mannered Unix CLI: if our downstream closes the pipe early
    // (e.g. `codegraph status | head -1`), exit quietly instead of panicking
    // on the println! EPIPE.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if msg.contains("Broken pipe") || msg.contains("broken pipe") {
            std::process::exit(0);
        }
        eprintln!("panic: {msg}");
        std::process::exit(101);
    }));

    let args: Vec<String> = std::env::args().collect();

    // `codegraph .` or `codegraph /some/path` → open the menu in that folder.
    // Matches normal Linux conventions: the tool follows the folder you point it at.
    if args.len() == 2 {
        let candidate = std::path::Path::new(&args[1]);
        if candidate.is_dir() {
            if let Err(e) = std::env::set_current_dir(candidate) {
                eprintln!("codegraph: cannot enter {}: {}", candidate.display(), e);
                std::process::exit(1);
            }
            return run_interactive_menu();
        }
    }

    // If the user just typed "codegraph" with no extra words, show the friendly menu.
    if std::env::args().len() == 1 {
        return run_interactive_menu();
    }

    let cli = Cli::parse();
    match cli.command {
        Command::Index { path, db } => {
            let repo = resolve_repo(path.as_deref());
            let db = db.unwrap_or_else(|| default_db_path(&repo));
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
            println!("store: {}", db.display());
        }
        Command::Serve { path, project, db } => {
            let repo = resolve_repo(project.as_deref().or(path.as_deref()));
            let db = db.unwrap_or_else(|| default_db_path(&repo));
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(codegraph_mcp::run_stdio(repo, db))?;
        }
        Command::Init {
            path,
            scope,
            global,
            agent,
            yes,
            dry_run,
            no_index,
        } => {
            let report = codegraph_cli::setup::init(&codegraph_cli::setup::InitOptions {
                path,
                scope: scope.map(ScopeArg::into),
                global,
                agents: agent,
                yes,
                dry_run,
                skip_index: no_index,
            })?;
            for (id, reason) in &report.skipped {
                eprintln!("skipped {id}: {reason}");
            }
        }
        Command::Agents { command } => match command {
            AgentsCommand::List { scope, path } => {
                codegraph_cli::setup::agents_list(scope.map(ScopeArg::into), path.as_deref())?;
            }
            AgentsCommand::Remove { id, scope, path } => {
                codegraph_cli::setup::agents_remove(
                    &id,
                    scope.map(ScopeArg::into),
                    path.as_deref(),
                )?;
            }
            AgentsCommand::Doctor { agent, scope, path } => {
                let code = codegraph_cli::setup::doctor(
                    &agent,
                    scope.map(ScopeArg::into),
                    path.as_deref(),
                )?;
                std::process::exit(code);
            }
        },
        Command::Doctor { agent, scope, path } => {
            let code =
                codegraph_cli::setup::doctor(&agent, scope.map(ScopeArg::into), path.as_deref())?;
            std::process::exit(code);
        }
        Command::McpConfig { format, path } => {
            codegraph_cli::setup::mcp_config(&format, path.as_deref())?;
        }
        Command::Query {
            tool,
            args,
            name,
            file,
            symbol,
            budget_tokens,
            path,
        } => {
            codegraph_cli::setup::query(
                &tool,
                args,
                codegraph_cli::setup::QueryArgs {
                    name,
                    file,
                    symbol,
                    budget_tokens,
                },
                path.as_deref(),
            )?;
        }
        Command::Update { check } => {
            let current = env!("CARGO_PKG_VERSION");
            let outcome = codegraph_cli::update::update(current, check)?;
            println!("{}", outcome.message());
        }
        Command::Status { path, db } => {
            let repo = resolve_repo(path.as_deref());
            let db = db.unwrap_or_else(|| default_db_path(&repo));
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
            let head = git_head_commit(&repo).unwrap_or_else(|| "unknown".into());
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
