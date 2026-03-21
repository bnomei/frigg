#[path = "common/mod.rs"]
mod support;

use criterion::{Criterion, criterion_group, criterion_main};
use frigg::indexer::ManifestBuilder;

fn bench_manifest_build(c: &mut Criterion) {
    let root = support::fixture_root();
    c.bench_function("manifest_build", |b| {
        b.iter(|| {
            let output = ManifestBuilder::default()
                .build_metadata_with_diagnostics(root)
                .expect("manifest build benchmark should succeed");
            criterion::black_box(output.entries.len());
        });
    });
}

criterion_group!(benches, bench_manifest_build);
criterion_main!(benches);
