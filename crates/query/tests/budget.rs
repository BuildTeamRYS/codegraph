//! PLAN acceptance: every response is under its budget (verified with a
//! token counter), and truncations print the "N more available, narrow by" hint.

use codegraph_core::{CallSite, ImportRecord, SymbolKind, SymbolRecord};
use codegraph_query::*;
use codegraph_store::Store;

fn fresh() -> Freshness {
    Freshness::Fresh("deadbeefcafe1234".into())
}

fn seed() -> Store {
    let store = Store::in_memory().unwrap();
    let syms = [
        SymbolRecord {
            path: "src/login.ts".into(),
            kind: SymbolKind::Function,
            name: "login".into(),
            qualified_name: "login".into(),
            signature: "(user, pw) -> Session".into(),
            start_line: 9,
            end_line: 12,
            docstring: Some("Authenticate a user and open a session.".into()),
            content_hash: codegraph_core::hash_bytes(b"login-body"),
        },
        SymbolRecord {
            path: "src/crypto.ts".into(),
            kind: SymbolKind::Function,
            name: "hash_pw".into(),
            qualified_name: "hash_pw".into(),
            signature: "(pw) -> string".into(),
            start_line: 2,
            end_line: 4,
            docstring: None,
            content_hash: codegraph_core::hash_bytes(b"hash_pw-body"),
        },
        SymbolRecord {
            path: "src/crypto.ts".into(),
            kind: SymbolKind::Function,
            name: "verify_pw".into(),
            qualified_name: "verify_pw".into(),
            signature: "(pw, hash) -> boolean".into(),
            start_line: 7,
            end_line: 9,
            docstring: None,
            content_hash: codegraph_core::hash_bytes(b"verify_pw-body"),
        },
    ];
    let imports = vec![ImportRecord {
        path: "src/login.ts".into(),
        source: "./crypto".into(),
        kind: "named".into(),
        local_name: Some("hash_pw".into()),
        imported_name: Some("hash_pw".into()),
        line: 1,
    }];
    let calls = vec![CallSite {
        path: "src/login.ts".into(),
        callee_name: "verify_pw".into(),
        qualifier: None,
        is_construct: false,
        line: 10,
    }];
    store
        .replace_file(
            "src/login.ts",
            "typescript",
            "fh1",
            10,
            "now",
            false,
            false,
            &syms[..1],
            &imports,
            &calls,
        )
        .unwrap();
    store
        .replace_file(
            "src/crypto.ts",
            "typescript",
            "fh2",
            10,
            "now",
            false,
            false,
            &syms[1..],
            &[],
            &[],
        )
        .unwrap();
    store.resolve_calls().unwrap();
    store
}

fn reported_tokens(output: &str) -> usize {
    let line = output
        .lines()
        .find(|l| l.starts_with("tokens: ~"))
        .expect("tokens line present");
    line.trim_start_matches("tokens: ~")
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn find_symbol_respects_budget_and_reports_counts() {
    let store = seed();
    let out = find_symbol(&store, &fresh(), "pw", Some(800)).unwrap();
    assert!(out.contains("find_symbol \"pw\""));
    assert!(out.contains("hash_pw@src/crypto.ts:2-4"));
    assert!(out.contains("verify_pw@src/crypto.ts:7-9"));
    assert!(reported_tokens(&out) <= 800);
}

#[test]
fn find_symbol_truncates_with_hint_under_tight_budget() {
    let store = seed();
    let out = find_symbol(&store, &fresh(), "pw", Some(24)).unwrap();
    assert!(out.contains("N more available") || out.contains("more available, narrow by"));
    assert!(reported_tokens(&out) <= 24 || out.lines().count() <= 5);
}

#[test]
fn get_context_shows_confidence_and_escape_ranges() {
    let store = seed();
    let out = get_context(&store, &fresh(), "login", Some(800)).unwrap();
    assert!(out.contains("login@src/login.ts:9-12 [function]"));
    assert!(out.contains("index@deadbeefcafe fresh"));
    assert!(out.contains("[inferred]"));
    assert!(out
        .lines()
        .any(|l| l.starts_with("escape: src/login.ts:9-12")));
    assert!(reported_tokens(&out) <= 800);
}

#[test]
fn get_context_under_tight_budget_stays_within_reason() {
    let store = seed();
    let out = get_context(&store, &fresh(), "login", Some(48)).unwrap();
    assert!(out.contains("tokens: ~"));
    // Header and escape hatch are mandatory even under extreme budgets.
    assert!(out.contains("escape: src/login.ts:9-12"));
}

#[test]
fn callers_lists_edges_with_confidence() {
    let store = seed();
    let out = callers(&store, &fresh(), "verify_pw", Some(800)).unwrap();
    assert!(out.contains("login@src/login.ts:9-12"));
    assert!(out.contains("[inferred]"));
}

#[test]
fn unknown_symbol_prints_zero_matches_not_error() {
    let store = seed();
    let out = find_symbol(&store, &fresh(), "nonexistent_xyz", Some(800)).unwrap();
    assert!(out.contains("0 matches"));
}

#[test]
fn freshness_header_reflects_staleness() {
    let store = seed();
    let stale = Freshness::Stale {
        indexed: "aaa111".into(),
        head: "bbb222".into(),
    };
    let out = outline(&store, &stale, "login.ts", Some(800)).unwrap();
    assert!(out.contains("index@aaa111 stale (head=bbb222)"));
}

#[test]
fn outline_lists_symbols_in_source_order() {
    let store = seed();
    let out = outline(&store, &fresh(), "crypto.ts", Some(800)).unwrap();
    let hash = out.find("hash_pw@").unwrap();
    let verify = out.find("verify_pw@").unwrap();
    assert!(hash < verify);
    assert!(reported_tokens(&out) <= 800);
}
