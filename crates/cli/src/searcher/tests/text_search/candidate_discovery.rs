use super::*;

#[test]
fn candidate_discovery_prefers_manifest_snapshot_across_search_modes() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-prefers-manifest");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/live_only.rs", "needle live-only\n"),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);

    let literal = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        literal,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    let regex = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle\s+\w+".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        regex,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    let hybrid = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 20,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(hybrid.matches.len(), 1);
    assert_eq!(hybrid.matches[0].document.path, "src/indexed.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn candidate_discovery_manifest_snapshot_respects_root_ignore_file() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-manifest-ignore");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("auxiliary/embedded-repo/src/lib.rs", "needle auxiliary\n"),
        ],
    )?;
    fs::write(root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["src/indexed.rs", "auxiliary/embedded-repo/src/lib.rs"],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);

    let literal = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        literal,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    let hybrid = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 20,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(hybrid.matches.len(), 1);
    assert_eq!(hybrid.matches[0].document.path, "src/indexed.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn candidate_discovery_rebuilds_after_stale_manifest_snapshot() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-stale-manifest");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/live_only.rs", "needle live-only\n"),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);

    let first = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        first,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    rewrite_file_with_new_mtime(&root.join("src/indexed.rs"), "changed\n")?;

    let literal = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        literal,
        vec![text_match(
            "repo-001",
            "src/live_only.rs",
            1,
            1,
            "needle live-only"
        )]
    );

    let hybrid = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 20,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(hybrid.matches.len(), 1);
    assert_eq!(hybrid.matches[0].document.path, "src/live_only.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn candidate_discovery_falls_back_to_repository_walk_without_manifest() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-fallback-walk");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/live_only.rs", "needle live-only\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let matches = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(
        matches,
        vec![
            text_match("repo-001", "src/indexed.rs", 1, 1, "needle indexed"),
            text_match("repo-001", "src/live_only.rs", 1, 1, "needle live-only"),
        ]
    );

    cleanup_workspace(&root);
    Ok(())
}
