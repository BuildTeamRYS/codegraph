use codegraph_cli::index_repo;
use codegraph_store::Store;

fn main() {
    let root = std::env::args().nth(1).unwrap();
    let db = std::env::args().nth(2).unwrap();
    let _ = std::fs::remove_file(&db);
    let store = Store::open(std::path::Path::new(&db)).unwrap();
    let stats = index_repo(&store, std::path::Path::new(&root)).unwrap();
    println!(
        "resolved={} unresolved={} unattributed={}",
        stats.calls_resolved, stats.calls_unresolved, stats.calls_unattributed
    );
}
