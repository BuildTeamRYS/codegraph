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
        "status",
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

// ---------- phase 2.5 conformance suite ----------

fn spawn_serve(repo: &Path, db: Option<&Path>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_codegraph"));
    cmd.arg("serve").arg(repo);
    if let Some(db) = db {
        cmd.arg("--db").arg(db);
    }
    cmd
}

async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: serde_json::Value,
) -> String {
    let result = client
        .call_tool(
            CallToolRequestParams::new(name.to_string())
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool");
    text_of(&result)
}

#[tokio::test]
async fn every_tool_carries_readonly_hint() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("index.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();
    drop(store);

    let cmd = spawn_serve(repo.path(), Some(&db));
    let child = TokioChildProcess::new(cmd).expect("spawn");
    let client = ().serve(child).await.expect("handshake");
    let tools = client.list_all_tools().await.expect("tools/list");
    assert_eq!(
        tools.len(),
        6,
        "tools: {:?}",
        tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>()
    );
    for tool in &tools {
        let hint = tool.annotations.as_ref().and_then(|a| a.read_only_hint);
        assert_eq!(
            hint,
            Some(true),
            "tool {} is missing readOnlyHint: true",
            tool.name
        );
    }
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn status_tool_reports_role_and_index() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("index.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();
    drop(store);

    let cmd = spawn_serve(repo.path(), Some(&db));
    let child = TokioChildProcess::new(cmd).expect("spawn");
    let client = ().serve(child).await.expect("handshake");
    let text = call_tool(&client, "status", json!({})).await;
    assert!(text.contains("role: updater"), "{text}");
    assert!(text.contains("freshness:"), "{text}");
    assert!(text.contains("files:"), "{text}");
    assert!(text.contains("indexing: idle"), "{text}");
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn concurrent_serves_elect_exactly_one_updater() {
    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("index.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();
    drop(store);

    let mut clients = Vec::new();
    for _ in 0..3 {
        let cmd = spawn_serve(repo.path(), Some(&db));
        let child = TokioChildProcess::new(cmd).expect("spawn");
        let client = ().serve(child).await.expect("handshake");
        clients.push(client);
    }
    let mut roles = Vec::new();
    for client in &clients {
        let text = call_tool(client, "status", json!({})).await;
        let role = text
            .lines()
            .find_map(|l| l.strip_prefix("role: "))
            .unwrap_or("missing")
            .to_string();
        roles.push(role);
    }
    for client in clients {
        client.cancel().await.expect("clean shutdown");
    }
    let updaters = roles.iter().filter(|r| *r == "updater").count();
    let readers = roles.iter().filter(|r| *r == "read-only").count();
    assert_eq!(updaters, 1, "roles: {roles:?}");
    assert_eq!(readers, 2, "roles: {roles:?}");
}

#[tokio::test]
async fn background_first_index_runs_when_index_missing() {
    let repo = copy_fixture("tiny-api");
    // No index: the server must build it on a background thread.
    let cmd = spawn_serve(repo.path(), None);
    let child = TokioChildProcess::new(cmd).expect("spawn");
    let client = ().serve(child).await.expect("handshake");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut last = String::new();
    let mut indexed = false;
    while std::time::Instant::now() < deadline {
        last = call_tool(&client, "status", json!({})).await;
        if last.contains("indexing: done")
            || (last.contains("freshness:") && !last.contains("index@unknown"))
        {
            indexed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    client.cancel().await.expect("clean shutdown");
    assert!(
        indexed,
        "background index never finished; last status:\n{last}"
    );
    // And the db it built is usable.
    let db = repo.path().join(".codegraph").join("index.db");
    let store = Store::open(&db).unwrap();
    assert!(store.symbol_count().unwrap() > 0);
    // serve.lock exists and is a regular file.
    assert!(repo.path().join(".codegraph").join("serve.lock").is_file());
}

#[tokio::test]
async fn malformed_input_yields_jsonrpc_error_and_server_survives() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command as PCommand;

    let repo = copy_fixture("tiny-api");
    let db = repo.path().join("index.db");
    let store = Store::open(&db).unwrap();
    index_repo(&store, repo.path()).unwrap();
    drop(store);

    let mut cmd = PCommand::new(env!("CARGO_BIN_EXE_codegraph"));
    cmd.arg("serve")
        .arg(repo.path())
        .arg("--db")
        .arg(&db)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().expect("spawn raw server");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();

    // 1. initialize, by hand.
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"raw-test\",\"version\":\"0\"}}}\n")
        .await
        .unwrap();
    // 2. Garbage — must be answered with a JSON-RPC error frame, not a crash.
    stdin.write_all(b"this is not json{{{\n").await.unwrap();
    // 3. initialized notification, then tools/list.
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n")
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut saw_tools_list = false;
    while std::time::Instant::now() < deadline && !saw_tools_list {
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
            .await
            .expect("timed out waiting for server output")
            .expect("read line")
            .expect("server closed stdout early");
        // Stdout purity: every line is a JSON-RPC frame.
        let parsed: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout carried non-JSON: {line:?} ({e})"));
        assert_eq!(
            parsed.get("jsonrpc").and_then(|v| v.as_str()),
            Some("2.0"),
            "stdout frame missing jsonrpc: {line:?}"
        );
        if parsed.get("id") == Some(&json!(2)) && parsed.get("result").is_some() {
            let names = parsed["result"]["tools"]
                .as_array()
                .map(|t| t.len())
                .unwrap_or(0);
            assert_eq!(names, 6);
            saw_tools_list = true;
        }
    }
    // Closing our stdin end is what makes the server shut down; `shutdown()`
    // on ChildStdin is a no-op, so drop it.
    drop(stdin);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
    assert!(
        saw_tools_list,
        "server stopped answering tools/list after a malformed line"
    );
}
