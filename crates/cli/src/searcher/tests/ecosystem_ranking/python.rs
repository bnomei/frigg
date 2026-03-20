use super::*;

#[test]
fn hybrid_ranking_python_runtime_queries_prefer_implementation_over_tests_in_lexical_only_mode()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-runtime-vs-tests-lexical-only");
    prepare_workspace(
        &root,
        &[
            (
                "src/config.py",
                "def load_source_path_config():\n    pass  # source path startup config handled runtime cli\n",
            ),
            (
                "src/app.py",
                "from config import load_source_path_config\n\
                 def main():\n\
                     load_source_path_config()  # source path startup config handled runtime cli\n",
            ),
            (
                "tests/test_source_path.py",
                "def test_source_path_startup_config():\n    assert True  # tests cover source path startup config\n",
            ),
            (
                "tests/test_config.py",
                "def test_config_loading():\n    assert True  # tests cover source path startup config\n",
            ),
            (
                "README.md",
                "source path startup config handled in docs and tests\n",
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
    let runtime_rank = ranked_paths
        .iter()
        .position(|path| matches!(*path, "src/config.py" | "src/app.py"))
        .expect("python runtime implementation should be ranked");
    let first_test_rank = ranked_paths
        .iter()
        .position(|path| path.starts_with("tests/"))
        .expect("python test should still be visible");

    assert!(
        runtime_rank < first_test_rank,
        "python runtime implementation should beat tests for handled queries in lexical-only mode: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_test_queries_prefer_tests_over_runtime_in_lexical_only_mode()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-tests-vs-runtime-lexical-only");
    prepare_workspace(
        &root,
        &[
            (
                "src/config.py",
                "def load_source_path_config():\n    pass  # source path startup config handled runtime cli\n",
            ),
            (
                "src/app.py",
                "from config import load_source_path_config\n\
                 def main():\n\
                     load_source_path_config()  # source path startup config handled runtime cli\n",
            ),
            (
                "tests/test_source_path.py",
                "def test_source_path_startup_config():\n    assert True  # tests cover source path startup config\n",
            ),
            (
                "tests/test_config.py",
                "def test_config_loading():\n    assert True  # tests cover source path startup config\n",
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
        .expect("python test should be ranked");
    let runtime_rank = ranked_paths
        .iter()
        .position(|path| matches!(*path, "src/config.py" | "src/app.py"))
        .expect("python runtime implementation should remain visible");

    assert!(
        first_test_rank < runtime_rank,
        "python tests should beat runtime files for coverage queries in lexical-only mode: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
