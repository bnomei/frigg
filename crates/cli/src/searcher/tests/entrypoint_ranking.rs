use super::*;

#[test]
fn hybrid_lexical_recall_regex_is_deterministic_for_multi_term_queries() {
    let pattern =
        build_hybrid_lexical_recall_regex("semantic runtime strict failure note metadata")
            .expect("multi-token query should emit lexical recall regex");
    assert_eq!(
        pattern,
        r"(?i)\b(?:semantic|runtime|strict|failure|note|metadata)\b"
    );

    assert!(
        build_hybrid_lexical_recall_regex("abc xyz").is_none(),
        "short tokens should not enable lexical recall expansion"
    );
}

#[test]
fn hybrid_lexical_recall_tokens_support_snake_case_terms() {
    assert_eq!(
        hybrid_lexical_recall_tokens("strict semantic failure unavailable semantic_status"),
        vec![
            "strict".to_owned(),
            "semantic".to_owned(),
            "failure".to_owned(),
            "unavailable".to_owned(),
            "semantic_status".to_owned(),
        ]
    );
}

#[test]
fn hybrid_ranking_lexical_hits_prefer_source_paths_over_playbooks() -> FriggResult<()> {
    let lexical = build_hybrid_lexical_hits(&[
        text_match(
            "repo-001",
            "playbooks/hybrid.md",
            1,
            1,
            "semantic runtime metadata",
        ),
        text_match("repo-001", "src/lib.rs", 1, 1, "semantic runtime metadata"),
    ]);
    let ranked = rank_hybrid_evidence(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        10,
    )?;

    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].document.path, "src/lib.rs");
    assert_eq!(ranked[1].document.path, "playbooks/hybrid.md");
    Ok(())
}

#[test]
fn hybrid_ranking_query_aware_lexical_hits_keep_public_docs_visible_with_runtime_and_tests()
-> FriggResult<()> {
    let query = "trace invalid_params typed error from public docs to runtime helper and tests";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "contracts/errors.md",
                1,
                1,
                "invalid_params maps to -32602",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/mcp/server.rs",
                1,
                1,
                "fn invalid_params_error() -> JsonRpcError",
            ),
            text_match(
                "repo-001",
                "crates/cli/tests/tool_handlers.rs",
                1,
                1,
                "invalid_params typed failure coverage",
            ),
        ],
        query,
    );
    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        3,
        query,
    )?;

    assert_eq!(ranked.len(), 3);
    assert!(
        ranked
            .iter()
            .any(|entry| entry.document.path == "contracts/errors.md"),
        "public docs witness should remain in the ranked set"
    );
    assert!(
        ranked
            .iter()
            .any(|entry| entry.document.path == "crates/cli/src/mcp/server.rs"),
        "runtime witness should remain in the ranked set"
    );
    assert!(
        ranked
            .iter()
            .any(|entry| entry.document.path == "crates/cli/tests/tool_handlers.rs"),
        "test witness should remain in the ranked set"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_http_auth_queries_demote_repo_metadata_noise() -> FriggResult<()> {
    let query = "where is the optional HTTP MCP auth token declared enforced and documented";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "Cargo.lock",
                1,
                1,
                "source = \"registry+https://github.com/rust-lang/crates.io-index\"",
            ),
            text_match(
                "repo-001",
                "README.md",
                1,
                1,
                "POST /mcp --mcp-http-auth-token FRIGG_MCP_HTTP_AUTH_TOKEN",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/main.rs",
                1,
                1,
                "mcp_http_auth_token bearer_auth_middleware serve_http",
            ),
        ],
        query,
    );
    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        3,
        query,
    )?;

    assert_eq!(ranked[0].document.path, "crates/cli/src/main.rs");
    assert_eq!(ranked[1].document.path, "README.md");
    assert_eq!(ranked[2].document.path, "Cargo.lock");
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_auth_queries_keep_runtime_and_readme_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-semantic-auth-runtime-readme");
    prepare_workspace(
        &root,
        &[
            (
                "README.md",
                "POST /mcp --mcp-http-auth-token FRIGG_MCP_HTTP_AUTH_TOKEN\n\
                     keep --mcp-http-auth-token set or use the FRIGG_MCP_HTTP_AUTH_TOKEN env var\n",
            ),
            (
                "crates/cli/src/main.rs",
                "mcp_http_auth_token: Option<String>\n\
                     env = \"FRIGG_MCP_HTTP_AUTH_TOKEN\"\n\
                     bearer_auth_middleware\n\
                     serve_http\n",
            ),
            (
                "contracts/errors.md",
                "## MCP payload guidance\n\
                     invalid_params payload guidance\n",
            ),
            (
                "benchmarks/mcp-tools.md",
                "# MCP Tool Benchmark Methodology\n\
                     benchmark notes for MCP tools\n",
            ),
            (
                "crates/cli/tests/security.rs",
                "fn auth_token_marker() { let marker = \"auth token\"; }\n",
            ),
            ("crates/cli/src/lib.rs", "pub mod domain;\n"),
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
                "crates/cli/src/main.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record("repo-001", "snapshot-001", "README.md", 0, vec![0.82, 0.0]),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "contracts/errors.md",
                0,
                vec![0.76, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "benchmarks/mcp-tools.md",
                0,
                vec![0.71, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "crates/cli/tests/security.rs",
                0,
                vec![0.36, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "crates/cli/src/lib.rs",
                0,
                vec![0.62, 0.0],
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
            query: "where is the optional HTTP MCP auth token declared enforced and documented"
                .to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert!(output.note.semantic_enabled);

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.contains(&"crates/cli/src/main.rs"),
        "runtime auth witness should remain visible under semantic-ok ranking: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.contains(&"README.md"),
        "README auth witness should remain visible when the query explicitly asks where behavior is documented: {ranked_paths:?}"
    );
    let readme_position = output
        .matches
        .iter()
        .position(|entry| entry.document.path == "README.md")
        .expect("README witness position should be present");
    let benchmark_position = output
        .matches
        .iter()
        .position(|entry| entry.document.path == "benchmarks/mcp-tools.md");
    assert!(
        benchmark_position.is_none() || Some(readme_position) < benchmark_position,
        "README auth docs should outrank benchmark docs for auth-entrypoint queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_queries_choose_build_anchor_excerpt() -> FriggResult<()> {
    let query = "where the app starts and builds the pipeline runner";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "src/main.rs",
                1081,
                26,
                "let mut runner = build_pipeline_runner(&self.config);",
            ),
            text_match(
                "repo-001",
                "src/main.rs",
                1453,
                5,
                "runner: &PipelineRunner,",
            ),
            text_match(
                "repo-001",
                "src/runner.rs",
                1216,
                12,
                "struct FakeInProcessExecutor {",
            ),
        ],
        query,
    );
    let main_hit = lexical
        .iter()
        .find(|hit| hit.document.path == "src/main.rs")
        .expect("main.rs lexical hit should exist");

    assert!(
        main_hit.excerpt.contains("build_pipeline_runner"),
        "entrypoint/build-flow queries should keep the strongest build anchor excerpt for main.rs, got {:?}",
        main_hit.excerpt
    );
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_queries_promote_main_over_runner_helpers() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-entrypoint-build-flow");
    prepare_workspace(
        &root,
        &[
            (
                "src/main.rs",
                "fn main() {\n\
                     let config = AppConfig::load();\n\
                     let mut runner = build_pipeline_runner(&config);\n\
                     run_pipeline(&mut runner);\n\
                     }\n\
                     fn build_pipeline_runner(config: &AppConfig) -> PipelineRunner {\n\
                     PipelineRunner::new(config.clone())\n\
                     }\n",
            ),
            (
                "src/runner.rs",
                "pub struct PipelineRunner;\n\
                     struct FakeInProcessExecutor;\n\
                     impl PipelineRunner {\n\
                     pub fn new(_config: AppConfig) -> Self { Self }\n\
                     }\n",
            ),
            (
                "tests/pipeline_runner_contract.rs",
                "#[test]\n\
                     fn contract() { let runner = PipelineRunner::default(); }\n",
            ),
            (
                "specs/01-pipeline-runner/design.md",
                "# Design\n\
                     The pipeline runner boots from the app startup flow.\n",
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
                "src/main.rs",
                0,
                vec![0.92, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/runner.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "tests/pipeline_runner_contract.rs",
                0,
                vec![0.72, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "specs/01-pipeline-runner/design.md",
                0,
                vec![0.95, 0.0],
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
            query: "where the app starts and builds the pipeline runner".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &semantic_executor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
    assert_eq!(output.matches[0].document.path, "src/main.rs");
    assert!(
        output.matches[0].excerpt.contains("build_pipeline_runner"),
        "top entrypoint/build-flow witness should surface the build anchor excerpt, got {:?}",
        output.matches[0].excerpt
    );
    assert!(
        output
            .matches
            .iter()
            .any(|entry| entry.document.path == "src/runner.rs"),
        "runner helper should remain available as a secondary witness"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_symbol_plus_entrypoint_queries_keep_runner_family_above_semantic_tail()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-entrypoint-symbol-tail");
    prepare_workspace(
        &root,
        &[
            (
                "src/main.rs",
                "fn main() {\n\
                     let config = load_config();\n\
                     let mut runner = build_pipeline_runner(&config);\n\
                     run_pipeline(&mut runner);\n\
                     }\n",
            ),
            (
                "src/runner.rs",
                "pub struct PipelineRunner;\n\
                     impl PipelineRunner {\n\
                     pub fn new() -> Self { Self }\n\
                     }\n",
            ),
            ("src/replay.rs", "pub fn bootstrap_replay() {}\n"),
            (
                "src/stt_google_tool.rs",
                "pub fn bootstrap_google_tool() {}\n",
            ),
            ("src/config.rs", "pub fn bootstrap_config() {}\n"),
            ("src/lib.rs", "pub fn bootstrap_runtime() {}\n"),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record("repo-001", "snapshot-001", "src/main.rs", 0, vec![1.0, 0.0]),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/runner.rs",
                0,
                vec![0.82, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/replay.rs",
                0,
                vec![0.96, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/stt_google_tool.rs",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src/config.rs",
                0,
                vec![0.94, 0.0],
            ),
            semantic_record("repo-001", "snapshot-001", "src/lib.rs", 0, vec![0.93, 0.0]),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "build_pipeline_runner entry point bootstrap".to_owned(),
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

    let runner_position = output
        .matches
        .iter()
        .position(|entry| entry.document.path == "src/runner.rs")
        .expect("runner witness should remain in the ranked set");
    let replay_position = output
        .matches
        .iter()
        .position(|entry| entry.document.path == "src/replay.rs");
    let stt_position = output
        .matches
        .iter()
        .position(|entry| entry.document.path == "src/stt_google_tool.rs");

    assert_eq!(output.matches[0].document.path, "src/main.rs");
    assert!(
        replay_position.is_none() || runner_position < replay_position.unwrap(),
        "runner witness should outrank replay semantic tail for mixed symbol-plus-entrypoint queries: {:?}",
        output.matches
    );
    assert!(
        stt_position.is_none() || runner_position < stt_position.unwrap(),
        "runner witness should outrank unrelated semantic tail for mixed symbol-plus-entrypoint queries: {:?}",
        output.matches
    );

    cleanup_workspace(&root);
    Ok(())
}
