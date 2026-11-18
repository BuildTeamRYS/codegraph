use std::path::Path;

fn main() {
    let root = std::env::args()
        .nth(1)
        .expect("usage: dump_symbols <repo-root>");
    let existing = std::collections::HashMap::new();
    let scan = codegraph_parse::scan(Path::new(&root), &existing).expect("scan");
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    for outcome in scan.outcomes {
        if let codegraph_parse::FileOutcome::Parsed(p) = outcome {
            symbols.extend(p.symbols);
            imports.extend(p.imports);
        }
    }
    symbols.sort_by(|a, b| (&a.path, a.start_line, &a.name).cmp(&(&b.path, b.start_line, &b.name)));
    imports.sort_by(|a, b| (&a.path, a.line, &a.source).cmp(&(&b.path, b.line, &b.source)));
    // Exclude content_hash from the dump: hashes can't be hand-verified and
    // are covered by dedicated hashing tests instead.
    let strip = |arr: serde_json::Value| {
        arr.as_array()
            .unwrap()
            .iter()
            .map(|v| {
                let mut o = v.as_object().unwrap().clone();
                o.remove("content_hash");
                serde_json::Value::Object(o)
            })
            .collect::<Vec<_>>()
    };
    let syms = strip(serde_json::to_value(&symbols).unwrap());
    let imps = serde_json::to_value(&imports).unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "symbols": syms,
            "imports": imps,
        }))
        .unwrap()
    );
}
