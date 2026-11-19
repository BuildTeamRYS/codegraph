//! `codegraph init` acceptance (phase 2): detection by filesystem markers,
//! config creation, merge-preserving-existing, idempotence, and never
//! clobbering an unparseable config.

use std::fs;
use std::path::{Path, PathBuf};

use codegraph_cli::init::*;

fn repo() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn fake_exe(dir: &Path) -> PathBuf {
    let exe = dir.join("codegraph");
    fs::write(&exe, "fake").unwrap();
    exe
}

#[test]
fn empty_repo_detects_nothing() {
    let r = repo();
    let h = home();
    let dets = detect(r.path(), h.path(), false, false);
    assert!(dets.is_empty());
}

#[test]
fn claude_detected_via_dot_claude_dir() {
    let r = repo();
    let h = home();
    fs::create_dir_all(r.path().join(".claude")).unwrap();
    let dets = detect(r.path(), h.path(), false, false);
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].agent, AgentKind::Claude);
    assert_eq!(dets[0].config_path, r.path().join(".mcp.json"));
    assert!(!dets[0].existed);
}

#[test]
fn apply_creates_claude_config() {
    let r = repo();
    let h = home();
    fs::create_dir_all(r.path().join(".claude")).unwrap();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(r.path(), h.path(), false, false);
    let outcomes = apply_all(&dets, &exe, r.path()).unwrap();
    assert_eq!(outcomes.len(), 1);

    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(r.path().join(".mcp.json")).unwrap()).unwrap();
    let entry = &doc["mcpServers"]["codegraph"];
    assert_eq!(entry["command"], exe.to_string_lossy().as_ref());
    assert_eq!(entry["args"][0], "serve");
    assert_eq!(entry["args"][1], r.path().to_string_lossy().as_ref());
}

#[test]
fn apply_merges_and_preserves_existing_servers() {
    let r = repo();
    let h = home();
    let existing = r#"{
      "keepTop": true,
      "mcpServers": {
        "other": { "command": "/opt/other", "args": ["run"] }
      }
    }"#;
    fs::write(r.path().join(".mcp.json"), existing).unwrap();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(r.path(), h.path(), false, false);
    assert_eq!(dets.len(), 1);
    assert!(dets[0].existed);

    let outcomes = apply_all(&dets, &exe, r.path()).unwrap();
    match &outcomes[0] {
        ApplyOutcome::Written { backup_path, .. } => {
            let backup = backup_path.as_ref().expect("backup written");
            assert_eq!(fs::read_to_string(backup).unwrap(), existing);
        }
        _ => panic!("expected Written"),
    }

    let raw = fs::read_to_string(r.path().join(".mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(doc["keepTop"], true);
    assert_eq!(doc["mcpServers"]["other"]["command"], "/opt/other");
    assert_eq!(
        doc["mcpServers"]["codegraph"]["command"],
        exe.to_string_lossy().as_ref()
    );
}

#[test]
fn apply_never_clobbers_unparseable_config() {
    let r = repo();
    let h = home();
    fs::write(r.path().join(".mcp.json"), "this is not json").unwrap();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(r.path(), h.path(), false, false);
    let outcomes = apply_all(&dets, &exe, r.path()).unwrap();
    assert!(matches!(
        outcomes[0],
        ApplyOutcome::SkippedUnparseable { .. }
    ));
    assert_eq!(
        fs::read_to_string(r.path().join(".mcp.json")).unwrap(),
        "this is not json"
    );
}

#[test]
fn opencode_project_config_merges_under_mcp_servers() {
    let r = repo();
    let h = home();
    let existing = r#"{
      "schema": "https://opencode.ai/config.json",
      "mcp": { "servers": { "existing": { "type": "local", "command": ["/bin/echo"] } } }
    }"#;
    fs::write(r.path().join("opencode.json"), existing).unwrap();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(r.path(), h.path(), false, false);
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].agent, AgentKind::OpenCode);

    apply_all(&dets, &exe, r.path()).unwrap();
    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(r.path().join("opencode.json")).unwrap()).unwrap();
    assert_eq!(doc["schema"], "https://opencode.ai/config.json");
    assert_eq!(doc["mcp"]["servers"]["existing"]["command"][0], "/bin/echo");
    let cg = &doc["mcp"]["servers"]["codegraph"];
    assert_eq!(cg["type"], "local");
    assert_eq!(cg["command"][0], exe.to_string_lossy().as_ref());
    assert_eq!(cg["command"][1], "serve");
}

#[test]
fn cursor_uses_cursor_dir_config() {
    let r = repo();
    let h = home();
    fs::create_dir_all(r.path().join(".cursor")).unwrap();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(r.path(), h.path(), false, false);
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].agent, AgentKind::Cursor);
    assert_eq!(
        dets[0].config_path,
        r.path().join(".cursor").join("mcp.json")
    );

    apply_all(&dets, &exe, r.path()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(r.path().join(".cursor").join("mcp.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        doc["mcpServers"]["codegraph"]["args"][1],
        r.path().to_string_lossy().as_ref()
    );
}

#[test]
fn second_run_reports_already_configured() {
    let r = repo();
    let h = home();
    fs::create_dir_all(r.path().join(".claude")).unwrap();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(r.path(), h.path(), false, false);
    apply_all(&dets, &exe, r.path()).unwrap();

    let dets = detect(r.path(), h.path(), false, false);
    assert_eq!(dets.len(), 1);
    assert!(dets[0].already_configured);
}

#[test]
fn apply_works_when_repo_path_contains_spaces() {
    let base = repo();
    let spaced = base.path().join("my repo");
    fs::create_dir_all(&spaced).unwrap();
    fs::create_dir_all(spaced.join(".claude")).unwrap();
    let h = home();
    let exe_dir = repo();
    let exe = fake_exe(exe_dir.path());

    let dets = detect(&spaced, h.path(), false, false);
    apply_all(&dets, &exe, &spaced).unwrap();
    let raw = fs::read_to_string(spaced.join(".mcp.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        doc["mcpServers"]["codegraph"]["args"][1],
        spaced.to_string_lossy().as_ref()
    );
}
