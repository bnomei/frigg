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
    PUBLIC_TOOL_NAMES, DocumentSymbolsParams, ImpactBundleParams, ListFilesParams,
    ReadFileParams, ReadMatchParams, ReadMatchResponse, ReadPresentationMode, ResponseMode,
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
    run_citation_read_file(report, &root).await;
    run_list_files_pagination(report, &root).await;
    run_document_symbols_outline(report, &root).await;
    run_impact_bundle_if_registered(report, &root).await;
    run_wrong_repo_zero_recovery(report, &root).await;

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

async fn run_citation_read_file(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let call = server
            .read_file(Parameters(ReadFileParams {
                path: "crates/cli/src/mcp/types.rs".to_owned(),
                repository_id: None,
                start_line: Some(1),
                end_line: Some(5),
                line_count: None,
                max_bytes: None,
                presentation_mode: Some(ReadPresentationMode::Citation),
                include_context_efficiency: None,
            }))
            .await
            .map_err(|e| format!("read_file citation failed: {e}"))?;
        require(
            call.structured_content.is_none(),
            "citation mode must not set structured_content",
        )?;
        let text = call
            .content
            .iter()
            .filter_map(|block| block.as_text().map(|t| t.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        require(
            text.contains("1|") || text.lines().any(|line| line.starts_with("1|")),
            format!("citation mode should emit LINE|content rows, got: {text:?}"),
        )?;
        Ok(())
    }
    .await;
    report
        .lock()
        .unwrap()
        .record("citation_mode_read_file", Surface::Dogfood, started, outcome);
}

async fn run_list_files_pagination(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let first = server
            .list_files(Parameters(ListFilesParams {
                repository_id: None,
                path_regex: Some(r"^crates/cli/src/mcp/".to_owned()),
                glob: Some("*.rs".to_owned()),
                language: None,
                path_class: None,
                include_hidden: None,
                limit: Some(2),
                resume_from: None,
            }))
            .await
            .map_err(|e| format!("list_files failed: {e}"))?
            .0;
        require(first.files.len() <= 2, format!("limit=2 over-returned: {:?}", first.files))?;
        require(
            first.total_files >= first.files.len(),
            format!("total_files inconsistent: {first:?}"),
        )?;
        if first.truncated || first.resume_from.is_some() {
            let resume = first
                .resume_from
                .clone()
                .ok_or_else(|| format!("truncated list missing resume_from: {first:?}"))?;
            let second = server
                .list_files(Parameters(ListFilesParams {
                    repository_id: None,
                    path_regex: Some(r"^crates/cli/src/mcp/".to_owned()),
                    glob: Some("*.rs".to_owned()),
                    language: None,
                    path_class: None,
                    include_hidden: None,
                    limit: Some(2),
                    resume_from: Some(resume),
                }))
                .await
                .map_err(|e| format!("list_files resume failed: {e}"))?
                .0;
            require(
                !second.files.is_empty() || second.total_files <= 2,
                format!("resume page unexpected: {second:?}"),
            )?;
        }
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "list_files_pagination",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_document_symbols_outline(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        let response = server
            .document_symbols(Parameters(DocumentSymbolsParams {
                path: "crates/cli/src/mcp/types.rs".to_owned(),
                repository_id: None,
                top_level_only: Some(true),
                limit: Some(5),
                resume_from: None,
                response_mode: Some(ResponseMode::Compact),
                include_follow_up_structural: None,
            }))
            .await
            .map_err(|e| format!("document_symbols failed: {e}"))?
            .0;
        require(
            response.total_symbols >= response.returned,
            format!("outline pagination dishonest: {response:?}"),
        )?;
        require(
            response.symbols.len() == response.returned,
            format!("returned vs symbols len mismatch: {response:?}"),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "document_symbols_outline_pagination",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_impact_bundle_if_registered(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        if !public_tool_registered("impact_bundle") {
            return Ok(());
        }
        let server = server_for_root(root).await;
        let response = server
            .impact_bundle(Parameters(ImpactBundleParams {
                symbol: "FriggMcpServer".to_owned(),
                repository_id: None,
                path_class: None,
                include_implementations: None,
                response_mode: Some(ResponseMode::Compact),
            }))
            .await
            .map_err(|e| format!("impact_bundle failed: {e}"))?
            .0;
        require(
            !response.symbols.is_empty(),
            format!("impact_bundle expected symbol hits: {response:?}"),
        )?;
        // Composition: prefer non-empty refs/callers when graph available; otherwise recovery.
        let composed = !response.references.is_empty() || !response.incoming_calls.is_empty();
        let has_recovery = response.recovery.correction_hint.is_some()
            || !response.recovery.suggested_next.is_empty()
            || response.recovery.error_code.is_some();
        require(
            composed || has_recovery || !response.suggested_next.is_empty(),
            format!(
                "impact_bundle should compose refs/callers or suggest next: refs={} callers={} recovery={:?}",
                response.references.len(),
                response.incoming_calls.len(),
                response.recovery
            ),
        )?;
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "impact_bundle_composition",
        Surface::Dogfood,
        started,
        outcome,
    );
}

async fn run_wrong_repo_zero_recovery(report: &Mutex<harness::BenchReport>, root: &Path) {
    let started = Instant::now();
    let outcome = async {
        let server = server_for_root(root).await;
        // Explicit nonsense repository_id should fail or zero with recovery, not hang.
        let result = server
            .search_text(Parameters(SearchTextParams {
                query: "PUBLIC_TOOL_NAMES".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: Some("repo-does-not-exist-futura-bench".to_owned()),
                path_regex: Some(r"^crates/".to_owned()),
                limit: Some(5),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await;
        match result {
            Ok(json) => {
                let response = json.0;
                require(
                    response.total_matches == 0,
                    format!("wrong-repo should not hit: {response:?}"),
                )?;
                let has_recovery = response.recovery.zero_hit_reason.is_some()
                    || response.recovery.error_code.is_some()
                    || !response.recovery.suggested_next.is_empty()
                    || response.recovery.correction_hint.is_some()
                    || response.recovery.message.is_some();
                require(
                    has_recovery,
                    format!("wrong-repo zero missing recovery: {:?}", response.recovery),
                )?;
            }
            Err(err) => {
                // Typed invalid/not-found is acceptable recovery for bad repository_id.
                let msg = format!("{err:?}");
                require(
                    msg.contains("repository")
                        || msg.contains("not found")
                        || msg.contains("invalid")
                        || msg.contains("UNKNOWN")
                        || msg.contains("error"),
                    format!("unexpected wrong-repo error shape: {msg}"),
                )?;
            }
        }
        Ok(())
    }
    .await;
    report.lock().unwrap().record(
        "wrong_repo_zero_recovery",
        Surface::Dogfood,
        started,
        outcome,
    );
}
