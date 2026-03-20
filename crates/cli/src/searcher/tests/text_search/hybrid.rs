use super::*;

#[test]
fn hybrid_path_witness_recall_supplements_manifest_with_hidden_workflows() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-hidden-workflow-supplement");
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
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "src-tauri/src/main.rs",
            "src-tauri/src/lib.rs",
            "src-tauri/src/proxy/config.rs",
            "src-tauri/src/modules/config.rs",
            "src-tauri/src/models/config.rs",
            "src-tauri/src/proxy/proxy_pool.rs",
            "src-tauri/src/commands/security.rs",
            "src-tauri/build.rs",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
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
    assert!(
        ranked_paths.iter().take(8).any(|path| {
            matches!(
                *path,
                ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
            )
        }),
        "manifest-backed path recall should still surface hidden GitHub workflow build configs in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_keeps_hidden_ci_workflows_for_entrypoint_build_config_queries()
-> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-hidden-ci-workflow-supplement");
    prepare_workspace(
        &root,
        &[
            (
                "src/bin/tool/main.rs",
                "mod app;\nfn main() { app::run(); }\n",
            ),
            ("src/bin/tool/app.rs", "pub fn run() {}\n"),
            (
                ".github/workflows/CICD.yml",
                "name: CI\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
            ),
            (
                ".github/workflows/require-changelog-for-PRs.yml",
                "name: Require changelog\njobs:\n  changelog:\n    steps:\n      - run: ./scripts/check-changelog.sh\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["src/bin/tool/main.rs", "src/bin/tool/app.rs"],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "entry point bootstrap build flow command runner main config cargo github workflow cicd require changelog"
                        .to_owned(),
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
                ".github/workflows/CICD.yml" | ".github/workflows/require-changelog-for-PRs.yml"
            )
        }),
        "entrypoint build-config queries should retain generic hidden CI workflows in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_lexical_expansion_repeated_runs_retain_runtime_docs_and_tests_under_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-lexical-rescan-bounded-retention");
    prepare_workspace(
        &root,
        &[
            (
                "src/runtime_helper.rs",
                "pub fn invalid_params_runtime_helper() {\n\
                 let code = \"invalid_params\";\n\
                 let category = \"typed error\";\n\
                 }\n",
            ),
            (
                "tests/runtime_helper_tests.rs",
                "#[test]\n\
                 fn invalid_params_runtime_helper_tests() {\n\
                 // invalid_params typed error runtime helper tests\n\
                 }\n",
            ),
            (
                "contracts/errors.md",
                "# Public error taxonomy\n\
                 invalid_params typed error public docs runtime helper tests\n",
            ),
        ],
    )?;

    fs::create_dir_all(root.join("docs/noise")).map_err(FriggError::Io)?;
    for index in 0..10 {
        let rel_path = format!("docs/noise/error-guide-{index:02}.md");
        let content = format!(
            "# Error guide {index:02}\n\
             public docs invalid_params helper typed error reference\n"
        );
        fs::write(root.join(rel_path), content).map_err(FriggError::Io)?;
    }

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let query = SearchHybridQuery {
        query: "trace invalid_params typed error from public docs to runtime helper and tests"
            .to_owned(),
        limit: 3,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    };

    let first = searcher.search_hybrid_with_filters_using_executor(
        query.clone(),
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    let second = searcher.search_hybrid_with_filters_using_executor(
        query,
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = first
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(
        first.matches, second.matches,
        "repeated lexical expansion runs should preserve deterministic hybrid ordering"
    );
    assert_eq!(
        first.diagnostics.entries, second.diagnostics.entries,
        "repeated lexical expansion runs should preserve deterministic diagnostics"
    );
    assert!(
        ranked_paths.contains(&"src/runtime_helper.rs"),
        "runtime helper witness should remain in top-k under lexical crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.contains(&"tests/runtime_helper_tests.rs"),
        "test witness should remain in top-k under lexical crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.contains(&"contracts/errors.md"),
        "public docs witness should remain in top-k under lexical crowding: {ranked_paths:?}"
    );
    let stage_attribution = first
        .stage_attribution
        .as_ref()
        .expect("lexical crowding regression should expose stage attribution");
    assert!(
        stage_attribution.scan.output_count > first.matches.len(),
        "lexical crowding regression should scan a broader lexical pool than the retained top-k: {stage_attribution:?}"
    );
    assert_eq!(
        stage_attribution.final_diversification.output_count,
        first.matches.len(),
        "final diversification should respect the requested top-k bound"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_manifest_backed_lua_entrypoint_queries_recover_repo_root_runtime_config()
-> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-lua-root-config-supplement");
    prepare_workspace(
        &root,
        &[
            (
                ".luarc.json",
                "{\n  \"runtime\": { \"version\": \"Lua 5.5\" }\n}\n",
            ),
            (
                "lua-language-server-scm-1.rockspec",
                "package = 'lua-language-server'\nversion = 'scm-1'\n",
            ),
            ("main.lua", "require 'cli'\nrequire 'service'\n"),
            (
                "script/cli/init.lua",
                "if _G['CHECK'] then require 'cli.check' end\nif _G['HELP'] then require 'cli.help' end\n",
            ),
            (
                "script/cli/check.lua",
                "local M = {}\nfunction M.runCLI() end\nreturn M\n",
            ),
            ("script/cli/help.lua", "return function() end\n"),
            ("script/cli/doc/export.lua", "return function() end\n"),
            ("script/service/init.lua", "return require 'service'\n"),
            ("test/command/init.lua", "require 'command.auto-require'\n"),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "main.lua",
            "script/cli/init.lua",
            "script/cli/check.lua",
            "script/cli/help.lua",
            "script/cli/doc/export.lua",
            "script/service/init.lua",
            "test/command/init.lua",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap init cli command runtime server".to_owned(),
            limit: 14,
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
            .take(14)
            .any(|path| { matches!(*path, ".luarc.json" | "lua-language-server-scm-1.rockspec") }),
        "manifest-backed Lua entrypoint queries should keep a repo-root runtime config visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "script/cli/init.lua"
                    | "script/cli/check.lua"
                    | "script/cli/help.lua"
                    | "script/cli/doc/export.lua"
            )
        }),
        "manifest-backed Lua entrypoint queries should still keep a CLI runtime entrypoint visible: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_manifest_backed_lua_entrypoint_queries_recover_root_runtime_config_with_language_filter()
-> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-lua-root-config-language-filter");
    prepare_workspace(
        &root,
        &[
            (
                ".luarc.json",
                "{\n  \"runtime\": { \"version\": \"Lua 5.5\" }\n}\n",
            ),
            (
                "lua-language-server-scm-1.rockspec",
                "package = 'lua-language-server'\nversion = 'scm-1'\n",
            ),
            ("main.lua", "require 'cli'\nrequire 'service'\n"),
            (
                "script/cli/init.lua",
                "if _G['CHECK'] then require 'cli.check' end\nif _G['HELP'] then require 'cli.help' end\n",
            ),
            (
                "script/cli/check.lua",
                "local M = {}\nfunction M.runCLI() end\nreturn M\n",
            ),
            ("script/cli/help.lua", "return function() end\n"),
            ("script/cli/doc/export.lua", "return function() end\n"),
            ("script/service/init.lua", "return require 'service'\n"),
            ("test/command/init.lua", "require 'command.auto-require'\n"),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "main.lua",
            "script/cli/init.lua",
            "script/cli/check.lua",
            "script/cli/help.lua",
            "script/cli/doc/export.lua",
            "script/service/init.lua",
            "test/command/init.lua",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap init cli command runtime server".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters {
            language: Some("lua".to_owned()),
            ..SearchFilters::default()
        },
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
            .take(14)
            .any(|path| { matches!(*path, ".luarc.json" | "lua-language-server-scm-1.rockspec") }),
        "language-filtered Lua entrypoint queries should still keep repo-root runtime config visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "script/cli/init.lua"
                    | "script/cli/check.lua"
                    | "script/cli/help.lua"
                    | "script/cli/doc/export.lua"
            )
        }),
        "language-filtered Lua entrypoint queries should still keep a CLI runtime entrypoint visible: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_manifest_backed_android_entrypoint_queries_recover_root_scoped_gradle_config()
-> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-android-root-config-supplement");
    prepare_workspace(
        &root,
        &[
            (
                "gradle/wrapper/gradle-wrapper.properties",
                "distributionUrl=https\\://services.gradle.org/distributions/gradle-8.6-bin.zip\n",
            ),
            (
                "app/build.gradle.kts",
                "plugins { id(\"com.android.application\") }\n",
            ),
            (
                "app/src/main/AndroidManifest.xml",
                "<manifest package=\"com.example.todoapp\" />\n",
            ),
            (
                "app/src/main/java/com/example/android/todoapp/TodoActivity.kt",
                "class TodoActivity\n",
            ),
            (
                "app/src/main/java/com/example/android/todoapp/TodoApplication.kt",
                "class TodoApplication\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "app/src/main/AndroidManifest.xml",
            "app/src/main/java/com/example/android/todoapp/TodoActivity.kt",
            "app/src/main/java/com/example/android/todoapp/TodoApplication.kt",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap app activity navigation main".to_owned(),
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
            .any(|path| *path == "gradle/wrapper/gradle-wrapper.properties"),
        "manifest-backed Android entrypoint queries should keep a root-scoped Gradle config visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(12).any(|path| {
            matches!(
                *path,
                "app/src/main/java/com/example/android/todoapp/TodoActivity.kt"
                    | "app/src/main/java/com/example/android/todoapp/TodoApplication.kt"
            )
        }),
        "manifest-backed Android entrypoint queries should still keep an Android startup witness visible: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
