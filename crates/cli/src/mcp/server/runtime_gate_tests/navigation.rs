#![allow(clippy::panic)]

//! Regression tests for navigation tool modes, result handles, and precise partial coverage.

use super::*;
use crate::domain::model::{ReferenceMatch, ReferenceMatchKind};
use crate::mcp::types::{
    CallHierarchyMatch, FindDeclarationsResponse, FindImplementationsResponse,
    FindReferencesResponse, ImplementationMatch, IncomingCallsResponse, NavigationAvailability,
    NavigationLocation, NavigationMode, OutgoingCallsResponse,
};
use rmcp::model::ErrorCode;
use serde::Serialize;

#[test]
fn precise_graph_prewarm_populates_latest_precise_cache() {
    let workspace_root = temp_workspace_root("precise-prewarm");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct User;\n\npub fn current_user() -> User { User }\n",
    )
    .expect("failed to write source fixture");
    write_scip_protobuf_fixture(&workspace_root, "fixture.scip");

    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");

    let _ = server.prewarm_precise_graph_for_workspace(&workspace);

    let cached = server
        .cache_state
        .latest_precise_graph_cache
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&workspace.repository_id)
        .cloned()
        .expect("precise prewarm should populate the latest precise graph cache");
    assert_eq!(cached.ingest_stats.artifacts_ingested, 1);
    assert_eq!(cached.ingest_stats.artifacts_failed, 0);
    assert_eq!(
        cached.coverage_mode,
        crate::mcp::server_state::PreciseCoverageMode::Full
    );
    assert!(
        server
            .cache_state
            .symbol_corpus_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .any(|key| key.repository_id == workspace.repository_id),
        "precise prewarm should also warm the symbol corpus cache used by navigation"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn precise_definition_fast_path_resolves_location_without_symbol_corpus_rebuild() {
    let workspace_root = temp_workspace_root("precise-fast-path");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct User;\n\npub fn current_user() -> User { User }\n",
    )
    .expect("failed to write source fixture");
    write_scip_protobuf_fixture(&workspace_root, "fixture.scip");

    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let corpora = server
        .collect_repository_symbol_corpora(Some(&workspace.repository_id))
        .expect("symbol corpus should build before precise fast path");

    let response = server
        .try_precise_definition_fast_path(
            &corpora,
            Some(&workspace.repository_id),
            "src/lib.rs",
            1,
            Some(13),
            false,
            10,
            &serde_json::json!({
                "mode": "manifest_only",
                "cacheable": true,
                "repositories": []
            }),
        )
        .expect("cached precise fast path should not error")
        .expect("cached precise fast path should resolve a definition");
    assert_eq!(response.1, workspace.repository_id);
    assert_eq!(response.2, "scip-rust pkg repo#User");
    assert_eq!(response.3, "precise");
    assert_eq!(response.0.0.matches.len(), 1);
    assert_eq!(response.0.0.matches[0].path, "src/lib.rs");
    assert_eq!(response.0.0.matches[0].line, 1);
    let metadata = response.0.0.metadata.expect("metadata should be present");
    assert_eq!(
        metadata["freshness_basis"]["mode"],
        serde_json::json!("manifest_only")
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn symbol_corpus_uses_runtime_repository_id_for_validated_live_storage() {
    let workspace_root = temp_workspace_root("symbol-corpus-runtime-repository-id");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/manifest.rs"),
        "pub struct StoredOnly;\n",
    )
    .expect("failed to write manifest-backed source fixture");
    fs::write(workspace_root.join("src/live.rs"), "pub struct LiveOnly;\n")
        .expect("failed to write live-only source fixture");

    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    assert_ne!(workspace.repository_id, workspace.runtime_repository_id);
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.runtime_repository_id,
        "snapshot-runtime",
        &["src/manifest.rs"],
    );

    let corpora = server
        .collect_repository_symbol_corpora(Some(&workspace.repository_id))
        .expect("symbol corpus should build from runtime manifest storage");
    assert_eq!(corpora.len(), 1);
    let corpus = &corpora[0];
    assert_eq!(corpus.repository_id, workspace.repository_id);
    assert_eq!(
        corpus.runtime_repository_id,
        workspace.runtime_repository_id
    );
    let canonical_workspace_root = workspace_root
        .canonicalize()
        .expect("workspace root should canonicalize");
    let source_paths = corpus
        .source_paths
        .iter()
        .map(|path| {
            path.strip_prefix(&canonical_workspace_root)
                .unwrap_or(path)
                .to_path_buf()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        source_paths,
        vec![
            PathBuf::from("src/live.rs"),
            PathBuf::from("src/manifest.rs")
        ]
    );
    assert!(
        corpus
            .symbols
            .iter()
            .any(|symbol| symbol.name == "StoredOnly")
    );
    assert!(
        corpus
            .symbols
            .iter()
            .any(|symbol| symbol.name == "LiveOnly")
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn inspect_syntax_tree_returns_focus_and_ancestor_stack() {
    let workspace_root = temp_workspace_root("inspect-syntax-tree");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn greet() {\n    helper();\n}\n\nfn helper() {}\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let response = server
        .inspect_syntax_tree(rmcp::handler::server::wrapper::Parameters(
            InspectSyntaxTreeParams {
                path: "src/lib.rs".to_owned(),
                repository_id: Some(repository_id),
                line: Some(2),
                column: Some(6),
                max_ancestors: Some(4),
                max_children: Some(6),
                include_follow_up_structural: None,
            },
        ))
        .await
        .expect("inspect_syntax_tree should succeed")
        .0;

    assert_eq!(response.language, "rust");
    assert_eq!(response.path, "src/lib.rs");
    assert_eq!(response.focus.line, 2);
    assert!(
        response.focus.kind == "call_expression"
            || response
                .ancestors
                .iter()
                .any(|node| node.kind == "call_expression"),
        "expected call_expression in focus or ancestor stack, got focus={:?}, ancestors={:?}",
        response.focus.kind,
        response
            .ancestors
            .iter()
            .map(|node| node.kind.clone())
            .collect::<Vec<_>>()
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn inspect_syntax_tree_normalizes_punctuation_focus_to_useful_named_node() {
    let workspace_root = temp_workspace_root("inspect-syntax-tree-punctuation");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn greet() {\n    helper();\n}\n\nfn helper() {}\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let response = server
        .inspect_syntax_tree(rmcp::handler::server::wrapper::Parameters(
            InspectSyntaxTreeParams {
                path: "src/lib.rs".to_owned(),
                repository_id: Some(repository_id),
                line: Some(2),
                column: Some(13),
                max_ancestors: Some(4),
                max_children: Some(6),
                include_follow_up_structural: None,
            },
        ))
        .await
        .expect("inspect_syntax_tree should normalize punctuation focus")
        .0;

    assert_eq!(response.focus.kind, "call_expression");
    let note_json: serde_json::Value = serde_json::from_str(
        response
            .note
            .as_ref()
            .expect("inspect_syntax_tree should emit metadata note"),
    )
    .expect("inspect note should be valid JSON");
    assert_eq!(note_json["focus_normalized"], true);
    assert_eq!(note_json["raw_focus_kind"], ")");

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn inspect_syntax_tree_opt_in_returns_follow_up_structural() {
    let workspace_root = temp_workspace_root("inspect-syntax-tree-follow-up");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn greet() {\n    helper();\n}\n\nfn helper() {}\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let response = server
        .inspect_syntax_tree(rmcp::handler::server::wrapper::Parameters(
            InspectSyntaxTreeParams {
                path: "src/lib.rs".to_owned(),
                repository_id: Some(repository_id),
                line: Some(2),
                column: Some(6),
                max_ancestors: Some(4),
                max_children: Some(6),
                include_follow_up_structural: Some(true),
            },
        ))
        .await
        .expect("inspect_syntax_tree should return follow-up suggestions")
        .0;

    assert_eq!(response.follow_up_structural.len(), 3);
    assert_eq!(
        response.follow_up_structural[0].params.query,
        "(call_expression) @match"
    );
    assert_eq!(
        response.follow_up_structural[0]
            .params
            .path_regex
            .as_deref(),
        Some("^src/lib\\.rs$")
    );
    assert_eq!(
        response.follow_up_structural[2].params.query,
        "(function_item) @match"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn search_structural_invalid_query_returns_recovery_guidance() {
    let workspace_root = temp_workspace_root("search-structural-guidance");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn greet() {}\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let error = match server
        .search_structural(rmcp::handler::server::wrapper::Parameters(
            SearchStructuralParams {
                query: "(function_item @broken".to_owned(),
                language: Some("rust".to_owned()),
                repository_id: Some(repository_id),
                path_regex: None,
                limit: Some(10),
                result_mode: None,
                primary_capture: None,
                include_follow_up_structural: None,
            },
        ))
        .await
    {
        Ok(_) => panic!("invalid structural query must error"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("likely_cause"))
            .and_then(|value| value.as_str()),
        Some("tree_sitter_node_shape_mismatch")
    );
    let fallback_tools = error
        .data
        .as_ref()
        .and_then(|value| value.get("fallback_tools"))
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        fallback_tools
            .iter()
            .any(|value| value.as_str() == Some("inspect_syntax_tree"))
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn navigation_rejects_line_past_eof_like_read_file_bounds() {
    let workspace_root = temp_workspace_root("navigation-line-past-eof");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "fn helper() {}\n\nfn wrapper() {\n    helper();\n}\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let corpora = server
        .collect_repository_symbol_corpora(Some(&repository_id))
        .expect("symbol corpus collection should succeed");

    let past_eof_line = 11usize;
    let error = match FriggMcpServer::resolve_navigation_target(
        &corpora,
        None,
        Some("src/lib.rs"),
        Some(past_eof_line),
        Some(1),
        None,
    ) {
        Ok(_) => panic!("navigation should reject line numbers past EOF"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error.message, "start_line is outside file bounds");
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("start_line")),
        Some(&serde_json::Value::from(past_eof_line))
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("total_lines")),
        Some(&serde_json::Value::from(5))
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn navigation_rejects_line_past_eof_for_ambiguous_corpus_path() {
    let workspace_root = temp_workspace_root("navigation-ambiguous-line-past-eof");
    let repo_a = workspace_root.join("repo-a");
    let repo_b = workspace_root.join("repo-b");
    for repo in [&repo_a, &repo_b] {
        fs::create_dir_all(repo.join("src")).expect("failed to create workspace src directory");
        fs::write(repo.join("src/lib.rs"), "fn helper() {}\n")
            .expect("failed to write source fixture");
    }

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![repo_a.clone(), repo_b.clone()])
            .expect("workspace roots must produce valid config"),
    );
    let workspaces = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces();
    for workspace in &workspaces {
        server
            .adopt_workspace(workspace, true)
            .expect("server should adopt known workspace");
    }
    let corpora = server
        .collect_repository_symbol_corpora(None)
        .expect("symbol corpus collection should succeed");

    let past_eof_line = 5usize;
    let error = match FriggMcpServer::resolve_navigation_target(
        &corpora,
        None,
        Some("src/lib.rs"),
        Some(past_eof_line),
        Some(1),
        None,
    ) {
        Ok(_) => panic!("navigation should reject line numbers past EOF"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error.message, "start_line is outside file bounds");
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("start_line")),
        Some(&serde_json::Value::from(past_eof_line))
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("total_lines")),
        Some(&serde_json::Value::from(1))
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn navigation_double_slash_path_resolves_same_as_single_slash() {
    let workspace_root = temp_workspace_root("navigation-double-slash");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "fn helper() {}\n\nfn wrapper() {\n    helper();\n}\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let corpora = server
        .collect_repository_symbol_corpora(Some(&repository_id))
        .expect("symbol corpus collection should succeed");

    let single_slash = FriggMcpServer::resolve_navigation_target(
        &corpora,
        None,
        Some("src/lib.rs"),
        Some(4),
        Some(8),
        None,
    )
    .expect("single-slash location target resolution should succeed");
    let double_slash = FriggMcpServer::resolve_navigation_target(
        &corpora,
        None,
        Some("src//lib.rs"),
        Some(4),
        Some(8),
        None,
    )
    .expect("double-slash location target resolution should succeed");

    assert_eq!(double_slash.symbol_query, single_slash.symbol_query);
    assert_eq!(
        double_slash.resolution_source,
        single_slash.resolution_source
    );
    assert_eq!(double_slash.symbol_query, "helper");
    assert_eq!(double_slash.resolution_source, "location_token_rust");

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn location_navigation_prefers_token_under_cursor_before_enclosing_symbol() {
    let workspace_root = temp_workspace_root("location-token-resolution");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "fn helper() {}\n\nfn wrapper() {\n    helper();\n}\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();
    let corpora = server
        .collect_repository_symbol_corpora(Some(&repository_id))
        .expect("symbol corpus collection should succeed");
    let resolved = FriggMcpServer::resolve_navigation_target(
        &corpora,
        None,
        Some("src/lib.rs"),
        Some(4),
        Some(8),
        None,
    )
    .expect("location target resolution should succeed");

    assert_eq!(resolved.symbol_query, "helper");
    assert_eq!(resolved.resolution_source, "location_token_rust");

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn call_hierarchy_availability_distinguishes_heuristic_and_unavailable_modes() {
    let no_precise = PreciseIngestStats::default();
    let unavailable =
        FriggMcpServer::call_hierarchy_availability(PreciseCoverageMode::None, &no_precise, 0, 0);
    assert_eq!(unavailable.status, "unavailable");
    assert_eq!(
        unavailable.reason.as_deref(),
        Some("no_scip_artifacts_discovered")
    );
    assert!(unavailable.precise_required_for_complete_results);

    let heuristic =
        FriggMcpServer::call_hierarchy_availability(PreciseCoverageMode::None, &no_precise, 0, 2);
    assert_eq!(heuristic.status, "heuristic");
    assert_eq!(
        heuristic.reason.as_deref(),
        Some("no_scip_artifacts_discovered")
    );
    assert!(heuristic.precise_required_for_complete_results);
}

#[test]
fn compact_navigation_presenters_strip_metadata_and_keep_handles() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false);

    let (metadata, note) = compact_metadata_note();
    let references = server.present_find_references_response(
        FindReferencesResponse {
            total_matches: 1,
            matches: vec![reference_match()],
            result_handle: None,
            mode: NavigationMode::HeuristicNoPrecise,
            target_selection: None,
            metadata,
            note,
        },
        None,
    );
    let serialized = assert_compact_shape("find_references", &references);
    assert_eq!(references.total_matches, 1);
    assert_eq!(references.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(references.matches[0].match_id.as_deref(), Some("m1"));
    assert_compact_handle(&server, &references.result_handle, 11, Some(7));
    assert!(serialized.get("matches").is_some());

    let (metadata, note) = compact_metadata_note();
    let declarations = server.present_find_declarations_response(
        FindDeclarationsResponse {
            matches: vec![navigation_location(12, 4)],
            result_handle: None,
            mode: NavigationMode::PrecisePartial,
            target_selection: None,
            metadata,
            note,
        },
        None,
    );
    assert_compact_shape("find_declarations", &declarations);
    assert_eq!(declarations.mode, NavigationMode::PrecisePartial);
    assert_eq!(declarations.matches[0].match_id.as_deref(), Some("m1"));
    assert_compact_handle(&server, &declarations.result_handle, 12, Some(4));

    let (metadata, note) = compact_metadata_note();
    let implementations = server.present_find_implementations_response(
        FindImplementationsResponse {
            matches: vec![implementation_match()],
            result_handle: None,
            mode: NavigationMode::HeuristicNoPrecise,
            target_selection: None,
            metadata,
            note,
        },
        None,
    );
    assert_compact_shape("find_implementations", &implementations);
    assert_eq!(implementations.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(implementations.matches[0].match_id.as_deref(), Some("m1"));
    assert_compact_handle(&server, &implementations.result_handle, 13, Some(5));

    let availability = NavigationAvailability {
        status: "heuristic".to_owned(),
        reason: Some("fixture".to_owned()),
        precise_required_for_complete_results: true,
    };

    let (metadata, note) = compact_metadata_note();
    let incoming = server.present_incoming_calls_response(
        IncomingCallsResponse {
            matches: vec![call_match(17, 9, "calls")],
            result_handle: None,
            mode: NavigationMode::HeuristicNoPrecise,
            availability: Some(availability.clone()),
            target_selection: None,
            metadata,
            note,
        },
        None,
    );
    let serialized = assert_compact_shape("incoming_calls", &incoming);
    assert_eq!(incoming.matches[0].match_id.as_deref(), Some("m1"));
    assert_eq!(
        incoming
            .availability
            .as_ref()
            .map(|value| value.status.as_str()),
        Some("heuristic")
    );
    assert_eq!(
        serialized
            .get("availability")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("heuristic")
    );
    assert_compact_handle(&server, &incoming.result_handle, 17, Some(9));

    let (metadata, note) = compact_metadata_note();
    let outgoing = server.present_outgoing_calls_response(
        OutgoingCallsResponse {
            matches: vec![call_match(19, 3, "calls")],
            result_handle: None,
            mode: NavigationMode::HeuristicNoPrecise,
            availability: Some(availability),
            target_selection: None,
            metadata,
            note,
        },
        None,
    );
    let serialized = assert_compact_shape("outgoing_calls", &outgoing);
    assert_eq!(outgoing.matches[0].match_id.as_deref(), Some("m1"));
    assert_eq!(
        serialized
            .get("availability")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("heuristic")
    );
    assert_compact_handle(&server, &outgoing.result_handle, 19, Some(3));
}

fn compact_metadata_note() -> (Option<crate::mcp::types::MetadataObject>, Option<String>) {
    FriggMcpServer::metadata_note_pair(serde_json::json!({ "diagnostic": true }))
}

fn assert_compact_shape<T: Serialize>(label: &str, response: &T) -> Value {
    let serialized = serde_json::to_value(response)
        .unwrap_or_else(|error| panic!("{label} should serialize: {error}"));
    assert!(
        serialized.get("metadata").is_none(),
        "{label} compact response should omit metadata"
    );
    assert!(
        serialized.get("note").is_none(),
        "{label} compact response should omit note"
    );
    assert!(
        serialized.get("result_handle").is_some(),
        "{label} compact response should keep result_handle"
    );
    serialized
}

fn assert_compact_handle(
    server: &FriggMcpServer,
    result_handle: &Option<String>,
    line: usize,
    column: Option<usize>,
) {
    let handle = result_handle
        .as_deref()
        .expect("compact presenter should return a result handle");
    let anchor = server
        .session_result_handle_match(handle, "m1")
        .expect("compact presenter should store match m1");
    assert_eq!(anchor.repository_id, "repo-compact");
    assert_eq!(anchor.path, "src/lib.rs");
    assert_eq!(anchor.line, line);
    assert_eq!(anchor.column, column);
}

fn reference_match() -> ReferenceMatch {
    ReferenceMatch {
        match_id: None,
        stable_symbol_id: None,
        repository_id: "repo-compact".to_owned(),
        symbol: "target".to_owned(),
        path: "src/lib.rs".to_owned(),
        line: 11,
        column: 7,
        match_kind: ReferenceMatchKind::Reference,
        precision: Some("heuristic".to_owned()),
        fallback_reason: Some("fixture".to_owned()),
        container: None,
        signature: None,
        follow_up_structural: Vec::new(),
    }
}

fn navigation_location(line: usize, column: usize) -> NavigationLocation {
    NavigationLocation {
        match_id: None,
        stable_symbol_id: None,
        symbol: "target".to_owned(),
        repository_id: "repo-compact".to_owned(),
        path: "src/lib.rs".to_owned(),
        line,
        column,
        kind: Some("function".to_owned()),
        container: None,
        signature: None,
        precision: Some("heuristic".to_owned()),
        follow_up_structural: Vec::new(),
    }
}

fn implementation_match() -> ImplementationMatch {
    ImplementationMatch {
        match_id: None,
        stable_symbol_id: None,
        symbol: "target".to_owned(),
        kind: Some("function".to_owned()),
        repository_id: "repo-compact".to_owned(),
        path: "src/lib.rs".to_owned(),
        line: 13,
        column: 5,
        relation: Some("implementation".to_owned()),
        container: None,
        signature: None,
        precision: Some("heuristic".to_owned()),
        fallback_reason: Some("fixture".to_owned()),
        follow_up_structural: Vec::new(),
    }
}

fn call_match(line: usize, column: usize, relation: &str) -> CallHierarchyMatch {
    CallHierarchyMatch {
        match_id: None,
        source_stable_symbol_id: None,
        target_stable_symbol_id: None,
        source_symbol: "caller".to_owned(),
        target_symbol: "target".to_owned(),
        repository_id: "repo-compact".to_owned(),
        path: "src/lib.rs".to_owned(),
        line,
        column,
        relation: relation.to_owned(),
        source_container: None,
        target_container: None,
        source_signature: None,
        target_signature: None,
        precision: Some("heuristic".to_owned()),
        call_path: Some("src/lib.rs".to_owned()),
        call_line: Some(line),
        call_column: Some(column),
        call_end_line: Some(line),
        call_end_column: Some(column + 4),
        follow_up_structural: Vec::new(),
    }
}
