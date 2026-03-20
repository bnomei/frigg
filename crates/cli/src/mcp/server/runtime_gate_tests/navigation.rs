#![allow(clippy::panic)]

use super::*;

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
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

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
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    let response = server
        .try_precise_definition_fast_path(
            Some(&workspace.repository_id),
            "src/lib.rs",
            1,
            Some(13),
            false,
            10,
        )
        .expect("cached precise fast path should not error")
        .expect("cached precise fast path should resolve a definition");
    assert_eq!(response.1, workspace.repository_id);
    assert_eq!(response.2, "scip-rust pkg repo#User");
    assert_eq!(response.3, "precise");
    assert_eq!(response.0.0.matches.len(), 1);
    assert_eq!(response.0.0.matches[0].path, "src/lib.rs");
    assert_eq!(response.0.0.matches[0].line, 1);

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
    let repository_id = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace")
        .repository_id;
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
        response
            .ancestors
            .iter()
            .any(|node| node.kind == "call_expression"),
        "expected call_expression in ancestor stack, got {:?}",
        response
            .ancestors
            .iter()
            .map(|node| node.kind.clone())
            .collect::<Vec<_>>()
    );

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
    let repository_id = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace")
        .repository_id;
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
    let repository_id = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace")
        .repository_id;
    let error = match server
        .search_structural(rmcp::handler::server::wrapper::Parameters(
            SearchStructuralParams {
                query: "(function_item @broken".to_owned(),
                language: Some("rust".to_owned()),
                repository_id: Some(repository_id),
                path_regex: None,
                limit: Some(10),
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
    let repository_id = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace")
        .repository_id;
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
