use super::*;

#[tokio::test]
async fn core_find_references_returns_heuristic_metadata_and_matches() {
    let workspace_root = temp_workspace_root("find-references");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should return heuristic references")
        .0;

    assert!(
        response.matches.len() >= 2,
        "expected at least two deterministic heuristic references"
    );
    assert_eq!(response.total_matches, response.matches.len());
    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 25);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit heuristic metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("find_references should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["heuristic"], true);
    assert_eq!(note_json["confidence"]["low"], response.matches.len());
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["ambiguous_query"], false);
    assert_eq!(note_json["target_selection"]["candidate_count"], 1);
    assert!(
        note_json["resource_budgets"]["source"]["max_file_bytes"]
            .as_u64()
            .is_some()
    );
    assert!(
        note_json["resource_usage"]["source"]["files_discovered"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_includes_definition_when_requested_by_default() {
    let workspace_root = temp_workspace_root("find-references-include-definition");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should return heuristic references with a definition row")
        .0;

    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(
        response
            .matches
            .first()
            .expect("definition row should be present")
            .match_kind,
        ReferenceMatchKind::Definition
    );
    assert!(
        response
            .matches
            .iter()
            .any(|entry| entry.match_kind == ReferenceMatchKind::Reference)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_opt_in_returns_follow_up_structural() {
    let workspace_root = temp_workspace_root("find-references-follow-up-structural");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn greeting() -> &'static str { \"hello\" }\n\
         pub fn caller() { let _ = greeting(); }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: Some(true),
            limit: Some(20),
        }))
        .await
        .expect("find_references should return follow-up structural suggestions when opted in")
        .0;

    let reference_match = response
        .matches
        .first()
        .expect("fixture should return a reference match");
    assert!(!reference_match.follow_up_structural.is_empty());
    assert_eq!(
        reference_match.follow_up_structural[0].params.repository_id,
        "repo-001"
    );
    assert_eq!(
        reference_match.follow_up_structural[0]
            .params
            .path_regex
            .as_deref(),
        Some("^src/lib\\.rs$")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn precision_precedence_find_references_prefers_precise_matches() {
    let workspace_root = temp_workspace_root("precision-precedence-precise");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn heuristic_marker() { let _ = User; }\n\
         pub fn precise_marker() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "references.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [2, 31, 35], "symbol_roles": 8 }
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise references first")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 32);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["reference_count"], 1);
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert!(
        note_json["precise"]["candidate_directories"]
            .as_array()
            .is_some_and(|directories| !directories.is_empty())
    );
    assert!(
        note_json["precise"]["discovered_artifacts"]
            .as_array()
            .is_some_and(|artifacts| !artifacts.is_empty())
    );
    assert!(
        note_json["resource_usage"]["scip"]["artifacts_discovered_bytes"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn precision_precedence_find_references_prefers_protobuf_scip_matches() {
    let workspace_root = temp_workspace_root("precision-precedence-precise-protobuf");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn heuristic_marker() { let _ = User; }\n\
         pub fn precise_marker() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_protobuf_fixture(&workspace_root, "references.scip");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise references from protobuf scip")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 32);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["reference_count"], 1);
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert!(
        note_json["precise"]["discovered_artifacts"][0]
            .as_str()
            .is_some_and(|path| path.ends_with(".frigg/scip/references.scip"))
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_falls_back_to_direct_precise_symbol_when_corpus_symbol_is_missing() {
    let workspace_root = temp_workspace_root("find-references-direct-precise-fallback");
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
        "translations.json",
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("Settings".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to direct precise symbols")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "Settings");
    assert_eq!(
        response.matches[0].path,
        "resources/views/welcome.blade.php"
    );
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, 7);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["resolution_source"], "symbol_precise_direct");
    assert_eq!(note_json["target_precise_symbol"], "trans/`json:Settings`.");
    assert_eq!(note_json["precise"]["reference_count"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_falls_back_to_direct_precise_config_symbol_when_corpus_symbol_is_missing()
{
    let workspace_root = temp_workspace_root("find-references-direct-precise-config");
    let config_root = workspace_root.join("config");
    let controller_root = workspace_root.join("app/Http/Controllers");
    fs::create_dir_all(&config_root).expect("failed to create config fixture root");
    fs::create_dir_all(&controller_root).expect("failed to create controller fixture root");
    let config_source =
        "return [\n    'registration_enabled' => env('REGISTRATION_ENABLED', true),\n];\n";
    let controller_source = "if (config('features.registration_enabled')) { return true; }\n";
    fs::write(config_root.join("features.php"), config_source)
        .expect("failed to seed config fixture");
    fs::write(
        controller_root.join("RegisterController.php"),
        controller_source,
    )
    .expect("failed to seed controller fixture");
    let config_definition_column = config_source
        .find("registration_enabled")
        .expect("config key should exist in definition");
    let controller_reference_column = controller_source
        .find("features.registration_enabled")
        .expect("config lookup should exist in reference");
    write_scip_fixture(
        &workspace_root,
        "config_references.json",
        &format!(
            r#"{{
          "documents": [
            {{
              "relative_path": "config/features.php",
              "occurrences": [
                {{ "symbol": "config/`key:features.registration_enabled`.", "range": [1, {config_definition_column}, {config_definition_column_end}], "symbol_roles": 1 }}
              ],
              "symbols": [
                {{
                  "symbol": "config/`key:features.registration_enabled`.",
                  "display_name": "features.registration_enabled",
                  "kind": "property",
                  "relationships": []
                }}
              ]
            }},
            {{
              "relative_path": "app/Http/Controllers/RegisterController.php",
              "occurrences": [
                {{ "symbol": "config/`key:features.registration_enabled`.", "range": [0, {controller_reference_column}, {controller_reference_column_end}], "symbol_roles": 8 }}
              ],
              "symbols": []
            }}
          ]
        }}"#,
            config_definition_column = config_definition_column,
            config_definition_column_end = config_definition_column + "registration_enabled".len(),
            controller_reference_column = controller_reference_column,
            controller_reference_column_end =
                controller_reference_column + "features.registration_enabled".len(),
        ),
    );
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("features.registration_enabled".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to direct precise config symbols")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "features.registration_enabled");
    assert_eq!(
        response.matches[0].path,
        "app/Http/Controllers/RegisterController.php"
    );
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, controller_reference_column + 1);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["resolution_source"], "symbol_precise_direct");
    assert_eq!(
        note_json["target_precise_symbol"],
        "config/`key:features.registration_enabled`."
    );
    assert_eq!(note_json["precise"]["reference_count"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn precision_precedence_find_references_falls_back_to_heuristic_when_precise_absent() {
    let workspace_root = temp_workspace_root("precision-precedence-heuristic");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to heuristic references")
        .0;

    assert!(
        response.matches.len() >= 2,
        "expected deterministic heuristic fallback references"
    );
    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["heuristic"], true);
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise_absence_reason"],
        "no_scip_artifacts_discovered"
    );
    assert_eq!(note_json["precise"]["artifacts_discovered"], 0);
    assert_eq!(note_json["precise"]["artifacts_failed"], 0);
    assert_eq!(note_json["precise"]["reference_count"], 0);
    assert!(
        note_json["precise"]["candidate_directories"]
            .as_array()
            .is_some_and(|directories| directories.iter().any(|path| {
                path.as_str()
                    .is_some_and(|path| path.ends_with(".frigg/scip"))
            }))
    );
    assert_eq!(
        note_json["precise"]["discovered_artifacts"]
            .as_array()
            .map(|artifacts| artifacts.len()),
        Some(0)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_reports_failed_scip_artifact_details_in_note_metadata() {
    let workspace_root = temp_workspace_root("find-references-failed-artifact-details");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(&workspace_root, "broken.json", "{ invalid json");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to heuristic references")
        .0;

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");

    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise_absence_reason"],
        "scip_artifact_ingest_failed"
    );
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "ingest_payload"
    );
    assert!(
        note_json["precise"]["failed_artifacts"][0]["artifact_label"]
            .as_str()
            .unwrap_or_default()
            .ends_with(".frigg/scip/broken.json")
    );
    assert!(
        !note_json["precise"]["failed_artifacts"][0]["detail"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "expected parse failure detail in failed artifact metadata"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_reports_target_selection_metadata_for_ambiguous_symbol_queries() {
    let workspace_root = temp_workspace_root("find-references-ambiguous-symbol");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(src_root.join("a.rs"), "pub fn invalid_params() {}\n")
        .expect("failed to seed first source file");
    fs::write(src_root.join("b.rs"), "pub fn invalid_params() {}\n")
        .expect("failed to seed second source file");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("invalid_params".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should succeed with ambiguous symbol names")
        .0;

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit selection metadata in note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["query"], "invalid_params");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/a.rs");
    assert_eq!(note_json["target_selection"]["selected_line"], 1);
    assert_eq!(note_json["target_selection"]["ambiguous_query"], true);
    assert_eq!(note_json["target_selection"]["candidate_count"], 2);
    assert_eq!(
        note_json["target_selection"]["same_rank_candidate_count"],
        2
    );
    assert_eq!(
        note_json["precise_absence_reason"],
        "no_scip_artifacts_discovered"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_precise_results_stay_pinned_to_runtime_target_selection() {
    let workspace_root = temp_workspace_root("find-references-precise-target-pinning");
    let src_root = workspace_root.join("src");
    let benches_root = workspace_root.join("benches");
    fs::create_dir_all(&src_root).expect("failed to create runtime fixture");
    fs::create_dir_all(&benches_root).expect("failed to create bench fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn try_execute() {}\n\
         pub fn runtime_caller() { try_execute(); }\n",
    )
    .expect("failed to seed runtime source file");
    fs::write(
        benches_root.join("runtime_bottlenecks.rs"),
        "pub fn try_execute() {}\n\
         pub fn bench_caller() { try_execute(); }\n",
    )
    .expect("failed to seed bench source file");
    write_scip_fixture(
        &workspace_root,
        "target-pinning.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "range": [0, 7, 18], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#runtime_caller", "range": [1, 7, 21], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "range": [1, 26, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#z_runtime_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#runtime_caller",
                  "display_name": "runtime_caller",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "is_reference": true }
                  ]
                }
              ]
            },
            {
              "relative_path": "benches/runtime_bottlenecks.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#a_bench_try_execute", "range": [0, 7, 18], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#bench_caller", "range": [1, 7, 19], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#a_bench_try_execute", "range": [1, 24, 35], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#a_bench_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#bench_caller",
                  "display_name": "bench_caller",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#a_bench_try_execute", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("try_execute".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should pin precise results to the selected runtime target")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/lib.rs");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );
    assert_eq!(note_json["precise"]["reference_count"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_matches_precise_typescript_symbols_without_display_names() {
    let workspace_root = temp_workspace_root("find-references-typescript-symbol-tail");
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
        "typescript-tail.json",
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise TypeScript references")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "requireServerUser");
    assert_eq!(response.matches[0].path, "src/auth.ts");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 5);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_precise_symbol"],
        "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser."
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_retains_precise_matches_when_other_scip_artifact_exceeds_budget() {
    let workspace_root = temp_workspace_root("find-references-scip-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n",
    )
    .expect("failed to seed source fixture");
    write_scip_fixture(
        &workspace_root,
        "references.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 27, 31], "symbol_roles": 8 }
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

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("oversized SCIP artifact should retain partial precise references")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    let note = response
        .note
        .as_ref()
        .expect("find_references should emit partial precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise_partial");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "artifact_budget_bytes"
    );
    assert!(
        note_json["precise"]["failed_artifacts"][0]["artifact_label"]
            .as_str()
            .unwrap_or_default()
            .ends_with(".frigg/scip/oversized.json")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_falls_back_when_partial_precise_absence_is_non_authoritative() {
    let workspace_root = temp_workspace_root("find-references-partial-absence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n",
    )
    .expect("failed to seed source fixture");
    write_scip_fixture(&workspace_root, "empty.json", r#"{ "documents": [] }"#);

    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("partial precise absence should fall back heuristically")
        .0;

    assert!(
        !response.matches.is_empty(),
        "heuristic fallback should still return lexical references"
    );
    let note = response
        .note
        .as_ref()
        .expect("find_references should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise_absence_reason"],
        "precise_partial_non_authoritative_absence"
    );
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_rejects_oversized_source_file_with_typed_timeout() {
    let workspace_root = temp_workspace_root("find-references-source-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    fs::write(src_root.join("zzz_large.rs"), "x".repeat(256))
        .expect("failed to seed oversized source file");

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 8);
    let error = match server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("oversized source file should return typed timeout"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(error_code_tag(&error), Some("timeout"));
    assert_eq!(retryable_tag(&error), Some(true));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("budget_scope"))
            .and_then(|value| value.as_str()),
        Some("source")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("budget_code"))
            .and_then(|value| value.as_str()),
        Some("source_file_bytes")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_prefers_location_resolution_when_symbol_and_location_are_both_supplied() {
    let workspace_root = temp_workspace_root("find-references-location-precedence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.php"),
        "<?php\nfunction alpha() {}\nfunction beta() {}\nalpha();\nbeta();\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(3),
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should prefer location resolution")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "beta");
    assert_eq!(response.matches[0].path, "src/lib.php");
    assert_eq!(response.matches[0].line, 5);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_enclosing_symbol");
    assert_eq!(note_json["target_selection"]["selected_symbol"], "beta");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_resolves_location_only_requests() {
    let workspace_root = temp_workspace_root("find-references-location-only");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.php"),
        "<?php\nfunction alpha() {}\nfunction beta() {}\nalpha();\nbeta();\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(3),
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve location-only requests")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "beta");
    assert_eq!(response.matches[0].path, "src/lib.php");
    assert_eq!(response.matches[0].line, 5);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_enclosing_symbol");
    assert_eq!(note_json["target_selection"]["selected_symbol"], "beta");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_rejects_requests_without_symbol_or_location() {
    let server = server_for_fixture();
    let error = match server
        .find_references(Parameters(FindReferencesParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("find_references should reject requests without a symbol or location"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error.message, "either `symbol` or (`path` + `line`) is required",
        "find_references should emit the typed invalid_params message when neither a symbol nor a location is provided"
    );
}
