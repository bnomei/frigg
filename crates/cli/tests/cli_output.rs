#![allow(clippy::panic)]

//! CLI integration tests for stdout/stderr output mode policy.

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
        "frigg-cli-output-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn create_simple_workspace(root: &Path) {
    fs::create_dir_all(root.join(".git")).expect("create git marker");
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write source file");
    fs::write(root.join("README.md"), "# Fixture\n").expect("write readme");
}

fn frigg_bin() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_frigg")
        .map(PathBuf::from)
        .expect("CARGO_BIN_EXE_frigg should be set for integration tests")
}

fn frigg_command() -> Command {
    let mut command = Command::new(frigg_bin());
    for name in [
        "FRIGG_MAX_FILE_BYTES",
        "FRIGG_FULL_SCIP_INGEST",
        "FRIGG_MCP_HTTP_AUTH_TOKEN",
        "FRIGG_SEMANTIC_RUNTIME_ENABLED",
        "FRIGG_SEMANTIC_RUNTIME_PROVIDER",
        "FRIGG_SEMANTIC_RUNTIME_MODEL",
        "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE",
        "FRIGG_WATCH_MODE",
        "FRIGG_LEXICAL_BACKEND",
        "FRIGG_RIPGREP_EXECUTABLE",
        "FRIGG_WATCH_DEBOUNCE_MS",
        "FRIGG_WATCH_RETRY_MS",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "RUST_LOG",
        "FRIGG_STARTUP_TRACE",
    ] {
        command.env_remove(name);
    }
    command
}

fn run_frigg(root: &Path, args: &[&str]) -> Output {
    let mut command = frigg_command();
    command.arg("--workspace-root").arg(root).args(args);
    command.output().expect("run frigg binary")
}

fn run_frigg_with_path(root: &Path, args: &[&str], path: &Path) -> Output {
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![path.to_path_buf()];
    path_entries.extend(std::env::split_paths(&inherited_path));
    let path_value = std::env::join_paths(path_entries).expect("test PATH should be joinable");
    let mut command = frigg_command();
    command
        .env("PATH", path_value)
        .arg("--workspace-root")
        .arg(root)
        .args(args);
    command.output().expect("run frigg binary")
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
fn init_normal_emits_summary_only_on_stdout() {
    let root = temp_workspace_root("init-normal");
    create_simple_workspace(&root);

    let output = run_frigg(&root, &["init"]);

    assert_success(&output);
    let stdout = stdout(&output);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1, "unexpected stdout: {stdout:?}");
    assert!(stdout.contains("ok init: complete status=ok repos=1"));
    assert!(!stdout.contains("ok init: repo"));
    assert_eq!(stderr(&output), "");
    cleanup_workspace(&root);
}

#[cfg(unix)]
#[test]
fn init_verbose_generates_precise_artifact() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_workspace_root("init-precise");
    create_simple_workspace(&root);
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write cargo manifest");

    let bin_dir = root.join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("create fake bin dir");
    let rust_analyzer = bin_dir.join("rust-analyzer");
    fs::write(
        &rust_analyzer,
        r#"#!/bin/sh
if [ "$1" = "--version" ] || [ "$1" = "version" ]; then
  printf '%s\n' "rust-analyzer test"
  exit 0
fi
if [ "$1" = "scip" ]; then
  printf '%s' "fake-rust-scip" > index.scip
  exit 0
fi
exit 1
"#,
    )
    .expect("write fake rust-analyzer");
    let mut permissions = fs::metadata(&rust_analyzer)
        .expect("fake rust-analyzer metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&rust_analyzer, permissions).expect("fake rust-analyzer executable");

    let output = run_frigg_with_path(&root, &["--verbose", "init"], &bin_dir);

    assert_success(&output);
    let stdout = stdout(&output);
    let stderr = stderr(&output);
    assert!(stdout.contains("ok init: complete status=ok repos=1"));
    assert!(stdout.contains("precise_generators=1"));
    assert!(stdout.contains("precise_succeeded=1"));
    assert!(stderr.contains("info precise: plan status=starting repo=repo-001 command=init"));
    assert!(
        stderr
            .contains("ok precise: generator status=ok repo=repo-001 command=init generator=rust")
    );
    assert_eq!(
        fs::read(root.join(".frigg/scip/rust.scip")).expect("generated rust scip"),
        b"fake-rust-scip"
    );
    cleanup_workspace(&root);
}

#[test]
fn index_quiet_suppresses_success_chatter() {
    let root = temp_workspace_root("index-quiet");
    create_simple_workspace(&root);

    let output = run_frigg(&root, &["--quiet", "index"]);

    assert_success(&output);
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "");
    cleanup_workspace(&root);
}

#[test]
fn index_verbose_emits_progress_on_stderr() {
    let root = temp_workspace_root("index-verbose");
    create_simple_workspace(&root);

    let output = run_frigg(&root, &["--verbose", "index"]);

    assert_success(&output);
    let stdout = stdout(&output);
    let stderr = stderr(&output);
    assert!(stdout.contains("ok index: complete status=ok mode=full"));
    assert!(stderr.contains("info index: plan status=starting repo=repo-001"));
    assert!(stderr.contains("ok index: repo status=ok repo=repo-001"));
    cleanup_workspace(&root);
}

#[test]
fn index_changed_verbose_emits_changed_paths_on_stderr() {
    let root = temp_workspace_root("index-changed-verbose");
    create_simple_workspace(&root);

    let first = run_frigg(&root, &["index"]);
    assert_success(&first);
    fs::write(
        root.join("src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .expect("update source file");

    let output = run_frigg(&root, &["--verbose", "index", "--changed"]);

    assert_success(&output);
    let stdout = stdout(&output);
    let stderr = stderr(&output);
    assert!(stdout.contains("ok index: complete status=ok mode=changed"));
    assert!(stderr.contains("info index: path action=modified repo=repo-001 -- src/main.rs"));
    let path_position = stderr
        .find("info index: path action=modified repo=repo-001 -- src/main.rs")
        .expect("verbose changed index should stream changed path");
    let repo_position = stderr
        .find("ok index: repo status=ok repo=repo-001")
        .expect("verbose changed index should emit repo completion");
    assert!(
        path_position < repo_position,
        "changed path events should be emitted before repo completion:\n{stderr}"
    );
    cleanup_workspace(&root);
}

#[test]
fn verify_subcommand_is_not_registered() {
    let root = temp_workspace_root("verify-removed");
    create_simple_workspace(&root);

    let output = run_frigg(&root, &["verify"]);

    assert_failure(&output);
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("unrecognized subcommand") || stderr.contains("unexpected argument"));
    assert!(stderr.contains("verify"));
    assert!(
        !root.join(".frigg/storage.sqlite3").exists(),
        "removed verify command should not touch storage"
    );
    cleanup_workspace(&root);
}

#[test]
fn storage_maintenance_commands_are_hidden_from_normal_help() {
    let output = frigg_command()
        .arg("--help")
        .output()
        .expect("run frigg help");

    assert_success(&output);
    assert_eq!(stderr(&output), "");
    let stdout = stdout(&output);
    assert!(stdout.contains("init"));
    assert!(stdout.contains("index"));
    assert!(stdout.contains("serve"));
    assert!(
        !stdout.contains("repair-storage"),
        "repair-storage should stay hidden from normal help:\n{stdout}"
    );
    assert!(
        !stdout.contains("prune-storage"),
        "prune-storage should stay hidden from normal help:\n{stdout}"
    );
}

#[test]
fn help_inlines_stable_defaults_for_env_and_context_options() {
    let output = frigg_command()
        .arg("--help")
        .output()
        .expect("run frigg help");

    assert_success(&output);
    assert_eq!(stderr(&output), "");
    let top_help = stdout(&output);
    for expected in [
        "[env: FRIGG_MAX_FILE_BYTES=2097152]",
        "[env: FRIGG_FULL_SCIP_INGEST=true]",
        "[env: FRIGG_SEMANTIC_RUNTIME_ENABLED=false]",
        "[env: FRIGG_SEMANTIC_RUNTIME_PROVIDER=local]",
        "[env: FRIGG_SEMANTIC_RUNTIME_MODEL=provider default]",
        "[env: FRIGG_SEMANTIC_RUNTIME_STRICT_MODE=false]",
        "[env: FRIGG_WATCH_MODE=auto]",
        "[env: FRIGG_LEXICAL_BACKEND=auto]",
        "[env: FRIGG_RIPGREP_EXECUTABLE=PATH lookup]",
        "[env: FRIGG_WATCH_DEBOUNCE_MS=2000]",
        "[env: FRIGG_WATCH_RETRY_MS=5000]",
    ] {
        assert!(
            top_help.contains(expected),
            "missing {expected:?} in help:\n{top_help}"
        );
    }

    let output = frigg_command()
        .args(["context", "--help"])
        .output()
        .expect("run frigg context help");

    assert_success(&output);
    assert_eq!(stderr(&output), "");
    let context_help = stdout(&output);
    assert!(context_help.contains("Defaults to `now - Duration::days(30)`"));
    assert!(context_help.contains("Defaults to `now`"));
    assert!(context_help.contains("--json"));
}

#[test]
fn context_summary_defaults_to_compact_saved_percent_and_json_alias_keeps_full_summary() {
    let root = temp_workspace_root("context-summary-compact");
    fs::create_dir_all(root.join(".git")).expect("create git marker");
    fs::create_dir_all(root.join(".frigg")).expect("create frigg dir");
    fs::write(
        root.join(".frigg/context.jsonl"),
        r#"{"timestamp":"2026-06-10T00:00:00Z","tool":"read_file","repository_id":"repo-1","snapshot_id":"snapshot-1","indexed_readable_files":2,"indexed_readable_bytes":200,"returned_unique_paths":1,"returned_unique_file_bytes":100,"returned_source_bytes_estimate":12,"matched_file_context_saved_bytes_estimate":88,"matched_file_context_saved_percent_estimate":88.0,"narrowing_ratio_estimate":8}"#,
    )
    .expect("context log should be written");

    let compact = run_frigg(
        &root,
        &["context", "--since", "2026-06-01", "--until", "2026-07-01"],
    );

    assert_success(&compact);
    assert_eq!(stdout(&compact), "88% saved, 1 tool call, 31 days\n");
    assert_eq!(stderr(&compact), "");

    let json = run_frigg(
        &root,
        &[
            "content",
            "--json",
            "--since",
            "2026-06-01",
            "--until",
            "2026-07-01",
        ],
    );

    assert_success(&json);
    assert_eq!(stderr(&json), "");
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&json)).expect("json summary should parse");
    assert_eq!(value["totals"]["events"], 1);
    assert_eq!(
        value["totals"]["matched_file_context_saved_bytes_estimate"],
        88
    );
    assert!(!stdout(&json).contains("88% saved"));
    cleanup_workspace(&root);
}

#[test]
fn status_json_reports_missing_storage_without_creating_it() {
    let root = temp_workspace_root("status-missing-storage");
    create_simple_workspace(&root);

    let output = run_frigg(&root, &["status", "--json"]);

    assert_success(&output);
    assert_eq!(stderr(&output), "");
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("status JSON should parse");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["frigg_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["repositories"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        value["repositories"][0]["storage"]["index_state"],
        "missing_db"
    );
    assert!(
        !root.join(".frigg").exists(),
        "status must not create storage"
    );
    cleanup_workspace(&root);
}

#[test]
fn quiet_verbose_conflict_fails_before_machine_output() {
    let mut command = frigg_command();
    let output = command
        .args(["--quiet", "--verbose", "hash"])
        .output()
        .expect("run frigg binary");

    assert_failure(&output);
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("error frigg: failed status=failed"));
    assert!(stderr.contains("--quiet and --verbose cannot be used together"));
    assert!(!stderr.contains("Error:"));
}

#[test]
fn startup_gate_failure_writes_summary_to_stderr_not_stdout() {
    let root = temp_workspace_root("startup-failure");
    fs::create_dir_all(root.join(".git")).expect("create git marker");

    let output = run_frigg(&root, &[]);

    assert_failure(&output);
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("error startup: failed status=failed"));
    assert!(stderr.contains("storage db file is missing"));
    assert!(!stderr.contains("Error:"));
    cleanup_workspace(&root);
}

#[test]
fn config_path_failure_uses_structured_error_line() {
    let root = temp_workspace_root("init-missing-root");

    let output = run_frigg(&root, &["init"]);

    assert_failure(&output);
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("error frigg: failed status=failed"));
    assert!(stderr.contains("workspace root does not exist"));
    assert!(!stderr.contains("Error:"));
}
