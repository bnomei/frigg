use super::*;

#[test]
fn hybrid_ranking_semantic_channel_surfaces_docs_runtime_and_tests_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-doc-runtime-tests");
    prepare_workspace(
        &root,
        &[
            (
                "contracts/errors.md",
                "invalid_params typed error public docs contract\n",
            ),
            (
                "crates/cli/src/mcp/server.rs",
                "invalid_params runtime helper\n",
            ),
            (
                "crates/cli/tests/tool_handlers.rs",
                "invalid_params tests coverage\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "contracts/errors.md",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "crates/cli/src/mcp/server.rs",
                0,
                vec![0.95, 0.05],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "crates/cli/tests/tool_handlers.rs",
                0,
                vec![0.90, 0.10],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "trace invalid_params typed error from public docs to runtime helper and tests"
                .to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;
    let paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert!(paths.contains(&"contracts/errors.md"));
    assert!(paths.contains(&"crates/cli/src/mcp/server.rs"));
    assert!(paths.contains(&"crates/cli/tests/tool_handlers.rs"));

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_ok_still_expands_lexical_recall_for_underfilled_queries()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-ok-lexical-recall");
    prepare_workspace(
        &root,
        &[
            (
                "contracts/tools/v1/README.md",
                "tool surface profile core extended_only tools/list contract\n",
            ),
            (
                "crates/cli/src/mcp/tool_surface.rs",
                "ToolSurfaceProfile::Core ToolSurfaceProfile::Extended runtime gating\n",
            ),
            (
                "crates/cli/tests/tool_surface_parity.rs",
                "runtime_tool_surface_parity tests\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "contracts/tools/v1/README.md",
                0,
                vec![0.0, 1.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "crates/cli/src/mcp/tool_surface.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "crates/cli/tests/tool_surface_parity.rs",
                0,
                vec![0.95, 0.05],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests"
                        .to_owned(),
                limit: 4,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
    let paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert_eq!(output.note.semantic_candidate_count, 3);
    assert_eq!(output.note.semantic_hit_count, 2);
    assert!(output.note.semantic_match_count >= 2);
    assert!(output.note.semantic_enabled);
    assert!(paths.contains(&"crates/cli/src/mcp/tool_surface.rs"));
    assert!(paths.contains(&"crates/cli/tests/tool_surface_parity.rs"));
    assert!(
        paths.contains(&"contracts/tools/v1/README.md"),
        "underfilled natural-language queries should still pull in tool-contract docs via lexical expansion when semantic retrieval is healthy; got {paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_hit_count_tracks_retained_documents_not_raw_chunks() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-retained-hit-count");
    prepare_workspace(
        &root,
        &[
            (
                "src/relevant.rs",
                "pub fn relevant() { let _ = \"needle\"; }\n",
            ),
            (
                "src/secondary.rs",
                "pub fn secondary() { let _ = \"needle\"; }\n",
            ),
            ("src/noisy.rs", "pub fn noisy() { let _ = \"needle\"; }\n"),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/relevant.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/relevant.rs",
                1,
                vec![0.82, 0.02],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/secondary.rs",
                0,
                vec![0.69, 0.72],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/noisy.rs",
                0,
                vec![0.41, 0.91],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        },
        &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert_eq!(
        output.note.semantic_candidate_count, 4,
        "semantic_candidate_count should expose the broader raw semantic chunk pool"
    );
    assert_eq!(
        output.note.semantic_hit_count, 1,
        "semantic_hit_count should reflect retained semantic documents, not raw chunk count"
    );
    assert_eq!(output.matches[0].document.path, "src/relevant.rs");
    assert!(output.note.semantic_match_count >= 1);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_unavailable_without_corpus_still_expands_lexical_recall()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-unavailable-lexical-recall");
    prepare_workspace(
        &root,
        &[
            (
                "src/config.rs",
                "pub fn resolve_config_path() {\n\
                     let precedence = \"cli then env then file\";\n\
                     }\n",
            ),
            (
                "src/main.rs",
                "pub fn load_config() {\n\
                     let config_loaded = true;\n\
                     }\n",
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "Where is config loaded and what is the precedence?".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;
    let paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        output.note.semantic_status,
        HybridSemanticStatus::Unavailable
    );
    assert_eq!(output.note.semantic_hit_count, 0);
    assert_eq!(output.note.semantic_match_count, 0);
    assert!(!output.note.semantic_enabled);
    assert!(
        output
            .note
            .semantic_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no semantic storage database")),
        "unavailable note should explain that no semantic storage database exists"
    );
    assert!(
        !output.matches.is_empty(),
        "semantic-unavailable hybrid search should still recover lexical matches when the semantic channel cannot run against a corpus"
    );
    assert!(paths.contains(&"src/config.rs"));
    assert!(paths.contains(&"src/main.rs"));

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_ok_empty_channel_when_active_index_is_filtered_out() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-ok-filtered-empty-channel");
    prepare_workspace(
        &root,
        &[("src/lib.rs", "pub fn rust_only() { let _ = \"needle\"; }\n")],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[semantic_record(
            "repo-001",
            "snapshot-001",
            "src/lib.rs",
            0,
            vec![1.0, 0.0],
        )],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters {
            repository_id: None,
            language: Some("php".to_owned()),
        },
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert_eq!(output.note.semantic_hit_count, 0);
    assert_eq!(output.note.semantic_match_count, 0);
    assert!(!output.note.semantic_enabled);
    assert!(output.note.semantic_reason.is_none());
    assert!(
        output.matches.is_empty(),
        "language-filtered semantic search should be allowed to return an empty but healthy result set"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_reports_unsupported_language_filter_as_unavailable() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-unsupported-language-filter");
    prepare_workspace(
        &root,
        &[("src/lib.rs", "pub fn rust_only() { let _ = \"needle\"; }\n")],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[semantic_record(
            "repo-001",
            "snapshot-001",
            "src/lib.rs",
            0,
            vec![1.0, 0.0],
        )],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters {
            repository_id: None,
            language: Some("typescript".to_owned()),
        },
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(
        output.note.semantic_status,
        HybridSemanticStatus::Unavailable
    );
    assert!(!output.note.semantic_enabled);
    assert_eq!(output.note.semantic_hit_count, 0);
    assert_eq!(output.note.semantic_match_count, 0);
    assert_eq!(
        output.note.semantic_reason.as_deref(),
        Some("requested language filter 'typescript' does not support semantic_chunking")
    );
    assert!(
        output.matches.is_empty(),
        "unsupported semantic language filters should skip semantic retrieval and keep the result set bounded"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_channel_falls_back_to_older_snapshot_when_latest_manifest_lacks_embeddings()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-fallback-split-snapshot");
    prepare_workspace(
        &root,
        &[
            (
                "src/current.rs",
                "pub fn current() { let _ = \"semantic needle\"; }\n",
            ),
            (
                "src/deleted.rs",
                "pub fn deleted() { let _ = \"semantic needle\"; }\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/current.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/deleted.rs",
                0,
                vec![0.95, 0.05],
            ),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-002", &["src/current.rs"])?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "semantic needle".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;
    let paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        output.note.semantic_status,
        HybridSemanticStatus::Unavailable
    );
    assert!(!output.note.semantic_enabled);
    assert!(
        output
            .note
            .semantic_reason
            .as_deref()
            .is_some_and(|reason| {
                reason.contains("snapshot-002") && reason.contains("no live semantic embeddings")
            }),
        "missing live semantic corpus should name the latest manifest snapshot"
    );
    assert!(
        paths.contains(&"src/current.rs"),
        "current manifest path should remain visible through lexical recovery when semantic is unavailable: {paths:?}"
    );
    assert!(
        !paths.contains(&"src/deleted.rs"),
        "paths removed from the latest manifest must not resurface when semantic storage is unavailable: {paths:?}"
    );
    assert!(
        output
            .matches
            .iter()
            .all(|entry| entry.semantic_score == 0.0),
        "semantic-unavailable recovery should not report retained semantic scores"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_disabled_expands_lexical_recall_for_multi_token_queries()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-disabled-lexical-recall");
    prepare_workspace(
        &root,
        &[
            (
                "playbooks/hybrid-search-context-retrieval.md",
                "semantic runtime strict failure note metadata\n",
            ),
            (
                "src/lib.rs",
                "pub fn strict_failure_note() {\n\
                     let semantic_status = \"strict_failure\";\n\
                     let semantic_reason = \"runtime metadata\";\n\
                     }\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "semantic runtime strict failure note metadata".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(
        output
            .matches
            .iter()
            .any(|entry| entry.document.path == "src/lib.rs"),
        "tokenized lexical recall should include source evidence even when phrase-literal match is doc-only"
    );
    assert_eq!(
        output.matches[0].document.path, "src/lib.rs",
        "source evidence should outrank playbook self-reference in lexical-only fallback mode"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_disabled_literal_floor_recovers_snake_case_only_matches()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-disabled-literal-floor");
    prepare_workspace(
        &root,
        &[(
            "src/lib.rs",
            "pub fn strict_failure_note() {\n\
                 let semantic_status = \"strict_failure\";\n\
                 }\n",
        )],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "strict semantic failure".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(
        !output.matches.is_empty(),
        "token literal floor should avoid empty degraded hybrid responses"
    );
    assert_eq!(output.matches[0].document.path, "src/lib.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_large_top_k_laravel_witness_queries_do_not_panic() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-large-topk-laravel-witness");
    prepare_workspace(
        &root,
        &[
            ("tests/CreatesApplication.php", "<?php\n"),
            ("tests/DuskTestCase.php", "<?php\n"),
            (
                "resources/views/auth/confirm-password.blade.php",
                "<div>confirm password</div>\n",
            ),
            (
                "resources/views/components/applications/advanced.blade.php",
                "<div>advanced</div>\n",
            ),
            ("app/Livewire/ActivityMonitor.php", "<?php\n"),
            ("app/Livewire/Dashboard.php", "<?php\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration creates application dusk case resources views auth confirm auth forgot view components app livewire activity monitor dashboard".to_owned(),
                limit: 200,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(
        !output.matches.is_empty(),
        "large lexical top-k witness queries should still return results instead of panicking"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_blends_lexical_graph_and_semantic_channels() -> FriggResult<()> {
    let lexical = vec![
        hybrid_hit("repo-001", "src/a.rs", 10.0, "lex-a"),
        hybrid_hit("repo-001", "src/b.rs", 8.0, "lex-b"),
    ];
    let graph = vec![
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::GraphPrecise,
            "repo-001",
            "src/b.rs",
            5.0,
            "graph-b",
        ),
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::GraphPrecise,
            "repo-001",
            "src/c.rs",
            4.0,
            "graph-c",
        ),
    ];
    let semantic = vec![
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::Semantic,
            "repo-001",
            "src/c.rs",
            0.9,
            "sem-c",
        ),
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::Semantic,
            "repo-001",
            "src/a.rs",
            0.2,
            "sem-a",
        ),
    ];

    let ranked = rank_hybrid_evidence(
        &lexical,
        &graph,
        &semantic,
        HybridChannelWeights::default(),
        10,
    )?;
    assert_eq!(ranked.len(), 3);
    assert_eq!(ranked[0].document.path, "src/b.rs");
    assert_eq!(ranked[1].document.path, "src/a.rs");
    assert_eq!(ranked[2].document.path, "src/c.rs");
    assert_eq!(ranked[0].lexical_sources, vec!["lex-b".to_owned()]);
    assert_eq!(ranked[0].graph_sources, vec!["graph-b".to_owned()]);
    assert_eq!(ranked[2].semantic_sources, vec!["sem-c".to_owned()]);

    Ok(())
}

#[test]
fn graph_channel_falls_back_to_exact_stem_candidates_when_lexical_paths_have_no_symbols()
-> FriggResult<()> {
    let root = temp_workspace_root("graph-channel-fallback-exact-stem");
    prepare_workspace(
        &root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrderHandler handle listener overview.\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let normalized_filters = normalize_search_filters(SearchFilters::default())?;
    let candidate_universe = searcher.build_candidate_universe(
        &SearchTextQuery {
            query: String::new(),
            path_regex: None,
            limit: 5,
        },
        &normalized_filters,
    );
    let hits = super::graph_channel::search_graph_channel_hits(
        &searcher,
        "OrderHandler handle listener",
        &candidate_universe,
        &[TextMatch {
            repository_id: "repo-001".to_owned(),
            path: "docs/handlers.md".to_owned(),
            line: 1,
            column: 1,
            excerpt: "OrderHandler handle listener overview".to_owned(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        }],
        5,
    )?;

    assert!(
        hits.iter()
            .any(|hit| hit.document.path == "src/Handlers/OrderHandler.php"),
        "graph fallback should recover the handler anchor from exact-stem candidates: {hits:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_graph_queries_reuse_snapshot_scoped_graph_artifacts() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-graph-artifact-cache-reuse");
    prepare_workspace(
        &root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder handler listener overview.\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "src/Handlers/OrderHandler.php",
            "src/Listeners/OrderListener.php",
            "docs/handlers.md",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    assert_eq!(
        searcher
            .hybrid_graph_artifact_cache
            .read()
            .expect("hybrid graph artifact cache should not be poisoned")
            .len(),
        0
    );

    let first = searcher.search_hybrid(SearchHybridQuery {
        query: "OrderHandler handle listener".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;
    assert!(
        first
            .matches
            .iter()
            .any(|entry| entry.document.path == "src/Listeners/OrderListener.php"),
        "initial graph query should surface listener evidence: {:?}",
        first.matches
    );
    assert_eq!(
        searcher
            .hybrid_graph_artifact_cache
            .read()
            .expect("hybrid graph artifact cache should not be poisoned")
            .len(),
        1
    );

    let second = searcher.search_hybrid(SearchHybridQuery {
        query: "OrderHandler handle listener".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;
    assert_eq!(first.matches, second.matches);
    assert_eq!(
        searcher
            .hybrid_graph_artifact_cache
            .read()
            .expect("hybrid graph artifact cache should not be poisoned")
            .len(),
        1
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_graph_artifact_cache_rebuilds_after_snapshot_change() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-graph-artifact-cache-snapshot-change");
    prepare_workspace(
        &root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder handler listener overview.\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "src/Handlers/OrderHandler.php",
            "src/Listeners/OrderListener.php",
            "docs/handlers.md",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let first = searcher.search_hybrid(SearchHybridQuery {
        query: "OrderHandler handle listener".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;
    assert!(
        first
            .matches
            .iter()
            .any(|entry| entry.document.path == "src/Listeners/OrderListener.php"),
        "baseline graph query should surface listener evidence: {:?}",
        first.matches
    );
    assert_eq!(
        searcher
            .hybrid_graph_artifact_cache
            .read()
            .expect("hybrid graph artifact cache should not be poisoned")
            .len(),
        1
    );

    prepare_workspace(
        &root,
        &[
            (
                "src/Handlers/PaymentHandler.php",
                "<?php\n\
                     namespace App\\Handlers;\n\
                     class PaymentHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
            ),
            (
                "src/Listeners/PaymentListener.php",
                "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\PaymentHandler;\n\
                     class PaymentListener {\n\
                         public function handlers(): array {\n\
                             return [[PaymentHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-002",
        &[
            "src/Handlers/OrderHandler.php",
            "src/Listeners/OrderListener.php",
            "src/Handlers/PaymentHandler.php",
            "src/Listeners/PaymentListener.php",
            "docs/handlers.md",
        ],
    )?;

    let second = searcher.search_hybrid(SearchHybridQuery {
        query: "PaymentHandler handle listener".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;
    let payment_listener = second
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/Listeners/PaymentListener.php")
        .expect("snapshot change should rebuild graph artifact for the new payment listener");
    assert!(
        payment_listener.graph_score > 0.0,
        "rebuilt graph artifact should contribute graph evidence for new snapshot content: {:?}",
        second.matches
    );
    assert_eq!(
        searcher
            .hybrid_graph_artifact_cache
            .read()
            .expect("hybrid graph artifact cache should not be poisoned")
            .len(),
        1
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_graph_channel_seeds_from_canonical_runtime_paths_without_exact_symbol_terms()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-graph-canonical-path-seed");
    prepare_workspace(
        &root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder listener wiring overview.\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid(SearchHybridQuery {
        query: "order listener wiring".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;
    let handler = output
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/Handlers/OrderHandler.php")
        .expect("canonical path-seeded graph search should surface the handler runtime file");

    assert!(
        handler.graph_score > 0.0,
        "graph channel should activate from canonical runtime path seeds even without exact symbol terms: {:?}",
        output.matches
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_respects_configured_channel_weights() -> FriggResult<()> {
    let lexical = vec![
        hybrid_hit("repo-001", "src/a.rs", 10.0, "lex-a"),
        hybrid_hit("repo-001", "src/b.rs", 8.0, "lex-b"),
    ];
    let graph = vec![
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::GraphPrecise,
            "repo-001",
            "src/b.rs",
            5.0,
            "graph-b",
        ),
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::GraphPrecise,
            "repo-001",
            "src/c.rs",
            4.0,
            "graph-c",
        ),
    ];
    let semantic = vec![
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::Semantic,
            "repo-001",
            "src/c.rs",
            0.9,
            "sem-c",
        ),
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::Semantic,
            "repo-001",
            "src/a.rs",
            0.2,
            "sem-a",
        ),
    ];
    let weights = HybridChannelWeights {
        lexical: 0.2,
        graph: 0.2,
        semantic: 0.6,
    };

    let ranked = rank_hybrid_evidence(&lexical, &graph, &semantic, weights, 10)?;
    assert_eq!(ranked.len(), 3);
    assert_eq!(ranked[0].document.path, "src/c.rs");
    assert_eq!(ranked[1].document.path, "src/b.rs");
    assert_eq!(ranked[2].document.path, "src/a.rs");

    Ok(())
}

#[test]
fn hybrid_ranking_is_deterministic_under_tied_scores() -> FriggResult<()> {
    let lexical = vec![
        hybrid_hit("repo-001", "src/b.rs", 1.0, "lex-b"),
        hybrid_hit("repo-001", "src/a.rs", 1.0, "lex-a"),
    ];
    let graph = vec![hybrid_hit_with_channel(
        crate::domain::EvidenceChannel::GraphPrecise,
        "repo-001",
        "src/c.rs",
        1.0,
        "graph-c",
    )];
    let semantic = vec![hybrid_hit_with_channel(
        crate::domain::EvidenceChannel::Semantic,
        "repo-001",
        "src/c.rs",
        1.0,
        "sem-c",
    )];

    let first = rank_hybrid_evidence(
        &lexical,
        &graph,
        &semantic,
        HybridChannelWeights::default(),
        10,
    )?;
    let reversed_lexical = lexical.into_iter().rev().collect::<Vec<_>>();
    let second = rank_hybrid_evidence(
        &reversed_lexical,
        &graph,
        &semantic,
        HybridChannelWeights::default(),
        10,
    )?;

    assert_eq!(first, second);
    assert_eq!(first[0].document.path, "src/a.rs");
    assert_eq!(first[1].document.path, "src/b.rs");
    assert_eq!(first[2].document.path, "src/c.rs");

    Ok(())
}

#[test]
fn hybrid_ranking_semantic_channel_blends_retrieval_when_enabled() -> FriggResult<()> {
    let (searcher, root) =
        semantic_hybrid_fixture("hybrid-semantic-enabled", semantic_runtime_enabled(false))?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![0.0, 1.0]);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert!(output.note.semantic_hit_count > 0);
    assert!(output.note.semantic_match_count > 0);
    assert!(output.note.semantic_enabled);
    assert!(output.note.semantic_reason.is_none());
    assert!(
        output.matches.len() >= 2,
        "expected at least two hybrid matches from lexical + semantic fixture"
    );
    assert_eq!(
        output.matches[0].document.path, "src/z.rs",
        "semantic similarity should promote src/z.rs above lexical tie ordering"
    );
    assert!(
        output.matches[0].semantic_score > output.matches[1].semantic_score,
        "top-ranked semantic score should be strictly greater for promoted path"
    );
    assert!(
        output.matches[0]
            .semantic_sources
            .iter()
            .any(|source| source.starts_with("chunk-src_z.rs")),
        "semantic sources should include deterministic chunk provenance ids"
    );
    let semantic_match = output
        .matches
        .iter()
        .find(|matched| matched.document.path == "src/z.rs")
        .expect("semantic-promoted match should be present");
    assert_eq!(semantic_match.document.line, 2);
    assert_eq!(semantic_match.anchor.start_line, 2);
    assert_eq!(semantic_match.anchor.end_line, 2);
    let semantic_channel = output
        .channel_results
        .iter()
        .find(|result| result.channel == crate::domain::EvidenceChannel::Semantic)
        .expect("semantic channel result should be present");
    let semantic_hit = semantic_channel
        .hits
        .iter()
        .find(|hit| hit.document.path == "src/z.rs")
        .expect("semantic hit for src/z.rs should be present");
    assert_eq!(semantic_hit.anchor.start_line, 2);
    assert_eq!(semantic_hit.anchor.end_line, 3);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_channel_ignores_excluded_paths_and_non_active_models() -> FriggResult<()>
{
    let root = temp_workspace_root("hybrid-semantic-filtered");
    prepare_workspace(
        &root,
        &[
            ("src/current.rs", "pub fn current() {}\n"),
            ("src/legacy.rs", "pub fn legacy() {}\n"),
            ("target/debug/app.rs", "pub fn target_artifact() {}\n"),
        ],
    )?;
    let mut legacy = semantic_record(
        "repo-001",
        "snapshot-001",
        "src/legacy.rs",
        0,
        vec![1.0, 0.0],
    );
    legacy.provider = "google".to_owned();
    legacy.model = "gemini-embedding-001".to_owned();
    let target = semantic_record(
        "repo-001",
        "snapshot-001",
        "target/debug/app.rs",
        0,
        vec![1.0, 0.0],
    );
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/current.rs",
                0,
                vec![1.0, 0.0],
            ),
            legacy,
            target,
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "current symbol".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: Some("test-gemini-key".to_owned()),
        },
        &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
    )?;

    let paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        paths.contains(&"src/current.rs"),
        "active-model semantic path should remain visible: {paths:?}"
    );
    assert!(
        !paths.contains(&"src/legacy.rs"),
        "rows for other provider/model combinations must be ignored: {paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.starts_with("target/")),
        "excluded runtime paths must not surface from semantic storage: {paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_channel_can_be_disabled_per_query_toggle() -> FriggResult<()> {
    let (searcher, root) = semantic_hybrid_fixture(
        "hybrid-semantic-toggle-off",
        semantic_runtime_enabled(false),
    )?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = PanicSemanticQueryEmbeddingExecutor;

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(output.note.semantic_hit_count, 0);
    assert_eq!(output.note.semantic_match_count, 0);
    assert!(!output.note.semantic_enabled);
    assert_eq!(
        output.note.semantic_reason.as_deref(),
        Some("semantic channel disabled by request toggle")
    );
    assert!(
        output
            .matches
            .iter()
            .all(|evidence| evidence.semantic_score == 0.0),
        "semantic channel scores should be zero when semantic toggle is disabled"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_channel_degrades_on_provider_failure_non_strict() -> FriggResult<()> {
    let (searcher, root) =
        semantic_hybrid_fixture("hybrid-semantic-degraded", semantic_runtime_enabled(false))?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor =
        MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Degraded);
    assert_eq!(output.note.semantic_hit_count, 0);
    assert_eq!(output.note.semantic_match_count, 0);
    assert!(!output.note.semantic_enabled);
    assert!(
        output
            .note
            .semantic_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("mock semantic provider unavailable")),
        "degraded note should include deterministic provider failure reason"
    );
    assert!(
        output
            .matches
            .iter()
            .all(|entry| entry.semantic_score == 0.0),
        "semantic scores should be zero when semantic channel degrades"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_channel_strict_mode_surfaces_strict_failure() -> FriggResult<()> {
    let (searcher, root) = semantic_hybrid_fixture(
        "hybrid-semantic-strict-failure",
        semantic_runtime_enabled(true),
    )?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor =
        MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

    let err = searcher
        .search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )
        .expect_err("strict semantic mode should fail on semantic provider errors");
    let err_message = err.to_string();
    assert!(
        err_message.contains("semantic_status=strict_failure"),
        "strict mode failure should carry deterministic strict status metadata: {err_message}"
    );
    assert!(
        err_message.contains("mock semantic provider unavailable"),
        "strict mode failure should include semantic channel failure reason: {err_message}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_enabled_replay_is_deterministic() -> FriggResult<()> {
    let (searcher, root) = semantic_hybrid_fixture(
        "hybrid-semantic-enabled-deterministic-replay",
        semantic_runtime_enabled(false),
    )?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![0.0, 1.0]);

    let first = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;
    let second = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.note, second.note);
    assert_eq!(first.diagnostics, second.diagnostics);
    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Ok);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_degraded_replay_is_deterministic() -> FriggResult<()> {
    let (searcher, root) = semantic_hybrid_fixture(
        "hybrid-semantic-degraded-deterministic-replay",
        semantic_runtime_enabled(false),
    )?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor =
        MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

    let first = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;
    let second = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.note, second.note);
    assert_eq!(first.diagnostics, second.diagnostics);
    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Degraded);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_strict_failure_replay_is_deterministic() -> FriggResult<()> {
    let (searcher, root) = semantic_hybrid_fixture(
        "hybrid-semantic-strict-deterministic-replay",
        semantic_runtime_enabled(true),
    )?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor =
        MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

    let first = searcher
        .search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )
        .expect_err("strict semantic mode should fail deterministically");
    let second = searcher
        .search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )
        .expect_err("strict semantic mode should fail deterministically");

    assert_eq!(first.to_string(), second.to_string());
    assert!(first.to_string().contains("semantic_status=strict_failure"));

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_search_semantic_query_embedding_works_inside_existing_tokio_runtime() -> FriggResult<()> {
    let (searcher, root) = semantic_hybrid_fixture(
        "hybrid-semantic-inside-current-runtime",
        semantic_runtime_enabled(false),
    )?;
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");

    let output = runtime.block_on(async {
        searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )
    })?;

    assert!(
        !output.matches.is_empty(),
        "hybrid search inside an existing runtime should still return matches"
    );
    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);

    cleanup_workspace(&root);
    Ok(())
}
