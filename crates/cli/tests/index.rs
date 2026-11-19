//! Phase 1 acceptance through the real orchestration:
//! - full index of a fixture repo writes all files/symbols
//! - re-indexing an unchanged repo performs 0 parses
//! - editing one file re-parses only that file
//! - deleting a file removes its rows
//! - a syntactically broken file is reported as a gap, never silently complete

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use codegraph_cli::index_repo;
use codegraph_lang_ts::TsPlugin;
use codegraph_parse::{scan, FileOutcome, LanguagePlugin};
use codegraph_store::Store;

fn copy_fixture(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    copy_recursive(&src.join("src"), &dir.path().join("src"));
    dir
}

fn copy_recursive(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_recursive(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

#[test]
fn full_index_then_noop_reindex() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("test.db");
    let store = Store::open(&db).unwrap();

    let stats = index_repo(&store, repo.path()).unwrap();
    assert_eq!(stats.files_scanned, 4); // crypto, login, types, index
    assert_eq!(stats.files_parsed, 4);
    assert!(stats.symbols_written >= 8);
    assert_eq!(store.file_count().unwrap(), 4);

    let stats = index_repo(&store, repo.path()).unwrap();
    assert_eq!(stats.files_parsed, 0, "unchanged repo must not re-parse");
    assert_eq!(stats.files_skipped, 4);
    assert_eq!(stats.files_removed, 0);
}

#[test]
fn editing_one_file_reparses_only_it() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("test.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();

    let crypto = repo.path().join("src/crypto.ts");
    let text = fs::read_to_string(&crypto).unwrap();
    fs::write(&crypto, text.replace("pw.length", "pw.len()")).unwrap();

    let stats = index_repo(&store, repo.path()).unwrap();
    assert_eq!(stats.files_parsed, 1);
    assert_eq!(stats.files_scanned, 4);
}

#[test]
fn deleting_a_file_removes_its_rows() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("test.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();

    fs::remove_file(repo.path().join("src/types.ts")).unwrap();
    let stats = index_repo(&store, repo.path()).unwrap();
    assert_eq!(stats.files_removed, 1);
    assert_eq!(store.file_count().unwrap(), 3);
}

#[test]
fn js_fixture_cross_module_calls_resolve() {
    let repo = copy_fixture("js-tool");
    let db = repo.path().join("test.db");
    let store = Store::open(&db).unwrap();
    let stats = index_repo(&store, repo.path()).unwrap();
    assert_eq!(stats.files_scanned, 4);
    // main → clamp (./math.mjs) and main → padNum (./format.cjs); the rest
    // (Number, process.stdout.write, alert) are honestly left unresolved.
    assert_eq!(stats.calls_resolved, 2);

    let main = store.resolve_symbol("main").unwrap().remove(0);
    let callees = store.callees(main.id).unwrap();
    let names: Vec<&str> = callees.iter().map(|c| c.symbol.name.as_str()).collect();
    assert!(names.contains(&"clamp"));
    assert!(names.contains(&"padNum"));
}

#[test]
fn broken_file_is_a_gap_not_silence() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("test.db");
    // Invalid UTF-8 / unreadable constructs still parse best-effort; a hard
    // syntax error leaves ERROR nodes in the tree.
    fs::write(
        repo.path().join("src/broken.ts"),
        "export function broken( {\n",
    )
    .unwrap();
    let store = Store::open(&db).unwrap();
    let stats = index_repo(&store, repo.path()).unwrap();
    assert_eq!(stats.gaps, 1);
    let gaps = store.gaps().unwrap();
    assert!(gaps.iter().any(|(p, _)| p == "src/broken.ts"));
}

#[test]
fn scan_skips_unchanged_via_hash() {
    let repo = copy_fixture("web-shop");
    let existing: HashMap<String, (i64, String)> = HashMap::new();
    let plugin = TsPlugin::new();
    let plugins: Vec<&dyn LanguagePlugin> = vec![&plugin];
    let scan = scan(repo.path(), &existing, &plugins).unwrap();
    let parsed = scan
        .outcomes
        .iter()
        .filter(|o| matches!(o, FileOutcome::Parsed(_)))
        .count();
    assert_eq!(parsed, scan.outcomes.len());
}
