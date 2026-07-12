//! search_text / hybrid polish integration tests.

use super::*;
use frigg::mcp::types::ResultUnit;

#[tokio::test]
async fn search_text_exact_pages_preserve_raw_total_and_exhaust_without_duplicates() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-text-continuation");
    fs::write(workspace_root.join("src/a.rs"), "needle\nneedle\n")
        .expect("seed first exact-search page fixture");
    fs::write(workspace_root.join("src/b.rs"), "needle\n")
        .expect("seed later exact-search page fixture");
    let server = server_for_workspace_root(&workspace_root).await;
    let params = SearchTextParams {
        query: "needle".to_owned(),
        pattern_type: Some(SearchPatternType::Literal),
        limit: Some(1),
        response_mode: Some(ResponseMode::Compact),
        ..Default::default()
    };

    let first = server
        .search_text(Parameters(params.clone()))
        .await
        .expect("first exact-search page should succeed")
        .0;
    assert_eq!(first.total_matches, 3);
    assert_eq!(first.completeness.unit, ResultUnit::Occurrence);
    assert_eq!(first.completeness.total, Some(3));
    assert_eq!(first.completeness.returned, 1);
    assert!(!first.completeness.complete);
    assert!(first.completeness.truncated);
    let second_params = SearchTextParams {
        continuation: first.completeness.continuation.clone(),
        ..params.clone()
    };
    let second = server
        .search_text(Parameters(second_params))
        .await
        .expect("second exact-search page should succeed")
        .0;
    let third_params = SearchTextParams {
        continuation: second.completeness.continuation.clone(),
        ..params.clone()
    };
    let third = server
        .search_text(Parameters(third_params))
        .await
        .expect("third exact-search page should succeed")
        .0;

    assert!(third.completeness.complete);
    assert_eq!(third.completeness.returned, 1);
    assert_eq!(third.completeness.total, Some(3));
    assert_eq!(third.completeness.continuation, None);
    let rows = [first, second, third]
        .into_iter()
        .flat_map(|page| page.matches)
        .map(|matched| (matched.path, matched.line, matched.column))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        rows.len(),
        3,
        "continuation must not duplicate or omit rows"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_text_count_only_echoes_flag_and_total() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-text-count-only");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "greeting".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            limit: None,
            context_lines: None,
            case_sensitive: None,
            ignore_case: None,
            word: None,
            files_with_matches: None,
            count_only: Some(true),
            glob: None,
            exclude_glob: None,
            include_hidden: None,
            max_count_per_file: None,
            collapse_by_file: None,
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("count_only search_text should succeed")
        .0;

    assert_eq!(response.count_only, Some(true));
    assert!(
        response.total_matches > 0,
        "count_only should report total_matches: {response:?}"
    );
    assert!(
        response.matches.is_empty(),
        "count_only intentionally returns empty matches[]"
    );
    assert!(response.result_handle.is_none());
    assert!(response.latency_class.is_some());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_text_scope_echo_and_regex_trap() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-text-scope-regex");
    let server = server_for_workspace_root(&workspace_root).await;

    let scoped = server
        .search_text(Parameters(SearchTextParams {
            query: "greeting".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(5),
            context_lines: None,
            case_sensitive: None,
            ignore_case: None,
            word: None,
            files_with_matches: None,
            count_only: None,
            glob: None,
            exclude_glob: None,
            include_hidden: None,
            max_count_per_file: None,
            collapse_by_file: None,
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("scoped search_text should succeed")
        .0;
    assert!(
        scoped
            .recovery
            .scope
            .as_ref()
            .and_then(|scope| scope.path_regex.as_deref())
            == Some("^src/"),
        "applied scope should echo path_regex: {:?}",
        scoped.recovery.scope
    );

    let regex_trap = server
        .search_text(Parameters(SearchTextParams {
            query: "foo|bar_nomatch_unique".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(5),
            context_lines: None,
            case_sensitive: None,
            ignore_case: None,
            word: None,
            files_with_matches: None,
            count_only: None,
            glob: None,
            exclude_glob: None,
            include_hidden: None,
            max_count_per_file: None,
            collapse_by_file: None,
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("regex-trap search_text should succeed")
        .0;
    assert_eq!(regex_trap.total_matches, 0);
    assert_eq!(
        regex_trap.recovery.error_code.as_deref(),
        Some("QUERY_LOOKS_LIKE_REGEX")
    );
    assert!(
        regex_trap
            .recovery
            .suggested_next
            .iter()
            .any(|next| next.pattern_type.as_deref() == Some("regex")),
        "regex trap should suggest pattern_type=regex: {:?}",
        regex_trap.recovery.suggested_next
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_hybrid_compact_includes_discovery_pivots() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-hybrid-pivots");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "where is greeting defined".to_owned(),
            repository_id: None,
            language: None,
            limit: Some(10),
            weights: None,
            semantic: Some(false),
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("search_hybrid should succeed")
        .0;

    assert_eq!(
        response.ranking_note.as_deref(),
        Some("discovery_only; lexical_only (semantic not contributing); confirm with exact search"),
        "semantic:false → lexical_only ranking_note (mode cliff, not readiness dump)"
    );
    assert!(
        !response.recovery.suggested_next.is_empty() || response.ranking_note.is_some(),
        "hybrid compact must surface pivots on empty or non-empty: {:?}",
        response.recovery
    );
    if !response.matches.is_empty() {
        assert!(
            response.best_pivot_path.is_some() || !response.recovery.suggested_next.is_empty(),
            "non-empty hybrid should expose best_pivot_path or suggested_next"
        );
    }

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_hybrid_compact_and_full_keep_canonical_actions_identical() {
    let workspace_root = fresh_fixture_root("tool-handlers-search-hybrid-action-parity");
    let server = server_for_workspace_root(&workspace_root).await;
    let params = SearchHybridParams {
        query: "where is greeting defined".to_owned(),
        repository_id: None,
        language: None,
        limit: Some(10),
        weights: None,
        semantic: Some(false),
        response_mode: Some(ResponseMode::Compact),
        include_context_efficiency: None,
    };
    let compact = server
        .search_hybrid(Parameters(params.clone()))
        .await
        .expect("compact search_hybrid should succeed")
        .0;
    let full = server
        .search_hybrid(Parameters(SearchHybridParams {
            response_mode: Some(ResponseMode::Full),
            ..params
        }))
        .await
        .expect("full search_hybrid should succeed")
        .0;

    assert_eq!(
        serde_json::to_value(&compact.recovery.next_actions)
            .expect("compact canonical actions serialize"),
        serde_json::to_value(&full.recovery.next_actions)
            .expect("full canonical actions serialize"),
        "response detail must not change executable action ids, roles, targets, or dependencies"
    );
    assert_eq!(
        compact.recovery.suggested_next,
        compact
            .recovery
            .next_actions
            .iter()
            .map(|action| action.to_legacy_suggestion())
            .collect::<Vec<_>>(),
        "compact legacy suggestions are generated from canonical actions"
    );
    assert_eq!(
        full.recovery.suggested_next,
        full.recovery
            .next_actions
            .iter()
            .map(|action| action.to_legacy_suggestion())
            .collect::<Vec<_>>(),
        "full legacy suggestions are generated from canonical actions"
    );

    cleanup_workspace_root(&workspace_root);
}
