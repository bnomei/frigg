//! Regression tests for ripgrep backend availability, manifest candidate filtering, and JSON stream parsing.

use super::*;
use crate::searcher::{ExactSearchExecutionOutput, SearchTextExecutionOptions};

fn assert_exact_execution_parity(
    expected: &ExactSearchExecutionOutput,
    actual: &ExactSearchExecutionOutput,
) {
    assert_eq!(actual.total_matches, expected.total_matches);
    assert_eq!(actual.total_rows, expected.total_rows);
    assert_eq!(actual.matches, expected.matches);
    assert_eq!(actual.coverage, expected.coverage);
}

#[test]
fn literal_search_ripgrep_backend_filters_hits_to_validated_candidate_universe() -> FriggResult<()>
{
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("literal-search-ripgrep-manifest");
    prepare_workspace(
        &root,
        &[
            ("src/indexed.rs", "needle indexed\n"),
            ("src/out_scope.rs", "needle out\n"),
        ],
    )?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;
    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/indexed.rs"},"lines":{"text":"needle indexed\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle"},"start":0,"end":6}]}}
{"type":"match","data":{"path":{"text":"src/out_scope.rs"},"lines":{"text":"needle out\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle"},"start":0,"end":6}]}}"#,
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let searcher = TextSearcher::new(config);

    let output = searcher.search_literal_with_filters_diagnostics(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(output.lexical_backend, Some(SearchLexicalBackend::Ripgrep));
    assert_eq!(
        output.matches,
        vec![
            text_match("repo-001", "src/indexed.rs", 1, 1, "needle indexed"),
            text_match("repo-001", "src/out_scope.rs", 1, 1, "needle out"),
        ]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn literal_search_falls_back_to_native_when_ripgrep_is_unavailable() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("literal-search-ripgrep-fallback");
    prepare_workspace(&root, &[("src/indexed.rs", "needle indexed\n")])?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(root.join("missing-rg-executable"));
    let searcher = TextSearcher::new(config);

    let output = searcher.search_literal_with_filters_diagnostics(
        SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 20,
        },
        SearchFilters::default(),
    )?;

    assert_eq!(output.lexical_backend, Some(SearchLexicalBackend::Native));
    assert!(
        output
            .lexical_backend_note
            .as_deref()
            .is_some_and(|note| note.contains("ripgrep unavailable")),
        "expected native fallback note, got {:?}",
        output.lexical_backend_note
    );
    assert_eq!(
        output.matches,
        vec![text_match(
            "repo-001",
            "src/indexed.rs",
            1,
            1,
            "needle indexed"
        )]
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_search_reports_ripgrep_backend_in_lexical_only_mode() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("hybrid-search-ripgrep-backend");
    prepare_workspace(&root, &[("src/runtime.rs", "pub fn needle_handler() {}\n")])?;
    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/runtime.rs"},"lines":{"text":"pub fn needle_handler() {}\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle_handler"},"start":7,"end":21}]}}"#,
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let searcher = TextSearcher::new(config);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle handler".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(matches!(
        output.note.lexical_backend,
        Some(SearchLexicalBackend::Ripgrep | SearchLexicalBackend::Mixed)
    ));
    assert!(
        output
            .note
            .lexical_backend_note
            .as_deref()
            .is_some_and(|note| note.contains("ripgrep accelerator active"))
    );
    assert_eq!(output.matches.len(), 1);
    assert_eq!(output.matches[0].document.path, "src/runtime.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_search_single_exact_term_uses_ripgrep_backend_when_available() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("hybrid-search-ripgrep-single-token");
    prepare_workspace(&root, &[("src/runtime.rs", "pub fn needle_handler() {}\n")])?;
    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/runtime.rs"},"lines":{"text":"pub fn needle_handler() {}\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle_handler"},"start":7,"end":21}]}}"#,
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let searcher = TextSearcher::new(config);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "needle_handler".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(matches!(
        output.note.lexical_backend,
        Some(SearchLexicalBackend::Ripgrep | SearchLexicalBackend::Mixed)
    ));
    assert_eq!(output.matches.len(), 1);
    assert_eq!(output.matches[0].document.path, "src/runtime.rs");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn exact_literal_search_forces_mixed_scrubbed_markdown_and_matches_native_cardinality()
-> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("exact-literal-mixed-ripgrep");
    prepare_workspace(
        &root,
        &[
            (
                "docs/readme.md",
                "<!-- needle hidden -->\nneedle markdown\n",
            ),
            ("src/plain.rs", "needle plain\nneedle plain again\n"),
        ],
    )?;
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 20,
    };
    let mut native_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    native_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Native;
    let native = TextSearcher::new(native_config)
        .search_literal_with_execution_options_diagnostics(
            query.clone(),
            SearchFilters::default(),
            SearchTextExecutionOptions::default(),
        )?;

    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/plain.rs"},"lines":{"text":"needle plain\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle"},"start":0,"end":6}]}}
{"type":"match","data":{"path":{"text":"src/plain.rs"},"lines":{"text":"needle plain again\n"},"line_number":2,"absolute_offset":13,"submatches":[{"match":{"text":"needle"},"start":0,"end":6}]}}"#,
    )?;
    let mut mixed_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    mixed_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    mixed_config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let mixed = TextSearcher::new(mixed_config).search_literal_with_execution_options_diagnostics(
        query,
        SearchFilters::default(),
        SearchTextExecutionOptions::default(),
    )?;

    assert_exact_execution_parity(&native, &mixed);
    assert_eq!(mixed.lexical_backend, Some(SearchLexicalBackend::Mixed));
    assert!(
        mixed
            .lexical_backend_note
            .as_deref()
            .is_some_and(|note| note.contains("native fallback for scrubbed content"))
    );

    cleanup_workspace(&root);
    Ok(())
}

#[cfg(unix)]
#[test]
fn exact_literal_search_falls_back_after_unavailable_or_failed_ripgrep() -> FriggResult<()> {
    use std::os::unix::fs::PermissionsExt;

    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("exact-literal-ripgrep-fallback");
    prepare_workspace(&root, &[("src/lib.rs", "needle\nneedle again\n")])?;
    let query = SearchTextQuery {
        query: "needle".to_owned(),
        path_regex: None,
        limit: 1,
    };
    let mut native_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    native_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Native;
    let native = TextSearcher::new(native_config)
        .search_literal_with_execution_options_diagnostics(
            query.clone(),
            SearchFilters::default(),
            SearchTextExecutionOptions::default(),
        )?;

    let missing_rg = root.join("missing-rg");
    let failed_rg = root.join("failed-rg.sh");
    std::fs::write(
        &failed_rg,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'ripgrep 15.1.0'; exit 0; fi\necho forced failure >&2\nexit 2\n",
    )
    .map_err(FriggError::Io)?;
    let mut permissions = std::fs::metadata(&failed_rg)
        .map_err(FriggError::Io)?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&failed_rg, permissions).map_err(FriggError::Io)?;

    for (executable, expected_note) in [
        (missing_rg, "ripgrep unavailable"),
        (failed_rg, "ripgrep execution failed"),
    ] {
        clear_ripgrep_availability_cache();
        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
        config.lexical_runtime.ripgrep_executable = Some(executable);
        let fallback = TextSearcher::new(config)
            .search_literal_with_execution_options_diagnostics(
                query.clone(),
                SearchFilters::default(),
                SearchTextExecutionOptions::default(),
            )?;
        assert_exact_execution_parity(&native, &fallback);
        assert_eq!(fallback.lexical_backend, Some(SearchLexicalBackend::Native));
        assert!(
            fallback
                .lexical_backend_note
                .as_deref()
                .is_some_and(|note| note.contains(expected_note))
        );
    }

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn exact_regex_search_matches_forced_native_and_ripgrep_backends() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("exact-regex-ripgrep-parity");
    prepare_workspace(
        &root,
        &[
            ("src/a.rs", "needle 10\n"),
            ("src/b.rs", "needle 20\nneedle 30\n"),
        ],
    )?;
    let query = SearchTextQuery {
        query: r"needle\s+\d+".to_owned(),
        path_regex: Some(Regex::new(r"^src/").expect("valid fixture regex")),
        limit: 20,
    };
    let mut native_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    native_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Native;
    let native = TextSearcher::new(native_config).search_regex_with_execution_options_diagnostics(
        query.clone(),
        SearchFilters::default(),
        SearchTextExecutionOptions::default(),
    )?;

    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/a.rs"},"lines":{"text":"needle 10\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle 10"},"start":0,"end":9}]}}
{"type":"match","data":{"path":{"text":"src/b.rs"},"lines":{"text":"needle 20\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle 20"},"start":0,"end":9}]}}
{"type":"match","data":{"path":{"text":"src/b.rs"},"lines":{"text":"needle 30\n"},"line_number":2,"absolute_offset":10,"submatches":[{"match":{"text":"needle 30"},"start":0,"end":9}]}}"#,
    )?;
    let mut ripgrep_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    ripgrep_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    ripgrep_config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let ripgrep = TextSearcher::new(ripgrep_config)
        .search_regex_with_execution_options_diagnostics(
            query,
            SearchFilters::default(),
            SearchTextExecutionOptions::default(),
        )?;

    assert_exact_execution_parity(&native, &ripgrep);
    assert_eq!(ripgrep.lexical_backend, Some(SearchLexicalBackend::Ripgrep));

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn exact_regex_search_matches_forced_native_ripgrep_and_mixed_backends() -> FriggResult<()> {
    clear_ripgrep_availability_cache();
    let root = temp_workspace_root("exact-regex-backend-parity");
    prepare_workspace(
        &root,
        &[
            ("src/a.rs", "needle 10\n"),
            ("src/b.rs", "needle 20\nneedle 30\n"),
            ("docs/readme.md", "<!-- needle 99 -->\nneedle 40\n"),
        ],
    )?;
    let query = SearchTextQuery {
        query: r"needle\s+\d+".to_owned(),
        path_regex: Some(Regex::new(r"^(?:src/|docs/)").expect("valid fixture regex")),
        limit: 20,
    };
    let mut native_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    native_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Native;
    let native = TextSearcher::new(native_config).search_regex_with_execution_options_diagnostics(
        query.clone(),
        SearchFilters::default(),
        SearchTextExecutionOptions::default(),
    )?;

    let fake_rg = write_fake_ripgrep_script(
        &root,
        r#"{"type":"match","data":{"path":{"text":"src/a.rs"},"lines":{"text":"needle 10\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle 10"},"start":0,"end":9}]}}
{"type":"match","data":{"path":{"text":"src/b.rs"},"lines":{"text":"needle 20\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"needle 20"},"start":0,"end":9}]}}
{"type":"match","data":{"path":{"text":"src/b.rs"},"lines":{"text":"needle 30\n"},"line_number":2,"absolute_offset":10,"submatches":[{"match":{"text":"needle 30"},"start":0,"end":9}]}}"#,
    )?;
    let mut mixed_config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    mixed_config.lexical_runtime.backend = crate::settings::LexicalBackendMode::Ripgrep;
    mixed_config.lexical_runtime.ripgrep_executable = Some(fake_rg);
    let mixed = TextSearcher::new(mixed_config).search_regex_with_execution_options_diagnostics(
        query,
        SearchFilters::default(),
        SearchTextExecutionOptions::default(),
    )?;

    assert_exact_execution_parity(&native, &mixed);
    assert_eq!(mixed.lexical_backend, Some(SearchLexicalBackend::Mixed));

    cleanup_workspace(&root);
    Ok(())
}
