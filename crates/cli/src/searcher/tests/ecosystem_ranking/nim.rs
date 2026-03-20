use super::*;

#[test]
fn hybrid_ranking_nim_runtime_queries_prefer_implementation_over_tests() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-nim-runtime-vs-tests");
    prepare_workspace(
        &root,
        &[
            (
                "src/config.nims",
                "proc loadSourcePathConfig*() = discard # source path startup config handled runtime cli\n",
            ),
            (
                "src/nimlsp.nim",
                "import config\nproc main*() = loadSourcePathConfig() # source path startup config handled runtime cli\n",
            ),
            (
                "tests/source_path_test.nim",
                "suite \"source path startup config\" = discard\n",
            ),
            (
                "tests/config_test.nim",
                "suite \"source path startup config\" = discard\n",
            ),
            (
                "README.md",
                "source path startup config handled in tests and docs\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "where is source path startup config handled".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let config_rank = ranked_paths
        .iter()
        .position(|path| *path == "src/config.nims")
        .expect("nim config implementation should be ranked");
    let main_rank = ranked_paths
        .iter()
        .position(|path| *path == "src/nimlsp.nim")
        .expect("nim runtime entrypoint should be ranked");
    let first_test_rank = ranked_paths
        .iter()
        .position(|path| path.starts_with("tests/"))
        .expect("nim test should still be visible");

    assert!(
        config_rank < first_test_rank || main_rank < first_test_rank,
        "runtime implementation should beat Nim tests for handled queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_nim_test_queries_prefer_tests_over_runtime_files() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-nim-tests-vs-runtime");
    prepare_workspace(
        &root,
        &[
            (
                "src/config.nims",
                "proc loadSourcePathConfig*() = discard # source path startup config handled runtime cli\n",
            ),
            (
                "src/nimlsp.nim",
                "import config\nproc main*() = loadSourcePathConfig() # source path startup config handled runtime cli\n",
            ),
            (
                "tests/source_path_test.nim",
                "suite \"source path startup config\" = discard # tests cover source path startup config\n",
            ),
            (
                "tests/config_test.nim",
                "suite \"source path startup config\" = discard # tests cover source path startup config\n",
            ),
            ("README.md", "tests cover source path startup config\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "what tests cover source path startup config".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let first_test_rank = ranked_paths
        .iter()
        .position(|path| path.starts_with("tests/"))
        .expect("nim tests should be ranked");
    let config_rank = ranked_paths
        .iter()
        .position(|path| *path == "src/config.nims")
        .expect("nim config implementation should still be ranked");

    assert!(
        first_test_rank < config_rank,
        "Nim tests should beat runtime files for test-coverage queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
