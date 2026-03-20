use super::*;

#[test]
fn hybrid_ranking_roc_entrypoint_queries_prefer_platform_main_over_host_crates_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-roc-platform-entrypoints");
    prepare_workspace(
        &root,
        &[
            (
                "platform/main.roc",
                "# entry point main app package platform runtime\nplatform \"cli\"\npackages {}\nprovides [main_for_host!]\n",
            ),
            ("platform/Arg.roc", "# platform arg runtime package\n"),
            ("platform/Cmd.roc", "# platform cmd runtime package\n"),
            ("platform/Host.roc", "# platform host runtime package\n"),
            (
                "examples/command.roc",
                "# example command package\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            (
                "crates/roc_host_bin/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                "crates/roc_host/src/lib.rs",
                "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
            ),
            (
                "ci/rust_http_server/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                ".github/workflows/deploy-docs.yml",
                "name: deploy docs\njobs:\n  deploy:\n    steps:\n      - run: cargo doc\n",
            ),
            (
                ".github/workflows/test_latest_release.yml",
                "name: test latest release\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point main app package platform runtime".to_owned(),
            limit: 10,
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
    let platform_main_rank = ranked_paths
        .iter()
        .position(|path| *path == "platform/main.roc")
        .expect("platform/main.roc should be ranked for Roc entrypoint queries");
    let host_lib_rank = ranked_paths
        .iter()
        .position(|path| *path == "crates/roc_host/src/lib.rs")
        .expect("host runtime lib.rs should be ranked as competing noise");

    assert!(
        platform_main_rank < 6,
        "platform/main.roc should stay visible near the top for Roc platform queries: {ranked_paths:?}"
    );
    assert!(
        platform_main_rank < host_lib_rank,
        "platform/main.roc should outrank generic host runtime library noise for Roc platform queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_roc_mixed_entrypoint_example_queries_recover_example_witnesses_under_runtime_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-roc-entrypoints-with-example-hints");
    prepare_workspace(
        &root,
        &[
            (
                "platform/main.roc",
                "# entry point main app package platform runtime\nplatform \"cli\"\npackages {}\nprovides [main_for_host!]\n",
            ),
            ("platform/Arg.roc", "# platform arg runtime package\n"),
            ("platform/Cmd.roc", "# platform cmd runtime package\n"),
            ("platform/Host.roc", "# platform host runtime package\n"),
            ("platform/Stdin.roc", "# stdin bytes runtime package\n"),
            (
                "examples/command.roc",
                "# example command line package\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            (
                "examples/command-line-args.roc",
                "# command line args bytes stdin example\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            (
                "examples/bytes-stdin-stdout.roc",
                "# bytes stdin stdout example\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            ("tests/cmd-test.roc", "# tests command bytes stdin\n"),
            (
                "crates/roc_host_bin/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                "crates/roc_host/src/lib.rs",
                "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
            ),
            (
                "crates/roc_command/src/lib.rs",
                "pub fn command_runtime() { let _ = \"command runtime\"; }\n",
            ),
            (
                "crates/roc_env/src/lib.rs",
                "pub fn env_runtime() { let _ = \"env runtime\"; }\n",
            ),
            (
                "ci/rust_http_server/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "entry point main app package platform runtime tests bytes stdin command line examples benches benchmark"
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
                "platform/main.roc"
                    | "crates/roc_host_bin/src/main.rs"
                    | "ci/rust_http_server/src/main.rs"
            )
        }),
        "Roc mixed entrypoint/example queries should keep at least one entrypoint witness visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| path.starts_with("examples/")),
        "Roc mixed entrypoint/example queries should recover an example witness under runtime/test noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_roc_saved_wave_queries_prefer_specific_example_witnesses_over_temp_dir_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-roc-saved-wave-specific-example-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "platform/main.roc",
                "# entry point main app package platform runtime\nplatform \"web\"\npackages {}\nprovides [main_for_host!]\n",
            ),
            ("platform/Cmd.roc", "# platform cmd runtime package\n"),
            ("platform/Dir.roc", "# platform dir runtime package\n"),
            ("platform/Env.roc", "# platform env runtime package\n"),
            ("platform/File.roc", "# platform file runtime package\n"),
            ("platform/Host.roc", "# platform host runtime package\n"),
            (
                "examples/command.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Cmd\n# command example\n",
            ),
            (
                "examples/dir.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Dir\nimport pf.Env\n# examples directory listing\n",
            ),
            (
                "examples/env.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Env\n# environment example\n",
            ),
            (
                "examples/temp-dir.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Env\n# temp dir example\n",
            ),
            ("tests/cmd-test.roc", "# tests command integration\n"),
            (
                "crates/roc_host_bin/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                "crates/roc_host/src/lib.rs",
                "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
            ),
            (
                ".github/workflows/test_latest_release.yml",
                "name: test latest release\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
            ),
            (
                ".github/workflows/deploy-docs.yml",
                "name: deploy docs\njobs:\n  deploy:\n    steps:\n      - run: cargo doc\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "tests fixtures integration entry point main app package platform runtime command dir examples benches benchmark"
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
        ranked_paths
            .iter()
            .take(14)
            .any(|path| { matches!(*path, "examples/command.roc" | "examples/dir.roc") }),
        "saved-wave Roc entrypoint/example queries should recover a specific example witness instead of only broad temp-dir noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
