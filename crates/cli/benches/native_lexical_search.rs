#[path = "common/mod.rs"]
mod support;

use criterion::{Criterion, criterion_group, criterion_main};
use frigg::searcher::{SearchFilters, SearchTextQuery, TextSearcher};

fn bench_native_lexical_search(c: &mut Criterion) {
    let root = support::fixture_root();
    let searcher = TextSearcher::new(support::native_search_config(root));
    let query = SearchTextQuery {
        query: "handle_checkout_request".to_owned(),
        path_regex: None,
        limit: 64,
    };
    c.bench_function("native_lexical_search", |b| {
        b.iter(|| {
            let output = searcher
                .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())
                .expect("native lexical search benchmark should succeed");
            criterion::black_box(output.matches.len());
        });
    });
}

criterion_group!(benches, bench_native_lexical_search);
criterion_main!(benches);
