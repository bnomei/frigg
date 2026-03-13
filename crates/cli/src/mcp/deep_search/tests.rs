use super::runtime::{decode_params, diff_trace_artifacts, normalize_trace_response_for_tool};
use super::{
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchPlaybookStep, DeepSearchTraceArtifact,
    DeepSearchTraceOutcome, DeepSearchTraceStep, allowed_step_tools,
};
use crate::domain::FriggError;
use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};
use crate::mcp::types::{ReadFileParams, SearchTextParams};
use crate::settings::FriggConfig;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn make_step(step_index: usize, step_id: &str) -> DeepSearchTraceStep {
    DeepSearchTraceStep {
        step_index,
        step_id: step_id.to_owned(),
        tool_name: "search_text".to_owned(),
        params_json: "{\"query\":\"hello\"}".to_owned(),
        outcome: DeepSearchTraceOutcome::Ok {
            response_json: "{\"matches\":[]}".to_owned(),
        },
    }
}

fn make_trace(step_count: usize, steps: Vec<DeepSearchTraceStep>) -> DeepSearchTraceArtifact {
    DeepSearchTraceArtifact {
        trace_schema: "frigg.deep_search.trace.v1".to_owned(),
        playbook_id: "playbook-suite".to_owned(),
        step_count,
        steps,
    }
}

fn fixture_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/citation_payload_trace.json")
}

fn load_fixture_trace() -> DeepSearchTraceArtifact {
    DeepSearchHarness::load_trace_artifact(&fixture_trace_path())
        .expect("citation payload fixture trace must parse")
}

fn temp_fixture_path(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "frigg-deep-search-unit-{test_name}-{}.json",
        std::process::id()
    ))
}

fn write_temp_fixture(test_name: &str, raw: &str) -> PathBuf {
    let path = temp_fixture_path(test_name);
    fs::write(&path, raw).unwrap_or_else(|err| {
        panic!(
            "failed to write temporary deep-search fixture {}: {err}",
            path.display()
        )
    });
    path
}

fn overwrite_step_response(trace: &mut DeepSearchTraceArtifact, step_id: &str, response: Value) {
    let step = trace
        .steps
        .iter_mut()
        .find(|step| step.step_id == step_id)
        .unwrap_or_else(|| panic!("expected trace step {step_id} to exist"));
    step.outcome = DeepSearchTraceOutcome::Ok {
        response_json: serde_json::to_string(&response)
            .expect("failed to serialize deep-search test response"),
    };
}

fn invalid_input_message(error: FriggError) -> String {
    match error {
        FriggError::InvalidInput(message) => message,
        other => panic!("expected invalid input error, got {other:?}"),
    }
}

fn test_harness() -> DeepSearchHarness {
    let workspace_root = std::env::current_dir()
        .expect("current working directory should exist for deep-search unit tests");
    let config = FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("current workspace should build a valid FriggConfig");
    DeepSearchHarness::new(crate::mcp::server::FriggMcpServer::new(config))
}

#[test]
fn playbook_suite_diff_reports_actual_steps_length_mismatch_before_zip() {
    let expected = make_trace(2, vec![make_step(0, "step-1"), make_step(1, "step-2")]);
    let actual = make_trace(2, vec![make_step(0, "step-1")]);

    let diff = diff_trace_artifacts(&expected, &actual);
    assert_eq!(
        diff.as_deref(),
        Some("actual trace steps length mismatch: step_count=2 steps_len=1")
    );
}

#[test]
fn playbook_suite_diff_reports_expected_steps_length_mismatch_before_zip() {
    let expected = make_trace(2, vec![make_step(0, "step-1")]);
    let actual = make_trace(2, vec![make_step(0, "step-1"), make_step(1, "step-2")]);

    let diff = diff_trace_artifacts(&expected, &actual);
    assert_eq!(
        diff.as_deref(),
        Some("expected trace steps length mismatch: step_count=2 steps_len=1")
    );
}

#[test]
fn playbook_suite_diff_prioritizes_actual_structure_mismatch_over_step_count_mismatch() {
    let expected = make_trace(
        3,
        vec![
            make_step(0, "step-1"),
            make_step(1, "step-2"),
            make_step(2, "step-3"),
        ],
    );
    let actual = make_trace(2, vec![make_step(0, "step-1")]);

    let diff = diff_trace_artifacts(&expected, &actual);
    assert_eq!(
        diff.as_deref(),
        Some("actual trace steps length mismatch: step_count=2 steps_len=1")
    );
}

#[test]
fn playbook_suite_load_playbook_reports_parse_failure_with_path_context() {
    let path = write_temp_fixture("invalid-playbook", "{");

    let error = DeepSearchHarness::load_playbook(&path)
        .expect_err("malformed playbook fixture should fail to parse");
    let message = invalid_input_message(error);

    assert!(message.contains("failed to parse deep-search playbook"));
    assert!(message.contains(&path.display().to_string()));

    let _ = fs::remove_file(path);
}

#[test]
fn playbook_suite_load_trace_artifact_reports_parse_failure_with_path_context() {
    let path = write_temp_fixture("invalid-trace-artifact", "{");

    let error = DeepSearchHarness::load_trace_artifact(&path)
        .expect_err("malformed trace artifact fixture should fail to parse");
    let message = invalid_input_message(error);

    assert!(message.contains("failed to parse deep-search trace artifact"));
    assert!(message.contains(&path.display().to_string()));

    let _ = fs::remove_file(path);
}

#[test]
fn playbook_suite_persist_trace_artifact_round_trips_canonical_json() {
    let path = temp_fixture_path("persist-trace-artifact");
    let artifact = make_trace(2, vec![make_step(0, "step-1"), make_step(1, "step-2")]);

    DeepSearchHarness::persist_trace_artifact(&path, &artifact)
        .expect("trace artifact persistence should succeed");
    let persisted = DeepSearchHarness::load_trace_artifact(&path)
        .expect("persisted trace artifact should load");

    assert_eq!(persisted, artifact);

    let _ = fs::remove_file(path);
}

#[test]
fn playbook_suite_decode_params_wraps_missing_required_fields_as_invalid_params() {
    let error = decode_params::<ReadFileParams>(&json!({}))
        .expect_err("missing read_file path should fail param decoding");

    assert_eq!(error.code, "INVALID_PARAMS");
    assert_eq!(error.error_code.as_deref(), Some("invalid_params"));
    assert!(error.message.contains("invalid playbook step params"));
    assert!(error.message.contains("missing field `path`"));
}

#[test]
fn playbook_suite_decode_params_wraps_type_errors_as_invalid_params() {
    let error = decode_params::<SearchTextParams>(&json!({ "query": 7 }))
        .expect_err("wrong query type should fail param decoding");

    assert_eq!(error.code, "INVALID_PARAMS");
    assert_eq!(error.error_code.as_deref(), Some("invalid_params"));
    assert!(error.message.contains("invalid playbook step params"));
    assert!(error.message.contains("expected a string"));
}

#[test]
fn playbook_suite_allowed_step_tools_remain_subset_of_core_manifest() {
    let core_tools = manifest_for_tool_surface_profile(ToolSurfaceProfile::Core)
        .tool_names
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    for tool_name in allowed_step_tools() {
        assert!(
            core_tools.contains(*tool_name),
            "deep-search allowed step tool must remain in the stable core surface: {tool_name}"
        );
    }
}

#[tokio::test]
async fn playbook_suite_validates_all_step_tools_before_executing_any_step() {
    let harness = test_harness();
    let playbook = DeepSearchPlaybook {
        playbook_id: "preflight-tool-validation".to_owned(),
        steps: vec![
            DeepSearchPlaybookStep {
                step_id: "tool-001".to_owned(),
                tool_name: "search_text".to_owned(),
                params: json!({
                    "query": "",
                    "repository_id": "repo-001",
                }),
            },
            DeepSearchPlaybookStep {
                step_id: "tool-002".to_owned(),
                tool_name: "write_file".to_owned(),
                params: json!({ "path": "src/lib.rs" }),
            },
        ],
    };

    let error = harness
        .run_playbook(&playbook)
        .await
        .expect_err("unsupported step tools should be rejected before executing earlier steps");
    let message = invalid_input_message(error);

    assert!(
        message.contains("tool-002"),
        "preflight validation should point at the unsupported step, got: {message}"
    );
    assert!(
        message.contains("allowed tools:"),
        "preflight validation should surface the stable-core allowlist, got: {message}"
    );
    assert!(
        !message.contains("failed with invalid_params"),
        "unsupported-tool validation should happen before earlier step execution, got: {message}"
    );
}

#[test]
fn playbook_suite_compose_citation_payload_rejects_invalid_response_json() {
    let mut trace = load_fixture_trace();
    let step = trace
        .steps
        .iter_mut()
        .find(|step| step.step_id == "tool-002")
        .expect("expected search_text fixture step");
    step.outcome = DeepSearchTraceOutcome::Ok {
        response_json: "{".to_owned(),
    };

    let error = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect_err("invalid step response_json should fail citation composition");
    let message = invalid_input_message(error);

    assert!(message.contains("failed to parse response_json for deep-search step tool-002"));
}

#[test]
fn playbook_suite_compose_citation_payload_requires_matches_array_for_match_tools() {
    let mut trace = load_fixture_trace();
    overwrite_step_response(&mut trace, "tool-002", json!({}));

    let error = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect_err("missing matches[] should fail citation composition");
    let message = invalid_input_message(error);

    assert_eq!(
        message,
        "tool search_text step tool-002 response is missing matches[] for citation composition"
    );
}

#[test]
fn playbook_suite_compose_citation_payload_requires_non_empty_string_fields() {
    let mut trace = load_fixture_trace();
    overwrite_step_response(
        &mut trace,
        "tool-003",
        json!({
            "bytes": 18,
            "content": "line 1\nline 2\n",
            "path": "src/lib.rs",
            "repository_id": "   "
        }),
    );

    let error = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect_err("blank repository_id should fail citation composition");
    let message = invalid_input_message(error);

    assert_eq!(
        message,
        "tool read_file step tool-003 is missing required string field 'repository_id' for citation composition"
    );
}

#[test]
fn playbook_suite_compose_citation_payload_requires_numeric_fields() {
    let mut trace = load_fixture_trace();
    overwrite_step_response(
        &mut trace,
        "tool-005",
        json!({
            "matches": [{
                "line": 3,
                "path": "src/lib.rs",
                "repository_id": "repo-001",
                "symbol": "greeting"
            }],
            "note": "{\"precision\":\"heuristic\"}"
        }),
    );

    let error = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect_err("missing numeric match field should fail citation composition");
    let message = invalid_input_message(error);

    assert_eq!(
        message,
        "tool find_references step tool-005 match 0 is missing required numeric field 'column' for citation composition"
    );
}

#[test]
fn playbook_suite_normalizes_list_repositories_to_stable_identity_fields() {
    let normalized = normalize_trace_response_for_tool(
        "list_repositories",
        json!({
            "repositories": [{
                "repository_id": "repo-001",
                "display_name": "fixture",
                "root_path": "/tmp/fixture",
                "storage": {
                    "exists": true,
                    "initialized": true
                },
                "health": {
                    "lexical": {
                        "state": "missing",
                        "reason": "missing_manifest_snapshot"
                    }
                }
            }]
        }),
    );

    assert_eq!(
        normalized,
        json!({
            "repositories": [{
                "repository_id": "repo-001",
                "display_name": "fixture",
                "root_path": "/tmp/fixture"
            }]
        })
    );
}

#[tokio::test]
async fn playbook_suite_run_step_rejects_unsupported_tool_with_invalid_params() {
    let harness = test_harness();
    let outcome = harness
        .run_step(&DeepSearchPlaybookStep {
            step_id: "tool-999".to_owned(),
            tool_name: "write_file".to_owned(),
            params: json!({ "path": "src/lib.rs" }),
        })
        .await;

    assert_eq!(
        outcome,
        DeepSearchTraceOutcome::Err {
            code: "INVALID_PARAMS".to_owned(),
            message: "unsupported tool in playbook step 'tool-999': write_file".to_owned(),
            error_code: Some("invalid_params".to_owned()),
        }
    );
}

#[tokio::test]
async fn playbook_suite_run_step_wraps_decode_failures_as_invalid_params() {
    let harness = test_harness();
    let outcome = harness
        .run_step(&DeepSearchPlaybookStep {
            step_id: "tool-002".to_owned(),
            tool_name: "read_file".to_owned(),
            params: json!({}),
        })
        .await;

    match outcome {
        DeepSearchTraceOutcome::Err {
            code,
            message,
            error_code,
        } => {
            assert_eq!(code, "INVALID_PARAMS");
            assert_eq!(error_code.as_deref(), Some("invalid_params"));
            assert!(message.contains("invalid playbook step params"));
            assert!(message.contains("missing field `path`"));
        }
        DeepSearchTraceOutcome::Ok { .. } => {
            panic!("invalid read_file params should not succeed")
        }
    }
}
