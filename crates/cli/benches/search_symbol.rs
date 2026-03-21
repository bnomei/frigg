#[path = "common/mod.rs"]
mod support;

use std::cell::Cell;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::mcp::types::{SearchSymbolParams, SearchSymbolPathClass};
use rmcp::handler::server::wrapper::Parameters;

fn bench_search_symbol(c: &mut Criterion) {
    let warm_session = support::attached_fixture_server_session();
    let mut group = c.benchmark_group("search_symbol");

    group.bench_function(BenchmarkId::from_parameter("warm_runtime_lookup"), |b| {
        b.iter(|| {
            let response = warm_session
                .runtime
                .block_on(
                    warm_session
                        .server
                        .search_symbol(Parameters(SearchSymbolParams {
                            query: "Widget032".to_owned(),
                            repository_id: Some(warm_session.repository_id.clone()),
                            path_class: Some(SearchSymbolPathClass::Runtime),
                            path_regex: None,
                            limit: Some(16),
                        })),
                )
                .expect("warm search_symbol benchmark should succeed")
                .0;
            criterion::black_box(response.matches.len());
        });
    });

    let stale_root = support::fresh_fixture_root("search-symbol-stale");
    let stale_session =
        support::attached_server_session(support::native_search_config(&stale_root), &stale_root);
    let hot_symbol_path = stale_root.join("src/hot_symbol.rs");
    support::rewrite_file_with_new_mtime(&hot_symbol_path, "pub struct FreshBenchSeed;\n");
    stale_session
        .runtime
        .block_on(
            stale_session
                .server
                .search_symbol(Parameters(SearchSymbolParams {
                    query: "FreshBenchSeed".to_owned(),
                    repository_id: Some(stale_session.repository_id.clone()),
                    path_class: Some(SearchSymbolPathClass::Runtime),
                    path_regex: None,
                    limit: Some(8),
                })),
        )
        .expect("initial stale benchmark warmup should succeed");
    let revision = Cell::new(0usize);

    group.bench_function(BenchmarkId::from_parameter("stale_corpus_rebuild"), |b| {
        b.iter(|| {
            let next_revision = revision.get().saturating_add(1);
            revision.set(next_revision);
            let symbol_name = format!("FreshBench{next_revision}");
            support::rewrite_file_with_new_mtime(
                &hot_symbol_path,
                &format!(
                    "pub struct {symbol_name};\n\
                     pub fn {symbol_name}_factory() -> {symbol_name} {{ {symbol_name} }}\n"
                ),
            );
            let response = stale_session
                .runtime
                .block_on(
                    stale_session
                        .server
                        .search_symbol(Parameters(SearchSymbolParams {
                            query: symbol_name.clone(),
                            repository_id: Some(stale_session.repository_id.clone()),
                            path_class: Some(SearchSymbolPathClass::Runtime),
                            path_regex: None,
                            limit: Some(8),
                        })),
                )
                .expect("stale search_symbol benchmark should succeed")
                .0;
            criterion::black_box(response.matches.len());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_search_symbol);
criterion_main!(benches);
