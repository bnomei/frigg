#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::{
    DeepSearchHarness, DeepSearchTraceArtifact, DeepSearchTraceOutcome, FriggMcpServer,
};
use frigg::settings::FriggConfig;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct PlaybookSuiteExpectation {
    suite_schema: String,
    playbook_id: String,
    steps: Vec<PlaybookStepExpectation>,
}

#[derive(Debug, Deserialize)]
struct PlaybookStepExpectation {
    step_id: String,
    tool_name: String,
    status: String,
    mcp_code: Option<String>,
    error_code: Option<String>,
    min_matches: Option<usize>,
    min_repositories: Option<usize>,
    require_note: Option<bool>,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repos/manifest-determinism")
}

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-playbook-suite-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn playbooks_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/playbooks")
}

fn copy_workspace_fixture(source: &Path, destination: &Path) {
    fs::create_dir_all(destination)
        .unwrap_or_else(|err| panic!("failed to create fixture workspace copy root: {err}"));
    for entry in fs::read_dir(source)
        .unwrap_or_else(|err| panic!("failed to read fixture workspace root: {err}"))
    {
        let entry =
            entry.unwrap_or_else(|err| panic!("failed to read fixture workspace entry: {err}"));
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .unwrap_or_else(|err| panic!("failed to stat fixture workspace entry: {err}"));
        if file_type.is_dir() {
            copy_workspace_fixture(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).unwrap_or_else(|err| {
                panic!(
                    "failed to copy fixture workspace file {} -> {}: {err}",
                    source_path.display(),
                    destination_path.display()
                )
            });
        }
    }
}

fn prepare_workspace(root: &Path) {
    copy_workspace_fixture(&fixture_root(), root);
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn build_server(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("playbook suite fixture root must produce valid config");
    FriggMcpServer::new(config)
}

fn suite_cases() -> [(&'static str, &'static str); 2] {
    [
        (
            "deep-search-suite-core.playbook.json",
            "deep-search-suite-core.expected.json",
        ),
        (
            "deep-search-suite-partial-channel.playbook.json",
            "deep-search-suite-partial-channel.expected.json",
        ),
    ]
}

fn load_expected(file_name: &str) -> PlaybookSuiteExpectation {
    let path = playbooks_root().join(file_name);
    let raw = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("failed to read playbook suite expected output fixture: {err}")
    });
    serde_json::from_str::<PlaybookSuiteExpectation>(&raw).unwrap_or_else(|err| {
        panic!(
            "failed to parse playbook suite expected output fixture {}: {err}",
            path.display()
        )
    })
}

fn parse_response_json(step_id: &str, response_json: &str) -> Value {
    serde_json::from_str::<Value>(response_json).unwrap_or_else(|err| {
        panic!("failed to parse response_json for playbook suite step {step_id}: {err}")
    })
}

fn response_matches_len(response: &Value) -> Option<usize> {
    response
        .get("matches")
        .and_then(Value::as_array)
        .map(Vec::len)
}

fn response_repositories_len(response: &Value) -> Option<usize> {
    response
        .get("repositories")
        .and_then(Value::as_array)
        .map(Vec::len)
}

fn normalize_mcp_code(raw: &str) -> &str {
    match raw {
        "ErrorCode(-32603)" => "INTERNAL_ERROR",
        "ErrorCode(-32602)" => "INVALID_PARAMS",
        "ErrorCode(-32001)" => "RESOURCE_NOT_FOUND",
        other => other,
    }
}

fn assert_step_expectations(
    artifact: &DeepSearchTraceArtifact,
    expected: &PlaybookSuiteExpectation,
) {
    assert_eq!(
        expected.suite_schema, "frigg.deep_search.playbook_suite.v1",
        "unexpected playbook suite expected-output schema"
    );
    assert_eq!(
        artifact.playbook_id, expected.playbook_id,
        "playbook id mismatch for suite execution"
    );
    assert_eq!(
        artifact.step_count,
        expected.steps.len(),
        "step count mismatch for {}",
        expected.playbook_id
    );

    for expected_step in &expected.steps {
        let actual_step = artifact
            .steps
            .iter()
            .find(|step| step.step_id == expected_step.step_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing expected step {} in playbook {}",
                    expected_step.step_id, expected.playbook_id
                )
            });
        assert_eq!(
            actual_step.tool_name, expected_step.tool_name,
            "tool mismatch for expected step {}",
            expected_step.step_id
        );

        match expected_step.status.as_str() {
            "ok" => {
                let response_json = match &actual_step.outcome {
                    DeepSearchTraceOutcome::Ok { response_json } => response_json,
                    DeepSearchTraceOutcome::Err {
                        code,
                        message,
                        error_code,
                    } => {
                        panic!(
                            "expected step {} to succeed but got error code={} message={} error_code={:?}",
                            expected_step.step_id, code, message, error_code
                        )
                    }
                };
                let response = parse_response_json(&expected_step.step_id, response_json);

                if let Some(min_matches) = expected_step.min_matches {
                    let actual_matches = response_matches_len(&response).unwrap_or_else(|| {
                        panic!(
                            "expected step {} response to include matches[]",
                            expected_step.step_id
                        )
                    });
                    assert!(
                        actual_matches >= min_matches,
                        "expected at least {min_matches} matches for {}, got {actual_matches}",
                        expected_step.step_id
                    );
                }

                if let Some(min_repositories) = expected_step.min_repositories {
                    let actual_repositories =
                        response_repositories_len(&response).unwrap_or_else(|| {
                            panic!(
                                "expected step {} response to include repositories[]",
                                expected_step.step_id
                            )
                        });
                    assert!(
                        actual_repositories >= min_repositories,
                        "expected at least {min_repositories} repositories for {}, got {actual_repositories}",
                        expected_step.step_id
                    );
                }

                if expected_step.require_note.unwrap_or(false) {
                    assert!(
                        response.get("note").and_then(Value::as_str).is_some(),
                        "expected step {} response to include note metadata",
                        expected_step.step_id
                    );
                }
            }
            "err" => match &actual_step.outcome {
                DeepSearchTraceOutcome::Ok { response_json } => {
                    panic!(
                        "expected step {} to fail but got response {}",
                        expected_step.step_id, response_json
                    )
                }
                DeepSearchTraceOutcome::Err {
                    code, error_code, ..
                } => {
                    if let Some(expected_code) = &expected_step.mcp_code {
                        assert_eq!(
                            normalize_mcp_code(code),
                            expected_code,
                            "mcp error code mismatch for step {}",
                            expected_step.step_id
                        );
                    }
                    if let Some(expected_error_code) = &expected_step.error_code {
                        assert_eq!(
                            error_code.as_deref(),
                            Some(expected_error_code.as_str()),
                            "error_code mismatch for step {}",
                            expected_step.step_id
                        );
                    }
                }
            },
            other => panic!(
                "unsupported expected status '{}' for step {}",
                other, expected_step.step_id
            ),
        }
    }
}

#[tokio::test]
async fn playbook_suite_executes_against_expected_outputs() {
    let workspace_root = temp_workspace_root("expected-outputs");
    prepare_workspace(&workspace_root);
    let harness = DeepSearchHarness::new(build_server(&workspace_root));
    for (playbook_file, expected_file) in suite_cases() {
        let playbook = DeepSearchHarness::load_playbook(&playbooks_root().join(playbook_file))
            .unwrap_or_else(|err| panic!("failed to load playbook fixture {playbook_file}: {err}"));
        let artifact = harness
            .run_playbook(&playbook)
            .await
            .unwrap_or_else(|err| panic!("playbook suite run failed for {playbook_file}: {err}"));
        let expected = load_expected(expected_file);
        assert_step_expectations(&artifact, &expected);
    }
    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn playbook_suite_execution_is_deterministic() {
    let workspace_root = temp_workspace_root("deterministic");
    prepare_workspace(&workspace_root);
    let harness = DeepSearchHarness::new(build_server(&workspace_root));
    for (playbook_file, _) in suite_cases() {
        let playbook = DeepSearchHarness::load_playbook(&playbooks_root().join(playbook_file))
            .unwrap_or_else(|err| panic!("failed to load playbook fixture {playbook_file}: {err}"));
        let first = harness.run_playbook(&playbook).await.unwrap_or_else(|err| {
            panic!("first playbook suite run failed for {playbook_file}: {err}")
        });
        let second = harness.run_playbook(&playbook).await.unwrap_or_else(|err| {
            panic!("second playbook suite run failed for {playbook_file}: {err}")
        });

        assert_eq!(
            first, second,
            "playbook suite output must be deterministic for {playbook_file}"
        );
    }
    cleanup_workspace(&workspace_root);
}
