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
        target_ref: None,
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
        graph_mode: None,
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
fn search_text_params_accept_legacy_pattern_alias() {
    let params: SearchTextParams = serde_json::from_value(json!({
        "pattern": "unwrap\\(|expect\\(",
        "pattern_type": "regex",
        "glob": "crates/cli/src/**/*.rs",
        "limit": 30
    }))
    .expect("search_text params should accept legacy pattern alias");

    assert_eq!(params.query, "unwrap\\(|expect\\(");
    assert_eq!(params.pattern_type, Some(SearchPatternType::Regex));
    assert_eq!(params.glob.as_deref(), Some("crates/cli/src/**/*.rs"));
    assert_eq!(params.limit, Some(30));
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
    assert!(value.get("freshness_basis").is_none());
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
            target_ref: None,
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
            target_ref: None,
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
    assert_eq!(metadata.matched_file_context_saved_bytes_estimate, Some(91));
    assert_eq!(
        metadata.matched_file_context_saved_percent_estimate,
        Some(91.0)
    );
    assert_eq!(metadata.corpus_context_saved_bytes_estimate, Some(131));
    assert_eq!(metadata.corpus_context_saved_percent_estimate, Some(93.57));
    assert_eq!(metadata.corpus_narrowing_ratio_estimate, Some(16));
    assert_eq!(metadata.narrowing_ratio_estimate, Some(11));
    assert_eq!(metadata.stage_attribution, None);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn context_efficiency_metadata_uses_runtime_repository_id_for_manifest_lookup() {
    let root = std::env::temp_dir().join(format!(
        "frigg-context-efficiency-runtime-id-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".frigg")).expect("temp frigg directory should be writable");
    let db_path = root.join(".frigg/storage.sqlite3");
    let storage = Storage::new(&db_path);
    storage.initialize().expect("storage should initialize");
    storage
        .upsert_manifest(
            "runtime-repo-001",
            "snapshot-001",
            &[manifest_entry("src/lib.rs", 100, Some(10))],
        )
        .expect("manifest should be writable");

    let workspace = AttachedWorkspace {
        repository_id: "public-repo-001".to_owned(),
        runtime_repository_id: "runtime-repo-001".to_owned(),
        display_name: "fixture".to_owned(),
        root: root.clone(),
        db_path,
    };
    let text_matches = vec![TextMatch {
        match_id: None,
        target_ref: None,
        repository_id: "public-repo-001".to_owned(),
        path: "src/lib.rs".to_owned(),
        line: 1,
        column: 1,
        excerpt: "alpha".to_owned(),
        witness_score_hint_millis: None,
        witness_provenance_ids: None,
    }];

    let text_metadata = FriggMcpServer::search_text_context_efficiency_metadata(
        std::slice::from_ref(&workspace),
        &text_matches,
        1,
    )
    .expect("text context-efficiency metadata should use runtime repository id");

    assert_eq!(text_metadata.indexed_readable_files, 1);
    assert_eq!(text_metadata.indexed_readable_bytes, 100);
    assert_eq!(text_metadata.returned_unique_file_bytes, Some(100));

    let mut hybrid_match = match_fixture(
        "src/lib.rs",
        Some(SourceClass::Runtime),
        &["runtime"],
        true,
        true,
        true,
    );
    hybrid_match.repository_id = "public-repo-001".to_owned();
    let hybrid_metadata = FriggMcpServer::search_hybrid_context_efficiency_metadata(
        std::slice::from_ref(&workspace),
        &[hybrid_match],
        None,
    )
    .expect("hybrid context-efficiency metadata should use runtime repository id");

    assert_eq!(hybrid_metadata.indexed_readable_files, 1);
    assert_eq!(hybrid_metadata.indexed_readable_bytes, 100);
    assert_eq!(hybrid_metadata.returned_unique_file_bytes, Some(100));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn search_text_context_efficiency_matches_absolute_manifest_paths() {
    let root = std::env::temp_dir().join(format!(
        "frigg-text-context-efficiency-absolute-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".frigg")).expect("temp frigg directory should be writable");
    let db_path = root.join(".frigg/storage.sqlite3");
    let storage = Storage::new(&db_path);
    storage.initialize().expect("storage should initialize");
    let absolute_manifest_path = root.join("src/lib.rs");
    storage
        .upsert_manifest(
            "repo-001",
            "snapshot-001",
            &[manifest_entry(
                &absolute_manifest_path.to_string_lossy(),
                100,
                Some(10),
            )],
        )
        .expect("manifest should be writable");

    let workspace = AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: "fixture".to_owned(),
        root: root.clone(),
        db_path,
    };
    let matches = vec![TextMatch {
        match_id: None,
        target_ref: None,
        repository_id: "repo-001".to_owned(),
        path: "src/lib.rs".to_owned(),
        line: 1,
        column: 1,
        excerpt: "alpha".to_owned(),
        witness_score_hint_millis: None,
        witness_provenance_ids: None,
    }];

    let metadata =
        FriggMcpServer::search_text_context_efficiency_metadata(&[workspace], &matches, 1)
            .expect("context-efficiency metadata should match absolute manifest paths");

    assert_eq!(metadata.returned_unique_file_bytes, Some(100));
    assert_eq!(metadata.returned_source_bytes_estimate, Some(5));
    assert_eq!(metadata.matched_file_context_saved_bytes_estimate, Some(95));
    assert_eq!(
        metadata.matched_file_context_saved_percent_estimate,
        Some(95.0)
    );
    assert_eq!(metadata.narrowing_ratio_estimate, Some(20));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn search_text_context_efficiency_falls_back_to_returned_bytes_for_unindexed_paths() {
    let root = std::env::temp_dir().join(format!(
        "frigg-text-context-efficiency-unindexed-{}",
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
    let matches = vec![TextMatch {
        match_id: None,
        target_ref: None,
        repository_id: "repo-001".to_owned(),
        path: "generated/live_only.rs".to_owned(),
        line: 1,
        column: 1,
        excerpt: "live".to_owned(),
        witness_score_hint_millis: None,
        witness_provenance_ids: None,
    }];

    let metadata =
        FriggMcpServer::search_text_context_efficiency_metadata(&[workspace], &matches, 1)
            .expect("context-efficiency metadata should build for unindexed paths");

    assert_eq!(metadata.returned_unique_paths, Some(1));
    assert_eq!(metadata.returned_unique_file_bytes, Some(4));
    assert_eq!(metadata.returned_source_bytes_estimate, Some(4));
    assert_eq!(metadata.matched_file_context_saved_bytes_estimate, Some(0));
    assert_eq!(
        metadata.matched_file_context_saved_percent_estimate,
        Some(0.0)
    );
    assert_eq!(metadata.narrowing_ratio_estimate, Some(1));

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
        repository_id: "public-repo-001".to_owned(),
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
        matched_file_context_saved_bytes_estimate: Some(93),
        matched_file_context_saved_percent_estimate: Some(93.0),
        corpus_context_saved_bytes_estimate: Some(93),
        corpus_context_saved_percent_estimate: Some(93.0),
        corpus_narrowing_ratio_estimate: Some(14),
        query_duration_ms: Some(11),
        narrowing_ratio_estimate: Some(14),
        stage_attribution: None,
    };

    FriggMcpServer::append_context_efficiency_log_for_workspaces_with_log_state(
        "search_text",
        std::slice::from_ref(&workspace),
        &metadata,
        false,
        Some("connected-session-full-123456"),
    );

    let log_path = root.join(".frigg/context.jsonl");
    assert!(!log_path.exists());

    FriggMcpServer::append_context_efficiency_log_for_workspaces_with_log_state(
        "search_text",
        std::slice::from_ref(&workspace),
        &metadata,
        true,
        Some("connected-session-full-123456"),
    );

    let logged = std::fs::read_to_string(&log_path).expect("context log should be readable");
    assert_eq!(logged.lines().count(), 1);
    let value: Value = serde_json::from_str(logged.trim()).expect("context row should be json");
    assert_eq!(value["tool"], "search_text");
    assert_eq!(value["session_id"], "connected-session-full-123456");
    assert_eq!(value["repository_id"], "public-repo-001");
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
        target_ref: None,
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
        cache_debug: Some(ResponseFreshnessBasisMetadata {
            mode: "manifest".to_owned(),
            cacheable: false,
            repositories: vec![],
            runtime_cache_contract: None,
        }),
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
    assert_eq!(metadata.matched_file_context_saved_bytes_estimate, Some(86));
    assert_eq!(
        metadata.matched_file_context_saved_percent_estimate,
        Some(86.0)
    );
    assert_eq!(metadata.corpus_context_saved_bytes_estimate, Some(126));
    assert_eq!(metadata.corpus_narrowing_ratio_estimate, Some(10));
    assert_eq!(metadata.narrowing_ratio_estimate, Some(7));

    let _ = std::fs::remove_dir_all(root);
}
