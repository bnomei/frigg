use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::searcher::{
    HybridSemanticStatus, SearchFilters, SearchHybridQuery, SearchTextQuery, TextSearcher,
};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};
use regex::Regex;

const BENCH_REPO_COUNT: usize = 2;
const BENCH_FILES_PER_REPO: usize = 60;
const BENCH_LINES_PER_FILE: usize = 80;
const BENCH_LOW_LIMIT: usize = 5;
const BENCH_HIGH_CARDINALITY_QUERY: &str = "needle_hotspot";
const BENCH_REGEX_SPARSE_QUERY: &str = r"literal_nohit_0_\d+_9";
const BENCH_REGEX_NO_HIT_QUERY: &str = r"prefilter_absent_token_\d+";
const BENCH_HYBRID_QUERY: &str = BENCH_HIGH_CARDINALITY_QUERY;

static BENCH_ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();

fn search_latency_benchmarks(c: &mut Criterion) {
    let roots = BENCH_ROOTS.get_or_init(prepare_bench_roots);
    let config = FriggConfig::from_workspace_roots(roots.clone())
        .expect("benchmark roots should always produce valid FriggConfig");
    let mut semantic_config = config.clone();
    semantic_config.semantic_runtime = semantic_runtime_enabled_non_strict();

    let searcher = TextSearcher::new(config);
    let semantic_searcher = TextSearcher::new(semantic_config);
    assert_low_limit_high_cardinality_workload(&searcher);
    assert_hybrid_semantic_toggle_off_workload(&searcher);
    assert_hybrid_semantic_degraded_workload(&semantic_searcher);

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

    group.finish();
}

fn prepare_bench_roots() -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(BENCH_REPO_COUNT);
    for repo_idx in 0..BENCH_REPO_COUNT {
        let root = std::env::temp_dir().join(format!(
            "frigg-search-bench-repo-{repo_idx}-{}",
            std::process::id()
        ));
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }

        populate_repo_fixture(&root, repo_idx);
        roots.push(root);
    }

    roots
}

fn populate_repo_fixture(root: &Path, repo_idx: usize) {
    fs::create_dir_all(root.join("src/nested"))
        .expect("benchmark fixture directory should be creatable");

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
