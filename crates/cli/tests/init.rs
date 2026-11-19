//! `codegraph init` / `codegraph agents` end-to-end (phase 2.5).
//!
//! Binary-driven: run `codegraph` as a subprocess against a temp repo with
//! an isolated HOME. Covers detection, dry-run, apply-with-confirmation,
//! merge-not-clobber, byte-for-byte restore on remove, the shim, .gitignore
//! offer, mcp-config output, and the query CLI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    home: PathBuf,
    /// Empty dir used as PATH so tests don't detect real agent binaries.
    path_env: PathBuf,
}

fn sandbox() -> Sandbox {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let home = tmp.path().join("home");
    let path_env = tmp.path().join("path");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&path_env).unwrap();
    Sandbox {
        _tmp: tmp,
        repo,
        home,
        path_env,
    }
}

fn codegraph(sb: &Sandbox, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codegraph"))
        .args(args)
        .current_dir(&sb.repo)
        // Deterministic detection: no agent binaries are ever on PATH here.
        .env("PATH", &sb.path_env)
        .env("HOME", &sb.home)
        .env("USERPROFILE", &sb.home)
        .env("XDG_CONFIG_HOME", sb.home.join(".config"))
        .env("LOCALAPPDATA", sb.home.join("AppData").join("Local"))
        .env("APPDATA", sb.home.join("AppData").join("Roaming"))
        .output()
        .expect("spawn codegraph")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn write(repo: &Path, rel: &str, content: &str) {
    let path = repo.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn empty_repo_detects_nothing_and_says_so() {
    let sb = sandbox();
    let out = codegraph(&sb, &["init", "--dry-run"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("no supported agents detected"), "{t}");
}

#[test]
fn dry_run_prints_diff_and_writes_nothing() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    let out = codegraph(&sb, &["init", "--dry-run"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("claude-code"), "{t}");
    assert!(t.contains("+"), "diff missing: {t}");
    assert!(
        !sb.repo.join(".mcp.json").exists(),
        "dry run must not write"
    );
}

#[test]
fn init_yes_configures_claude_code_and_writes_instruction_block() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    let out = codegraph(&sb, &["init", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    let raw = fs::read_to_string(sb.repo.join(".mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let entry = &doc["mcpServers"]["codegraph"];
    assert_eq!(entry["args"][0], "serve");
    // Shim lives in the fake HOME and is what the config points at:
    //   Unix:    <home>/.codegraph/bin/codegraph
    //   Windows: <home>/AppData/Local/codegraph/bin/codegraph.exe
    let shim = if cfg!(windows) {
        sb.home
            .join("AppData")
            .join("Local")
            .join("codegraph")
            .join("bin")
            .join("codegraph.exe")
    } else {
        sb.home.join(".codegraph").join("bin").join("codegraph")
    };
    assert!(shim.is_file(), "shim missing: {t}");
    assert_eq!(entry["command"], serde_json::json!(shim.to_string_lossy()));
    // Instruction block landed in CLAUDE.md.
    let claude_md = fs::read_to_string(sb.repo.join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("<!-- codegraph:start -->"));
    // Post-install guidance printed.
    assert!(t.contains("next steps for Claude Code"), "{t}");
}

#[test]
fn second_init_is_idempotent() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    assert!(codegraph(&sb, &["init", "--yes"]).status.success());
    let before = fs::read_to_string(sb.repo.join(".mcp.json")).unwrap();
    let out = codegraph(&sb, &["init", "--yes"]);
    let t = text(&out);
    assert!(t.contains("already configured"), "{t}");
    assert_eq!(
        fs::read_to_string(sb.repo.join(".mcp.json")).unwrap(),
        before
    );
}

#[test]
fn init_merges_into_existing_config_and_remove_restores_bytes() {
    let sb = sandbox();
    let original = "{\n  \"keepTop\": true,\n  \"mcpServers\": {\n    \"other\": {\n      \"command\": \"/opt/other\",\n      \"args\": [\"run\"]\n    }\n  }\n}\n";
    write(&sb.repo, ".mcp.json", original);
    let out = codegraph(&sb, &["init", "--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    let merged = fs::read_to_string(sb.repo.join(".mcp.json")).unwrap();
    assert!(merged.contains("\"keepTop\": true"));
    assert!(merged.contains("\"other\""));
    assert!(merged.contains("\"codegraph\""));
    assert!(sb.repo.join(".mcp.json.codegraph.bak").exists());

    let out = codegraph(&sb, &["agents", "remove", "claude-code"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("restored to the exact pre-install bytes"), "{t}");
    assert_eq!(
        fs::read_to_string(sb.repo.join(".mcp.json")).unwrap(),
        original
    );
}

#[test]
fn agents_remove_deletes_files_init_created() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    assert!(codegraph(&sb, &["init", "--yes"]).status.success());
    let out = codegraph(&sb, &["agents", "remove", "claude-code"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(
        !sb.repo.join(".mcp.json").exists(),
        "init-created file must be removed: {t}"
    );
    assert!(
        !sb.repo.join("CLAUDE.md").exists(),
        "init-created file must be removed: {t}"
    );
}

#[test]
fn codex_project_scope_writes_toml() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".codex")).unwrap();
    let out = codegraph(&sb, &["init", "--yes", "--agent", "codex-cli"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    let raw = fs::read_to_string(sb.repo.join(".codex").join("config.toml")).unwrap();
    assert!(raw.contains("[mcp_servers.codegraph]"), "{raw}");
    assert!(raw.contains("serve"), "{raw}");
}

#[test]
fn opencode_writes_array_command_under_mcp_servers() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".opencode")).unwrap();
    let out = codegraph(&sb, &["init", "--yes", "--agent", "opencode"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    let raw = fs::read_to_string(sb.repo.join("opencode.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let entry = &doc["mcp"]["servers"]["codegraph"];
    assert_eq!(entry["type"], "local");
    assert_eq!(
        entry["command"][0],
        doc["mcp"]["servers"]["codegraph"]["command"][0].clone()
    );
    assert_eq!(entry["command"][1], "serve");
    let _ = entry;
}

#[test]
fn explicit_agent_on_blank_machine_still_configures() {
    let sb = sandbox();
    // No markers, nothing on PATH we rely on: explicit ask still works.
    let out = codegraph(&sb, &["init", "--yes", "--agent", "gemini-cli"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(
        sb.repo.join(".gemini").join("settings.json").exists(),
        "{t}"
    );
}

#[test]
fn mcp_config_prints_json_toml_yaml() {
    let sb = sandbox();
    for (fmt, needle) in [
        ("json", "\"mcpServers\""),
        ("toml", "[mcp_servers.codegraph]"),
        ("yaml", "mcpServers:"),
    ] {
        let out = codegraph(&sb, &["mcp-config", "--format", fmt]);
        let t = text(&out);
        assert!(out.status.success(), "{t}");
        assert!(t.contains(needle), "{t}");
        assert!(t.contains("codegraph"), "{t}");
    }
}

#[test]
fn agents_list_shows_scopes_and_detection() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    let out = codegraph(&sb, &["agents", "list"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("claude-code"), "{t}");
    assert!(t.contains("project scope"), "{t}");
    assert!(t.contains("user scope"), "{t}");
    assert!(t.contains("claude-desktop"), "{t}");
}

#[test]
fn gitignore_gets_codegraph_appended_when_present() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    write(&sb.repo, ".gitignore", "target/\n");
    let out = codegraph(&sb, &["init", "--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    let gitignore = fs::read_to_string(sb.repo.join(".gitignore")).unwrap();
    assert!(gitignore.contains("target/\n.codegraph/"), "{gitignore}");
}

#[test]
fn query_reports_same_text_as_mcp_tools() {
    let sb = sandbox();
    // Build a tiny fixture index inside the temp repo.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tiny-api/src");
    let dst = sb.repo.join("src");
    fs::create_dir_all(&dst).unwrap();
    for entry in fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
    }
    let out = codegraph(&sb, &["index", &sb.repo.to_string_lossy()]);
    assert!(out.status.success(), "{}", text(&out));

    let out = codegraph(&sb, &["query", "find_symbol", "login"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("login@src/login.ts:9-12"), "{t}");
    assert!(t.contains("index@"), "{t}");

    let out = codegraph(
        &sb,
        &["query", "get_context", "login", "--budget-tokens", "400"],
    );
    let t = text(&out);
    assert!(t.contains("escape: src/login.ts:9-12"), "{t}");

    let out = codegraph(&sb, &["query", "outline", "--file", "cart.ts"]);
    let t = text(&out);
    assert!(t.contains("outline"), "{t}");

    let out = codegraph(&sb, &["query", "callees", "--symbol", "login"]);
    let t = text(&out);
    assert!(t.contains("[inferred]"), "{t}");

    let out = codegraph(&sb, &["query", "status"]);
    let t = text(&out);
    assert!(t.contains("files:"), "{t}");
}

#[test]
fn doctor_flags_broken_config_with_a_fix() {
    let sb = sandbox();
    // Hand-write a broken config: unparseable JSONC without comments allowed.
    write(&sb.repo, ".mcp.json", "{ this is not json ]");
    let out = codegraph(&sb, &["doctor", "--agent", "claude-code"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(1), "{t}");
    assert!(t.contains("FAIL"), "{t}");
    assert!(t.contains("fix:"), "{t}");
}

#[test]
fn doctor_flags_missing_server_binary_with_a_fix() {
    let sb = sandbox();
    write(
        &sb.repo,
        ".mcp.json",
        "{\"mcpServers\": {\"codegraph\": {\"command\": \"/nonexistent/codegraph\", \"args\": [\"serve\", \".\"]}}}\n",
    );
    let out = codegraph(&sb, &["doctor", "--agent", "claude-code"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(1), "{t}");
    assert!(t.contains("server binary not found"), "{t}");
    assert!(t.contains("fix:"), "{t}");
}

#[test]
fn doctor_passes_on_a_correct_setup() {
    let sb = sandbox();
    fs::create_dir_all(sb.repo.join(".claude")).unwrap();
    let out = codegraph(&sb, &["init", "--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    let out = codegraph(&sb, &["doctor"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(0), "{t}");
    assert!(t.contains("hard checks passed"), "{t}");
}
