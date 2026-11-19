//! Merge semantics: fixtures standing in for real user configs.
//! Every test here is an acceptance criterion of phase 2.5.

use codegraph_agents::edit;
use codegraph_agents::spec::{self, AGENTS};
use codegraph_agents::{self as agents, Scope};
use std::fs;
use std::path::Path;

const CLAUDE: &str = "claude-code";
const OPENCODE: &str = "opencode";
const CODEX: &str = "codex-cli";
const GEMINI: &str = "gemini-cli";
const VSCODE: &str = "vscode-copilot";

fn spec(id: &str) -> &'static spec::AgentSpec {
    agents::find(id).unwrap_or_else(|| panic!("no such agent {id}"))
}

fn cmd() -> (String, Vec<String>) {
    (
        "/home/u/.codegraph/bin/codegraph".to_string(),
        vec!["serve".to_string(), "/home/u/repo".to_string()],
    )
}

// ---------- upsert: JSON with existing content ----------

#[test]
fn json_merge_preserves_other_servers_and_formatting() {
    let (command, args) = cmd();
    let before = "{\n  \"mcpServers\": {\n    \"other-tool\": {\n      \"command\": \"/opt/other\",\n      \"args\": []\n    }\n  },\n  \"unrelated\": {\n    \"keep\": true\n  }\n}\n";
    let after = edit::upsert_entry(before, spec(CLAUDE), &command, &args).unwrap();
    // Other content survives untouched.
    assert!(after.contains("\"other-tool\""));
    assert!(after.contains("\"command\": \"/opt/other\""));
    assert!(after.contains("\"unrelated\""));
    assert!(after.contains("\"keep\": true"));
    // Our entry appears, once.
    assert_eq!(after.matches("\"codegraph\"").count(), 1);
    assert!(after.contains("\"command\": \"/home/u/.codegraph/bin/codegraph\""));
    // The pre-existing lines are byte-identical (we only append a new property).
    let before_lines = before.lines().collect::<Vec<_>>();
    for line in &before_lines {
        assert!(after.contains(line), "lost line: {line}");
    }
}

#[test]
fn json_upsert_is_idempotent() {
    let (command, args) = cmd();
    let once = edit::upsert_entry("{}\n", spec(CLAUDE), &command, &args).unwrap();
    let twice = edit::upsert_entry(&once, spec(CLAUDE), &command, &args).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn jsonc_merge_preserves_comments_and_trailing_commas() {
    let (command, args) = cmd();
    let before = "{\n  // my servers (do not remove)\n  \"mcpServers\": {\n    // already have this one\n    \"other-tool\": {\n      \"command\": \"/opt/other\",\n      \"args\": [],\n    },\n  },\n}\n";
    let after = edit::upsert_entry(before, spec(CLAUDE), &command, &args).unwrap();
    assert!(after.contains("// my servers (do not remove)"));
    assert!(after.contains("// already have this one"));
    assert!(after.contains("\"other-tool\""));
    assert_eq!(after.matches("\"codegraph\"").count(), 1);
}

#[test]
fn opencode_uses_nested_mcp_servers_and_array_command() {
    let (command, args) = cmd();
    let after = edit::upsert_entry("{}\n", spec(OPENCODE), &command, &args).unwrap();
    let v: serde_json::Value = serde_json::from_str(&after).unwrap();
    let entry = &v["mcp"]["servers"]["codegraph"];
    assert_eq!(entry["type"], "local");
    assert_eq!(
        entry["command"],
        serde_json::json!(["/home/u/.codegraph/bin/codegraph", "serve", "/home/u/repo"])
    );
}

#[test]
fn opencode_jsonc_comments_survive() {
    let (command, args) = cmd();
    let before = "{\n  // comment\n  \"mcp\": {\n    \"servers\": {\n      \"ctx7\": {\n        \"type\": \"local\",\n        \"command\": [\"npx\", \"-y\", \"ctx7\"],\n      },\n    },\n  },\n}\n";
    let after = edit::upsert_entry(before, spec(OPENCODE), &command, &args).unwrap();
    assert!(after.contains("// comment"));
    assert!(after.contains("\"ctx7\""));
    assert!(after.contains("\"codegraph\""));
}

#[test]
fn toml_merge_preserves_comments_and_adds_table() {
    let (command, args) = cmd();
    let before = "# codex config\nmodel = \"gpt-5\"\n\n[mcp_servers.other] # keep\ncommand = \"/opt/other\"\nargs = []\n";
    let after = edit::upsert_entry(before, spec(CODEX), &command, &args).unwrap();
    assert!(after.contains("# codex config"));
    assert!(after.contains("model = \"gpt-5\""));
    assert!(after.contains("# keep"));
    assert!(after.contains("[mcp_servers.other]"));
    assert!(after.contains("[mcp_servers.codegraph]"));
    assert!(after.contains("\"/home/u/.codegraph/bin/codegraph\""));
}

#[test]
fn toml_roundtrip_is_idempotent() {
    let (command, args) = cmd();
    let once = edit::upsert_entry("", spec(CODEX), &command, &args).unwrap();
    let twice = edit::upsert_entry(&once, spec(CODEX), &command, &args).unwrap();
    assert_eq!(once, twice);
}

// ---------- upsert: failure modes leave the file untouched ----------

#[test]
fn unparseable_json_is_rejected() {
    let (command, args) = cmd();
    let before = "{ \"mcpServers\": {\n  bad json here";
    let err = edit::upsert_entry(before, spec(CLAUDE), &command, &args).unwrap_err();
    assert!(matches!(err, edit::EditError::Unparseable { .. }));
}

#[test]
fn non_object_servers_key_is_rejected() {
    let (command, args) = cmd();
    let before = "{ \"mcpServers\": [\"not\", \"an\", \"object\"] }";
    let err = edit::upsert_entry(before, spec(CLAUDE), &command, &args).unwrap_err();
    assert!(matches!(err, edit::EditError::Invalid(_)));
}

#[test]
fn non_object_root_is_rejected() {
    let (command, args) = cmd();
    let before = "[\"array\", \"root\"]";
    let err = edit::upsert_entry(before, spec(CLAUDE), &command, &args).unwrap_err();
    assert!(matches!(err, edit::EditError::Invalid(_)));
}

#[test]
fn unparseable_toml_is_rejected() {
    let (command, args) = cmd();
    let err = edit::upsert_entry("this is not [toml", spec(CODEX), &command, &args).unwrap_err();
    assert!(matches!(err, edit::EditError::Unparseable { .. }));
}

// ---------- remove: byte-for-byte restore and surgical removal ----------

fn write_fixture(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

/// Repo and home are tempdirs; HOME expansion uses the injected home.
fn sandbox() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let home = dir.path().join("home");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&home).unwrap();
    (dir, repo, home)
}

#[test]
fn install_then_remove_restores_original_bytes() {
    let (_tmp, repo, home) = sandbox();
    let original = "{\n  \"mcpServers\": {\n    \"other-tool\": {\n      \"command\": \"/opt/other\",\n      \"args\": []\n    }\n  }\n}\n";
    let config = write_fixture(&repo, ".mcp.json", original);
    let (command, args) = cmd();

    let det = agents::detect(&repo, &home, Scope::Project, &[CLAUDE])
        .pop()
        .expect("claude-code detected via .mcp.json");
    let plan = agents::plan(&repo, &home, &det, &command, &args).unwrap();
    let outcomes = agents::apply(&[plan]).unwrap();
    assert!(matches!(outcomes[0], agents::ApplyOutcome::Written { .. }));
    assert_ne!(fs::read_to_string(&config).unwrap(), original);

    let report = agents::remove(&repo, &home, spec(CLAUDE), Scope::Project);
    assert_eq!(report.config.unwrap(), agents::RemoveStep::Restored);
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    // Instruction file created by us (CLAUDE.md) must be gone too.
    assert!(!repo.join("CLAUDE.md").exists());
}

#[test]
fn remove_is_surgical_when_user_edited_after_install() {
    let (_tmp, repo, home) = sandbox();
    let original = "{\n  \"mcpServers\": {\n    \"other-tool\": {\n      \"command\": \"/opt/other\",\n      \"args\": []\n    }\n  }\n}\n";
    let config = write_fixture(&repo, ".mcp.json", original);
    let (command, args) = cmd();

    let det = agents::detect(&repo, &home, Scope::Project, &[CLAUDE])
        .pop()
        .unwrap();
    let plan = agents::plan(&repo, &home, &det, &command, &args).unwrap();
    agents::apply(&[plan]).unwrap();

    // Simulate a user edit after our write: they add another server by hand.
    let mut after_user_edit = fs::read_to_string(&config).unwrap();
    after_user_edit = after_user_edit.replace(
        "\"other-tool\": {",
        "\"user-added\": {\n      \"command\": \"/opt/user\",\n      \"args\": []\n    },\n    \"other-tool\": {",
    );
    fs::write(&config, &after_user_edit).unwrap();

    let report = agents::remove(&repo, &home, spec(CLAUDE), Scope::Project);
    assert_eq!(report.config.unwrap(), agents::RemoveStep::Surgical);
    let final_text = fs::read_to_string(&config).unwrap();
    assert!(
        final_text.contains("\"user-added\""),
        "user's edit must survive"
    );
    assert!(final_text.contains("\"other-tool\""));
    assert!(!final_text.contains("\"codegraph\""));
}

#[test]
fn apply_refuses_to_clobber_a_file_changed_since_planning() {
    let (_tmp, repo, home) = sandbox();
    let config = write_fixture(&repo, ".mcp.json", "{}\n");
    let (command, args) = cmd();
    let det = agents::detect(&repo, &home, Scope::Project, &[CLAUDE])
        .pop()
        .unwrap();
    let plan = agents::plan(&repo, &home, &det, &command, &args).unwrap();
    // User edits between plan and apply.
    fs::write(&config, "{\n  \"hand\": \"edited\"\n}\n").unwrap();
    let outcomes = agents::apply(&[plan]).unwrap();
    assert!(matches!(outcomes[0], agents::ApplyOutcome::Skipped { .. }));
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "{\n  \"hand\": \"edited\"\n}\n"
    );
}

#[test]
fn toml_install_then_remove_restores_original_bytes() {
    let (_tmp, repo, home) = sandbox();
    let original = "# codex\nmodel = \"gpt-5\"\n";
    let config = write_fixture(&repo, ".codex/config.toml", original);
    let (command, args) = cmd();
    let det = agents::detect(&repo, &home, Scope::Project, &[CODEX])
        .pop()
        .unwrap();
    let plan = agents::plan(&repo, &home, &det, &command, &args).unwrap();
    agents::apply(&[plan]).unwrap();
    assert!(fs::read_to_string(&config)
        .unwrap()
        .contains("[mcp_servers.codegraph]"));

    let report = agents::remove(&repo, &home, spec(CODEX), Scope::Project);
    assert_eq!(report.config.unwrap(), agents::RemoveStep::Restored);
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
}

#[test]
fn opencode_project_jsonc_is_preferred_when_present() {
    let (_tmp, repo, home) = sandbox();
    write_fixture(&repo, "opencode.jsonc", "{\n  // jsonc\n}\n");
    write_fixture(&repo, "opencode.json", "{}\n");
    let path = agents::config_path(spec(OPENCODE), Scope::Project, &repo, &home).unwrap();
    assert_eq!(path, repo.join("opencode.jsonc"));
}

#[test]
fn backup_is_created_once_and_never_overwritten() {
    let (_tmp, repo, home) = sandbox();
    write_fixture(&repo, ".mcp.json", "{}\n");
    let (command, args) = cmd();
    let det = agents::detect(&repo, &home, Scope::Project, &[CLAUDE])
        .pop()
        .unwrap();
    let plan = agents::plan(&repo, &home, &det, &command, &args).unwrap();
    agents::apply(&[plan]).unwrap();
    let bak = repo.join(".mcp.json.codegraph.bak");
    assert!(bak.exists());
    let bak_bytes = fs::read(&bak).unwrap();
    // Apply again (plan was made before the first write; re-plan now).
    let det2 = agents::detect(&repo, &home, Scope::Project, &[CLAUDE])
        .pop()
        .unwrap();
    let plan2 = agents::plan(&repo, &home, &det2, &command, &args).unwrap();
    agents::apply(&[plan2]).unwrap();
    assert_eq!(
        fs::read(&bak).unwrap(),
        bak_bytes,
        "backup must never be overwritten"
    );
}

// ---------- registry sanity ----------

#[test]
fn table_has_all_nine_tier1_agents_with_unique_ids() {
    let ids: Vec<&str> = AGENTS.iter().map(|a| a.id).collect();
    for expected in [
        "claude-code",
        "cursor",
        "vscode-copilot",
        "codex-cli",
        "gemini-cli",
        "antigravity",
        "opencode",
        "windsurf",
        "claude-desktop",
    ] {
        assert!(ids.contains(&expected), "missing Tier 1 agent {expected}");
    }
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "duplicate agent ids");
    assert!(AGENTS.iter().all(|a| a.tier == 1));
}

#[test]
fn vscode_copilot_shares_root_mcp_json_with_claude_without_conflict() {
    // Both agents' project configs are root .mcp.json; entries must coexist
    // and neither clobbers the other's keys.
    let (command, args) = cmd();
    let det = |id: &str| agents::Detection {
        spec: spec(id),
        scope: Scope::Project,
        config_path: Path::new("/tmp/does-not-matter/.mcp.json").to_path_buf(),
        evidence: vec![],
        existed: false,
        already_configured: false,
        scope_supported: true,
    };
    let plan_c = agents::plan(
        Path::new("/tmp"),
        Path::new("/tmp"),
        &det(CLAUDE),
        &command,
        &args,
    )
    .unwrap();
    let merged =
        edit::upsert_entry(&plan_c.config.new_text, spec(VSCODE), &command, &args).unwrap();
    assert!(
        merged.matches("\"codegraph\"").count() == 1,
        "one entry total"
    );
    let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
    // Type must be the portable-stdio shape both agents accept.
    assert_eq!(v["mcpServers"]["codegraph"]["type"], "stdio");
}

#[test]
fn gemini_typeless_stdio_entry() {
    let (command, args) = cmd();
    let after = edit::upsert_entry("{}\n", spec(GEMINI), &command, &args).unwrap();
    let v: serde_json::Value = serde_json::from_str(&after).unwrap();
    let entry = &v["mcpServers"]["codegraph"];
    assert!(entry.get("type").is_none());
    assert_eq!(
        entry["command"],
        serde_json::json!("/home/u/.codegraph/bin/codegraph")
    );
    assert_eq!(entry["args"], serde_json::json!(["serve", "/home/u/repo"]));
}

#[test]
fn shim_path_layout_is_stable() {
    let home = Path::new("/home/example");
    let shim = agents::shim_path(home);
    let s = shim.to_string_lossy().replace('\\', "/");
    assert!(s.contains("codegraph"));
    assert!(s.ends_with("codegraph") || s.ends_with("codegraph.exe"));
}
