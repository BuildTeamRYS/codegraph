//! Phase 2 acceptance: the MCP server works end-to-end over stdio the same
//! way a real coding agent talks to it — initialize, tools/list, tools/call.

use std::fs;
use std::path::Path;
use tokio::process::Command;

use codegraph_cli::index_repo;
use codegraph_store::Store;
use rmcp::{model::CallToolRequestParams, transport::TokioChildProcess, ServiceExt};
use serde_json::json;

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

fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn mcp_server_lists_and_answers_tools() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("index.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();
    drop(store);

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codegraph"));
    cmd.arg("serve").arg(repo.path()).arg("--db").arg(&db);
    let child = TokioChildProcess::new(cmd).expect("spawn codegraph serve");

    let client = ().serve(child).await.expect("MCP initialize handshake");

    let tools = client.list_all_tools().await.expect("tools/list");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "find_symbol",
        "outline",
        "callers",
        "callees",
        "get_context",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}; have {names:?}"
        );
    }

    let find = client
        .call_tool(
            CallToolRequestParams::new("find_symbol")
                .with_arguments(json!({ "name": "login" }).as_object().unwrap().clone()),
        )
        .await
        .expect("find_symbol call");
    let text = text_of(&find);
    assert!(
        text.contains("login@src/login.ts:9-12"),
        "unexpected find_symbol output:\n{text}"
    );
    assert!(text.contains("index@"));

    let ctx = client
        .call_tool(
            CallToolRequestParams::new("get_context").with_arguments(
                json!({ "symbol": "login", "budget_tokens": 800 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("get_context call");
    let text = text_of(&ctx);
    assert!(text.contains("login@src/login.ts:9-12"), "{text}");
    assert!(text.contains("escape: src/login.ts:9-12"), "{text}");
    assert!(text.contains("[inferred]"), "{text}");

    let outline = client
        .call_tool(
            CallToolRequestParams::new("outline")
                .with_arguments(json!({ "file": "cart.ts" }).as_object().unwrap().clone()),
        )
        .await
        .expect("outline call (suffix match)");
    assert!(text_of(&outline).contains("outline"));

    // Unknown symbol must be a helpful zero-match answer, not a protocol error.
    let zero = client
        .call_tool(
            CallToolRequestParams::new("find_symbol").with_arguments(
                json!({ "name": "zzz_not_here_zzz" })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .expect("find_symbol zero-result call");
    assert!(text_of(&zero).contains("0 matches"));

    client.cancel().await.expect("clean shutdown");
}
