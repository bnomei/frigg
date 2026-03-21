#[path = "common/mod.rs"]
mod support;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::searcher::{HybridChannelWeights, SearchFilters, SearchHybridQuery, TextSearcher};

fn bench_hybrid_search(c: &mut Criterion) {
    let root = support::fixture_root();
    let searcher = TextSearcher::new(support::native_search_config(root));
    let mut group = c.benchmark_group("hybrid_search");

    for (label, query) in [
        ("exact_component", "handle_checkout_request"),
        ("natural_language", "where is checkout handled"),
    ] {
        let query = SearchHybridQuery {
            query: query.to_owned(),
            limit: 16,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        };
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let output = searcher
                    .search_hybrid_with_filters(query.clone(), SearchFilters::default())
                    .expect("hybrid search benchmark should succeed");
                criterion::black_box(output.matches.len());
                criterion::black_box(output.note.lexical_only_mode);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_hybrid_search);
criterion_main!(benches);
