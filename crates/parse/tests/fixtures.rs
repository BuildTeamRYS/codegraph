//! Phase 1 acceptance: extracted symbols match the hand-verified expected JSON.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use codegraph_parse::{scan, FileOutcome};
use serde_json::Value;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .unwrap()
}

fn dump_fixture(name: &str) -> (Value, Value) {
    let root = fixture_root().join(name);
    let scan = scan(&root, &HashMap::new()).expect("scan fixture");
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    for outcome in scan.outcomes {
        if let FileOutcome::Parsed(p) = outcome {
            symbols.extend(p.symbols);
            imports.extend(p.imports);
        }
    }
    symbols.sort_by(|a, b| (&a.path, a.start_line, &a.name).cmp(&(&b.path, b.start_line, &b.name)));
    imports.sort_by(|a, b| (&a.path, a.line, &a.source).cmp(&(&b.path, b.line, &b.source)));

    let mut syms: Vec<Value> = serde_json::to_value(&symbols)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    for s in &mut syms {
        s.as_object_mut().unwrap().remove("content_hash");
    }
    (Value::Array(syms), serde_json::to_value(&imports).unwrap())
}

fn expected(name: &str, file: &str) -> Value {
    let path = fixture_root().join(name).join(file);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn tiny_api_symbols_match_expected() {
    let (symbols, imports) = dump_fixture("tiny-api");
    assert_eq!(symbols, expected("tiny-api", "expected.symbols.json"));
    assert_eq!(imports, expected("tiny-api", "expected.imports.json"));
}

#[test]
fn web_shop_symbols_match_expected() {
    let (symbols, imports) = dump_fixture("web-shop");
    assert_eq!(symbols, expected("web-shop", "expected.symbols.json"));
    assert_eq!(imports, expected("web-shop", "expected.imports.json"));
}

#[test]
fn cli_tool_symbols_match_expected() {
    let (symbols, imports) = dump_fixture("cli-tool");
    assert_eq!(symbols, expected("cli-tool", "expected.symbols.json"));
    assert_eq!(imports, expected("cli-tool", "expected.imports.json"));
}
