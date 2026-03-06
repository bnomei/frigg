use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    DeepSearchComposeCitationsParams, DeepSearchPlaybookContract, DeepSearchPlaybookStepContract,
    DeepSearchReplayParams, DeepSearchRunParams, DeepSearchTraceOutcomeContract,
    DocumentSymbolsParams, FindDeclarationsParams, FindImplementationsParams, FindReferencesParams,
    GoToDefinitionParams, IncomingCallsParams, ListRepositoriesParams, OutgoingCallsParams,
    ReadFileParams, SearchHybridParams, SearchPatternType, SearchStructuralParams,
    SearchSymbolParams, SearchTextParams,
};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use tokio::runtime::Runtime;

const BENCH_FILES: usize = 80;
const BENCH_LINES_PER_FILE: usize = 40;
const BENCH_REPOSITORY_ID: &str = "repo-001";
const BENCH_PRECISE_SYMBOL: &str = "Entity010";
const BENCH_PROVENANCE_WRITE_CALLS: usize = 16;
const BENCH_PROVENANCE_WORKLOAD_ID: &str = "read-file-repeated-16x";
const BENCH_PRECISE_SCIP_FIXTURE: &str = "mcp-bench-precise-references.json";
const BENCH_PRECISE_NAVIGATION_SCIP_FIXTURE: &str = "mcp-bench-precise-navigation.json";
const BENCH_NAVIGATION_SYMBOL: &str = "Service";
const BENCH_NAVIGATION_CALLER_SYMBOL: &str = "consumer";
const BENCH_HYBRID_QUERY: &str = "needle_hotspot";
const BENCH_HYBRID_LIMIT: usize = 20;
const BENCH_DEEP_SEARCH_PLAYBOOK_ID: &str = "deep-search-bench-basic-v1";
const BENCH_DEEP_SEARCH_VARIANT: &str = "basic-playbook";

static BENCH_ROOT: OnceLock<PathBuf> = OnceLock::new();
static BENCH_RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn tool_latency_benchmarks(c: &mut Criterion) {
    let root = BENCH_ROOT.get_or_init(prepare_bench_root);
    let runtime = BENCH_RUNTIME.get_or_init(|| {
        Runtime::new().expect("benchmark runtime should be constructible for mcp tool latency")
    });
    let config = FriggConfig::from_workspace_roots(vec![root.clone()])
        .expect("benchmark root should produce valid FriggConfig");
    let mut semantic_config = config.clone();
    semantic_config.semantic_runtime = semantic_runtime_enabled_non_strict();

    let server = FriggMcpServer::new(config);
    let semantic_server = FriggMcpServer::new(semantic_config);
    assert_precise_reference_workload(runtime, &server);
    assert_precise_navigation_workload(runtime, &server);
    assert_search_hybrid_semantic_toggle_off_workload(runtime, &server);
    assert_search_hybrid_semantic_degraded_workload(runtime, &semantic_server);
    let deep_search_playbook = build_deep_search_playbook();
    let deep_search_trace_artifact = runtime
        .block_on(server.deep_search_run(Parameters(DeepSearchRunParams {
            playbook: deep_search_playbook.clone(),
        })))
        .expect("deep_search benchmark probe should succeed")
        .0
        .trace_artifact;
    assert_eq!(
        deep_search_trace_artifact.step_count,
        deep_search_playbook.steps.len(),
        "deep_search benchmark probe must include one trace step per playbook step"
    );
    assert!(
        deep_search_trace_artifact
            .steps
            .iter()
            .all(|step| matches!(step.outcome, DeepSearchTraceOutcomeContract::Ok { .. })),
        "deep_search benchmark probe should produce successful deterministic trace steps"
    );
    let deep_search_citations_probe = runtime
        .block_on(server.deep_search_compose_citations(Parameters(
            DeepSearchComposeCitationsParams {
                trace_artifact: deep_search_trace_artifact.clone(),
                answer: None,
            },
        )))
        .expect("deep_search compose_citations benchmark probe should succeed")
        .0;
    assert!(
        !deep_search_citations_probe
            .citation_payload
            .citations
            .is_empty(),
        "deep_search compose_citations benchmark probe should emit citations"
    );

    let mut group = c.benchmark_group("mcp_tool_latency");
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("list_repositories", "default"), |b| {
        b.iter(|| {
            let response = runtime
                .block_on(server.list_repositories(Parameters(ListRepositoriesParams {})))
                .expect("list_repositories benchmark should succeed");
            criterion::black_box(response);
        });
    });

    group.bench_function(BenchmarkId::new("read_file", "single-rust-file"), |b| {
        b.iter(|| {
            let params = Parameters(ReadFileParams {
                path: "src/file_001.rs".to_owned(),
                repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                max_bytes: None,
                line_start: None,
                line_end: None,
            });
            let response = runtime
                .block_on(server.read_file(params))
                .expect("read_file benchmark should succeed");
            criterion::black_box(response);
        });
    });

    group.bench_function(BenchmarkId::new("search_text", "literal-scoped"), |b| {
        b.iter(|| {
            let params = Parameters(SearchTextParams {
                query: "needle".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                path_regex: Some(r"^src/.*\.rs$".to_owned()),
                limit: Some(200),
            });
            let response = runtime
                .block_on(server.search_text(params))
                .expect("search_text benchmark should succeed");
            criterion::black_box(response);
        });
    });

    group.bench_function(BenchmarkId::new("search_symbol", "tree-sitter"), |b| {
        b.iter(|| {
            let params = Parameters(SearchSymbolParams {
                query: "Entity001".to_owned(),
                repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                limit: Some(200),
            });
            let response = runtime
                .block_on(server.search_symbol(params))
                .expect("search_symbol benchmark should succeed");
            criterion::black_box(response);
        });
    });

    group.bench_function(BenchmarkId::new("find_references", "heuristic"), |b| {
        b.iter(|| {
            let params = Parameters(FindReferencesParams {
                symbol: "Entity001".to_owned(),
                repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                limit: Some(200),
            });
            let response = runtime
                .block_on(server.find_references(params))
                .expect("find_references benchmark should succeed");
            criterion::black_box(response);
        });
    });

    group.bench_function(BenchmarkId::new("find_references", "precise"), |b| {
        b.iter(|| {
            let params = Parameters(FindReferencesParams {
                symbol: BENCH_PRECISE_SYMBOL.to_owned(),
                repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                limit: Some(200),
            });
            let response = runtime
                .block_on(server.find_references(params))
                .expect("find_references precise benchmark should succeed");
            criterion::black_box(response);
        });
    });

    group.bench_function(
        BenchmarkId::new("go_to_definition", "precise-symbol"),
        |b| {
            b.iter(|| {
                let params = Parameters(GoToDefinitionParams {
                    symbol: Some(BENCH_PRECISE_SYMBOL.to_owned()),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    path: None,
                    line: None,
                    column: None,
                    limit: Some(200),
                });
                let response = runtime
                    .block_on(server.go_to_definition(params))
                    .expect("go_to_definition benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("find_declarations", "precise-symbol"),
        |b| {
            b.iter(|| {
                let params = Parameters(FindDeclarationsParams {
                    symbol: Some(BENCH_PRECISE_SYMBOL.to_owned()),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    path: None,
                    line: None,
                    column: None,
                    limit: Some(200),
                });
                let response = runtime
                    .block_on(server.find_declarations(params))
                    .expect("find_declarations benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("find_implementations", "precise-relationships"),
        |b| {
            b.iter(|| {
                let params = Parameters(FindImplementationsParams {
                    symbol: Some(BENCH_NAVIGATION_SYMBOL.to_owned()),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    path: None,
                    line: None,
                    column: None,
                    limit: Some(200),
                });
                let response = runtime
                    .block_on(server.find_implementations(params))
                    .expect("find_implementations benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("incoming_calls", "precise-relationships"),
        |b| {
            b.iter(|| {
                let params = Parameters(IncomingCallsParams {
                    symbol: Some(BENCH_NAVIGATION_SYMBOL.to_owned()),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    path: None,
                    line: None,
                    column: None,
                    limit: Some(200),
                });
                let response = runtime
                    .block_on(server.incoming_calls(params))
                    .expect("incoming_calls benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("outgoing_calls", "precise-relationships"),
        |b| {
            b.iter(|| {
                let params = Parameters(OutgoingCallsParams {
                    symbol: Some(BENCH_NAVIGATION_CALLER_SYMBOL.to_owned()),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    path: None,
                    line: None,
                    column: None,
                    limit: Some(200),
                });
                let response = runtime
                    .block_on(server.outgoing_calls(params))
                    .expect("outgoing_calls benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("document_symbols", "single-rust-file"),
        |b| {
            b.iter(|| {
                let params = Parameters(DocumentSymbolsParams {
                    path: "src/file_010.rs".to_owned(),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                });
                let response = runtime
                    .block_on(server.document_symbols(params))
                    .expect("document_symbols benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("search_structural", "rust-function-scoped"),
        |b| {
            b.iter(|| {
                let params = Parameters(SearchStructuralParams {
                    query: "(function_item) @fn".to_owned(),
                    language: Some("rust".to_owned()),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    path_regex: Some(r"^src/file_010\.rs$".to_owned()),
                    limit: Some(200),
                });
                let response = runtime
                    .block_on(server.search_structural(params))
                    .expect("search_structural benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("search_hybrid", "semantic-toggle-off"),
        |b| {
            b.iter(|| {
                let params = Parameters(SearchHybridParams {
                    query: BENCH_HYBRID_QUERY.to_owned(),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    language: Some("rust".to_owned()),
                    limit: Some(BENCH_HYBRID_LIMIT),
                    weights: None,
                    semantic: Some(false),
                });
                let response = runtime
                    .block_on(server.search_hybrid(params))
                    .expect("search_hybrid semantic-toggle-off benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("search_hybrid", "semantic-degraded-missing-credentials"),
        |b| {
            b.iter(|| {
                let params = Parameters(SearchHybridParams {
                    query: BENCH_HYBRID_QUERY.to_owned(),
                    repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                    language: Some("rust".to_owned()),
                    limit: Some(BENCH_HYBRID_LIMIT),
                    weights: None,
                    semantic: Some(true),
                });
                let response = runtime
                    .block_on(semantic_server.search_hybrid(params))
                    .expect("search_hybrid semantic-degraded benchmark should succeed in non-strict mode");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("deep_search_run", BENCH_DEEP_SEARCH_VARIANT),
        |b| {
            b.iter(|| {
                let params = Parameters(DeepSearchRunParams {
                    playbook: deep_search_playbook.clone(),
                });
                let response = runtime
                    .block_on(server.deep_search_run(params))
                    .expect("deep_search_run benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("deep_search_replay", BENCH_DEEP_SEARCH_VARIANT),
        |b| {
            b.iter(|| {
                let params = Parameters(DeepSearchReplayParams {
                    playbook: deep_search_playbook.clone(),
                    expected_trace_artifact: deep_search_trace_artifact.clone(),
                });
                let response = runtime
                    .block_on(server.deep_search_replay(params))
                    .expect("deep_search_replay benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("deep_search_compose_citations", BENCH_DEEP_SEARCH_VARIANT),
        |b| {
            b.iter(|| {
                let params = Parameters(DeepSearchComposeCitationsParams {
                    trace_artifact: deep_search_trace_artifact.clone(),
                    answer: Some("benchmark deep-search answer".to_owned()),
                });
                let response = runtime
                    .block_on(server.deep_search_compose_citations(params))
                    .expect("deep_search_compose_citations benchmark should succeed");
                criterion::black_box(response);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("provenance_write_overhead", BENCH_PROVENANCE_WORKLOAD_ID),
        |b| {
            b.iter(|| {
                for _ in 0..BENCH_PROVENANCE_WRITE_CALLS {
                    let params = Parameters(ReadFileParams {
                        path: "src/file_001.rs".to_owned(),
                        repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                        max_bytes: None,
                        line_start: None,
                        line_end: None,
                    });
                    let response = runtime
                        .block_on(server.read_file(params))
                        .expect("provenance_write_overhead benchmark should succeed");
                    criterion::black_box(response);
                }
            });
        },
    );

    group.finish();
}

fn build_deep_search_playbook() -> DeepSearchPlaybookContract {
    DeepSearchPlaybookContract {
        playbook_id: BENCH_DEEP_SEARCH_PLAYBOOK_ID.to_owned(),
        steps: vec![
            DeepSearchPlaybookStepContract {
                step_id: "s01-list-repositories".to_owned(),
                tool_name: "list_repositories".to_owned(),
                params: json!({}),
            },
            DeepSearchPlaybookStepContract {
                step_id: "s02-search-text".to_owned(),
                tool_name: "search_text".to_owned(),
                params: json!({
                    "query": "create_001",
                    "pattern_type": "literal",
                    "repository_id": BENCH_REPOSITORY_ID,
                    "path_regex": "^src/file_001\\.rs$",
                    "limit": 20
                }),
            },
            DeepSearchPlaybookStepContract {
                step_id: "s03-read-file".to_owned(),
                tool_name: "read_file".to_owned(),
                params: json!({
                    "path": "src/file_001.rs",
                    "repository_id": BENCH_REPOSITORY_ID
                }),
            },
            DeepSearchPlaybookStepContract {
                step_id: "s04-search-symbol".to_owned(),
                tool_name: "search_symbol".to_owned(),
                params: json!({
                    "query": BENCH_PRECISE_SYMBOL,
                    "repository_id": BENCH_REPOSITORY_ID,
                    "limit": 20
                }),
            },
            DeepSearchPlaybookStepContract {
                step_id: "s05-find-references".to_owned(),
                tool_name: "find_references".to_owned(),
                params: json!({
                    "symbol": BENCH_PRECISE_SYMBOL,
                    "repository_id": BENCH_REPOSITORY_ID,
                    "limit": 20
                }),
            },
        ],
    }
}

fn assert_precise_reference_workload(runtime: &Runtime, server: &FriggMcpServer) {
    let params = Parameters(FindReferencesParams {
        symbol: BENCH_PRECISE_SYMBOL.to_owned(),
        repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
        limit: Some(200),
    });
    let response = runtime
        .block_on(server.find_references(params))
        .expect("precise benchmark fixture should support find_references")
        .0;
    assert!(
        !response.matches.is_empty(),
        "precise benchmark fixture should return at least one precise match"
    );
    let note = response.note.unwrap_or_default();
    assert!(
        note.contains("\"precision\":\"precise\""),
        "precise benchmark fixture should force precise resolution metadata, got note: {note}"
    );
}

fn assert_precise_navigation_workload(runtime: &Runtime, server: &FriggMcpServer) {
    let implementations = runtime
        .block_on(
            server.find_implementations(Parameters(FindImplementationsParams {
                symbol: Some(BENCH_NAVIGATION_SYMBOL.to_owned()),
                repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
                path: None,
                line: None,
                column: None,
                limit: Some(200),
            })),
        )
        .expect("precise benchmark fixture should support find_implementations")
        .0;
    assert!(
        !implementations.matches.is_empty(),
        "precise benchmark fixture should return implementation matches"
    );
    let incoming = runtime
        .block_on(server.incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some(BENCH_NAVIGATION_SYMBOL.to_owned()),
            repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(200),
        })))
        .expect("precise benchmark fixture should support incoming_calls")
        .0;
    assert!(
        !incoming.matches.is_empty(),
        "precise benchmark fixture should return incoming call matches"
    );
    let outgoing = runtime
        .block_on(server.outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some(BENCH_NAVIGATION_CALLER_SYMBOL.to_owned()),
            repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(200),
        })))
        .expect("precise benchmark fixture should support outgoing_calls")
        .0;
    assert!(
        !outgoing.matches.is_empty(),
        "precise benchmark fixture should return outgoing call matches"
    );
}

fn assert_search_hybrid_semantic_toggle_off_workload(runtime: &Runtime, server: &FriggMcpServer) {
    let first = runtime
        .block_on(server.search_hybrid(Parameters(SearchHybridParams {
            query: BENCH_HYBRID_QUERY.to_owned(),
            repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(BENCH_HYBRID_LIMIT),
            weights: None,
            semantic: Some(false),
        })))
        .expect("search_hybrid semantic-toggle-off benchmark probe should succeed")
        .0;
    let second = runtime
        .block_on(server.search_hybrid(Parameters(SearchHybridParams {
            query: BENCH_HYBRID_QUERY.to_owned(),
            repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(BENCH_HYBRID_LIMIT),
            weights: None,
            semantic: Some(false),
        })))
        .expect("search_hybrid semantic-toggle-off benchmark probe should be deterministic")
        .0;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.note, second.note);
    assert_eq!(
        semantic_note_string_field(&first.note, "semantic_status").as_deref(),
        Some("disabled"),
        "search_hybrid semantic-toggle-off benchmark probe must emit disabled semantic_status"
    );
    assert_eq!(
        semantic_note_bool_field(&first.note, "semantic_enabled"),
        Some(false),
        "search_hybrid semantic-toggle-off benchmark probe must mark semantic_enabled=false"
    );
    assert_eq!(
        semantic_note_string_field(&first.note, "semantic_reason").as_deref(),
        Some("semantic channel disabled by request toggle"),
        "search_hybrid semantic-toggle-off benchmark probe must emit explicit semantic_reason"
    );
}

fn assert_search_hybrid_semantic_degraded_workload(runtime: &Runtime, server: &FriggMcpServer) {
    let first = runtime
        .block_on(server.search_hybrid(Parameters(SearchHybridParams {
            query: BENCH_HYBRID_QUERY.to_owned(),
            repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(BENCH_HYBRID_LIMIT),
            weights: None,
            semantic: Some(true),
        })))
        .expect("search_hybrid semantic-degraded benchmark probe should succeed")
        .0;
    let second = runtime
        .block_on(server.search_hybrid(Parameters(SearchHybridParams {
            query: BENCH_HYBRID_QUERY.to_owned(),
            repository_id: Some(BENCH_REPOSITORY_ID.to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(BENCH_HYBRID_LIMIT),
            weights: None,
            semantic: Some(true),
        })))
        .expect("search_hybrid semantic-degraded benchmark probe should be deterministic")
        .0;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.note, second.note);
    assert_eq!(
        semantic_note_string_field(&first.note, "semantic_status").as_deref(),
        Some("degraded"),
        "search_hybrid semantic-degraded benchmark probe must emit degraded semantic_status"
    );
    assert_eq!(
        semantic_note_bool_field(&first.note, "semantic_enabled"),
        Some(false),
        "search_hybrid semantic-degraded benchmark probe must mark semantic_enabled=false"
    );
    assert!(
        semantic_note_string_field(&first.note, "semantic_reason")
            .is_some_and(|reason| reason.contains("semantic_runtime.model must not be blank")),
        "search_hybrid semantic-degraded benchmark probe should surface deterministic semantic startup-validation reason"
    );
}

fn semantic_note_string_field(note: &Option<String>, field: &str) -> Option<String> {
    let raw = note.as_deref()?;
    let payload: Value = serde_json::from_str(raw).ok()?;
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn semantic_note_bool_field(note: &Option<String>, field: &str) -> Option<bool> {
    let raw = note.as_deref()?;
    let payload: Value = serde_json::from_str(raw).ok()?;
    payload.get(field).and_then(Value::as_bool)
}

fn semantic_runtime_enabled_non_strict() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some(" ".to_owned()),
        strict_mode: false,
    }
}

fn prepare_bench_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "frigg-mcp-bench-repo-{}-{nonce}",
        std::process::id()
    ));
    populate_fixture(&root);
    root
}

fn populate_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("benchmark fixture root should be creatable");

    for file_idx in 0..BENCH_FILES {
        let relative = format!("src/file_{file_idx:03}.rs");
        let content = build_file_content(file_idx);
        fs::write(root.join(relative), content).expect("benchmark fixture file should be writable");
    }

    let scip_fixture_root = root.join(".frigg/scip");
    fs::create_dir_all(&scip_fixture_root)
        .expect("benchmark scip fixture root should be creatable");
    let fixture_source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scip")
        .join(BENCH_PRECISE_SCIP_FIXTURE);
    let fixture_target = scip_fixture_root.join(BENCH_PRECISE_SCIP_FIXTURE);
    fs::copy(&fixture_source, &fixture_target)
        .expect("benchmark precise scip fixture should be copied to repository fixture root");

    fs::write(
        root.join("src/navigation.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n\
         pub fn consumer() { let _ = ServiceMarker; }\n\
         pub struct ServiceMarker;\n",
    )
    .expect("benchmark navigation fixture source should be writable");
    fs::write(
        scip_fixture_root.join(BENCH_PRECISE_NAVIGATION_SCIP_FIXTURE),
        r#"{
  "documents": [
    {
      "relative_path": "src/navigation.rs",
      "occurrences": [
        { "symbol": "scip-rust pkg frigg-bench#Service", "range": [0, 10, 17], "symbol_roles": 1 },
        { "symbol": "scip-rust pkg frigg-bench#Impl", "range": [1, 11, 15], "symbol_roles": 1 },
        { "symbol": "scip-rust pkg frigg-bench#consumer", "range": [3, 7, 15], "symbol_roles": 1 }
      ],
      "symbols": [
        {
          "symbol": "scip-rust pkg frigg-bench#Service",
          "display_name": "Service",
          "kind": "trait",
          "relationships": []
        },
        {
          "symbol": "scip-rust pkg frigg-bench#Impl",
          "display_name": "Impl",
          "kind": "struct",
          "relationships": [
            { "symbol": "scip-rust pkg frigg-bench#Service", "is_implementation": true }
          ]
        },
        {
          "symbol": "scip-rust pkg frigg-bench#consumer",
          "display_name": "consumer",
          "kind": "function",
          "relationships": [
            { "symbol": "scip-rust pkg frigg-bench#Service", "is_reference": true }
          ]
        }
      ]
    }
  ]
}"#,
    )
    .expect("benchmark navigation scip fixture should be writable");
}

fn build_file_content(file_idx: usize) -> String {
    let entity = format!("Entity{file_idx:03}");
    let mut lines = Vec::with_capacity(BENCH_LINES_PER_FILE + 4);
    lines.push(format!("pub struct {entity};"));
    lines.push(format!(
        "pub fn create_{file_idx:03}() -> {entity} {{ {entity} }}"
    ));
    lines.push(format!(
        "pub fn use_{file_idx:03}() {{ let _ = {entity}; }}"
    ));
    lines.push(format!(
        "pub fn marker_{file_idx:03}() {{ let _ = {entity}; }}"
    ));

    for line_idx in 0..BENCH_LINES_PER_FILE {
        let suffix = (line_idx + file_idx) % 10_000;
        lines.push(format!(
            "// deterministic bench line {line_idx:03} needle {suffix}"
        ));
    }

    lines.join("\n")
}

criterion_group!(
    name = benches;
    config = Criterion::default().configure_from_args();
    targets = tool_latency_benchmarks
);
criterion_main!(benches);
