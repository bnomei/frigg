#[path = "common/mod.rs"]
mod support;

use criterion::{Criterion, criterion_group, criterion_main};
use frigg::mcp::benchmark_build_symbol_corpora_for_server;

fn bench_symbol_corpus(c: &mut Criterion) {
    let session = support::attached_fixture_server_session();
    c.bench_function("symbol_corpus", |b| {
        b.iter(|| {
            let summary = benchmark_build_symbol_corpora_for_server(
                &session.server,
                Some(&session.repository_id),
            )
            .expect("symbol corpus benchmark should succeed");
            criterion::black_box(summary.symbol_count);
        });
    });
}

criterion_group!(benches, bench_symbol_corpus);
criterion_main!(benches);
