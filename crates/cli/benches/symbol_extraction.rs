#[path = "common/mod.rs"]
mod support;

use criterion::{Criterion, criterion_group, criterion_main};
use frigg::indexer::extract_symbols_for_paths;

fn bench_symbol_extraction(c: &mut Criterion) {
    let root = support::fixture_root();
    let source_paths = support::manifest_source_paths(root);
    c.bench_function("symbol_extraction", |b| {
        b.iter(|| {
            let output = extract_symbols_for_paths(&source_paths);
            criterion::black_box(output.symbols.len());
        });
    });
}

criterion_group!(benches, bench_symbol_extraction);
criterion_main!(benches);
