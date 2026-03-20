use std::fs;
use std::path::PathBuf;

use protobuf::{EnumOrUnknown, Message};

use super::*;

fn symbol(symbol_id: &str, display_name: &str, kind: &str, path: &str, line: usize) -> SymbolNode {
    SymbolNode::new(symbol_id, "repo-001", display_name, kind, path, line)
}

#[test]
fn relation_traversal_registers_symbols_and_relations_deterministically() {
    let mut graph = SymbolGraph::default();
    graph.register_symbols([
        symbol("sym-class-user", "User", "class", "src/user.php", 3),
        symbol("sym-method-save", "save", "method", "src/user.php", 5),
        symbol(
            "sym-method-validate",
            "validate",
            "method",
            "src/user.php",
            9,
        ),
        symbol("sym-const-limit", "LIMIT", "constant", "src/user.php", 12),
    ]);

    assert_eq!(graph.symbol_count(), 4);

    assert!(
        graph
            .add_relation("sym-method-save", "sym-class-user", RelationKind::DefinedIn)
            .expect("defined_in insertion should succeed")
    );
    assert!(
        graph
            .add_relation(
                "sym-method-save",
                "sym-method-validate",
                RelationKind::Calls
            )
            .expect("calls insertion should succeed")
    );
    assert!(
        graph
            .add_relation("sym-method-save", "sym-const-limit", RelationKind::RefersTo)
            .expect("refers_to insertion should succeed")
    );

    // Duplicate edges with same relation are rejected deterministically.
    assert!(
        !graph
            .add_relation(
                "sym-method-save",
                "sym-method-validate",
                RelationKind::Calls
            )
            .expect("duplicate calls edge should be deduplicated")
    );

    assert_eq!(graph.relation_count(), 3);

    let outgoing = graph.outgoing_relations("sym-method-save");
    assert_eq!(
        outgoing,
        vec![
            SymbolRelation {
                from_symbol: "sym-method-save".to_string(),
                to_symbol: "sym-class-user".to_string(),
                relation: RelationKind::DefinedIn,
            },
            SymbolRelation {
                from_symbol: "sym-method-save".to_string(),
                to_symbol: "sym-const-limit".to_string(),
                relation: RelationKind::RefersTo,
            },
            SymbolRelation {
                from_symbol: "sym-method-save".to_string(),
                to_symbol: "sym-method-validate".to_string(),
                relation: RelationKind::Calls,
            },
        ]
    );

    let incoming = graph.incoming_relations("sym-method-validate");
    assert_eq!(
        incoming,
        vec![SymbolRelation {
            from_symbol: "sym-method-save".to_string(),
            to_symbol: "sym-method-validate".to_string(),
            relation: RelationKind::Calls,
        }]
    );

    let outgoing_neighbors = graph.outgoing_adjacency("sym-method-save");
    assert_eq!(
        outgoing_neighbors
            .iter()
            .map(|adjacent| (adjacent.relation, adjacent.symbol.symbol_id.clone()))
            .collect::<Vec<_>>(),
        vec![
            (RelationKind::DefinedIn, "sym-class-user".to_string()),
            (RelationKind::RefersTo, "sym-const-limit".to_string()),
            (RelationKind::Calls, "sym-method-validate".to_string()),
        ]
    );
}

#[test]
fn relation_traversal_requires_pre_registered_symbols() {
    let mut graph = SymbolGraph::default();
    graph.register_symbol(symbol(
        "sym-existing",
        "existing",
        "function",
        "src/lib.rs",
        1,
    ));

    let from_error = graph
        .add_relation("sym-missing", "sym-existing", RelationKind::RefersTo)
        .expect_err("missing source symbol should fail relation insertion");
    assert_eq!(
        from_error,
        SymbolGraphError::UnknownFromSymbol("sym-missing".to_string())
    );

    let to_error = graph
        .add_relation("sym-existing", "sym-also-missing", RelationKind::RefersTo)
        .expect_err("missing target symbol should fail relation insertion");
    assert_eq!(
        to_error,
        SymbolGraphError::UnknownToSymbol("sym-also-missing".to_string())
    );
}

#[test]
fn relation_traversal_register_symbol_upserts_existing_entry() {
    let mut graph = SymbolGraph::default();

    assert!(graph.register_symbol(symbol("sym-user", "User", "struct", "src/user.rs", 3,)));

    assert!(!graph.register_symbol(symbol(
        "sym-user",
        "UserRenamed",
        "struct",
        "src/user.rs",
        44,
    )));

    let symbol = graph
        .symbol("sym-user")
        .expect("registered symbol should be queryable");
    assert_eq!(symbol.display_name, "UserRenamed");
    assert_eq!(symbol.line, 44);
    assert_eq!(graph.symbol_count(), 1);
}

#[test]
fn heuristic_relation_hints_are_confidence_ranked_and_deterministic() {
    let mut graph = SymbolGraph::default();
    graph.register_symbols([
        symbol("sym-target", "User", "class", "src/user.php", 3),
        symbol("sym-calls", "save", "method", "src/service.php", 11),
        symbol("sym-contains", "Service", "class", "src/service.php", 1),
        symbol("sym-defined-in", "User", "class", "src/user.php", 3),
    ]);

    assert!(
        graph
            .add_relation("sym-calls", "sym-target", RelationKind::Calls)
            .expect("calls relation should be accepted")
    );
    assert!(
        graph
            .add_relation("sym-contains", "sym-target", RelationKind::Contains)
            .expect("contains relation should be accepted")
    );
    assert!(
        graph
            .add_relation("sym-defined-in", "sym-target", RelationKind::DefinedIn)
            .expect("defined_in relation should be accepted")
    );

    let first = graph.heuristic_relation_hints_for_target("sym-target");
    let second = graph.heuristic_relation_hints_for_target("sym-target");

    assert_eq!(first, second, "hint ordering should be deterministic");
    assert_eq!(
        first
            .iter()
            .map(|hint| (hint.source_symbol.symbol_id.clone(), hint.confidence))
            .collect::<Vec<_>>(),
        vec![
            ("sym-calls".to_string(), HeuristicConfidence::High),
            ("sym-contains".to_string(), HeuristicConfidence::Medium),
            ("sym-defined-in".to_string(), HeuristicConfidence::Low),
        ]
    );
}

#[test]
fn scip_ingest_maps_and_persists_normalized_records() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#User", "range": [1, 18, 22], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Entity", "is_reference": true },
                    { "symbol": "scip-rust pkg a#Entity", "is_implementation": true }
                  ]
                }
              ]
            },
            {
              "relative_path": "src/base.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#Entity", "range": [0, 11, 17], "symbol_roles": 1 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg a#Entity", "display_name": "Entity", "kind": 7, "relationships": [] }
              ]
            }
          ]
        }"#;

    let summary = graph
        .ingest_scip_json("repo-001", "fixture:scip.json", payload)
        .expect("valid scip payload should ingest successfully");
    assert_eq!(summary.documents_ingested, 2);
    assert_eq!(summary.symbols_upserted, 2);
    assert_eq!(summary.occurrences_upserted, 3);
    assert_eq!(summary.relationships_upserted, 2);

    let counts = graph.precise_counts();
    assert_eq!(counts.symbols, 2);
    assert_eq!(counts.occurrences, 3);
    assert_eq!(counts.relationships, 2);

    let user_symbol = graph
        .precise_symbol("repo-001", "scip-rust pkg a#User")
        .expect("expected precise symbol");
    assert_eq!(user_symbol.display_name, "User");
    assert_eq!(user_symbol.kind, "struct");

    let user_occurrences = graph.precise_occurrences_for_symbol("repo-001", "scip-rust pkg a#User");
    assert_eq!(
        user_occurrences
            .iter()
            .map(|occurrence| {
                (
                    occurrence.path.clone(),
                    occurrence.range.start_line,
                    occurrence.range.start_column,
                    occurrence.symbol_roles,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("src/a.rs".to_string(), 1, 8, 1),
            ("src/a.rs".to_string(), 2, 19, 8),
        ]
    );

    let user_references = graph.precise_references_for_symbol("repo-001", "scip-rust pkg a#User");
    assert_eq!(user_references.len(), 1);
    assert_eq!(user_references[0].range.start_line, 2);
    assert!(!user_references[0].is_definition());

    let relationships = graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#User");
    assert_eq!(
        relationships
            .iter()
            .map(|relationship| relationship.kind)
            .collect::<Vec<_>>(),
        vec![
            PreciseRelationshipKind::Reference,
            PreciseRelationshipKind::Implementation
        ]
    );
}

#[test]
fn scip_protobuf_ingest_maps_and_persists_normalized_records() {
    let mut graph = SymbolGraph::default();
    let mut index = ScipIndexProto::new();

    let mut user_doc = ScipDocumentProto::new();
    user_doc.relative_path = "src/a.rs".to_owned();
    let mut user_definition = ScipOccurrenceProto::new();
    user_definition.symbol = "scip-rust pkg a#User".to_owned();
    user_definition.range = vec![0, 7, 11];
    user_definition.symbol_roles = 1;
    user_doc.occurrences.push(user_definition);

    let mut user_reference = ScipOccurrenceProto::new();
    user_reference.symbol = "scip-rust pkg a#User".to_owned();
    user_reference.range = vec![1, 18, 22];
    user_reference.symbol_roles = 8;
    user_doc.occurrences.push(user_reference);

    let mut user_symbol = ScipSymbolInformationProto::new();
    user_symbol.symbol = "scip-rust pkg a#User".to_owned();
    user_symbol.display_name = "User".to_owned();
    user_symbol.kind = EnumOrUnknown::from_i32(7);

    let mut relationship_reference = ScipRelationshipProto::new();
    relationship_reference.symbol = "scip-rust pkg a#Entity".to_owned();
    relationship_reference.is_reference = true;
    user_symbol.relationships.push(relationship_reference);

    let mut relationship_implementation = ScipRelationshipProto::new();
    relationship_implementation.symbol = "scip-rust pkg a#Entity".to_owned();
    relationship_implementation.is_implementation = true;
    user_symbol.relationships.push(relationship_implementation);
    user_doc.symbols.push(user_symbol);

    let mut entity_doc = ScipDocumentProto::new();
    entity_doc.relative_path = "src/base.rs".to_owned();
    let mut entity_occurrence = ScipOccurrenceProto::new();
    entity_occurrence.symbol = "scip-rust pkg a#Entity".to_owned();
    entity_occurrence.range = vec![0, 11, 17];
    entity_occurrence.symbol_roles = 1;
    entity_doc.occurrences.push(entity_occurrence);

    let mut entity_symbol = ScipSymbolInformationProto::new();
    entity_symbol.symbol = "scip-rust pkg a#Entity".to_owned();
    entity_symbol.display_name = "Entity".to_owned();
    entity_symbol.kind = EnumOrUnknown::from_i32(7);
    entity_doc.symbols.push(entity_symbol);

    index.documents.push(user_doc);
    index.documents.push(entity_doc);

    let payload = index
        .write_to_bytes()
        .expect("protobuf fixture payload should serialize");

    let summary = graph
        .ingest_scip_protobuf("repo-001", "fixture:scip.scip", &payload)
        .expect("valid protobuf scip payload should ingest successfully");
    assert_eq!(summary.documents_ingested, 2);
    assert_eq!(summary.symbols_upserted, 2);
    assert_eq!(summary.occurrences_upserted, 3);
    assert_eq!(summary.relationships_upserted, 2);

    let counts = graph.precise_counts();
    assert_eq!(counts.symbols, 2);
    assert_eq!(counts.occurrences, 3);
    assert_eq!(counts.relationships, 2);

    let relationships = graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#User");
    assert_eq!(
        relationships
            .iter()
            .map(|relationship| relationship.kind)
            .collect::<Vec<_>>(),
        vec![
            PreciseRelationshipKind::Reference,
            PreciseRelationshipKind::Implementation
        ]
    );
}

#[test]
fn precise_navigation_symbol_selection_is_deterministic() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [],
              "symbols": [
                { "symbol": "scip-rust pkg a#User", "display_name": "User", "kind": "struct", "relationships": [] },
                { "symbol": "scip-rust pkg a#user_lower", "display_name": "user", "kind": "struct", "relationships": [] }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:precise-navigation.json", payload)
        .expect("fixture payload should ingest");

    let exact = graph
        .select_precise_symbol_for_navigation("repo-001", "scip-rust pkg a#User", "fallback")
        .expect("exact symbol query should resolve");
    assert_eq!(exact.symbol, "scip-rust pkg a#User");

    let case_insensitive = graph
        .select_precise_symbol_for_navigation("repo-001", "USER", "fallback")
        .expect("case-insensitive display-name query should resolve");
    assert_eq!(case_insensitive.symbol, "scip-rust pkg a#User");

    let fallback = graph
        .select_precise_symbol_for_navigation("repo-001", "missing", "user")
        .expect("fallback display-name query should resolve");
    assert_eq!(fallback.symbol, "scip-rust pkg a#user_lower");

    assert!(
        graph
            .select_precise_symbol_for_navigation("repo-001", "missing", "also-missing")
            .is_none(),
        "missing query and fallback should return None"
    );
}

#[test]
fn precise_navigation_symbol_selection_matches_symbol_tail_when_display_name_is_missing() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [0, 6, 23],
                  "symbol_roles": 1
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "display_name": "",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:precise-navigation-tail.json", payload)
        .expect("fixture payload should ingest");

    let matched = graph
        .select_precise_symbol_for_navigation("repo-001", "requireServerUser", "fallback")
        .expect("symbol tail should resolve when display_name is missing");
    assert_eq!(
        matched.symbol,
        "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser."
    );
}

#[test]
fn precise_navigation_location_selection_prefers_containing_occurrence() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#Entity", "range": [0, 13, 19], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#User", "range": [1, 18, 22], "symbol_roles": 8 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg a#User", "display_name": "User", "kind": "struct", "relationships": [] },
                { "symbol": "scip-rust pkg a#Entity", "display_name": "Entity", "kind": "struct", "relationships": [] }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:precise-location.json", payload)
        .expect("fixture payload should ingest");

    let containing = graph
        .select_precise_symbol_for_location("repo-001", "src/a.rs", 1, Some(9))
        .expect("containing occurrence should resolve");
    assert_eq!(containing.symbol, "scip-rust pkg a#User");

    let later = graph
        .select_precise_symbol_for_location("repo-001", "src/a.rs", 1, Some(15))
        .expect("later containing occurrence should resolve");
    assert_eq!(later.symbol, "scip-rust pkg a#Entity");

    let reference = graph
        .select_precise_symbol_for_location("repo-001", "src/a.rs", 2, Some(20))
        .expect("reference occurrence should resolve");
    assert_eq!(reference.symbol, "scip-rust pkg a#User");
}

#[test]
fn scip_ingest_returns_typed_invalid_input_and_preserves_state() {
    let mut graph = SymbolGraph::default();
    let valid = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg a#User", "display_name": "User", "kind": "struct", "relationships": [] }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:valid.json", valid)
        .expect("valid ingest should succeed");
    let before = graph.precise_counts();

    let invalid = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7], "symbol_roles": 8 }
              ],
              "symbols": []
            }
          ]
        }"#;
    let error = graph
        .ingest_scip_json("repo-001", "fixture:invalid-range.json", invalid)
        .expect_err("invalid range payload should fail with typed invalid-input error");
    assert_eq!(
            error,
            ScipIngestError::InvalidInput {
                diagnostic: ScipInvalidInputDiagnostic {
                    artifact_label: "fixture:invalid-range.json".to_string(),
                    code: ScipInvalidInputCode::InvalidRange,
                    message: "occurrence range for symbol 'scip-rust pkg a#User' in 'src/a.rs' must have 3 or 4 numbers".to_string(),
                    line: None,
                    column: None,
                }
            }
        );

    let after = graph.precise_counts();
    assert_eq!(
        before, after,
        "failed ingest must not mutate precise graph state"
    );
    assert_eq!(
        graph
            .precise_occurrences_for_symbol("repo-001", "scip-rust pkg a#User")
            .len(),
        1
    );
}

#[test]
fn scip_ingest_rejects_payload_budget_overflow_with_typed_error() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": [],
          "padding": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
        }"#;

    let error = graph
        .ingest_scip_json_with_budgets(
            "repo-001",
            "fixture:payload-budget.json",
            payload,
            ScipResourceBudgets {
                max_payload_bytes: 16,
                max_documents: usize::MAX,
                max_elapsed_ms: u64::MAX,
            },
        )
        .expect_err("oversized payload should fail with typed resource-budget error");
    assert_eq!(
        error,
        ScipIngestError::ResourceBudgetExceeded {
            diagnostic: ScipResourceBudgetDiagnostic {
                artifact_label: "fixture:payload-budget.json".to_string(),
                code: ScipResourceBudgetCode::PayloadBytes,
                message: "scip payload bytes exceed configured budget".to_string(),
                limit: 16,
                actual: u64::try_from(payload.len()).unwrap_or(u64::MAX),
            },
        }
    );
    assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
}

#[test]
fn scip_ingest_rejects_document_budget_overflow_with_typed_error() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": [
            { "relative_path": "src/a.rs", "occurrences": [], "symbols": [] },
            { "relative_path": "src/b.rs", "occurrences": [], "symbols": [] }
          ]
        }"#;

    let error = graph
        .ingest_scip_json_with_budgets(
            "repo-001",
            "fixture:document-budget.json",
            payload,
            ScipResourceBudgets {
                max_payload_bytes: usize::MAX,
                max_documents: 1,
                max_elapsed_ms: u64::MAX,
            },
        )
        .expect_err("document overflow should fail with typed resource-budget error");
    assert_eq!(
        error,
        ScipIngestError::ResourceBudgetExceeded {
            diagnostic: ScipResourceBudgetDiagnostic {
                artifact_label: "fixture:document-budget.json".to_string(),
                code: ScipResourceBudgetCode::Documents,
                message: "scip document count exceeds configured budget".to_string(),
                limit: 1,
                actual: 2,
            },
        }
    );
    assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
}

#[test]
fn scip_ingest_rejects_zero_elapsed_budget_with_typed_error() {
    let mut graph = SymbolGraph::default();
    let payload = br#"{
          "documents": []
        }"#;

    let error = graph
        .ingest_scip_json_with_budgets(
            "repo-001",
            "fixture:elapsed-budget.json",
            payload,
            ScipResourceBudgets {
                max_payload_bytes: usize::MAX,
                max_documents: usize::MAX,
                max_elapsed_ms: 0,
            },
        )
        .expect_err("zero elapsed budget should fail deterministically");
    assert_eq!(
        error,
        ScipIngestError::ResourceBudgetExceeded {
            diagnostic: ScipResourceBudgetDiagnostic {
                artifact_label: "fixture:elapsed-budget.json".to_string(),
                code: ScipResourceBudgetCode::ElapsedMs,
                message: "scip ingest elapsed time budget is zero".to_string(),
                limit: 0,
                actual: 0,
            },
        }
    );
    assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
}

#[test]
fn scip_ingest_replaces_file_level_precise_occurrences() {
    let mut graph = SymbolGraph::default();
    let first = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#User", "range": [1, 7, 11], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Entity", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:first.json", first)
        .expect("first ingest should succeed");

    let second = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [2, 7, 11], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:second.json", second)
        .expect("second ingest should succeed");

    let file_occurrences = graph.precise_occurrences_for_file("repo-001", "src/a.rs");
    assert_eq!(file_occurrences.len(), 1);
    assert_eq!(file_occurrences[0].range.start_line, 3);

    let relationships = graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#User");
    assert!(
        relationships.is_empty(),
        "file-level reingest should replace prior relationships"
    );
}

#[test]
fn scip_incremental_update_replaces_only_target_file_and_preserves_unaffected_data() {
    let mut graph = SymbolGraph::default();
    let initial = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Base", "is_reference": true }
                  ]
                }
              ]
            },
            {
              "relative_path": "src/b.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg b#Service", "range": [0, 7, 14], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg b#Service",
                  "display_name": "Service",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Base", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#;

    graph
        .ingest_scip_json("repo-001", "fixture:initial.json", initial)
        .expect("initial payload should ingest");

    let before_b_occurrences = graph.precise_occurrences_for_file("repo-001", "src/b.rs");
    let before_b_relationships =
        graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg b#Service");
    assert_eq!(before_b_occurrences.len(), 1);
    assert_eq!(before_b_relationships.len(), 1);
    assert!(
        graph
            .precise_symbol("repo-001", "scip-rust pkg a#User")
            .is_some(),
        "expected initial symbol for src/a.rs"
    );

    let incremental = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#Account", "range": [2, 7, 14], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#Account",
                  "display_name": "Account",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;

    graph
        .ingest_scip_json("repo-001", "fixture:incremental.json", incremental)
        .expect("incremental payload should ingest");

    // Updated file replaced.
    assert!(
        graph
            .precise_symbol("repo-001", "scip-rust pkg a#User")
            .is_none(),
        "old symbol from updated file should be removed"
    );
    assert!(
        graph
            .precise_symbol("repo-001", "scip-rust pkg a#Account")
            .is_some(),
        "new symbol from updated file should be present"
    );
    let a_occurrences = graph.precise_occurrences_for_file("repo-001", "src/a.rs");
    assert_eq!(a_occurrences.len(), 1);
    assert_eq!(a_occurrences[0].symbol, "scip-rust pkg a#Account");
    assert!(
        graph
            .precise_relationships_from_symbol("repo-001", "scip-rust pkg a#Account")
            .is_empty(),
        "updated file relationships should reflect replacement payload"
    );

    // Unaffected file preserved exactly.
    assert_eq!(
        graph.precise_occurrences_for_file("repo-001", "src/b.rs"),
        before_b_occurrences
    );
    assert_eq!(
        graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg b#Service"),
        before_b_relationships
    );
}

#[test]
fn scip_incremental_update_is_deterministic_across_repeated_reingest() {
    let mut graph = SymbolGraph::default();
    let seed = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "src/b.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg b#Service", "range": [0, 7, 14], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg b#Service",
                  "display_name": "Service",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:seed.json", seed)
        .expect("seed ingest should succeed");

    let incremental = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#Account", "range": [2, 7, 14], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#Account", "range": [3, 10, 17], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#Account",
                  "display_name": "Account",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg b#Service", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#;
    graph
        .ingest_scip_json("repo-001", "fixture:inc-1.json", incremental)
        .expect("first incremental ingest should succeed");

    let counts_after_first = graph.precise_counts();
    let file_a_after_first = graph.precise_occurrences_for_file("repo-001", "src/a.rs");
    let refs_after_first =
        graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#Account");

    graph
        .ingest_scip_json("repo-001", "fixture:inc-2.json", incremental)
        .expect("second incremental ingest should succeed");

    assert_eq!(graph.precise_counts(), counts_after_first);
    assert_eq!(
        graph.precise_occurrences_for_file("repo-001", "src/a.rs"),
        file_a_after_first
    );
    assert_eq!(
        graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#Account"),
        refs_after_first
    );
    assert!(
        graph
            .precise_symbol("repo-001", "scip-rust pkg a#User")
            .is_none(),
        "stale symbols must not reappear across repeated incremental updates"
    );
}

#[test]
fn scip_overlay_ingest_preserves_overlapping_same_file_precise_data() {
    let mut graph = SymbolGraph::default();
    let canary = br#"{
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
        }"#;
    graph
        .overlay_scip_json_with_budgets(
            "repo-001",
            "fixture:canary.json",
            canary,
            ScipResourceBudgets::default(),
        )
        .expect("canary overlay should ingest");

    let main = br#"{
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
        }"#;
    graph
        .overlay_scip_json_with_budgets(
            "repo-001",
            "fixture:main.json",
            main,
            ScipResourceBudgets::default(),
        )
        .expect("main overlay should ingest");

    let matched = graph.matching_precise_symbols_for_navigation("repo-001", "Service", "Service");
    assert_eq!(matched.len(), 2);
    assert_eq!(
        matched[0].symbol,
        "rust-analyzer cargo repo 0.1.0 svc/Service#"
    );
    assert_eq!(matched[1].symbol, "scip-rust pkg repo#Service");
    assert!(
        graph
            .precise_symbol("repo-001", "scip-rust pkg repo#Service")
            .is_some(),
        "overlay ingest should preserve earlier same-file symbol namespace"
    );
    assert_eq!(
        graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg repo#Impl"),
        vec![PreciseRelationshipRecord {
            repository_id: "repo-001".to_owned(),
            from_symbol: "scip-rust pkg repo#Impl".to_owned(),
            to_symbol: "scip-rust pkg repo#Service".to_owned(),
            kind: PreciseRelationshipKind::Implementation,
        }]
    );
}

#[test]
fn scip_fixture_matrix_definitions_and_references() {
    let mut graph = SymbolGraph::default();
    let payload = load_scip_fixture("matrix-definitions-references.json");

    let summary = graph
        .ingest_scip_json(
            "repo-001",
            "fixture:matrix-definitions-references.json",
            &payload,
        )
        .expect("fixture payload should ingest");
    assert_eq!(summary.documents_ingested, 2);
    assert_eq!(summary.symbols_upserted, 1);
    assert_eq!(summary.occurrences_upserted, 2);
    assert_eq!(summary.relationships_upserted, 0);

    let occurrences =
        graph.precise_occurrences_for_symbol("repo-001", "scip-rust pkg matrix#Thing");
    assert_eq!(occurrences.len(), 2);
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| (occurrence.path.clone(), occurrence.range.start_line))
            .collect::<Vec<_>>(),
        vec![
            ("src/defs.rs".to_string(), 1),
            ("src/use.rs".to_string(), 3)
        ]
    );
    assert_eq!(
        graph
            .precise_references_for_symbol("repo-001", "scip-rust pkg matrix#Thing")
            .len(),
        1
    );
}

#[test]
fn scip_fixture_matrix_relationship_expansion_and_dedup() {
    let mut graph = SymbolGraph::default();
    let payload = load_scip_fixture("matrix-relationships.json");

    graph
        .ingest_scip_json("repo-001", "fixture:matrix-relationships.json", &payload)
        .expect("fixture payload should ingest");

    let relationships =
        graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg matrix#Thing");
    assert_eq!(relationships.len(), 4);
    assert_eq!(
        relationships
            .iter()
            .map(|relationship| relationship.kind)
            .collect::<Vec<_>>(),
        vec![
            PreciseRelationshipKind::Definition,
            PreciseRelationshipKind::Reference,
            PreciseRelationshipKind::Implementation,
            PreciseRelationshipKind::TypeDefinition,
        ]
    );
}

#[test]
fn scip_fixture_matrix_role_bits_classification_edges() {
    let mut graph = SymbolGraph::default();
    let payload = load_scip_fixture("matrix-role-bits.json");

    graph
        .ingest_scip_json("repo-001", "fixture:matrix-role-bits.json", &payload)
        .expect("fixture payload should ingest");

    let occurrences =
        graph.precise_occurrences_for_symbol("repo-001", "scip-rust pkg matrix#Roleful");
    assert_eq!(occurrences.len(), 5);
    let definition_count = occurrences
        .iter()
        .filter(|occurrence| occurrence.is_definition())
        .count();
    assert_eq!(
        definition_count, 2,
        "roles 1 and 9 should be classified as definitions"
    );

    let references =
        graph.precise_references_for_symbol("repo-001", "scip-rust pkg matrix#Roleful");
    assert_eq!(references.len(), 3);
    assert_eq!(
        references
            .iter()
            .map(|occurrence| (occurrence.range.start_line, occurrence.symbol_roles))
            .collect::<Vec<_>>(),
        vec![(3, 2), (4, 4), (5, 0)]
    );
}

#[test]
fn scip_fixture_matrix_invalid_range_returns_typed_diagnostic() {
    let mut graph = SymbolGraph::default();
    let payload = load_scip_fixture("matrix-invalid-range.json");

    let error = graph
        .ingest_scip_json("repo-001", "fixture:matrix-invalid-range.json", &payload)
        .expect_err("invalid fixture should return typed invalid-input error");
    assert_eq!(
            error,
            ScipIngestError::InvalidInput {
                diagnostic: ScipInvalidInputDiagnostic {
                    artifact_label: "fixture:matrix-invalid-range.json".to_string(),
                    code: ScipInvalidInputCode::InvalidRange,
                    message:
                        "occurrence range for symbol 'scip-rust pkg matrix#Broken' in 'src/invalid.rs' must have 3 or 4 numbers"
                            .to_string(),
                    line: None,
                    column: None,
                },
            }
        );
    assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
}

fn load_scip_fixture(file_name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scip")
        .join(file_name);
    fs::read(&path).expect("SCIP fixture must exist")
}
