use super::*;

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
            ("src/lib.java", "needle 19\n"),
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
            ("src/main.java", "needle 20\n"),
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

    let java_matches = searcher.search_regex_with_filters(
        SearchTextQuery {
            query: r"needle".to_owned(),
            path_regex: None,
            limit: 10,
        },
        SearchFilters {
            repository_id: None,
            language: Some("java".to_owned()),
        },
    )?;
    assert_eq!(
        java_matches,
        vec![
            text_match("repo-001", "src/lib.java", 1, 1, "needle 19"),
            text_match("repo-002", "src/main.java", 1, 1, "needle 20"),
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
            language: Some("javascript".to_owned()),
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
