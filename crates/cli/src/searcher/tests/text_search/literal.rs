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
