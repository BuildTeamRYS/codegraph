//! PLAN.md phase 1 acceptance: the core schema contains no TypeScript-specific
//! columns. Language-specific data flows through free-form fields (kind TEXT,
//! value TEXT, free-form TEXT columns), never through dedicated TS/JSX columns.

use codegraph_store::Store;

#[test]
fn schema_has_no_language_specific_columns() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("index.db")).unwrap();

    // Banned identifiers: anything that only makes sense for one language
    // family. Free-form column names (kind, value, source_tool, lang) stay.
    // Exact-token match against identifiers in the DDL, not substrings —
    // `facts` must not trip `cts`.
    let banned = [
        "typescript",
        "javascript",
        "tsx",
        "jsx",
        "mts",
        "cts",
        "tsconfig",
        "mtsx",
        "ctsx",
        "enum_",
        "interface_",
        "d_ts",
    ];

    let ddl: Vec<String> = store.schema_ddl().unwrap();
    assert!(!ddl.is_empty(), "expected table DDL in sqlite_master");
    for statement in &ddl {
        let lower = statement.to_lowercase();
        let identifiers: Vec<&str> = lower
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|t| !t.is_empty())
            .collect();
        for ident in identifiers {
            assert!(
                !banned.contains(&ident),
                "schema identifier `{ident}` is language-specific:\n{statement}"
            );
        }
    }
}
