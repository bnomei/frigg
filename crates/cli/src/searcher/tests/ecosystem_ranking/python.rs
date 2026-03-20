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

#[test]
fn hybrid_ranking_python_lexical_only_queries_use_ripgrep_backend_when_available() -> FriggResult<()>
{
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("hybrid-python-ripgrep-lexical-only");
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
                "README.md",
                "source path startup config handled in docs and tests\n",
            ),
        ],
    )?;
    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/config.py"},"lines":{"text":"def load_source_path_config():\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"load_source_path_config"},"start":4,"end":27}]}}
{"type":"match","data":{"path":{"text":"src/app.py"},"lines":{"text":"from config import load_source_path_config\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"load_source_path_config"},"start":19,"end":42}]}}
{"type":"match","data":{"path":{"text":"tests/test_source_path.py"},"lines":{"text":"def test_source_path_startup_config():\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"source_path_startup_config"},"start":9,"end":35}]}}
{"type":"match","data":{"path":{"text":"README.md"},"lines":{"text":"source path startup config handled in docs and tests\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"source path startup config"},"start":0,"end":26}]}}"#,
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let searcher = TextSearcher::new(config);

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

    assert!(matches!(
        output.note.lexical_backend,
        Some(SearchLexicalBackend::Ripgrep | SearchLexicalBackend::Mixed)
    ));

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let runtime_rank = ranked_paths
        .iter()
        .position(|path| matches!(*path, "src/config.py" | "src/app.py"))
        .expect("python runtime implementation should be ranked");
    let test_rank = ranked_paths
        .iter()
        .position(|path| path.starts_with("tests/"))
        .expect("python test should still be visible");

    assert!(
        runtime_rank < test_rank,
        "ripgrep-backed lexical-only queries should still prefer python implementations over tests: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
