//! Integration tests for `search_batch` multi-probe merge.

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
            continuation: None,
        }))
        .await
        .expect("search_batch should succeed")
        .0;

    assert!(
        !response.matches.is_empty(),
        "expected merged matches for greeting probes: {response:?}"
    );
    assert_eq!(response.probe_summary.len(), 2);
    assert_eq!(response.completeness.returned, response.matches.len());
    assert_eq!(response.completeness.total, Some(response.matches.len()));
    assert!(response.completeness.complete);
    assert!(
        response
            .probe_summary
            .iter()
            .all(|summary| summary.completeness.complete)
    );
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
            continuation: None,
        }))
        .await
        .expect("all-zero search_batch should succeed")
        .0;

    assert!(response.matches.is_empty());
    assert_eq!(response.probe_summary.len(), 2);
    assert!(
        response
            .probe_summary
            .iter()
            .all(|summary| summary.hits == 0)
    );
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
    assert!(response.completeness.complete);
    assert_eq!(response.completeness.total, Some(0));

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
            continuation: None,
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
            continuation: None,
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
        completeness: ResultCompleteness::complete(ResultUnit::BatchProbe, 0, 0)
            .expect("empty batch fixture is complete"),
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
    assert!(value.get("completeness").is_some());
    assert_eq!(value.get("returned"), Some(&serde_json::json!(0)));
    assert_eq!(value.get("truncated"), Some(&serde_json::json!(false)));
}

#[tokio::test]
async fn search_batch_exposes_child_caps_and_never_upgrades_aggregate_truth() {
    let workspace_root = temp_workspace_root("search-batch-child-cap-completeness");
    fs::create_dir_all(workspace_root.join("src")).expect("create fixture source dir");
    let rows = (0..45)
        .map(|index| format!("pub const NEEDLE_{index}: &str = \"batch_cap_needle\";"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace_root.join("src/lib.rs"), rows).expect("seed cap fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .search_batch(Parameters(SearchBatchParams {
            probes: vec![
                SearchBatchProbe {
                    id: "first".to_owned(),
                    kind: SearchBatchProbeKind::Text,
                    query: "batch_cap_needle".to_owned(),
                    repository_id: Some("repo-001".to_owned()),
                    path_regex: Some("^src/".to_owned()),
                    glob: None,
                    path_class: None,
                    pattern_type: Some(SearchPatternType::Literal),
                },
                SearchBatchProbe {
                    id: "second".to_owned(),
                    kind: SearchBatchProbeKind::Text,
                    query: "batch_cap_needle".to_owned(),
                    repository_id: Some("repo-001".to_owned()),
                    path_regex: Some("^src/".to_owned()),
                    glob: None,
                    path_class: None,
                    pattern_type: Some(SearchPatternType::Literal),
                },
            ],
            merge: None,
            limit: Some(40),
            repository_id: Some("repo-001".to_owned()),
            response_mode: Some(ResponseMode::Compact),
            resume_from: None,
            continuation: None,
        }))
        .await
        .expect("batch with capped children should succeed")
        .0;

    assert!(response.probe_summary.iter().all(|summary| {
        summary.completeness.truncated
            && !summary.completeness.complete
            && summary.completeness.total == Some(45)
    }));
    assert!(response.completeness.truncated);
    assert!(!response.completeness.complete);
    assert!(
        response
            .completeness
            .truncation_reasons
            .contains(&frigg::mcp::types::ResultTruncationReason::ChildLimit)
    );
    assert!(
        response
            .completeness
            .incomplete_reasons
            .contains(&ResultIncompleteReason::ChildIncomplete)
    );
    assert!(response.completeness.total.is_none());
    assert!(response.completeness.continuation.is_none());
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_v2_pages_are_deterministic_and_reject_mixed_cursors() {
    let workspace_root = temp_workspace_root("search-batch-v2-pages");
    fs::create_dir_all(workspace_root.join("src")).expect("create fixture source dir");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "const one: &str = \"batch_page_needle\";\nconst two: &str = \"batch_page_needle\";\nconst three: &str = \"batch_page_needle\";\n",
    )
    .expect("seed paging fixture");
    let server = server_for_workspace_root(&workspace_root).await;
    let base = SearchBatchParams {
        probes: vec![
            SearchBatchProbe {
                id: "first".to_owned(),
                kind: SearchBatchProbeKind::Text,
                query: "batch_page_needle".to_owned(),
                repository_id: Some("repo-001".to_owned()),
                path_regex: Some("^src/".to_owned()),
                glob: None,
                path_class: None,
                pattern_type: Some(SearchPatternType::Literal),
            },
            SearchBatchProbe {
                id: "second".to_owned(),
                kind: SearchBatchProbeKind::Text,
                query: "batch_page_needle".to_owned(),
                repository_id: Some("repo-001".to_owned()),
                path_regex: Some("^src/".to_owned()),
                glob: None,
                path_class: None,
                pattern_type: Some(SearchPatternType::Literal),
            },
        ],
        merge: None,
        limit: Some(1),
        repository_id: Some("repo-001".to_owned()),
        response_mode: Some(ResponseMode::Compact),
        resume_from: None,
        continuation: None,
    };
    let first = server
        .search_batch(Parameters(base.clone()))
        .await
        .expect("first batch page")
        .0;
    let token = first
        .completeness
        .continuation
        .clone()
        .expect("first exhaustive batch page needs v2 continuation");
    let mut second_params = base.clone();
    second_params.continuation = Some(token);
    let second = server
        .search_batch(Parameters(second_params))
        .await
        .expect("second batch page")
        .0;
    let token = second
        .completeness
        .continuation
        .clone()
        .expect("second batch page needs v2 continuation");
    let mut final_params = base.clone();
    final_params.continuation = Some(token);
    let final_page = server
        .search_batch(Parameters(final_params))
        .await
        .expect("final batch page")
        .0;

    let lines = [
        first.matches[0].line,
        second.matches[0].line,
        final_page.matches[0].line,
    ];
    assert_eq!(lines, [1, 2, 3]);
    assert!(final_page.completeness.complete);
    assert_eq!(final_page.completeness.total, Some(3));
    assert!(final_page.completeness.continuation.is_none());

    let mut mixed = base;
    mixed.resume_from = Some(1);
    mixed.continuation = Some("continuation-foreign-000001".to_owned());
    let error = server.search_batch(Parameters(mixed)).await;
    assert!(error.is_err(), "mixed legacy/v2 cursors must be rejected");
    cleanup_workspace_root(&workspace_root);
}
