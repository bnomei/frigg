#![allow(clippy::panic)]

//! CLI integration tests for project-local Frigg adoption writes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-cli-adopt-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn frigg_bin() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_frigg")
        .map(PathBuf::from)
        .expect("CARGO_BIN_EXE_frigg should be set for integration tests")
}

fn run_frigg(root: &Path, args: &[&str]) -> Output {
    Command::new(frigg_bin())
        .arg("--workspace-root")
        .arg(root)
        .args(args)
        .output()
        .expect("run frigg binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

#[test]
fn adopt_cli_dry_run_prints_plan_and_writes_nothing() {
    let root = temp_workspace_root("dry-run");
    fs::create_dir_all(&root).expect("create temp root");

    let output = run_frigg(&root, &["adopt", "--target", "agents-md", "--dry-run"]);

    assert_success(&output);
    assert!(stdout(&output).contains("action=create"));
    assert!(!root.join("AGENTS.md").exists());
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_check_fails_when_pending_and_passes_when_unchanged() {
    let root = temp_workspace_root("check");
    fs::create_dir_all(&root).expect("create temp root");

    let pending = run_frigg(&root, &["adopt", "--target", "agents-md", "--check"]);
    assert_failure(&pending);
    assert!(stdout(&pending).contains("status=pending"));
    assert!(!root.join("AGENTS.md").exists());

    let applied = run_frigg(&root, &["adopt", "--target", "agents-md"]);
    assert_success(&applied);

    let unchanged = run_frigg(&root, &["adopt", "--target", "agents-md", "--check"]);
    assert_success(&unchanged);
    assert!(stdout(&unchanged).contains("action=unchanged"));
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_applies_markdown_cursor_legacy_cursor_and_mcp_targets_idempotently() {
    let root = temp_workspace_root("apply-targets");
    fs::create_dir_all(&root).expect("create temp root");

    let first = run_frigg(
        &root,
        &[
            "adopt",
            "--target",
            "agents-md",
            "--target",
            "cursor",
            "--target",
            "mcp-project",
            "--target",
            "mcp-cursor",
            "--legacy-cursor",
        ],
    );
    assert_success(&first);
    assert!(stdout(&first).contains("writes=5"));
    assert!(!stdout(&first).contains("adopt plan"));

    for path in ["AGENTS.md", ".cursor/rules/frigg.mdc", ".cursorrules"] {
        let contents = fs::read_to_string(root.join(path)).expect("read adopted markdown");
        assert!(contents.contains("frigg-directive:start"));
        assert!(contents.contains("frigg-directive:end"));
    }

    for path in [".mcp.json", ".cursor/mcp.json"] {
        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(path)).expect("read adopted mcp config"),
        )
        .expect("parse mcp config");
        assert_eq!(
            value["mcpServers"]["frigg"]["url"],
            "http://127.0.0.1:37444/mcp"
        );
    }

    let second = run_frigg(
        &root,
        &[
            "adopt",
            "--target",
            "agents-md",
            "--target",
            "cursor",
            "--target",
            "mcp-project",
            "--target",
            "mcp-cursor",
            "--legacy-cursor",
        ],
    );
    assert_success(&second);
    assert!(stdout(&second).contains("unchanged=5"));
    assert!(stdout(&second).contains("writes=0"));
    assert!(!stdout(&second).contains("adopt apply writes="));
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_uninstall_removes_only_frigg_owned_content() {
    let root = temp_workspace_root("uninstall");
    fs::create_dir_all(root.join(".cursor/rules")).expect("create cursor rules dir");
    fs::write(root.join("AGENTS.md"), "# Team notes\n").expect("write agents");

    let applied = run_frigg(
        &root,
        &[
            "adopt",
            "--target",
            "agents-md",
            "--target",
            "cursor",
            "--target",
            "mcp-project",
        ],
    );
    assert_success(&applied);

    fs::write(
        root.join(".mcp.json"),
        r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:37444/mcp"},"other":{"command":"other"}},"unrelated":true}"#,
    )
    .expect("add sibling server");

    let removed = run_frigg(
        &root,
        &[
            "adopt",
            "--target",
            "agents-md",
            "--target",
            "cursor",
            "--target",
            "mcp-project",
            "--uninstall",
        ],
    );
    assert_success(&removed);

    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("read agents");
    assert_eq!(agents, "# Team notes\n");
    assert!(!root.join(".cursor/rules/frigg.mdc").exists());

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
            .expect("parse mcp");
    assert!(value["mcpServers"].get("frigg").is_none());
    assert_eq!(value["mcpServers"]["other"]["command"], "other");
    assert_eq!(value["unrelated"], true);
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_force_replaces_diverged_frigg_mcp_entry() {
    let root = temp_workspace_root("force");
    fs::create_dir_all(&root).expect("create temp root");
    fs::write(
        root.join(".mcp.json"),
        r#"{"mcpServers":{"frigg":{"command":"custom"},"other":{"command":"other"}}}"#,
    )
    .expect("write diverged mcp");

    let skipped = run_frigg(&root, &["adopt", "--target", "mcp-project"]);
    assert_success(&skipped);
    assert!(stdout(&skipped).contains("skipped=1"));
    assert!(!stdout(&skipped).contains("adopt plan"));
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
            .expect("parse mcp");
    assert_eq!(value["mcpServers"]["frigg"]["command"], "custom");

    let forced = run_frigg(&root, &["adopt", "--target", "mcp-project", "--force"]);
    assert_success(&forced);
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
            .expect("parse mcp");
    assert_eq!(
        value["mcpServers"]["frigg"]["url"],
        "http://127.0.0.1:37444/mcp"
    );
    assert_eq!(value["mcpServers"]["other"]["command"], "other");
    cleanup_workspace(&root);
}
