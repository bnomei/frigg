//! Dogfood board (`surface=dogfood`).
//!
//! Default: realistic monorepo-shaped fixture under
//! `tests/fixtures/futura_dogfood/` (stable `^crates/` anchors).
//!
//! Optional live checkout: set `FUTURA_BENCH_DOGFOOD_ROOT` to pin the Frigg
//! repository root under test (slower; writes `.frigg/` under that root).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use frigg::mcp::types::{
    PUBLIC_TOOL_NAMES, ReadMatchParams, ReadMatchResponse, ReadPresentationMode, ResponseMode,
    SearchBatchParams, SearchBatchProbe, SearchBatchProbeKind, SearchHybridParams,
    SearchPatternType, SearchSymbolParams, SearchTextParams, WorkspaceParams,
};
use rmcp::handler::server::wrapper::Parameters;

use crate::harness::{
    self, Surface, adopt_workspace, cleanup_workspace_root, fixtures_root,
    materialize_fixture_workspace, public_tool_registered, require, server_for_root,
    structured_tool_result,
};

/// Resolve dogfood workspace: env pin, else materialize the dogfood-shaped fixture.
fn prepare_dogfood_workspace() -> (PathBuf, bool) {
    if let Ok(override_root) = std::env::var("FUTURA_BENCH_DOGFOOD_ROOT") {
        let path = PathBuf::from(override_root);
        assert!(
            path.is_dir(),
            "FUTURA_BENCH_DOGFOOD_ROOT is not a directory: {}",
            path.display()
        );
        let path = path
            .canonicalize()
            .unwrap_or_else(|err| panic!("canonicalize dogfood root: {err}"));
        return (path, false);
    }
    let fixture = fixtures_root().join("futura_dogfood");
    assert!(
        fixture.join("crates/cli/src/mcp/types.rs").is_file(),
        "missing dogfood fixture at {}",
        fixture.display()
    );
    (materialize_fixture_workspace(&fixture, "dogfood-board"), true)
}

pub async fn run_all(report: &Mutex<harness::BenchReport>) {
    let (root, ephemeral) = prepare_dogfood_workspace();
    println!("FUTURA_BENCH dogfood root={}", root.display());

    run_workspace_recommended_action(report, &root).await;
    run_hybrid_ranking_note(report, &root).await;
    run_search_batch_multi_probe(report, &root).await;
    run_known_symbol(report, &root).await;
    run_zero_hit_recovery(report, &root).await;
    run_read_match_handle(report, &root).await;
    run_ignored_docs_absence(report, &root).await;

    if ephemeral {
        cleanup_workspace_root(&root);
    }
}

async fn run_workspace_recommended_action(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        adopt_workspace(&server, root).await;
        let response = server
            .workspace(Parameters(WorkspaceParams {
                path: Some(root.display().to_string()),
                repository_id: None,
                set_default: Some(true),
                resolve_mode: None,
            }))
            .await
            .map_err(|e| format!("workspace failed: {e}"))?
            .0;
        require(
            response.recommended_action.is_some(),
            format!("workspace recommended_action missing: {response:?}"),
        )?;
        require(
            response.repository.is_some() || response.session_default,
            "workspace should expose adopted repository",
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "workspace_recommended_action",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_hybrid_ranking_note(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let response = server
            .search_hybrid(Parameters(SearchHybridParams {
                query: "where is the MCP tool surface filtered by profile?".to_owned(),
                repository_id: None,
                language: None,
                limit: Some(10),
                weights: None,
                semantic: Some(false),
                response_mode: Some(ResponseMode::Compact),
                include_context_efficiency: None,
            }))
            .await
            .map_err(|e| format!("search_hybrid failed: {e}"))?
            .0;

        require(
            response.ranking_note.is_some(),
            format!("hybrid ranking_note missing: {:?}", response.ranking_note),
        )?;
        require(
            !response.recovery.suggested_next.is_empty() || response.ranking_note.is_some(),
            format!(
                "hybrid must expose suggested_next or ranking_note: {:?}",
                response.recovery
            ),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "hybrid_ranking_note_suggested_next",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_search_batch_multi_probe(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        if !public_tool_registered("search_batch") {
            return Err("search_batch not in PUBLIC_TOOL_NAMES — skip only if missing".to_owned());
        }
        let server = server_for_root(root).await;
        let response = server
            .search_batch(Parameters(SearchBatchParams {
                probes: vec![
                    SearchBatchProbe {
                        id: "text-public-tools".to_owned(),
                        kind: SearchBatchProbeKind::Text,
                        query: "PUBLIC_TOOL_NAMES".to_owned(),
                        repository_id: None,
                        path_regex: Some(r"^crates/cli/src/mcp/".to_owned()),
                        glob: None,
                        path_class: None,
                        pattern_type: Some(SearchPatternType::Literal),
                    },
                    SearchBatchProbe {
                        id: "symbol-frigg-mcp".to_owned(),
                        kind: SearchBatchProbeKind::Symbol,
                        query: "FriggMcpServer".to_owned(),
                        repository_id: None,
                        path_regex: Some(r"^crates/cli/src/mcp/".to_owned()),
                        glob: None,
                        path_class: None,
                        pattern_type: None,
                    },
                ],
                merge: None,
                limit: Some(20),
                repository_id: None,
                response_mode: Some(ResponseMode::Compact),
                resume_from: None,
            }))
            .await
            .map_err(|e| format!("search_batch failed: {e}"))?
            .0;

        require(
            response.probe_summary.len() == 2,
            format!(
                "expected 2 probe summaries, got {}",
                response.probe_summary.len()
            ),
        )?;
        require(
            !response.matches.is_empty(),
            format!("search_batch should hit dogfood anchors: {response:?}"),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "search_batch_multi_probe",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_known_symbol(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let response = server
            .search_symbol(Parameters(SearchSymbolParams {
                query: "FriggMcpServer".to_owned(),
                repository_id: None,
                path_class: None,
                path_regex: Some(r"^crates/cli/src/mcp/".to_owned()),
                limit: Some(20),
                response_mode: Some(ResponseMode::Compact),
            }))
            .await
            .map_err(|e| format!("search_symbol failed: {e}"))?
            .0;

        require(
            !response.matches.is_empty(),
            format!("expected FriggMcpServer symbol hits: {response:?}"),
        )?;
        require(
            response
                .matches
                .iter()
                .any(|m| m.symbol.contains("FriggMcpServer") || m.path.contains("server")),
            format!("symbol matches look wrong: {:?}", response.matches),
        )?;
        require(
            PUBLIC_TOOL_NAMES.contains(&"search_text"),
            "PUBLIC_TOOL_NAMES must list search_text",
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "known_symbol_search_symbol",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_zero_hit_recovery(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let response = server
            .search_text(Parameters(SearchTextParams {
                query: "zzznomatch_futura_dogfood_unique_token_42".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some(r"^crates/cli/src/mcp/".to_owned()),
                limit: Some(5),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("search_text zero failed: {e}"))?
            .0;

        require(response.total_matches == 0, "expected zero hits")?;
        let has_recovery = response.recovery.zero_hit_reason.is_some()
            || response.recovery.error_code.is_some()
            || !response.recovery.suggested_next.is_empty()
            || response.recovery.correction_hint.is_some();
        require(
            has_recovery,
            format!("zero-hit recovery fields missing: {:?}", response.recovery),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "zero_hit_recovery_fields",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_read_match_handle(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let search = server
            .search_text(Parameters(SearchTextParams {
                query: "PUBLIC_TOOL_NAMES".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some(r"^crates/cli/src/mcp/types\.rs$".to_owned()),
                limit: Some(5),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("search_text for handle failed: {e}"))?
            .0;

        require(
            !search.matches.is_empty(),
            format!("expected PUBLIC_TOOL_NAMES hits: {search:?}"),
        )?;
        let result_handle = search
            .result_handle
            .clone()
            .ok_or_else(|| format!("missing result_handle: {search:?}"))?;
        let match_id = search.matches[0]
            .match_id
            .clone()
            .ok_or_else(|| format!("missing match_id: {:?}", search.matches[0]))?;

        let read: ReadMatchResponse = structured_tool_result(
            server
                .read_match(Parameters(ReadMatchParams {
                    result_handle,
                    match_id,
                    before: Some(1),
                    after: Some(1),
                    presentation_mode: Some(ReadPresentationMode::Json),
                    include_context_efficiency: None,
                }))
                .await
                .map_err(|e| format!("read_match failed: {e}"))?,
        )?;

        require(
            read.content.contains("PUBLIC_TOOL_NAMES") || !read.content.is_empty(),
            format!("read_match content unexpected: {read:?}"),
        )?;
        require(
            read.path.contains("types.rs") || read.path.contains("mcp"),
            format!("read_match path unexpected: {}", read.path),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "read_match_handle_path",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_ignored_docs_absence(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        // Fixture (and live Frigg) keep /docs/ out of the indexed tree. Distinctive
        // futura contract phrases must not appear under docs/ in search hits.
        let server = server_for_root(root).await;
        let response = server
            .search_text(Parameters(SearchTextParams {
                query: "frigg-futura-bench golden suite dogfoods this repository".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: None,
                limit: Some(10),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("ignored-docs search failed: {e}"))?
            .0;

        let hit_docs = response
            .matches
            .iter()
            .any(|m| m.path.starts_with("docs/") || m.path.contains("/docs/"));
        require(
            !hit_docs,
            format!(
                "indexed search must not hit gitignored /docs/: {:?}",
                response.matches
            ),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "ignored_docs_absence",
        Surface::Dogfood,
        started,
        outcome,
    );
}
