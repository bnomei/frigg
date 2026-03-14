use super::*;

#[test]
fn hybrid_ranking_python_entrypoint_queries_keep_python_witnesses_above_frontend_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-entrypoints-vs-frontend-noise");
    prepare_workspace(
        &root,
        &[
            (
                "classic/original_autogpt/autogpt/app/main.py",
                "from autogpt.app.cli import run_cli\n\
                     run_cli()\n",
            ),
            (
                "autogpt_platform/backend/backend/app.py",
                "from fastapi import FastAPI\n\
                     application = FastAPI()\n",
            ),
            (
                "autogpt_platform/backend/backend/copilot/executor/__main__.py",
                "from backend.copilot.executor.processor import Processor\n\
                     Processor().run()\n",
            ),
            (
                "autogpt_platform/backend/pyproject.toml",
                "[project]\n\
                     name = \"autogpt-backend\"\n\
                     [project.scripts]\n\
                     backend = \"backend.app:app\"\n",
            ),
            (
                "classic/benchmark/tests/test_benchmark_workflow.py",
                "def verify_graph_shape() -> None:\n\
                     assert True\n",
            ),
            (
                "autogpt_platform/frontend/src/components/renderers/InputRenderer/docs/HEIRARCHY.md",
                "# Hierarchy\n\
                     app startup cli main renderer bootstrap guide\n",
            ),
            (
                "autogpt_platform/frontend/CONTRIBUTING.md",
                "# Frontend contributing\n\
                     app startup cli main contributor notes\n",
            ),
            (
                "docs/platform/advanced_setup.md",
                "# Advanced setup\n\
                     bootstrap app startup cli main platform setup\n",
            ),
            (
                "classic/benchmark/frontend/package.json",
                "{\n\
                     \"name\": \"frontend-benchmark\",\n\
                     \"main\": \"index.js\"\n\
                     }\n",
            ),
            (
                "autogpt_platform/frontend/src/app/api/openapi.json",
                "{\n\
                     \"openapi\": \"3.1.0\",\n\
                     \"info\": {\"title\": \"frontend app main api\"}\n\
                     }\n",
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
                "autogpt_platform/frontend/src/components/renderers/InputRenderer/docs/HEIRARCHY.md",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "docs/platform/advanced_setup.md",
                0,
                vec![0.99, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/frontend/src/app/api/openapi.json",
                0,
                vec![0.97, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/frontend/CONTRIBUTING.md",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/benchmark/frontend/package.json",
                0,
                vec![0.93, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/original_autogpt/autogpt/app/main.py",
                0,
                vec![0.78, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/app.py",
                0,
                vec![0.76, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/copilot/executor/__main__.py",
                0,
                vec![0.74, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/pyproject.toml",
                0,
                vec![0.70, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/benchmark/tests/test_benchmark_workflow.py",
                0,
                vec![0.68, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap app startup cli main".to_owned(),
            limit: 8,
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

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| *path == "classic/original_autogpt/autogpt/app/main.py"),
        "python main entrypoint should appear in the top witness set: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| *path == "autogpt_platform/backend/backend/app.py"),
        "python app runtime witness should appear in the top witness set: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| *path == "autogpt_platform/backend/pyproject.toml"),
        "python runtime config should appear in the top witness set: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| *path == "classic/benchmark/tests/test_benchmark_workflow.py"),
        "python tests should remain visible in the top witness set: {ranked_paths:?}"
    );

    let main_position = ranked_paths
        .iter()
        .position(|path| *path == "classic/original_autogpt/autogpt/app/main.py")
        .expect("python main entrypoint should be ranked");
    let app_position = ranked_paths
        .iter()
        .position(|path| *path == "autogpt_platform/backend/backend/app.py")
        .expect("python app witness should be ranked");
    if let Some(openapi_position) = ranked_paths
        .iter()
        .position(|path| *path == "autogpt_platform/frontend/src/app/api/openapi.json")
    {
        assert!(
            main_position < openapi_position,
            "python main entrypoint should outrank frontend openapi noise: {ranked_paths:?}"
        );
    }
    if let Some(frontend_doc_position) = ranked_paths.iter().position(|path| {
        *path
            == "autogpt_platform/frontend/src/components/renderers/InputRenderer/docs/HEIRARCHY.md"
    }) {
        assert!(
            app_position < frontend_doc_position,
            "python app witness should outrank frontend hierarchy docs: {ranked_paths:?}"
        );
    }

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_entrypoint_queries_prefer_canonical_entrypoints_over_backend_modules()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-entrypoints-vs-backend-modules");
    let mut files = vec![
            (
                "classic/original_autogpt/autogpt/app/main.py".to_owned(),
                "from autogpt.app.cli import run_cli\nrun_cli()\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/app.py".to_owned(),
                "from fastapi import FastAPI\napplication = FastAPI()\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/copilot/executor/__main__.py".to_owned(),
                "from backend.copilot.executor.processor import Processor\nProcessor().run()\n"
                    .to_owned(),
            ),
            (
                "autogpt_platform/backend/pyproject.toml".to_owned(),
                "[project]\nname = \"autogpt-backend\"\n[project.scripts]\nbackend = \"backend.app:app\"\n"
                    .to_owned(),
            ),
            (
                "autogpt_platform/autogpt_libs/pyproject.toml".to_owned(),
                "[project]\nname = \"autogpt-libs\"\n".to_owned(),
            ),
            (
                "classic/original_autogpt/pyproject.toml".to_owned(),
                "[project]\nname = \"classic-autogpt\"\n".to_owned(),
            ),
            (
                "classic/benchmark/tests/test_benchmark_workflow.py".to_owned(),
                "def verify_graph_shape() -> None:\n    assert True\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/blocks/twitter/tweets/manage.py".to_owned(),
                "def main() -> None:\n    return None\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/cli.py".to_owned(),
                "def main() -> None:\n    return None\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/blocks/notion/read_database.py".to_owned(),
                "def read_database() -> dict:\n    return {\"status\": \"ok\"}\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/api/features/mcp/test_routes.py".to_owned(),
                "def test_routes_health() -> None:\n    assert True\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/copilot/executor/processor.py".to_owned(),
                "class Processor:\n    def run(self) -> None:\n        pass\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/blocks/data_manipulation.py".to_owned(),
                "def transform_records() -> None:\n    return None\n".to_owned(),
            ),
        ];
    for index in 0..40 {
        files.push((
            format!("autogpt_platform/backend/backend/blocks/generated/noise_{index}.py"),
            format!("def generated_module_{index}() -> None:\n    return None\n"),
        ));
    }
    let file_refs = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    prepare_workspace(&root, &file_refs)?;

    let mut semantic_records = vec![
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/pyproject.toml",
            0,
            vec![1.0, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/autogpt_libs/pyproject.toml",
            0,
            vec![0.995, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "classic/original_autogpt/pyproject.toml",
            0,
            vec![0.992, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/blocks/notion/read_database.py",
            0,
            vec![0.99, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/blocks/twitter/tweets/manage.py",
            0,
            vec![0.985, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/api/features/mcp/test_routes.py",
            0,
            vec![0.98, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/cli.py",
            0,
            vec![0.975, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/copilot/executor/processor.py",
            0,
            vec![0.97, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/blocks/data_manipulation.py",
            0,
            vec![0.96, 0.0],
        ),
    ];
    for index in 0..40 {
        semantic_records.push(semantic_record(
            "repo-001",
            "snapshot-001",
            &format!("autogpt_platform/backend/backend/blocks/generated/noise_{index}.py"),
            0,
            vec![0.95 - (index as f32 * 0.002), 0.0],
        ));
    }
    semantic_records.extend([
        semantic_record(
            "repo-001",
            "snapshot-001",
            "classic/original_autogpt/autogpt/app/main.py",
            0,
            vec![0.78, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/app.py",
            0,
            vec![0.76, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "autogpt_platform/backend/backend/copilot/executor/__main__.py",
            0,
            vec![0.74, 0.0],
        ),
        semantic_record(
            "repo-001",
            "snapshot-001",
            "classic/benchmark/tests/test_benchmark_workflow.py",
            0,
            vec![0.72, 0.0],
        ),
    ]);
    seed_semantic_embeddings(&root, "repo-001", "snapshot-001", &semantic_records)?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 8;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap app startup cli main config tests benchmark workflow"
                .to_owned(),
            limit: 8,
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

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .take(8)
            .any(|path| *path == "classic/original_autogpt/autogpt/app/main.py"),
        "main.py should remain visible via path-shaped witness recall even without content overlap: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(8)
            .any(|path| *path == "autogpt_platform/backend/backend/app.py"),
        "app.py should remain visible via path-shaped witness recall even without content overlap: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(8)
            .any(|path| *path == "autogpt_platform/backend/backend/copilot/executor/__main__.py"),
        "__main__.py should remain visible via path-shaped witness recall even without content overlap: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(8)
            .any(|path| *path == "classic/benchmark/tests/test_benchmark_workflow.py"),
        "python tests should remain visible in the crowded anchored witness set: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_config_queries_prefer_runtime_manifests_over_readmes() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-config-vs-readmes");
    prepare_workspace(
        &root,
        &[
            (
                "README.md",
                "# Setup\nconfig setup pyproject installation guide\n",
            ),
            (
                "docs/setup.md",
                "# Platform setup\nconfig setup pyproject walkthrough\n",
            ),
            (
                "autogpt_platform/backend/pyproject.toml",
                "[project]\nname = \"autogpt-backend\"\n",
            ),
            (
                "classic/original_autogpt/setup.py",
                "from setuptools import setup\nsetup(name=\"classic-autogpt\")\n",
            ),
            (
                "autogpt_platform/frontend/package.json",
                "{\n  \"name\": \"frontend\"\n}\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record("repo-001", "snapshot-001", "README.md", 0, vec![1.0, 0.0]),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "docs/setup.md",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/frontend/package.json",
                0,
                vec![0.96, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/pyproject.toml",
                0,
                vec![0.82, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/original_autogpt/setup.py",
                0,
                vec![0.8, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 5;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "config setup pyproject".to_owned(),
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

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    let backend_pyproject_position = ranked_paths
        .iter()
        .position(|path| *path == "autogpt_platform/backend/pyproject.toml")
        .expect("pyproject witness should be ranked");
    let readme_position = ranked_paths
        .iter()
        .position(|path| *path == "README.md")
        .expect("README noise should still be ranked");

    assert!(
        ranked_paths
            .iter()
            .take(3)
            .any(|path| *path == "autogpt_platform/backend/pyproject.toml"),
        "runtime manifest should appear near the top for focused config queries: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(4)
            .any(|path| *path == "classic/original_autogpt/setup.py"),
        "setup.py witness should remain visible for focused config queries: {ranked_paths:?}"
    );
    assert!(
        backend_pyproject_position < readme_position,
        "runtime manifest should outrank README drift for focused config queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_test_queries_prefer_backend_tests_over_frontend_docs() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-tests-vs-frontend-docs");
    prepare_workspace(
        &root,
        &[
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                "def test_e2e_auth_flow() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                "def build_test_helpers() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/frontend/src/tests/CLAUDE.md",
                "# Frontend tests\ntests e2e helpers guidance\n",
            ),
            (
                "autogpt_platform/frontend/CLAUDE.md",
                "# Frontend guide\ntests e2e helpers overview\n",
            ),
            (
                "docs/testing.md",
                "# Testing guide\ntests e2e helpers reference\n",
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
                "autogpt_platform/frontend/src/tests/CLAUDE.md",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/frontend/CLAUDE.md",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "docs/testing.md",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                0,
                vec![0.82, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                0,
                vec![0.80, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 5;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests e2e helpers".to_owned(),
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

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    let backend_test_position = ranked_paths
        .iter()
        .position(|path| *path == "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py")
        .expect("backend test witness should be ranked");
    let frontend_doc_position = ranked_paths
        .iter()
        .position(|path| *path == "autogpt_platform/frontend/src/tests/CLAUDE.md")
        .expect("frontend doc noise should still be ranked");

    assert!(
        ranked_paths
            .iter()
            .take(3)
            .any(|path| *path == "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py"),
        "backend test witness should appear near the top for focused tests queries: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(4)
            .any(|path| *path == "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py"),
        "test helper witness should remain visible for focused tests queries: {ranked_paths:?}"
    );
    assert!(
        backend_test_position < frontend_doc_position,
        "backend test witness should outrank frontend test docs: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_runtime_entrypoint_test_queries_keep_packet_backend_tests_visible()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-runtime-entrypoints-packet-tests");
    prepare_workspace(
        &root,
        &[
            (
                "autogpt_platform/backend/backend/api/test_helpers.py",
                "def build_test_helpers() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                "def test_e2e_auth_flow() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                "def load_test_helper_graph() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
                "def test_server_bootstrap() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/pyproject.toml",
                "[project]\nname = \"autogpt-backend\"\n[project.scripts]\nbackend = \"backend.app:app\"\n",
            ),
            (
                "autogpt_platform/autogpt_libs/pyproject.toml",
                "[project]\nname = \"autogpt-libs\"\n",
            ),
            (
                "classic/original_autogpt/autogpt/app/setup.py",
                "from setuptools import setup\nsetup(name=\"classic-autogpt-app\")\n",
            ),
            (
                "classic/benchmark/pyproject.toml",
                "[project]\nname = \"agbenchmark\"\n",
            ),
            (
                "classic/forge/pyproject.toml",
                "[project]\nname = \"forge\"\n",
            ),
            (
                "classic/original_autogpt/setup.py",
                "from setuptools import setup\nsetup(name=\"classic-autogpt\")\n",
            ),
            (
                "classic/original_autogpt/pyproject.toml",
                "[project]\nname = \"classic-autogpt\"\n",
            ),
            (
                "autogpt_platform/backend/test/sdk/conftest.py",
                "def pytest_configure() -> None:\n    return None\n",
            ),
            (
                "classic/original_autogpt/tests/unit/test_config.py",
                "def test_runtime_config() -> None:\n    assert True\n",
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 16;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests fixtures integration helpers e2e config setup pyproject".to_owned(),
            limit: 16,
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

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let required_witnesses = [
        "autogpt_platform/backend/backend/api/test_helpers.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
    ];

    let first_required_position = ranked_paths
        .iter()
        .position(|path| required_witnesses.iter().any(|required| required == path))
        .expect("at least one packet test witness should be ranked");
    let classic_test_config_position = ranked_paths
        .iter()
        .position(|path| *path == "classic/original_autogpt/tests/unit/test_config.py")
        .expect("classic test-config noise should still be ranked");

    assert!(
        ranked_paths
            .iter()
            .take(12)
            .any(|path| required_witnesses.iter().any(|required| required == path)),
        "at least one required packet test witness should stay visible under runtime-config crowding: {ranked_paths:?}"
    );
    assert!(
        first_required_position < classic_test_config_position,
        "packet backend test witnesses should outrank generic config-heavy test noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_entrypoint_test_queries_prefer_package_scoped_tests_over_sibling_integration_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-entrypoint-tests-package-scoped");
    prepare_workspace(
        &root,
        &[
            (
                "src/backend/base/langflow/main.py",
                "from langflow.interface.run import main\nmain()\n",
            ),
            (
                "src/backend/base/langflow/__main__.py",
                "from langflow.main import main\nmain()\n",
            ),
            (
                "src/backend/base/langflow/agentic/mcp/__main__.py",
                "from langflow.agentic.mcp.server import serve\nserve()\n",
            ),
            (
                "src/backend/base/langflow/interface/run.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "src/backend/base/langflow/server.py",
                "def serve() -> None:\n    return None\n",
            ),
            (
                "src/lfx/src/lfx/__main__.py",
                "from lfx.cli.run import main\nmain()\n",
            ),
            (
                "src/lfx/src/lfx/cli/run.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "src/lfx/src/lfx/interface/run.py",
                "def run_interface() -> None:\n    return None\n",
            ),
            (
                "src/lfx/src/lfx/components/datastax/run.py",
                "def run_component() -> None:\n    return None\n",
            ),
            (
                "src/backend/base/pyproject.toml",
                "[project]\nname = \"langflow-backend\"\n[project.scripts]\nlangflow = \"langflow.main:main\"\n",
            ),
            (
                "src/lfx/pyproject.toml",
                "[project]\nname = \"lfx\"\n[project.scripts]\nlfx = \"lfx.cli.run:main\"\n",
            ),
            ("pyproject.toml", "[project]\nname = \"langflow-root\"\n"),
            (
                ".github/workflows/deploy-docs-draft.yml",
                "name: deploy docs\njobs:\n  build:\n    steps:\n      - run: python -m langflow\n",
            ),
            (
                ".github/workflows/docker-build-v2.yml",
                "name: docker build\njobs:\n  docker:\n    steps:\n      - run: python src/backend/base/langflow/main.py\n",
            ),
            (
                "src/backend/base/langflow/tests/api/v1/test_openai_responses_error.py",
                "def test_openai_responses_error() -> None:\n    assert True\n",
            ),
            (
                "src/backend/base/langflow/tests/services/database/models/test_normalize_string_or_none.py",
                "def test_normalize_string_or_none() -> None:\n    assert True\n",
            ),
            (
                "src/backend/base/langflow/tests/services/database/models/test_parse_uuid.py",
                "def test_parse_uuid() -> None:\n    assert True\n",
            ),
            (
                "src/backend/tests/integration/test_openai_responses_extended.py",
                "async def test_openai_responses_extended() -> None:\n    assert True\n",
            ),
            (
                "src/backend/base/langflow/initial_setup/starter_projects/Basic Prompting.json",
                "{\"name\": \"Basic Prompting\"}\n",
            ),
            (
                "src/backend/base/langflow/initial_setup/starter_projects/Blog Writer.json",
                "{\"name\": \"Blog Writer\"}\n",
            ),
            (
                "src/backend/base/langflow/initial_setup/starter_projects/Basic Prompt Chaining.json",
                "{\"name\": \"Basic Prompt Chaining\"}\n",
            ),
            (
                "src/backend/base/langflow/initial_setup/starter_projects/Custom Component Generator.json",
                "{\"name\": \"Custom Component Generator\"}\n",
            ),
            (
                "src/frontend/src/App.tsx",
                "export const App = () => null;\n",
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 16;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration entry point bootstrap app startup cli main openai responses normalize string config setup".to_owned(),
                limit: 16,
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

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let witness_paths = output
        .channel_results
        .iter()
        .find(|result| result.channel == crate::domain::EvidenceChannel::PathSurfaceWitness)
        .map(|result| {
            result
                .hits
                .iter()
                .map(|hit| hit.document.path.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let package_scoped_tests = [
        "src/backend/base/langflow/tests/api/v1/test_openai_responses_error.py",
        "src/backend/base/langflow/tests/services/database/models/test_normalize_string_or_none.py",
        "src/backend/base/langflow/tests/services/database/models/test_parse_uuid.py",
    ];
    let sibling_integration_test =
        "src/backend/tests/integration/test_openai_responses_extended.py";

    assert!(
        ranked_paths
            .iter()
            .take(16)
            .any(|path| *path == "src/backend/base/pyproject.toml"),
        "runtime config should stay visible under mixed entrypoint/test crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(16)
            .any(|path| package_scoped_tests.iter().any(|required| required == path)),
        "package-scoped tests should stay visible under sibling integration crowding: {ranked_paths:?}"
    );

    let package_scoped_position = ranked_paths
        .iter()
        .position(|path| package_scoped_tests.iter().any(|required| required == path))
        .expect("a package-scoped test should be ranked");
    let sibling_integration_position = ranked_paths
        .iter()
        .position(|path| *path == sibling_integration_test)
        .expect("sibling integration test noise should still be ranked");

    assert!(
        package_scoped_position < sibling_integration_position,
        "package-scoped runtime-family tests should outrank sibling integration noise: {ranked_paths:?}"
    );
    assert!(
        witness_paths
            .iter()
            .any(|path| package_scoped_tests.iter().any(|required| required == path)),
        "path surface witnesses should expose package-scoped tests for mixed entrypoint/test queries: {witness_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_entrypoint_queries_recover_saved_wave_backend_tests_under_classic_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-python-saved-wave-entrypoint-backend-tests");
    prepare_workspace(
        &root,
        &[
            (
                ".github/workflows/classic-autogpt-docker-release.yml",
                "name: Docker release\njobs:\n  release:\n    steps:\n      - run: python classic/cli.py\n",
            ),
            (
                "classic/original_autogpt/autogpt/app/main.py",
                "from autogpt.app.cli import run_cli\nrun_cli()\n",
            ),
            (
                "classic/benchmark/agbenchmark/main.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "classic/benchmark/agbenchmark/utils/dependencies/main.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/copilot/executor/__main__.py",
                "from backend.copilot.executor.server import serve\nserve()\n",
            ),
            (
                "autogpt_platform/backend/backend/app.py",
                "from fastapi import FastAPI\napp = FastAPI()\n",
            ),
            (
                "autogpt_platform/backend/backend/cli.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "classic/benchmark/agbenchmark/__main__.py",
                "from agbenchmark.main import main\nmain()\n",
            ),
            (
                "classic/benchmark/agbenchmark/app.py",
                "def app() -> None:\n    return None\n",
            ),
            ("classic/cli.py", "def main() -> None:\n    return None\n"),
            (
                "classic/forge/forge/__main__.py",
                "from forge.app import app\napp()\n",
            ),
            (
                "classic/original_autogpt/autogpt/__main__.py",
                "from autogpt.app.main import run\nrun()\n",
            ),
            (
                "classic/original_autogpt/autogpt/app/cli.py",
                "def run_cli() -> None:\n    return None\n",
            ),
            (
                "classic/benchmark/frontend/src/pages/index.tsx",
                "export const Home = 'entry point bootstrap app startup cli main';\n",
            ),
            (
                "autogpt_platform/backend/pyproject.toml",
                "[project]\nname = \"autogpt-backend\"\n[project.scripts]\nbackend = \"backend.app:app\"\n",
            ),
            (
                "autogpt_platform/backend/test/agent_generator/test_service.py",
                "def test_service_runtime() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/api/test_helpers.py",
                "def build_test_helpers() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                "def test_e2e_auth_flow() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                "def load_test_helper_graph() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_integration.py",
                "def test_integration_bridge() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_mcp.py",
                "def test_mcp_runtime() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_oauth.py",
                "def test_oauth_flow() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
                "def test_server_bootstrap() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/test/test_block.py",
                "def test_block_contract() -> None:\n    assert True\n",
            ),
            (
                "classic/original_autogpt/tests/integration/test_setup.py",
                "def test_setup() -> None:\n    assert True\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap app startup cli main".to_owned(),
            limit: 16,
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
    let required_backend_tests = [
        "autogpt_platform/backend/backend/api/test_helpers.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_integration.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_mcp.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_oauth.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
        "autogpt_platform/backend/backend/blocks/test/test_block.py",
    ];
    let _witness_paths = output
        .channel_results
        .iter()
        .find(|result| result.channel == crate::domain::EvidenceChannel::PathSurfaceWitness)
        .map(|result| {
            result
                .hits
                .iter()
                .map(|hit| hit.document.path.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    assert!(
        ranked_paths
            .iter()
            .take(16)
            .any(|path| *path == "autogpt_platform/backend/pyproject.toml"),
        "saved-wave entrypoint queries should keep backend runtime config visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(16)
            .any(|path| required_backend_tests
                .iter()
                .any(|required| required == path)),
        "saved-wave entrypoint queries should recover a backend test witness under classic crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_python_tests_queries_recover_saved_wave_backend_tests_under_setup_crowding()
-> FriggResult<()> {
    let intent = HybridRankingIntent::from_query(
        "tests fixtures integration helpers e2e config setup pyproject",
    );
    assert!(intent.wants_test_witness_recall);

    let root = temp_workspace_root("hybrid-python-saved-wave-tests-backend-witness");
    prepare_workspace(
        &root,
        &[
            (
                "classic/original_autogpt/tests/integration/test_setup.py",
                "def test_setup() -> None:\n    assert True\n",
            ),
            (
                "classic/original_autogpt/tests/unit/test_config.py",
                "def test_runtime_config() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/pyproject.toml",
                "[project]\nname = \"autogpt-backend\"\n[project.scripts]\nbackend = \"backend.app:app\"\n",
            ),
            (
                "classic/original_autogpt/autogpt/app/setup.py",
                "from setuptools import setup\nsetup(name=\"classic-autogpt-app\")\n",
            ),
            (
                "classic/benchmark/pyproject.toml",
                "[project]\nname = \"agbenchmark\"\n",
            ),
            (
                "classic/forge/pyproject.toml",
                "[project]\nname = \"forge\"\n",
            ),
            (
                "classic/original_autogpt/setup.py",
                "from setuptools import setup\nsetup(name=\"classic-autogpt\")\n",
            ),
            (
                "autogpt_platform/autogpt_libs/pyproject.toml",
                "[project]\nname = \"autogpt-libs\"\n",
            ),
            (
                "classic/original_autogpt/pyproject.toml",
                "[project]\nname = \"classic-autogpt\"\n",
            ),
            ("classic/cli.py", "def main() -> None:\n    return None\n"),
            (
                "classic/original_autogpt/autogpt/app/cli.py",
                "def run_cli() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/cli.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "classic/benchmark/agbenchmark/__main__.py",
                "from agbenchmark.main import main\nmain()\n",
            ),
            (
                "classic/benchmark/agbenchmark/app.py",
                "def app() -> None:\n    return None\n",
            ),
            (
                "classic/forge/forge/__main__.py",
                "from forge.app import app\napp()\n",
            ),
            (
                "autogpt_platform/backend/backend/app.py",
                "from fastapi import FastAPI\napp = FastAPI()\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/twitter/tweets/manage.py",
                "def main() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/test/agent_generator/test_service.py",
                "def test_service_runtime() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/api/test_helpers.py",
                "def build_test_helpers() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                "def test_e2e_auth_flow() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                "def load_test_helper_graph() -> None:\n    return None\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_integration.py",
                "def test_integration_bridge() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_mcp.py",
                "def test_mcp_runtime() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_oauth.py",
                "def test_oauth_flow() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
                "def test_server_bootstrap() -> None:\n    assert True\n",
            ),
            (
                "autogpt_platform/backend/backend/blocks/test/test_block.py",
                "def test_block_contract() -> None:\n    assert True\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests fixtures integration helpers e2e config setup pyproject".to_owned(),
            limit: 16,
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
    let required_backend_tests = [
        "autogpt_platform/backend/backend/api/test_helpers.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_integration.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_mcp.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_oauth.py",
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
        "autogpt_platform/backend/backend/blocks/test/test_block.py",
    ];

    assert!(
        ranked_paths
            .iter()
            .take(16)
            .any(|path| *path == "autogpt_platform/backend/pyproject.toml"),
        "saved-wave tests queries should keep backend runtime config visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(16)
            .any(|path| required_backend_tests
                .iter()
                .any(|required| required == path)),
        "saved-wave tests queries should recover a backend test witness under setup crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
