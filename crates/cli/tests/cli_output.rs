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
        "FRIGG_SEMANTIC_RUNTIME_ENABLED",
        "FRIGG_SEMANTIC_RUNTIME_PROVIDER",
        "FRIGG_SEMANTIC_RUNTIME_MODEL",
        "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE",
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
    assert!(stdout.contains("init summary status=ok repositories=1"));
    assert!(!stdout.contains("init ok repository_id="));
    assert_eq!(stderr(&output), "");
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
    assert!(stdout.contains("index summary status=ok mode=full"));
    assert!(stderr.contains("index ok mode=full repository_id=repo-001"));
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
fn quiet_verbose_conflict_fails_before_machine_output() {
    let mut command = frigg_command();
    let output = command
        .args(["--quiet", "--verbose", "hash"])
        .output()
        .expect("run frigg binary");

    assert_failure(&output);
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("--quiet and --verbose cannot be used together"));
}

#[test]
fn startup_gate_failure_writes_summary_to_stderr_not_stdout() {
    let root = temp_workspace_root("startup-failure");
    fs::create_dir_all(&root).expect("create temp root");

    let output = run_frigg(&root, &[]);

    assert_failure(&output);
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("startup summary status=failed"));
    assert!(stderr.contains("storage db file is missing"));
    cleanup_workspace(&root);
}
