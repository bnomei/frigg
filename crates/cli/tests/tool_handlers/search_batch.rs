//! Integration tests for `search_batch` multi-probe merge (`FUT-008`).

use super::*;
use frigg::mcp::types::{
    SearchBatchParams, SearchBatchProbe, SearchBatchProbeKind, SearchBatchResponse,
};

#[tokio::test]
async fn search_batch_merges_multi_probe_hits_with_probe_ids() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-batch-merge");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .search_batch(Parameters(SearchBatchParams {
            probes: vec![
                SearchBatchProbe {
                    id: "text-greeting".to_owned(),
                    kind: SearchBatchProbeKind::Text,
                    query: "greeting".to_owned(),
                    repository_id: None,
                    path_regex: Some("^src/".to_owned()),
                    glob: None,
                    path_class: None,
                    pattern_type: Some(SearchPatternType::Literal),
                },
                SearchBatchProbe {
                    id: "symbol-greeting".to_owned(),
                    kind: SearchBatchProbeKind::Symbol,
                    query: "greeting".to_owned(),
                    repository_id: None,
                    path_regex: Some("^src/".to_owned()),
                    glob: None,
                    path_class: Some(SearchSymbolPathClass::Runtime),
                    pattern_type: None,
                },
            ],
            merge: None,
            limit: Some(20),
            repository_id: None,
            response_mode: Some(ResponseMode::Compact),
            resume_from: None,
        }))
        .await
        .expect("search_batch should succeed")
        .0;

    assert!(
        !response.matches.is_empty(),
        "expected merged matches for greeting probes: {response:?}"
    );
    assert_eq!(response.probe_summary.len(), 2);
    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.probe_ids.iter().any(|id| id.contains("greeting"))),
        "matches should carry probe_id(s)"
    );
    assert!(
        response.result_handle.is_some(),
        "batch should assign a result_handle for read_match"
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.match_id.is_some()),
        "batch matches should expose match_ids"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_all_zero_emits_probe_diagnostics() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-batch-zero");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .search_batch(Parameters(SearchBatchParams {
            probes: vec![
                SearchBatchProbe {
                    id: "p1".to_owned(),
                    kind: SearchBatchProbeKind::Text,
                    query: "zzznomatch_batch_probe_one_unique".to_owned(),
                    repository_id: None,
                    path_regex: Some("^src/".to_owned()),
                    glob: None,
                    path_class: None,
                    pattern_type: Some(SearchPatternType::Literal),
                },
                SearchBatchProbe {
                    id: "p2".to_owned(),
                    kind: SearchBatchProbeKind::Symbol,
                    query: "ZzNoMatchBatchProbeTwoUnique".to_owned(),
                    repository_id: None,
                    path_regex: Some("^src/".to_owned()),
                    glob: None,
                    path_class: Some(SearchSymbolPathClass::Runtime),
                    pattern_type: None,
                },
            ],
            merge: None,
            limit: Some(10),
            repository_id: None,
            response_mode: Some(ResponseMode::Compact),
            resume_from: None,
        }))
        .await
        .expect("all-zero search_batch should succeed")
        .0;

    assert!(response.matches.is_empty());
    assert_eq!(response.probe_summary.len(), 2);
    assert!(response.probe_summary.iter().all(|summary| summary.hits == 0));
    assert!(
        response
            .probe_summary
            .iter()
            .any(|summary| summary.zero_hit_reason.is_some()
                || summary.correction_hint.is_some()
                || !summary.suggested_next.is_empty()),
        "per-probe diagnostics should surface on all-zero batch: {:?}",
        response.probe_summary
    );
    assert!(
        !response.recovery.suggested_next.is_empty()
            || response.recovery.error_code.as_deref() == Some("BATCH_ALL_ZERO"),
        "batch recovery should be actionable: {:?}",
        response.recovery
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_rejects_one_probe_and_more_than_eight() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-batch-bounds");
    let server = server_for_workspace_root(&workspace_root).await;

    let one = server
        .search_batch(Parameters(SearchBatchParams {
            probes: vec![SearchBatchProbe {
                id: "only".to_owned(),
                kind: SearchBatchProbeKind::Text,
                query: "greeting".to_owned(),
                repository_id: None,
                path_regex: None,
                glob: None,
                path_class: None,
                pattern_type: None,
            }],
            merge: None,
            limit: None,
            repository_id: None,
            response_mode: None,
            resume_from: None,
        }))
        .await;
    assert!(one.is_err(), "1-probe batch must be rejected");

    let many = (0..9)
        .map(|idx| SearchBatchProbe {
            id: format!("p{idx}"),
            kind: SearchBatchProbeKind::Text,
            query: format!("q{idx}"),
            repository_id: None,
            path_regex: None,
            glob: None,
            path_class: None,
            pattern_type: None,
        })
        .collect::<Vec<_>>();
    let too_many = server
        .search_batch(Parameters(SearchBatchParams {
            probes: many,
            merge: None,
            limit: None,
            repository_id: None,
            response_mode: None,
            resume_from: None,
        }))
        .await;
    assert!(too_many.is_err(), ">8 probes must be rejected");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_response_wire_shape_includes_summaries() {
    let value = serde_json::to_value(SearchBatchResponse {
        matches: Vec::new(),
        probe_summary: Vec::new(),
        returned: 0,
        truncated: false,
        resume_from: None,
        result_handle: None,
        handle_scope: None,
        handle_expires: None,
        latency_class: None,
        recovery: RecoveryFields::default(),
    })
    .expect("search_batch response should serialize");
    assert!(value.get("matches").is_some());
    assert!(value.get("probe_summary").is_some());
    assert_eq!(value.get("returned"), Some(&serde_json::json!(0)));
    assert_eq!(value.get("truncated"), Some(&serde_json::json!(false)));
}
