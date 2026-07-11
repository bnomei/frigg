//! Freshness and ignore-truth integration tests.

use super::*;
use frigg::mcp::types::{ReadFileResponse, WorkspaceGateAction};

#[tokio::test]
async fn gitignored_tmp_content_absent_from_search_text_but_on_disk() {
    let workspace_root = fresh_fixture_root("tool-handlers-gitignore-search");
    // Fixture seeds src/ignored.tmp with "temporary artifact" and .gitignore "*.tmp".
    assert!(
        workspace_root.join("src/ignored.tmp").is_file(),
        "fixture must keep ignored.tmp on disk"
    );
    let on_disk = fs::read_to_string(workspace_root.join("src/ignored.tmp"))
        .expect("ignored.tmp should be readable from disk");
    assert!(
        on_disk.contains("temporary artifact"),
        "disk content should be present even when ignored"
    );

    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "temporary artifact".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: None,
            limit: Some(20),
            context_lines: None,
            case_sensitive: None,
            ignore_case: None,
            word: None,
            files_with_matches: None,
            count_only: None,
            glob: None,
            exclude_glob: None,
            include_hidden: None,
            max_count_per_file: None,
            collapse_by_file: None,
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("search_text should succeed")
        .0;

    assert_eq!(
        response.total_matches, 0,
        "gitignored *.tmp content must not appear in indexed search_text: {response:?}"
    );
    assert!(
        response
            .matches
            .iter()
            .all(|m| !m.path.ends_with("ignored.tmp")),
        "ignored.tmp must not appear in matches: {:?}",
        response.matches
    );

    // Direct read of an indexed path still works (live disk path for known files).
    let read = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: None,
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .map(structured_tool_result::<ReadFileResponse>)
        .expect("read_file of indexed source should succeed");
    assert_eq!(read.path, "src/lib.rs");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn workspace_gate_live_disk_for_touched_dirty_paths() {
    let workspace_root = fresh_fixture_root("tool-handlers-workspace-live-disk");
    let repository_id = stable_public_repository_id_for_root(&workspace_root);
    // Seed a ready index so the gate prefers path-scoped live-disk over reindex.
    seed_manifest_snapshot(
        &workspace_root,
        &repository_id,
        "snapshot-gate-001",
        &["src/lib.rs", "src/nested/data.txt", "README.md"],
    );
    let server = server_for_workspace_root(&workspace_root).await;

    server.test_mark_workspace_dirty_root(&workspace_root);
    server.test_record_gate_dirty_paths(&repository_id, &[String::from("src/lib.rs")], &[]);

    let response = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await
        .expect("workspace should return gate status")
        .0;

    assert_eq!(
        response.working_tree_dirty,
        Some(true),
        "dirty root / changed paths must set working_tree_dirty"
    );
    assert!(
        response
            .changed_paths_since_snapshot
            .iter()
            .any(|path| path == "src/lib.rs"),
        "changed_paths_since_snapshot should include path-scoped dirty path: {:?}",
        response.changed_paths_since_snapshot
    );
    assert_eq!(
        response.recommended_action,
        Some(WorkspaceGateAction::UseLiveDiskForTouchedFiles),
        "dirty touched paths with ready index should advise path-scoped live-disk, not repo-wide grep: {:?}",
        response.recommended_action
    );
    assert!(
        response
            .fresh_enough_for
            .as_ref()
            .is_some_and(|tools| tools.iter().any(|tool| tool == "read_file")),
        "live-disk advice should still allow read_file for touched paths: {:?}",
        response.fresh_enough_for
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_and_hybrid_emit_latency_class() {
    let workspace_root = fresh_fixture_root("tool-handlers-latency-class");
    let server = server_for_workspace_root(&workspace_root).await;

    let symbol = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: None,
            path_class: Some(SearchSymbolPathClass::Runtime),
            path_regex: Some("^src/".to_owned()),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;
    assert!(
        symbol.latency_class.is_some(),
        "search_symbol must emit latency_class: {symbol:?}"
    );

    let hybrid = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "greeting helper".to_owned(),
            repository_id: None,
            language: None,
            limit: Some(5),
            weights: None,
            semantic: Some(false),
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("search_hybrid should succeed")
        .0;
    assert!(
        hybrid.latency_class.is_some(),
        "search_hybrid must emit latency_class: {hybrid:?}"
    );

    cleanup_workspace_root(&workspace_root);
}
