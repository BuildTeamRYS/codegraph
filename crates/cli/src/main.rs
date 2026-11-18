use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use codegraph_cli::{default_db_path, git_head_commit, index_repo};
use codegraph_store::Store;

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
    /// Print store status: schema version, freshness, counts, gaps.
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        db: Option<PathBuf>,
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
