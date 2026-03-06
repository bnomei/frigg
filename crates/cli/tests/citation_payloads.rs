#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use frigg::mcp::types::DeepSearchComposeCitationsParams;
use frigg::mcp::{
    DeepSearchCitationPayload, DeepSearchHarness, DeepSearchTraceArtifact, DeepSearchTraceOutcome,
    FriggMcpServer,
};
use frigg::settings::FriggConfig;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;

fn fixture_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/citation_payload_trace.json")
}

fn load_fixture_trace() -> DeepSearchTraceArtifact {
    DeepSearchHarness::load_trace_artifact(&fixture_trace_path())
        .expect("citation payload fixture trace must parse")
}

fn runtime_server() -> FriggMcpServer {
    let workspace_root = std::env::current_dir()
        .expect("current working directory should exist for runtime citation tests");
    let config = FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("runtime citation tests must build a valid FriggConfig");
    FriggMcpServer::new_with_runtime_options(config, false, true)
}

fn claim_text_for_tool_call(payload: &DeepSearchCitationPayload, tool_call_id: &str) -> String {
    let citation_id = payload
        .citations
        .iter()
        .find(|citation| citation.tool_call_id == tool_call_id)
        .map(|citation| citation.citation_id.clone())
        .unwrap_or_else(|| panic!("expected citation for tool call {tool_call_id}"));

    payload
        .claims
        .iter()
        .find(|claim| claim.citation_ids.iter().any(|id| id == &citation_id))
        .map(|claim| claim.text.clone())
        .unwrap_or_else(|| panic!("expected claim linked to citation {citation_id}"))
}

fn update_search_text_fragments(
    trace: &mut DeepSearchTraceArtifact,
    excerpt: Option<&str>,
    snippet: Option<&str>,
) {
    let step = trace
        .steps
        .iter_mut()
        .find(|step| step.tool_name == "search_text")
        .expect("expected search_text step in citation payload fixture");

    let response_json = match &step.outcome {
        DeepSearchTraceOutcome::Ok { response_json } => response_json,
        DeepSearchTraceOutcome::Err { .. } => {
            panic!("search_text fixture step must be an ok outcome")
        }
    };
    let mut response = serde_json::from_str::<Value>(response_json)
        .expect("search_text fixture response must parse");
    let matched = response
        .get_mut("matches")
        .and_then(Value::as_array_mut)
        .and_then(|matches| matches.first_mut())
        .and_then(Value::as_object_mut)
        .expect("search_text fixture response must include matches[0] object");
    matched.remove("excerpt");
    matched.remove("snippet");
    if let Some(excerpt) = excerpt {
        matched.insert("excerpt".to_owned(), Value::String(excerpt.to_owned()));
    }
    if let Some(snippet) = snippet {
        matched.insert("snippet".to_owned(), Value::String(snippet.to_owned()));
    }
    step.outcome = DeepSearchTraceOutcome::Ok {
        response_json: serde_json::to_string(&response)
            .expect("failed to serialize updated search_text fixture response"),
    };
}

fn mark_step_error(
    trace: &mut DeepSearchTraceArtifact,
    tool_call_id: &str,
    code: &str,
    message: &str,
    error_code: Option<&str>,
) {
    let step = trace
        .steps
        .iter_mut()
        .find(|step| step.step_id == tool_call_id)
        .unwrap_or_else(|| panic!("expected trace step {} to exist", tool_call_id));
    step.outcome = DeepSearchTraceOutcome::Err {
        code: code.to_owned(),
        message: message.to_owned(),
        error_code: error_code.map(ToOwned::to_owned),
    };
}

#[test]
fn citation_payloads_fixture_completeness() {
    let trace = load_fixture_trace();
    let payload = DeepSearchHarness::compose_citation_payload(&trace, "")
        .expect("citation payload composition should succeed for fixture trace");

    assert_eq!(payload.answer_schema, "frigg.deep_search.answer.v1");
    assert_eq!(payload.playbook_id, "citation-payload-fixture-v1");
    assert!(
        !payload.claims.is_empty(),
        "expected claims to be composed from fixture trace evidence"
    );
    assert_eq!(
        payload.claims.len(),
        payload.citations.len(),
        "current citation composer should emit one citation per claim"
    );

    let citation_ids = payload
        .citations
        .iter()
        .map(|citation| citation.citation_id.clone())
        .collect::<BTreeSet<_>>();
    let step_ids = trace
        .steps
        .iter()
        .map(|step| step.step_id.clone())
        .collect::<BTreeSet<_>>();

    for claim in &payload.claims {
        assert!(
            !claim.citation_ids.is_empty(),
            "claim {} must reference at least one citation",
            claim.claim_id
        );
        for citation_id in &claim.citation_ids {
            assert!(
                citation_ids.contains(citation_id),
                "claim {} references missing citation {}",
                claim.claim_id,
                citation_id
            );
        }
    }

    for citation in &payload.citations {
        assert!(
            step_ids.contains(&citation.tool_call_id),
            "citation {} references unknown tool call id {}",
            citation.citation_id,
            citation.tool_call_id
        );
        assert!(
            !citation.path.trim().is_empty(),
            "citation {} must include a concrete path",
            citation.citation_id
        );
        assert!(
            citation.span.start_line > 0 && citation.span.start_column > 0,
            "citation {} must include positive span coordinates",
            citation.citation_id
        );
    }

    let search_text_claim = claim_text_for_tool_call(&payload, "tool-002");
    assert!(
        search_text_claim.contains("let _ = greeting();"),
        "search_text claim should include excerpt text fragment"
    );
}

#[test]
fn citation_payloads_fixture_deterministic_ordering() {
    let trace = load_fixture_trace();
    let first = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect("first citation payload composition should succeed");
    let second = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect("second citation payload composition should succeed");

    assert_eq!(first, second, "citation payloads must be deterministic");

    let by_tool_call =
        first
            .citations
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut acc, citation| {
                *acc.entry(citation.tool_call_id.clone()).or_insert(0) += 1;
                acc
            });

    assert_eq!(by_tool_call.get("tool-002"), Some(&1));
    assert_eq!(by_tool_call.get("tool-003"), Some(&1));
    assert_eq!(by_tool_call.get("tool-004"), Some(&1));
    assert_eq!(by_tool_call.get("tool-005"), Some(&1));
    assert!(
        !by_tool_call.contains_key("tool-001"),
        "list_repositories is non-file evidence and should not emit citations"
    );
}

#[test]
fn citation_payloads_prefers_excerpt_over_legacy_snippet() {
    let mut trace = load_fixture_trace();
    update_search_text_fragments(
        &mut trace,
        Some("excerpt fragment should win"),
        Some("legacy snippet fallback"),
    );

    let payload = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect("citation payload composition should succeed with dual search_text fragments");
    let search_text_claim = claim_text_for_tool_call(&payload, "tool-002");

    assert!(
        search_text_claim.contains("excerpt fragment should win"),
        "search_text claim should use excerpt when it is present"
    );
    assert!(
        !search_text_claim.contains("legacy snippet fallback"),
        "search_text claim should not use snippet when excerpt is present"
    );
}

#[test]
fn citation_payloads_falls_back_to_legacy_snippet_when_excerpt_missing() {
    let mut trace = load_fixture_trace();
    update_search_text_fragments(&mut trace, None, Some("legacy snippet fallback"));

    let payload = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect("citation payload composition should succeed with snippet fallback");
    let search_text_claim = claim_text_for_tool_call(&payload, "tool-002");

    assert!(
        search_text_claim.contains("legacy snippet fallback"),
        "search_text claim should fall back to snippet when excerpt is absent"
    );
}

#[test]
fn citation_payloads_skip_error_steps_deterministically() {
    let mut trace = load_fixture_trace();
    mark_step_error(
        &mut trace,
        "tool-002",
        "ErrorCode(-32603)",
        "semantic channel strict failure",
        Some("unavailable"),
    );

    let first = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect("citation payload composition should skip error steps");
    let second = DeepSearchHarness::compose_citation_payload(&trace, "answer")
        .expect("citation payload composition should be deterministic when skipping error steps");

    assert_eq!(first, second);
    assert!(
        first
            .citations
            .iter()
            .all(|citation| citation.tool_call_id != "tool-002"),
        "errored step citations must be excluded from composed payload"
    );
}

#[tokio::test]
async fn citation_payloads_runtime_handler_compose_is_deterministic() {
    let server = runtime_server();
    let trace = load_fixture_trace();

    let first = server
        .deep_search_compose_citations(Parameters(DeepSearchComposeCitationsParams {
            trace_artifact: trace.clone().into(),
            answer: None,
        }))
        .await
        .expect("runtime compose citations should succeed")
        .0;
    let second = server
        .deep_search_compose_citations(Parameters(DeepSearchComposeCitationsParams {
            trace_artifact: trace.into(),
            answer: None,
        }))
        .await
        .expect("runtime compose citations should be deterministic")
        .0;

    assert_eq!(first, second);
    assert_eq!(
        first.citation_payload.answer_schema,
        "frigg.deep_search.answer.v1"
    );
    assert!(
        !first.citation_payload.claims.is_empty(),
        "runtime compose citations should emit claims"
    );
    assert_eq!(
        first.citation_payload.claims.len(),
        first.citation_payload.citations.len(),
        "runtime compose citations should emit one citation per claim for current v1 contract"
    );
}
