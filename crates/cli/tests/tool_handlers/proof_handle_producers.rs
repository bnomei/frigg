//! Revision-proof coverage for public result-handle producers.
//!
//! The source mutation follows handle issuance and precedes any watch/index invalidation, so
//! this is deterministic proof validation rather than an event-delivery test.

use super::*;
use frigg::mcp::types::{SearchBatchParams, SearchBatchProbe, SearchBatchProbeKind};

#[derive(Debug)]
struct IssuedPair {
    origin: &'static str,
    result_handle: String,
    match_id: String,
}

fn issued_pair(
    origin: &'static str,
    result_handle: Option<String>,
    match_id: Option<String>,
) -> IssuedPair {
    IssuedPair {
        origin,
        result_handle: result_handle
            .unwrap_or_else(|| panic!("{origin} should issue a result_handle")),
        match_id: match_id.unwrap_or_else(|| panic!("{origin} should issue a match_id")),
    }
}

fn assert_stale_proof_anchor(error: &rmcp::ErrorData, pair: &IssuedPair) {
    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(error_code_tag(error), Some("STALE_PROOF_ANCHOR"));
    let data = error
        .data
        .as_ref()
        .expect("stale proof failure should include structured recovery");
    assert_eq!(data["origin_tool"], pair.origin);
    assert_eq!(data["result_handle"], pair.result_handle);
    assert_eq!(data["match_id"], pair.match_id);
    assert!(data["repository_id"].is_string());
    assert!(data["path"].is_string());
    assert!(data["correction_hint"].is_string());
    assert!(data["related_tools"].is_array());
    assert_eq!(data["retryable"], false);
    let allowed = [
        "error_code",
        "repository_id",
        "path",
        "origin_tool",
        "result_handle",
        "match_id",
        "correction_hint",
        "related_tools",
        "retryable",
    ];
    for key in data
        .as_object()
        .expect("stale proof recovery must be a JSON object")
        .keys()
    {
        assert!(
            allowed.contains(&key.as_str()),
            "stale proof recovery must not leak sensitive or replay field {key:?}: {data:?}"
        );
    }
}

async fn assert_pair_is_stale_after_edit(server: &FriggMcpServer, pair: &IssuedPair) {
    let error = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: pair.result_handle.clone(),
            match_id: pair.match_id.clone(),
            before: Some(0),
            after: Some(0),
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect_err("mutated source must reject its historical proof pair");
    assert_stale_proof_anchor(&error, pair);
}

async fn issue_text_proof(server: &FriggMcpServer) -> IssuedPair {
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "greeting".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search_text should issue a bound proof")
        .0;
    issued_pair(
        "search_text",
        response.result_handle,
        response
            .matches
            .first()
            .and_then(|row| row.match_id.clone()),
    )
}

#[tokio::test]
async fn proof_handle_producers_reject_mutated_source_before_invalidation() {
    let workspace_root = temp_workspace_root("proof-handle-producer-matrix");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("fixture source directory should exist");
    let source_path = src_root.join("lib.rs");
    fs::write(
        &source_path,
        "pub trait Worker { fn work(&self); }\n\
         pub struct WorkerImpl;\n\
         impl Worker for WorkerImpl { fn work(&self) {} }\n\
         pub fn target() {}\n\
         pub fn caller() { target(); }\n\
         pub mod nested { pub fn child() {} }\n",
    )
    .expect("fixture source should persist");
    write_scip_fixture(
        &workspace_root,
        "proof-producers.json",
        r#"{
          "documents": [{
            "relative_path": "src/lib.rs",
            "occurrences": [
              { "symbol": "scip-rust pkg repo#Worker", "range": [0, 10, 16], "symbol_roles": 1 },
              { "symbol": "scip-rust pkg repo#WorkerImpl", "range": [1, 11, 21], "symbol_roles": 1 },
              { "symbol": "scip-rust pkg repo#Worker", "range": [2, 5, 11], "symbol_roles": 8 },
              { "symbol": "scip-rust pkg repo#WorkerImpl", "range": [2, 16, 26], "symbol_roles": 8 },
              { "symbol": "scip-rust pkg repo#target", "range": [3, 7, 13], "symbol_roles": 1 },
              { "symbol": "scip-rust pkg repo#caller", "range": [4, 7, 13], "symbol_roles": 1 },
              { "symbol": "scip-rust pkg repo#target", "range": [4, 18, 24], "symbol_roles": 8 }
            ],
            "symbols": [
              { "symbol": "scip-rust pkg repo#Worker", "display_name": "Worker", "kind": "trait", "relationships": [] },
              { "symbol": "scip-rust pkg repo#WorkerImpl", "display_name": "WorkerImpl", "kind": "struct",
                "relationships": [{ "symbol": "scip-rust pkg repo#Worker", "is_implementation": true }] },
              { "symbol": "scip-rust pkg repo#target", "display_name": "target", "kind": "function", "relationships": [] },
              { "symbol": "scip-rust pkg repo#caller", "display_name": "caller", "kind": "function", "relationships": [] }
            ]
          }]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root).await;

    let text = server
        .search_text(Parameters(SearchTextParams {
            query: "target".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(20),
            ..Default::default()
        }))
        .await
        .expect("search_text should find target")
        .0;
    let text_pair = issued_pair(
        "search_text",
        text.result_handle,
        text.matches.first().and_then(|r| r.match_id.clone()),
    );

    let symbol = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("search_symbol should find target")
        .0;
    let symbol_pair = issued_pair(
        "search_symbol",
        symbol.result_handle,
        symbol.matches.first().and_then(|r| r.match_id.clone()),
    );

    let batch = server
        .search_batch(Parameters(SearchBatchParams {
            probes: vec![
                SearchBatchProbe {
                    id: "target-text".to_owned(),
                    kind: SearchBatchProbeKind::Text,
                    query: "target".to_owned(),
                    repository_id: Some("repo-001".to_owned()),
                    path_regex: Some(r"^src/lib\.rs$".to_owned()),
                    glob: None,
                    path_class: None,
                    pattern_type: Some(SearchPatternType::Literal),
                },
                SearchBatchProbe {
                    id: "target-symbol".to_owned(),
                    kind: SearchBatchProbeKind::Symbol,
                    query: "target".to_owned(),
                    repository_id: Some("repo-001".to_owned()),
                    path_regex: Some(r"^src/lib\.rs$".to_owned()),
                    glob: None,
                    path_class: None,
                    pattern_type: None,
                },
            ],
            merge: None,
            limit: Some(20),
            repository_id: Some("repo-001".to_owned()),
            response_mode: Some(ResponseMode::Compact),
            resume_from: None,
        }))
        .await
        .expect("search_batch should find target")
        .0;
    let batch_pair = issued_pair(
        "search_batch",
        batch.result_handle,
        batch.matches.first().and_then(|r| r.match_id.clone()),
    );

    let hybrid = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "target".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(20),
            weights: None,
            semantic: Some(false),
            response_mode: Some(ResponseMode::Compact),
            include_context_efficiency: None,
        }))
        .await
        .expect("search_hybrid should find target")
        .0;
    let hybrid_pair = issued_pair(
        "search_hybrid",
        hybrid.result_handle,
        hybrid.matches.first().and_then(|r| r.match_id.clone()),
    );

    let references = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("target".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(true),
            include_follow_up_structural: None,
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("find_references should find target")
        .0;
    let references_pair = issued_pair(
        "find_references",
        references.result_handle,
        references.matches.first().and_then(|r| r.match_id.clone()),
    );

    let definition = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("target".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("go_to_definition should find target")
        .0;
    let definition_pair = issued_pair(
        "go_to_definition",
        definition.result_handle,
        definition.matches.first().and_then(|r| r.match_id.clone()),
    );

    let declarations = server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("target".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("find_declarations should find target")
        .0;
    let declarations_pair = issued_pair(
        "find_declarations",
        declarations.result_handle,
        declarations
            .matches
            .first()
            .and_then(|r| r.match_id.clone()),
    );

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Worker".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("find_implementations should find WorkerImpl")
        .0;
    let implementations_pair = issued_pair(
        "find_implementations",
        implementations.result_handle,
        implementations
            .matches
            .first()
            .and_then(|r| r.match_id.clone()),
    );

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("target".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("incoming_calls should find caller")
        .0;
    let incoming_pair = issued_pair(
        "incoming_calls",
        incoming.result_handle,
        incoming.matches.first().and_then(|r| r.match_id.clone()),
    );

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_follow_up_structural: None,
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("outgoing_calls should find target")
        .0;
    let outgoing_pair = issued_pair(
        "outgoing_calls",
        outgoing.result_handle,
        outgoing.matches.first().and_then(|r| r.match_id.clone()),
    );

    let symbols = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            include_follow_up_structural: None,
            top_level_only: Some(false),
            limit: Some(20),
            resume_from: None,
            continuation: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("document_symbols should find target")
        .0;
    let document_pair = issued_pair(
        "document_symbols",
        symbols.result_handle.clone(),
        symbols
            .symbols
            .iter()
            .find(|r| r.symbol == "target")
            .and_then(|r| r.match_id.clone()),
    );
    let nested_document_pair = issued_pair(
        "document_symbols",
        symbols.result_handle.clone(),
        symbols
            .symbols
            .iter()
            .find(|row| row.symbol == "nested")
            .and_then(|row| row.children.first())
            .and_then(|row| row.match_id.clone()),
    );

    let impact = server
        .impact_bundle(Parameters(ImpactBundleParams {
            symbol: "Worker".to_owned(),
            path_class: None,
            repository_id: Some("repo-001".to_owned()),
            include_implementations: Some(true),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("impact_bundle should compose proof-bearing child families")
        .0;
    let impact_wire = serde_json::to_value(&impact).expect("impact_bundle should serialize");
    assert!(
        impact_wire.get("result_handle").is_none(),
        "impact_bundle must not introduce an aggregate proof handle"
    );
    let mut impact_pairs = Vec::new();
    if !impact.symbols.is_empty() {
        impact_pairs.push(issued_pair(
            "search_symbol",
            impact.symbols_result_handle,
            impact.symbols.first().and_then(|r| r.match_id.clone()),
        ));
    }
    if !impact.references.is_empty() {
        impact_pairs.push(issued_pair(
            "find_references",
            impact.references_result_handle,
            impact.references.first().and_then(|r| r.match_id.clone()),
        ));
    }
    if !impact.incoming_calls.is_empty() {
        impact_pairs.push(issued_pair(
            "incoming_calls",
            impact.incoming_calls_result_handle,
            impact
                .incoming_calls
                .first()
                .and_then(|r| r.match_id.clone()),
        ));
    }
    if !impact.implementations.is_empty() {
        impact_pairs.push(issued_pair(
            "find_implementations",
            impact.implementations_result_handle,
            impact
                .implementations
                .first()
                .and_then(|r| r.match_id.clone()),
        ));
    }

    fs::write(
        &source_path,
        "// inserted before every historic anchor\n\
        pub trait Worker { fn work(&self); }\n\
        pub struct WorkerImpl;\n\
        impl Worker for WorkerImpl { fn work(&self) {} }\n\
        pub fn target() { /* replacement */ }\n\
        pub fn caller() { target(); }\n",
    )
    .expect("fixture mutation should persist without a watcher callback");

    for pair in [
        &text_pair,
        &symbol_pair,
        &batch_pair,
        &hybrid_pair,
        &references_pair,
        &definition_pair,
        &declarations_pair,
        &implementations_pair,
        &incoming_pair,
        &outgoing_pair,
        &document_pair,
        &nested_document_pair,
    ] {
        assert_pair_is_stale_after_edit(&server, pair).await;
    }
    for pair in &impact_pairs {
        assert_pair_is_stale_after_edit(&server, pair).await;
    }

    let delete = server
        .search_text(Parameters(SearchTextParams {
            query: "target".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search_text should issue a proof before source deletion")
        .0;
    let delete_pair = issued_pair(
        "search_text",
        delete.result_handle,
        delete.matches.first().and_then(|row| row.match_id.clone()),
    );
    fs::remove_file(&source_path).expect("fixture source deletion should persist");
    assert_pair_is_stale_after_edit(&server, &delete_pair).await;
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn proof_handle_same_bytes_survive_and_all_presentations_fail_closed_after_edit() {
    let workspace_root = fresh_fixture_root("proof-handle-presentation-modes");
    let source_path = workspace_root.join("src/lib.rs");
    let server = server_for_workspace_root(&workspace_root).await;

    let same_bytes_pair = issue_text_proof(&server).await;
    let original = fs::read_to_string(&source_path).expect("fixture source should be readable");
    fs::write(&source_path, &original).expect("identical-byte rewrite should persist");
    for mode in [
        ReadPresentationMode::Json,
        ReadPresentationMode::Text,
        ReadPresentationMode::Citation,
    ] {
        server
            .read_match(Parameters(ReadMatchParams {
                result_handle: same_bytes_pair.result_handle.clone(),
                match_id: same_bytes_pair.match_id.clone(),
                before: Some(0),
                after: Some(0),
                presentation_mode: Some(mode),
                include_context_efficiency: None,
            }))
            .await
            .expect("identical bytes must retain a valid proof for every presentation mode");
    }

    let stale_pair = issue_text_proof(&server).await;
    fs::write(&source_path, format!("// revision changed\n{original}"))
        .expect("fixture edit should persist without watch invalidation");
    for mode in [
        ReadPresentationMode::Json,
        ReadPresentationMode::Text,
        ReadPresentationMode::Citation,
    ] {
        let error = server
            .read_match(Parameters(ReadMatchParams {
                result_handle: stale_pair.result_handle.clone(),
                match_id: stale_pair.match_id.clone(),
                before: Some(0),
                after: Some(0),
                presentation_mode: Some(mode),
                include_context_efficiency: None,
            }))
            .await
            .expect_err("changed source must reject proof before formatting content");
        assert_stale_proof_anchor(&error, &stale_pair);
    }

    let fresh_pair = issue_text_proof(&server).await;
    server
        .read_match(Parameters(ReadMatchParams {
            result_handle: fresh_pair.result_handle,
            match_id: fresh_pair.match_id,
            before: Some(0),
            after: Some(0),
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect("fresh producer rerun should issue a usable proof");
    cleanup_workspace_root(&workspace_root);
}
