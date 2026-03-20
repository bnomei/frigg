use super::*;

#[test]
fn literal_search_ripgrep_backend_filters_hits_to_manifest_candidate_universe() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("literal-search-ripgrep-manifest");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/out_scope.rs", "needle out\n"),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;
    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/indexed.rs"},"lines":{"text":"needle indexed\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle"},"start":0,"end":6}]}}
{"type":"match","data":{"path":{"text":"src/out_scope.rs"},"lines":{"text":"needle out\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle"},"start":0,"end":6}]}}"#,
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let searcher = TextSearcher::new(config);

    let output = searcher.search_literal_with_filters_diagnostics(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(output.lexical_backend, Some(SearchLexicalBackend::Ripgrep));
    assert_eq!(
        output.matches,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_falls_back_to_native_when_ripgrep_is_unavailable() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("literal-search-ripgrep-fallback");
    prepare_workspace(&root, &[("src/indexed.rs", "needle indexed\n")])?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(root.join("missing-rg-executable"));
    let searcher = TextSearcher::new(config);

    let output = searcher.search_literal_with_filters_diagnostics(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(output.lexical_backend, Some(SearchLexicalBackend::Native));
    assert!(
        output
            .lexical_backend_note
            .as_deref()
            .is_some_and(|note| note.contains("ripgrep unavailable")),
        "expected native fallback note, got {:?}",
        output.lexical_backend_note
    );
    assert_eq!(
        output.matches,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_search_reports_ripgrep_backend_in_lexical_only_mode() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("hybrid-search-ripgrep-backend");
    prepare_workspace(&root, &[("src/runtime.rs", "pub fn needle_handler() {}\n")])?;
    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/runtime.rs"},"lines":{"text":"pub fn needle_handler() {}\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle_handler"},"start":7,"end":21}]}}"#,
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let searcher = TextSearcher::new(config);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle handler".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(matches!(
        output.note.lexical_backend,
        Some(SearchLexicalBackend::Ripgrep | SearchLexicalBackend::Mixed)
    ));
    assert!(
        output
            .note
            .lexical_backend_note
            .as_deref()
            .is_some_and(|note| note.contains("ripgrep accelerator active"))
    );
    assert_eq!(output.matches.len(), 1);
    assert_eq!(output.matches[0].document.path, "src/runtime.rs");

    cleanup_workspace(&root);
    Ok(())
}
