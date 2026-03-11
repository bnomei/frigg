use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use frigg::graph::{
    PreciseGraphCounts, RelationKind, ScipIngestError, ScipInvalidInputCode, SymbolGraph,
    SymbolNode,
};
use protobuf::{EnumOrUnknown, Message};
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    Relationship as ScipRelationshipProto, SymbolInformation as ScipSymbolInformationProto,
};
use serde_json::json;

const BENCH_REPOSITORY_ID: &str = "repo-001";
const BENCH_ROOT_SYMBOL_ID: &str = "sym-root-hotpath";
const BENCH_TARGET_SYMBOL_ID: &str = "sym-target-hotspot";
const BENCH_PRECISE_SYMBOL_ID: &str = "scip-rust pkg bench#HotType";
const BENCH_NAVIGATION_FILE_PATH: &str = "src/navigation_hotspot.rs";
const BENCH_NAVIGATION_TARGET_INDEX: usize = 153;
const BENCH_NAVIGATION_TARGET_SYMBOL_ID: &str = "scip-rust pkg bench#ServiceHot153";
const BENCH_NAVIGATION_TARGET_DISPLAY_NAME: &str = "ServiceHot153";
const BENCH_NAVIGATION_TARGET_LINE: usize = BENCH_NAVIGATION_TARGET_INDEX * 2 + 1;
const BENCH_OUTPUT_ROOT: &str = "target/criterion";

const WORKLOAD_RELATION_TRAVERSAL: &str = "graph_hot_path_latency/relation_traversal/hot-fanout";
const WORKLOAD_PRECISE_REFERENCES: &str =
    "graph_hot_path_latency/precise_references/hot-symbol-contention";
const WORKLOAD_PRECISE_NAVIGATION: &str =
    "graph_hot_path_latency/precise_navigation/location-aware-selection";
const WORKLOAD_SCIP_INGEST_COLD_CACHE: &str = "graph_hot_path_latency/scip_ingest/cold-cache";
const WORKLOAD_SCIP_INGEST_PROTOBUF_COLD_CACHE: &str =
    "graph_hot_path_latency/scip_ingest_protobuf/cold-cache";

const BENCH_RELATION_SOURCES: usize = 192;
const BENCH_NAVIGATION_SYMBOLS: usize = 256;
const BENCH_SCIP_DOCUMENTS: usize = 48;
const BENCH_SCIP_OCCURRENCES_PER_DOCUMENT: usize = 6;
const BENCH_SCIP_PROTOBUF_RELATIONSHIPS_PER_DOCUMENT: usize = 2;
const BENCH_PRECISE_REFERENCE_DOCUMENTS: usize = 96;
const BENCH_SAMPLE_COUNT: usize = 30;
const BENCH_WARMUP_SAMPLES: usize = 5;

static RELATION_GRAPH: OnceLock<SymbolGraph> = OnceLock::new();
static PRECISE_REFERENCE_GRAPH: OnceLock<SymbolGraph> = OnceLock::new();
static PRECISE_NAVIGATION_GRAPH: OnceLock<SymbolGraph> = OnceLock::new();
static SCIP_COLD_CACHE_PAYLOAD: OnceLock<Vec<u8>> = OnceLock::new();
static SCIP_COLD_CACHE_PROTOBUF_PAYLOAD: OnceLock<Vec<u8>> = OnceLock::new();

fn main() {
    graph_hot_path_benchmarks();
}

fn graph_hot_path_benchmarks() {
    let relation_graph = RELATION_GRAPH.get_or_init(build_relation_graph_fixture);
    let precise_reference_graph =
        PRECISE_REFERENCE_GRAPH.get_or_init(build_precise_reference_graph_fixture);
    let precise_navigation_graph =
        PRECISE_NAVIGATION_GRAPH.get_or_init(build_precise_navigation_graph_fixture);
    let cold_cache_payload = SCIP_COLD_CACHE_PAYLOAD.get_or_init(build_cold_cache_scip_payload);
    let cold_cache_protobuf_payload =
        SCIP_COLD_CACHE_PROTOBUF_PAYLOAD.get_or_init(build_cold_cache_scip_protobuf_payload);

    assert_relation_traversal_is_deterministic(relation_graph);
    assert_precise_reference_query_is_deterministic(precise_reference_graph);
    assert_precise_navigation_queries_are_deterministic(precise_navigation_graph);
    assert_typed_invalid_input_is_preserved();

    run_workload(WORKLOAD_RELATION_TRAVERSAL, BENCH_SAMPLE_COUNT, 4, || {
        let outgoing = relation_graph.outgoing_relations(BENCH_ROOT_SYMBOL_ID);
        let incoming = relation_graph.incoming_relations(BENCH_TARGET_SYMBOL_ID);
        let hints = relation_graph.heuristic_relation_hints_for_target(BENCH_TARGET_SYMBOL_ID);
        std::hint::black_box((outgoing, incoming, hints));
    });

    run_workload(WORKLOAD_PRECISE_REFERENCES, BENCH_SAMPLE_COUNT, 6, || {
        let references = precise_reference_graph
            .precise_references_for_symbol(BENCH_REPOSITORY_ID, BENCH_PRECISE_SYMBOL_ID);
        std::hint::black_box(references);
    });

    run_workload(WORKLOAD_PRECISE_NAVIGATION, BENCH_SAMPLE_COUNT, 6, || {
        let by_location = precise_navigation_graph
            .select_precise_symbol_for_location(
                BENCH_REPOSITORY_ID,
                BENCH_NAVIGATION_FILE_PATH,
                BENCH_NAVIGATION_TARGET_LINE,
                Some(10),
            )
            .expect("precise navigation location-aware benchmark should resolve a target symbol");
        let by_navigation = precise_navigation_graph
            .select_precise_symbol_for_navigation(
                BENCH_REPOSITORY_ID,
                BENCH_NAVIGATION_TARGET_SYMBOL_ID,
                BENCH_NAVIGATION_TARGET_DISPLAY_NAME,
            )
            .expect("precise navigation symbol benchmark should resolve a target symbol");
        std::hint::black_box((by_location, by_navigation));
    });

    run_workload(
        WORKLOAD_SCIP_INGEST_COLD_CACHE,
        BENCH_SAMPLE_COUNT,
        1,
        || {
            let mut graph = SymbolGraph::default();
            let summary = graph
                .ingest_scip_json(
                    BENCH_REPOSITORY_ID,
                    "bench:scip-cold-cache",
                    cold_cache_payload,
                )
                .expect("cold-cache SCIP ingest benchmark should succeed");
            assert_eq!(
                summary.documents_ingested, BENCH_SCIP_DOCUMENTS,
                "cold-cache fixture must ingest every deterministic document"
            );
            assert_eq!(
                summary.occurrences_upserted,
                BENCH_SCIP_DOCUMENTS * BENCH_SCIP_OCCURRENCES_PER_DOCUMENT,
                "cold-cache fixture must upsert all deterministic occurrences"
            );
            std::hint::black_box((summary, graph.precise_counts()));
        },
    );

    run_workload(
        WORKLOAD_SCIP_INGEST_PROTOBUF_COLD_CACHE,
        BENCH_SAMPLE_COUNT,
        1,
        || {
            let mut graph = SymbolGraph::default();
            let summary = graph
                .ingest_scip_protobuf(
                    BENCH_REPOSITORY_ID,
                    "bench:scip-protobuf-cold-cache",
                    cold_cache_protobuf_payload,
                )
                .expect("cold-cache protobuf SCIP ingest benchmark should succeed");
            assert_eq!(
                summary.documents_ingested, BENCH_SCIP_DOCUMENTS,
                "cold-cache protobuf fixture must ingest every deterministic document"
            );
            assert_eq!(
                summary.occurrences_upserted,
                BENCH_SCIP_DOCUMENTS * BENCH_SCIP_OCCURRENCES_PER_DOCUMENT,
                "cold-cache protobuf fixture must upsert all deterministic occurrences"
            );
            assert_eq!(
                summary.relationships_upserted,
                BENCH_SCIP_DOCUMENTS * BENCH_SCIP_PROTOBUF_RELATIONSHIPS_PER_DOCUMENT,
                "cold-cache protobuf fixture must map the deterministic relationship fanout"
            );
            std::hint::black_box((summary, graph.precise_counts()));
        },
    );
}

fn run_workload<F>(workload_id: &str, sample_count: usize, iters_per_sample: u64, mut workload: F)
where
    F: FnMut(),
{
    assert!(sample_count > 0, "sample_count must be greater than zero");
    assert!(
        iters_per_sample > 0,
        "iters_per_sample must be greater than zero"
    );

    for _ in 0..BENCH_WARMUP_SAMPLES {
        for _ in 0..iters_per_sample {
            workload();
        }
    }

    let mut iters = Vec::with_capacity(sample_count);
    let mut times = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let start = Instant::now();
        for _ in 0..iters_per_sample {
            workload();
        }
        let elapsed_ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        iters.push(iters_per_sample);
        times.push(elapsed_ns);
    }

    write_sample_file(workload_id, &iters, &times);
    println!("workload={workload_id} samples={sample_count} iters_per_sample={iters_per_sample}");
}

fn write_sample_file(workload_id: &str, iters: &[u64], times: &[u64]) {
    let sample_path = PathBuf::from(BENCH_OUTPUT_ROOT)
        .join(workload_id)
        .join("new")
        .join("sample.json");
    if let Some(parent) = sample_path.parent() {
        fs::create_dir_all(parent).expect("benchmark sample directory should be creatable");
    }

    let payload = json!({
        "iters": iters,
        "times": times,
    });
    let raw = serde_json::to_vec(&payload).expect("benchmark sample payload should serialize");
    fs::write(&sample_path, raw).expect("benchmark sample.json should be writable");
}

fn build_relation_graph_fixture() -> SymbolGraph {
    let mut graph = SymbolGraph::default();
    let mut symbols = Vec::with_capacity(BENCH_RELATION_SOURCES + 2);
    symbols.push(SymbolNode::new(
        BENCH_ROOT_SYMBOL_ID,
        BENCH_REPOSITORY_ID,
        "RootService",
        "module",
        "src/root.rs",
        1,
    ));
    symbols.push(SymbolNode::new(
        BENCH_TARGET_SYMBOL_ID,
        BENCH_REPOSITORY_ID,
        "HotspotType",
        "struct",
        "src/hotspot.rs",
        2,
    ));
    for source_idx in 0..BENCH_RELATION_SOURCES {
        symbols.push(SymbolNode::new(
            format!("sym-source-{source_idx:03}"),
            BENCH_REPOSITORY_ID,
            format!("Source{source_idx:03}"),
            "function",
            format!("src/module_{:03}.rs", source_idx % 24),
            source_idx + 3,
        ));
    }
    graph.register_symbols(symbols);

    for source_idx in 0..BENCH_RELATION_SOURCES {
        let source_symbol_id = format!("sym-source-{source_idx:03}");
        graph
            .add_relation(
                BENCH_ROOT_SYMBOL_ID,
                &source_symbol_id,
                RelationKind::Contains,
            )
            .expect("fixture root relation should insert");
        graph
            .add_relation(
                &source_symbol_id,
                BENCH_TARGET_SYMBOL_ID,
                relation_kind_for_source(source_idx),
            )
            .expect("fixture hotspot relation should insert");
    }

    graph
}

fn relation_kind_for_source(source_idx: usize) -> RelationKind {
    match source_idx % 5 {
        0 => RelationKind::Calls,
        1 => RelationKind::RefersTo,
        2 => RelationKind::Implements,
        3 => RelationKind::Extends,
        _ => RelationKind::DefinedIn,
    }
}

fn build_precise_reference_graph_fixture() -> SymbolGraph {
    let mut graph = SymbolGraph::default();
    let payload = build_precise_reference_payload();
    graph
        .ingest_scip_json(
            BENCH_REPOSITORY_ID,
            "bench:precise-reference-contention",
            &payload,
        )
        .expect("precise reference contention fixture should ingest");
    graph
}

fn build_precise_navigation_graph_fixture() -> SymbolGraph {
    let mut graph = SymbolGraph::default();
    let payload = build_precise_navigation_payload();
    graph
        .ingest_scip_json(
            BENCH_REPOSITORY_ID,
            "bench:precise-navigation-location-aware",
            &payload,
        )
        .expect("precise navigation benchmark fixture should ingest");
    graph
}

fn build_precise_reference_payload() -> Vec<u8> {
    let mut documents = Vec::with_capacity(BENCH_PRECISE_REFERENCE_DOCUMENTS);
    for doc_idx in 0..BENCH_PRECISE_REFERENCE_DOCUMENTS {
        let mut occurrences = Vec::with_capacity(3);
        if doc_idx == 0 {
            occurrences.push(json!({
                "symbol": BENCH_PRECISE_SYMBOL_ID,
                "range": [0, 4, 11],
                "symbol_roles": 1
            }));
        }
        occurrences.push(json!({
            "symbol": BENCH_PRECISE_SYMBOL_ID,
            "range": [doc_idx + 1, 8, 19],
            "symbol_roles": 8
        }));
        occurrences.push(json!({
            "symbol": BENCH_PRECISE_SYMBOL_ID,
            "range": [doc_idx + 2, 10, 22],
            "symbol_roles": 8
        }));

        documents.push(json!({
            "relative_path": format!("src/contention_{doc_idx:03}.rs"),
            "occurrences": occurrences,
            "symbols": [{
                "symbol": BENCH_PRECISE_SYMBOL_ID,
                "display_name": "HotType",
                "kind": "struct",
                "relationships": []
            }]
        }));
    }

    serde_json::to_vec(&json!({ "documents": documents }))
        .expect("benchmark precise reference payload should serialize")
}

fn build_cold_cache_scip_payload() -> Vec<u8> {
    let mut documents = Vec::with_capacity(BENCH_SCIP_DOCUMENTS);
    for doc_idx in 0..BENCH_SCIP_DOCUMENTS {
        let symbol = format!("scip-rust pkg bench#Cold{doc_idx:03}");
        let mut occurrences = Vec::with_capacity(BENCH_SCIP_OCCURRENCES_PER_DOCUMENT);
        occurrences.push(json!({
            "symbol": symbol.clone(),
            "range": [0, 4, 12],
            "symbol_roles": 1
        }));
        for occurrence_idx in 1..BENCH_SCIP_OCCURRENCES_PER_DOCUMENT {
            occurrences.push(json!({
                "symbol": symbol.clone(),
                "range": [occurrence_idx, occurrence_idx + 4, occurrence_idx + 11],
                "symbol_roles": 8
            }));
        }

        documents.push(json!({
            "relative_path": format!("src/cold_{doc_idx:03}.rs"),
            "occurrences": occurrences,
            "symbols": [{
                "symbol": symbol.clone(),
                "display_name": format!("Cold{doc_idx:03}"),
                "kind": "struct",
                "relationships": []
            }]
        }));
    }

    serde_json::to_vec(&json!({ "documents": documents }))
        .expect("benchmark cold-cache payload should serialize")
}

fn build_cold_cache_scip_protobuf_payload() -> Vec<u8> {
    let mut index = ScipIndexProto::new();
    for doc_idx in 0..BENCH_SCIP_DOCUMENTS {
        let symbol = format!("scip-rust pkg bench#ColdProto{doc_idx:03}");
        let mut document = ScipDocumentProto::new();
        document.relative_path = format!("src/cold_proto_{doc_idx:03}.rs");

        let mut definition = ScipOccurrenceProto::new();
        definition.symbol = symbol.clone();
        definition.range = vec![0, 4, 12];
        definition.symbol_roles = 1;
        document.occurrences.push(definition);

        for occurrence_idx in 1..BENCH_SCIP_OCCURRENCES_PER_DOCUMENT {
            let mut reference = ScipOccurrenceProto::new();
            reference.symbol = symbol.clone();
            reference.range = vec![
                occurrence_idx as i32,
                (occurrence_idx + 4) as i32,
                (occurrence_idx + 11) as i32,
            ];
            reference.symbol_roles = 8;
            document.occurrences.push(reference);
        }

        let mut symbol_info = ScipSymbolInformationProto::new();
        symbol_info.symbol = symbol;
        symbol_info.display_name = format!("ColdProto{doc_idx:03}");
        symbol_info.kind = EnumOrUnknown::from_i32(7);

        let mut reference_relationship = ScipRelationshipProto::new();
        reference_relationship.symbol = "scip-rust pkg bench#SharedBase".to_owned();
        reference_relationship.is_reference = true;
        symbol_info.relationships.push(reference_relationship);

        let mut implementation_relationship = ScipRelationshipProto::new();
        implementation_relationship.symbol = "scip-rust pkg bench#SharedBase".to_owned();
        implementation_relationship.is_implementation = true;
        symbol_info.relationships.push(implementation_relationship);

        document.symbols.push(symbol_info);
        index.documents.push(document);
    }

    index
        .write_to_bytes()
        .expect("benchmark cold-cache protobuf payload should serialize")
}

fn build_precise_navigation_payload() -> Vec<u8> {
    let mut occurrences = Vec::with_capacity(BENCH_NAVIGATION_SYMBOLS * 2);
    let mut symbols = Vec::with_capacity(BENCH_NAVIGATION_SYMBOLS);

    for symbol_idx in 0..BENCH_NAVIGATION_SYMBOLS {
        let symbol = format!("scip-rust pkg bench#ServiceHot{symbol_idx:03}");
        let display_name = format!("ServiceHot{symbol_idx:03}");
        let definition_line = symbol_idx * 2;
        occurrences.push(json!({
            "symbol": symbol.clone(),
            "range": [definition_line, 4, 16],
            "symbol_roles": 1
        }));
        occurrences.push(json!({
            "symbol": symbol.clone(),
            "range": [definition_line, 24, 36],
            "symbol_roles": 8
        }));
        symbols.push(json!({
            "symbol": symbol,
            "display_name": display_name,
            "kind": "function",
            "relationships": []
        }));
    }

    serde_json::to_vec(&json!({
        "documents": [{
            "relative_path": BENCH_NAVIGATION_FILE_PATH,
            "occurrences": occurrences,
            "symbols": symbols
        }]
    }))
    .expect("benchmark precise navigation payload should serialize")
}

fn assert_relation_traversal_is_deterministic(graph: &SymbolGraph) {
    let first_outgoing = graph.outgoing_relations(BENCH_ROOT_SYMBOL_ID);
    let second_outgoing = graph.outgoing_relations(BENCH_ROOT_SYMBOL_ID);
    assert_eq!(
        first_outgoing, second_outgoing,
        "relation traversal output ordering must be deterministic"
    );
    assert_eq!(
        first_outgoing.len(),
        BENCH_RELATION_SOURCES,
        "relation traversal fixture should include a full fanout set"
    );

    let first_hints = graph.heuristic_relation_hints_for_target(BENCH_TARGET_SYMBOL_ID);
    let second_hints = graph.heuristic_relation_hints_for_target(BENCH_TARGET_SYMBOL_ID);
    assert_eq!(
        first_hints, second_hints,
        "heuristic relation hints must preserve deterministic ordering"
    );
    assert_eq!(
        first_hints.len(),
        BENCH_RELATION_SOURCES,
        "heuristic relation hints should include all contention sources"
    );
}

fn assert_precise_reference_query_is_deterministic(graph: &SymbolGraph) {
    let first = graph.precise_references_for_symbol(BENCH_REPOSITORY_ID, BENCH_PRECISE_SYMBOL_ID);
    let second = graph.precise_references_for_symbol(BENCH_REPOSITORY_ID, BENCH_PRECISE_SYMBOL_ID);
    assert_eq!(
        first, second,
        "precise reference query should preserve deterministic ordering for repeated input"
    );
    assert_eq!(
        first.len(),
        (BENCH_PRECISE_REFERENCE_DOCUMENTS * 2),
        "contention fixture should include two references per document"
    );
}

fn assert_precise_navigation_queries_are_deterministic(graph: &SymbolGraph) {
    let first_by_location = graph
        .select_precise_symbol_for_location(
            BENCH_REPOSITORY_ID,
            BENCH_NAVIGATION_FILE_PATH,
            BENCH_NAVIGATION_TARGET_LINE,
            Some(10),
        )
        .expect("precise navigation location-aware fixture should resolve a target symbol");
    let second_by_location = graph
        .select_precise_symbol_for_location(
            BENCH_REPOSITORY_ID,
            BENCH_NAVIGATION_FILE_PATH,
            BENCH_NAVIGATION_TARGET_LINE,
            Some(10),
        )
        .expect("precise navigation location-aware fixture should be deterministic");
    assert_eq!(
        first_by_location, second_by_location,
        "location-aware precise navigation should preserve deterministic ordering"
    );
    assert_eq!(
        first_by_location.symbol, BENCH_NAVIGATION_TARGET_SYMBOL_ID,
        "location-aware precise navigation should resolve the hotspot symbol"
    );

    let first_by_navigation = graph
        .select_precise_symbol_for_navigation(
            BENCH_REPOSITORY_ID,
            BENCH_NAVIGATION_TARGET_SYMBOL_ID,
            BENCH_NAVIGATION_TARGET_DISPLAY_NAME,
        )
        .expect("precise navigation symbol fixture should resolve a target symbol");
    let second_by_navigation = graph
        .select_precise_symbol_for_navigation(
            BENCH_REPOSITORY_ID,
            BENCH_NAVIGATION_TARGET_SYMBOL_ID,
            BENCH_NAVIGATION_TARGET_DISPLAY_NAME,
        )
        .expect("precise navigation symbol fixture should be deterministic");
    assert_eq!(
        first_by_navigation, second_by_navigation,
        "symbol-query precise navigation should preserve deterministic ordering"
    );
    assert_eq!(
        first_by_navigation.symbol, BENCH_NAVIGATION_TARGET_SYMBOL_ID,
        "symbol-query precise navigation should resolve the hotspot symbol"
    );
}

fn assert_typed_invalid_input_is_preserved() {
    let mut graph = SymbolGraph::default();
    let invalid_payload = br#"{
      "documents": [
        {
          "relative_path": "src/invalid.rs",
          "occurrences": [
            { "symbol": "scip-rust pkg bench#Broken", "range": [0, 4], "symbol_roles": 8 }
          ],
          "symbols": []
        }
      ]
    }"#;

    let error = graph
        .ingest_scip_json(BENCH_REPOSITORY_ID, "bench:invalid-range", invalid_payload)
        .expect_err("invalid SCIP payload should preserve typed invalid-input errors");
    assert!(
        matches!(
            error,
            ScipIngestError::InvalidInput { ref diagnostic }
                if diagnostic.code == ScipInvalidInputCode::InvalidRange
        ),
        "expected typed invalid-input error, got {error:?}"
    );
    assert_eq!(
        graph.precise_counts(),
        PreciseGraphCounts::default(),
        "failed ingest should not mutate graph state"
    );
}
