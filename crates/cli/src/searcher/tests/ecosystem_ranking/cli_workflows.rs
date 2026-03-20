use super::*;

#[test]
fn hybrid_ranking_cli_entrypoint_queries_prefer_cli_test_witnesses_over_runtime_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-cli-test-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/src/commands/analyze_graph.rs",
                "pub fn analyze_graph() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/checkers/ast/analyze/expression.rs",
                "pub fn analyze_expression() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/checkers/ast/analyze/module.rs",
                "pub fn analyze_module() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/checkers/ast/analyze/suite.rs",
                "pub fn analyze_suite() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/lib.rs",
                "pub fn lib_runtime() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/resources/test/fixtures/isort/pyproject.toml",
                "[tool.ruff]\nline-length = 88\n",
            ),
            (
                "crates/ruff/tests/integration_test.rs",
                "mod integration_test {}\n",
            ),
            (
                ".github/workflows/ci.yaml",
                "ruff analyze cli entrypoint workflow\n",
            ),
            (
                "crates/ruff/tests/cli/analyze_graph.rs",
                "mod cli_analyze_graph {}\n",
            ),
            ("crates/ruff/tests/cli/main.rs", "mod cli_main {}\n"),
            ("crates/ruff/tests/cli/format.rs", "mod cli_format {}\n"),
            ("crates/ruff/tests/cli/lint.rs", "mod cli_lint {}\n"),
            ("crates/ruff/tests/config.rs", "mod config_test {}\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "ruff analyze ruff cli entrypoint".to_owned(),
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
    assert!(
        ranked_paths
            .iter()
            .take(4)
            .any(|path| *path == "crates/ruff/tests/cli/analyze_graph.rs"),
        "CLI analyze_graph test witness should land near the top for the saved query: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| *path == "crates/ruff/tests/cli/main.rs"),
        "secondary CLI test witness should remain visible near the top: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_ci_workflow_queries_surface_hidden_workflow_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-ci-workflow-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                ".github/workflows/autofix.yml",
                "name: autofix.ci\njobs:\n  autofix:\n    steps:\n      - run: cargo codegen\n",
            ),
            (
                ".github/workflows/bench_cli.yml",
                "name: Bench CLI\njobs:\n  bench:\n    steps:\n      - run: cargo bench\n",
            ),
            (
                "crates/noise/src/github.rs",
                "pub fn github_reporter() { let _ = \"github workflow autofix\"; }\n",
            ),
            (
                "crates/noise/src/autofix.rs",
                "pub fn autofix_runtime() { let _ = \"autofix runtime\"; }\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "github workflow autofix bench cli".to_owned(),
            limit: 4,
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
        ranked_paths.iter().take(4).any(|path| matches!(
            *path,
            ".github/workflows/autofix.yml" | ".github/workflows/bench_cli.yml"
        )),
        "CI workflow query should surface a hidden workflow witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_scripts_ops_queries_surface_script_and_justfile_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-scripts-ops-witnesses");
    prepare_workspace(
        &root,
        &[
            ("justfile", "fmt:\n\tcargo fmt\n"),
            (
                "scripts/print-changelog.sh",
                "#!/usr/bin/env bash\necho changelog\n",
            ),
            (
                "scripts/update-manifests.mjs",
                "console.log('update manifests');\n",
            ),
            ("docs/changelog.md", "# Changelog\nupdate notes\n"),
            ("src/version.rs", "pub const VERSION: &str = \"1.0.0\";\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "scripts justfile changelog manifests".to_owned(),
            limit: 4,
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
        ranked_paths.iter().take(4).any(|path| matches!(
            *path,
            "justfile" | "scripts/print-changelog.sh" | "scripts/update-manifests.mjs"
        )),
        "scripts/ops query should surface a concrete script witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[ignore = "workstream-c escalation target"]
#[test]
fn hybrid_ranking_entrypoint_queries_surface_build_workflow_configs() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-entrypoint-build-workflows");
    prepare_workspace(
        &root,
        &[
            (
                "src-tauri/src/main.rs",
                "fn main() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
            ),
            (
                "src-tauri/src/lib.rs",
                "pub fn run() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
            ),
            (
                "src-tauri/src/proxy/config.rs",
                "pub struct ProxyConfig;\n\
                     impl ProxyConfig { pub fn load() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/modules/config.rs",
                "pub struct ModuleConfig;\n\
                     impl ModuleConfig { pub fn load() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/models/config.rs",
                "pub struct AppConfig;\n\
                     impl AppConfig { pub fn load() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/proxy/proxy_pool.rs",
                "pub struct ProxyPool;\n\
                     impl ProxyPool { pub fn runner() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/commands/security.rs",
                "pub fn security_command_runner() {}\n",
            ),
            ("src-tauri/build.rs", "fn main() { tauri_build::build() }\n"),
            (
                ".github/workflows/deploy-pages.yml",
                "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Deploy to GitHub Pages\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor_with_trace(
        SearchHybridQuery {
            query: "entry point bootstrap build flow command runner main config".to_owned(),
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
    let anchor_paths = output
        .ranked_anchors
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let grouped_paths = output
        .coverage_grouped_pool
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths
            .iter()
            .take(8)
            .any(|path| *path == "src-tauri/src/main.rs"),
        "entrypoint runtime witness should remain visible near the top: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(8).any(|path| {
            matches!(
                *path,
                ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
            )
        }),
        "entrypoint/build-flow queries should surface at least one GitHub workflow config witness in top-k: ranked={ranked_paths:?} witness={witness_paths:?} anchors={anchor_paths:?} grouped={grouped_paths:?} trace={:?}",
        output.post_selection_trace
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_build_flow_queries_keep_runtime_entrypoints_visible_under_workflow_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-entrypoint-vs-workflow-crowding");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ruff_dev/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ruff_python_formatter/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ty/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ty_completion_bench/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                ".github/workflows/build-binaries.yml",
                "name: Build binaries\njobs:\n  build:\n    steps:\n      - run: cargo build --release --bin ruff\n",
            ),
            (
                ".github/workflows/build-docker.yml",
                "name: Build docker\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/build-wasm.yml",
                "name: Build wasm\njobs:\n  build:\n    steps:\n      - run: cargo build --target wasm32-unknown-unknown\n",
            ),
            (
                ".github/workflows/publish-playground.yml",
                "name: Publish playground\njobs:\n  publish:\n    steps:\n      - run: cargo run --bin playground\n",
            ),
            (
                ".github/workflows/publish-ty-playground.yml",
                "name: Publish ty playground\njobs:\n  publish:\n    steps:\n      - run: cargo run --bin ty-playground\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\njobs:\n  release:\n    steps:\n      - run: cargo build --release\n",
            ),
            (
                ".github/workflows/publish-docs.yml",
                "name: Publish docs\njobs:\n  publish:\n    steps:\n      - run: cargo doc --no-deps\n",
            ),
            (
                ".github/workflows/publish-mirror.yml",
                "name: Publish mirror\njobs:\n  publish:\n    steps:\n      - run: echo mirror\n",
            ),
            (
                ".github/workflows/publish-pypi.yml",
                "name: Publish pypi\njobs:\n  publish:\n    steps:\n      - run: maturin publish\n",
            ),
            (
                ".github/workflows/publish-versions.yml",
                "name: Publish versions\njobs:\n  publish:\n    steps:\n      - run: cargo metadata --format-version 1\n",
            ),
            (
                ".github/workflows/publish-wasm.yml",
                "name: Publish wasm\njobs:\n  publish:\n    steps:\n      - run: wasm-pack build\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap build flow command runner main".to_owned(),
            limit: 11,
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
        ranked_paths.iter().take(11).any(|path| {
            matches!(
                *path,
                "crates/ruff/src/main.rs"
                    | "crates/ruff_dev/src/main.rs"
                    | "crates/ruff_python_formatter/src/main.rs"
                    | "crates/ty/src/main.rs"
                    | "crates/ty_completion_bench/src/main.rs"
            )
        }),
        "a runtime entrypoint witness should remain visible under workflow crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(11)
            .any(|path| path.starts_with(".github/workflows/")),
        "workflow witnesses should remain visible for entrypoint build-flow queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_build_flow_queries_recover_bat_build_config_witnesses()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-bat-build-config-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"bat\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("rustfmt.toml", "edition = \"2021\"\n"),
            (
                ".github/workflows/CICD.yml",
                "name: CICD\njobs:\n  build:\n    steps:\n      - run: cargo build --locked\n",
            ),
            (
                ".github/workflows/require-changelog-for-PRs.yml",
                "name: Require changelog\njobs:\n  check:\n    steps:\n      - run: ./tests/scripts/license-checks.sh\n",
            ),
            (
                "src/lib.rs",
                "pub fn run() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "src/bin/bat/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            ("src/bin/bat/app.rs", "pub fn build_app() {}\n"),
            ("src/bin/bat/assets.rs", "pub fn build_assets() {}\n"),
            ("src/bin/bat/clap_app.rs", "pub fn clap_app() {}\n"),
            (
                "src/bin/bat/completions.rs",
                "pub fn generate_completions() {}\n",
            ),
            ("src/bin/bat/config.rs", "pub fn load_bat_config() {}\n"),
            ("src/config.rs", "pub struct RuntimeConfig;\n"),
            ("tests/scripts/license-checks.sh", "#!/bin/sh\necho check\n"),
            (
                "tests/examples/system_config/bat/config",
                "--theme=\"TwoDark\"\n",
            ),
            (
                "tests/syntax-tests/highlighted/Elixir/command.ex",
                "defmodule Command do\nend\n",
            ),
            (
                "tests/syntax-tests/highlighted/Go/main.go",
                "package main\nfunc main() {}\n",
            ),
            (
                "tests/syntax-tests/source/Elixir/command.ex",
                "defmodule Command do\nend\n",
            ),
            (
                "tests/syntax-tests/source/Go/main.go",
                "package main\nfunc main() {}\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main config cargo github workflow cicd require changelog".to_owned(),
                limit: 11,
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
        ranked_paths.iter().take(11).any(|path| {
            matches!(
                *path,
                "Cargo.toml"
                    | ".github/workflows/CICD.yml"
                    | ".github/workflows/require-changelog-for-PRs.yml"
            )
        }),
        "build-config entrypoint queries should recover a Cargo/workflow witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_queries_surface_build_workflow_configs_with_semantic_runtime()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-entrypoint-build-workflows-semantic");
    prepare_workspace(
        &root,
        &[
            (
                "src-tauri/src/main.rs",
                "fn main() {\n\
                     // entry point bootstrap build flow command runner main config\n\
                     let config = load_config();\n\
                     run_build_flow(config);\n\
                     }\n",
            ),
            (
                "src-tauri/src/proxy/config.rs",
                "pub struct ProxyConfig;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/lib.rs",
                "pub fn run() {\n// entry point bootstrap build flow command runner main config\n}\n",
            ),
            (
                "src-tauri/src/modules/config.rs",
                "pub struct ModuleConfig;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/models/config.rs",
                "pub struct ModelConfig;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/proxy/proxy_pool.rs",
                "pub struct ProxyPool;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/build.rs",
                "fn main() {\n tauri_build::build();\n}\n",
            ),
            (
                "src-tauri/src/commands/security.rs",
                "pub fn security_command() {\n// entry point bootstrap build flow command runner main config\n}\n",
            ),
            (
                ".github/workflows/deploy-pages.yml",
                "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Upload artifact\n\
                             run: echo upload build artifacts\n\
                           - name: Deploy to GitHub Pages\n\
                             run: echo deploy release pages\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n\
                             run: cargo build --release\n\
                           - name: Publish release artifacts\n\
                             run: echo publish release artifacts\n",
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
                "src-tauri/src/main.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/proxy/config.rs",
                0,
                vec![0.99, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/lib.rs",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/modules/config.rs",
                0,
                vec![0.97, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/models/config.rs",
                0,
                vec![0.96, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/proxy/proxy_pool.rs",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/build.rs",
                0,
                vec![0.94, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/commands/security.rs",
                0,
                vec![0.93, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                ".github/workflows/release.yml",
                0,
                vec![0.82, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                ".github/workflows/deploy-pages.yml",
                0,
                vec![0.81, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 8;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor_with_trace(
        SearchHybridQuery {
            query: "entry point bootstrap build flow command runner main config".to_owned(),
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
    let anchor_paths = output
        .ranked_anchors
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let grouped_paths = output
        .coverage_grouped_pool
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths.iter().take(8).any(|path| {
            matches!(
                *path,
                ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
            )
        }),
        "entrypoint/build-flow queries should keep a workflow config witness visible even under semantic runtime pressure: ranked={ranked_paths:?} witness={witness_paths:?} anchors={anchor_paths:?} grouped={grouped_paths:?} trace={:?}",
        output.post_selection_trace
    );

    cleanup_workspace(&root);
    Ok(())
}
