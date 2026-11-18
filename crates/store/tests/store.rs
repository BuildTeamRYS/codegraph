use std::collections::HashSet;

use codegraph_core::{SymbolKind, SymbolRecord};
use codegraph_store::{Store, SCHEMA_VERSION};

fn sample_symbol(path: &str, name: &str) -> SymbolRecord {
    SymbolRecord {
        path: path.into(),
        kind: SymbolKind::Function,
        name: name.into(),
        qualified_name: name.into(),
        signature: "() -> void".into(),
        start_line: 1,
        end_line: 3,
        docstring: None,
        content_hash: hash_bytes_placeholder(name),
    }
}

fn hash_bytes_placeholder(s: &str) -> String {
    codegraph_core::hash_bytes(s.as_bytes())
}

#[test]
fn schema_version_is_written_and_migration_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    {
        let store = Store::open(&db).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }
    // Reopen: migration must be a no-op, not an error.
    let store = Store::open(&db).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn replace_file_roundtrip_and_delete() {
    let store = Store::in_memory().unwrap();
    let symbols = vec![
        sample_symbol("src/a.ts", "alpha"),
        sample_symbol("src/a.ts", "beta"),
    ];
    let written = store
        .replace_file(
            "src/a.ts",
            "typescript",
            "hash1",
            100,
            "now",
            false,
            false,
            &symbols,
            &[],
        )
        .unwrap();
    assert_eq!(written, 2);
    assert_eq!(store.symbol_count().unwrap(), 2);
    assert_eq!(store.file_count().unwrap(), 1);

    // Replacing the file drops its old rows.
    let written = store
        .replace_file(
            "src/a.ts",
            "typescript",
            "hash2",
            120,
            "now2",
            false,
            false,
            &symbols[..1],
            &[],
        )
        .unwrap();
    assert_eq!(written, 1);
    assert_eq!(store.symbol_count().unwrap(), 1);

    // Deleting the file from the seen set removes it and its symbols.
    let removed = store.remove_files_not_in(&HashSet::new()).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(store.symbol_count().unwrap(), 0);
    assert_eq!(store.file_count().unwrap(), 0);
}

#[test]
fn gaps_are_recorded_and_cleared() {
    let store = Store::in_memory().unwrap();
    store
        .record_gap("src/broken.ts", "syntax error at line 3")
        .unwrap();
    assert_eq!(
        store.gaps().unwrap(),
        vec![(
            "src/broken.ts".to_string(),
            "syntax error at line 3".to_string()
        )]
    );
    store.clear_gap("src/broken.ts").unwrap();
    assert!(store.gaps().unwrap().is_empty());
}
