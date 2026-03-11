use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::indexer::{ReindexMode, reindex_repository};
use frigg::searcher::{
    HybridSemanticStatus, SearchFilters, SearchHybridQuery, SearchStageAttribution,
    SearchTextQuery, TextSearcher,
};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};
use frigg::storage::ensure_provenance_db_parent_dir;
use regex::Regex;
use serde::Serialize;

const BENCH_REPO_COUNT: usize = 2;
const BENCH_FILES_PER_REPO: usize = 60;
const BENCH_LINES_PER_FILE: usize = 80;
const BENCH_LOW_LIMIT: usize = 5;
const BENCH_PATH_WITNESS_LIMIT: usize = 8;
const BENCH_BENCHMARK_WITNESS_LIMIT: usize = 8;
const BENCH_HIGH_CARDINALITY_QUERY: &str = "needle_hotspot";
const BENCH_REGEX_SPARSE_QUERY: &str = r"literal_nohit_0_\d+_9";
const BENCH_REGEX_NO_HIT_QUERY: &str = r"prefilter_absent_token_\d+";
const BENCH_HYBRID_QUERY: &str = BENCH_HIGH_CARDINALITY_QUERY;
const BENCH_GRAPH_QUERY: &str = "OrderHandler handle listener";
const BENCH_PATH_WITNESS_QUERY: &str =
    "entry point bootstrap build flow command runner main config";
const BENCH_BENCHMARK_WITNESS_QUERY: &str = "benchmark latest report budget metrics";

static BENCH_ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
static BENCH_INDEXED_ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
static BENCH_WITNESS_ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();

#[derive(Debug, Serialize)]
struct SearchStageAttributionSnapshot {
    report_version: &'static str,
    workloads: Vec<SearchStageAttributionWorkload>,
}

#[derive(Debug, Serialize)]
struct SearchStageAttributionWorkload {
    id: String,
    stage_attribution: SearchStageAttribution,
}

fn search_stage_attribution_snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/search-stage-attribution.latest.json")
}

fn write_search_stage_attribution_snapshot(workloads: Vec<SearchStageAttributionWorkload>) {
    let snapshot = SearchStageAttributionSnapshot {
        report_version: "v1",
        workloads,
    };
    let path = search_stage_attribution_snapshot_path();
    fs::write(
        &path,
        serde_json::to_string_pretty(&snapshot)
            .expect("search stage attribution snapshot should serialize"),
    )
    .expect("search stage attribution snapshot should be writable");
}

fn hybrid_stage_snapshot(
    searcher: &TextSearcher,
    workload_id: &str,
    query: SearchHybridQuery,
) -> SearchStageAttributionWorkload {
    let output = searcher
        .search_hybrid_with_filters(query, SearchFilters::default())
        .expect("stage-attributed hybrid benchmark fixture should be searchable");
    let stage_attribution = output
        .stage_attribution
        .expect("hybrid benchmark output should expose stage attribution");
    SearchStageAttributionWorkload {
        id: workload_id.to_owned(),
        stage_attribution,
    }
}

fn search_latency_benchmarks(c: &mut Criterion) {
    let roots = BENCH_ROOTS.get_or_init(prepare_bench_roots);
    let indexed_roots = BENCH_INDEXED_ROOTS.get_or_init(prepare_indexed_bench_roots);
    let witness_roots = BENCH_WITNESS_ROOTS.get_or_init(prepare_witness_bench_roots);
    let config = FriggConfig::from_workspace_roots(roots.clone())
        .expect("benchmark roots should always produce valid FriggConfig");
    let indexed_config = FriggConfig::from_workspace_roots(indexed_roots.clone())
        .expect("indexed benchmark roots should always produce valid FriggConfig");
    let witness_config = FriggConfig::from_workspace_roots(witness_roots.clone())
        .expect("witness benchmark roots should always produce valid FriggConfig");
    let mut semantic_config = config.clone();
    semantic_config.semantic_runtime = semantic_runtime_enabled_non_strict();

    let searcher = TextSearcher::new(config);
    let indexed_searcher = TextSearcher::new(indexed_config);
    let witness_searcher = TextSearcher::new(witness_config);
    let semantic_searcher = TextSearcher::new(semantic_config);
    assert_low_limit_high_cardinality_workload(&searcher);
    assert_manifest_backed_low_limit_high_cardinality_workload(&indexed_searcher);
    assert_hybrid_graph_target_evidence_workload(&searcher);
    assert_hybrid_benchmark_witness_workload(&witness_searcher);
    assert_hybrid_path_witness_build_flow_workload(&witness_searcher);
    assert_hybrid_semantic_toggle_off_workload(&searcher);
    assert_hybrid_semantic_degraded_workload(&semantic_searcher);
    write_search_stage_attribution_snapshot(vec![
        hybrid_stage_snapshot(
            &searcher,
            "search_latency/hybrid/semantic-toggle-off",
            SearchHybridQuery {
                query: BENCH_HYBRID_QUERY.to_owned(),
                limit: BENCH_LOW_LIMIT,
                weights: Default::default(),
                semantic: Some(false),
            },
        ),
        hybrid_stage_snapshot(
            &semantic_searcher,
            "search_latency/hybrid/semantic-degraded-missing-credentials",
            SearchHybridQuery {
                query: BENCH_HYBRID_QUERY.to_owned(),
                limit: BENCH_LOW_LIMIT,
                weights: Default::default(),
                semantic: Some(true),
            },
        ),
        hybrid_stage_snapshot(
            &searcher,
            "search_latency/hybrid/graph-php-target-evidence",
            SearchHybridQuery {
                query: BENCH_GRAPH_QUERY.to_owned(),
                limit: BENCH_LOW_LIMIT,
                weights: Default::default(),
                semantic: Some(false),
            },
        ),
        hybrid_stage_snapshot(
            &witness_searcher,
            "search_latency/hybrid/benchmark-witness-recall",
            SearchHybridQuery {
                query: BENCH_BENCHMARK_WITNESS_QUERY.to_owned(),
                limit: BENCH_BENCHMARK_WITNESS_LIMIT,
                weights: Default::default(),
                semantic: Some(false),
            },
        ),
        hybrid_stage_snapshot(
            &witness_searcher,
            "search_latency/hybrid/path-witness-build-flow",
            SearchHybridQuery {
                query: BENCH_PATH_WITNESS_QUERY.to_owned(),
                limit: BENCH_PATH_WITNESS_LIMIT,
                weights: Default::default(),
                semantic: Some(false),
            },
        ),
    ]);

    let mut group = c.benchmark_group("search_latency");
    group.sample_size(30);

    group.bench_function(BenchmarkId::new("literal", "global"), |b| {
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 200,
        };
        b.iter(|| {
            let matches = searcher
                .search_literal_with_filters(query.clone(), SearchFilters::default())
                .expect("literal benchmark should not fail");
            criterion::black_box(matches);
        });
    });

    group.bench_function(BenchmarkId::new("literal", "global-low-limit"), |b| {
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: BENCH_LOW_LIMIT,
        };
        b.iter(|| {
            let matches = searcher
                .search_literal_with_filters(query.clone(), SearchFilters::default())
                .expect("low-limit literal benchmark should not fail");
            criterion::black_box(matches);
        });
    });

    group.bench_function(
        BenchmarkId::new("literal", "global-low-limit-high-cardinality"),
        |b| {
            let query = SearchTextQuery {
                query: BENCH_HIGH_CARDINALITY_QUERY.to_owned(),
                path_regex: None,
                limit: BENCH_LOW_LIMIT,
            };
            b.iter(|| {
                let matches = searcher
                    .search_literal_with_filters(query.clone(), SearchFilters::default())
                    .expect("high-cardinality literal benchmark should not fail");
                criterion::black_box(matches);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("literal", "indexed-manifest-low-limit-high-cardinality"),
        |b| {
            let query = SearchTextQuery {
                query: BENCH_HIGH_CARDINALITY_QUERY.to_owned(),
                path_regex: None,
                limit: BENCH_LOW_LIMIT,
            };
            b.iter(|| {
                let matches = indexed_searcher
                    .search_literal_with_filters(query.clone(), SearchFilters::default())
                    .expect("manifest-backed literal benchmark should not fail");
                criterion::black_box(matches);
            });
        },
    );

    group.bench_function(BenchmarkId::new("literal", "repo+path+lang"), |b| {
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: Some(
                Regex::new(r"^src/.*\.rs$").expect("hardcoded benchmark regex should compile"),
            ),
            limit: 200,
        };
        let filters = SearchFilters {
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
        };
        b.iter(|| {
            let matches = searcher
                .search_literal_with_filters(query.clone(), filters.clone())
                .expect("filtered literal benchmark should not fail");
            criterion::black_box(matches);
        });
    });

    group.bench_function(BenchmarkId::new("regex", "repo+path+lang"), |b| {
        let query = SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(
                Regex::new(r"^src/.*\.rs$").expect("hardcoded benchmark regex should compile"),
            ),
            limit: 200,
        };
        let filters = SearchFilters {
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
        };
        b.iter(|| {
            let matches = searcher
                .search_regex_with_filters(query.clone(), filters.clone())
                .expect("filtered regex benchmark should not fail");
            criterion::black_box(matches);
        });
    });

    group.bench_function(
        BenchmarkId::new("regex", "global-sparse-required-literal"),
        |b| {
            let query = SearchTextQuery {
                query: BENCH_REGEX_SPARSE_QUERY.to_owned(),
                path_regex: None,
                limit: 200,
            };
            b.iter(|| {
                let matches = searcher
                    .search_regex_with_filters(query.clone(), SearchFilters::default())
                    .expect("global sparse regex benchmark should not fail");
                criterion::black_box(matches);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("regex", "global-no-hit-required-literal"),
        |b| {
            let query = SearchTextQuery {
                query: BENCH_REGEX_NO_HIT_QUERY.to_owned(),
                path_regex: None,
                limit: 200,
            };
            b.iter(|| {
                let matches = searcher
                    .search_regex_with_filters(query.clone(), SearchFilters::default())
                    .expect("global no-hit regex benchmark should not fail");
                criterion::black_box(matches);
            });
        },
    );

    group.bench_function(BenchmarkId::new("hybrid", "semantic-toggle-off"), |b| {
        let query = SearchHybridQuery {
            query: BENCH_HYBRID_QUERY.to_owned(),
            limit: BENCH_LOW_LIMIT,
            weights: Default::default(),
            semantic: Some(false),
        };
        b.iter(|| {
            let output = searcher
                .search_hybrid_with_filters(query.clone(), SearchFilters::default())
                .expect("hybrid semantic-toggle-off benchmark should not fail");
            criterion::black_box(output);
        });
    });

    group.bench_function(
        BenchmarkId::new("hybrid", "semantic-degraded-missing-credentials"),
        |b| {
            let query = SearchHybridQuery {
                query: BENCH_HYBRID_QUERY.to_owned(),
                limit: BENCH_LOW_LIMIT,
                weights: Default::default(),
                semantic: Some(true),
            };
            b.iter(|| {
                let output = semantic_searcher
                    .search_hybrid_with_filters(query.clone(), SearchFilters::default())
                    .expect(
                        "hybrid semantic-degraded benchmark should not fail in non-strict mode",
                    );
                criterion::black_box(output);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("hybrid", "graph-php-target-evidence"),
        |b| {
            let query = SearchHybridQuery {
                query: BENCH_GRAPH_QUERY.to_owned(),
                limit: BENCH_LOW_LIMIT,
                weights: Default::default(),
                semantic: Some(false),
            };
            b.iter(|| {
                let output = searcher
                    .search_hybrid_with_filters(query.clone(), SearchFilters::default())
                    .expect("hybrid graph-target-evidence benchmark should not fail");
                criterion::black_box(output);
            });
        },
    );

    group.bench_function(
        BenchmarkId::new("hybrid", "benchmark-witness-recall"),
        |b| {
            let query = SearchHybridQuery {
                query: BENCH_BENCHMARK_WITNESS_QUERY.to_owned(),
                limit: BENCH_BENCHMARK_WITNESS_LIMIT,
                weights: Default::default(),
                semantic: Some(false),
            };
            b.iter(|| {
                let output = witness_searcher
                    .search_hybrid_with_filters(query.clone(), SearchFilters::default())
                    .expect("hybrid benchmark-witness workload should not fail");
                criterion::black_box(output);
            });
        },
    );

    group.bench_function(BenchmarkId::new("hybrid", "path-witness-build-flow"), |b| {
        let query = SearchHybridQuery {
            query: BENCH_PATH_WITNESS_QUERY.to_owned(),
            limit: BENCH_PATH_WITNESS_LIMIT,
            weights: Default::default(),
            semantic: Some(false),
        };
        b.iter(|| {
            let output = witness_searcher
                .search_hybrid_with_filters(query.clone(), SearchFilters::default())
                .expect("hybrid path-witness benchmark should not fail");
            criterion::black_box(output);
        });
    });

    group.finish();
}

fn prepare_bench_roots() -> Vec<PathBuf> {
    prepare_named_bench_roots("walk", false, false)
}

fn prepare_indexed_bench_roots() -> Vec<PathBuf> {
    prepare_named_bench_roots("indexed", true, false)
}

fn prepare_witness_bench_roots() -> Vec<PathBuf> {
    prepare_named_bench_roots("witness", true, true)
}

fn prepare_named_bench_roots(
    label: &str,
    manifest_backed: bool,
    include_witness_fixtures: bool,
) -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(BENCH_REPO_COUNT);
    for repo_idx in 0..BENCH_REPO_COUNT {
        let root = std::env::temp_dir().join(format!(
            "frigg-search-bench-{label}-repo-{repo_idx}-{}",
            std::process::id()
        ));
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }

        populate_repo_fixture(&root, repo_idx, include_witness_fixtures);
        if manifest_backed {
            index_bench_root(repo_idx, &root);
        }
        roots.push(root);
    }

    roots
}

fn index_bench_root(repo_idx: usize, root: &Path) {
    let repository_id = format!("repo-{:03}", repo_idx + 1);
    let db_path = ensure_provenance_db_parent_dir(root)
        .expect("indexed benchmark root should allow provenance storage initialization");
    reindex_repository(&repository_id, root, &db_path, ReindexMode::Full)
        .expect("indexed benchmark root should reindex deterministically");
}

fn populate_repo_fixture(root: &Path, repo_idx: usize, include_witness_fixtures: bool) {
    fs::create_dir_all(root.join("src/nested"))
        .expect("benchmark fixture directory should be creatable");
    fs::create_dir_all(root.join("src/Handlers"))
        .expect("benchmark handler fixture directory should be creatable");
    fs::create_dir_all(root.join("src/Listeners"))
        .expect("benchmark listener fixture directory should be creatable");
    fs::create_dir_all(root.join("docs"))
        .expect("benchmark docs fixture directory should be creatable");

    for file_idx in 0..BENCH_FILES_PER_REPO {
        let rel = if file_idx % 4 == 0 {
            format!("src/nested/file_{file_idx:03}.php")
        } else if file_idx % 2 == 0 {
            format!("src/file_{file_idx:03}.md")
        } else {
            format!("src/file_{file_idx:03}.rs")
        };
        let content = build_file_content(repo_idx, file_idx);
        fs::write(root.join(rel), content).expect("benchmark fixture file should be writable");
    }

    if repo_idx == 0 {
        fs::write(
            root.join("src/Handlers/OrderHandler.php"),
            "<?php\n\
             namespace App\\Handlers;\n\
             class OrderHandler {\n\
                 public function handle(): void {}\n\
             }\n",
        )
        .expect("benchmark order handler fixture should be writable");
        fs::write(
            root.join("src/Listeners/OrderListener.php"),
            "<?php\n\
             namespace App\\Listeners;\n\
             use App\\Handlers\\OrderHandler;\n\
             class OrderListener {\n\
                 public function handlers(): array {\n\
                     return [[OrderHandler::class, 'handle']];\n\
                 }\n\
             }\n",
        )
        .expect("benchmark order listener fixture should be writable");
        fs::write(
            root.join("docs/handlers.md"),
            "# Handlers\nOrder handler listener overview.\n",
        )
        .expect("benchmark order handler docs should be writable");
    }

    if repo_idx == 0 && include_witness_fixtures {
        fs::create_dir_all(root.join("benchmarks"))
            .expect("benchmark methodology fixture directory should be creatable");
        fs::create_dir_all(root.join("crates/cli/benches"))
            .expect("benchmark support fixture directory should be creatable");
        for (rel_path, content) in [
            (
                "benchmarks/latest-report.md",
                "# Benchmark latest report\n\
                 - budget metrics: search latency, graph latency, storage latency\n\
                 - latest report budget metrics benchmark replay summary\n",
            ),
            (
                "benchmarks/search.md",
                "# Search benchmark methodology\n\
                 Benchmark budget metrics should keep the latest report deterministic.\n",
            ),
            (
                "crates/cli/benches/search_latency_support.rs",
                "pub fn benchmark_metrics_fixture() {\n\
                 let _ = \"benchmark latest report budget metrics\";\n\
                 }\n",
            ),
            (
                "crates/cli/benches/tool_latency_support.rs",
                "pub fn replay_benchmark_metrics() {\n\
                 let _ = \"benchmark budget metrics replay latest report\";\n\
                 }\n",
            ),
        ] {
            fs::write(root.join(rel_path), content)
                .expect("benchmark witness fixture file should be writable");
        }

        for rel_path in [
            ".github/workflows",
            "src-tauri/src/proxy",
            "src-tauri/src/modules",
            "src-tauri/src/models",
            "src-tauri/src/commands",
        ] {
            fs::create_dir_all(root.join(rel_path))
                .expect("path-witness benchmark fixture directory should be creatable");
        }

        for (rel_path, content) in [
            (
                "src-tauri/src/main.rs",
                "fn main() {\n\
                 // entry point bootstrap build flow command runner main config\n\
                 let config = load_config();\n\
                 run_build_flow(config);\n\
                 }\n",
            ),
            (
                "src-tauri/src/lib.rs",
                "pub fn run() {\n\
                 // entry point bootstrap build flow command runner main config\n\
                 }\n",
            ),
            (
                "src-tauri/src/proxy/config.rs",
                "pub struct ProxyConfig;\n\
                 // entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/modules/config.rs",
                "pub struct ModuleConfig;\n\
                 // entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/models/config.rs",
                "pub struct ModelConfig;\n\
                 // entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/commands/security.rs",
                "pub fn security_command() {\n\
                 // entry point bootstrap build flow command runner main config\n\
                 }\n",
            ),
            (
                ".github/workflows/deploy-pages.yml",
                "name: Deploy static content to Pages\n\
                 jobs:\n\
                   deploy:\n\
                     steps:\n\
                       - name: Upload artifact\n\
                         run: echo upload build artifacts\n\
                       - name: Deploy to GitHub Pages\n\
                         run: echo deploy release pages\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\n\
                 jobs:\n\
                   build-tauri:\n\
                     steps:\n\
                       - name: Build the app\n\
                         run: cargo build --release\n\
                       - name: Publish release artifacts\n\
                         run: echo publish release artifacts\n",
            ),
        ] {
            fs::write(root.join(rel_path), content)
                .expect("path-witness benchmark fixture file should be writable");
        }
    }
}

fn assert_low_limit_high_cardinality_workload(searcher: &TextSearcher) {
    let full_query = SearchTextQuery {
        query: BENCH_HIGH_CARDINALITY_QUERY.to_owned(),
        path_regex: None,
        limit: 10_000,
    };
    let limited_query = SearchTextQuery {
        query: BENCH_HIGH_CARDINALITY_QUERY.to_owned(),
        path_regex: None,
        limit: BENCH_LOW_LIMIT,
    };
    let full = searcher
        .search_literal_with_filters(full_query, SearchFilters::default())
        .expect("high-cardinality benchmark fixture should be searchable");
    let first_limited = searcher
        .search_literal_with_filters(limited_query.clone(), SearchFilters::default())
        .expect("high-cardinality low-limit benchmark fixture should be searchable");
    let second_limited = searcher
        .search_literal_with_filters(limited_query, SearchFilters::default())
        .expect("high-cardinality low-limit benchmark fixture should be stable");

    assert_eq!(
        first_limited.len(),
        BENCH_LOW_LIMIT,
        "high-cardinality low-limit fixture should hit the configured limit"
    );
    assert_eq!(
        first_limited, second_limited,
        "high-cardinality low-limit fixture should be deterministic across repeated runs"
    );
    assert_eq!(
        first_limited,
        full.into_iter().take(BENCH_LOW_LIMIT).collect::<Vec<_>>(),
        "high-cardinality low-limit fixture should match deterministic sorted prefix"
    );
}

fn assert_manifest_backed_low_limit_high_cardinality_workload(searcher: &TextSearcher) {
    let query = SearchTextQuery {
        query: BENCH_HIGH_CARDINALITY_QUERY.to_owned(),
        path_regex: None,
        limit: BENCH_LOW_LIMIT,
    };
    let first = searcher
        .search_literal_with_filters(query.clone(), SearchFilters::default())
        .expect("manifest-backed high-cardinality benchmark fixture should be searchable");
    let second = searcher
        .search_literal_with_filters(query, SearchFilters::default())
        .expect("manifest-backed high-cardinality benchmark fixture should be deterministic");

    assert_eq!(
        first.len(),
        BENCH_LOW_LIMIT,
        "manifest-backed high-cardinality benchmark should honor the low limit"
    );
    assert_eq!(
        first, second,
        "manifest-backed high-cardinality benchmark should be deterministic across repeated runs"
    );
}

fn assert_hybrid_semantic_toggle_off_workload(searcher: &TextSearcher) {
    let query = SearchHybridQuery {
        query: BENCH_HYBRID_QUERY.to_owned(),
        limit: BENCH_LOW_LIMIT,
        weights: Default::default(),
        semantic: Some(false),
    };
    let first = searcher
        .search_hybrid_with_filters(query.clone(), SearchFilters::default())
        .expect("hybrid semantic-toggle-off benchmark fixture should be searchable");
    let second = searcher
        .search_hybrid_with_filters(query, SearchFilters::default())
        .expect("hybrid semantic-toggle-off benchmark fixture should be deterministic");

    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(!first.note.semantic_enabled);
    assert_eq!(
        first.note.semantic_reason.as_deref(),
        Some("semantic channel disabled by request toggle"),
        "hybrid semantic-toggle-off benchmark should emit explicit disabled reason"
    );
    assert_eq!(
        first.matches, second.matches,
        "hybrid semantic-toggle-off benchmark should be deterministic across repeated runs"
    );
    assert_eq!(
        first.note, second.note,
        "hybrid semantic-toggle-off benchmark should emit deterministic note metadata"
    );
    assert!(
        first.stage_attribution.is_some(),
        "hybrid semantic-toggle-off benchmark should expose stage attribution"
    );
}

fn assert_hybrid_graph_target_evidence_workload(searcher: &TextSearcher) {
    let query = SearchHybridQuery {
        query: BENCH_GRAPH_QUERY.to_owned(),
        limit: BENCH_LOW_LIMIT,
        weights: Default::default(),
        semantic: Some(false),
    };
    let first = searcher
        .search_hybrid_with_filters(query.clone(), SearchFilters::default())
        .expect("hybrid graph-target-evidence benchmark fixture should be searchable");
    let second = searcher
        .search_hybrid_with_filters(query, SearchFilters::default())
        .expect("hybrid graph-target-evidence benchmark fixture should be deterministic");

    let listener_match = first
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/Listeners/OrderListener.php")
        .expect("graph-target-evidence benchmark should surface listener witness");
    assert!(
        listener_match.graph_score > 0.0,
        "graph-target-evidence benchmark should capture graph score for listener witness: {:?}",
        first.matches
    );
    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(!first.note.semantic_enabled);
    assert_eq!(
        first.matches, second.matches,
        "hybrid graph-target-evidence benchmark should be deterministic across repeated runs"
    );
    assert_eq!(
        first.note, second.note,
        "hybrid graph-target-evidence benchmark should emit deterministic note metadata"
    );
    let stage_attribution = first
        .stage_attribution
        .as_ref()
        .expect("hybrid graph-target-evidence benchmark should expose stage attribution");
    assert!(
        stage_attribution.graph_expansion.output_count > 0,
        "graph-target-evidence benchmark should report graph expansion output: {stage_attribution:?}"
    );
}

fn assert_hybrid_benchmark_witness_workload(searcher: &TextSearcher) {
    let query = SearchHybridQuery {
        query: BENCH_BENCHMARK_WITNESS_QUERY.to_owned(),
        limit: BENCH_BENCHMARK_WITNESS_LIMIT,
        weights: Default::default(),
        semantic: Some(false),
    };
    let first = searcher
        .search_hybrid_with_filters(query.clone(), SearchFilters::default())
        .expect("hybrid benchmark-witness benchmark fixture should be searchable");
    let second = searcher
        .search_hybrid_with_filters(query, SearchFilters::default())
        .expect("hybrid benchmark-witness benchmark fixture should be deterministic");

    let ranked_paths = first
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths
            .iter()
            .take(BENCH_BENCHMARK_WITNESS_LIMIT)
            .any(|path| *path == "benchmarks/latest-report.md"),
        "benchmark-intent workload should surface benchmark report docs near the top: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(BENCH_BENCHMARK_WITNESS_LIMIT)
            .any(|path| *path == "crates/cli/benches/search_latency_support.rs"),
        "benchmark-intent workload should surface bench support witnesses near the top: {ranked_paths:?}"
    );
    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(!first.note.semantic_enabled);
    assert_eq!(
        first.matches, second.matches,
        "hybrid benchmark-witness workload should be deterministic across repeated runs"
    );
    assert_eq!(
        first.note, second.note,
        "hybrid benchmark-witness workload should emit deterministic note metadata"
    );
    let stage_attribution = first
        .stage_attribution
        .as_ref()
        .expect("hybrid benchmark-witness workload should expose stage attribution");
    assert!(
        stage_attribution.witness_scoring.output_count > 0,
        "benchmark-witness workload should report witness scoring output: {stage_attribution:?}"
    );
}

fn assert_hybrid_path_witness_build_flow_workload(searcher: &TextSearcher) {
    let query = SearchHybridQuery {
        query: BENCH_PATH_WITNESS_QUERY.to_owned(),
        limit: BENCH_PATH_WITNESS_LIMIT,
        weights: Default::default(),
        semantic: Some(false),
    };
    let first = searcher
        .search_hybrid_with_filters(query.clone(), SearchFilters::default())
        .expect("hybrid path-witness benchmark fixture should be searchable");
    let second = searcher
        .search_hybrid_with_filters(query, SearchFilters::default())
        .expect("hybrid path-witness benchmark fixture should be deterministic");

    let ranked_paths = first
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths
            .iter()
            .take(BENCH_PATH_WITNESS_LIMIT)
            .any(|path| *path == "src-tauri/src/main.rs"),
        "path-witness benchmark should surface an entrypoint runtime witness near the top: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(BENCH_PATH_WITNESS_LIMIT)
            .any(|path| {
                matches!(
                    *path,
                    ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
                )
            }),
        "path-witness benchmark should surface hidden workflow witnesses near the top: {ranked_paths:?}"
    );
    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Disabled);
    assert!(!first.note.semantic_enabled);
    assert_eq!(
        first.matches, second.matches,
        "hybrid path-witness benchmark should be deterministic across repeated runs"
    );
    assert_eq!(
        first.note, second.note,
        "hybrid path-witness benchmark should emit deterministic note metadata"
    );
    let stage_attribution = first
        .stage_attribution
        .as_ref()
        .expect("hybrid path-witness workload should expose stage attribution");
    assert!(
        stage_attribution.witness_scoring.output_count > 0,
        "path-witness benchmark should report witness scoring output: {stage_attribution:?}"
    );
}

fn assert_hybrid_semantic_degraded_workload(searcher: &TextSearcher) {
    let query = SearchHybridQuery {
        query: BENCH_HYBRID_QUERY.to_owned(),
        limit: BENCH_LOW_LIMIT,
        weights: Default::default(),
        semantic: Some(true),
    };
    let first = searcher
        .search_hybrid_with_filters(query.clone(), SearchFilters::default())
        .expect("hybrid semantic-degraded benchmark fixture should degrade without failing");
    let second = searcher
        .search_hybrid_with_filters(query, SearchFilters::default())
        .expect("hybrid semantic-degraded benchmark fixture should be deterministic");

    assert_eq!(first.note.semantic_status, HybridSemanticStatus::Degraded);
    assert!(!first.note.semantic_enabled);
    assert!(
        first
            .note
            .semantic_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("semantic_runtime.model must not be blank")),
        "hybrid semantic-degraded benchmark should expose deterministic semantic startup-validation reason"
    );
    assert_eq!(
        first.matches, second.matches,
        "hybrid semantic-degraded benchmark should be deterministic across repeated runs"
    );
    assert_eq!(
        first.note, second.note,
        "hybrid semantic-degraded benchmark should emit deterministic note metadata"
    );
    assert!(
        first.stage_attribution.is_some(),
        "hybrid semantic-degraded benchmark should expose stage attribution"
    );
}

fn semantic_runtime_enabled_non_strict() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some(" ".to_owned()),
        strict_mode: false,
    }
}

fn build_file_content(repo_idx: usize, file_idx: usize) -> String {
    let mut lines = Vec::with_capacity(BENCH_LINES_PER_FILE + 2);
    lines.push(format!(
        "// repo={repo_idx} file={file_idx} deterministic benchmark fixture"
    ));
    for line_idx in 0..BENCH_LINES_PER_FILE {
        if line_idx % 5 == 0 {
            lines.push(format!(
                "let dense_hits_{line_idx} = \"{0} {0} {0} {0} {0} {0}\";",
                BENCH_HIGH_CARDINALITY_QUERY
            ));
        } else if line_idx % 7 == 0 {
            lines.push(format!(
                "fn line_{line_idx}() {{ let token = \"needle {}\"; }}",
                line_idx + file_idx + repo_idx
            ));
        } else if line_idx % 9 == 0 {
            lines.push(format!(
                "literal_nohit_{}_{}_{}",
                repo_idx, file_idx, line_idx
            ));
        } else {
            lines.push(format!(
                "struct Placeholder{line_idx} {{ value: usize }} // repo={repo_idx} file={file_idx}"
            ));
        }
    }
    lines.push("end_of_fixture".to_owned());
    lines.join("\n")
}

criterion_group!(
    name = benches;
    config = Criterion::default().configure_from_args();
    targets = search_latency_benchmarks
);
criterion_main!(benches);
