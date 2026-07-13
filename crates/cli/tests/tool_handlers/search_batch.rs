//! Integration tests for `search_batch` multi-probe merge.

use super::*;
use frigg::mcp::types::{
    NextAction, NextActionOrigin, NextActionTarget, ReplayOriginTarget, SearchBatchMergeMode,
    SearchBatchMergeStrategy, SearchBatchParams, SearchBatchProbe, SearchBatchProbeKind,
    SearchBatchResponse,
};

#[tokio::test]
async fn search_batch_merges_multi_probe_hits_with_probe_ids() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-batch-merge");
    let server = server_for_workspace_root(&workspace_root).await;
    let params = SearchBatchParams {
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
    };

    let response = server
        .search_batch(Parameters(params.clone()))
        .await
        .expect("search_batch should succeed")
        .0;

    assert!(
        !response.matches.is_empty(),
        "expected merged matches for greeting probes: {response:?}"
    );
    assert_eq!(response.probe_summary.len(), 2);
    assert_eq!(
        response.merge_strategy,
        SearchBatchMergeStrategy::ReciprocalRankFusion
    );
    assert_eq!(response.merge_algorithm_version, "rrf-v1");
    assert!(response.matches.iter().all(|matched| {
        matched.consensus_count == matched.evidence.len()
            && matched.rrf_score > 0.0
            && !matched.evidence.is_empty()
    }));
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
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.target_ref.is_some()),
        "every handle-bound batch row should publish its executable target_ref"
    );
    assert_eq!(
        response.recovery.suggested_next,
        response
            .recovery
            .next_actions
            .iter()
            .map(|action| action.to_legacy_suggestion())
            .collect::<Vec<_>>(),
        "legacy rows are a projection of canonical batch actions"
    );
    let proof = response
        .recovery
        .next_actions
        .iter()
        .find_map(|action| match &action.target {
            NextActionTarget::ReadMatch(params) => Some(params.clone()),
            _ => None,
        })
        .expect("batch success must expose an exact proof-read action");
    assert_eq!(
        proof.result_handle,
        response
            .result_handle
            .clone()
            .expect("batch success must expose a result handle")
    );
    assert_eq!(
        proof.match_id,
        response.matches[0]
            .match_id
            .clone()
            .expect("merged batch match must have an opaque match id"),
        "batch proof action must select the top merged row"
    );
    assert_eq!(
        serde_json::to_value(proof.origin.clone()).expect("proof origin serializes"),
        serde_json::to_value(Some(NextActionOrigin(ReplayOriginTarget::SearchBatch(
            params.clone()
        ))))
        .expect("expected batch origin serializes"),
        "batch proof action must carry the exact producer request"
    );
    server
        .read_match(Parameters(proof))
        .await
        .expect("batch proof action must replay through read_match");
    assert!(
        response
            .probe_summary
            .iter()
            .filter(|summary| summary.hits != 0)
            .all(|summary| summary.next_actions.is_empty()),
        "successful probe summaries must not retain child-handle actions"
    );
    let top_target = response.matches[0]
        .target_ref
        .clone()
        .expect("the top rebound batch row should expose a target");
    let target_actions = response
        .recovery
        .next_actions
        .iter()
        .filter(|action| {
            matches!(
                action.target,
                NextActionTarget::GoToDefinition(_)
                    | NextActionTarget::FindReferences(_)
                    | NextActionTarget::ImpactBundle(_)
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(target_actions.len(), 3);
    for action in target_actions {
        match action.target {
            NextActionTarget::GoToDefinition(params) => {
                assert_eq!(params.target.as_ref(), Some(&top_target));
                server
                    .go_to_definition(Parameters(params))
                    .await
                    .expect("batch definition action should replay unchanged");
            }
            NextActionTarget::FindReferences(params) => {
                assert_eq!(params.target.as_ref(), Some(&top_target));
                server
                    .find_references(Parameters(params))
                    .await
                    .expect("batch references action should replay unchanged");
            }
            NextActionTarget::ImpactBundle(params) => {
                assert_eq!(params.target.as_ref(), Some(&top_target));
                assert!(params.symbol.is_empty());
                server
                    .impact_bundle(Parameters(params))
                    .await
                    .expect("batch impact action should replay unchanged");
            }
            _ => unreachable!(),
        }
    }

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_legacy_merge_emits_compatibility_note() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-batch-legacy-merge");
    let server = server_for_workspace_root(&workspace_root).await;
    let params: SearchBatchParams = serde_json::from_value(serde_json::json!({
        "probes": [
            { "id": "text", "kind": "text", "query": "greeting" },
            { "id": "symbol", "kind": "symbol", "query": "greeting" }
        ],
        "merge": "rank_by_probe_hit_strength",
        "limit": 5
    }))
    .expect("the schema-hidden legacy merge spelling deserializes");
    assert_eq!(
        params.merge,
        Some(SearchBatchMergeMode::RankByProbeHitStrength)
    );

    let response = server
        .search_batch(Parameters(params))
        .await
        .expect("legacy merge input normalizes to fixed RRF")
        .0;
    assert_eq!(
        response.merge_strategy,
        SearchBatchMergeStrategy::ReciprocalRankFusion
    );
    assert!(
        response
            .compatibility_note
            .as_deref()
            .is_some_and(|note| note.contains("Deprecated merge=rank_by_probe_hit_strength")),
        "legacy input must be observable as a compatibility deprecation"
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
        merge_strategy: SearchBatchMergeStrategy::ReciprocalRankFusion,
        merge_algorithm_version: "rrf-v1".to_owned(),
        compatibility_note: None,
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

    let mut mixed = base.clone();
    mixed.resume_from = Some(1);
    mixed.continuation = Some("continuation-foreign-000001".to_owned());
    let error = match server.search_batch(Parameters(mixed)).await {
        Ok(_) => panic!("mixed legacy/v2 cursors must be rejected"),
        Err(error) => error,
    };
    let data = error
        .data
        .expect("mixed cursor rejection includes recovery data");
    let actions = data["next_actions"]
        .as_array()
        .expect("mixed cursor rejection includes an exact producer retry");
    assert_eq!(actions.len(), 1);
    let action: NextAction =
        serde_json::from_value(actions[0].clone()).expect("producer retry action is canonical");
    let NextActionTarget::SearchBatch(retry) = action.target else {
        panic!("mixed cursor retry must target search_batch");
    };
    assert_eq!(
        serde_json::to_value(retry).expect("retry serializes"),
        serde_json::to_value(base).expect("base request serializes"),
        "mixed cursor retry must preserve the producer request with both cursor forms removed"
    );
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_rejects_mismatched_continuation_with_exact_producer_retry() {
    let workspace_root = temp_workspace_root("search-batch-continuation-retry");
    fs::create_dir_all(workspace_root.join("src")).expect("create fixture source dir");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "const one: &str = \"batch_retry_needle\";\nconst two: &str = \"batch_retry_needle\";\n",
    )
    .expect("seed retry fixture");
    let server = server_for_workspace_root(&workspace_root).await;
    let base = SearchBatchParams {
        probes: vec![
            SearchBatchProbe {
                id: "first".to_owned(),
                kind: SearchBatchProbeKind::Text,
                query: "batch_retry_needle".to_owned(),
                repository_id: Some("repo-001".to_owned()),
                path_regex: Some("^src/".to_owned()),
                glob: None,
                path_class: None,
                pattern_type: Some(SearchPatternType::Literal),
            },
            SearchBatchProbe {
                id: "second".to_owned(),
                kind: SearchBatchProbeKind::Text,
                query: "batch_retry_needle".to_owned(),
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
        .expect("first page should issue a continuation")
        .0;
    let mut mismatched = base.clone();
    mismatched.probes[0].path_regex = Some("^tests/".to_owned());
    mismatched.continuation = first.completeness.continuation;
    let error = match server.search_batch(Parameters(mismatched)).await {
        Ok(_) => panic!("scope-mismatched continuation must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    let data = error
        .data
        .expect("continuation failure includes recovery data");
    assert_eq!(
        data["continuation"]["code"].as_str(),
        Some("CONTINUATION_SCOPE_MISMATCH")
    );
    let actions = data["next_actions"]
        .as_array()
        .expect("continuation failure includes an exact producer retry");
    assert_eq!(actions.len(), 1);
    let action: NextAction =
        serde_json::from_value(actions[0].clone()).expect("producer retry action is canonical");
    let NextActionTarget::SearchBatch(retry) = action.target else {
        panic!("continuation retry must target search_batch");
    };
    let mut expected_retry = base;
    expected_retry.probes[0].path_regex = Some("^tests/".to_owned());
    assert_eq!(
        serde_json::to_value(retry).expect("retry serializes"),
        serde_json::to_value(expected_retry).expect("retry serializes"),
        "retry must preserve the attempted producer request without its stale cursor"
    );
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_batch_preserves_all_probe_kinds_and_order_across_response_profiles() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-batch-profile-matrix");
    let server = server_for_workspace_root(&workspace_root).await;
    let params = SearchBatchParams {
        probes: vec![
            SearchBatchProbe {
                id: "literal-first".to_owned(),
                kind: SearchBatchProbeKind::Text,
                query: "greeting".to_owned(),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                glob: None,
                path_class: None,
                pattern_type: Some(SearchPatternType::Literal),
            },
            SearchBatchProbe {
                id: "symbol-second".to_owned(),
                kind: SearchBatchProbeKind::Symbol,
                query: "greeting".to_owned(),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                glob: None,
                path_class: Some(SearchSymbolPathClass::Runtime),
                pattern_type: None,
            },
            SearchBatchProbe {
                id: "hybrid-third".to_owned(),
                kind: SearchBatchProbeKind::Hybrid,
                query: "greeting".to_owned(),
                repository_id: None,
                path_regex: None,
                glob: None,
                path_class: None,
                pattern_type: None,
            },
        ],
        merge: None,
        limit: Some(20),
        repository_id: None,
        response_mode: Some(ResponseMode::Compact),
        resume_from: None,
        continuation: None,
    };

    let compact = server
        .search_batch(Parameters(params.clone()))
        .await
        .expect("all three independent probe kinds should compose")
        .0;
    let mut full_params = params;
    full_params.response_mode = Some(ResponseMode::Full);
    let full = server
        .search_batch(Parameters(full_params))
        .await
        .expect("full response must retain the same batch merge")
        .0;

    let summary_kinds = |response: &SearchBatchResponse| {
        response
            .probe_summary
            .iter()
            .map(|summary| {
                (
                    summary.id.clone(),
                    summary.kind,
                    summary.trust,
                    summary.completeness.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        summary_kinds(&compact),
        summary_kinds(&full),
        "compact/full must preserve probe order, kind, trust, and completeness"
    );
    assert_eq!(
        compact
            .probe_summary
            .iter()
            .map(|summary| summary.kind)
            .collect::<Vec<_>>(),
        vec![
            SearchBatchProbeKind::Text,
            SearchBatchProbeKind::Symbol,
            SearchBatchProbeKind::Hybrid,
        ],
        "probe summaries preserve request order and all public probe kinds"
    );

    let ordering_projection = |response: &SearchBatchResponse| {
        response
            .matches
            .iter()
            .map(|matched| {
                (
                    matched.repository_id.clone(),
                    matched.path.clone(),
                    matched.line,
                    matched.column,
                    matched.kind,
                    matched.evidence.clone(),
                    matched.consensus_count,
                    matched.rrf_score,
                    matched.match_strength,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ordering_projection(&compact),
        ordering_projection(&full),
        "response detail must not alter consensus-first RRF ordering or evidence"
    );
    assert!(compact.matches.iter().all(|matched| {
        matched.consensus_count == matched.evidence.len()
            && matched
                .evidence
                .iter()
                .map(|evidence| &evidence.probe_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == matched.evidence.len()
    }));

    // A second fresh execution is the public regression anchor for stable ordering, rather than
    // relying on a continuation token or cached response identity.
    let repeat = server
        .search_batch(Parameters(SearchBatchParams {
            response_mode: Some(ResponseMode::Compact),
            probes: compact
                .probe_summary
                .iter()
                .map(|summary| SearchBatchProbe {
                    id: summary.id.clone(),
                    kind: summary.kind,
                    query: "greeting".to_owned(),
                    repository_id: None,
                    path_regex: (summary.kind != SearchBatchProbeKind::Hybrid)
                        .then(|| "^src/".to_owned()),
                    glob: None,
                    path_class: (summary.kind == SearchBatchProbeKind::Symbol)
                        .then_some(SearchSymbolPathClass::Runtime),
                    pattern_type: (summary.kind == SearchBatchProbeKind::Text)
                        .then_some(SearchPatternType::Literal),
                })
                .collect(),
            merge: None,
            limit: Some(20),
            repository_id: None,
            resume_from: None,
            continuation: None,
        }))
        .await
        .expect("a fresh identical batch should retain stable ordering")
        .0;
    assert_eq!(ordering_projection(&compact), ordering_projection(&repeat));

    cleanup_workspace_root(&workspace_root);
}
