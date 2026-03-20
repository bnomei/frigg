use super::*;

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
