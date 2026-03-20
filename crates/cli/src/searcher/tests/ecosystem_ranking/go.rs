use super::*;

#[test]
fn hybrid_ranking_go_entrypoint_queries_surface_cmd_command_packages() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-go-cmd-entrypoint-witnesses");
    prepare_workspace(
        &root,
        &[
            ("cmd/frpc/main.go", "package main\nfunc main() {}\n"),
            ("cmd/frps/main.go", "package main\nfunc main() {}\n"),
            ("cmd/frps/root.go", "package frps\nfunc Execute() {}\n"),
            ("cmd/frps/verify.go", "package frps\nfunc Verify() {}\n"),
            ("cmd/frpc/sub/admin.go", "package sub\nfunc Admin() {}\n"),
            (
                "cmd/frpc/sub/nathole.go",
                "package sub\nfunc NatHole() {}\n",
            ),
            ("cmd/frpc/sub/proxy.go", "package sub\nfunc Proxy() {}\n"),
            ("cmd/frpc/sub/root.go", "package sub\nfunc Root() {}\n"),
            ("go.mod", "module github.com/example/frp\n"),
            ("go.sum", "github.com/example/dependency v1.0.0 h1:test\n"),
            (
                ".github/workflows/build-and-push-image.yml",
                "name: build and push\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                "pkg/config/legacy/server.go",
                "package legacy\nfunc Server() {}\n",
            ),
            (
                "pkg/config/v1/validation/server.go",
                "package validation\nfunc Server() {}\n",
            ),
            (
                "pkg/metrics/mem/server.go",
                "package mem\nfunc Server() {}\n",
            ),
            (
                "web/frpc/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/api/server.ts",
                "export const api = 'server';\n",
            ),
            (
                "web/frps/src/types/server.ts",
                "export const server = 'type';\n",
            ),
            (
                "web/frpc/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "web/frps/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "test/e2e/mock/server/httpserver/server.go",
                "package httpserver\nfunc Server() {}\n",
            ),
            (
                "test/e2e/mock/server/streamserver/server.go",
                "package streamserver\nfunc Server() {}\n",
            ),
            (
                "test/e2e/legacy/basic/server.go",
                "package basic\nfunc Server() {}\n",
            ),
            (
                "test/e2e/legacy/plugin/server.go",
                "package plugin\nfunc Server() {}\n",
            ),
            (
                "test/e2e/v1/basic/server.go",
                "package basic\nfunc Server() {}\n",
            ),
            (
                "test/e2e/v1/plugin/server.go",
                "package plugin\nfunc Server() {}\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap server api main cli command".to_owned(),
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
            .any(|path| path.starts_with("cmd/")),
        "go entrypoint queries should recover a cmd/ command witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_go_package_queries_surface_pkg_test_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-go-package-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "client/http/controller.go",
                "package http\nfunc Controller() {}\n",
            ),
            (
                "client/http/controller_test.go",
                "package http\nfunc TestController() {}\n",
            ),
            (
                "client/config_manager.go",
                "package client\nfunc ConfigManager() {}\n",
            ),
            (
                "client/config_manager_test.go",
                "package client\nfunc TestConfigManager() {}\n",
            ),
            (
                "client/proxy/proxy_manager.go",
                "package proxy\nfunc Manager() {}\n",
            ),
            (
                "client/visitor/visitor_manager.go",
                "package visitor\nfunc Manager() {}\n",
            ),
            (
                "pkg/config/source/aggregator_test.go",
                "package source\nfunc TestAggregator() {}\n",
            ),
            (
                "pkg/config/source/base_source_test.go",
                "package source\nfunc TestBaseSource() {}\n",
            ),
            (
                "pkg/config/source/config_source_test.go",
                "package source\nfunc TestConfigSource() {}\n",
            ),
            (
                "pkg/auth/oidc_test.go",
                "package auth\nfunc TestOIDC() {}\n",
            ),
            (
                "pkg/config/load_test.go",
                "package config\nfunc TestLoad() {}\n",
            ),
            (
                "pkg/config/source/aggregator.go",
                "package source\nfunc NewAggregator() {}\n",
            ),
            (
                "pkg/config/source/base_source.go",
                "package source\nfunc NewBaseSource() {}\n",
            ),
            (
                "pkg/config/source/clone.go",
                "package source\nfunc Clone() {}\n",
            ),
            ("pkg/config/flags.go", "package config\nfunc Flags() {}\n"),
            ("go.mod", "module github.com/example/frp\n"),
            ("go.sum", "github.com/example/dependency v1.0.0 h1:test\n"),
            ("cmd/frpc/sub/root.go", "package sub\nfunc Root() {}\n"),
            ("cmd/frps/root.go", "package frps\nfunc Execute() {}\n"),
            ("web/frpc/tsconfig.json", "{ \"compilerOptions\": {} }\n"),
            ("web/frps/tsconfig.json", "{ \"compilerOptions\": {} }\n"),
            (
                "web/frpc/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/api/server.ts",
                "export const api = 'frontend server';\n",
            ),
            (
                "web/frps/src/types/server.ts",
                "export const server = 'frontend type';\n",
            ),
            (
                "web/frpc/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "web/frps/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "test/e2e/legacy/basic/config.go",
                "package basic\nfunc Config() {}\n",
            ),
            ("package.sh", "#!/bin/sh\necho package\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests packages internal library integration config manager controller"
                .to_owned(),
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
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "pkg/config/source/aggregator_test.go"
                    | "pkg/config/source/base_source_test.go"
                    | "pkg/config/source/config_source_test.go"
                    | "pkg/auth/oidc_test.go"
                    | "pkg/config/load_test.go"
                    | "pkg/config/source/aggregator.go"
                    | "pkg/config/source/base_source.go"
                    | "pkg/config/source/clone.go"
            )
        }),
        "go package/test queries should recover a pkg/ witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_go_runtime_queries_prefer_implementation_over_tests_in_lexical_only_mode()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-go-runtime-vs-tests-lexical-only");
    prepare_workspace(
        &root,
        &[
            (
                "pkg/config/source_path.go",
                "package config\nfunc LoadSourcePathConfig() {} // source path startup config handled runtime cli\n",
            ),
            (
                "cmd/nimlsp/main.go",
                "package main\nfunc main() { LoadSourcePathConfig() } // source path startup config handled runtime cli\n",
            ),
            (
                "tests/source_path_test.go",
                "package tests\nfunc TestSourcePathConfig() {} // tests cover source path startup config\n",
            ),
            (
                "tests/config_test.go",
                "package tests\nfunc TestConfig() {} // tests cover source path startup config\n",
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
        .position(|path| matches!(*path, "pkg/config/source_path.go" | "cmd/nimlsp/main.go"))
        .expect("go runtime implementation should be ranked");
    let first_test_rank = ranked_paths
        .iter()
        .position(|path| path.starts_with("tests/"))
        .expect("go test should still be visible");

    assert!(
        runtime_rank < first_test_rank,
        "go runtime implementation should beat tests for handled queries in lexical-only mode: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_go_test_queries_prefer_tests_over_runtime_in_lexical_only_mode() -> FriggResult<()>
{
    let root = temp_workspace_root("hybrid-go-tests-vs-runtime-lexical-only");
    prepare_workspace(
        &root,
        &[
            (
                "pkg/config/source_path.go",
                "package config\nfunc LoadSourcePathConfig() {} // source path startup config handled runtime cli\n",
            ),
            (
                "cmd/nimlsp/main.go",
                "package main\nfunc main() { LoadSourcePathConfig() } // source path startup config handled runtime cli\n",
            ),
            (
                "tests/source_path_test.go",
                "package tests\nfunc TestSourcePathConfig() {} // tests cover source path startup config\n",
            ),
            (
                "tests/config_test.go",
                "package tests\nfunc TestConfig() {} // tests cover source path startup config\n",
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
        .expect("go test should be ranked");
    let runtime_rank = ranked_paths
        .iter()
        .position(|path| matches!(*path, "pkg/config/source_path.go" | "cmd/nimlsp/main.go"))
        .expect("go runtime implementation should remain visible");

    assert!(
        first_test_rank < runtime_rank,
        "go tests should beat runtime files for coverage queries in lexical-only mode: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
