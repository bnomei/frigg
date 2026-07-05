#![allow(clippy::panic)]

//! Regression tests for precise-graph generation tasks, timeouts, and cache reuse after index.

use super::*;

#[test]
fn workspace_index_health_reports_rust_precise_generation_with_positional_path() {
    let workspace_root = temp_workspace_root("precise-generator-health");
    let bin_dir = temp_workspace_root("precise-generator-health-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source fixture");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write Cargo fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("failed to write source fixture");

    let _rust_analyzer = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "rust-analyzer",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "rust-analyzer 1.85.0"
  exit 0
fi
if [ "${1:-}" != "scip" ] || [ -z "${2:-}" ]; then
  printf '%s\n' "missing positional workspace path" >&2
  exit 42
fi
printf '%s' "fake-scip-rust"
"#,
    );
    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        assert_eq!(health.precise_generators.len(), 1);
        let rust_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("rust"))
            .expect("rust generator should be reported");
        assert!(
            rust_generator
                .tool
                .as_deref()
                .expect("rust generator should report a resolved tool")
                .ends_with("rust-analyzer"),
            "rust generator should report the resolved rust-analyzer tool"
        );
        assert_eq!(
            rust_generator
                .expected_output_path
                .as_deref()
                .expect("expected output path should be reported"),
            workspace
                .root
                .join(".frigg/scip/rust.scip")
                .display()
                .to_string()
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("Cargo.toml")],
            &[],
        );

        let expected_artifact = workspace.root.join(".frigg/scip/rust.scip");
        for _ in 0..200 {
            if expected_artifact.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            expected_artifact.is_file(),
            "fake rust-analyzer should have written a cached SCIP artifact"
        );
        assert_eq!(
            fs::read_to_string(&expected_artifact).expect("artifact should be readable"),
            "fake-scip-rust"
        );

        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_rust_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("rust"))
            .expect("rust generator should still be reported");
        assert!(
            refreshed_rust_generator.last_generation.is_some(),
            "successful generation should be cached even if preflight availability varies by environment"
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn precise_generation_failed_summary_does_not_suppress_retry() {
    let workspace_root = temp_workspace_root("precise-failed-retry");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source fixture");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write Cargo fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    let _active = server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::PreciseGenerate,
            workspace.repository_id.clone(),
            "precise_generation",
            None,
        );

    let summary_with_status = |status: crate::mcp::types::WorkspacePreciseGenerationStatus| {
        crate::mcp::types::WorkspacePreciseGenerationSummary {
            status,
            generated_at_ms: 0,
            duration_ms: None,
            artifact_path: None,
            artifact_count: None,
            artifact_bytes: None,
            artifact_sample_paths: Vec::new(),
            failure_class: None,
            recommended_action: None,
            detail: None,
        }
    };

    server.scip_cache_workspace_precise_generation(
        &workspace.repository_id,
        "rust",
        summary_with_status(crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded),
    );
    let action = server.maybe_spawn_workspace_precise_generation(&workspace, &[], &[]);
    assert!(
        matches!(
            action,
            crate::mcp::types::WorkspacePreciseGenerationAction::SkippedNoWork
        ),
        "a succeeded summary with no changes should be SkippedNoWork, got {action:?}"
    );

    server.scip_cache_workspace_precise_generation(
        &workspace.repository_id,
        "rust",
        summary_with_status(crate::mcp::types::WorkspacePreciseGenerationStatus::MissingTool),
    );
    let action = server.maybe_spawn_workspace_precise_generation(&workspace, &[], &[]);
    assert!(
        !matches!(
            action,
            crate::mcp::types::WorkspacePreciseGenerationAction::SkippedNoWork
        ),
        "a failed/missing-tool summary must not suppress retry, got {action:?}"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn precise_generation_records_dirty_paths_when_skipped_for_active_task() {
    let workspace_root = temp_workspace_root("precise-skip-records-dirty");
    let bin_dir = temp_workspace_root("precise-skip-records-dirty-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source fixture");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write Cargo fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("failed to write source fixture");

    let _rust_analyzer = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "rust-analyzer",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "rust-analyzer 1.85.0"
  exit 0
fi
printf '%s' "fake-scip-rust"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        let _active_task = server
            .runtime_state
            .runtime_task_registry
            .write()
            .expect("runtime task registry should not be poisoned")
            .start_task(
                RuntimeTaskKind::PreciseGenerate,
                workspace.repository_id.clone(),
                "precise_generation",
                Some("active task".to_owned()),
            );

        let action = server.maybe_spawn_workspace_precise_generation(
            &workspace,
            &[String::from("Cargo.toml")],
            &[String::from("old/dropped.rs")],
        );
        assert!(
            matches!(
                action,
                crate::mcp::types::WorkspacePreciseGenerationAction::SkippedActiveTask
            ),
            "expected SkippedActiveTask while a precise generation is in flight, got {action:?}"
        );

        let pending = server
            .take_pending_precise_dirty_paths(&workspace.repository_id)
            .expect("skipped dirty paths must be recorded for replay");
        assert_eq!(pending.0, vec!["Cargo.toml".to_owned()]);
        assert_eq!(pending.1, vec!["old/dropped.rs".to_owned()]);
        assert!(
            server
                .take_pending_precise_dirty_paths(&workspace.repository_id)
                .is_none()
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn precise_generation_spawn_failure_returns_failed_and_preserves_detail() {
    let workspace_root = temp_workspace_root("precise-spawn-failure");
    let bin_dir = temp_workspace_root("precise-spawn-failure-bin");
    fs::create_dir_all(workspace_root.join(".git")).expect("failed to create git fixture");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source fixture");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write Cargo fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("failed to write source fixture");

    let _rust_analyzer = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "rust-analyzer",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "rust-analyzer 1.85.0"
  exit 0
fi
printf '%s' "fake-scip-rust"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        server.scip_cache_workspace_precise_generation(
            &workspace.repository_id,
            "rust",
            crate::mcp::types::WorkspacePreciseGenerationSummary {
                status: crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded,
                generated_at_ms: 0,
                duration_ms: None,
                artifact_path: None,
                artifact_count: None,
                artifact_bytes: None,
                artifact_sample_paths: Vec::new(),
                failure_class: None,
                recommended_action: None,
                detail: Some("stale successful generation".to_owned()),
            },
        );

        FriggMcpServer::force_next_precise_generation_thread_spawn_failure();
        let action = server.maybe_spawn_workspace_precise_generation(
            &workspace,
            &[String::from("Cargo.toml")],
            &[],
        );
        assert!(matches!(
            action,
            crate::mcp::types::WorkspacePreciseGenerationAction::Failed
        ));

        let tasks = server
            .runtime_state
            .runtime_task_registry
            .read()
            .expect("runtime task registry should not be poisoned")
            .recent_tasks();
        let task = tasks
            .iter()
            .find(|task| {
                task.kind == RuntimeTaskKind::PreciseGenerate
                    && task.repository_id == workspace.repository_id
            })
            .expect("failed spawn should leave a recent precise generation task");
        assert_eq!(task.status, RuntimeTaskStatus::Failed);
        assert!(
            task.detail
                .as_deref()
                .is_some_and(|detail| detail.contains("failed to spawn precise generation thread")),
            "failed task should preserve the spawn error detail, got {:?}",
            task.detail
        );

        let lifecycle = server.workspace_precise_lifecycle_summary(
            &workspace,
            action,
            &server.workspace_precise_summary_for_workspace(&workspace, Some(action)),
            false,
            false,
        );
        assert_eq!(
            lifecycle
                .last_generation
                .as_ref()
                .map(|summary| summary.status),
            Some(crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded),
            "fixture should retain a stale successful generation to prove the fresh action wins"
        );
        assert_eq!(
            lifecycle.phase,
            crate::mcp::types::WorkspacePreciseLifecyclePhase::Failed,
            "spawn failure should surface as failed lifecycle instead of not_started"
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_detach_clears_pending_precise_dirty_paths_after_last_adoption() {
    let workspace_root = temp_workspace_root("precise-detach-clears-pending");
    let bin_dir = temp_workspace_root("precise-detach-clears-pending-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source fixture");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write Cargo fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("failed to write source fixture");

    let _rust_analyzer = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "rust-analyzer",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "rust-analyzer 1.85.0"
  exit 0
fi
printf '%s' "fake-scip-rust"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test tokio runtime should build");
        runtime.block_on(async {
            let server = FriggMcpServer::new(
                FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                    .expect("workspace root must produce valid config"),
            );
            let workspace = server
                .known_workspaces()
                .into_iter()
                .next()
                .expect("server should register workspace");
            server
                .adopt_workspace(&workspace, true)
                .expect("test session should adopt workspace");
            let _active_task = RuntimeTaskGuard::start(
                Arc::clone(&server.runtime_state.runtime_task_registry),
                RuntimeTaskKind::PreciseGenerate,
                workspace.repository_id.clone(),
                "precise_generation",
                Some("active task".to_owned()),
            );

            let action = server.maybe_spawn_workspace_precise_generation(
                &workspace,
                &[String::from("Cargo.toml")],
                &[String::from("old/dropped.rs")],
            );
            assert!(
                matches!(
                    action,
                    crate::mcp::types::WorkspacePreciseGenerationAction::SkippedActiveTask
                ),
                "expected pending paths to be recorded behind active generation, got {action:?}"
            );
            assert!(
                server
                    .take_pending_precise_dirty_paths(&workspace.repository_id)
                    .is_some(),
                "pending paths should be present before detach"
            );
            server.maybe_spawn_workspace_precise_generation(
                &workspace,
                &[String::from("Cargo.toml")],
                &[String::from("old/dropped.rs")],
            );

            server
                .workspace_detach(Parameters(WorkspaceDetachParams {
                    repository_id: Some(workspace.repository_id.clone()),
                }))
                .await
                .expect("workspace_detach should detach the last session");

            assert!(
                server
                    .take_pending_precise_dirty_paths(&workspace.repository_id)
                    .is_none(),
                "detaching the last session should clear abandoned precise replay paths"
            );
        });
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn precise_generation_handoff_does_not_strand_pending_paths_after_finish() {
    let workspace_root = temp_workspace_root("precise-handoff-no-strand");
    let bin_dir = temp_workspace_root("precise-handoff-no-strand-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source fixture");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write Cargo fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("failed to write source fixture");

    let _rust_analyzer = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "rust-analyzer",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "rust-analyzer 1.85.0"
  exit 0
fi
printf '%s' "fake-scip-rust"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let active_task_id = server
            .runtime_state
            .runtime_task_registry
            .write()
            .expect("runtime task registry should not be poisoned")
            .start_task(
                RuntimeTaskKind::PreciseGenerate,
                workspace.repository_id.clone(),
                "precise_generation",
                Some("active task".to_owned()),
            );

        let server_for_thread = server.clone();
        let workspace_for_thread = workspace.clone();
        let mut pending_guard = server
            .runtime_state
            .precise_generation_pending_dirty_paths
            .write()
            .expect("precise generation pending dirty paths should not be poisoned");
        let handle = std::thread::spawn(move || {
            server_for_thread.maybe_spawn_workspace_precise_generation(
                &workspace_for_thread,
                &[String::from("Cargo.toml")],
                &[String::from("old/dropped.rs")],
            )
        });

        server
            .runtime_state
            .runtime_task_registry
            .write()
            .expect("runtime task registry should not be poisoned")
            .finish_task(
                &active_task_id,
                RuntimeTaskStatus::Succeeded,
                Some("active task finished".to_owned()),
            );
        assert!(
            pending_guard.remove(&workspace.repository_id).is_none(),
            "finishing before the caller enters the handoff gate should find no pending paths"
        );
        drop(pending_guard);

        let action = handle
            .join()
            .expect("precise generation handoff caller should not panic");
        assert!(
            matches!(
                action,
                crate::mcp::types::WorkspacePreciseGenerationAction::Triggered
            ),
            "caller released after finish should start generation instead of stranding pending paths, got {action:?}"
        );
        assert!(
            server
                .take_pending_precise_dirty_paths(&workspace.repository_id)
                .is_none(),
            "no pending dirty paths should remain without an active generation owner"
        );

        let expected_artifact = workspace.root.join(".frigg/scip/rust.scip");
        for _ in 0..200 {
            if expected_artifact.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            expected_artifact.is_file(),
            "triggered precise generation should finish the fake artifact before cleanup"
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_php_precise_generation_prefers_repo_local_vendor_bin() {
    let workspace_root = temp_workspace_root("php-precise-generator-health");
    let bin_dir = temp_workspace_root("php-precise-generator-health-bin");
    fs::create_dir_all(workspace_root.join("vendor/bin"))
        .expect("failed to create vendor bin directory");
    fs::create_dir_all(workspace_root.join("vendor/davidrjenni/scip-php/src/Composer"))
        .expect("failed to create scip-php composer source directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("composer.json"),
        "{\n  \"name\": \"demo/demo\"\n}\n",
    )
    .expect("failed to write composer fixture");
    fs::write(workspace_root.join("composer.lock"), "{ }\n")
        .expect("failed to write composer lock fixture");
    fs::write(
        workspace_root.join("vendor/davidrjenni/scip-php/src/Composer/Composer.php"),
        r#"<?php
final class Composer
{
    public function __construct(string $projectRoot)
    {
        $scipPhpVendorDir = self::join(__DIR__, '..', '..', 'vendor');
        if (realpath($scipPhpVendorDir) === false) {
            throw new RuntimeException("Invalid scip-php vendor directory: {$scipPhpVendorDir}.");
        }
        $this->scipPhpVendorDir = realpath($scipPhpVendorDir);
    }
}
"#,
    )
    .expect("failed to write scip-php Composer fixture");

    let _path_scip_php = write_fake_precise_generator_script(
        &bin_dir,
        "scip-php",
        "scip-php 9.9.9",
        "wrong-scip-php",
    );
    let _local_scip_laravel = write_fake_precise_generator_script_with_body(
        &workspace_root.join("vendor/bin"),
        "scip-laravel",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "scip-laravel 1.0.0"
  exit 0
fi
printf '%s' "wrong-local-scip-laravel"
"#,
    );
    let _local_scip_php = write_fake_precise_generator_script_with_body(
        &workspace_root.join("vendor/bin"),
        "scip-php",
        r#"#!/bin/sh
if [ "${1:-}" = "--help" ] || [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "scip-php 1.0.0"
  exit 0
fi
composer_file="$(dirname "$0")/../davidrjenni/scip-php/src/Composer/Composer.php"
if ! grep -q "https://github.com/davidrjenni/scip-php/issues/235" "$composer_file"; then
  printf '%s\n' "missing FRIGG workaround comment" >&2
  exit 51
fi
if ! grep -q "projectRoot, 'vendor'" "$composer_file"; then
  printf '%s\n' "missing project vendor fallback" >&2
  exit 52
fi
printf '%s' "local-scip-php"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        assert_eq!(health.precise_generators.len(), 1);
        let php_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("php"))
            .expect("php generator should be reported");
        assert_eq!(php_generator.language.as_deref(), Some("php"));
        assert!(
            php_generator
                .tool
                .as_deref()
                .expect("php generator should report a resolved tool")
                .ends_with("vendor/bin/scip-php"),
            "repo-local vendor/bin/scip-php should be preferred in health reporting"
        );
        assert!(
            php_generator
                .repo_local_touch_risk
                .writes
                .iter()
                .any(|path| path.ends_with(".frigg/scip/php.scip")),
            "scorecard should report the repo-local PHP artifact write"
        );
        assert!(
            php_generator
                .repo_local_touch_risk
                .executes
                .iter()
                .any(|path| path.ends_with("vendor/bin/scip-php")),
            "scorecard should report the repo-local PHP generator execution"
        );
        assert!(
            php_generator
                .repo_local_touch_risk
                .may_patch
                .iter()
                .any(|path| path.ends_with(
                    "vendor/davidrjenni/scip-php/src/Composer/Composer.php"
                )),
            "scorecard should report the repo-local scip-php patch risk"
        );
        assert_eq!(
            php_generator
                .expected_output_path
                .as_deref()
                .expect("expected output path should be reported"),
            workspace
                .root
                .join(".frigg/scip/php.scip")
                .display()
                .to_string()
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("composer.lock")],
            &[],
        );

        let expected_artifact = workspace.root.join(".frigg/scip/php.scip");
        for _ in 0..200 {
            if expected_artifact.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            expected_artifact.is_file(),
            "repo-local vendor/bin/scip-php should have written a cached SCIP artifact"
        );
        assert_eq!(
            fs::read_to_string(&expected_artifact).expect("artifact should be readable"),
            "local-scip-php"
        );
        let patched_composer = fs::read_to_string(
            workspace_root.join("vendor/davidrjenni/scip-php/src/Composer/Composer.php"),
        )
        .expect("patched scip-php Composer source should be readable");
        assert!(
            patched_composer.contains("https://github.com/davidrjenni/scip-php/issues/235"),
            "patched Composer.php should include the upstream issue reference"
        );
        assert!(
            patched_composer.contains("self::join($projectRoot, 'vendor')"),
            "patched Composer.php should fall back to the project vendor directory"
        );

        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_php_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("php"))
            .expect("php generator should still be reported");
        assert_eq!(
            refreshed_php_generator.state,
            WorkspacePreciseGeneratorState::Available
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_php_precise_generation_prefers_laravel_vendor_bin() {
    let workspace_root = temp_workspace_root("laravel-precise-generator-health");
    let bin_dir = temp_workspace_root("laravel-precise-generator-health-bin");
    fs::create_dir_all(workspace_root.join("bootstrap"))
        .expect("failed to create bootstrap directory");
    fs::create_dir_all(workspace_root.join("vendor/bin"))
        .expect("failed to create vendor bin directory");
    fs::create_dir_all(workspace_root.join("vendor/davidrjenni/scip-php/src/Composer"))
        .expect("failed to create scip-php composer source directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("composer.json"),
        "{\n  \"name\": \"demo/demo\"\n}\n",
    )
    .expect("failed to write composer fixture");
    fs::write(workspace_root.join("composer.lock"), "{ }\n")
        .expect("failed to write composer lock fixture");
    fs::write(
        workspace_root.join("bootstrap/app.php"),
        "<?php\nreturn [];\n",
    )
    .expect("failed to write Laravel bootstrap fixture");
    fs::write(
        workspace_root.join("vendor/davidrjenni/scip-php/src/Composer/Composer.php"),
        r#"<?php
final class Composer
{
    public function __construct(string $projectRoot)
    {
        $scipPhpVendorDir = self::join(__DIR__, '..', '..', 'vendor');
        if (realpath($scipPhpVendorDir) === false) {
            throw new RuntimeException("Invalid scip-php vendor directory: {$scipPhpVendorDir}.");
        }
        $this->scipPhpVendorDir = realpath($scipPhpVendorDir);
    }
}
"#,
    )
    .expect("failed to write scip-php Composer fixture");

    let _path_scip_php = write_fake_precise_generator_script(
        &bin_dir,
        "scip-php",
        "scip-php 9.9.9",
        "wrong-scip-php",
    );
    let _local_scip_laravel = write_fake_precise_generator_script_with_body(
        &workspace_root.join("vendor/bin"),
        "scip-laravel",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "scip-laravel 1.0.0"
  exit 0
fi
printf '%s' "local-scip-laravel"
"#,
    );
    let _local_scip_php = write_fake_precise_generator_script_with_body(
        &workspace_root.join("vendor/bin"),
        "scip-php",
        r#"#!/bin/sh
if [ "${1:-}" = "--help" ] || [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "scip-php 1.0.0"
  exit 0
fi
composer_file="$(dirname "$0")/../davidrjenni/scip-php/src/Composer/Composer.php"
if ! grep -q "https://github.com/davidrjenni/scip-php/issues/235" "$composer_file"; then
  printf '%s\n' "missing FRIGG workaround comment" >&2
  exit 51
fi
if ! grep -q "projectRoot, 'vendor'" "$composer_file"; then
  printf '%s\n' "missing project vendor fallback" >&2
  exit 52
fi
printf '%s' "wrong-local-scip-php"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        assert_eq!(health.precise_generators.len(), 1);
        let php_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("php"))
            .expect("php generator should be reported");
        assert!(
            php_generator
                .tool
                .as_deref()
                .expect("php generator should report a resolved tool")
                .ends_with("vendor/bin/scip-laravel"),
            "Laravel workspaces should prefer repo-local vendor/bin/scip-laravel"
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("composer.lock")],
            &[],
        );

        let expected_artifact = workspace.root.join(".frigg/scip/php.scip");
        for _ in 0..200 {
            if expected_artifact.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            expected_artifact.is_file(),
            "repo-local vendor/bin/scip-laravel should have written a cached SCIP artifact"
        );
        assert_eq!(
            fs::read_to_string(&expected_artifact).expect("artifact should be readable"),
            "local-scip-laravel"
        );

        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_php_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("php"))
            .expect("php generator should still be reported");
        assert_eq!(
            refreshed_php_generator.state,
            WorkspacePreciseGeneratorState::Available
        );
        assert!(
            refreshed_php_generator
                .last_generation
                .as_ref()
                .expect("php generation should be cached")
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("scip-laravel"),
            "cached generation summary should reflect the Laravel tool"
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_go_precise_generation_uses_explicit_output_and_local_caches() {
    let workspace_root = temp_workspace_root("go-precise-generator-health");
    let bin_dir = temp_workspace_root("go-precise-generator-health-bin");
    fs::create_dir_all(&workspace_root).expect("failed to create go workspace root");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("go.mod"),
        "module example.com/demo\n\ngo 1.24.0\n",
    )
    .expect("failed to write go.mod fixture");
    fs::create_dir_all(workspace_root.join("cmd/demo"))
        .expect("failed to create go source fixture directory");
    fs::write(
        workspace_root.join("cmd/demo/main.go"),
        "package main\nfunc main() {}\n",
    )
    .expect("failed to write go source fixture");
    let stale_artifact = workspace_root.join(".frigg/scip/go.scip");
    fs::create_dir_all(
        stale_artifact
            .parent()
            .expect("artifact path should have a parent"),
    )
    .expect("failed to prepare stale artifact directory");
    fs::write(&stale_artifact, "stale-go-log").expect("failed to seed stale artifact");

    let _scip_go = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "scip-go",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "scip-go 0.1.26"
  exit 0
fi
if [ "${1:-}" != "-q" ] || [ "${2:-}" != "-o" ] || [ -z "${3:-}" ]; then
  printf '%s\n' "missing quiet/output args" >&2
  exit 41
fi
if [ -z "${GOCACHE:-}" ] || [ -z "${GOMODCACHE:-}" ] || [ -z "${GOPATH:-}" ]; then
  printf '%s\n' "missing go cache env" >&2
  exit 42
fi
printf '%s\n' "Resolving module name"
printf '%s' "binary-go-scip" > "$3"
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("go.mod")],
            &[],
        );

        for _ in 0..200 {
            let ready = fs::read(&stale_artifact)
                .map(|contents| contents == b"binary-go-scip")
                .unwrap_or(false);
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert_eq!(
            fs::read(&stale_artifact).expect("artifact should be readable"),
            b"binary-go-scip"
        );

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        let go_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("go"))
            .expect("go generator should be reported");
        let expected_tool = bin_dir.join("scip-go").display().to_string();
        assert_eq!(go_generator.tool.as_deref(), Some(expected_tool.as_str()));
        let generation = go_generator
            .last_generation
            .as_ref()
            .expect("go generation should be cached");
        assert_eq!(
            generation.status,
            crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded
        );
        assert_eq!(
            generation
                .artifact_path
                .as_deref()
                .expect("artifact path should be recorded"),
            fs::canonicalize(&stale_artifact)
                .expect("artifact path should canonicalize")
                .display()
                .to_string()
        );
    });
    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_python_precise_generation_prefers_repo_local_node_bin() {
    let workspace_root = temp_workspace_root("python-precise-generator-health");
    let bin_dir = temp_workspace_root("python-precise-generator-health-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create python src fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");
    let stale_artifact = workspace_root.join(".frigg/scip/python.scip");
    fs::create_dir_all(
        stale_artifact
            .parent()
            .expect("artifact path should have a parent"),
    )
    .expect("failed to prepare stale python artifact directory");
    fs::write(&stale_artifact, "stale-python-scip").expect("failed to seed stale python artifact");

    let _global_scip_python = write_fake_precise_generator_script(
        &bin_dir,
        "scip-python",
        "scip-python 9.9.9",
        "wrong-python-scip",
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let expected_project_name = FriggMcpServer::derived_python_precise_project_name(&workspace);
        let _local_scip_python = write_fake_precise_generator_script_with_body(
            &workspace_root.join("node_modules/.bin"),
            "scip-python",
            &format!(
                r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.0"
  exit 0
fi
if [ "${{1:-}}" = "index" ] && [ "${{2:-}}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
if [ "${{1:-}}" != "index" ] || [ "${{2:-}}" != "--quiet" ] || [ "${{3:-}}" != "--project-name" ] || [ "${{4:-}}" != "{expected_project_name}" ] || [ "${{5:-}}" != "--output" ] || [ -z "${{6:-}}" ] || [ -n "${{7:-}}" ]; then
  printf '%s\n' "unexpected python args: $*" >&2
  exit 61
fi
printf '%s' "local-python-scip" > "${{6}}"
"#
            ),
        );

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        let python_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("python"))
            .expect("python generator should be reported");
        assert!(
            python_generator
                .tool
                .as_deref()
                .expect("python generator should report a resolved tool")
                .ends_with("node_modules/.bin/scip-python"),
            "repo-local node_modules/.bin/scip-python should be preferred"
        );
        assert!(
            python_generator
                .repo_local_touch_risk
                .writes
                .iter()
                .any(|path| path.ends_with(".frigg/scip/python.scip")),
            "scorecard should report the repo-local precise artifact write"
        );
        assert!(
            python_generator
                .repo_local_touch_risk
                .executes
                .iter()
                .any(|path| path.ends_with("node_modules/.bin/scip-python")),
            "scorecard should report the repo-local generator execution"
        );
        assert!(
            python_generator.repo_local_touch_risk.may_patch.is_empty(),
            "python generation should not report a repo-local patch risk"
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("pyproject.toml")],
            &[],
        );

        for _ in 0..200 {
            let ready = fs::read(&stale_artifact)
                .map(|contents| contents == b"local-python-scip")
                .unwrap_or(false);
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert_eq!(
            fs::read(&stale_artifact).expect("python artifact should be readable"),
            b"local-python-scip"
        );

        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_python_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("python"))
            .expect("python generator should still be reported");
        let generation = refreshed_python_generator
            .last_generation
            .as_ref()
            .expect("python generation should be cached");
        assert_eq!(
            generation.status,
            crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded
        );
        let expected_artifact_path = fs::canonicalize(&stale_artifact)
            .expect("python artifact path should canonicalize")
            .display()
            .to_string();
        assert_eq!(
            generation.artifact_path.as_deref(),
            Some(expected_artifact_path.as_str())
        );
        assert!(generation.duration_ms.is_some());
        assert_eq!(generation.artifact_count, Some(1));
        assert_eq!(
            generation.artifact_bytes,
            Some("local-python-scip".len() as u64)
        );
        assert!(generation.artifact_sample_paths.is_empty());
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_python_precise_generation_keeps_monolith_on_large_output() {
    let workspace_root = temp_workspace_root("python-precise-generator-monolith");
    let bin_dir = temp_workspace_root("python-precise-generator-monolith-bin");
    fs::create_dir_all(workspace_root.join("src/pkg_a"))
        .expect("failed to create python pkg_a fixture");
    fs::create_dir_all(workspace_root.join("src/pkg_b"))
        .expect("failed to create python pkg_b fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/pkg_a/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");
    fs::write(
        workspace_root.join("src/pkg_b/app.py"),
        "def beta():\n    return 2\n",
    )
    .expect("failed to write python source fixture");
    let stale_artifact = workspace_root.join(".frigg/scip/python.scip");
    fs::create_dir_all(
        stale_artifact
            .parent()
            .expect("artifact path should have a parent"),
    )
    .expect("failed to prepare stale python artifact directory");
    fs::write(&stale_artifact, "stale-python-scip").expect("failed to seed stale python artifact");

    let _global_scip_python = write_fake_precise_generator_script(
        &bin_dir,
        "scip-python",
        "scip-python 9.9.9",
        "wrong-python-scip",
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config");
        config.max_file_bytes = 1;
        let server = FriggMcpServer::new(config);
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let expected_project_name = FriggMcpServer::derived_python_precise_project_name(&workspace);
        let _local_scip_python = write_fake_precise_generator_script_with_body(
            &workspace_root.join("node_modules/.bin"),
            "scip-python",
            &format!(
                r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.6"
  exit 0
fi
if [ "${{1:-}}" = "index" ] && [ "${{2:-}}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
if [ "${{1:-}}" != "index" ] || [ "${{2:-}}" != "--quiet" ] || [ "${{3:-}}" != "--project-name" ] || [ "${{4:-}}" != "{expected_project_name}" ] || [ "${{5:-}}" != "--output" ] || [ -z "${{6:-}}" ] || [ -n "${{7:-}}" ]; then
  printf '%s\n' "unexpected python args: $*" >&2
  exit 71
fi
printf '%s' "0123456789" > "${{6}}"
"#
            ),
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("pyproject.toml")],
            &[],
        );

        let scip_dir = workspace.root.join(".frigg/scip");
        for _ in 0..200 {
            let ready = fs::read(&stale_artifact)
                .map(|contents| contents == b"0123456789")
                .unwrap_or(false);
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            stale_artifact.is_file(),
            "python precise generation should keep publishing a monolithic python.scip artifact"
        );
        assert_eq!(
            fs::read(&stale_artifact).expect("python artifact should be readable"),
            b"0123456789"
        );
        assert!(
            fs::read_dir(&scip_dir)
                .expect("scip directory should exist")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .all(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_none_or(|name| !name.starts_with("python--"))
                }),
            "python precise generation should not publish shard artifacts"
        );

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_python_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("python"))
            .expect("python generator should still be reported");
        let generation = refreshed_python_generator
            .last_generation
            .as_ref()
            .expect("python generation should be cached");
        assert_eq!(
            generation.status,
            crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded
        );
        let expected_artifact_path = fs::canonicalize(&stale_artifact)
            .expect("python artifact path should canonicalize")
            .display()
            .to_string();
        assert_eq!(
            generation.artifact_path.as_deref(),
            Some(expected_artifact_path.as_str())
        );
        assert!(generation.duration_ms.is_some());
        assert_eq!(generation.artifact_count, Some(1));
        assert_eq!(generation.artifact_bytes, Some(10));
        assert!(
            generation.artifact_sample_paths.is_empty(),
            "monolithic python generation should not report shard samples"
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_python_precise_generation_cleans_up_stale_shards() {
    let workspace_root = temp_workspace_root("python-precise-generator-cleans-stale-shards");
    let bin_dir = temp_workspace_root("python-precise-generator-cleans-stale-shards-bin");
    fs::create_dir_all(workspace_root.join("src/pkg_a"))
        .expect("failed to create python pkg_a fixture");
    fs::create_dir_all(workspace_root.join("src/pkg_b"))
        .expect("failed to create python pkg_b fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/pkg_a/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");
    fs::write(
        workspace_root.join("src/pkg_b/app.py"),
        "def beta():\n    return 2\n",
    )
    .expect("failed to write python source fixture");
    let scip_dir = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&scip_dir).expect("failed to prepare stale python artifact directory");
    let stale_shard_a = scip_dir.join("python--seed-a.scip");
    let stale_shard_b = scip_dir.join("python--seed-b.scip");
    fs::write(&stale_shard_a, "stale-a").expect("failed to seed stale python shard");
    fs::write(&stale_shard_b, "stale-b").expect("failed to seed stale python shard");

    let _global_scip_python = write_fake_precise_generator_script(
        &bin_dir,
        "scip-python",
        "scip-python 9.9.9",
        "wrong-python-scip",
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config");
        config.max_file_bytes = 20;
        let server = FriggMcpServer::new(config);
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let expected_project_name = FriggMcpServer::derived_python_precise_project_name(&workspace);
        let _local_scip_python = write_fake_precise_generator_script_with_body(
            &workspace_root.join("node_modules/.bin"),
            "scip-python",
            &format!(
                r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.6"
  exit 0
fi
if [ "${{1:-}}" = "index" ] && [ "${{2:-}}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
if [ "${{1:-}}" != "index" ] || [ "${{2:-}}" != "--quiet" ] || [ "${{3:-}}" != "--project-name" ] || [ "${{4:-}}" != "{expected_project_name}" ] || [ "${{5:-}}" != "--output" ] || [ -z "${{6:-}}" ] || [ -n "${{7:-}}" ]; then
  printf '%s\n' "unexpected python args: $*" >&2
  exit 81
fi
printf '%s' "root" > "${{6}}"
"#
            ),
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("pyproject.toml")],
            &[],
        );

        for _ in 0..200 {
            if workspace.root.join(".frigg/scip/python.scip").is_file()
                && !stale_shard_a.exists()
                && !stale_shard_b.exists()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            workspace.root.join(".frigg/scip/python.scip").is_file(),
            "python regeneration should publish a monolithic python.scip artifact"
        );
        assert!(
            !stale_shard_a.exists() && !stale_shard_b.exists(),
            "stale shard artifacts should be removed during regeneration"
        );

        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_python_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("python"))
            .expect("python generator should still be reported");
        let generation = refreshed_python_generator
            .last_generation
            .as_ref()
            .expect("python generation should be cached");
        assert_eq!(
            generation.status,
            crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded
        );
        assert!(generation.duration_ms.is_some());
        assert_eq!(generation.artifact_count, Some(1));
        assert_eq!(generation.artifact_bytes, Some("root".len() as u64));
        assert_eq!(generation.artifact_sample_paths.len(), 0);
        let expected_artifact_path =
            fs::canonicalize(workspace.root.join(".frigg/scip/python.scip"))
                .expect("python artifact path should canonicalize")
                .display()
                .to_string();
        assert!(
            generation.artifact_path.as_deref() == Some(expected_artifact_path.as_str()),
            "python regeneration should report the monolithic artifact path"
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_python_precise_generation_missing_tool_for_source_only_workspace()
{
    let workspace_root = temp_workspace_root("python-precise-generator-missing-tool");
    let bin_dir = temp_workspace_root("python-precise-generator-missing-tool-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create python src fixture");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("src/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        let python_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("python"))
            .expect("python source fallback should report a generator entry");
        assert_eq!(
            python_generator.state,
            WorkspacePreciseGeneratorState::MissingTool
        );
        let expected_output_path = workspace.root.join(".frigg/scip/python.scip");
        let expected_output_path = expected_output_path.display().to_string();
        assert_eq!(
            python_generator.expected_output_path.as_deref(),
            Some(expected_output_path.as_str())
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_ignores_python_generator_for_nested_target_output_only() {
    let workspace_root = temp_workspace_root("python-precise-generator-nested-target-only");
    fs::create_dir_all(workspace_root.join("src/main/java"))
        .expect("failed to create java source fixture");
    fs::create_dir_all(workspace_root.join("codec-native-quic/target/boringssl-source/util"))
        .expect("failed to create nested target fixture");
    fs::write(
        workspace_root.join("pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion></project>\n",
    )
    .expect("failed to write maven fixture");
    fs::write(
        workspace_root.join("src/main/java/App.java"),
        "final class App {}\n",
    )
    .expect("failed to write java source fixture");
    fs::write(
        workspace_root
            .join("codec-native-quic/target/boringssl-source/util/generate_build_files.py"),
        "print('generated')\n",
    )
    .expect("failed to write nested target python fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    let storage = FriggMcpServer::workspace_storage_summary(&workspace);
    let health = server.workspace_index_health_summary(&workspace, &storage);
    assert!(
        !health
            .precise_generators
            .iter()
            .any(|generator| generator.language.as_deref() == Some("python")),
        "nested target output should not make a non-Python workspace report scip-python"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_index_health_reports_python_precise_generation_classifies_env_failures() {
    let workspace_root = temp_workspace_root("python-precise-generator-env-failure");
    let bin_dir = temp_workspace_root("python-precise-generator-env-failure-bin");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create python src fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");

    let _global_scip_python = write_fake_precise_generator_script(
        &bin_dir,
        "scip-python",
        "scip-python 9.9.9",
        "wrong-python-scip",
    );

    let _local_scip_python = write_fake_precise_generator_script_with_body(
        &workspace_root.join("node_modules/.bin"),
        "scip-python",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.0"
  exit 0
fi
if [ "${1:-}" = "index" ] && [ "${2:-}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
printf '%s\n' "No module named pip" >&2
exit 63
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let storage = FriggMcpServer::workspace_storage_summary(&workspace);

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("src/app.py")],
            &[],
        );

        let generation = (0..200).find_map(|_| {
            let health = server.workspace_index_health_summary(&workspace, &storage);
            let python_generator = health
                .precise_generators
                .iter()
                .find(|generator| generator.language.as_deref() == Some("python"))?;
            let generation = python_generator.last_generation.clone()?;
            (generation.status == crate::mcp::types::WorkspacePreciseGenerationStatus::Failed)
                .then_some(generation)
                .or_else(|| {
                    std::thread::sleep(Duration::from_millis(50));
                    None
                })
        });

        let generation = generation.expect("python failure summary should be cached");
        assert_eq!(
            generation.failure_class,
            Some(crate::mcp::types::WorkspacePreciseFailureClass::ToolEnvFailure)
        );
        assert_eq!(
            generation.recommended_action,
            Some(crate::mcp::types::WorkspaceRecommendedAction::CheckEnvironment)
        );
        assert!(generation.duration_ms.is_some());
        assert_eq!(generation.artifact_bytes, None);
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_reports_kotlin_precise_generation_for_gradle_workspaces() {
    let workspace_root = temp_workspace_root("kotlin-precise-generator-health");
    let bin_dir = temp_workspace_root("kotlin-precise-generator-health-bin");
    fs::create_dir_all(workspace_root.join("src/main/kotlin"))
        .expect("failed to create kotlin source fixture directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .expect("failed to write settings.gradle.kts fixture");
    fs::write(
        workspace_root.join("build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.2.20\" }\n",
    )
    .expect("failed to write build.gradle.kts fixture");
    fs::write(workspace_root.join("src/main/kotlin/App.kt"), "class App\n")
        .expect("failed to write kotlin source fixture");
    let stale_artifact = workspace_root.join(".frigg/scip/kotlin.scip");
    fs::create_dir_all(
        stale_artifact
            .parent()
            .expect("artifact path should have a parent"),
    )
    .expect("failed to prepare stale kotlin artifact directory");
    fs::write(&stale_artifact, "stale-kotlin-scip").expect("failed to seed stale kotlin artifact");

    let _scip_java = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "scip-java",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ] || [ "${1:-}" = "--help" ]; then
  printf '%s\n' "scip-java 0.11.2"
  exit 0
fi
if [ "${1:-}" != "index" ]; then
  printf '%s\n' "unexpected kotlin args: $*" >&2
  exit 71
fi
printf '%s' "kotlin-scip" > index.scip
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let storage = FriggMcpServer::workspace_storage_summary(&workspace);
        let health = server.workspace_index_health_summary(&workspace, &storage);
        let kotlin_generator = health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("kotlin"))
            .expect("kotlin generator should be reported");
        let expected_output_path = workspace.root.join(".frigg/scip/kotlin.scip");
        let expected_output_path = expected_output_path.display().to_string();
        assert_eq!(
            kotlin_generator.expected_output_path.as_deref(),
            Some(expected_output_path.as_str())
        );

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("build.gradle.kts")],
            &[],
        );

        for _ in 0..200 {
            let ready = fs::read(&stale_artifact)
                .map(|contents| contents == b"kotlin-scip")
                .unwrap_or(false);
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert_eq!(
            fs::read(&stale_artifact).expect("kotlin artifact should be readable"),
            b"kotlin-scip"
        );

        let refreshed_health = server.workspace_index_health_summary(&workspace, &storage);
        let refreshed_kotlin_generator = refreshed_health
            .precise_generators
            .iter()
            .find(|generator| generator.language.as_deref() == Some("kotlin"))
            .expect("kotlin generator should still be reported");
        let generation = refreshed_kotlin_generator
            .last_generation
            .as_ref()
            .expect("kotlin generation should be cached");
        assert_eq!(
            generation.status,
            crate::mcp::types::WorkspacePreciseGenerationStatus::Succeeded
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn workspace_index_health_omits_kotlin_generator_without_gradle_source_pairing() {
    let workspace_root = temp_workspace_root("kotlin-precise-generator-not-applicable");
    fs::create_dir_all(&workspace_root).expect("failed to create kotlin workspace root");
    fs::write(
        workspace_root.join("build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.2.20\" }\n",
    )
    .expect("failed to write build.gradle.kts fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    let storage = FriggMcpServer::workspace_storage_summary(&workspace);
    let health = server.workspace_index_health_summary(&workspace, &storage);
    assert!(
        health
            .precise_generators
            .iter()
            .all(|generator| generator.language.as_deref() != Some("kotlin")),
        "kotlin precise generation should stay unavailable without Kotlin source files"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_index_health_reports_kotlin_precise_generation_classifies_env_failures() {
    let workspace_root = temp_workspace_root("kotlin-precise-generator-env-failure");
    let bin_dir = temp_workspace_root("kotlin-precise-generator-env-failure-bin");
    fs::create_dir_all(workspace_root.join("src/main/kotlin"))
        .expect("failed to create kotlin source fixture directory");
    fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
    fs::write(
        workspace_root.join("settings.gradle.kts"),
        "rootProject.name = \"demo\"\n",
    )
    .expect("failed to write settings.gradle.kts fixture");
    fs::write(
        workspace_root.join("build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.2.20\" }\n",
    )
    .expect("failed to write build.gradle.kts fixture");
    fs::write(workspace_root.join("src/main/kotlin/App.kt"), "class App\n")
        .expect("failed to write kotlin source fixture");

    let _scip_java = write_fake_precise_generator_script_with_body(
        &bin_dir,
        "scip-java",
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ] || [ "${1:-}" = "version" ] || [ "${1:-}" = "--help" ]; then
  printf '%s\n' "scip-java 0.11.2"
  exit 0
fi
printf '%s\n' "Unable to locate a Java Runtime." >&2
exit 73
"#,
    );

    with_fake_precise_generator_path(&bin_dir, || {
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        let storage = FriggMcpServer::workspace_storage_summary(&workspace);

        server.maybe_spawn_workspace_precise_generation_for_paths(
            &workspace,
            &[String::from("src/main/kotlin/App.kt")],
            &[],
        );

        let generation = (0..200).find_map(|_| {
            let health = server.workspace_index_health_summary(&workspace, &storage);
            let kotlin_generator = health
                .precise_generators
                .iter()
                .find(|generator| generator.language.as_deref() == Some("kotlin"))?;
            let generation = kotlin_generator.last_generation.clone()?;
            (generation.status == crate::mcp::types::WorkspacePreciseGenerationStatus::Failed)
                .then_some(generation)
                .or_else(|| {
                    std::thread::sleep(Duration::from_millis(50));
                    None
                })
        });

        let generation = generation.expect("kotlin failure summary should be cached");
        assert_eq!(
            generation.failure_class,
            Some(crate::mcp::types::WorkspacePreciseFailureClass::ToolEnvFailure)
        );
        assert_eq!(
            generation.recommended_action,
            Some(crate::mcp::types::WorkspaceRecommendedAction::CheckEnvironment)
        );
    });

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_dir_all(bin_dir);
}

#[test]
fn precise_artifact_discovery_is_scoped_to_runtime_scip_directory() {
    let workspace_root = PathBuf::from("/tmp/frigg-runtime-scip-scope");
    let directories = FriggMcpServer::scip_candidate_directories(&workspace_root);

    assert_eq!(directories, [workspace_root.join(".frigg/scip")]);
}

#[test]
fn precise_artifact_discovery_includes_json_and_scip_files() {
    let workspace_root = temp_workspace_root("scip-discovery-extensions");
    let scip_root = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&scip_root).expect("failed to create scip fixture directory");
    fs::write(scip_root.join("a.json"), "{}").expect("failed to write json fixture");
    fs::write(scip_root.join("b.scip"), [0_u8, 1_u8, 2_u8])
        .expect("failed to write protobuf fixture");
    fs::write(scip_root.join("ignored.txt"), "x").expect("failed to write ignored fixture");

    let discovery = FriggMcpServer::collect_scip_artifact_digests(&workspace_root);
    assert_eq!(discovery.artifact_digests.len(), 2);
    assert_eq!(
        discovery
            .artifact_digests
            .iter()
            .map(|digest| digest.path.file_name().and_then(|name| name.to_str()))
            .collect::<Vec<_>>(),
        vec![Some("a.json"), Some("b.scip")]
    );
    assert_eq!(
        discovery
            .artifact_digests
            .iter()
            .map(|digest| digest.format.as_str())
            .collect::<Vec<_>>(),
        vec!["json", "protobuf"]
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[cfg(unix)]
#[test]
fn precise_artifact_discovery_rejects_symlink_escape() {
    let workspace_root = temp_workspace_root("scip-discovery-symlink-escape");
    let scip_root = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&scip_root).expect("failed to create scip fixture directory");
    let outside_path = workspace_root.with_extension("outside.scip");
    fs::write(&outside_path, [0_u8, 1_u8, 2_u8]).expect("failed to write outside fixture");
    std::os::unix::fs::symlink(&outside_path, scip_root.join("leak.scip"))
        .expect("failed to create symlinked scip fixture");

    let discovery = FriggMcpServer::collect_scip_artifact_digests(&workspace_root);
    assert!(
        discovery.artifact_digests.is_empty(),
        "escaping symlinked SCIP artifacts must not be discovered for ingest"
    );

    let _ = fs::remove_dir_all(workspace_root);
    let _ = fs::remove_file(outside_path);
}

#[test]
fn manifest_source_paths_filter_to_symbol_corpus_capability() {
    let digests = vec![
        FileMetadataDigest {
            path: PathBuf::from("src/lib.rs"),
            size_bytes: 10,
            mtime_ns: Some(1),
        },
        FileMetadataDigest {
            path: PathBuf::from("src/server.php"),
            size_bytes: 20,
            mtime_ns: Some(2),
        },
        FileMetadataDigest {
            path: PathBuf::from("src/app.ts"),
            size_bytes: 30,
            mtime_ns: Some(3),
        },
        FileMetadataDigest {
            path: PathBuf::from("README.md"),
            size_bytes: 40,
            mtime_ns: Some(4),
        },
    ];

    let source_paths = FriggMcpServer::manifest_source_paths_for_digests(&digests);

    assert_eq!(
        source_paths,
        vec![
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/server.php"),
            PathBuf::from("src/app.ts")
        ]
    );
}
