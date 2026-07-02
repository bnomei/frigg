//! Regression tests for hybrid search warning metadata and lexical-only fallback response shaping.

use super::*;
use crate::mcp::types::ResponseFreshnessBasisMetadata;
use crate::storage::ManifestEntry;

fn manifest_entry(path: &str, size_bytes: u64, mtime_ns: Option<u64>) -> ManifestEntry {
    ManifestEntry {
        path: path.to_owned(),
        sha256: format!("hash-{path}"),
        size_bytes,
        mtime_ns,
    }
}

fn match_fixture(
    path: &str,
    source_class: Option<SourceClass>,
    surface_families: &[&str],
    pivotable: bool,
    document_symbols: bool,
    go_to_definition: bool,
) -> SearchHybridMatch {
    SearchHybridMatch {
        match_id: None,
        repository_id: "repo-001".to_string(),
        path: path.to_string(),
        line: 1,
        column: 1,
        excerpt: "fixture".to_string(),
        anchor: None,
        blended_score: 1.0,
        lexical_score: 1.0,
        graph_score: 0.0,
        semantic_score: 0.0,
        lexical_sources: vec![],
        graph_sources: vec![],
        semantic_sources: vec![],
        path_class: None,
        source_class,
        surface_families: surface_families
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        navigation_hint: Some(SearchHybridNavigationHint {
            pivotable,
            document_symbols,
            go_to_definition,
        }),
        rank_reasons: vec![],
    }
}

#[test]
fn search_hybrid_utility_summary_prefers_runtime_source_pivot() {
    let matches = vec![
        match_fixture(
            "README.md",
            Some(SourceClass::Project),
            &["docs"],
            false,
            false,
            false,
        ),
        match_fixture(
            "tests/runtime_test.rs",
            Some(SourceClass::Tests),
            &["tests"],
            true,
            true,
            false,
        ),
        match_fixture(
            "src/runtime/server.rs",
            Some(SourceClass::Runtime),
            &["runtime"],
            true,
            true,
            true,
        ),
    ];

    let summary = FriggMcpServer::search_hybrid_utility_summary(&matches);
    assert_eq!(summary.pivotable_match_count, 2);
    assert_eq!(summary.best_pivot_rank, Some(3));
    assert_eq!(
        summary.best_pivot_path.as_deref(),
        Some("src/runtime/server.rs")
    );
    assert!(summary.symbol_navigation_ready);
}

#[test]
fn search_hybrid_utility_summary_reports_miss_without_pivotable_matches() {
    let matches = vec![
        match_fixture(
            "guides/overview.md",
            Some(SourceClass::Project),
            &["docs"],
            false,
            false,
            false,
        ),
        match_fixture(
            "package.json",
            Some(SourceClass::Project),
            &["package_surface"],
            false,
            false,
            false,
        ),
    ];

    let summary = FriggMcpServer::search_hybrid_utility_summary(&matches);
    assert_eq!(summary.pivotable_match_count, 0);
    assert_eq!(summary.best_pivot_rank, None);
    assert_eq!(summary.best_pivot_path, None);
    assert!(!summary.symbol_navigation_ready);
}

#[test]
fn search_text_metadata_maps_ripgrep_backend() {
    let metadata = FriggMcpServer::search_text_metadata(
        Some(SearchLexicalBackend::Ripgrep),
        Some("ripgrep accelerator active".to_owned()),
    )
    .expect("metadata should exist");
    assert_eq!(
        metadata.lexical_backend,
        Some(SearchLexicalBackendMetadata::Ripgrep)
    );
    assert_eq!(
        metadata.lexical_backend_note.as_deref(),
        Some("ripgrep accelerator active")
    );
}

#[test]
fn search_text_metadata_returns_none_without_backend() {
    assert!(FriggMcpServer::search_text_metadata(None, None).is_none());
}

#[test]
fn search_text_params_accept_context_efficiency_opt_in() {
    let params: SearchTextParams = serde_json::from_value(json!({
        "query": "runtime capture",
        "include_context_efficiency": true
    }))
    .expect("search_text params should accept include_context_efficiency");

    assert_eq!(params.include_context_efficiency, Some(true));
}

#[test]
fn search_text_metadata_omits_context_efficiency_by_default() {
    let metadata = SearchTextMetadata {
        lexical_backend: Some(SearchLexicalBackendMetadata::Native),
        lexical_backend_note: None,
        context_efficiency: None,
    };

    let value = serde_json::to_value(metadata).expect("metadata should serialize");
    assert!(value.get("context_efficiency").is_none());
}

#[test]
fn search_text_context_efficiency_metadata_uses_manifest_and_excerpts_without_stages() {
    let root = std::env::temp_dir().join(format!(
        "frigg-text-context-efficiency-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".frigg")).expect("temp frigg directory should be writable");
    let db_path = root.join(".frigg/storage.sqlite3");
    let storage = Storage::new(&db_path);
    storage.initialize().expect("storage should initialize");
    storage
        .upsert_manifest(
            "repo-001",
            "snapshot-001",
            &[
                manifest_entry("src/lib.rs", 100, Some(10)),
                manifest_entry("README.md", 40, Some(20)),
            ],
        )
        .expect("manifest should be writable");

    let workspace = AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: "fixture".to_owned(),
        root: root.clone(),
        db_path,
    };
    let matches = vec![
        TextMatch {
            match_id: None,
            repository_id: "repo-001".to_owned(),
            path: "src/lib.rs".to_owned(),
            line: 1,
            column: 1,
            excerpt: "alpha".to_owned(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        },
        TextMatch {
            match_id: None,
            repository_id: "repo-001".to_owned(),
            path: "src/lib.rs".to_owned(),
            line: 2,
            column: 1,
            excerpt: "beta".to_owned(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        },
    ];

    let metadata =
        FriggMcpServer::search_text_context_efficiency_metadata(&[workspace], &matches, 4)
            .expect("context-efficiency metadata should build");

    assert_eq!(metadata.indexed_readable_files, 2);
    assert_eq!(metadata.indexed_readable_bytes, 140);
    assert_eq!(metadata.candidate_input_count, None);
    assert_eq!(metadata.candidate_output_count, None);
    assert_eq!(metadata.returned_match_count, Some(4));
    assert_eq!(metadata.returned_unique_paths, Some(1));
    assert_eq!(metadata.returned_unique_file_bytes, Some(100));
    assert_eq!(metadata.returned_source_bytes_estimate, Some(9));
    assert_eq!(metadata.stage_attribution, None);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn context_efficiency_log_for_workspaces_respects_log_state() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "frigg-context-efficiency-workspace-log-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".frigg")).expect("temp frigg directory should be writable");
    let db_path = root.join(".frigg/storage.sqlite3");
    let storage = Storage::new(&db_path);
    storage.initialize().expect("storage should initialize");
    storage
        .upsert_manifest(
            "repo-001",
            "snapshot-001",
            &[manifest_entry("src/lib.rs", 100, Some(10))],
        )
        .expect("manifest should be writable");

    let workspace = AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: "fixture".to_owned(),
        root: root.clone(),
        db_path,
    };
    let metadata = ContextEfficiencyMetadata {
        indexed_readable_files: 1,
        indexed_readable_bytes: 100,
        indexed_min_mtime_ns: Some(10),
        indexed_max_mtime_ns: Some(10),
        candidate_input_count: None,
        candidate_output_count: None,
        returned_match_count: Some(1),
        returned_unique_paths: Some(1),
        returned_unique_file_bytes: Some(100),
        returned_source_bytes_estimate: Some(7),
        narrowing_ratio_estimate: Some(100.0 / 7.0),
        stage_attribution: None,
    };

    FriggMcpServer::append_context_efficiency_log_for_workspaces_with_log_state(
        "search_text",
        std::slice::from_ref(&workspace),
        &metadata,
        false,
    );

    let log_path = root.join(".frigg/context.jsonl");
    assert!(!log_path.exists());

    FriggMcpServer::append_context_efficiency_log_for_workspaces_with_log_state(
        "search_text",
        std::slice::from_ref(&workspace),
        &metadata,
        true,
    );

    let logged = std::fs::read_to_string(&log_path).expect("context log should be readable");
    assert_eq!(logged.lines().count(), 1);
    let value: Value = serde_json::from_str(logged.trim()).expect("context row should be json");
    assert_eq!(value["tool"], "search_text");
    assert_eq!(value["repository_id"], "repo-001");
    assert_eq!(value["snapshot_id"], "snapshot-001");
    assert_eq!(value["returned_match_count"], 1);
    assert!(value.get("query").is_none());
    assert!(value.get("content").is_none());
    assert!(value.get("excerpt").is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn context_efficiency_env_only_metric_failure_degrades_to_no_metadata() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "frigg-context-efficiency-metric-failure-{}-{unique}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let frigg_dir = root.join(".frigg");
    std::fs::create_dir_all(&frigg_dir).expect("temp frigg directory should be writable");
    let workspace = AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: "fixture".to_owned(),
        root: root.clone(),
        db_path: frigg_dir.clone(),
    };
    let matches = vec![TextMatch {
        match_id: None,
        repository_id: "repo-001".to_owned(),
        path: "src/lib.rs".to_owned(),
        line: 1,
        column: 1,
        excerpt: "alpha".to_owned(),
        witness_score_hint_millis: None,
        witness_provenance_ids: None,
    }];

    let env_only = FriggMcpServer::context_efficiency_metadata_for_controls(None, true, || {
        FriggMcpServer::search_text_context_efficiency_metadata(
            std::slice::from_ref(&workspace),
            &matches,
            1,
        )
    })
    .expect("env-only metric failure should not fail the tool path");

    assert!(env_only.is_none());
    assert!(!root.join(".frigg/context.jsonl").exists());

    let explicit_response =
        FriggMcpServer::context_efficiency_metadata_for_controls(Some(true), true, || {
            FriggMcpServer::search_text_context_efficiency_metadata(
                std::slice::from_ref(&workspace),
                &matches,
                1,
            )
        });

    assert!(explicit_response.is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn search_hybrid_params_accept_context_efficiency_opt_in() {
    let params: SearchHybridParams = serde_json::from_value(json!({
        "query": "runtime capture",
        "include_context_efficiency": true
    }))
    .expect("search_hybrid params should accept include_context_efficiency");

    assert_eq!(params.include_context_efficiency, Some(true));
}

#[test]
fn search_hybrid_metadata_omits_context_efficiency_by_default() {
    let metadata = SearchHybridMetadata {
        channels: BTreeMap::new(),
        lexical_backend: None,
        lexical_backend_note: None,
        semantic_requested: None,
        semantic_enabled: None,
        semantic_status: None,
        semantic_reason: None,
        semantic_candidate_count: None,
        semantic_hit_count: None,
        semantic_match_count: None,
        lexical_only_mode: None,
        query_shape: None,
        warning: None,
        exact_pivot_assistance: None,
        witness_demotion_applied: None,
        diagnostics_count: 0,
        diagnostics: SearchHybridDiagnosticsSummary {
            walk: 0,
            read: 0,
            total: 0,
        },
        stage_attribution: None,
        semantic_capability: None,
        utility: None,
        context_efficiency: None,
        freshness_basis: ResponseFreshnessBasisMetadata {
            mode: "manifest".to_owned(),
            cacheable: false,
            repositories: vec![],
        },
    };

    let value = serde_json::to_value(metadata).expect("metadata should serialize");
    assert!(value.get("context_efficiency").is_none());
}

#[test]
fn search_hybrid_context_efficiency_metadata_uses_manifest_and_excerpts() {
    let root = std::env::temp_dir().join(format!(
        "frigg-hybrid-context-efficiency-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".frigg")).expect("temp frigg directory should be writable");
    let db_path = root.join(".frigg/storage.sqlite3");
    let storage = Storage::new(&db_path);
    storage.initialize().expect("storage should initialize");
    storage
        .upsert_manifest(
            "repo-001",
            "snapshot-001",
            &[
                manifest_entry("src/lib.rs", 100, Some(10)),
                manifest_entry("README.md", 40, Some(20)),
            ],
        )
        .expect("manifest should be writable");

    let workspace = AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: "fixture".to_owned(),
        root: root.clone(),
        db_path,
    };
    let matches = vec![
        match_fixture(
            "src/lib.rs",
            Some(SourceClass::Runtime),
            &["runtime"],
            true,
            true,
            true,
        ),
        match_fixture(
            "src/lib.rs",
            Some(SourceClass::Runtime),
            &["runtime"],
            true,
            true,
            true,
        ),
    ];

    let metadata =
        FriggMcpServer::search_hybrid_context_efficiency_metadata(&[workspace], &matches, None)
            .expect("context-efficiency metadata should build");

    assert_eq!(metadata.indexed_readable_files, 2);
    assert_eq!(metadata.indexed_readable_bytes, 140);
    assert_eq!(metadata.returned_match_count, Some(2));
    assert_eq!(metadata.returned_unique_paths, Some(1));
    assert_eq!(metadata.returned_unique_file_bytes, Some(100));
    assert_eq!(metadata.returned_source_bytes_estimate, Some(14));
    assert_eq!(metadata.narrowing_ratio_estimate, Some(10.0));

    let _ = std::fs::remove_dir_all(root);
}
