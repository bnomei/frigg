#[path = "common/mod.rs"]
mod support;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::searcher::{SearchFilters, SearchTextQuery, TextSearcher};

fn bench_lexical_backend_compare(c: &mut Criterion) {
    let root = support::fixture_root();
    let native_searcher = TextSearcher::new(support::native_search_config(root));
    let mut group = c.benchmark_group("lexical_backend_compare");

    let literal_query = SearchTextQuery {
        query: "handle_checkout_request".to_owned(),
        path_regex: None,
        limit: 64,
    };
    group.bench_function(BenchmarkId::new("native", "literal"), |b| {
        b.iter(|| {
            let output = native_searcher
                .search_literal_with_filters_diagnostics(
                    literal_query.clone(),
                    SearchFilters::default(),
                )
                .expect("native literal benchmark should succeed");
            criterion::black_box(output.matches.len());
            criterion::black_box(output.lexical_backend.map(|backend| backend.as_str()));
        });
    });

    let regex_query = SearchTextQuery {
        query: "handle_checkout_request|render_summary".to_owned(),
        path_regex: None,
        limit: 64,
    };
    group.bench_function(BenchmarkId::new("native", "regex"), |b| {
        b.iter(|| {
            let output = native_searcher
                .search_regex_with_filters_diagnostics(
                    regex_query.clone(),
                    SearchFilters::default(),
                )
                .expect("native regex benchmark should succeed");
            criterion::black_box(output.matches.len());
            criterion::black_box(output.lexical_backend.map(|backend| backend.as_str()));
        });
    });

    if let Some(rg_executable) = support::rg_executable() {
        let ripgrep_searcher =
            TextSearcher::new(support::ripgrep_search_config(root, rg_executable));
        group.bench_function(BenchmarkId::new("ripgrep", "literal"), |b| {
            b.iter(|| {
                let output = ripgrep_searcher
                    .search_literal_with_filters_diagnostics(
                        literal_query.clone(),
                        SearchFilters::default(),
                    )
                    .expect("ripgrep literal benchmark should succeed");
                criterion::black_box(output.matches.len());
                criterion::black_box(output.lexical_backend.map(|backend| backend.as_str()));
            });
        });
        group.bench_function(BenchmarkId::new("ripgrep", "regex"), |b| {
            b.iter(|| {
                let output = ripgrep_searcher
                    .search_regex_with_filters_diagnostics(
                        regex_query.clone(),
                        SearchFilters::default(),
                    )
                    .expect("ripgrep regex benchmark should succeed");
                criterion::black_box(output.matches.len());
                criterion::black_box(output.lexical_backend.map(|backend| backend.as_str()));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_lexical_backend_compare);
criterion_main!(benches);
