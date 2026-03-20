use super::*;

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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should resolve precise definition")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
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
async fn navigation_go_to_definition_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("go-to-definition-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
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
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
        }))
        .await
        .expect("go_to_definition should bypass stale cache after edit")
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
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
        }))
        .await
    {
        Ok(_) => panic!("go_to_definition should not reuse stale cached matches"),
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(1),
            column: Some(35),
            include_follow_up_structural: None,
            limit: Some(20),
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/app.rs".to_owned()),
            line: Some(2),
            column: Some(use_line.find("helper").expect("import token present") + 1),
            include_follow_up_structural: None,
            limit: Some(20),
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            line: Some(6),
            column: Some(call_line.rfind("render").expect("method token present") + 1),
            include_follow_up_structural: None,
            limit: Some(20),
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
async fn navigation_go_to_definition_prefers_runtime_paths_for_ambiguous_exact_name_queries() {
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("try_execute".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should prefer runtime code for ambiguous exact-name queries")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/lib.rs");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );
    assert_eq!(note_json["target_selection"]["ambiguous_query"], true);
    assert_eq!(note_json["target_selection"]["candidate_count"], 2);
    assert_eq!(
        note_json["target_selection"]["same_rank_candidate_count"],
        2
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_precise_results_stay_pinned_to_runtime_target_selection() {
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("try_execute".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should keep precise definitions pinned to the selected runtime target")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
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

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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
    let server = server_for_fixture();
    let response = server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_declarations should return deterministic fallback")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
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
async fn navigation_find_declarations_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("find-declarations-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
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
            symbol: Some("beta_beta".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
        }))
        .await
        .expect("find_declarations should bypass stale cache after edit")
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
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(10),
        }))
        .await
    {
        Ok(_) => panic!("find_declarations should not reuse stale cached matches"),
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
    let server = server_for_workspace_root(&workspace_root);

    let go_to_definition = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
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
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
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
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should return deterministic heuristic fallback")
        .0;

    assert!(
        !response.matches.is_empty(),
        "expected heuristic implementation matches from symbol corpus fallback"
    );
    let first = &response.matches[0];
    assert_eq!(first.repository_id, "repo-001");
    assert_eq!(first.path, "src/lib.rs");
    assert_eq!(first.symbol, "Impl");
    assert_eq!(first.relation.as_deref(), Some("implements"));
    assert_eq!(first.precision.as_deref(), Some("heuristic"));
    assert_eq!(first.fallback_reason.as_deref(), Some("precise_absent"));

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise"]["implementation_count"].as_u64(),
        Some(response.matches.len() as u64)
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

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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
                { "symbol": "scip-rust pkg repo#consumer", "range": [4, 7, 15], "symbol_roles": 1 }
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
    let server = server_for_workspace_root(&workspace_root);

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should resolve precise relationships")
        .0;
    assert_eq!(incoming.matches.len(), 1);
    assert_eq!(incoming.matches[0].source_symbol, "consumer");
    assert_eq!(incoming.matches[0].target_symbol, "Service");
    assert_eq!(incoming.matches[0].relation, "calls");
    assert_eq!(incoming.matches[0].precision.as_deref(), Some("precise"));

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("consumer".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
        }))
        .await
        .expect("outgoing_calls should resolve precise relationships")
        .0;
    assert_eq!(outgoing.matches.len(), 1);
    assert_eq!(outgoing.matches[0].source_symbol, "consumer");
    assert_eq!(outgoing.matches[0].target_symbol, "serve");
    assert_eq!(outgoing.matches[0].relation, "calls");
    assert_eq!(outgoing.matches[0].precision.as_deref(), Some("precise"));

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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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
    let server = server_for_workspace_root(&workspace_root);

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should return follow-up structural suggestions when opted in")
        .0;
    assert_eq!(implementations.matches.len(), 1);
    assert!(!implementations.matches[0].follow_up_structural.is_empty());

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should return follow-up structural suggestions when opted in")
        .0;
    assert_eq!(incoming.matches.len(), 1);
    assert!(!incoming.matches[0].follow_up_structural.is_empty());

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("consumer".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: Some(true),
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("callee".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("handler".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
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
