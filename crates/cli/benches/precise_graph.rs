#[path = "common/mod.rs"]
mod support;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::mcp::benchmark_precise_graph_for_server;

fn bench_precise_graph(c: &mut Criterion) {
    let mut group = c.benchmark_group("precise_graph");

    group.bench_function(BenchmarkId::from_parameter("cold_ingest"), |b| {
        b.iter_batched(
            || {
                let root = support::fresh_fixture_root("precise-graph-cold");
                support::write_scip_protobuf_fixture(&root, "fixture.scip");
                support::attached_server_session(support::native_search_config(&root), &root)
            },
            |session| {
                let summary =
                    benchmark_precise_graph_for_server(&session.server, &session.repository_id)
                        .expect("cold precise graph benchmark should succeed");
                criterion::black_box(summary.precise_occurrence_count);
                criterion::black_box(summary.artifacts_ingested);
            },
            BatchSize::SmallInput,
        );
    });

    let hot_root = support::fresh_fixture_root("precise-graph-hot");
    support::write_scip_protobuf_fixture(&hot_root, "fixture.scip");
    let hot_session =
        support::attached_server_session(support::native_search_config(&hot_root), &hot_root);
    let warm_summary =
        benchmark_precise_graph_for_server(&hot_session.server, &hot_session.repository_id)
            .expect("precise graph warmup should succeed");
    criterion::black_box(warm_summary.precise_symbol_count);

    group.bench_function(BenchmarkId::from_parameter("warm_cached_reuse"), |b| {
        b.iter(|| {
            let summary =
                benchmark_precise_graph_for_server(&hot_session.server, &hot_session.repository_id)
                    .expect("warm precise graph benchmark should succeed");
            criterion::black_box(summary.reused_cache);
            criterion::black_box(summary.precise_occurrence_count);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_precise_graph);
criterion_main!(benches);
