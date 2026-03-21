use super::*;

#[tokio::test]
async fn core_search_symbol_returns_tree_sitter_matches() {
    let server = server_for_fixture();
    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "greeting");
    assert_eq!(response.matches[0].kind, "function");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 1);

    let note = response
        .note
        .as_ref()
        .expect("search_symbol should emit deterministic note metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("search_symbol note should be valid JSON");
    assert_eq!(note_json["source"], "tree_sitter");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn search_symbol_preserves_exact_case_prefix_and_infix_rank_order() {
    let workspace_root = temp_workspace_root("search-symbol-rank-order");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn target() {}\n\
         pub fn Target() {}\n\
         pub fn target_prefix() {}\n\
         pub fn other_target() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    let symbols = response
        .matches
        .iter()
        .map(|matched| matched.symbol.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        symbols,
        vec!["target", "Target", "target_prefix", "other_target"]
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_blade_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-blade");
    let blade_root = workspace_root.join("resources/views/components/dashboard");
    fs::create_dir_all(&blade_root).expect("failed to create temporary blade fixture");
    fs::write(
        blade_root.join("panel.blade.php"),
        "@section('hero')\n\
         @props(['title' => 'Dashboard'])\n\
         <x-slot:icon />\n\
         <livewire:orders.table />\n",
    )
    .expect("failed to seed temporary blade fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "dashboard.panel".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should return blade component matches")
        .0;

    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.symbol == "dashboard.panel" && matched.kind == "component")
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.path == "resources/views/components/dashboard/panel.blade.php")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_typescript_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-typescript");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("App.tsx"),
        "type Props = {};\n\
         export class DashboardCard {\n\
             title: string;\n\
             render(_props: Props) { return <Card />; }\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "DashboardCard".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should return typescript matches")
        .0;

    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.symbol == "DashboardCard" && matched.kind == "class")
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.path == "src/App.tsx")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_python_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-python");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary python fixture");
    fs::write(
        src_root.join("app.py"),
        concat!(
            "type Alias = str\n",
            "class Service:\n",
            "    def run(self) -> Alias:\n",
            "        return \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary python fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "Service".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should return python matches")
        .0;

    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.symbol == "Service" && matched.kind == "class")
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.path == "src/app.py")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_additional_language_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-additional-languages");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary multi-language fixture");
    fs::write(
        src_root.join("main.go"),
        concat!(
            "package main\n",
            "type GoService struct{}\n",
            "func goHelper() string { return \"ok\" }\n",
        ),
    )
    .expect("failed to seed temporary go fixture");
    fs::write(
        src_root.join("Main.kt"),
        concat!(
            "class KotlinService\n",
            "fun kotlinHelper(): String = \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary kotlin fixture");
    fs::write(
        src_root.join("init.lua"),
        concat!("function luaRun()\n", "    return \"ok\"\n", "end\n",),
    )
    .expect("failed to seed temporary lua fixture");
    fs::write(
        src_root.join("app.nim"),
        concat!("proc nimHelper(): string =\n", "  \"ok\"\n",),
    )
    .expect("failed to seed temporary nim fixture");
    fs::write(
        src_root.join("main.roc"),
        concat!("UserId := U64\n", "rocGreet = \\name -> name\n",),
    )
    .expect("failed to seed temporary roc fixture");
    let server = server_for_workspace_root(&workspace_root);

    for (query, expected_kind, expected_path) in [
        ("GoService", "struct", "src/main.go"),
        ("KotlinService", "class", "src/Main.kt"),
        ("luaRun", "function", "src/init.lua"),
        ("nimHelper", "function", "src/app.nim"),
        ("rocGreet", "function", "src/main.roc"),
    ] {
        let response = server
            .search_symbol(Parameters(SearchSymbolParams {
                query: query.to_owned(),
                repository_id: Some("repo-001".to_owned()),
                path_class: None,
                path_regex: None,
                limit: Some(20),
            }))
            .await
            .expect("search_symbol should return baseline-language matches")
            .0;

        assert!(
            response.matches.iter().any(|matched| {
                matched.symbol == query
                    && matched.kind == expected_kind
                    && matched.path == expected_path
            }),
            "expected {query} {expected_kind} match in {expected_path}, got {:?}",
            response
                .matches
                .iter()
                .map(|matched| (&matched.symbol, &matched.kind, &matched.path))
                .collect::<Vec<_>>()
        );
    }

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_resolves_php_canonical_queries() {
    let workspace_root = temp_workspace_root("search-symbol-php-canonical");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("OrderHandler.php"),
        "<?php\n\
         namespace App\\Handlers;\n\
         class OrderHandler {\n\
             public function handle(): void {}\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let class_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "App\\Handlers\\OrderHandler".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should resolve canonical php class queries")
        .0;
    assert!(
        class_response.matches.iter().any(|matched| {
            matched.symbol == "OrderHandler"
                && matched.kind == "class"
                && matched.path == "src/OrderHandler.php"
        }),
        "expected canonical class query to resolve class symbol"
    );

    let method_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "App\\Handlers\\OrderHandler::handle".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should resolve canonical php member queries")
        .0;
    assert!(
        method_response.matches.iter().any(|matched| {
            matched.symbol == "handle"
                && matched.kind == "method"
                && matched.path == "src/OrderHandler.php"
        }),
        "expected canonical member query to resolve method symbol"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_prefers_runtime_paths_within_same_lexical_rank() {
    let workspace_root = temp_workspace_root("search-symbol-runtime-first");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create tests fixture");
    fs::create_dir_all(workspace_root.join("benches")).expect("failed to create benches fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn run() {}\n")
        .expect("failed to write runtime symbol fixture");
    fs::write(workspace_root.join("tests/support.rs"), "pub fn run() {}\n")
        .expect("failed to write tests symbol fixture");
    fs::write(workspace_root.join("benches/bench.rs"), "pub fn run() {}\n")
        .expect("failed to write bench symbol fixture");

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "run".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    let paths = response
        .matches
        .iter()
        .map(|matched| matched.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths[0], "src/lib.rs");
    assert!(paths.contains(&"tests/support.rs"));
    assert!(paths.contains(&"benches/bench.rs"));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_runtime_queries_filter_inline_rust_test_symbols() {
    let workspace_root = temp_workspace_root("search-symbol-inline-rust-tests");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::write(
        workspace_root.join("src/simulator.rs"),
        "pub struct Simulator;\n\
         impl Simulator { pub fn run(&self) {} }\n\
         #[cfg(test)]\n\
         mod tests {\n\
             fn simulator_smoke() {}\n\
         }\n",
    )
    .expect("failed to write rust fixture");

    let server = server_for_workspace_root(&workspace_root);
    let runtime_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "simulator".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: Some(SearchSymbolPathClass::Runtime),
            path_regex: Some("^src/".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    assert!(
        runtime_response
            .matches
            .iter()
            .all(|matched| matched.symbol != "simulator_smoke"),
        "runtime-filtered symbol search should exclude inline test symbols: {:?}",
        runtime_response
            .matches
            .iter()
            .map(|matched| matched.symbol.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        runtime_response
            .matches
            .iter()
            .any(|matched| matched.symbol == "Simulator" && matched.kind == "struct")
    );

    let broad_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "simulator".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("broad search_symbol should succeed")
        .0;

    let symbols = broad_response
        .matches
        .iter()
        .map(|matched| matched.symbol.as_str())
        .collect::<Vec<_>>();
    let simulator_index = symbols
        .iter()
        .position(|symbol| *symbol == "Simulator")
        .expect("runtime symbol should be present");
    let smoke_index = symbols
        .iter()
        .position(|symbol| *symbol == "simulator_smoke")
        .expect("inline test symbol should still be present in broad results");
    assert!(
        simulator_index < smoke_index,
        "runtime symbols should outrank inline test symbols: {:?}",
        symbols
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_filters_by_path_class_and_path_regex() {
    let workspace_root = temp_workspace_root("search-symbol-filters");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create tests fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn run() {}\n\
         pub fn helper() {}\n",
    )
    .expect("failed to write runtime source");
    fs::write(workspace_root.join("tests/support.rs"), "pub fn run() {}\n")
        .expect("failed to write support source");

    let server = server_for_workspace_root(&workspace_root);
    let support_only = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "run".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: Some(SearchSymbolPathClass::Support),
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should respect path_class filter")
        .0;
    assert_eq!(support_only.matches.len(), 1);
    assert_eq!(support_only.matches[0].path, "tests/support.rs");

    let runtime_slice = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "run".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some(r"^src/.*\.rs$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should respect path_regex filter")
        .0;
    assert_eq!(runtime_slice.matches.len(), 1);
    assert_eq!(runtime_slice.matches[0].path, "src/lib.rs");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_search_symbol_rejects_abusive_path_regex_with_typed_invalid_params() {
    let server = server_for_fixture();
    let abusive_path_regex = "a".repeat(600);

    let error = match server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some(abusive_path_regex.clone()),
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("abusive search_symbol path_regex should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(
        error.message.contains("invalid path_regex"),
        "path_regex validation should produce a typed invalid_params message"
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("path_regex"))
            .and_then(|value| value.as_str()),
        Some(abusive_path_regex.as_str())
    );
}

#[tokio::test]
async fn search_symbol_rebuilds_stale_manifest_snapshot_before_reusing_cached_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-stale-manifest-snapshot");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let source_path = src_root.join("lib.rs");
    fs::write(&source_path, "pub fn old_name() {}\n")
        .expect("failed to seed temporary fixture source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "old_name".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed for warm snapshot")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "old_name");

    rewrite_file_with_new_mtime(&source_path, "pub fn new_name() {}\n");

    let second = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "new_name".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should rebuild stale snapshot")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "new_name");

    let stale = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "old_name".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should not keep stale symbol results")
        .0;
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_rebuilds_stale_manifest_backed_corpus_after_edit() {
    let workspace_root = temp_workspace_root("search-symbol-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("initial search_symbol call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "alpha");

    fs::write(&lib_path, "pub fn beta_beta() {}\n").expect("failed to edit source in place");

    let second = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "beta_beta".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_symbol should rebuild stale corpus after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "beta_beta");

    let stale = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_symbol should not reuse stale corpus matches")
        .0;
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_text_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("search-text-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_text(Parameters(SearchTextParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("initial search_text call should succeed")
        .0;
    assert_eq!(first.total_matches, 1);
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .search_text(Parameters(SearchTextParams {
            query: "beta_beta".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_text should bypass stale cache after edit")
        .0;
    assert_eq!(second.total_matches, 1);
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert!(second.matches[0].excerpt.contains("beta_beta"));

    let stale = server
        .search_text(Parameters(SearchTextParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_text should not reuse stale cached matches")
        .0;
    assert_eq!(stale.total_matches, 0);
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_hybrid_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("search-hybrid-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("initial search_hybrid call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "beta_beta".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should bypass stale cache after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert!(second.matches[0].excerpt.contains("beta_beta"));
    assert_eq!(
        second
            .metadata
            .as_ref()
            .map(|metadata| serde_json::to_value(metadata).expect("metadata should serialize"))
            .as_ref()
            .and_then(|metadata| metadata.get("freshness_basis"))
            .and_then(|value| value.get("cacheable"))
            .and_then(|value| value.as_bool()),
        Some(false),
        "stale manifest-backed queries should surface non-cacheable freshness metadata until a fresh snapshot exists"
    );

    let stale = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should not reuse stale cached matches")
        .0;
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}
