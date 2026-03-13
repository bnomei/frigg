use super::*;

#[test]
fn literal_search_returns_sorted_deterministic_matches() -> FriggResult<()> {
    let root_a = temp_workspace_root("literal-search-sort-a");
    let root_b = temp_workspace_root("literal-search-sort-b");
    prepare_workspace(
        &root_a,
        &[
            ("zeta.txt", "needle zeta\n"),
            ("alpha.txt", "needle alpha\nnext needle\n"),
        ],
    )?;
    prepare_workspace(&root_b, &[("beta.txt", "beta needle\n")])?;

    let config = FriggConfig::from_workspace_roots(vec![root_b.clone(), root_a.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 100,
    };

    let first = searcher.search(query.clone())?;
    let second = searcher.search(query)?;
    assert_eq!(first, second);
    assert_eq!(
        first,
        vec![
            text_match("repo-001", "beta.txt", 1, 6, "beta needle"),
            text_match("repo-002", "alpha.txt", 1, 1, "needle alpha"),
            text_match("repo-002", "alpha.txt", 2, 6, "next needle"),
            text_match("repo-002", "zeta.txt", 1, 1, "needle zeta"),
        ]
    );

    cleanup_workspace(&root_a);
    cleanup_workspace(&root_b);
    Ok(())
}

#[test]
fn literal_search_walk_fallback_respects_gitignored_contract_artifacts() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-gitignored-contracts");
    prepare_workspace(&root, &[("contracts/errors.md", "invalid_params\n")])?;
    fs::write(root.join(".gitignore"), "contracts\n").map_err(FriggError::Io)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let matches = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "invalid_params".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters::default(),
    )?;

    assert!(
        matches
            .iter()
            .all(|entry| entry.path != "contracts/errors.md"),
        "walk fallback should respect gitignored contract artifacts"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_scrubs_generic_markdown_leading_comment_metadata() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-markdown-leading-comment");
    prepare_workspace(
        &root,
        &[(
            "docs/guide.md",
            "<!-- hidden metadata secret-token -->\n# Guide\npublic content\n",
        )],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let hidden = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "secret-token".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters::default(),
    )?;
    assert!(
        hidden.is_empty(),
        "leading markdown comment metadata should not pollute literal search: {:?}",
        hidden
    );

    let public = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "public".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].path, "docs/guide.md");
    assert_eq!(public[0].line, 3);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_walk_fallback_excludes_target_artifacts_without_gitignore() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-target-exclusion");
    prepare_workspace(
        &root,
        &[
            ("src/main.rs", "needle\n"),
            ("target/debug/app", "needle\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let matches = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters::default(),
    )?;

    assert!(
        matches
            .iter()
            .all(|entry| !entry.path.starts_with("target/")),
        "walk fallback must not search target artifacts: {matches:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_walk_fallback_respects_root_ignore_file_for_auxiliary_trees() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-root-ignore");
    prepare_workspace(
        &root,
        &[
            ("src/main.rs", "needle main\n"),
            ("auxiliary/embedded-repo/src/lib.rs", "needle auxiliary\n"),
        ],
    )?;
    fs::write(root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let matches = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(
        matches,
        vec![text_match("repo-001", "src/main.rs", 1, 1, "needle main")]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_applies_path_regex_filter() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-path-filter");
    prepare_workspace(
        &root,
        &[
            ("src/lib.rs", "needle here\n"),
            ("README.md", "needle docs\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let query =
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*\.rs$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 100,
        };

    let matches = searcher.search(query)?;
    assert_eq!(
        matches,
        vec![text_match("repo-001", "src/lib.rs", 1, 1, "needle here")]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_applies_repository_filter_and_limit_after_sorting() -> FriggResult<()> {
    let root_a = temp_workspace_root("literal-search-repo-filter-a");
    let root_b = temp_workspace_root("literal-search-repo-filter-b");
    prepare_workspace(&root_a, &[("a.txt", "needle a\nneedle aa\n")])?;
    prepare_workspace(&root_b, &[("b.txt", "needle b\nneedle bb\n")])?;

    let config = FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 1,
    };

    let matches = searcher.search_literal(query, Some("repo-002"))?;
    assert_eq!(
        matches,
        vec![text_match("repo-002", "b.txt", 1, 1, "needle b")]
    );

    cleanup_workspace(&root_a);
    cleanup_workspace(&root_b);
    Ok(())
}

#[test]
fn literal_search_small_limit_matches_sorted_prefix_of_full_results() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-small-limit-prefix");
    prepare_workspace(
        &root,
        &[
            ("z.txt", "needle zeta\n"),
            ("a.txt", "needle alpha\nneedle again\n"),
            ("nested/b.txt", "prefix needle\nneedle suffix\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let full_query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 100,
    };
    let limited_query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 3,
    };

    let full = searcher.search_literal_with_filters(full_query, SearchFilters::default())?;
    let first_limited =
        searcher.search_literal_with_filters(limited_query.clone(), SearchFilters::default())?;
    let second_limited =
        searcher.search_literal_with_filters(limited_query, SearchFilters::default())?;

    assert_eq!(first_limited, second_limited);
    assert_eq!(
        first_limited,
        full.into_iter().take(3).collect::<Vec<_>>(),
        "limited search should match deterministic sorted prefix"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn diagnostics_literal_search_reports_read_failures_deterministically() -> FriggResult<()> {
    let root = temp_workspace_root("literal-search-diagnostics-read-failure");
    fs::create_dir_all(root.join("src")).map_err(FriggError::Io)?;
    fs::write(
        root.join("src/good.rs"),
        "pub fn hotspot() { let _ = \"needle_hotspot\"; }\n",
    )
    .map_err(FriggError::Io)?;
    fs::write(root.join("src/bad.rs"), [0xff, b'\n']).map_err(FriggError::Io)?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: "needle_hotspot".to_owned(),
        path_regex: None,
        limit: 20,
    };

    let first = searcher
        .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
    let second =
        searcher.search_literal_with_filters_diagnostics(query, SearchFilters::default())?;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].repository_id, "repo-001");
    assert_eq!(first.matches[0].path, "src/good.rs");

    assert_eq!(first.diagnostics.entries, second.diagnostics.entries);
    assert_eq!(first.diagnostics.total_count(), 1);
    assert_eq!(
        first.diagnostics.count_by_kind(SearchDiagnosticKind::Read),
        1
    );
    assert_eq!(
        first.diagnostics.count_by_kind(SearchDiagnosticKind::Walk),
        0
    );
    assert_eq!(first.diagnostics.entries[0].repository_id, "repo-001");
    assert_eq!(
        first.diagnostics.entries[0].path.as_deref(),
        Some("src/bad.rs")
    );
    assert_eq!(
        first.diagnostics.entries[0].kind,
        SearchDiagnosticKind::Read
    );
    assert!(
        !first.diagnostics.entries[0].message.is_empty(),
        "diagnostic message should be populated for read failures"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_reuses_validated_manifest_candidates_across_repeated_queries() -> FriggResult<()>
{
    let root = temp_workspace_root("literal-search-manifest-cache-hit");
    prepare_workspace(
        &root,
        &[("src/lib.rs", "pub fn cached() { let _ = \"needle\"; }\n")],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;

    let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let searcher = TextSearcher::with_validated_manifest_candidate_cache(
        FriggConfig::from_workspace_roots(vec![root.clone()])?,
        Arc::clone(&cache),
    );
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 10,
    };

    let first = searcher
        .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
    let second =
        searcher.search_literal_with_filters_diagnostics(query, SearchFilters::default())?;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.matches.len(), 1);
    let stats = cache
        .read()
        .expect("validated manifest candidate cache should not be poisoned")
        .stats();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.dirty_bypasses, 0);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_dirty_validated_manifest_cache_falls_back_to_walk_for_new_files()
-> FriggResult<()> {
    let root = temp_workspace_root("literal-search-manifest-cache-dirty");
    prepare_workspace(&root, &[("src/lib.rs", "pub fn cached() {}\n")])?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;

    let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let searcher = TextSearcher::with_validated_manifest_candidate_cache(
        FriggConfig::from_workspace_roots(vec![root.clone()])?,
        Arc::clone(&cache),
    );
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 10,
    };

    let first = searcher
        .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
    assert_eq!(first.matches.len(), 0);

    prepare_workspace(
        &root,
        &[("src/new.rs", "pub fn fresh() { let _ = \"needle\"; }\n")],
    )?;
    cache
        .write()
        .expect("validated manifest candidate cache should not be poisoned")
        .mark_dirty_root(&root);

    let second =
        searcher.search_literal_with_filters_diagnostics(query, SearchFilters::default())?;

    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].path, "src/new.rs");
    let stats = cache
        .read()
        .expect("validated manifest candidate cache should not be poisoned")
        .stats();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.dirty_bypasses, 1);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn candidate_discovery_prefers_manifest_snapshot_across_search_modes() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-prefers-manifest");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/live_only.rs", "needle live-only\n"),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);

    let literal = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        literal,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    let regex = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle\s+\w+".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        regex,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    let hybrid = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 20,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(hybrid.matches.len(), 1);
    assert_eq!(hybrid.matches[0].document.path, "src/indexed.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn candidate_discovery_manifest_snapshot_respects_root_ignore_file() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-manifest-ignore");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("auxiliary/embedded-repo/src/lib.rs", "needle auxiliary\n"),
        ],
    )?;
    fs::write(root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["src/indexed.rs", "auxiliary/embedded-repo/src/lib.rs"],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);

    let literal = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        literal,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    let hybrid = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 20,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(hybrid.matches.len(), 1);
    assert_eq!(hybrid.matches[0].document.path, "src/indexed.rs");

    cleanup_workspace(&root);
    Ok(())
}

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

#[test]
fn candidate_discovery_rebuilds_after_stale_manifest_snapshot() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-stale-manifest");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/live_only.rs", "needle live-only\n"),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);

    let first = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        first,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    rewrite_file_with_new_mtime(&root.join("src/indexed.rs"), "changed\n")?;

    let literal = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;
    assert_eq!(
        literal,
        vec![text_match(
            "repo-001",
            "src/live_only.rs",
            1,
            1,
            "needle live-only"
        )]
    );

    let hybrid = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle".to_owned(),
            limit: 20,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;
    assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
    assert_eq!(hybrid.matches.len(), 1);
    assert_eq!(hybrid.matches[0].document.path, "src/live_only.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn candidate_discovery_falls_back_to_repository_walk_without_manifest() -> FriggResult<()> {
    let root = temp_workspace_root("candidate-discovery-fallback-walk");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/live_only.rs", "needle live-only\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let matches = searcher.search_literal_with_filters(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(
        matches,
        vec![
            text_match("repo-001", "src/indexed.rs", 1, 1, "needle indexed"),
            text_match("repo-001", "src/live_only.rs", 1, 1, "needle live-only"),
        ]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_low_limit_large_corpus_matches_sorted_prefix_deterministically() -> FriggResult<()>
{
    const FILE_COUNT: usize = 96;
    const LIMIT: usize = 5;

    let root = temp_workspace_root("literal-search-large-corpus-low-limit");
    fs::create_dir_all(root.join("src/nested")).map_err(FriggError::Io)?;
    for file_idx in 0..FILE_COUNT {
        let relative = if file_idx % 2 == 0 {
            format!("src/file_{file_idx:03}.rs")
        } else {
            format!("src/nested/file_{file_idx:03}.rs")
        };
        let mut lines = Vec::with_capacity(40);
        lines.push(format!(
            "// deterministic large-corpus fixture file={file_idx:03}"
        ));
        for line_idx in 0..36 {
            if line_idx % 4 == 0 {
                lines.push(format!(
                    "let hotspot_{line_idx:03} = \"needle_hotspot {file_idx} {line_idx}\";"
                ));
            } else {
                lines.push(format!(
                    "let filler_{line_idx:03} = {};",
                    file_idx + line_idx
                ));
            }
        }
        fs::write(root.join(relative), lines.join("\n")).map_err(FriggError::Io)?;
    }

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let full_query = SearchTextQuery {
        query: "needle_hotspot".to_owned(),
        path_regex: None,
        limit: 10_000,
    };
    let limited_query = SearchTextQuery {
        query: "needle_hotspot".to_owned(),
        path_regex: None,
        limit: LIMIT,
    };

    let full = searcher.search_literal_with_filters(full_query, SearchFilters::default())?;
    let first_limited =
        searcher.search_literal_with_filters(limited_query.clone(), SearchFilters::default())?;
    let second_limited =
        searcher.search_literal_with_filters(limited_query, SearchFilters::default())?;

    assert_eq!(first_limited.len(), LIMIT);
    assert_eq!(first_limited, second_limited);
    assert_eq!(
        first_limited,
        full.into_iter().take(LIMIT).collect::<Vec<_>>(),
        "low-limit search should stay equal to deterministic sorted prefix on large corpus"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn regex_search_returns_sorted_deterministic_matches() -> FriggResult<()> {
    let root_a = temp_workspace_root("regex-search-sort-a");
    let root_b = temp_workspace_root("regex-search-sort-b");
    prepare_workspace(
        &root_a,
        &[
            ("zeta.txt", "needle zeta\n"),
            ("alpha.txt", "needle alpha\nnext needle\n"),
        ],
    )?;
    prepare_workspace(&root_b, &[("beta.txt", "beta needle\n")])?;

    let config = FriggConfig::from_workspace_roots(vec![root_b.clone(), root_a.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: r"needle\s+\w+".to_owned(),
        path_regex: None,
        limit: 100,
    };

    let first = searcher.search_regex(query.clone(), None)?;
    let second = searcher.search_regex(query, None)?;
    assert_eq!(first, second);
    assert_eq!(
        first,
        vec![
            text_match("repo-002", "alpha.txt", 1, 1, "needle alpha"),
            text_match("repo-002", "zeta.txt", 1, 1, "needle zeta"),
        ]
    );

    cleanup_workspace(&root_a);
    cleanup_workspace(&root_b);
    Ok(())
}

#[test]
fn regex_search_applies_repository_and_path_filters() -> FriggResult<()> {
    let root_a = temp_workspace_root("regex-search-filter-a");
    let root_b = temp_workspace_root("regex-search-filter-b");
    prepare_workspace(
        &root_a,
        &[
            ("src/lib.rs", "needle 123\n"),
            ("README.md", "needle docs\n"),
        ],
    )?;
    prepare_workspace(
        &root_b,
        &[
            ("src/main.rs", "needle 999\n"),
            ("README.md", "needle docs\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])?;
    let searcher = TextSearcher::new(config);
    let query =
        SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*\.rs$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 10,
        };

    let matches = searcher.search_regex(query, Some("repo-002"))?;
    assert_eq!(
        matches,
        vec![text_match("repo-002", "src/main.rs", 1, 1, "needle 999")]
    );

    cleanup_workspace(&root_a);
    cleanup_workspace(&root_b);
    Ok(())
}

#[test]
fn regex_prefilter_plan_extracts_required_literals_for_safe_patterns() {
    let plan = build_regex_prefilter_plan(r"needle\s+\d+")
        .expect("safe regex pattern should produce a deterministic prefilter plan");
    assert_eq!(plan.required_literals(), vec!["needle"]);
    assert!(plan.file_may_match("prefix needle 42 suffix"));
    assert!(!plan.file_may_match("prefix token 42 suffix"));
}

#[test]
fn regex_prefilter_plan_falls_back_for_unsupported_constructs() {
    assert!(build_regex_prefilter_plan(r"(needle|token)\s+\d+").is_none());
}

#[test]
fn regex_prefilter_matches_unfiltered_baseline_without_false_negatives() -> FriggResult<()> {
    let root = temp_workspace_root("regex-prefilter-baseline-equivalence");
    prepare_workspace(
        &root,
        &[
            ("src/a.rs", "needle 100\nneedle words\n"),
            ("src/b.rs", "prefix needle 300 suffix\n"),
            ("src/c.rs", "completely unrelated\n"),
            ("src/nested/d.rs", "needle 101\nneedle 102\n"),
            ("README.md", "needle 999\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let query =
        SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*\.rs$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 3,
        };

    assert!(
        build_regex_prefilter_plan(&query.query).is_some(),
        "expected prefilter plan for deterministic baseline comparison"
    );

    let accelerated =
        searcher.search_regex_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
    let accelerated_again =
        searcher.search_regex_with_filters_diagnostics(query.clone(), SearchFilters::default())?;

    let matcher = compile_safe_regex(&query.query)
        .map_err(|err| FriggError::InvalidInput(format!("test regex compile failed: {err}")))?;
    let normalized_filters = normalize_search_filters(SearchFilters::default())?;
    let baseline = searcher.search_with_matcher(
        &query,
        &normalized_filters,
        |_| true,
        |line, columns| {
            columns.clear();
            columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
        },
    )?;

    assert_eq!(accelerated.matches, accelerated_again.matches);
    assert_eq!(accelerated.matches, baseline.matches);
    assert_eq!(
        accelerated.diagnostics.entries,
        baseline.diagnostics.entries
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn regex_prefilter_keeps_repeated_literal_quantifier_matches() -> FriggResult<()> {
    let root = temp_workspace_root("regex-prefilter-repeated-quantifier");
    prepare_workspace(
        &root,
        &[("src/lib.rs", "abbc\nabc\n"), ("src/other.rs", "noise\n")],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: r"ab{2}c".to_owned(),
        path_regex: None,
        limit: 20,
    };

    let matches = searcher.search_regex_with_filters(query, SearchFilters::default())?;
    assert_eq!(
        matches,
        vec![text_match("repo-001", "src/lib.rs", 1, 1, "abbc")]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn regex_search_rejects_invalid_pattern_with_typed_error() -> FriggResult<()> {
    let compile_error = compile_safe_regex("(unterminated");
    assert!(matches!(
        compile_error,
        Err(RegexSearchError::InvalidRegex(_))
    ));

    let root = temp_workspace_root("regex-search-invalid");
    prepare_workspace(&root, &[("a.txt", "text\n")])?;
    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: "(unterminated".to_owned(),
        path_regex: None,
        limit: 10,
    };

    let search_error = searcher
        .search_regex(query, None)
        .expect_err("invalid regex pattern should fail");
    let error_message = search_error.to_string();
    assert!(
        error_message.contains("regex_invalid_pattern"),
        "unexpected regex invalid error: {error_message}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn regex_search_rejects_abusive_pattern_length_with_typed_error() {
    let abusive = "a".repeat(MAX_REGEX_PATTERN_BYTES + 1);
    let result = compile_safe_regex(&abusive);
    assert!(matches!(
        result,
        Err(RegexSearchError::PatternTooLong { .. })
    ));
}

#[test]
fn security_regex_search_rejects_abusive_alternations_with_typed_error() {
    let terms = (0..(MAX_REGEX_ALTERNATIONS + 2))
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>();
    let abusive = terms.join("|");
    let result = compile_safe_regex(&abusive);
    assert!(matches!(
        result,
        Err(RegexSearchError::TooManyAlternations { .. })
    ));
}

#[test]
fn security_regex_search_rejects_abusive_groups_with_typed_error() {
    let abusive = "(needle)".repeat(MAX_REGEX_GROUPS + 1);
    let result = compile_safe_regex(&abusive);
    assert!(matches!(
        result,
        Err(RegexSearchError::TooManyGroups { .. })
    ));
}

#[test]
fn security_regex_search_rejects_abusive_quantifiers_with_typed_error() {
    let abusive = "needle+".repeat(MAX_REGEX_QUANTIFIERS + 1);
    let result = compile_safe_regex(&abusive);
    assert!(matches!(
        result,
        Err(RegexSearchError::TooManyQuantifiers { .. })
    ));
}

#[test]
fn security_regex_search_maps_abuse_to_typed_invalid_input_error() -> FriggResult<()> {
    let root = temp_workspace_root("security-regex-abuse");
    prepare_workspace(&root, &[("src/lib.rs", "needle 1\n")])?;
    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let abusive = "needle+".repeat(MAX_REGEX_QUANTIFIERS + 1);
    let query = SearchTextQuery {
        query: abusive,
        path_regex: None,
        limit: 5,
    };

    let error = searcher
        .search_regex(query, None)
        .expect_err("abusive regex should fail with typed invalid-input error");
    assert!(
        error.to_string().contains("regex_too_many_quantifiers"),
        "unexpected abuse regex error: {error}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn ordering_literal_search_repeated_runs_are_identical() -> FriggResult<()> {
    let root_a = temp_workspace_root("ordering-literal-a");
    let root_b = temp_workspace_root("ordering-literal-b");
    prepare_workspace(
        &root_a,
        &[("z.txt", "needle z\n"), ("a.txt", "x needle\ny needle\n")],
    )?;
    prepare_workspace(&root_b, &[("b.txt", "needle b\n")])?;

    let config = FriggConfig::from_workspace_roots(vec![root_b.clone(), root_a.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 100,
    };

    let first = searcher.search_literal_with_filters(query.clone(), SearchFilters::default())?;
    let second = searcher.search_literal_with_filters(query, SearchFilters::default())?;
    assert_eq!(first, second);

    cleanup_workspace(&root_a);
    cleanup_workspace(&root_b);
    Ok(())
}

#[test]
fn ordering_regex_search_repeated_runs_are_identical() -> FriggResult<()> {
    let root = temp_workspace_root("ordering-regex");
    prepare_workspace(
        &root,
        &[
            ("src/lib.rs", "needle 1\nneedle 2\n"),
            ("README.md", "needle docs\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    let searcher = TextSearcher::new(config);
    let query = SearchTextQuery {
        query: r"needle\s+\d+".to_owned(),
        path_regex: None,
        limit: 100,
    };

    let first = searcher.search_regex_with_filters(query.clone(), SearchFilters::default())?;
    let second = searcher.search_regex_with_filters(query, SearchFilters::default())?;
    assert_eq!(first, second);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn ordering_filter_normalization_applies_repo_path_and_language() -> FriggResult<()> {
    let root_a = temp_workspace_root("ordering-filters-a");
    let root_b = temp_workspace_root("ordering-filters-b");
    prepare_workspace(
        &root_a,
        &[
            ("src/lib.rs", "needle 1\n"),
            ("src/lib.php", "needle 2\n"),
            ("src/lib.tsx", "needle 3\n"),
            ("src/lib.py", "needle 4\n"),
            ("src/lib.go", "needle 5\n"),
            ("src/lib.kts", "needle 6\n"),
            ("src/lib.lua", "needle 7\n"),
            ("src/lib.roc", "needle 8\n"),
            ("src/lib.nims", "needle 14\n"),
        ],
    )?;
    prepare_workspace(
        &root_b,
        &[
            ("src/main.rs", "needle 9\n"),
            ("src/main.php", "needle 10\n"),
            ("src/main.ts", "needle 11\n"),
            ("src/main.py", "needle 12\n"),
            ("src/main.go", "needle 13\n"),
            ("src/main.kt", "needle 15\n"),
            ("src/main.lua", "needle 16\n"),
            ("src/main.roc", "needle 17\n"),
            ("src/main.nim", "needle 18\n"),
        ],
    )?;

    let config = FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])?;
    let searcher = TextSearcher::new(config);
    let query =
        SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 100,
        };

    let matches = searcher.search_regex_with_filters(
        query,
        SearchFilters {
            repository_id: Some("  repo-002  ".to_owned()),
            language: Some("  RS ".to_owned()),
        },
    )?;
    assert_eq!(
        matches,
        vec![text_match("repo-002", "src/main.rs", 1, 1, "needle 9")]
    );

    let typescript_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("tsx".to_owned()),
        },
    )?;
    assert_eq!(
        typescript_matches,
        vec![
            text_match("repo-001", "src/lib.tsx", 1, 1, "needle 3"),
            text_match("repo-002", "src/main.ts", 1, 1, "needle 11"),
        ]
    );

    let python_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("py".to_owned()),
        },
    )?;
    assert_eq!(
        python_matches,
        vec![
            text_match("repo-001", "src/lib.py", 1, 1, "needle 4"),
            text_match("repo-002", "src/main.py", 1, 1, "needle 12"),
        ]
    );

    let go_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("golang".to_owned()),
        },
    )?;
    assert_eq!(
        go_matches,
        vec![
            text_match("repo-001", "src/lib.go", 1, 1, "needle 5"),
            text_match("repo-002", "src/main.go", 1, 1, "needle 13"),
        ]
    );

    let kotlin_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("kt".to_owned()),
        },
    )?;
    assert_eq!(
        kotlin_matches,
        vec![
            text_match("repo-001", "src/lib.kts", 1, 1, "needle 6"),
            text_match("repo-002", "src/main.kt", 1, 1, "needle 15"),
        ]
    );

    let lua_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("lua".to_owned()),
        },
    )?;
    assert_eq!(
        lua_matches,
        vec![
            text_match("repo-001", "src/lib.lua", 1, 1, "needle 7"),
            text_match("repo-002", "src/main.lua", 1, 1, "needle 16"),
        ]
    );

    let roc_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("roc".to_owned()),
        },
    )?;
    assert_eq!(
        roc_matches,
        vec![
            text_match("repo-001", "src/lib.roc", 1, 1, "needle 8"),
            text_match("repo-002", "src/main.roc", 1, 1, "needle 17"),
        ]
    );

    let nim_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("nim".to_owned()),
        },
    )?;
    assert_eq!(
        nim_matches,
        vec![
            text_match("repo-001", "src/lib.nims", 1, 1, "needle 14"),
            text_match("repo-002", "src/main.nim", 1, 1, "needle 18"),
        ]
    );

    let unsupported_language = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("java".to_owned()),
        },
    );
    let err = unsupported_language.expect_err("unsupported language filter should fail");
    assert!(
        err.to_string().contains("unsupported language filter"),
        "unexpected unsupported-language error: {err}"
    );

    cleanup_workspace(&root_a);
    cleanup_workspace(&root_b);
    Ok(())
}
