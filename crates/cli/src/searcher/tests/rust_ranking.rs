use super::*;

#[test]
fn hybrid_ranking_rust_config_queries_rescue_cargo_manifests_from_path_witness_recall()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-config-path-witness");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/src/commands/config.rs",
                "pub fn config_command() { let _ = \"config cargo\"; }\n",
            ),
            ("crates/ruff/Cargo.toml", "[package]\nname = \"ruff\"\n"),
            (
                "README.md",
                "# Config guide\nconfig cargo setup walkthrough\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "config cargo".to_owned(),
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
    let cargo_position = ranked_paths
        .iter()
        .position(|path| *path == "crates/ruff/Cargo.toml")
        .expect("Cargo.toml witness should be ranked");
    let readme_position = ranked_paths
        .iter()
        .position(|path| *path == "README.md")
        .expect("README noise should still be ranked");

    assert!(
        cargo_position < readme_position,
        "Cargo.toml should outrank README drift for `config cargo` queries: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(3)
            .any(|path| *path == "crates/ruff/Cargo.toml"),
        "Cargo.toml should land near the top via config-artifact path recall: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_rust_workspace_config_queries_prefer_root_rust_configs_over_nested_pyprojects()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-workspace-config-vs-pyproject");
    let mut files = vec![
        (
            "Cargo.toml".to_owned(),
            "[workspace]\nmembers = [\"crates/*\"]\n".to_owned(),
        ),
        (
            "Cargo.lock".to_owned(),
            "[[package]]\nname = \"ruff\"\n".to_owned(),
        ),
        (
            ".cargo/config.toml".to_owned(),
            "[build]\ntarget-dir = \"target\"\n".to_owned(),
        ),
        (
            "rust-toolchain.toml".to_owned(),
            "[toolchain]\nchannel = \"stable\"\n".to_owned(),
        ),
        ("rustfmt.toml".to_owned(), "edition = \"2021\"\n".to_owned()),
        ("clippy.toml".to_owned(), "msrv = \"1.80\"\n".to_owned()),
    ];
    files.extend((0..8).map(|index| {
        (
            format!("crates/noise_{index:02}/pyproject.toml"),
            "[tool.pytest.ini_options]\naddopts = \"-q\"\n".to_owned(),
        )
    }));
    let file_refs = files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect::<Vec<_>>();
    prepare_workspace(&root, &file_refs)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "workspace cargo toolchain config cargo lock".to_owned(),
            limit: 9,
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
    let first_rust_config = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "Cargo.toml"
                    | "Cargo.lock"
                    | ".cargo/config.toml"
                    | "rust-toolchain.toml"
                    | "rustfmt.toml"
                    | "clippy.toml"
            )
        })
        .expect("a rust workspace config witness should be ranked");
    let first_pyproject = ranked_paths
        .iter()
        .position(|path| path.ends_with("pyproject.toml"))
        .expect("pyproject noise should still be ranked");

    assert!(
        first_rust_config < first_pyproject,
        "rust workspace config should outrank nested pyproject noise: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(5).any(|path| {
            matches!(
                *path,
                "Cargo.toml"
                    | "Cargo.lock"
                    | ".cargo/config.toml"
                    | "rust-toolchain.toml"
                    | "rustfmt.toml"
                    | "clippy.toml"
            )
        }),
        "a rust workspace config witness should land near the top: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_examples_queries_keep_examples_and_benches_visible_over_test_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-examples-benches");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/tests/cli/main.rs",
                "tests examples fixtures integration benchmark\n",
            ),
            (
                "crates/ruff/tests/cli/lint.rs",
                "tests examples fixtures integration benchmark\n",
            ),
            (
                "crates/ruff_annotate_snippets/tests/examples.rs",
                "tests examples fixtures integration benchmark\n",
            ),
            (
                "crates/ruff_annotate_snippets/examples/expected_type.rs",
                "pub fn demo_example() {}\n",
            ),
            (
                "crates/ruff_benchmark/benches/formatter.rs",
                "pub fn bench_formatter() {}\n",
            ),
            (
                "crates/ruff_benchmark/benches/ty.rs",
                "pub fn bench_ty() {}\n",
            ),
            (
                "crates/ruff/src/cache.rs",
                "tests examples fixtures integration benchmark\n",
            ),
            (
                "docs/examples.md",
                "# Examples\ntests examples fixtures integration benchmark\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests examples fixtures integration benchmark".to_owned(),
            limit: 6,
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
    assert!(
        ranked_paths.iter().take(3).any(|path| matches!(
            *path,
            "crates/ruff_annotate_snippets/examples/expected_type.rs"
                | "crates/ruff_benchmark/benches/formatter.rs"
        )),
        "an examples-or-benches witness should land near the top: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
#[ignore = "workstream-c escalation target"]
fn hybrid_ranking_graphite_queries_prefer_editor_subtree_over_sibling_runtime_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-graphite-editor-subtree");
    prepare_workspace(
        &root,
        &[
            (
                "editor/src/messages/panels/layers.rs",
                "pub fn canvas_panel_runtime() { let _ = \"editor panels canvas runtime\"; }\n",
            ),
            ("editor/tests/canvas_runtime.rs", "mod canvas_runtime {}\n"),
            (
                "node-graph/src/runtime.rs",
                "pub fn node_graph_runtime() { let _ = \"node graph runtime\"; }\n",
            ),
            (
                "desktop/wrapper/src/messages.rs",
                "pub fn desktop_wrapper_messages() { let _ = \"graphite editor panels canvas layout messages desktop wrapper svelte\"; }\n",
            ),
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"editor\", \"desktop\"]\n",
            ),
            ("Cargo.lock", "[[package]]\nname = \"graphite\"\n"),
            (
                "website/content/editor.md",
                "# Editor runtime\neditor panels canvas runtime\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "graphite editor panels canvas layout messages desktop wrapper svelte"
                .to_owned(),
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
    let editor_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "editor/src/messages/panels/layers.rs" | "editor/tests/canvas_runtime.rs"
            )
        })
        .expect("an editor subtree witness should be ranked");
    let sibling_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "node-graph/src/runtime.rs"
                    | "website/content/editor.md"
                    | "desktop/wrapper/src/messages.rs"
                    | "Cargo.toml"
                    | "Cargo.lock"
            )
        })
        .expect("sibling runtime noise should still be ranked");

    assert!(
        editor_position < sibling_position,
        "editor subtree witnesses should outrank sibling runtime noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
#[ignore = "workstream-c escalation target"]
fn hybrid_ranking_ruff_queries_keep_runtime_surfaces_above_docs_and_readme() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-ruff-runtime-vs-docs");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff_server/src/lib.rs",
                "pub fn formatter_server() { let _ = \"formatter server wasm flow\"; }\n",
            ),
            (
                "crates/ruff_wasm/src/lib.rs",
                "pub fn formatter_wasm() { let _ = \"formatter server wasm flow\"; }\n",
            ),
            ("README.md", "# Ruff\nformatter server wasm flow overview\n"),
            (
                "docs/formatter.md",
                "# Formatter\nformatter server wasm flow guide\n",
            ),
            (
                "CONTRIBUTING.md",
                "# Contributing\nformatter server wasm flow guide\n",
            ),
            ("Cargo.lock", "[[package]]\nname = \"ruff\"\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "formatter server wasm flow rust runtime".to_owned(),
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
    let runtime_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "crates/ruff_server/src/lib.rs" | "crates/ruff_wasm/src/lib.rs"
            )
        })
        .expect("a runtime surface should be ranked");
    let docs_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "README.md" | "docs/formatter.md" | "CONTRIBUTING.md" | "Cargo.lock"
            )
        })
        .expect("docs/readme/meta drift should still be ranked");

    assert!(
        runtime_position < docs_position,
        "runtime surfaces should outrank docs/readme/meta drift: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_rust_tests_queries_keep_required_tests_visible_under_examples_and_benches_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-tests-vs-examples-benches-crowding");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/tests/analyze_graph.rs",
                "mod analyze_graph {}\n",
            ),
            (
                "crates/ruff/tests/cli/analyze_graph.rs",
                "mod cli_analyze_graph {}\n",
            ),
            ("crates/ruff/tests/cli/format.rs", "mod cli_format {}\n"),
            ("crates/ruff/tests/cli/lint.rs", "mod cli_lint {}\n"),
            ("crates/ruff/tests/cli/main.rs", "mod cli_main {}\n"),
            ("crates/ruff/tests/config.rs", "mod config_test {}\n"),
            (
                "crates/ruff_annotate_snippets/examples/footer.rs",
                "Level::Error.title(\"mismatched types\").footer(Level::Note.title(\"expected type\"));\n",
            ),
            (
                "crates/ruff_annotate_snippets/examples/footer.svg",
                "<svg><text>expected type</text><text>footer</text></svg>\n",
            ),
            (
                "crates/ruff_python_formatter/tests/fixtures.rs",
                "fn black_compatibility() { format_range(); }\n",
            ),
            (
                "crates/ruff_benchmark/benches/linter.rs",
                "fn benchmark_linter() { criterion_group!(benches); }\n",
            ),
            (
                "crates/ruff_benchmark/benches/ty.rs",
                "fn benchmark_ty() { criterion_group!(benches); }\n",
            ),
            (
                "crates/ruff_benchmark/benches/ty_walltime.rs",
                "fn benchmark_ty_walltime() { criterion_group!(benches); }\n",
            ),
            (
                "crates/ruff_annotate_snippets/examples/expected_type.rs",
                "Level::Note.title(\"expected type\");\n",
            ),
            (
                "crates/ruff_python_parser/tests/fixtures.rs",
                "fn parse_fixture() { parse_module(\"x = 1\"); }\n",
            ),
            (
                "crates/ruff_annotate_snippets/examples/expected_type.svg",
                "<svg><text>expected type</text></svg>\n",
            ),
            (
                "crates/ruff_annotate_snippets/tests/examples.rs",
                "fn examples_snapshot() { assert_snapshot!(); }\n",
            ),
            (
                "crates/ruff_benchmark/benches/formatter.rs",
                "fn benchmark_formatter() { criterion_group!(benches); }\n",
            ),
            (
                "crates/ruff_benchmark/benches/lexer.rs",
                "fn benchmark_lexer() { criterion_group!(benches); }\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests fixtures integration analyze graph entrypoint".to_owned(),
            limit: 12,
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

    assert!(
        ranked_paths.iter().take(12).any(|path| {
            matches!(
                *path,
                "crates/ruff/tests/analyze_graph.rs"
                    | "crates/ruff/tests/cli/analyze_graph.rs"
                    | "crates/ruff/tests/cli/format.rs"
                    | "crates/ruff/tests/cli/lint.rs"
                    | "crates/ruff/tests/cli/main.rs"
                    | "crates/ruff/tests/config.rs"
            )
        }),
        "a required Rust test witness should remain visible under example/bench crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_tests_queries_keep_cli_runtime_witnesses_visible_over_bounded_doc_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-cli-runtime-test-witnesses");
    let mut files = (0..24)
        .map(|index| {
            (
                format!("docs/alpha-{index:02}.md"),
                format!(
                    "# Alpha {index}\ntests examples fixtures integration benchmark latest tool\n"
                ),
            )
        })
        .collect::<Vec<_>>();
    files.extend([
        (
            "docs/cli/latest.md".to_owned(),
            "# `mise latest`\nGets the latest available version for a plugin\n".to_owned(),
        ),
        (
            "docs/cli/tool.md".to_owned(),
            "# `mise tool`\nGets information about a tool\n".to_owned(),
        ),
        (
            "src/cli/latest.rs".to_owned(),
            "/// Gets the latest available version for a plugin\npub struct Latest;\n".to_owned(),
        ),
        (
            "src/cli/test_tool.rs".to_owned(),
            "/// Test a tool installs and executes\npub struct TestTool;\n".to_owned(),
        ),
        (
            "src/test.rs".to_owned(),
            "pub fn init_test_env() { let _ = \"tests fixtures integration\"; }\n".to_owned(),
        ),
    ]);
    let file_refs = files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect::<Vec<_>>();
    prepare_workspace(&root, &file_refs)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests examples fixtures integration benchmark latest tool".to_owned(),
            limit: 8,
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
    assert!(
        ranked_paths.contains(&"src/cli/latest.rs"),
        "path witness recall should keep the CLI runtime witness visible in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_rust_mixed_tests_queries_keep_bench_witnesses_visible_under_test_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-mixed-tests-vs-benches");
    let mut files = (0..14)
        .map(|index| {
            (
                format!("crates/biome_cli/tests/cases/noise_{index:02}.rs"),
                "tests fixtures integration assist biome json css analyzer\n".to_owned(),
            )
        })
        .collect::<Vec<_>>();
    files.extend([
        (
            "crates/biome_cli/tests/cases/assist.rs".to_owned(),
            "tests fixtures integration assist biome json css analyzer\n".to_owned(),
        ),
        (
            "crates/biome_service/tests/fixtures/basic/biome.jsonc".to_owned(),
            "{ \"tests\": \"fixtures integration assist biome json css analyzer\" }\n".to_owned(),
        ),
        (
            "crates/biome_configuration/benches/biome_json.rs".to_owned(),
            "pub fn bench_biome_json() {}\n".to_owned(),
        ),
        (
            "crates/biome_css_analyze/benches/css_analyzer.rs".to_owned(),
            "pub fn bench_css_analyzer() {}\n".to_owned(),
        ),
        (
            "benchmark/biome.json".to_owned(),
            "{ \"benchmark\": true, \"biome\": \"json\" }\n".to_owned(),
        ),
    ]);
    let file_refs = files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect::<Vec<_>>();
    prepare_workspace(&root, &file_refs)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration assist biome json examples benches benchmark css analyzer"
                    .to_owned(),
                limit: 12,
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
    assert!(
        ranked_paths.iter().take(12).any(|path| {
            matches!(
                *path,
                "crates/biome_configuration/benches/biome_json.rs"
                    | "crates/biome_css_analyze/benches/css_analyzer.rs"
            )
        }),
        "a bench witness should remain visible for mixed rust tests queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_rust_mixed_examples_queries_keep_test_witnesses_visible_under_bench_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-mixed-benches-vs-tests");
    let mut files = (0..14)
        .map(|index| {
            (
                format!("crates/biome_package/benches/noise_{index:02}.rs"),
                "examples benches benchmark biome json css analyzer\n".to_owned(),
            )
        })
        .collect::<Vec<_>>();
    files.extend([
        (
            "crates/biome_configuration/benches/biome_json.rs".to_owned(),
            "pub fn bench_biome_json() {}\n".to_owned(),
        ),
        (
            "crates/biome_css_analyze/benches/css_analyzer.rs".to_owned(),
            "pub fn bench_css_analyzer() {}\n".to_owned(),
        ),
        (
            "crates/biome_cli/tests/cases/assist.rs".to_owned(),
            "assert_cli_snapshot();\n".to_owned(),
        ),
        (
            "crates/biome_cli/tests/cases/configuration.rs".to_owned(),
            "assert_cli_snapshot();\n".to_owned(),
        ),
    ]);
    let file_refs = files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect::<Vec<_>>();
    prepare_workspace(&root, &file_refs)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "examples benches benchmark biome json css analyzer tests assist".to_owned(),
            limit: 12,
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
    assert!(
        ranked_paths
            .iter()
            .take(12)
            .any(|path| *path == "crates/biome_cli/tests/cases/assist.rs"),
        "a targeted test witness should remain visible for mixed rust examples queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_graphite_editor_subtree_companion_retrieval_preserves_editor_runtime_tests()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-graphite-editor-subtree-companion");
    prepare_workspace(
        &root,
        &[
            (
                "editor/src/messages/panels.rs",
                "pub fn render_panels() { let _ = \"editor panels runtime\"; }\n",
            ),
            (
                "editor/tests/panels.rs",
                "#[cfg(test)] mod panels_tests {}\n",
            ),
            (
                "desktop/src/messages/layout.rs",
                "pub fn desktop_messages() { let _ = \"desktop layout runtime\"; }\n",
            ),
            (
                "desktop/tests/layout.rs",
                "#[cfg(test)] mod layout_tests {}\n",
            ),
            (
                "website/content/editor.md",
                "# Graphite editor\neditor panels runtime\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "graphite editor panels runtime messages".to_owned(),
            limit: 6,
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
    let witness_health = output
        .channel_results
        .iter()
        .find(|result| result.channel == crate::domain::EvidenceChannel::PathSurfaceWitness)
        .map(|result| (&result.health.status, result.health.reason.as_deref()));
    let trace = output.post_selection_trace.clone();

    let editor_runtime_position = ranked_paths
        .iter()
        .position(|path| *path == "editor/src/messages/panels.rs")
        .unwrap_or_else(|| {
            panic!(
                "editor runtime witness should be ranked: ranked={ranked_paths:?} witness={witness_paths:?} witness_health={witness_health:?} trace={trace:?}"
            )
        });
    let editor_test_position = ranked_paths
        .iter()
        .position(|path| *path == "editor/tests/panels.rs")
        .unwrap_or_else(|| {
            panic!(
                "editor test witness should be ranked: ranked={ranked_paths:?} witness={witness_paths:?} witness_health={witness_health:?} trace={trace:?}"
            )
        });
    let desktop_noise_position = ranked_paths
        .iter()
        .position(|path| *path == "desktop/tests/layout.rs")
        .unwrap_or_else(|| {
            panic!(
                "desktop noise witness should still be ranked: ranked={ranked_paths:?} witness={witness_paths:?} witness_health={witness_health:?} trace={trace:?}"
            )
        });

    assert!(
        editor_runtime_position < desktop_noise_position
            && editor_test_position < desktop_noise_position,
        "editor subtree companions should outrank sibling desktop noise: {ranked_paths:?}",
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_rust_editor_and_wasm_companion_surfaces_stay_localized() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-editor-wasm-companion");
    prepare_workspace(
        &root,
        &[
            (
                "crates/editor/src/lib.rs",
                "pub fn editor_runtime() { let _ = \"editor wasm runtime\"; }\n",
            ),
            (
                "crates/editor/tests/runtime.rs",
                "#[cfg(test)] mod runtime_tests {}\n",
            ),
            (
                "crates/desktop/src/engine.rs",
                "pub fn desktop_runtime() { let _ = \"desktop runtime\"; }\n",
            ),
            (
                "crates/desktop/tests/engine.rs",
                "#[cfg(test)] mod engine_tests {}\n",
            ),
            (
                "crates/wasm/src/lib.rs",
                "pub fn wasm_runtime() { let _ = \"wasm runtime\"; }\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "editor desktop runtime wasm tests".to_owned(),
            limit: 6,
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

    let workspace_position = ranked_paths
        .iter()
        .position(|path| path.ends_with("Cargo.toml"))
        .unwrap_or(usize::MAX);
    let editor_position = ranked_paths
        .iter()
        .position(|path| *path == "crates/editor/src/lib.rs")
        .expect("editor runtime should be ranked");
    let desktop_position = ranked_paths
        .iter()
        .position(|path| *path == "crates/desktop/src/engine.rs")
        .expect("desktop noise should be ranked");

    assert!(
        editor_position < desktop_position,
        "editor companion should outrank unrelated desktop runtime: {ranked_paths:?}",
    );
    assert!(
        workspace_position == usize::MAX || workspace_position <= 6,
        "searcher should keep workspace/config surfaces reachable in top-k: {ranked_paths:?}",
    );

    cleanup_workspace(&root);
    Ok(())
}
