//! Integration tests for navigation MCP handlers (go-to-definition, implementations, and call hierarchy).

use super::*;
use frigg::mcp::types::{
    ImpactBundleParams, ImpactProofRowTarget, ImpactProofTarget, ImpactSection,
    ImpactSectionExecution, ImpactSectionResult, ImpactSectionRows, ImpactSectionTrust,
    MetadataObject, NavigationMode, NavigationResolutionSource, NavigationTargetSelectionStatus,
    NextActionId, ResultCompleteness, ResultUnit, TargetRef,
};
use frigg::mcp::types::{NextActionOrigin, NextActionTarget, ReplayOriginTarget};

fn root_signature_for_manifest_paths(workspace_root: &Path, paths: &[&str]) -> String {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    fn write_bytes(state: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *state ^= u64::from(*byte);
            *state = state.wrapping_mul(FNV_PRIME);
        }
    }

    fn write_separator(state: &mut u64) {
        write_bytes(state, &[0xff]);
    }

    let mut entries = paths
        .iter()
        .map(|path| {
            let metadata = fs::metadata(workspace_root.join(path))
                .expect("manifest signature path should exist");
            (
                workspace_root.join(path).to_string_lossy().into_owned(),
                metadata.len(),
                metadata.modified().ok().and_then(system_time_to_unix_nanos),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut state = OFFSET_BASIS;
    for (path, size_bytes, mtime_ns) in entries {
        write_bytes(&mut state, path.as_bytes());
        write_separator(&mut state);
        write_bytes(&mut state, &size_bytes.to_le_bytes());
        write_separator(&mut state);
        match mtime_ns {
            Some(value) => {
                write_bytes(&mut state, &[1]);
                write_bytes(&mut state, &value.to_le_bytes());
                write_separator(&mut state);
            }
            None => {
                write_bytes(&mut state, &[0]);
                write_separator(&mut state);
            }
        }
    }
    format!("stable-symbol-v2:{state:016x}")
}

fn assert_exact_target_selection(
    selection: Option<&frigg::mcp::types::NavigationTargetSelectionSummary>,
    expected_source: NavigationResolutionSource,
    expected_stable_symbol_id: &str,
) {
    let selection = selection.expect("target request should expose selection evidence");
    assert_eq!(selection.status, NavigationTargetSelectionStatus::Resolved);
    assert_eq!(selection.resolution_source, expected_source);
    assert_eq!(
        selection.selected_stable_symbol_id.as_deref(),
        Some(expected_stable_symbol_id)
    );
    assert_eq!(selection.candidate_count, 1);
    assert_eq!(selection.same_rank_candidate_count, 1);
    assert!(!selection.ambiguous_query);
    assert!(selection.candidates.is_empty());
}

fn assert_bound_target_pair(
    family: &str,
    result_handle: Option<&str>,
    match_id: Option<&str>,
    target_ref: Option<&TargetRef>,
) {
    let result_handle = result_handle.unwrap_or_else(|| panic!("{family} should issue a handle"));
    let match_id = match_id.unwrap_or_else(|| panic!("{family} should issue a match id"));
    assert!(
        matches!(
            target_ref,
            Some(TargetRef::ResultMatch {
                result_handle: target_handle,
                match_id: target_match_id,
                ..
            }) if target_handle == result_handle && target_match_id == match_id
        ),
        "{family} child row should carry the exact bound target pair"
    );
}

async fn assert_result_target_replays_to_definition(
    server: &FriggMcpServer,
    family: &str,
    target: &TargetRef,
    expected_repository_id: &str,
    expected_stable_symbol_id: Option<&str>,
) {
    let replay = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(target.clone()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .unwrap_or_else(|error| panic!("{family} child target should replay unchanged: {error:?}"))
        .0;
    let selection = replay
        .target_selection
        .as_ref()
        .expect("replayed result target should expose selection evidence");
    assert_eq!(selection.status, NavigationTargetSelectionStatus::Resolved);
    assert_eq!(
        selection.resolution_source,
        NavigationResolutionSource::ResultMatch
    );
    assert_eq!(selection.candidate_count, 1);
    assert_eq!(selection.same_rank_candidate_count, 1);
    assert!(!selection.ambiguous_query);
    if let Some(expected_stable_symbol_id) = expected_stable_symbol_id {
        assert_eq!(
            selection.selected_stable_symbol_id.as_deref(),
            Some(expected_stable_symbol_id)
        );
    } else {
        assert!(
            selection.selected_stable_symbol_id.is_some(),
            "coordinate-bound {family} target should resolve one indexed symbol"
        );
    }
    assert!(
        replay
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id),
        "{family} child target must remain repository-bound"
    );
}

fn assert_target_error_contract(error: &rmcp::ErrorData, expected_code: &str, private_root: &Path) {
    assert_eq!(error_code_tag(error), Some(expected_code));
    let data = error
        .data
        .as_ref()
        .expect("target failure should include structured recovery");
    assert_eq!(data["error_code"], expected_code);
    assert!(data["correction_hint"].is_string());
    assert!(data["related_tools"].is_array());
    assert_eq!(data["next_actions"], serde_json::json!([]));
    assert_eq!(data["suggested_next"], serde_json::json!([]));
    let wire = serde_json::to_string(data).expect("target error data should serialize");
    assert!(
        !wire.contains(&private_root.to_string_lossy().into_owned()),
        "target errors must not expose absolute workspace paths"
    );
    for private_key in [
        "source_digest",
        "source_bytes",
        "source_revision",
        "resolved_absolute_path",
        "cache_key",
        "authorization",
    ] {
        assert!(data.get(private_key).is_none());
    }
}

async fn assert_target_routes_through_every_consumer(
    server: &FriggMcpServer,
    target: &TargetRef,
    expected_source: NavigationResolutionSource,
    expected_repository_id: &str,
    expected_stable_symbol_id: &str,
) {
    let references = server
        .find_references(Parameters(FindReferencesParams {
            target: Some(target.clone()),
            include_definition: Some(true),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("find_references should accept the exact target")
        .0;
    assert_exact_target_selection(
        references.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert!(
        references
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id)
    );

    let definition = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(target.clone()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("go_to_definition should accept the exact target")
        .0;
    assert_exact_target_selection(
        definition.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert!(
        definition
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id)
    );

    let declarations = server
        .find_declarations(Parameters(FindDeclarationsParams {
            target: Some(target.clone()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("find_declarations should accept the exact target")
        .0;
    assert_exact_target_selection(
        declarations.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert!(
        declarations
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id)
    );

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: Some(target.clone()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("find_implementations should accept the exact target")
        .0;
    assert_exact_target_selection(
        implementations.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert!(
        implementations
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id)
    );

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: Some(target.clone()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("incoming_calls should accept the exact target")
        .0;
    assert_exact_target_selection(
        incoming.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert!(
        incoming
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id)
    );

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: Some(target.clone()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("outgoing_calls should accept the exact target")
        .0;
    assert_exact_target_selection(
        outgoing.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert!(
        outgoing
            .matches
            .iter()
            .all(|matched| matched.repository_id == expected_repository_id)
    );

    let impact = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: Some(target.clone()),
            symbol: String::new(),
            path_class: None,
            repository_id: None,
            include_implementations: Some(true),
            include_test_mentions: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("impact_bundle should accept the exact target")
        .0;
    assert_exact_target_selection(
        impact.target_selection.as_ref(),
        expected_source,
        expected_stable_symbol_id,
    );
    assert_eq!(impact.symbol, "target");
    assert_eq!(impact.symbols.len(), 1);
    assert!(
        !impact.references.is_empty(),
        "target fixture must produce impact reference children"
    );
    assert!(
        !impact.incoming_calls.is_empty(),
        "target fixture must produce impact incoming-call children"
    );
    assert!(
        !impact.implementations.is_empty(),
        "target fixture must produce impact implementation children"
    );
    assert_eq!(impact.symbols[0].repository_id, expected_repository_id);
    assert_eq!(
        impact.symbols[0].stable_symbol_id.as_deref(),
        Some(expected_stable_symbol_id)
    );
    assert_bound_target_pair(
        "impact symbols",
        impact.symbols_result_handle.as_deref(),
        impact.symbols[0].match_id.as_deref(),
        impact.symbols[0].target_ref.as_ref(),
    );
    let symbol_section = impact
        .sections
        .iter()
        .find(|section| section.section == ImpactSection::Symbol)
        .expect("target-mode impact should retain the resolved symbol section");
    assert_eq!(symbol_section.execution, ImpactSectionExecution::Included);
    assert_eq!(
        symbol_section.result_handle.as_deref(),
        impact.symbols_result_handle.as_deref(),
        "the authoritative symbol section must keep the legacy handle projection"
    );
    assert_eq!(symbol_section.proof_targets.len(), 1);
    let symbol_proof = &symbol_section.proof_targets[0];
    assert!(impact.proof_targets.contains(symbol_proof));
    assert_eq!(
        symbol_proof.target.result_handle.as_str(),
        impact
            .symbols_result_handle
            .as_deref()
            .expect("target-mode impact symbol should issue a handle")
    );
    assert_eq!(
        symbol_proof.target.match_id.as_str(),
        impact.symbols[0]
            .match_id
            .as_deref()
            .expect("target-mode impact symbol should issue a match id")
    );
    let issued_symbol_target = impact.symbols[0]
        .target_ref
        .clone()
        .expect("target-mode impact symbol should issue a child target");
    assert_result_target_replays_to_definition(
        server,
        "impact symbols",
        &issued_symbol_target,
        expected_repository_id,
        Some(
            impact.symbols[0]
                .stable_symbol_id
                .as_deref()
                .expect("impact symbol child should preserve stable identity"),
        ),
    )
    .await;
    if let Some(implementation_action) =
        impact
            .recovery
            .next_actions
            .iter()
            .find_map(|action| match &action.target {
                NextActionTarget::FindImplementations(params) => Some(params),
                _ => None,
            })
    {
        assert_eq!(
            implementation_action.target.as_ref(),
            Some(&issued_symbol_target)
        );
        assert!(implementation_action.symbol.is_none());
        assert!(implementation_action.path.is_none());
        server
            .find_implementations(Parameters(implementation_action.clone()))
            .await
            .expect("impact's target-bearing implementation action should replay unchanged");
    }
    for matched in &impact.references {
        assert_eq!(matched.repository_id, expected_repository_id);
        assert_bound_target_pair(
            "impact references",
            impact.references_result_handle.as_deref(),
            matched.match_id.as_deref(),
            matched.target_ref.as_ref(),
        );
        assert_result_target_replays_to_definition(
            server,
            "impact references",
            matched
                .target_ref
                .as_ref()
                .expect("impact reference child should issue a target"),
            expected_repository_id,
            Some(
                matched
                    .stable_symbol_id
                    .as_deref()
                    .expect("impact reference child should preserve stable identity"),
            ),
        )
        .await;
    }
    for matched in &impact.incoming_calls {
        assert_eq!(matched.repository_id, expected_repository_id);
        assert_bound_target_pair(
            "impact incoming calls",
            impact.incoming_calls_result_handle.as_deref(),
            matched.match_id.as_deref(),
            matched.target_ref.as_ref(),
        );
        assert_result_target_replays_to_definition(
            server,
            "impact incoming calls",
            matched
                .target_ref
                .as_ref()
                .expect("impact incoming-call child should issue a target"),
            expected_repository_id,
            Some(
                matched
                    .source_stable_symbol_id
                    .as_deref()
                    .expect("impact incoming-call child should preserve source identity"),
            ),
        )
        .await;
    }
    for matched in &impact.implementations {
        assert_eq!(matched.repository_id, expected_repository_id);
        assert_bound_target_pair(
            "impact implementations",
            impact.implementations_result_handle.as_deref(),
            matched.match_id.as_deref(),
            matched.target_ref.as_ref(),
        );
        assert_result_target_replays_to_definition(
            server,
            "impact implementations",
            matched
                .target_ref
                .as_ref()
                .expect("impact implementation child should issue a target"),
            expected_repository_id,
            matched.stable_symbol_id.as_deref(),
        )
        .await;
    }
}

fn assert_response_metadata_has_freshness(metadata: &Option<MetadataObject>, tool_name: &str) {
    let metadata = metadata
        .as_ref()
        .unwrap_or_else(|| panic!("{tool_name} should emit typed metadata"));
    let freshness = metadata
        .get("freshness_basis")
        .unwrap_or_else(|| panic!("{tool_name} metadata should include freshness_basis"));
    assert!(
        freshness
            .get("cacheable")
            .and_then(|value| value.as_bool())
            .is_some(),
        "{tool_name} freshness_basis should include cacheable"
    );
    let repository = freshness
        .get("repositories")
        .and_then(|value| value.as_array())
        .and_then(|repositories| repositories.first())
        .unwrap_or_else(|| panic!("{tool_name} freshness_basis should include repository details"));
    assert!(
        repository
            .get("dirty_root")
            .and_then(|value| value.as_bool())
            .is_some(),
        "{tool_name} freshness repository should include dirty_root"
    );
    assert!(
        repository
            .get("candidate_source")
            .and_then(|value| value.as_str())
            .is_some(),
        "{tool_name} freshness repository should include candidate_source"
    );
    assert!(
        repository
            .get("using_live_walk")
            .and_then(|value| value.as_bool())
            .is_some(),
        "{tool_name} freshness repository should include using_live_walk"
    );
    assert!(
        repository
            .get("refresh_in_progress")
            .and_then(|value| value.as_bool())
            .is_some(),
        "{tool_name} freshness repository should include refresh_in_progress"
    );
    assert!(
        repository
            .get("active_index_tasks")
            .and_then(|value| value.as_array())
            .is_some(),
        "{tool_name} freshness repository should include active_index_tasks"
    );
    assert!(
        repository
            .get("recommended_client_behavior")
            .and_then(|value| value.as_str())
            .is_some(),
        "{tool_name} freshness repository should include recommended_client_behavior"
    );
}

#[tokio::test]
async fn navigation_go_to_definition_prefers_precise_matches() {
    let workspace_root = temp_workspace_root("go-to-definition-precise");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 33, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;
    let repository_id = public_repository_id(&server).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should resolve precise definition")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, repository_id);
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, 12);
    assert_eq!(response.matches[0].kind.as_deref(), Some("struct"));
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("go_to_definition should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_defaults_to_compact_but_keeps_mode_and_handles() {
    let workspace_root = temp_workspace_root("go-to-definition-compact-default");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_compact.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 33, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: None,
        }))
        .await
        .expect("compact go_to_definition should resolve")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert!(response.metadata.is_none());
    assert!(response.note.is_none());
    assert!(
        response.result_handle.is_some(),
        "compact go_to_definition should return a result handle"
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.match_id.is_some()),
        "compact go_to_definition matches should expose match ids"
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.target_ref.is_some()),
        "compact navigation rows should publish target refs with their match ids"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn result_target_enforces_exclusive_inputs_repository_assertions_and_source_freshness() {
    let workspace_root = temp_workspace_root("result-target-request-validation");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let source_path = src_root.join("lib.rs");
    fs::write(&source_path, "pub fn target() {}\n")
        .expect("failed to seed temporary fixture source");
    let other_root = temp_workspace_root("result-target-request-validation-other");
    fs::create_dir_all(other_root.join("src")).expect("failed to create second fixture");
    fs::write(other_root.join("src/lib.rs"), "pub fn other() {}\n")
        .expect("failed to seed second fixture source");
    let config =
        FriggConfig::from_workspace_roots(vec![workspace_root.clone(), other_root.clone()])
            .expect("two fixture roots must produce valid config");
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;
    let repository_id = stable_public_repository_id_for_root(&workspace_root);
    let other_repository_id = stable_public_repository_id_for_root(&other_root);

    let produced = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("search_symbol should issue a target")
        .0;
    let target = produced
        .matches
        .first()
        .and_then(|row| row.target_ref.clone())
        .expect("symbol row should carry its executable target");

    let _accepted_stable_assertion = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(target.clone()),
            repository_id: Some(repository_id.clone()),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("an equal repository assertion should be accepted");

    let text_target = server
        .search_text(Parameters(SearchTextParams {
            query: "target".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search_text should issue a coordinate-bound target")
        .0
        .matches
        .into_iter()
        .find_map(|row| row.target_ref)
        .expect("text row should carry its coordinate target");
    let _accepted_result_assertion = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(text_target),
            repository_id: Some(repository_id.clone()),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("an exact symbol-start coordinate target should resolve");

    let conflict = match server
        .find_references(Parameters(FindReferencesParams {
            target: Some(target.clone()),
            symbol: Some("target".to_owned()),
            repository_id: Some(repository_id.clone()),
            include_definition: Some(true),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("target plus direct symbol must be rejected"),
    };
    assert_eq!(conflict.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&conflict), Some("CONFLICTING_TARGET_INPUT"));

    let precedence_conflict = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(target.clone()),
            symbol: Some("target".to_owned()),
            repository_id: Some(repository_id.clone()),
            limit: Some(0),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("target conflict preflight must run before generic limit validation"),
    };
    assert_eq!(precedence_conflict.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error_code_tag(&precedence_conflict),
        Some("CONFLICTING_TARGET_INPUT")
    );

    let precedence_mismatch = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(target.clone()),
            repository_id: Some(other_repository_id.clone()),
            limit: Some(0),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("target repository preflight must run before generic limit validation"),
    };
    assert_eq!(precedence_mismatch.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error_code_tag(&precedence_mismatch),
        Some("TARGET_REPOSITORY_MISMATCH")
    );

    let mismatch = match server
        .find_references(Parameters(FindReferencesParams {
            target: Some(target.clone()),
            repository_id: Some(other_repository_id),
            include_definition: Some(true),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a conflicting repository assertion must be rejected"),
    };
    assert_eq!(mismatch.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error_code_tag(&mismatch),
        Some("TARGET_REPOSITORY_MISMATCH")
    );

    fs::write(
        &source_path,
        "// changed before any watcher invalidation\npub fn target() {}\n",
    )
    .expect("fixture mutation should persist");
    let stale = match server
        .find_references(Parameters(FindReferencesParams {
            target: Some(target),
            repository_id: Some(repository_id),
            include_definition: Some(true),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("navigation must verify the producer source revision before dispatch"),
    };
    assert_eq!(stale.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(error_code_tag(&stale), Some("STALE_PROOF_ANCHOR"));
    let data = stale
        .data
        .expect("stale target failure should include structured recovery");
    assert!(data["correction_hint"].is_string());
    assert!(data["related_tools"].is_array());
    assert_eq!(data["next_actions"], serde_json::json!([]));
    assert!(data.get("resolved_absolute_path").is_none());
    assert!(data.get("source_revision").is_none());

    cleanup_workspace_root(&workspace_root);
    cleanup_workspace_root(&other_root);
}

#[tokio::test]
async fn result_target_routes_through_every_navigation_consumer() {
    let workspace_root = temp_workspace_root("result-target-consumer-matrix-a");
    let collision_root = temp_workspace_root("result-target-consumer-matrix-b");
    let source = "pub fn target() {}\n\
                  pub fn target_impl() {}\n\
                  pub fn caller() { target(); }\n";
    let precise_fixture = r#"{
      "documents": [{
        "relative_path": "src/lib.rs",
        "occurrences": [
          { "symbol": "scip-rust pkg shared#target", "range": [0, 7, 13], "symbol_roles": 1 },
          { "symbol": "scip-rust pkg shared#target_impl", "range": [1, 7, 18], "symbol_roles": 1 },
          { "symbol": "scip-rust pkg shared#caller", "range": [2, 7, 13], "symbol_roles": 1 },
          { "symbol": "scip-rust pkg shared#target", "range": [2, 18, 24], "symbol_roles": 8 }
        ],
        "symbols": [
          { "symbol": "scip-rust pkg shared#target", "display_name": "target", "kind": "function", "relationships": [] },
          { "symbol": "scip-rust pkg shared#target_impl", "display_name": "target_impl", "kind": "function",
            "relationships": [{ "symbol": "scip-rust pkg shared#target", "is_implementation": true }] },
          { "symbol": "scip-rust pkg shared#caller", "display_name": "caller", "kind": "function", "relationships": [] }
        ]
      }]
    }"#;
    for root in [&workspace_root, &collision_root] {
        fs::create_dir_all(root.join("src")).expect("failed to create collision fixture");
        fs::write(root.join("src/lib.rs"), source)
            .expect("failed to seed collision fixture source");
        write_scip_fixture(root, "same-stable-id.json", precise_fixture);
        let repository_id = stable_public_repository_id_for_root(root);
        seed_manifest_snapshot(root, &repository_id, "snapshot-001", &["src/lib.rs"]);
    }
    let repository_id = stable_public_repository_id_for_root(&workspace_root);
    let collision_repository_id = stable_public_repository_id_for_root(&collision_root);
    let config =
        FriggConfig::from_workspace_roots(vec![workspace_root.clone(), collision_root.clone()])
            .expect("collision roots must produce valid config");
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;

    let produced = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("search_symbol should issue the matrix target")
        .0;
    let row = produced
        .matches
        .first()
        .expect("symbol fixture should expose target");
    let stable_symbol_id = row
        .stable_symbol_id
        .clone()
        .expect("symbol row should expose stable identity");
    let result_target = row
        .target_ref
        .clone()
        .expect("symbol row should carry the matrix target");

    let collision = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some(collision_repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("collision repository should expose the same target")
        .0;
    assert_eq!(collision.matches[0].symbol, "target");
    let collision_stable_symbol_id = collision.matches[0]
        .stable_symbol_id
        .clone()
        .expect("collision symbol should expose stable identity");
    let stable_target = TargetRef::StableSymbol {
        repository_id: repository_id.clone(),
        stable_symbol_id: stable_symbol_id.clone(),
        snapshot_token: root_signature_for_manifest_paths(&workspace_root, &["src/lib.rs"]),
    };

    assert_target_routes_through_every_consumer(
        &server,
        &result_target,
        NavigationResolutionSource::ResultMatch,
        &repository_id,
        &stable_symbol_id,
    )
    .await;

    assert_target_routes_through_every_consumer(
        &server,
        &stable_target,
        NavigationResolutionSource::StableSymbol,
        &repository_id,
        &stable_symbol_id,
    )
    .await;

    let target_actions = produced
        .recovery
        .next_actions
        .iter()
        .filter(|action| {
            matches!(
                action.target,
                NextActionTarget::GoToDefinition(_)
                    | NextActionTarget::FindReferences(_)
                    | NextActionTarget::ImpactBundle(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(target_actions.len(), 3);
    for action in target_actions {
        match &action.target {
            NextActionTarget::GoToDefinition(params) => {
                assert_eq!(params.target.as_ref(), Some(&result_target));
                server
                    .go_to_definition(Parameters(params.clone()))
                    .await
                    .expect("issued definition action should replay unchanged");
            }
            NextActionTarget::FindReferences(params) => {
                assert_eq!(params.target.as_ref(), Some(&result_target));
                server
                    .find_references(Parameters(params.clone()))
                    .await
                    .expect("issued references action should replay unchanged");
            }
            NextActionTarget::ImpactBundle(params) => {
                assert_eq!(params.target.as_ref(), Some(&result_target));
                assert!(params.symbol.is_empty());
                server
                    .impact_bundle(Parameters(params.clone()))
                    .await
                    .expect("issued impact action should replay unchanged");
            }
            _ => unreachable!(),
        }
    }
    assert_eq!(
        collision_stable_symbol_id, stable_symbol_id,
        "matching source and SCIP identity in two repositories must expose the same public stable symbol id"
    );

    cleanup_workspace_root(&workspace_root);
    cleanup_workspace_root(&collision_root);
}

#[tokio::test]
async fn stable_and_coordinate_targets_cover_fixed_errors_mutation_and_legacy_sources() {
    let workspace_root = temp_workspace_root("target-error-and-coordinate-matrix");
    let other_root = temp_workspace_root("target-error-and-coordinate-matrix-other");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create target fixture");
    fs::create_dir_all(other_root.join("src")).expect("failed to create assertion fixture");
    let source_path = workspace_root.join("src/lib.rs");
    fs::write(
        &source_path,
        "// orphan_marker\npub fn target() { let inside_marker = 1; }// boundary_marker\n",
    )
    .expect("failed to seed target fixture");
    fs::write(other_root.join("src/lib.rs"), "pub fn other() {}\n")
        .expect("failed to seed assertion fixture");
    let repository_id = stable_public_repository_id_for_root(&workspace_root);
    let other_repository_id = stable_public_repository_id_for_root(&other_root);
    seed_manifest_snapshot(
        &workspace_root,
        &repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    seed_manifest_snapshot(
        &other_root,
        &other_repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    let config =
        FriggConfig::from_workspace_roots(vec![workspace_root.clone(), other_root.clone()])
            .expect("target fixtures must produce valid config");
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;

    let symbol_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("symbol fixture should be indexed")
        .0;
    let stable_symbol_id = symbol_response.matches[0]
        .stable_symbol_id
        .clone()
        .expect("symbol fixture should expose stable identity");
    let snapshot_token = root_signature_for_manifest_paths(&workspace_root, &["src/lib.rs"]);
    let stable_target = TargetRef::StableSymbol {
        repository_id: repository_id.clone(),
        stable_symbol_id: stable_symbol_id.clone(),
        snapshot_token: snapshot_token.clone(),
    };

    let stale_snapshot = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(TargetRef::StableSymbol {
                repository_id: repository_id.clone(),
                stable_symbol_id: stable_symbol_id.clone(),
                snapshot_token: "stale-snapshot-token".to_owned(),
            }),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a stale corpus token must fail"),
    };
    assert_target_error_contract(&stale_snapshot, "STALE_TARGET_SNAPSHOT", &workspace_root);

    let pre_algorithm_snapshot = snapshot_token
        .strip_prefix("stable-symbol-v2:")
        .expect("current target snapshots must carry the stable-symbol algorithm version")
        .to_owned();
    let pre_algorithm_target = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(TargetRef::StableSymbol {
                repository_id: repository_id.clone(),
                stable_symbol_id: stable_symbol_id.clone(),
                snapshot_token: pre_algorithm_snapshot,
            }),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a pre-algorithm-version target must fail closed"),
    };
    assert_target_error_contract(
        &pre_algorithm_target,
        "STALE_TARGET_SNAPSHOT",
        &workspace_root,
    );

    let missing_symbol = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(TargetRef::StableSymbol {
                repository_id: repository_id.clone(),
                stable_symbol_id: "stable-id-not-in-this-corpus".to_owned(),
                snapshot_token: snapshot_token.clone(),
            }),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("an absent stable symbol must fail"),
    };
    assert_target_error_contract(&missing_symbol, "TARGET_NOT_FOUND", &workspace_root);

    let repository_mismatch = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(stable_target.clone()),
            repository_id: Some(other_repository_id.clone()),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a mismatched stable-target repository assertion must fail"),
    };
    assert_target_error_contract(
        &repository_mismatch,
        "TARGET_REPOSITORY_MISMATCH",
        &workspace_root,
    );

    let missing_repository = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(TargetRef::StableSymbol {
                repository_id: "repo-not-attached".to_owned(),
                stable_symbol_id: stable_symbol_id.clone(),
                snapshot_token: snapshot_token.clone(),
            }),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("an absent target repository must fail"),
    };
    assert_target_error_contract(&missing_repository, "REPOSITORY_NOT_FOUND", &workspace_root);

    let orphan_response = server
        .search_text(Parameters(SearchTextParams {
            query: "orphan_marker".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("orphan marker should produce a coordinate target")
        .0;
    assert!(
        orphan_response
            .recovery
            .next_actions
            .iter()
            .all(|action| !matches!(
                action.target,
                NextActionTarget::GoToDefinition(_)
                    | NextActionTarget::FindReferences(_)
                    | NextActionTarget::ImpactBundle(_)
            )),
        "an anchor known to be semantically unresolvable must not advertise navigation actions"
    );
    let orphan_target = orphan_response.matches[0]
        .target_ref
        .clone()
        .expect("text row should expose a result target");
    let insufficient = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(orphan_target),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a coordinate outside every indexed symbol must fail closed"),
    };
    assert_target_error_contract(&insufficient, "TARGET_ANCHOR_INSUFFICIENT", &workspace_root);

    let boundary_response = server
        .search_text(Parameters(SearchTextParams {
            query: "boundary_marker".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("same-line boundary marker should produce a coordinate target")
        .0;
    assert!(
        boundary_response
            .recovery
            .next_actions
            .iter()
            .all(|action| !matches!(
                action.target,
                NextActionTarget::GoToDefinition(_)
                    | NextActionTarget::FindReferences(_)
                    | NextActionTarget::ImpactBundle(_)
            )),
        "a row at a symbol's exclusive end must not advertise navigation actions"
    );
    let boundary_target = boundary_response.matches[0]
        .target_ref
        .clone()
        .expect("boundary text row should expose a proof target");
    let boundary_error = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(boundary_target),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("tree-sitter's exclusive symbol end must not resolve as containment"),
    };
    assert_target_error_contract(
        &boundary_error,
        "TARGET_ANCHOR_INSUFFICIENT",
        &workspace_root,
    );

    let inside_target = server
        .search_text(Parameters(SearchTextParams {
            query: "inside_marker".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("inside marker should produce a coordinate target")
        .0
        .matches[0]
        .target_ref
        .clone()
        .expect("inside text row should expose a result target");
    let enclosing = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(inside_target),
            ..Default::default()
        }))
        .await
        .expect("the unique smallest enclosing symbol should resolve")
        .0;
    assert_exact_target_selection(
        enclosing.target_selection.as_ref(),
        NavigationResolutionSource::ResultMatch,
        &stable_symbol_id,
    );

    let direct_symbol = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("target".to_owned()),
            repository_id: Some(repository_id.clone()),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("legacy symbol navigation should remain valid")
        .0;
    assert_exact_target_selection(
        direct_symbol.target_selection.as_ref(),
        NavigationResolutionSource::DirectSymbol,
        &stable_symbol_id,
    );
    let direct_location = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            repository_id: Some(repository_id.clone()),
            path: Some("src/lib.rs".to_owned()),
            line: Some(2),
            column: Some(27),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("legacy location navigation should remain valid")
        .0;
    assert_exact_target_selection(
        direct_location.target_selection.as_ref(),
        NavigationResolutionSource::DirectLocation,
        &stable_symbol_id,
    );

    rewrite_file_with_new_mtime(
        &source_path,
        "// orphan_marker\npub fn changed_target() { let inside_marker = 1; }\n",
    );
    seed_manifest_snapshot(
        &workspace_root,
        &repository_id,
        "snapshot-002",
        &["src/lib.rs"],
    );
    let mutated_config =
        FriggConfig::from_workspace_roots(vec![workspace_root.clone(), other_root.clone()])
            .expect("mutated target fixtures must produce valid config");
    let mutated_server = FriggMcpServer::new(mutated_config);
    attach_session_repositories(&mutated_server).await;
    let changed = mutated_server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "changed_target".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("the changed corpus should be adopted before stale-target validation")
        .0;
    assert_eq!(changed.matches[0].symbol, "changed_target");
    let stale_after_mutation = match mutated_server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(stable_target),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("an old stable target must fail after corpus mutation"),
    };
    assert_target_error_contract(
        &stale_after_mutation,
        "STALE_TARGET_SNAPSHOT",
        &workspace_root,
    );

    cleanup_workspace_root(&workspace_root);
    cleanup_workspace_root(&other_root);
}

#[tokio::test]
async fn navigation_go_to_definition_falls_back_to_direct_precise_symbol_when_corpus_symbol_is_missing()
 {
    let workspace_root = temp_workspace_root("go-to-definition-direct-precise-symbol");
    let views_root = workspace_root.join("resources/views");
    let lang_root = workspace_root.join("lang");
    fs::create_dir_all(&views_root).expect("failed to create blade fixture root");
    fs::create_dir_all(&lang_root).expect("failed to create lang fixture root");
    fs::write(
        views_root.join("welcome.blade.php"),
        "{{ __('Settings') }}\n",
    )
    .expect("failed to seed blade fixture");
    fs::write(
        lang_root.join("en.json"),
        "{\n  \"Settings\": \"Settings\"\n}\n",
    )
    .expect("failed to seed lang fixture");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_translations.json",
        r#"{
          "documents": [
            {
              "relative_path": "lang/en.json",
              "occurrences": [
                { "symbol": "trans/`json:Settings`.", "range": [1, 3, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "trans/`json:Settings`.",
                  "display_name": "Settings",
                  "kind": "string",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "resources/views/welcome.blade.php",
              "occurrences": [
                { "symbol": "trans/`json:Settings`.", "range": [0, 6, 16], "symbol_roles": 8 }
              ],
              "symbols": []
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("Settings".to_owned()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should fall back to direct precise symbols")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "Settings");
    assert_eq!(response.matches[0].path, "lang/en.json");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 4);
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "symbol_precise_direct");
    assert_eq!(note_json["target_precise_symbol"], "trans/`json:Settings`.");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_uses_php_helper_literal_for_direct_precise_lookup() {
    let workspace_root = temp_workspace_root("go-to-definition-php-helper-precise");
    let views_root = workspace_root.join("resources/views");
    let lang_root = workspace_root.join("lang");
    fs::create_dir_all(&views_root).expect("failed to create blade fixture root");
    fs::create_dir_all(&lang_root).expect("failed to create lang fixture root");
    let blade_source = "{{ __('Settings') }}\n";
    fs::write(views_root.join("welcome.blade.php"), blade_source)
        .expect("failed to seed blade fixture");
    fs::write(
        lang_root.join("en.json"),
        "{\n  \"Settings\": \"Settings\"\n}\n",
    )
    .expect("failed to seed lang fixture");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_php_helper.json",
        r#"{
          "documents": [
            {
              "relative_path": "lang/en.json",
              "occurrences": [
                { "symbol": "trans/`json:Settings`.", "range": [1, 3, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "trans/`json:Settings`.",
                  "display_name": "Settings",
                  "kind": "string",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("resources/views/welcome.blade.php".to_owned()),
            line: Some(1),
            column: Some(blade_source.find("Settings").expect("literal should exist") + 4),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should use the helper literal for precise lookup")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "Settings");
    assert_eq!(response.matches[0].path, "lang/en.json");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 4);

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_php_helper");
    assert_eq!(note_json["target_precise_symbol"], "trans/`json:Settings`.");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_uses_route_helper_literal_for_direct_precise_lookup() {
    let workspace_root = temp_workspace_root("go-to-definition-route-helper-precise");
    let views_root = workspace_root.join("resources/views");
    let routes_root = workspace_root.join("routes");
    fs::create_dir_all(&views_root).expect("failed to create blade fixture root");
    fs::create_dir_all(&routes_root).expect("failed to create routes fixture root");
    let blade_source = "{{ route('dashboard') }}\n";
    let route_source = "Route::get('/dashboard', fn () => view('welcome'))->name('dashboard');\n";
    fs::write(views_root.join("welcome.blade.php"), blade_source)
        .expect("failed to seed blade fixture");
    fs::write(routes_root.join("web.php"), route_source).expect("failed to seed route fixture");
    let route_definition_column = route_source
        .rfind("dashboard")
        .expect("route name should exist in definition");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_route_helper.json",
        &format!(
            r#"{{
          "documents": [
            {{
              "relative_path": "routes/web.php",
              "occurrences": [
                {{ "symbol": "route/`name:dashboard`.", "range": [0, {route_definition_column}, {route_definition_column_end}], "symbol_roles": 1 }}
              ],
              "symbols": [
                {{
                  "symbol": "route/`name:dashboard`.",
                  "display_name": "dashboard",
                  "kind": "route",
                  "relationships": []
                }}
              ]
            }}
          ]
        }}"#,
            route_definition_column = route_definition_column,
            route_definition_column_end = route_definition_column + "dashboard".len(),
        ),
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("resources/views/welcome.blade.php".to_owned()),
            line: Some(1),
            column: Some(
                blade_source
                    .find("dashboard")
                    .expect("literal should exist")
                    + 4,
            ),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should use the route helper literal for precise lookup")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "dashboard");
    assert_eq!(response.matches[0].path, "routes/web.php");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, route_definition_column + 1);

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_php_helper");
    assert_eq!(
        note_json["target_precise_symbol"],
        "route/`name:dashboard`."
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_uses_blade_attribute_route_helper_literal_for_direct_precise_lookup()
 {
    let workspace_root = temp_workspace_root("go-to-definition-blade-attribute-route-helper");
    let views_root = workspace_root.join("resources/views/partials/sidebar");
    let routes_root = workspace_root.join("routes");
    fs::create_dir_all(&views_root).expect("failed to create blade fixture root");
    fs::create_dir_all(&routes_root).expect("failed to create routes fixture root");
    let blade_source = r#"<x-nav-link href="{{ route('dashboard') }}">Dashboard</x-nav-link>"#;
    let route_source = "Route::get('/dashboard', fn () => view('welcome'))->name('dashboard');\n";
    fs::write(views_root.join("primary-nav.blade.php"), blade_source)
        .expect("failed to seed blade fixture");
    fs::write(routes_root.join("web.php"), route_source).expect("failed to seed route fixture");
    let route_definition_column = route_source
        .rfind("dashboard")
        .expect("route name should exist in definition");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_blade_attribute_route_helper.json",
        &format!(
            r#"{{
          "documents": [
            {{
              "relative_path": "routes/web.php",
              "occurrences": [
                {{ "symbol": "route/`name:dashboard`.", "range": [0, {route_definition_column}, {route_definition_column_end}], "symbol_roles": 1 }}
              ],
              "symbols": [
                {{
                  "symbol": "route/`name:dashboard`.",
                  "display_name": "dashboard",
                  "kind": "route",
                  "relationships": []
                }}
              ]
            }}
          ]
        }}"#,
            route_definition_column = route_definition_column,
            route_definition_column_end = route_definition_column + "dashboard".len(),
        ),
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("resources/views/partials/sidebar/primary-nav.blade.php".to_owned()),
            line: Some(1),
            column: Some(
                blade_source
                    .find("dashboard")
                    .expect("literal should exist")
                    + 4,
            ),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should use the Blade attribute route helper literal")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "dashboard");
    assert_eq!(response.matches[0].path, "routes/web.php");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, route_definition_column + 1);

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_php_helper");
    assert_eq!(
        note_json["target_precise_symbol"],
        "route/`name:dashboard`."
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_prefers_route_helper_precise_match_when_cursor_is_on_helper_prefix()
 {
    let workspace_root = temp_workspace_root("go-to-definition-blade-route-helper-prefix");
    let sidebar_root = workspace_root.join("resources/views/partials/sidebar");
    let flux_root = workspace_root.join("resources/views/flux/navlist");
    let routes_root = workspace_root.join("routes");
    fs::create_dir_all(&sidebar_root).expect("failed to create sidebar blade fixture root");
    fs::create_dir_all(&flux_root).expect("failed to create flux blade fixture root");
    fs::create_dir_all(&routes_root).expect("failed to create routes fixture root");
    let blade_source = concat!(
        "<flux:navlist.group class=\"grid\">\n",
        "    <flux:navlist.item icon=\"home\" :href=\"route('dashboard')\" :current=\"request()->routeIs('dashboard')\" wire:navigate>\n",
        "        <span class=\"sidebar-nav-label\">{{ __('Dashboard') }}</span>\n",
        "    </flux:navlist.item>\n",
    );
    let group_source = "<div>shadow dashboard module</div>\n";
    let route_source = "Route::get('/dashboard', fn () => view('welcome'))->name('dashboard');\n";
    fs::write(sidebar_root.join("primary-nav.blade.php"), blade_source)
        .expect("failed to seed sidebar blade fixture");
    fs::write(flux_root.join("group.blade.php"), group_source)
        .expect("failed to seed flux blade fixture");
    fs::write(routes_root.join("web.php"), route_source).expect("failed to seed route fixture");
    let line_one = blade_source.lines().next().expect("line one should exist");
    let line_two = blade_source.lines().nth(1).expect("line two should exist");
    let line_three = blade_source
        .lines()
        .nth(2)
        .expect("line three should exist");
    let group_reference_column = line_one
        .find("navlist.group")
        .expect("group reference should exist");
    let route_literal_column = line_two
        .find("'dashboard'")
        .expect("route literal should exist")
        + 1;
    let current_route_literal_column = line_two
        .rfind("'dashboard'")
        .expect("routeIs literal should exist")
        + 1;
    let route_helper_column = line_two.find("route(").expect("route helper should exist") + 2;
    let dashboard_label_column = line_three
        .find("Dashboard")
        .expect("dashboard label should exist");
    let route_definition_column = route_source
        .rfind("dashboard")
        .expect("route definition should exist");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_blade_route_helper_prefix.json",
        &format!(
            r#"{{
          "documents": [
            {{
              "relative_path": "resources/views/partials/sidebar/primary-nav.blade.php",
              "occurrences": [
                {{ "symbol": "views/`partials.sidebar.primary-nav`.", "range": [0, 0, 1], "symbol_roles": 1 }},
                {{ "symbol": "alpha/dashboard.", "range": [0, {group_reference_column}, {group_reference_column_end}], "symbol_roles": 8 }},
                {{ "symbol": "route/`name:dashboard`.", "range": [1, {route_literal_column}, {route_literal_column_end}], "symbol_roles": 8 }},
                {{ "symbol": "route/`name:dashboard`.", "range": [1, {current_route_literal_column}, {current_route_literal_column_end}], "symbol_roles": 8 }},
                {{ "symbol": "trans/`json:Dashboard`.", "range": [2, {dashboard_label_column}, {dashboard_label_column_end}], "symbol_roles": 8 }}
              ],
              "symbols": [
                {{
                  "symbol": "views/`partials.sidebar.primary-nav`.",
                  "display_name": "partials.sidebar.primary-nav",
                  "kind": "module",
                  "relationships": []
                }}
              ]
            }},
            {{
              "relative_path": "resources/views/flux/navlist/group.blade.php",
              "occurrences": [
                {{ "symbol": "alpha/dashboard.", "range": [0, 0, 1], "symbol_roles": 1 }}
              ],
              "symbols": [
                {{
                  "symbol": "alpha/dashboard.",
                  "display_name": "dashboard",
                  "kind": "module",
                  "relationships": []
                }}
              ]
            }},
            {{
              "relative_path": "routes/web.php",
              "occurrences": [
                {{ "symbol": "route/`name:dashboard`.", "range": [0, {route_definition_column}, {route_definition_column_end}], "symbol_roles": 1 }}
              ],
              "symbols": [
                {{
                  "symbol": "route/`name:dashboard`.",
                  "display_name": "dashboard",
                  "kind": "route",
                  "relationships": []
                }}
              ]
            }}
          ]
        }}"#,
            group_reference_column = group_reference_column,
            group_reference_column_end = group_reference_column + "navlist.group".len(),
            route_literal_column = route_literal_column,
            route_literal_column_end = route_literal_column + "dashboard".len(),
            current_route_literal_column = current_route_literal_column,
            current_route_literal_column_end = current_route_literal_column + "dashboard".len(),
            dashboard_label_column = dashboard_label_column,
            dashboard_label_column_end = dashboard_label_column + "Dashboard".len(),
            route_definition_column = route_definition_column,
            route_definition_column_end = route_definition_column + "dashboard".len(),
        ),
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("resources/views/partials/sidebar/primary-nav.blade.php".to_owned()),
            line: Some(2),
            column: Some(route_helper_column),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should prefer route helper precise match")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "dashboard");
    assert_eq!(response.matches[0].path, "routes/web.php");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, route_definition_column + 1);

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_php_helper");
    assert_eq!(
        note_json["target_precise_symbol"],
        "route/`name:dashboard`."
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_prefers_route_source_fallback_for_blade_attribute_helpers() {
    let workspace_root = temp_workspace_root("go-to-definition-blade-route-helper-source-fallback");
    let views_root = workspace_root.join("resources/views/partials/sidebar");
    let general_views_root = workspace_root.join("resources/views");
    let routes_root = workspace_root.join("routes");
    fs::create_dir_all(&views_root).expect("failed to create blade fixture root");
    fs::create_dir_all(&general_views_root).expect("failed to create general blade fixture root");
    fs::create_dir_all(&routes_root).expect("failed to create routes fixture root");
    let blade_source = r#"<x-nav-link href="{{ route('dashboard') }}">Dashboard</x-nav-link>"#;
    let route_source = "Route::get('/dashboard', fn () => view('welcome'))->name('dashboard');\n";
    fs::write(views_root.join("primary-nav.blade.php"), blade_source)
        .expect("failed to seed blade fixture");
    fs::write(
        general_views_root.join("dashboard.blade.php"),
        "<div>dashboard view</div>\n",
    )
    .expect("failed to seed same-named blade view fixture");
    fs::write(routes_root.join("web.php"), route_source).expect("failed to seed route fixture");
    let route_definition_column = route_source
        .rfind("dashboard")
        .expect("route name should exist in definition");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("resources/views/partials/sidebar/primary-nav.blade.php".to_owned()),
            line: Some(1),
            column: Some(
                blade_source
                    .find("dashboard")
                    .expect("literal should exist")
                    + 4,
            ),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should prefer route source fallback")
        .0;

    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "dashboard");
    assert_eq!(response.matches[0].path, "routes/web.php");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, route_definition_column + 1);
    assert_eq!(response.matches[0].kind.as_deref(), Some("route"));
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(
        note_json["resolution_source"],
        "location_token_php_helper_route_source"
    );
    assert_eq!(note_json["fallback_reason"], "route_helper_source");
    assert_eq!(note_json["target_route_name"], "dashboard");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_recomputes_stale_manifest_scoped_results_after_edit() {
    let workspace_root = temp_workspace_root("go-to-definition-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root).await;
    let first = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("alpha".to_owned()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("initial go_to_definition call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "alpha");
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("beta_beta".to_owned()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should recompute after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "beta_beta");
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert_eq!(
        second
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("freshness_basis"))
            .and_then(|value| value.get("cacheable"))
            .and_then(|value| value.as_bool()),
        Some(false),
        "stale manifest-backed navigation should surface non-cacheable freshness metadata until a fresh snapshot exists"
    );

    let stale = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
    {
        Ok(_) => panic!("go_to_definition should not return stale matches"),
        Err(error) => error,
    };
    assert_eq!(error_code_tag(&stale), Some("resource_not_found"));
    assert_eq!(retryable_tag(&stale), Some(false));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_resolves_same_line_target_by_path_line_and_column() {
    let workspace_root = temp_workspace_root("go-to-definition-location-same-line");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.php"),
        "<?php function alpha() {} function beta() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(1),
            column: Some(35),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should resolve by location")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "beta");
    assert_eq!(response.matches[0].path, "src/lib.php");

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_enclosing_symbol");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_rust_use_path_prefers_imported_symbol_over_same_file_name() {
    let workspace_root = temp_workspace_root("go-to-definition-rust-use-import");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create rust fixture");
    fs::write(src_root.join("worker.rs"), "pub fn helper() {}\n")
        .expect("failed to seed imported helper fixture");
    let use_line = "use crate::worker::helper;\n";
    fs::write(
        src_root.join("app.rs"),
        format!("pub fn helper() {{}}\n{use_line}pub fn call() {{ helper(); }}\n"),
    )
    .expect("failed to seed ambiguous import fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/app.rs".to_owned()),
            line: Some(2),
            column: Some(use_line.find("helper").expect("import token present") + 1),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should prefer the imported Rust symbol at use sites")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "helper");
    assert_eq!(response.matches[0].path, "src/worker.rs");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit location-token metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_rust");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_rust_reexport_alias_resolves_underlying_symbol() {
    let workspace_root = temp_workspace_root("go-to-definition-rust-reexport-alias");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create rust fixture");
    fs::write(src_root.join("worker.rs"), "pub fn helper() {}\n")
        .expect("failed to seed imported helper fixture");
    let reexport_line = "pub use crate::worker::helper as local_helper;\n";
    fs::write(
        src_root.join("lib.rs"),
        format!("{reexport_line}pub fn local_helper() {{}}\n"),
    )
    .expect("failed to seed re-export alias fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            line: Some(1),
            column: Some(
                reexport_line
                    .find("local_helper")
                    .expect("alias token present")
                    + 1,
            ),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should resolve the underlying re-exported Rust symbol")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "helper");
    assert_eq!(response.matches[0].path, "src/worker.rs");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit location-token metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_rust");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_rust_method_call_prefers_impl_method_over_free_function() {
    let workspace_root = temp_workspace_root("go-to-definition-rust-method-vs-function");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create rust fixture");
    let call_line = "    fn call(&self) { self.render(); }\n";
    fs::write(
        src_root.join("lib.rs"),
        format!(
            "fn render() {{}}\n\
             trait Renderer {{ fn render(&self); }}\n\
             struct App;\n\
             impl Renderer for App {{\n\
                 fn render(&self) {{}}\n\
{call_line}\
             }}\n"
        ),
    )
    .expect("failed to seed rust method fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            line: Some(6),
            column: Some(call_line.rfind("render").expect("method token present") + 1),
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should prefer the impl method at a Rust field call site")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "render");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 5);
    assert_eq!(response.matches[0].kind.as_deref(), Some("method"));
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit location-token metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_rust");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_requires_disambiguation_for_same_rank_symbol_queries() {
    let workspace_root = temp_workspace_root("go-to-definition-runtime-first");
    let src_root = workspace_root.join("src");
    let benches_root = workspace_root.join("benches");
    fs::create_dir_all(&src_root).expect("failed to create runtime fixture");
    fs::create_dir_all(&benches_root).expect("failed to create bench fixture");
    fs::write(src_root.join("lib.rs"), "pub fn try_execute() {}\n")
        .expect("failed to seed runtime fixture source");
    fs::write(
        benches_root.join("runtime_bottlenecks.rs"),
        "pub fn try_execute() {}\n",
    )
    .expect("failed to seed bench fixture source");

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("try_execute".to_owned()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should report disambiguation for ambiguous exact-name queries")
        .0;

    assert!(response.matches.is_empty());
    assert_eq!(response.mode, NavigationMode::UnavailableNoPrecise);
    let selection = response
        .target_selection
        .as_ref()
        .expect("go_to_definition should surface target selection details");
    assert_eq!(
        selection.status,
        NavigationTargetSelectionStatus::DisambiguationRequired
    );
    assert_eq!(selection.symbol_query, "try_execute");
    assert_eq!(selection.candidate_count, 2);
    assert_eq!(selection.same_rank_candidate_count, 2);
    assert_eq!(selection.candidates.len(), 2);
    assert_eq!(selection.candidates[0].path, "src/lib.rs");
    assert_eq!(
        selection.candidates[1].path,
        "benches/runtime_bottlenecks.rs"
    );
    assert!(selection.candidates[0].stable_symbol_id.is_some());

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit disambiguation metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["disambiguation_required"], true);
    assert_eq!(
        note_json["target_selection"]["status"],
        "disambiguation_required"
    );
    assert_eq!(note_json["target_selection"]["candidate_count"], 2);
    assert_eq!(
        note_json["target_selection"]["same_rank_candidate_count"],
        2
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_precise_results_round_trip_through_stable_symbol_id() {
    let workspace_root = temp_workspace_root("go-to-definition-precise-target-pinning");
    let src_root = workspace_root.join("src");
    let benches_root = workspace_root.join("benches");
    fs::create_dir_all(&src_root).expect("failed to create runtime fixture");
    fs::create_dir_all(&benches_root).expect("failed to create bench fixture");
    fs::write(src_root.join("lib.rs"), "pub fn try_execute() {}\n")
        .expect("failed to seed runtime fixture source");
    fs::write(
        benches_root.join("runtime_bottlenecks.rs"),
        "pub fn try_execute() {}\n",
    )
    .expect("failed to seed bench fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_target_pinning.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "range": [0, 7, 18], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#z_runtime_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "benches/runtime_bottlenecks.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#a_bench_try_execute", "range": [0, 7, 18], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#a_bench_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let search = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "try_execute".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("search_symbol should surface ambiguous exact-name candidates")
        .0;
    let runtime_symbol_id = search
        .matches
        .iter()
        .find(|matched| matched.path == "src/lib.rs")
        .and_then(|matched| matched.stable_symbol_id.clone())
        .expect("runtime search result should expose a stable symbol id");

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some(runtime_symbol_id.clone()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should resolve a stable symbol id without name ambiguity")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(
        response.matches[0].stable_symbol_id.as_deref(),
        Some(runtime_symbol_id.as_str())
    );
    let selection = response
        .target_selection
        .as_ref()
        .expect("go_to_definition should keep resolved target selection details");
    assert_eq!(selection.status, NavigationTargetSelectionStatus::Resolved);
    assert_eq!(
        selection.selected_stable_symbol_id.as_deref(),
        Some(runtime_symbol_id.as_str())
    );

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/lib.rs");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );
    assert_eq!(note_json["precision"], "precise");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_degrades_when_any_scip_artifact_exceeds_budget() {
    let workspace_root = temp_workspace_root("go-to-definition-scip-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 33, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120).await;
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should retain partial precise definitions")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(
        response.matches[0].precision.as_deref(),
        Some("precise_partial")
    );

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit partial precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["precision"], "precise_partial");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "artifact_budget_bytes"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_falls_back_when_partial_precise_has_no_target_match() {
    let workspace_root = temp_workspace_root("go-to-definition-partial-precise-absence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "other_symbol.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Admin", "range": [0, 0, 5], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Admin",
                  "display_name": "Admin",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120).await;
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: None,
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should fall back when partial precise data lacks the target")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(
        note_json["precise_absence_reason"],
        "precise_partial_non_authoritative_absence"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_declarations_falls_back_to_heuristic_without_precise_data() {
    let server = server_for_fixture().await;
    let repository_id = public_repository_id(&server).await;
    let response = server
        .find_declarations(Parameters(FindDeclarationsParams {
            target: None,
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_declarations should return deterministic fallback")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, repository_id);
    assert_eq!(response.matches[0].symbol, "greeting");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("find_declarations should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_declarations note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["declaration_mode"], "definition_anchor_v1");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
}

#[tokio::test]
async fn navigation_find_declarations_recomputes_stale_manifest_scoped_results_after_edit() {
    let workspace_root = temp_workspace_root("find-declarations-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root).await;
    let first = server
        .find_declarations(Parameters(FindDeclarationsParams {
            target: None,
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("initial find_declarations call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "alpha");
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .find_declarations(Parameters(FindDeclarationsParams {
            target: None,
            symbol: Some("beta_beta".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_declarations should recompute after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "beta_beta");
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert_eq!(
        second
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("freshness_basis"))
            .and_then(|value| value.get("cacheable"))
            .and_then(|value| value.as_bool()),
        Some(false),
        "stale manifest-backed declaration lookup should surface non-cacheable freshness metadata until a fresh snapshot exists"
    );

    let stale = match server
        .find_declarations(Parameters(FindDeclarationsParams {
            target: None,
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
    {
        Ok(_) => panic!("find_declarations should not return stale matches"),
        Err(error) => error,
    };
    assert_eq!(error_code_tag(&stale), Some("resource_not_found"));
    assert_eq!(retryable_tag(&stale), Some(false));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_location_tools_opt_in_return_follow_up_structural() {
    let workspace_root = temp_workspace_root("navigation-location-follow-up-structural");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn greeting() -> &'static str { \"hello\" }\n\
         pub fn caller() { let _ = greeting(); }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root).await;

    let go_to_definition = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("greeting".to_owned()),
            target: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("go_to_definition should return follow-up structural suggestions when opted in")
        .0;
    assert!(!go_to_definition.matches.is_empty());
    assert!(!go_to_definition.matches[0].follow_up_structural.is_empty());
    assert_eq!(
        go_to_definition.matches[0].follow_up_structural[0]
            .params
            .query,
        "(function_item) @match"
    );

    let declarations = server
        .find_declarations(Parameters(FindDeclarationsParams {
            target: None,
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_declarations should return follow-up structural suggestions when opted in")
        .0;
    assert!(!declarations.matches.is_empty());
    assert!(!declarations.matches[0].follow_up_structural.is_empty());
    assert_eq!(
        declarations.matches[0].follow_up_structural[0]
            .params
            .path_regex
            .as_deref(),
        Some("^src/lib\\.rs$")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_falls_back_to_symbol_impl_heuristic() {
    let workspace_root = temp_workspace_root("navigation-implementations-heuristic");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root).await;
    let repository_id = public_repository_id(&server).await;

    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should return deterministic heuristic fallback")
        .0;

    assert!(
        !response.matches.is_empty(),
        "expected heuristic implementation matches from symbol corpus fallback"
    );
    let first = &response.matches[0];
    assert_eq!(first.repository_id, repository_id);
    assert_eq!(first.path, "src/lib.rs");
    assert_eq!(first.symbol, "Impl");
    assert_eq!(first.relation.as_deref(), Some("implements"));
    assert_eq!(first.precision.as_deref(), Some("heuristic"));
    assert_eq!(
        first.fallback_reason.as_deref(),
        Some("precise_absent_rust_impl_index")
    );
    assert_response_metadata_has_freshness(&response.metadata, "find_implementations");

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(
        note_json["fallback_reason"],
        "precise_absent_rust_impl_index"
    );
    assert_eq!(
        note_json["precise"]["implementation_count"].as_u64(),
        Some(response.matches.len() as u64)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_reports_missing_precise_matches_when_php_symbols_lack_relationships()
 {
    let workspace_root = temp_workspace_root("navigation-implementations-php-missing-edges");
    let contracts_root = workspace_root.join("app/Support/Analytics/Contracts");
    let drivers_root = workspace_root.join("app/Support/Analytics/Drivers");
    fs::create_dir_all(&contracts_root).expect("failed to create contracts fixture root");
    fs::create_dir_all(&drivers_root).expect("failed to create drivers fixture root");
    let interface_source = "<?php\n\
namespace App\\Support\\Analytics\\Contracts;\n\
\n\
interface AnalyticsRecorder\n\
{\n\
    public function capture(object $event): void;\n\
}\n";
    let implementation_source = "<?php\n\
namespace App\\Support\\Analytics\\Drivers;\n\
\n\
use App\\Support\\Analytics\\Contracts\\AnalyticsRecorder;\n\
\n\
class NullAnalyticsRecorder implements AnalyticsRecorder\n\
{\n\
    public function capture(object $event): void {}\n\
}\n";
    fs::write(
        contracts_root.join("AnalyticsRecorder.php"),
        interface_source,
    )
    .expect("failed to seed interface fixture");
    fs::write(
        drivers_root.join("NullAnalyticsRecorder.php"),
        implementation_source,
    )
    .expect("failed to seed implementation fixture");
    write_scip_fixture(
        &workspace_root,
        "php_missing_implementation_edges.json",
        r#"{
          "documents": [
            {
              "relative_path": "app/Support/Analytics/Contracts/AnalyticsRecorder.php",
              "occurrences": [
                { "symbol": "scip-php composer app#App\\Support\\Analytics\\Contracts\\AnalyticsRecorder", "range": [3, 10, 27], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-php composer app#App\\Support\\Analytics\\Contracts\\AnalyticsRecorder",
                  "display_name": "AnalyticsRecorder",
                  "kind": "interface",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "app/Support/Analytics/Drivers/NullAnalyticsRecorder.php",
              "occurrences": [
                { "symbol": "scip-php composer app#App\\Support\\Analytics\\Drivers\\NullAnalyticsRecorder", "range": [5, 6, 27], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-php composer app#App\\Support\\Analytics\\Drivers\\NullAnalyticsRecorder",
                  "display_name": "NullAnalyticsRecorder",
                  "kind": "class",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("AnalyticsRecorder".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should report missing precise matches when relationships are absent")
        .0;

    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "NullAnalyticsRecorder");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(
        note_json["precise_absence_reason"],
        "required_precise_matches_not_present_in_precise_graph"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_classifies_blanket_rust_impls_without_precise_graph() {
    let workspace_root = temp_workspace_root("navigation-implementations-rust-blanket");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait DeterministicDrawExt {}\n\
         pub struct Wrapper<T>(pub T);\n\
         impl<T> DeterministicDrawExt for Wrapper<T> {}\n",
    )
    .expect("failed to seed blanket impl fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("DeterministicDrawExt".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should classify blanket impl fallback")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "Wrapper<T>");
    assert_eq!(
        response.matches[0].relation.as_deref(),
        Some("implements_blanket")
    );
    assert_eq!(
        response.matches[0].fallback_reason.as_deref(),
        Some("precise_absent_rust_impl_index")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_degrades_when_scip_artifact_exceeds_budget() {
    let workspace_root = temp_workspace_root("navigation-implementations-scip-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "implementations.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );

    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120).await;
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should retain partial precise implementations")
        .0;

    assert!(
        !response.matches.is_empty(),
        "partial precise mode should still return implementation matches"
    );
    assert_eq!(
        response.matches[0].precision.as_deref(),
        Some("precise_partial")
    );
    assert_eq!(response.matches[0].fallback_reason, None);

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit partial precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "precise_partial");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "artifact_budget_bytes"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_implementations_and_call_hierarchy_prefer_precise_relationships() {
    let workspace_root = temp_workspace_root("navigation-precise-relationships");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n\
         pub fn serve() {}\n\
         pub fn consumer() { serve(); let _ = ServiceMarker; }\n\
         pub struct ServiceMarker;\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "relationships.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#serve", "range": [3, 7, 12], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#consumer", "range": [4, 7, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#serve", "range": [4, 28, 33] }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#consumer",
                  "display_name": "consumer",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_reference": true },
                    { "symbol": "scip-rust pkg repo#serve", "is_reference": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#serve",
                  "display_name": "serve",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should resolve precise relationships")
        .0;
    assert_eq!(implementations.matches.len(), 1);
    assert_eq!(implementations.matches[0].symbol, "Impl");
    assert_eq!(
        implementations.matches[0].relation.as_deref(),
        Some("implementation")
    );
    assert_eq!(
        implementations.matches[0].precision.as_deref(),
        Some("precise")
    );
    assert_response_metadata_has_freshness(&implementations.metadata, "find_implementations");

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("incoming_calls should resolve precise relationships")
        .0;
    assert_eq!(incoming.matches.len(), 1);
    assert_eq!(incoming.matches[0].source_symbol, "consumer");
    assert_eq!(incoming.matches[0].target_symbol, "Service");
    assert_eq!(incoming.matches[0].relation, "refers_to");
    assert_eq!(incoming.matches[0].precision.as_deref(), Some("precise"));
    assert_response_metadata_has_freshness(&incoming.metadata, "incoming_calls");

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: None,
            symbol: Some("consumer".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("outgoing_calls should resolve precise relationships")
        .0;
    assert_eq!(outgoing.matches.len(), 1);
    assert_eq!(outgoing.matches[0].source_symbol, "consumer");
    assert_eq!(outgoing.matches[0].target_symbol, "serve");
    assert_eq!(outgoing.matches[0].relation, "calls");
    assert_eq!(outgoing.matches[0].precision.as_deref(), Some("precise"));
    assert_response_metadata_has_freshness(&outgoing.metadata, "outgoing_calls");
    assert_eq!(
        outgoing.trust,
        frigg::mcp::types::NavigationEdgeTrust::Provisional,
        "live outgoing_calls must keep wire trust=provisional"
    );
    assert!(
        outgoing.trust_note.contains("provisional") && outgoing.trust_note.contains("read_file"),
        "live outgoing_calls must keep always-on trust_note: {}",
        outgoing.trust_note
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_prefers_relationship_bearing_precise_candidate_across_artifacts()
 {
    let workspace_root = temp_workspace_root("navigation-implementations-precise-overlay");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct ImplA;\n\
         impl Service for ImplA {}\n\
         pub struct ImplB;\n\
         impl Service for ImplB {}\n\
         pub struct ImplC;\n\
         impl Service for ImplC {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "a-canary.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#ImplA", "range": [1, 11, 16], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#ImplB", "range": [3, 11, 16], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#ImplC", "range": [5, 11, 16], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#ImplA",
                  "display_name": "ImplA",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#ImplB",
                  "display_name": "ImplB",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#ImplC",
                  "display_name": "ImplC",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );
    write_scip_fixture(
        &workspace_root,
        "z-main.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "rust-analyzer cargo repo 0.1.0 svc/Service#", "range": [0, 10, 17], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "rust-analyzer cargo repo 0.1.0 svc/Service#",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should resolve precise overlay relationships")
        .0;

    assert_eq!(response.matches.len(), 3);
    assert_eq!(response.matches[0].symbol, "ImplA");
    assert_eq!(response.matches[1].symbol, "ImplB");
    assert_eq!(response.matches[2].symbol, "ImplC");
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.precision.as_deref() == Some("precise"))
    );

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_precise_symbol"],
        "scip-rust pkg repo#Service"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_phase_two_precise_tools_opt_in_return_follow_up_structural() {
    let workspace_root = temp_workspace_root("navigation-phase-two-follow-up-structural");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         pub fn consumer(_service: &dyn Service) { serve(); }\n\
         pub fn serve() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "phase_two_follow_up.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#consumer", "range": [2, 7, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Service", "range": [2, 31, 38], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#serve", "range": [2, 42, 47], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#serve", "range": [3, 7, 12], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#consumer",
                  "display_name": "consumer",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_reference": true },
                    { "symbol": "scip-rust pkg repo#serve", "is_reference": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#serve",
                  "display_name": "serve",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should return follow-up structural suggestions when opted in")
        .0;
    assert_eq!(implementations.matches.len(), 1);
    assert!(!implementations.matches[0].follow_up_structural.is_empty());

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("incoming_calls should return follow-up structural suggestions when opted in")
        .0;
    assert_eq!(incoming.matches.len(), 1);
    assert!(!incoming.matches[0].follow_up_structural.is_empty());

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: None,
            symbol: Some("consumer".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("outgoing_calls should return follow-up structural suggestions when opted in")
        .0;
    assert_eq!(outgoing.matches.len(), 1);
    assert!(!outgoing.matches[0].follow_up_structural.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_uses_precise_occurrences_when_relationships_are_absent() {
    let workspace_root = temp_workspace_root("navigation-implementations-precise-occurrences");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "impl-occurrences.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Service", "range": [2, 5, 12], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("find_implementations should derive precise implementations from occurrences")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "Impl");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 12);
    assert_eq!(
        response.matches[0].relation.as_deref(),
        Some("implementation")
    );
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_uses_precise_occurrences_when_relationships_are_absent() {
    let workspace_root = temp_workspace_root("navigation-incoming-precise-occurrences");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub fn first(_service: &dyn Service) {}\n\
         pub fn second(_service: &dyn Service) {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "incoming.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Service", "range": [1, 28, 35], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#Service", "range": [2, 29, 36], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#first", "range": [1, 7, 12], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#second", "range": [2, 7, 13], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#first",
                  "display_name": "first",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#second",
                  "display_name": "second",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: None,
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("incoming_calls should derive precise callers from precise references")
        .0;

    assert_eq!(response.matches.len(), 2);
    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches[0].source_symbol, "first");
    assert_eq!(response.matches[1].source_symbol, "second");
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.precision.as_deref() == Some("precise"))
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.relation == "refers_to")
    );

    let note = response
        .note
        .as_ref()
        .expect("incoming_calls should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("incoming_calls note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["precise"]["incoming_count"], 2);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_marks_callable_precise_occurrences_as_calls() {
    let workspace_root = temp_workspace_root("navigation-incoming-precise-call-sites");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn callee() {}\n\
         pub fn caller() {\n\
             callee();\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "incoming-calls.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#callee", "range": [0, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#caller", "range": [1, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#callee", "range": [2, 4, 10], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#callee",
                  "display_name": "callee",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#caller",
                  "display_name": "caller",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: None,
            symbol: Some("callee".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("incoming_calls should classify callable precise references as calls")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "caller");
    assert_eq!(response.matches[0].target_symbol, "callee");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(response.matches[0].call_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));
    assert_eq!(response.matches[0].call_end_line, Some(3));
    assert_eq!(response.matches[0].call_end_column, Some(11));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_matches_precise_typescript_symbols_without_display_names() {
    let workspace_root = temp_workspace_root("navigation-incoming-typescript-symbol-tail");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "const requireServerUser = () => {};\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-incoming.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [0, 6, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:handler.",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "display_name": "",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:handler.",
                  "display_name": "handler",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: None,
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("incoming_calls should resolve precise TypeScript callers")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "handler");
    assert_eq!(response.matches[0].target_symbol, "requireServerUser");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(
        response.matches[0].call_path.as_deref(),
        Some("src/auth.ts")
    );
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));

    let note = response
        .note
        .as_ref()
        .expect("incoming_calls should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("incoming_calls note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_precise_symbol"],
        "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser."
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_marks_unspecified_typescript_occurrences_as_calls() {
    let workspace_root =
        temp_workspace_root("navigation-incoming-typescript-unspecified-callable-kind");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "export function requireServerUser() {}\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-incoming-unspecified.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [0, 16, 33],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            target: None,
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("incoming_calls should classify explicit TypeScript call sites as calls")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "handler");
    assert_eq!(response.matches[0].target_symbol, "requireServerUser");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(
        response.matches[0].call_path.as_deref(),
        Some("src/auth.ts")
    );
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_uses_precise_occurrences_when_relationships_are_absent() {
    let workspace_root = temp_workspace_root("navigation-outgoing-precise-occurrences");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn alpha() {}\n\
         pub fn beta() {}\n\
         pub const GAMMA: usize = 1;\n\
         pub struct Marker;\n\
         pub fn caller() {\n\
             alpha();\n\
             beta();\n\
             let _ = GAMMA;\n\
             let _ = Marker;\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "outgoing.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#alpha", "range": [0, 7, 12], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#beta", "range": [1, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#GAMMA", "range": [2, 10, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Marker", "range": [3, 11, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#caller", "range": [4, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#alpha", "range": [5, 4, 9], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#beta", "range": [6, 4, 8], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#GAMMA", "range": [7, 11, 16], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#Marker", "range": [8, 11, 17], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#alpha",
                  "display_name": "alpha",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#beta",
                  "display_name": "beta",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#GAMMA",
                  "display_name": "GAMMA",
                  "kind": "constant",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Marker",
                  "display_name": "Marker",
                  "kind": "struct",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#caller",
                  "display_name": "caller",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: None,
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("outgoing_calls should derive precise callees from precise references")
        .0;

    assert_eq!(response.matches.len(), 2);
    assert_eq!(response.matches[0].source_symbol, "caller");
    assert_eq!(response.matches[0].target_symbol, "alpha");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(response.matches[0].call_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(response.matches[0].call_line, Some(6));
    assert_eq!(response.matches[0].call_column, Some(5));
    assert_eq!(response.matches[0].call_end_line, Some(6));
    assert_eq!(response.matches[0].call_end_column, Some(10));
    assert_eq!(response.matches[1].source_symbol, "caller");
    assert_eq!(response.matches[1].target_symbol, "beta");
    assert_eq!(response.matches[1].relation, "calls");
    assert_eq!(response.matches[1].precision.as_deref(), Some("precise"));
    assert_eq!(response.matches[1].call_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(response.matches[1].call_line, Some(7));
    assert_eq!(response.matches[1].call_column, Some(5));
    assert_eq!(response.matches[1].call_end_line, Some(7));
    assert_eq!(response.matches[1].call_end_column, Some(9));

    let note = response
        .note
        .as_ref()
        .expect("outgoing_calls should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("outgoing_calls note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("outgoing_calls should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["precise"]["outgoing_count"], 2);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_matches_typescript_callees_with_unspecified_kind() {
    let workspace_root =
        temp_workspace_root("navigation-outgoing-typescript-unspecified-callable-kind");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "export function requireServerUser() {}\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-outgoing-unspecified.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [0, 16, 33],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: None,
            symbol: Some("handler".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("outgoing_calls should keep explicit TypeScript call sites when kind data is weak")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "handler");
    assert_eq!(response.matches[0].target_symbol, "requireServerUser");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].path, "src/auth.ts");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, 17);
    assert_eq!(
        response.matches[0].call_path.as_deref(),
        Some("src/auth.ts")
    );
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_ignores_precise_callable_references_without_call_syntax() {
    let workspace_root = temp_workspace_root("navigation-outgoing-precise-non-call-reference");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary rust fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn alpha() {}\n\
         pub fn caller() {\n\
             let _ = alpha;\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "rust-outgoing-non-call-reference.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                {
                  "symbol": "scip-rust pkg repo#alpha",
                  "range": [0, 7, 12],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-rust pkg repo#caller",
                  "range": [1, 7, 13],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-rust pkg repo#alpha",
                  "range": [2, 12, 17],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#alpha",
                  "display_name": "alpha",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#caller",
                  "display_name": "caller",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: None,
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("outgoing_calls should reject non-call precise references")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert!(response.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_heuristic_fallback_keeps_empty_set_instead_of_widening_to_non_callable_refs()
 {
    let workspace_root = temp_workspace_root("navigation-outgoing-heuristic-callable-only");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn alpha() {}\n\
         pub const GAMMA: usize = 1;\n\
         pub struct Marker;\n\
         pub fn caller() {\n\
             alpha();\n\
             let _ = GAMMA;\n\
             let _ = Marker;\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");

    let server = server_for_workspace_root(&workspace_root).await;
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            target: None,
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("outgoing_calls should keep an empty heuristic result instead of widening")
        .0;

    assert!(response.matches.is_empty());

    let note = response
        .note
        .as_ref()
        .expect("outgoing_calls should emit heuristic metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("outgoing_calls note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_empty_params_returns_recovery() {
    let server = server_for_fixture().await;
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams::default()))
        .await
        .expect("empty go_to_definition should return recovery response, not transport error")
        .0;

    assert!(response.matches.is_empty());
    assert_eq!(
        response.recovery.error_code.as_deref(),
        Some("EMPTY_GO_TO_DEFINITION")
    );
    assert!(
        !response.recovery.suggested_next.is_empty(),
        "empty go_to_definition recovery must be actionable"
    );
    assert!(
        response
            .recovery
            .related_tools
            .iter()
            .any(|tool| tool == "search_symbol"),
        "related tools should include search_symbol"
    );
}

#[tokio::test]
async fn navigation_go_to_definition_empty_params_reject_zero_limit() {
    let server = server_for_fixture().await;
    let error = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            limit: Some(0),
            target: None,
            ..GoToDefinitionParams::default()
        }))
        .await
    {
        Ok(_) => panic!("zero-width navigation request must be rejected before recovery"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
}

#[tokio::test]
async fn navigation_go_to_definition_path_line_without_symbol_sets_location_warning() {
    let workspace_root = temp_workspace_root("go-to-definition-dense-line");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn alpha() {}\npub fn beta(a: i32, b: i32, c: i32) { let _ = a + b + c; }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            path: Some("src/lib.rs".to_owned()),
            target: None,
            line: Some(2),
            symbol: None,
            column: None,
            ..Default::default()
        }))
        .await
        .expect("path+line go_to_definition should succeed")
        .0;

    assert!(
        response.location_warning.is_some(),
        "path+line without symbol should surface location_warning: {response:?}"
    );
    assert_eq!(
        response.ambiguous_location,
        Some(true),
        "path+line without symbol must set ambiguous_location for agent branching: {response:?}"
    );
    if !response.matches.is_empty() {
        assert!(
            response.recovery.correction_hint.is_some(),
            "non-empty path+line defs should include correction_hint: {response:?}"
        );
        assert!(
            !response.recovery.suggested_next.is_empty(),
            "non-empty path+line defs should include suggested_next replan: {response:?}"
        );
    }
    // Do not invent DisambiguationRequired solely for density; leave real multi-candidate shape alone.
    if let Some(selection) = response.target_selection.as_ref() {
        if selection.status == NavigationTargetSelectionStatus::DisambiguationRequired {
            assert!(
                selection.selected_stable_symbol_id.is_none() || !selection.candidates.is_empty(),
                "true disambiguation should clear selection or list candidates: {selection:?}"
            );
        }
    }
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn impact_bundle_composes_symbol_refs_and_callers() {
    let workspace_root = temp_workspace_root("impact-bundle-compose");
    let src_root = workspace_root.join("src");
    let tests_root = workspace_root.join("tests");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::create_dir_all(&tests_root).expect("failed to create temporary test fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn target() {}\npub fn caller() { target(); }\n",
    )
    .expect("failed to seed temporary fixture source");
    fs::write(
        tests_root.join("target_mentions.rs"),
        "use crate::target;\nfn checks_target() { target(); }\n",
    )
    .expect("failed to seed exact test mention fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "target".to_owned(),
            path_class: None,
            repository_id: Some("repo-001".to_owned()),
            include_implementations: None,
            include_test_mentions: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("impact_bundle should compose")
        .0;

    assert_eq!(response.symbol, "target");
    assert_eq!(response.path_class, "runtime");
    assert!(
        !response.symbols.is_empty(),
        "impact_bundle should include symbol hits: {response:?}"
    );
    assert_eq!(
        response.summary.symbols_count,
        response.symbols.len(),
        "summary counts must match list lengths"
    );
    assert_eq!(response.summary.references_count, response.references.len());
    assert_eq!(
        response.summary.incoming_calls_count,
        response.incoming_calls.len()
    );
    assert_eq!(
        response.symbols_completeness.returned,
        response.symbols.len()
    );
    assert_eq!(
        response.references_completeness.returned,
        response.references.len()
    );
    assert_eq!(
        response.incoming_calls_completeness.returned,
        response.incoming_calls.len()
    );
    assert!(
        !response.completeness.complete
            || (response.symbols_completeness.complete
                && response.references_completeness.complete
                && response.incoming_calls_completeness.complete),
        "impact aggregate cannot upgrade a child section: {response:?}"
    );
    assert!(response.implementations_completeness.is_none());
    assert!(!response.implementations_included);
    assert_eq!(response.summary.references_mode, response.references_mode);
    assert_eq!(
        response.summary.incoming_calls_mode,
        response.incoming_calls_mode
    );
    assert_eq!(
        response.sections.len(),
        5,
        "every closed impact section is present"
    );
    for section in &response.sections {
        match section.section {
            ImpactSection::Implementation | ImpactSection::TestMention => {
                assert_eq!(section.execution, ImpactSectionExecution::OmittedByPolicy);
                assert!(section.trust.is_none());
                assert!(section.completeness.is_none());
                assert!(section.result_handle.is_none());
                assert!(section.proof_targets.is_empty());
            }
            ImpactSection::Symbol | ImpactSection::Reference | ImpactSection::IncomingCall => {
                assert_eq!(section.execution, ImpactSectionExecution::Included);
                assert!(section.trust.is_some());
                assert!(section.completeness.is_some());
            }
        }
    }
    assert!(
        !response.summary.top_paths.is_empty(),
        "non-empty impact should populate top_paths: {:?}",
        response.summary.top_paths
    );
    assert!(
        response
            .summary
            .top_paths
            .iter()
            .any(|p| p.path.contains("lib.rs")),
        "fixture path should appear in top_paths: {:?}",
        response.summary.top_paths
    );

    // composition: symbols + (refs or mode/recovery note) + (callers or provisional note).
    let refs_ok = !response.references.is_empty()
        || matches!(
            response.references_mode,
            NavigationMode::HeuristicNoPrecise
                | NavigationMode::UnavailableNoPrecise
                | NavigationMode::PrecisePartial
        )
        || response.recovery.correction_hint.is_some()
        || response
            .recovery
            .suggested_next
            .iter()
            .any(|step| step.tool == "find_references" || step.tool == "search_text");
    assert!(
        refs_ok,
        "impact_bundle must surface references or explain graph/heuristic mode: refs={}, mode={:?}, recovery={:?}, suggested_next={:?}",
        response.references.len(),
        response.references_mode,
        response.recovery,
        response.recovery.suggested_next
    );

    let callers_ok = !response.incoming_calls.is_empty()
        || matches!(
            response.incoming_calls_mode,
            NavigationMode::HeuristicNoPrecise
                | NavigationMode::UnavailableNoPrecise
                | NavigationMode::PrecisePartial
        )
        || response
            .recovery
            .suggested_next
            .iter()
            .any(|step| step.tool == "incoming_calls" || step.tool == "read_file");
    assert!(
        callers_ok,
        "impact_bundle must surface incoming_calls or provisional/mode guidance: callers={}, mode={:?}, suggested_next={:?}",
        response.incoming_calls.len(),
        response.incoming_calls_mode,
        response.recovery.suggested_next
    );

    // Prefer real refs/callers when the fixture supports SCIP/heuristic resolution.
    if !response.references.is_empty() {
        assert!(
            response
                .references
                .iter()
                .any(|r| r.path.contains("lib.rs")),
            "fixture references should point at lib.rs: {:?}",
            response.references
        );
    }
    if !response.incoming_calls.is_empty() {
        assert!(
            response.incoming_calls.iter().any(|c| {
                c.path.contains("lib.rs")
                    || c.source_symbol.contains("caller")
                    || c.target_symbol.contains("caller")
            }),
            "fixture callers should include caller()/lib.rs: {:?}",
            response.incoming_calls
        );
    }

    assert!(
        !response.recovery.suggested_next.is_empty(),
        "impact_bundle should expose canonical implementation/proof follow-ups"
    );
    assert!(
        response
            .recovery
            .next_actions
            .iter()
            .all(|action| !matches!(action.target, NextActionTarget::SearchText(_))),
        "default impact requests must not search or claim test evidence"
    );
    assert!(
        response
            .recovery
            .suggested_next
            .iter()
            .any(|step| step.tool == "read_match"
                || step.tool == "search_text"
                || step.tool == "read_file"),
        "suggested_next should include proof/tests guidance: {:?}",
        response.recovery.suggested_next
    );
    assert_eq!(
        response.recovery.suggested_next,
        response
            .recovery
            .next_actions
            .iter()
            .map(|action| action.to_legacy_suggestion())
            .collect::<Vec<_>>(),
        "impact legacy suggestions are generated only from canonical actions"
    );
    let proof = response
        .recovery
        .next_actions
        .iter()
        .find_map(|action| match &action.target {
            NextActionTarget::ReadMatch(params) => Some(params.clone()),
            _ => None,
        })
        .expect("impact success must offer a replayable proof-read action");
    assert!(
        [
            response.symbols_result_handle.as_ref(),
            response.references_result_handle.as_ref(),
            response.incoming_calls_result_handle.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|handle| handle == &proof.result_handle),
        "impact proof action must bind one concrete section handle"
    );
    assert_eq!(
        serde_json::to_value(proof.origin.clone()).expect("impact origin serializes"),
        serde_json::to_value(Some(NextActionOrigin(ReplayOriginTarget::ImpactBundle(
            ImpactBundleParams {
                target: None,
                symbol: "target".to_owned(),
                path_class: None,
                repository_id: Some("repo-001".to_owned()),
                include_implementations: None,
                include_test_mentions: None,
                response_mode: Some(ResponseMode::Compact),
            },
        ))))
        .expect("expected impact origin serializes"),
        "impact proof action must preserve the exact producer request"
    );
    server
        .read_match(Parameters(proof))
        .await
        .expect("impact proof action must replay through read_match");
    for proof_target in &response.proof_targets {
        let section = response
            .sections
            .iter()
            .find(|section| section.section == proof_target.section)
            .expect("every bundle proof has its section envelope");
        assert!(section.proof_targets.contains(proof_target));
        let action = response
            .recovery
            .next_actions
            .iter()
            .find(|action| action.id == proof_target.action_id)
            .expect("every proof target has one canonical action");
        let NextActionTarget::ReadMatch(params) = &action.target else {
            panic!("impact proof actions must use read_match");
        };
        assert_eq!(params.result_handle, proof_target.target.result_handle);
        assert_eq!(params.match_id, proof_target.target.match_id);
        server
            .read_match(Parameters(params.clone()))
            .await
            .expect("every section-qualified proof action must replay");
    }

    let opted_in = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "target".to_owned(),
            path_class: None,
            repository_id: Some("repo-001".to_owned()),
            include_implementations: None,
            include_test_mentions: Some(true),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("opted-in impact should search exact test mentions")
        .0;
    let test_section = opted_in
        .sections
        .iter()
        .find(|section| section.section == ImpactSection::TestMention)
        .expect("opted-in impact must retain the test section");
    let test_rows = match &test_section.rows {
        ImpactSectionRows::TestMention(rows) => rows,
        _ => panic!("test section must contain only text rows"),
    };
    assert_eq!(test_section.execution, ImpactSectionExecution::Included);
    assert_eq!(
        test_section.trust,
        Some(ImpactSectionTrust::ExactLiteralText)
    );
    assert_eq!(
        test_section
            .completeness
            .as_ref()
            .map(|completeness| completeness.returned),
        Some(test_rows.len()),
        "test section completeness must describe the returned exact rows"
    );
    assert!(
        !test_rows.is_empty() && test_section.proof_targets.len() == test_rows.len(),
        "each opted-in test row must carry one section-qualified proof"
    );
    assert!(test_section.proof_targets.iter().all(|proof| {
        opted_in.proof_targets.contains(proof)
            && opted_in
                .recovery
                .next_actions
                .iter()
                .any(|action| action.id == proof.action_id)
    }));

    let full = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "target".to_owned(),
            path_class: None,
            repository_id: Some("repo-001".to_owned()),
            include_implementations: None,
            include_test_mentions: Some(true),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("full impact should preserve the same contract")
        .0;
    assert_eq!(opted_in.target_selection, full.target_selection);
    let section_shape = |section: &ImpactSectionResult| {
        let row_count = match &section.rows {
            ImpactSectionRows::Symbol(rows) => rows.len(),
            ImpactSectionRows::Reference(rows) => rows.len(),
            ImpactSectionRows::IncomingCall(rows) => rows.len(),
            ImpactSectionRows::Implementation(rows) => rows.len(),
            ImpactSectionRows::TestMention(rows) => rows.len(),
        };
        (
            section.section,
            section.execution,
            section.trust.clone(),
            section.completeness.clone(),
            row_count,
            section.proof_targets.len(),
        )
    };
    assert_eq!(
        opted_in
            .sections
            .iter()
            .map(section_shape)
            .collect::<Vec<_>>(),
        full.sections.iter().map(section_shape).collect::<Vec<_>>(),
        "response detail must preserve section execution, trust, completeness, rows, and proofs"
    );
    assert_eq!(
        opted_in
            .proof_targets
            .iter()
            .map(|proof| proof.section)
            .collect::<Vec<_>>(),
        full.proof_targets
            .iter()
            .map(|proof| proof.section)
            .collect::<Vec<_>>(),
        "response detail must preserve each proof-bearing section"
    );
    assert_eq!(opted_in.completeness, full.completeness);
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn impact_bundle_stale_target_fails_closed_before_child_execution() {
    let workspace_root = temp_workspace_root("impact-bundle-stale-target");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create stale-target fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn target() {}\npub fn caller() { target(); }\n",
    )
    .expect("failed to seed stale-target fixture source");
    let server = server_for_workspace_root(&workspace_root).await;
    let repository_id = public_repository_id(&server).await;
    let symbol = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: Some(SearchSymbolPathClass::Runtime),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("fixture symbol should be indexed")
        .0;
    let stable_symbol_id = symbol.matches[0]
        .stable_symbol_id
        .clone()
        .expect("fixture symbol should expose a stable identity");

    let stale = match server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: Some(TargetRef::StableSymbol {
                repository_id,
                stable_symbol_id,
                snapshot_token: "stale-snapshot-token".to_owned(),
            }),
            symbol: String::new(),
            path_class: None,
            repository_id: None,
            include_implementations: None,
            include_test_mentions: Some(true),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a stale impact target must fail before running children"),
    };
    assert_target_error_contract(&stale, "STALE_TARGET_SNAPSHOT", &workspace_root);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn impact_bundle_opted_in_test_mentions_keeps_an_honest_zero_section() {
    let workspace_root = temp_workspace_root("impact-bundle-test-mentions-zero");
    let src_root = workspace_root.join("src");
    let tests_root = workspace_root.join("tests");
    fs::create_dir_all(&src_root).expect("failed to create source fixture");
    fs::create_dir_all(&tests_root).expect("failed to create test fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn target() {}\npub fn caller() { target(); }\n",
    )
    .expect("failed to seed source fixture");
    fs::write(tests_root.join("unrelated.rs"), "fn unrelated() {}\n")
        .expect("failed to seed unrelated test fixture");
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "target".to_owned(),
            path_class: None,
            repository_id: Some("repo-001".to_owned()),
            include_implementations: None,
            include_test_mentions: Some(true),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("opted-in impact should retain an exact test zero")
        .0;
    let test_section = response
        .sections
        .iter()
        .find(|section| section.section == ImpactSection::TestMention)
        .expect("opted-in impact must retain the test section");
    assert_eq!(test_section.execution, ImpactSectionExecution::Included);
    assert_eq!(
        test_section.trust,
        Some(ImpactSectionTrust::ExactLiteralText)
    );
    assert!(matches!(&test_section.rows, ImpactSectionRows::TestMention(rows) if rows.is_empty()));
    assert_eq!(
        test_section
            .completeness
            .as_ref()
            .map(|completeness| completeness.returned),
        Some(0),
        "the opted-in exact search must report its actual zero"
    );
    assert!(test_section.result_handle.is_none());
    assert!(test_section.proof_targets.is_empty());
    assert!(
        response
            .proof_targets
            .iter()
            .all(|proof| proof.section != ImpactSection::TestMention)
    );
    assert!(
        response
            .recovery
            .next_actions
            .iter()
            .all(|action| !action.id.0.contains("test-mention")),
        "an exact zero must not fabricate a test-mention proof action"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn impact_bundle_same_rank_legacy_symbol_requires_disambiguation() {
    let workspace_root = temp_workspace_root("impact-bundle-anchor-selected-symbol");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn target() {}\npub fn caller() { target(); }\n",
    )
    .expect("failed to seed primary fixture source");
    fs::write(
        src_root.join("other.rs"),
        "pub fn target() {}\npub fn other_caller() { target(); }\n",
    )
    .expect("failed to seed duplicate fixture source");
    write_scip_fixture(
        &workspace_root,
        "impact_bundle_duplicate_target.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#src/lib.rs::target", "range": [0, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#src/lib.rs::target", "range": [1, 18, 24], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#src/lib.rs::target",
                  "display_name": "target",
                  "kind": "function",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "src/other.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#src/other.rs::target", "range": [0, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#src/other.rs::target", "range": [1, 24, 30], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#src/other.rs::target",
                  "display_name": "target",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "target".to_owned(),
            path_class: Some(SearchSymbolPathClass::Runtime),
            repository_id: Some("repo-001".to_owned()),
            include_implementations: None,
            include_test_mentions: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("impact_bundle should surface duplicate-name target selection")
        .0;

    let selection = response
        .target_selection
        .expect("same-rank legacy symbol must surface target selection");
    assert!(
        matches!(
            selection.status,
            frigg::mcp::types::NavigationTargetSelectionStatus::DisambiguationRequired
        ),
        "same-rank legacy symbol must require disambiguation: {selection:?}"
    );
    assert_eq!(selection.candidate_count, 2);
    assert_eq!(selection.same_rank_candidate_count, 2);
    assert!(selection.selected_stable_symbol_id.is_none());
    assert_eq!(
        response.symbols.len(),
        2,
        "symbol alternatives remain visible"
    );
    assert!(
        response.references.is_empty(),
        "must not choose a target for references"
    );
    assert!(
        response.incoming_calls.is_empty(),
        "must not choose a target for incoming calls"
    );
    assert!(response.proof_targets.is_empty());
    assert!(response.recovery.next_actions.is_empty());
    assert!(response.sections.iter().all(|section| {
        section.execution == ImpactSectionExecution::NotRunTargetUnresolved
            && section.trust.is_none()
            && section.completeness.is_none()
            && section.result_handle.is_none()
            && section.proof_targets.is_empty()
            && match &section.rows {
                ImpactSectionRows::Symbol(rows) => rows.is_empty(),
                ImpactSectionRows::Reference(rows) => rows.is_empty(),
                ImpactSectionRows::IncomingCall(rows) => rows.is_empty(),
                ImpactSectionRows::Implementation(rows) => rows.is_empty(),
                ImpactSectionRows::TestMention(rows) => rows.is_empty(),
            }
    }));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn impact_bundle_legacy_path_class_constrains_target_selection() {
    let workspace_root = temp_workspace_root("impact-bundle-path-class-selection");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create runtime fixture");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create support fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn target() {}\npub fn runtime_caller() { target(); }\n",
    )
    .expect("failed to seed runtime target");
    fs::write(
        workspace_root.join("tests/support.rs"),
        "pub fn target() {}\npub fn support_caller() { target(); }\n",
    )
    .expect("failed to seed support target");
    write_scip_fixture(
        &workspace_root,
        "impact_bundle_path_class.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#runtime_target", "range": [0, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#runtime_caller", "range": [1, 7, 21], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#runtime_target", "range": [1, 26, 32], "symbol_roles": 8 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg repo#runtime_target", "display_name": "target", "kind": "function", "relationships": [] },
                { "symbol": "scip-rust pkg repo#runtime_caller", "display_name": "runtime_caller", "kind": "function", "relationships": [] }
              ]
            },
            {
              "relative_path": "tests/support.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#support_target", "range": [0, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#support_caller", "range": [1, 7, 21], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#support_target", "range": [1, 26, 32], "symbol_roles": 8 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg repo#support_target", "display_name": "target", "kind": "function", "relationships": [] },
                { "symbol": "scip-rust pkg repo#support_caller", "display_name": "support_caller", "kind": "function", "relationships": [] }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let response = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "target".to_owned(),
            path_class: Some(SearchSymbolPathClass::Support),
            repository_id: Some("repo-001".to_owned()),
            include_implementations: Some(false),
            include_test_mentions: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("legacy support-only impact selection should resolve exactly")
        .0;

    assert_eq!(response.symbols.len(), 1);
    assert_eq!(response.symbols[0].path, "tests/support.rs");
    let selection = response
        .target_selection
        .as_ref()
        .expect("legacy impact should expose its selected target");
    assert_eq!(selection.status, NavigationTargetSelectionStatus::Resolved);
    assert_eq!(
        selection.resolution_source,
        NavigationResolutionSource::DirectSymbol
    );
    assert_eq!(selection.candidate_count, 1);
    assert_eq!(selection.same_rank_candidate_count, 1);
    assert_eq!(
        selection.selected_stable_symbol_id,
        response.symbols[0].stable_symbol_id
    );
    assert!(
        !response.references.is_empty()
            && response
                .references
                .iter()
                .all(|matched| matched.path == "tests/support.rs"),
        "support path_class must constrain reference composition"
    );
    assert!(
        !response.incoming_calls.is_empty()
            && response
                .incoming_calls
                .iter()
                .all(|matched| matched.path == "tests/support.rs"),
        "support path_class must constrain incoming-call composition"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn impact_bundle_missing_symbol_returns_recovery() {
    let server = server_for_fixture().await;
    let response = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: String::new(),
            ..Default::default()
        }))
        .await
        .expect("empty symbol impact_bundle should return recovery")
        .0;

    assert!(response.symbols.is_empty());
    assert_eq!(
        response.recovery.error_code.as_deref(),
        Some("MISSING_SYMBOL")
    );
    assert!(!response.recovery.suggested_next.is_empty());
}

#[tokio::test]
async fn impact_bundle_unknown_symbol_returns_recovery() {
    let server = server_for_fixture().await;
    let response = server
        .impact_bundle(Parameters(ImpactBundleParams {
            target: None,
            symbol: "DefinitelyMissingSymbolZzz".to_owned(),
            path_class: None,
            repository_id: Some("repo-001".to_owned()),
            include_implementations: None,
            include_test_mentions: None,
            response_mode: None,
        }))
        .await
        .expect("unknown symbol impact_bundle should return recovery")
        .0;

    assert!(response.symbols.is_empty());
    assert!(
        !response.recovery.is_empty(),
        "unknown symbol must surface recovery/suggested_next: {response:?}"
    );
}

#[test]
fn impact_bundle_section_types_are_closed_and_proof_targets_are_exact() {
    let params: ImpactBundleParams = serde_json::from_value(serde_json::json!({
        "symbol": "target"
    }))
    .expect("legacy impact request should retain the opt-in default");
    assert!(!params.includes_test_mentions());

    let schema = serde_json::to_value(schemars::schema_for!(ImpactBundleParams))
        .expect("impact params schema should serialize");
    assert_eq!(
        schema["properties"]["include_test_mentions"]["default"],
        serde_json::json!(false)
    );

    let proof = ImpactProofTarget::new(
        ImpactSection::Reference,
        ImpactProofRowTarget::new(
            "impact-handle".into(),
            "reference:m1".into(),
            "scope".into(),
        )
        .expect("non-empty bound row target"),
        NextActionId::new("impact-reference-proof").expect("non-empty canonical action id"),
    )
    .expect("proof target must retain section, exact row, and action id");
    let proof_json = serde_json::to_value(&proof).expect("proof target should serialize");
    assert_eq!(proof_json["section"], "reference");
    assert_eq!(proof_json["target"]["result_handle"], "impact-handle");
    assert_eq!(proof_json["target"]["match_id"], "reference:m1");
    assert_eq!(proof_json["action_id"], "impact-reference-proof");
    assert!(
        serde_json::from_value::<ImpactProofTarget>(serde_json::json!({
            "section": "outgoing_call",
            "target": {"result_handle": "h", "match_id": "m", "target_scope": "s"},
            "action_id": "proof"
        }))
        .is_err()
    );
    assert!(serde_json::from_value::<ImpactProofTarget>(serde_json::json!({
        "section": "reference",
        "target": {"repository_id": "repo", "stable_symbol_id": "symbol", "snapshot_token": "snapshot"},
        "action_id": "proof"
    }))
    .is_err());
    assert!(
        serde_json::from_value::<ImpactSectionTrust>(serde_json::json!({
            "kind": "navigation",
            "mode": "precise",
            "extra": true
        }))
        .is_err()
    );
    assert!(matches!(
        serde_json::from_value::<ImpactSectionTrust>(serde_json::json!({
            "kind": "navigation",
            "mode": "precise_partial"
        }))
        .expect("known trust variant should deserialize"),
        ImpactSectionTrust::Navigation {
            mode: NavigationMode::PrecisePartial
        }
    ));
}

#[test]
fn impact_section_result_owns_rows_completeness_and_execution_truth() {
    let included = ImpactSectionResult::new(
        ImpactSection::TestMention,
        ImpactSectionExecution::Included,
        Some(ImpactSectionTrust::ExactLiteralText),
        Some(
            ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
                .expect("empty included test section is complete"),
        ),
        Some("impact-test-mentions-handle".into()),
        ImpactSectionRows::TestMention(Vec::new()),
        Vec::new(),
    )
    .expect("included empty section retains its own coverage truth");
    let included_json = serde_json::to_value(&included).expect("section envelope serializes");
    assert_eq!(included_json["execution"], "included");
    assert_eq!(included_json["rows"]["row_kind"], "test_mention");
    assert_eq!(included_json["completeness"]["total"], 0);
    assert_eq!(
        included_json["result_handle"],
        "impact-test-mentions-handle"
    );

    assert!(
        ImpactSectionResult::new(
            ImpactSection::TestMention,
            ImpactSectionExecution::Included,
            Some(ImpactSectionTrust::ExactLiteralText),
            Some(ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0).unwrap()),
            None,
            ImpactSectionRows::TestMention(Vec::new()),
            Vec::new(),
        )
        .is_some(),
        "an exact zero section has no proofable row and therefore no result handle"
    );

    assert!(
        ImpactSectionResult::new(
            ImpactSection::Implementation,
            ImpactSectionExecution::OmittedByPolicy,
            None,
            Some(ResultCompleteness::complete(ResultUnit::Implementation, 0, 0).unwrap()),
            None,
            ImpactSectionRows::Implementation(Vec::new()),
            Vec::new(),
        )
        .is_none()
    );
    assert!(
        ImpactSectionResult::new(
            ImpactSection::Reference,
            ImpactSectionExecution::NotRunTargetUnresolved,
            None,
            None,
            None,
            ImpactSectionRows::Symbol(Vec::new()),
            Vec::new(),
        )
        .is_none()
    );
}
