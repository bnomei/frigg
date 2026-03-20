use super::*;

fn match_fixture(
    path: &str,
    source_class: Option<SourceClass>,
    surface_families: &[&str],
    pivotable: bool,
    document_symbols: bool,
    go_to_definition: bool,
) -> SearchHybridMatch {
    SearchHybridMatch {
        repository_id: "repo-001".to_string(),
        path: path.to_string(),
        line: 1,
        column: 1,
        excerpt: "fixture".to_string(),
        anchor: None,
        blended_score: 1.0,
        lexical_score: 1.0,
        graph_score: 0.0,
        semantic_score: 0.0,
        lexical_sources: vec![],
        graph_sources: vec![],
        semantic_sources: vec![],
        path_class: None,
        source_class,
        surface_families: surface_families
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        navigation_hint: Some(SearchHybridNavigationHint {
            pivotable,
            document_symbols,
            go_to_definition,
        }),
    }
}

#[test]
fn search_hybrid_utility_summary_prefers_runtime_source_pivot() {
    let matches = vec![
        match_fixture(
            "README.md",
            Some(SourceClass::Project),
            &["docs"],
            false,
            false,
            false,
        ),
        match_fixture(
            "tests/runtime_test.rs",
            Some(SourceClass::Tests),
            &["tests"],
            true,
            true,
            false,
        ),
        match_fixture(
            "src/runtime/server.rs",
            Some(SourceClass::Runtime),
            &["runtime"],
            true,
            true,
            true,
        ),
    ];

    let summary = FriggMcpServer::search_hybrid_utility_summary(&matches);
    assert_eq!(summary.pivotable_match_count, 2);
    assert_eq!(summary.best_pivot_rank, Some(3));
    assert_eq!(
        summary.best_pivot_path.as_deref(),
        Some("src/runtime/server.rs")
    );
    assert!(summary.symbol_navigation_ready);
}

#[test]
fn search_hybrid_utility_summary_reports_miss_without_pivotable_matches() {
    let matches = vec![
        match_fixture(
            "guides/overview.md",
            Some(SourceClass::Project),
            &["docs"],
            false,
            false,
            false,
        ),
        match_fixture(
            "package.json",
            Some(SourceClass::Project),
            &["package_surface"],
            false,
            false,
            false,
        ),
    ];

    let summary = FriggMcpServer::search_hybrid_utility_summary(&matches);
    assert_eq!(summary.pivotable_match_count, 0);
    assert_eq!(summary.best_pivot_rank, None);
    assert_eq!(summary.best_pivot_path, None);
    assert!(!summary.symbol_navigation_ready);
}

#[test]
fn search_text_metadata_maps_ripgrep_backend() {
    let metadata = FriggMcpServer::search_text_metadata(
        Some(SearchLexicalBackend::Ripgrep),
        Some("ripgrep accelerator active".to_owned()),
    )
    .expect("metadata should exist");
    assert_eq!(
        metadata.lexical_backend,
        SearchLexicalBackendMetadata::Ripgrep
    );
    assert_eq!(
        metadata.lexical_backend_note.as_deref(),
        Some("ripgrep accelerator active")
    );
}

#[test]
fn search_text_metadata_returns_none_without_backend() {
    assert!(FriggMcpServer::search_text_metadata(None, None).is_none());
}
