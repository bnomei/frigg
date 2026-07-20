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
    let root = std::env::temp_dir().join(format!(
        "frigg-cli-adopt-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".git")).expect("create fixture git root");
    root
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
    assert!(stdout(&output).contains("policy=lightweight"));
    assert!(!root.join("AGENTS.md").exists());
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_default_markdown_is_lightweight_and_expanded_opt_in() {
    let root = temp_workspace_root("policy-modes");
    fs::create_dir_all(&root).expect("create temp root");

    let lightweight = run_frigg(&root, &["adopt", "--target", "agents-md"]);
    assert_success(&lightweight);
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("read agents");
    assert!(agents.contains("frigg-directive:start version=2026-07-08"));
    assert!(agents.contains("frigg-first-code-search"));
    assert!(!agents.contains("## Compact scenario picker"));
    assert!(!agents.contains("Known string or regex -> search_text"));
    assert!(!agents.contains("Hard anti-patterns"));

    let expanded = run_frigg(
        &root,
        &["adopt", "--target", "agents-md", "--policy", "expanded"],
    );
    assert_success(&expanded);
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("read expanded agents");
    assert!(agents.contains("## Compact scenario picker"));
    assert!(agents.contains("Known string or regex -> search_text"));
    assert!(agents.contains("Shell → Frigg"));
    assert!(!agents.contains("BAD: hybrid -> grep"));

    let check_expanded = run_frigg(
        &root,
        &[
            "adopt",
            "--target",
            "agents-md",
            "--policy",
            "expanded",
            "--check",
        ],
    );
    assert_success(&check_expanded);
    assert!(stdout(&check_expanded).contains("action=unchanged"));
    assert!(stdout(&check_expanded).contains("policy=expanded"));

    let check_lightweight = run_frigg(&root, &["adopt", "--target", "agents-md", "--check"]);
    assert_failure(&check_lightweight);
    assert!(stdout(&check_lightweight).contains("status=pending"));

    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_check_fails_when_pending_and_passes_when_unchanged() {
    let root = temp_workspace_root("check");
    fs::create_dir_all(&root).expect("create temp root");

    let pending = run_frigg(&root, &["adopt", "--target", "agents-md", "--check"]);
    assert_failure(&pending);
    assert!(stdout(&pending).contains("status=pending"));
    let pending_stderr = stderr(&pending);
    assert!(pending_stderr.contains("error adopt: failed status=failed mode=check pending=1"));
    assert!(!pending_stderr.contains("Error:"));
    assert!(!root.join("AGENTS.md").exists());

    let applied = run_frigg(&root, &["adopt", "--target", "agents-md"]);
    assert_success(&applied);

    let unchanged = run_frigg(&root, &["adopt", "--target", "agents-md", "--check"]);
    assert_success(&unchanged);
    assert!(stdout(&unchanged).contains("action=unchanged"));
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_applies_markdown_cursor_and_mcp_targets_idempotently() {
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
        ],
    );
    assert_success(&first);
    assert!(stdout(&first).contains("writes=4"));
    assert!(!stdout(&first).contains("adopt plan"));

    for path in ["AGENTS.md", ".cursor/rules/frigg.mdc"] {
        let contents = fs::read_to_string(root.join(path)).expect("read adopted markdown");
        assert!(contents.contains("frigg-directive:start"));
        assert!(contents.contains("frigg-directive:end"));
    }
    assert!(
        !root.join(".cursorrules").exists(),
        "adopt should no longer create legacy Cursor rules"
    );

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
        ],
    );
    assert_success(&second);
    assert!(stdout(&second).contains("unchanged=4"));
    assert!(stdout(&second).contains("writes=0"));
    assert!(!stdout(&second).contains("adopt apply writes="));
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_rejects_legacy_cursor_surface() {
    let root = temp_workspace_root("reject-legacy-cursor");
    fs::create_dir_all(&root).expect("create temp root");

    let flag = run_frigg(&root, &["adopt", "--legacy-cursor"]);
    assert_failure(&flag);
    assert!(stderr(&flag).contains("--legacy-cursor"));

    let target = run_frigg(&root, &["adopt", "--target", "legacy-cursor"]);
    assert_failure(&target);
    assert!(stderr(&target).contains("legacy-cursor"));

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
    let cursor_rule = root.join(".cursor/rules/frigg.mdc");
    assert!(cursor_rule.exists(), "uninstall must not delete the file");
    assert!(
        fs::read_to_string(&cursor_rule)
            .expect("read cursor rule")
            .trim()
            .is_empty(),
        "the managed block should be gone, leaving no residue"
    );

    let removed_again = run_frigg(&root, &["adopt", "--target", "cursor", "--uninstall"]);
    assert_success(&removed_again);
    assert!(stdout(&removed_again).contains("unchanged=1"));
    assert!(stdout(&removed_again).contains("writes=0"));
    assert!(
        cursor_rule.exists(),
        "repeated uninstall must keep the file"
    );
    assert!(
        fs::read_to_string(&cursor_rule)
            .expect("read cursor rule after repeated uninstall")
            .is_empty(),
        "repeated uninstall must leave the file empty"
    );

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
            .expect("parse mcp");
    assert!(value["mcpServers"].get("frigg").is_none());
    assert_eq!(value["mcpServers"]["other"]["command"], "other");
    assert_eq!(value["unrelated"], true);
    cleanup_workspace(&root);
}

#[test]
fn adopt_cli_honors_custom_mcp_http_port() {
    let root = temp_workspace_root("custom-mcp-port");
    fs::create_dir_all(&root).expect("create temp root");

    let output = run_frigg(
        &root,
        &[
            "adopt",
            "--target",
            "mcp-project",
            "--mcp-http-port",
            "5000",
        ],
    );
    assert_success(&output);

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".mcp.json")).expect("read mcp"))
            .expect("parse mcp");
    assert_eq!(
        value["mcpServers"]["frigg"]["url"],
        "http://127.0.0.1:5000/mcp"
    );
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

/// The Claude skills-directory plugin already carries an MCP server and a PreToolUse hook.
/// Combining it with the standalone project targets double-registers both, so adopt must say so —
/// and must stay quiet when there is no overlap.
#[test]
fn adopt_cli_warns_only_when_claude_plugin_overlaps_project_targets() {
    let root = temp_workspace_root("claude-plugin-overlap");
    let home = root.join("fake-home");
    fs::create_dir_all(home.join(".claude/skills")).expect("create claude skills parent");
    let skill_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("skills/frigg-first-code-search");
    if !skill_source.join(".claude-plugin/plugin.json").is_file() {
        cleanup_workspace(&root);
        return;
    }

    let run = |args: &[&str]| -> Output {
        Command::new(frigg_bin())
            .arg("--workspace-root")
            .arg(&root)
            .args(args)
            .env("HOME", &home)
            .env("FRIGG_SKILL_SOURCE", &skill_source)
            .output()
            .expect("run frigg binary")
    };

    const WARNING: &str = "claude-duplicate-wiring";

    // Plugin only: nothing to collide with.
    let plugin_only = run(&[
        "adopt",
        "--target",
        "claude-md",
        "--skill-provider",
        "claude",
    ]);
    assert_success(&plugin_only);
    assert!(
        !stdout(&plugin_only).contains(WARNING),
        "plugin alone must not warn\nstdout:\n{}",
        stdout(&plugin_only)
    );

    // Plugin plus the targets it already provides: warn.
    let overlapping = run(&[
        "adopt",
        "--target",
        "mcp-project",
        "--target",
        "hook",
        "--skill-provider",
        "claude",
    ]);
    assert_success(&overlapping);
    let overlap_output = stdout(&overlapping);
    assert!(
        overlap_output.contains(WARNING),
        "overlapping wiring must warn\nstdout:\n{overlap_output}"
    );
    assert!(
        overlap_output.contains("mcp-project") && overlap_output.contains("hook"),
        "warning should name the overlapping targets\nstdout:\n{overlap_output}"
    );

    // Wiring left by an EARLIER run, with no target passed now. `hook` is excluded from marker
    // detection, so this is invisible to the plan and must be detected on disk instead.
    fs::remove_dir_all(home.join(".claude/skills/frigg-first-code-search"))
        .expect("remove installed plugin");
    let hook_first = run(&["adopt", "--target", "hook"]);
    assert_success(&hook_first);
    let plugin_after_hook = run(&["adopt", "--skill-provider", "claude"]);
    assert_success(&plugin_after_hook);
    let after_hook_output = stdout(&plugin_after_hook);
    assert!(
        after_hook_output.contains(WARNING) && after_hook_output.contains("hook"),
        "a hook adopted by an earlier run must still be reported\nstdout:\n{after_hook_output}"
    );

    // Same targets without the plugin: nothing to collide with.
    let _ = fs::remove_dir_all(home.join(".claude/skills/frigg-first-code-search"));
    let targets_only = run(&["adopt", "--target", "mcp-project", "--target", "hook"]);
    assert_success(&targets_only);
    assert!(
        !stdout(&targets_only).contains(WARNING),
        "targets without the plugin must not warn\nstdout:\n{}",
        stdout(&targets_only)
    );

    cleanup_workspace(&root);
}
