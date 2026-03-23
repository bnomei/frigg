use super::*;

#[tokio::test]
async fn document_symbols_returns_outline_for_supported_files() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "greeting" && symbol.kind == "function")
    );
    assert!(
        response
            .symbols
            .iter()
            .all(|symbol| symbol.path == "src/lib.rs" && symbol.repository_id == repository_id)
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["source"],
        "tree_sitter"
    );

    let note = response
        .note
        .as_ref()
        .expect("document_symbols should emit metadata note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("document_symbols note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["source"], "tree_sitter");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn document_symbols_returns_php_metadata_evidence_counts() {
    let workspace_root = temp_workspace_root("document-symbols-php-evidence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary php fixture");
    fs::write(
        src_root.join("OrderListener.php"),
        "<?php\n\
         namespace App\\Listeners;\n\
         \n\
         use App\\Attributes\\AsListener;\n\
         use App\\Contracts\\Dispatcher;\n\
         use App\\Exceptions\\OrderException;\n\
         use App\\Handlers\\OrderHandler;\n\
         \n\
         #[AsListener]\n\
         final class OrderListener\n\
         {\n\
             public function __construct(\n\
                 private Dispatcher $dispatcher,\n\
                 private OrderHandler $handler,\n\
             ) {}\n\
         \n\
             #[AsListener]\n\
             public function boot(Dispatcher $dispatcher): void\n\
             {\n\
                 $listener = new OrderHandler();\n\
                 $class = OrderHandler::class;\n\
                 $callable = [OrderHandler::class, 'handle'];\n\
                 dispatch(handler: $listener, options: ['queue' => 'orders']);\n\
         \n\
                 try {\n\
                     $dispatcher->dispatch($listener);\n\
                 } catch (OrderException $exception) {\n\
                     report($exception);\n\
                 }\n\
             }\n\
         }\n",
    )
    .expect("failed to seed temporary php fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/OrderListener.php".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return php outline metadata")
        .0;

    let php_metadata = response
        .metadata
        .as_ref()
        .expect("document_symbols should emit typed metadata")
        .get("php")
        .expect("php metadata summary should be present");
    assert!(
        php_metadata["canonical_name_count"]
            .as_u64()
            .expect("canonical name count should be numeric")
            >= 3
    );
    assert!(
        php_metadata["type_evidence_count"]
            .as_u64()
            .expect("type evidence count should be numeric")
            >= 3
    );
    assert!(
        php_metadata["target_evidence_count"]
            .as_u64()
            .expect("target evidence count should be numeric")
            >= 4
    );
    assert!(
        php_metadata["literal_evidence_count"]
            .as_u64()
            .expect("literal evidence count should be numeric")
            >= 2
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_opt_in_returns_follow_up_structural() {
    let server = server_for_fixture();
    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: Some(true),
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return outline with follow-up structural suggestions")
        .0;

    let symbol = response
        .symbols
        .first()
        .expect("fixture outline should contain a top-level symbol");
    assert_eq!(symbol.symbol, "greeting");
    assert!(!symbol.follow_up_structural.is_empty());
    assert_eq!(
        symbol.follow_up_structural[0].params.query,
        "(function_item) @match"
    );
    assert_eq!(
        symbol.follow_up_structural[0].params.path_regex.as_deref(),
        Some("^src/lib\\.rs$")
    );
}

#[tokio::test]
async fn document_symbols_returns_typescript_outline_for_tsx_files() {
    let workspace_root = temp_workspace_root("document-symbols-typescript");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("App.tsx"),
        "type Props = {};\n\
         export function App(_props: Props) {\n\
             return <Card />;\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/App.tsx".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return typescript outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Props" && symbol.kind == "type_alias")
    );
    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "App" && symbol.kind == "function")
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "typescript"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_python_outline() {
    let workspace_root = temp_workspace_root("document-symbols-python");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary python fixture");
    fs::write(
        src_root.join("app.py"),
        concat!(
            "type Alias = str\n",
            "class Service:\n",
            "    def run(self) -> None:\n",
            "        pass\n",
            "\n",
            "def helper() -> Alias:\n",
            "    return \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary python fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/app.py".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return python outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Alias" && symbol.kind == "type_alias")
    );
    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Service" && symbol.kind == "class")
    );
    assert!(
        response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "method")
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "python"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_additional_baseline_language_outlines() {
    let workspace_root = temp_workspace_root("document-symbols-additional-baselines");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary baseline fixtures");
    fs::write(
        src_root.join("main.go"),
        concat!(
            "package main\n",
            "type Service struct{}\n",
            "func helper() string { return \"ok\" }\n",
        ),
    )
    .expect("failed to seed temporary go fixture");
    fs::write(
        src_root.join("App.kt"),
        concat!(
            "class Service {\n",
            "    fun run(): String = \"ok\"\n",
            "}\n",
            "typealias Alias = String\n",
        ),
    )
    .expect("failed to seed temporary kotlin fixture");
    fs::write(
        src_root.join("Main.java"),
        concat!(
            "class JavaService {\n",
            "    String run() { return \"ok\"; }\n",
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
        concat!("UserId := U64\n", "greet = \\name -> name\n",),
    )
    .expect("failed to seed temporary roc fixture");
    fs::write(
        src_root.join("main.nim"),
        concat!(
            "type Service = object\n",
            "proc helper(): string =\n",
            "  \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary nim fixture");
    let server = server_for_workspace_root(&workspace_root);

    let go_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/main.go".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return go outline")
        .0;
    assert!(
        go_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "main" && symbol.kind == "module")
    );
    assert_eq!(
        go_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "go"
    );

    let kotlin_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/App.kt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return kotlin outline")
        .0;
    assert!(
        kotlin_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Service" && symbol.kind == "class")
    );
    assert!(
        kotlin_response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "method")
    );
    assert_eq!(
        kotlin_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "kotlin"
    );

    let java_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/Main.java".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return java outline")
        .0;
    assert!(
        java_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "JavaService" && symbol.kind == "class")
    );
    assert!(
        java_response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "method")
    );
    assert_eq!(
        java_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "java"
    );

    let lua_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/init.lua".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return lua outline")
        .0;
    assert!(
        lua_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "function")
    );
    assert_eq!(
        lua_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "lua"
    );

    let roc_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/main.roc".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return roc outline")
        .0;
    assert!(
        roc_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "UserId" && symbol.kind == "type_alias")
    );
    assert!(
        roc_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "greet" && symbol.kind == "function")
    );
    assert_eq!(
        roc_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "roc"
    );

    let nim_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/main.nim".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return nim outline")
        .0;
    assert!(
        nim_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Service" && symbol.kind == "struct")
    );
    assert!(
        nim_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "helper" && symbol.kind == "function")
    );
    assert_eq!(
        nim_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "nim"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_hierarchy_for_nested_symbols() {
    let workspace_root = temp_workspace_root("document-symbols-hierarchy");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "mod inner {\n    pub fn nested() {}\n}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return nested outline")
        .0;

    assert_eq!(response.symbols.len(), 1);
    assert_eq!(response.symbols[0].symbol, "inner");
    assert_eq!(response.symbols[0].children.len(), 1);
    assert_eq!(response.symbols[0].children[0].symbol, "nested");
    assert_eq!(
        response.symbols[0].children[0].container.as_deref(),
        Some("inner")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_top_level_only_defaults_to_compact_and_clears_children() {
    let workspace_root = temp_workspace_root("document-symbols-top-level-only");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "mod inner {\n    pub fn nested() {}\n}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            top_level_only: Some(true),
            ..Default::default()
        }))
        .await
        .expect("compact document_symbols should return a top-level outline")
        .0;

    assert_eq!(response.symbols.len(), 1);
    assert_eq!(response.symbols[0].symbol, "inner");
    assert!(
        response.symbols[0].children.is_empty(),
        "top_level_only should suppress child symbol trees"
    );
    assert!(response.metadata.is_none());
    assert!(response.note.is_none());
    assert!(
        response.result_handle.is_some(),
        "compact document_symbols should return a result handle"
    );
    assert!(
        response.symbols[0].match_id.is_some(),
        "compact document_symbols should expose match ids"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_blade_outline() {
    let workspace_root = temp_workspace_root("document-symbols-blade");
    let blade_root = workspace_root.join("resources/views/components/dashboard");
    fs::create_dir_all(&blade_root).expect("failed to create temporary blade fixture");
    fs::write(
        blade_root.join("panel.blade.php"),
        "@section('hero')\n\
         @props(['title' => 'Dashboard'])\n\
         @aware(['tone'])\n\
         <x-slot:icon />\n\
         <x-alert.banner />\n\
         <livewire:orders.table />\n\
         <flux:button wire:click=\"save\" wire:model.live=\"state\" />\n",
    )
    .expect("failed to seed temporary blade fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "resources/views/components/dashboard/panel.blade.php".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
        .expect("document_symbols should return blade outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "components.dashboard.panel" && symbol.kind == "module")
    );
    assert!(
        response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "dashboard.panel" && symbol.kind == "component")
    );
    assert!(
        response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "hero" && symbol.kind == "section")
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["source"],
        "tree_sitter"
    );
    let blade_metadata = response
        .metadata
        .as_ref()
        .expect("document_symbols should emit typed metadata")
        .get("blade")
        .expect("blade metadata summary should be present");
    assert_eq!(blade_metadata["relations_detected"], 1);
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

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_rejects_unsupported_extension_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "README.md".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
    {
        Ok(_) => panic!("unsupported document_symbols extension should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error_data_field(&error, "supported_extensions"),
        &serde_json::json!([
            ".rs",
            ".php",
            ".blade.php",
            ".ts",
            ".tsx",
            ".py",
            ".go",
            ".kt",
            ".kts",
            ".lua",
            ".roc",
            ".nim",
            ".nims"
        ])
    );
}

#[tokio::test]
async fn document_symbols_rejects_over_budget_source_with_typed_error() {
    let workspace_root = temp_workspace_root("document-symbols-max-bytes");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(src_root.join("lib.rs"), "pub fn oversized_symbol() {}\n")
        .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 8);

    let error = match server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            response_mode: Some(ResponseMode::Full),
            ..Default::default()
        }))
        .await
    {
        Ok(_) => panic!("over-budget document_symbols request should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));

    let data = error
        .data
        .as_ref()
        .expect("document_symbols over-budget error should carry structured data");
    assert_eq!(data["path"], "src/lib.rs");
    assert_eq!(data["max_bytes"], 8);
    assert!(
        data["bytes"]
            .as_u64()
            .expect("document_symbols bytes should be numeric")
            > 8
    );

    cleanup_workspace_root(&workspace_root);
}
