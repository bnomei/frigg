//! Synthetic fixture board (`surface=synth`) for forced zeros, regex trap, count_only, handles.

use std::fs;
use std::sync::Mutex;
use std::time::Instant;

use frigg::mcp::types::{
    ReadMatchParams, ReadMatchResponse, ReadPresentationMode, ResponseMode, SearchPatternType,
    SearchTextParams, WorkspaceGateAction, WorkspaceParams,
};
use rmcp::handler::server::wrapper::Parameters;

use crate::harness::{
    self, Surface, cleanup_workspace_root, fixtures_root, materialize_fixture_workspace, require,
    server_for_root, structured_tool_result,
};

pub async fn run_all(report: &Mutex<harness::BenchReport>) {
    let fixture = fixtures_root().join("futura_synth/seed");
    assert!(
        fixture.join("src/lib.rs").is_file(),
        "missing synth seed at {}",
        fixture.display()
    );

    run_regex_trap(report, &fixture).await;
    run_count_only(report, &fixture).await;
    run_zero_hit(report, &fixture).await;
    run_handle_path(report, &fixture).await;
    run_post_edit_dirty_gate(report, &fixture).await;
    crate::slo::run_search_text_latency(report, &fixture).await;
}

async fn run_regex_trap(report: &Mutex<harness::BenchReport>, fixture: &std::path::Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-regex-trap");
    let outcome = async {
        let server = server_for_root(&root).await;
        let response = server
            .search_text(Parameters(SearchTextParams {
                query: "foo|bar_nomatch_unique_synth".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                limit: Some(5),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("regex trap search failed: {e}"))?
            .0;

        require(response.total_matches == 0, "regex trap should zero")?;
        require(
            response.recovery.error_code.as_deref() == Some("QUERY_LOOKS_LIKE_REGEX"),
            format!(
                "expected QUERY_LOOKS_LIKE_REGEX, got {:?}",
                response.recovery.error_code
            ),
        )?;
        require(
            response
                .recovery
                .suggested_next
                .iter()
                .any(|n| n.pattern_type.as_deref() == Some("regex")),
            format!(
                "regex trap should suggest pattern_type=regex: {:?}",
                response.recovery.suggested_next
            ),
        )?;
        Ok(())
    }
    .await;
    cleanup_workspace_root(&root);
    report.lock().unwrap().record(
        "search_text_regex_trap_recovery",
        Surface::Synth,
        started,
        outcome,
    );
}

async fn run_count_only(report: &Mutex<harness::BenchReport>, fixture: &std::path::Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-count-only");
    let outcome = async {
        let server = server_for_root(&root).await;
        let response = server
            .search_text(Parameters(SearchTextParams {
                query: "FUTURA_SYNTH_COUNT_TOKEN".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                limit: None,
                count_only: Some(true),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("count_only search failed: {e}"))?
            .0;

        require(
            response.count_only == Some(true),
            format!("count_only flag not echoed: {response:?}"),
        )?;
        require(
            response.total_matches > 0,
            format!("count_only total_matches should be > 0: {response:?}"),
        )?;
        require(
            response.matches.is_empty(),
            "count_only must leave matches[] empty (not a failure)",
        )?;
        Ok(())
    }
    .await;
    cleanup_workspace_root(&root);
    report
        .lock()
        .unwrap()
        .record("count_only_shape", Surface::Synth, started, outcome);
}

async fn run_zero_hit(report: &Mutex<harness::BenchReport>, fixture: &std::path::Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-zero-hit");
    let outcome = async {
        let server = server_for_root(&root).await;
        let response = server
            .search_text(Parameters(SearchTextParams {
                query: "zzznomatch_synth_unique_token_99".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                limit: Some(5),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("synth zero search failed: {e}"))?
            .0;

        require(response.total_matches == 0, "expected synth zero")?;
        let has = response.recovery.zero_hit_reason.is_some()
            || response.recovery.error_code.is_some()
            || !response.recovery.suggested_next.is_empty()
            || response.recovery.scope.is_some();
        require(
            has,
            format!("synth zero-hit recovery missing: {:?}", response.recovery),
        )?;
        // Scope echo when path_regex applied.
        if let Some(scope) = &response.recovery.scope {
            require(
                scope.path_regex.as_deref() == Some("^src/"),
                format!("scope path_regex echo wrong: {scope:?}"),
            )?;
        }
        Ok(())
    }
    .await;
    cleanup_workspace_root(&root);
    report
        .lock()
        .unwrap()
        .record("zero_hit_recovery_synth", Surface::Synth, started, outcome);
}

async fn run_handle_path(report: &Mutex<harness::BenchReport>, fixture: &std::path::Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-handle");
    let outcome = async {
        let server = server_for_root(&root).await;
        let search = server
            .search_text(Parameters(SearchTextParams {
                query: "FUTURA_SYNTH_HANDLE_ANCHOR".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                limit: Some(5),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("handle search failed: {e}"))?
            .0;

        require(
            !search.matches.is_empty(),
            format!("no handle hits: {search:?}"),
        )?;
        let result_handle = search
            .result_handle
            .ok_or_else(|| "missing result_handle".to_owned())?;
        let match_id = search.matches[0]
            .match_id
            .clone()
            .ok_or_else(|| "missing match_id".to_owned())?;

        let read: ReadMatchResponse = structured_tool_result(
            server
                .read_match(Parameters(ReadMatchParams {
                    result_handle,
                    match_id,
                    before: None,
                    after: None,
                    presentation_mode: Some(ReadPresentationMode::Json),
                    include_context_efficiency: None,
                    origin: None,
                }))
                .await
                .map_err(|e| format!("read_match failed: {e}"))?,
        )?;

        require(
            read.content.contains("FUTURA_SYNTH_HANDLE_ANCHOR") || !read.content.is_empty(),
            format!("handle read content unexpected: {read:?}"),
        )?;
        Ok(())
    }
    .await;
    cleanup_workspace_root(&root);
    report
        .lock()
        .unwrap()
        .record("read_match_handle_synth", Surface::Synth, started, outcome);
}

/// post-edit gate: real file edit + production dirty signals (same path watch uses).
///
/// Watch is not started in bench; after the edit we mark the validated-manifest dirty root and
/// record path-scoped pending dirty paths the way the watch/hot-reindex queue does, then assert
/// `workspace` recommends path-scoped live-disk only (never a repo-wide shell license).
async fn run_post_edit_dirty_gate(report: &Mutex<harness::BenchReport>, fixture: &std::path::Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-post-edit-dirty");
    let outcome = async {
        let server = server_for_root(&root).await;
        let before = server
            .workspace(Parameters(WorkspaceParams {
                path: Some(root.display().to_string()),
                repository_id: None,
                set_default: Some(true),
                resolve_mode: None,
            }))
            .await
            .map_err(|e| format!("workspace before edit failed: {e}"))?
            .0;
        let repository_id = before
            .repository
            .as_ref()
            .map(|r| r.repository_id.clone())
            .ok_or_else(|| "missing repository after adopt".to_owned())?;

        // Real on-disk edit (agent post-edit path).
        let edit_path = root.join("src/lib.rs");
        let mut body = fs::read_to_string(&edit_path).map_err(|e| format!("read lib.rs: {e}"))?;
        body.push_str("\n// FUTURA_POST_EDIT_MARKER\n");
        fs::write(&edit_path, &body).map_err(|e| format!("write lib.rs: {e}"))?;

        // Simulate watch/hot-reindex queue notifications after the edit (same APIs production uses).
        server.test_mark_workspace_dirty_root(&root);
        server.test_record_gate_dirty_paths(&repository_id, &[String::from("src/lib.rs")], &[]);

        let response = server
            .workspace(Parameters(WorkspaceParams {
                path: Some(root.display().to_string()),
                repository_id: None,
                set_default: Some(true),
                resolve_mode: None,
            }))
            .await
            .map_err(|e| format!("workspace after edit failed: {e}"))?
            .0;

        require(
            response.working_tree_dirty == Some(true),
            format!("expected working_tree_dirty after edit: {response:?}"),
        )?;
        require(
            response
                .changed_paths_since_snapshot
                .iter()
                .any(|p| p == "src/lib.rs" || p.ends_with("lib.rs")),
            format!(
                "expected path-scoped changed path src/lib.rs: {:?}",
                response.changed_paths_since_snapshot
            ),
        )?;
        require(
            matches!(
                response.recommended_action,
                Some(WorkspaceGateAction::UseLiveDiskForTouchedFiles)
                    | Some(WorkspaceGateAction::WaitWatch)
                    | Some(WorkspaceGateAction::Reindex)
            ),
            format!(
                "post-edit gate should not stay Ready: {:?}",
                response.recommended_action
            ),
        )?;
        // Path-scoped only: never interpret gate as repo-wide shell grep license.
        if let Some(fresh) = response.fresh_enough_for.as_ref() {
            require(
                fresh
                    .iter()
                    .all(|t| t == "read_file" || t == "read_match" || t == "search_text"),
                format!("fresh_enough_for should stay path-scoped tools: {fresh:?}"),
            )?;
        }
        Ok(())
    }
    .await;
    cleanup_workspace_root(&root);
    report.lock().unwrap().record(
        "post_edit_dirty_gate_path_scoped",
        Surface::Synth,
        started,
        outcome,
    );
}
