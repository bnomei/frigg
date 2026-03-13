use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::types::{
    DeepSearchPlaybookContract, DeepSearchPlaybookStepContract, DeepSearchReplayParams,
    DeepSearchRunParams, DeepSearchTraceArtifactContract,
};
use frigg::mcp::{DeepSearchHarness, DeepSearchTraceOutcome, FriggMcpServer};
use frigg::settings::FriggConfig;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use serde_json::json;

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-mcp-deep-search-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn prepare_workspace(root: &Path) {
    let src = root.join("src");
    fs::create_dir_all(&src).expect("failed to create deep-search replay workspace fixture");
    fs::write(
        src.join("lib.rs"),
        "pub fn greeting() -> &'static str { \"hello replay\" }\n\
         pub fn callsite() { let _ = greeting(); }\n",
    )
    .expect("failed to seed replay workspace source fixture");
}

fn build_server(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root fixture must produce valid config");
    FriggMcpServer::new(config)
}

fn build_runtime_server(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root fixture must produce valid config");
    FriggMcpServer::new_with_runtime_options(config, false, true)
}

fn error_code_tag(error: &rmcp::ErrorData) -> Option<&str> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
}

fn retryable_tag(error: &rmcp::ErrorData) -> Option<bool> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("retryable"))
        .and_then(|value| value.as_bool())
}

fn playbook_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/playbooks/deep-search-replay-basic.playbook.json")
}

fn partial_channel_playbook_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/playbooks/deep-search-replay-partial-channel.playbook.json")
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn deep_search_replay_run_persists_trace_artifact() {
    let workspace_root = temp_workspace_root("persist-artifact");
    prepare_workspace(&workspace_root);
    let harness = DeepSearchHarness::new(build_server(&workspace_root));

    let playbook = DeepSearchHarness::load_playbook(&playbook_path())
        .expect("expected deep-search playbook fixture to parse");
    let artifact = harness
        .run_playbook(&playbook)
        .await
        .expect("playbook run should succeed");
    let artifact_path = workspace_root.join(".frigg").join("deep-search-trace.json");
    DeepSearchHarness::persist_trace_artifact(&artifact_path, &artifact)
        .expect("expected trace artifact persistence to succeed");
    let persisted = DeepSearchHarness::load_trace_artifact(&artifact_path)
        .expect("expected persisted trace artifact to load");

    assert_eq!(artifact, persisted);
    assert_eq!(artifact.step_count, playbook.steps.len());
    assert!(
        artifact
            .steps
            .iter()
            .all(|step| matches!(step.outcome, DeepSearchTraceOutcome::Ok { .. })),
        "expected all playbook steps to succeed in baseline replay fixture"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_matches_persisted_trace_on_identical_replay() {
    let workspace_root = temp_workspace_root("identical-replay");
    prepare_workspace(&workspace_root);
    let harness = DeepSearchHarness::new(build_server(&workspace_root));

    let playbook = DeepSearchHarness::load_playbook(&playbook_path())
        .expect("expected deep-search playbook fixture to parse");
    let artifact = harness
        .run_playbook(&playbook)
        .await
        .expect("initial deep-search run should succeed");
    let artifact_path = workspace_root.join(".frigg").join("deep-search-trace.json");
    DeepSearchHarness::persist_trace_artifact(&artifact_path, &artifact)
        .expect("expected trace artifact persistence to succeed");

    let replay = harness
        .replay_from_artifact_path(&playbook, &artifact_path)
        .await
        .expect("replay check should execute");
    assert!(
        replay.matches,
        "expected replay to match persisted artifact"
    );
    assert!(
        replay.diff.is_none(),
        "expected no diff for deterministic replay: {:?}",
        replay.diff
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_detects_diff_after_repository_change() {
    let workspace_root = temp_workspace_root("diff-detection");
    prepare_workspace(&workspace_root);
    let harness = DeepSearchHarness::new(build_server(&workspace_root));

    let playbook = DeepSearchHarness::load_playbook(&playbook_path())
        .expect("expected deep-search playbook fixture to parse");
    let artifact = harness
        .run_playbook(&playbook)
        .await
        .expect("initial deep-search run should succeed");
    let artifact_path = workspace_root.join(".frigg").join("deep-search-trace.json");
    DeepSearchHarness::persist_trace_artifact(&artifact_path, &artifact)
        .expect("expected trace artifact persistence to succeed");

    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn salute() -> &'static str { \"hello changed\" }\n\
         pub fn callsite() { let _ = salute(); }\n",
    )
    .expect("failed to mutate replay workspace");

    let replay = harness
        .replay_from_artifact_path(&playbook, &artifact_path)
        .await
        .expect("replay check should execute");
    assert!(
        !replay.matches,
        "expected replay mismatch after repository content change"
    );
    let diff = replay
        .diff
        .expect("expected diff details after repository change");
    assert!(
        diff.contains("step["),
        "expected step-level diff diagnostics, got: {diff}"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_runtime_handlers_execute_deterministically() {
    let workspace_root = temp_workspace_root("runtime-handlers");
    prepare_workspace(&workspace_root);
    let server = build_runtime_server(&workspace_root);

    let playbook = DeepSearchHarness::load_playbook(&playbook_path())
        .expect("expected deep-search playbook fixture to parse");
    let run = server
        .deep_search_run(Parameters(DeepSearchRunParams {
            playbook: playbook.clone().into(),
        }))
        .await
        .expect("deep_search_run runtime handler should succeed")
        .0;

    assert_eq!(run.trace_artifact.step_count, playbook.steps.len());

    let replay = server
        .deep_search_replay(Parameters(DeepSearchReplayParams {
            playbook: playbook.into(),
            expected_trace_artifact: run.trace_artifact.clone(),
        }))
        .await
        .expect("deep_search_replay runtime handler should succeed")
        .0;
    assert!(replay.matches, "expected deterministic replay match");
    assert!(
        replay.diff.is_none(),
        "expected no replay diff for identical runtime input"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_runtime_handler_rejects_unsupported_step_tool_with_invalid_params() {
    let workspace_root = temp_workspace_root("runtime-unsupported-step");
    prepare_workspace(&workspace_root);
    let server = build_runtime_server(&workspace_root);

    let playbook = DeepSearchPlaybookContract {
        playbook_id: "unsupported-step-tool".to_owned(),
        steps: vec![DeepSearchPlaybookStepContract {
            step_id: "tool-001".to_owned(),
            tool_name: "write_file".to_owned(),
            params: json!({ "path": "src/lib.rs" }),
        }],
    };
    let error = match server
        .deep_search_run(Parameters(DeepSearchRunParams { playbook }))
        .await
    {
        Ok(_) => panic!("unsupported step tool should fail with typed invalid_params"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("unsupported tool"),
        "expected unsupported tool diagnostics, got: {}",
        error.message
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_runtime_handler_rejects_unsupported_step_tool_during_replay() {
    let workspace_root = temp_workspace_root("runtime-replay-unsupported-step");
    prepare_workspace(&workspace_root);
    let server = build_runtime_server(&workspace_root);

    let playbook = DeepSearchPlaybookContract {
        playbook_id: "unsupported-step-tool-replay".to_owned(),
        steps: vec![DeepSearchPlaybookStepContract {
            step_id: "tool-001".to_owned(),
            tool_name: "write_file".to_owned(),
            params: json!({ "path": "src/lib.rs" }),
        }],
    };
    let error = match server
        .deep_search_replay(Parameters(DeepSearchReplayParams {
            playbook,
            expected_trace_artifact: DeepSearchTraceArtifactContract {
                trace_schema: "frigg.deep_search.trace.v1".to_owned(),
                playbook_id: "unsupported-step-tool-replay".to_owned(),
                step_count: 0,
                steps: Vec::new(),
            },
        }))
        .await
    {
        Ok(_) => panic!("unsupported replay step tool should fail with typed invalid_params"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("unsupported tool"),
        "expected unsupported tool diagnostics, got: {}",
        error.message
    );
    assert!(
        error.message.contains("allowed tools"),
        "expected replay rejection to surface the stable-core allowlist, got: {}",
        error.message
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_runtime_handler_rejects_invalid_step_params_with_invalid_params() {
    let workspace_root = temp_workspace_root("runtime-invalid-params");
    prepare_workspace(&workspace_root);
    let server = build_runtime_server(&workspace_root);

    let playbook = DeepSearchPlaybookContract {
        playbook_id: "invalid-step-params".to_owned(),
        steps: vec![DeepSearchPlaybookStepContract {
            step_id: "tool-001".to_owned(),
            tool_name: "search_text".to_owned(),
            params: json!({
                "query": "",
                "repository_id": "repo-001",
            }),
        }],
    };
    let error = match server
        .deep_search_run(Parameters(DeepSearchRunParams { playbook }))
        .await
    {
        Ok(_) => panic!("invalid playbook step params should fail with typed invalid_params"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("invalid_params"),
        "expected invalid_params diagnostics, got: {}",
        error.message
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_partial_channel_playbook_is_deterministic() {
    let workspace_root = temp_workspace_root("partial-channel-deterministic");
    prepare_workspace(&workspace_root);
    let harness = DeepSearchHarness::new(build_server(&workspace_root));

    let playbook = DeepSearchHarness::load_playbook(&partial_channel_playbook_path())
        .expect("expected deep-search partial-channel playbook fixture to parse");
    let first = harness
        .run_playbook(&playbook)
        .await
        .expect("first partial-channel deep-search run should succeed");
    let second = harness
        .run_playbook(&playbook)
        .await
        .expect("second partial-channel deep-search run should succeed");

    assert_eq!(
        first, second,
        "partial-channel replay artifact should be deterministic"
    );
    assert_eq!(first.step_count, 3);
    assert!(
        matches!(
            &first.steps[0].outcome,
            DeepSearchTraceOutcome::Err {
                error_code: Some(error_code),
                ..
            } if error_code == "resource_not_found"
        ),
        "first step should capture deterministic resource_not_found error outcome"
    );
    assert!(
        matches!(&first.steps[1].outcome, DeepSearchTraceOutcome::Ok { .. }),
        "second step should still execute and succeed after first-step error"
    );
    assert!(
        matches!(&first.steps[2].outcome, DeepSearchTraceOutcome::Ok { .. }),
        "third step should still execute and succeed after first-step error"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn deep_search_replay_runtime_handler_matches_partial_channel_trace_deterministically() {
    let workspace_root = temp_workspace_root("runtime-partial-channel");
    prepare_workspace(&workspace_root);
    let server = build_runtime_server(&workspace_root);

    let playbook = DeepSearchHarness::load_playbook(&partial_channel_playbook_path())
        .expect("expected deep-search partial-channel playbook fixture to parse");
    let run = server
        .deep_search_run(Parameters(DeepSearchRunParams {
            playbook: playbook.clone().into(),
        }))
        .await
        .expect("deep_search_run should succeed for partial-channel playbook")
        .0;
    let runtime_trace_artifact: frigg::mcp::DeepSearchTraceArtifact =
        run.trace_artifact.clone().into();

    assert!(
        runtime_trace_artifact
            .steps
            .iter()
            .any(|step| matches!(step.outcome, DeepSearchTraceOutcome::Err { .. })),
        "partial-channel playbook should emit at least one deterministic error step"
    );

    let replay = server
        .deep_search_replay(Parameters(DeepSearchReplayParams {
            playbook: playbook.into(),
            expected_trace_artifact: run.trace_artifact.clone(),
        }))
        .await
        .expect("deep_search_replay should execute for partial-channel trace")
        .0;
    assert!(
        replay.matches,
        "partial-channel replay should remain deterministic for identical inputs"
    );
    assert!(
        replay.diff.is_none(),
        "partial-channel deterministic replay should not produce diff"
    );

    cleanup_workspace(&workspace_root);
}
