use super::*;
use frigg::mcp::types::{StructuralAnchorSelection, StructuralResultMode};

#[tokio::test]
async fn search_structural_returns_deterministic_rust_matches() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let first = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return matches")
        .0;
    let second = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should be deterministic")
        .0;

    assert_eq!(
        first
            .matches
            .iter()
            .map(|matched| {
                (
                    matched.repository_id.clone(),
                    matched.path.clone(),
                    matched.line,
                    matched.column,
                    matched.end_line,
                    matched.end_column,
                    matched.excerpt.clone(),
                )
            })
            .collect::<Vec<_>>(),
        second
            .matches
            .iter()
            .map(|matched| {
                (
                    matched.repository_id.clone(),
                    matched.path.clone(),
                    matched.line,
                    matched.column,
                    matched.end_line,
                    matched.end_column,
                    matched.excerpt.clone(),
                )
            })
            .collect::<Vec<_>>()
    );
    assert!(!first.matches.is_empty());
    assert_eq!(first.matches[0].repository_id, repository_id);
    assert_eq!(first.matches[0].path, "src/lib.rs");
    assert!(first.matches[0].line >= 1);

    let note = first
        .note
        .as_ref()
        .expect("search_structural should emit metadata note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("search_structural note should be valid JSON");
    assert_eq!(
        first
            .metadata
            .as_ref()
            .expect("search_structural should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["source"], "tree_sitter_query");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn search_structural_returns_deterministic_blade_matches() {
    let workspace_root = temp_workspace_root("search-structural-blade");
    let blade_root = workspace_root.join("resources/views/components/dashboard");
    fs::create_dir_all(&blade_root).expect("failed to create temporary blade fixture");
    fs::write(
        blade_root.join("panel.blade.php"),
        "<x-slot:icon />\n\
         <x-alert.banner />\n\
         <livewire:orders.table />\n\
         <flux:button wire:click=\"save\" wire:model.live=\"state\" />\n",
    )
    .expect("failed to seed temporary blade fixture");
    let server = server_for_workspace_root(&workspace_root);

    let first = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(self_closing_tag (tag_name) @tag)".to_owned(),
            language: Some("blade".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"panel\.blade\.php$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return blade matches")
        .0;
    let second = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(self_closing_tag (tag_name) @tag)".to_owned(),
            language: Some("blade".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"panel\.blade\.php$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should deterministically return blade matches")
        .0;

    assert_eq!(first.matches.len(), second.matches.len());
    assert!(!first.matches.is_empty());
    assert!(
        first
            .matches
            .iter()
            .any(|matched| matched.excerpt == "x-slot:icon"),
        "expected blade structural match for x-slot tag"
    );
    let blade_metadata = first
        .metadata
        .as_ref()
        .expect("search_structural should emit typed metadata")
        .get("blade")
        .expect("blade aggregate metadata should be present");
    assert_eq!(
        blade_metadata["livewire_components"],
        serde_json::json!(["orders.table"])
    );
    assert_eq!(
        blade_metadata["wire_directives"],
        serde_json::json!(["wire:click", "wire:model.live"])
    );
    assert_eq!(
        blade_metadata["flux_components"],
        serde_json::json!(["flux:button"])
    );
    assert_eq!(blade_metadata["flux_registry_version"], "2026-03-08-mvp");
    assert_eq!(blade_metadata["relations_detected"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_typescript_tsx_matches() {
    let workspace_root = temp_workspace_root("search-structural-typescript");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary tsx fixture");
    fs::write(
        src_root.join("App.tsx"),
        "export function App() {\n\
             return <Card />;\n\
         }\n",
    )
    .expect("failed to seed temporary tsx fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(jsx_self_closing_element) @jsx".to_owned(),
            language: Some("tsx".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"App\.tsx$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return typescript matches")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].path, "src/App.tsx");
    assert_eq!(response.matches[0].excerpt, "<Card />");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("search_structural should emit typed metadata")["language"],
        "typescript"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_python_matches() {
    let workspace_root = temp_workspace_root("search-structural-python");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary python fixture");
    fs::write(
        src_root.join("app.py"),
        concat!(
            "def first():\n",
            "    return 1\n",
            "\n",
            "def second():\n",
            "    return 2\n",
        ),
    )
    .expect("failed to seed temporary python fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_definition) @fn".to_owned(),
            language: Some("py".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"app\.py$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return python matches")
        .0;

    assert_eq!(response.matches.len(), 2);
    assert_eq!(response.matches[0].path, "src/app.py");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("search_structural should emit typed metadata")["language"],
        "python"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_additional_baseline_language_matches() {
    let workspace_root = temp_workspace_root("search-structural-additional-baselines");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary baseline fixtures");
    fs::write(
        src_root.join("main.go"),
        concat!("package main\n", "func helper() string { return \"ok\" }\n",),
    )
    .expect("failed to seed temporary go fixture");
    fs::write(
        src_root.join("App.kt"),
        concat!(
            "class Service {\n",
            "    fun run(): String = \"ok\"\n",
            "}\n",
            "fun helper(): String = \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary kotlin fixture");
    fs::write(
        src_root.join("Main.java"),
        concat!(
            "class JavaService {\n",
            "    String run() { return \"ok\"; }\n",
            "}\n",
            "interface Runner {\n",
            "    String find();\n",
            "}\n",
        ),
    )
    .expect("failed to seed temporary java fixture");
    fs::write(
        src_root.join("init.lua"),
        concat!("function Service.run()\n", "    return \"ok\"\n", "end\n",),
    )
    .expect("failed to seed temporary lua fixture");
    fs::write(
        src_root.join("main.roc"),
        concat!("greet = \\name -> name\n", "id = 1\n",),
    )
    .expect("failed to seed temporary roc fixture");
    fs::write(
        src_root.join("main.nim"),
        concat!("proc helper(): string =\n", "  \"ok\"\n",),
    )
    .expect("failed to seed temporary nim fixture");
    let server = server_for_workspace_root(&workspace_root);

    let go_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_declaration) @fn".to_owned(),
            language: Some("golang".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"main\.go$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return go matches")
        .0;
    assert_eq!(go_response.matches.len(), 1);
    assert_eq!(
        go_response.metadata.as_ref().expect("typed metadata")["language"],
        "go"
    );

    let kotlin_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_declaration) @fn".to_owned(),
            language: Some("kt".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"App\.kt$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return kotlin matches")
        .0;
    assert_eq!(kotlin_response.matches.len(), 2);
    assert_eq!(
        kotlin_response.metadata.as_ref().expect("typed metadata")["language"],
        "kotlin"
    );

    let java_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(method_declaration) @fn".to_owned(),
            language: Some("java".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"Main\.java$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return java matches")
        .0;
    assert_eq!(java_response.matches.len(), 2);
    assert_eq!(
        java_response.metadata.as_ref().expect("typed metadata")["language"],
        "java"
    );

    let lua_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_declaration) @fn".to_owned(),
            language: Some("lua".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"init\.lua$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return lua matches")
        .0;
    assert_eq!(lua_response.matches.len(), 1);
    assert_eq!(
        lua_response.metadata.as_ref().expect("typed metadata")["language"],
        "lua"
    );

    let roc_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(value_declaration) @value".to_owned(),
            language: Some("roc".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"main\.roc$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return roc matches")
        .0;
    assert_eq!(roc_response.matches.len(), 2);
    assert_eq!(
        roc_response.metadata.as_ref().expect("typed metadata")["language"],
        "roc"
    );

    let nim_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(proc_declaration) @proc".to_owned(),
            language: Some("nims".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"main\.nim$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("search_structural should return nim matches")
        .0;
    assert_eq!(nim_response.matches.len(), 1);
    assert_eq!(
        nim_response.metadata.as_ref().expect("typed metadata")["language"],
        "nim"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_rejects_unsupported_language_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("javascript".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
    {
        Ok(_) => panic!("unsupported structural search language should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error_data_field(&error, "supported_languages"),
        &serde_json::json!([
            "rust",
            "php",
            "blade",
            "typescript",
            "python",
            "go",
            "kotlin",
            "java",
            "lua",
            "roc",
            "nim"
        ])
    );
}

#[tokio::test]
async fn search_structural_rejects_invalid_query_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item @broken".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
    {
        Ok(_) => panic!("invalid structural query should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("invalid structural query"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn search_structural_opt_in_returns_per_match_follow_up_structural() {
    let server = server_for_fixture();
    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: Some(true),
        }))
        .await
        .expect("search_structural should return matches with follow-up suggestions")
        .0;

    let first_match = response
        .matches
        .first()
        .expect("search_structural should return at least one match");
    assert_eq!(first_match.path, "src/lib.rs");
    assert_eq!(first_match.follow_up_structural.len(), 2);
    assert_eq!(
        first_match.follow_up_structural[0].params.query,
        "(function_item) @match"
    );
    assert_eq!(
        first_match.follow_up_structural[0]
            .params
            .path_regex
            .as_deref(),
        Some("^src/lib\\.rs$")
    );
    assert_eq!(first_match.follow_up_structural[1].params.path_regex, None);
}

#[tokio::test]
async fn search_structural_defaults_to_grouped_match_rows_for_multi_capture_query() {
    let server = server_for_fixture();
    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item name: (identifier) @name) @match".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
            result_mode: None,
            primary_capture: None,
            include_follow_up_structural: None,
        }))
        .await
        .expect("grouped structural search should succeed")
        .0;

    assert_eq!(response.result_mode, StructuralResultMode::Matches);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(
        response.matches[0].anchor_capture_name.as_deref(),
        Some("match")
    );
    assert_eq!(
        response.matches[0].anchor_selection,
        StructuralAnchorSelection::MatchCapture
    );
    assert_eq!(response.matches[0].captures.len(), 2);
    assert_eq!(response.matches[0].captures[0].name, "match");
    assert_eq!(response.matches[0].captures[1].name, "name");
    assert!(
        response
            .metadata
            .as_ref()
            .and_then(|value| value.get("noisy_result_hints"))
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty())
    );
}

#[tokio::test]
async fn search_structural_capture_mode_remains_available_for_debugging() {
    let server = server_for_fixture();
    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item name: (identifier) @name) @match".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
            result_mode: Some(StructuralResultMode::Captures),
            primary_capture: Some("name".to_owned()),
            include_follow_up_structural: None,
        }))
        .await
        .expect("capture mode structural search should succeed")
        .0;

    assert_eq!(response.result_mode, StructuralResultMode::Captures);
    assert_eq!(response.matches.len(), 2);
    assert!(response.matches.iter().all(|matched| {
        matched.anchor_selection == StructuralAnchorSelection::CaptureRow
            && matched.captures.len() == 1
    }));
}
