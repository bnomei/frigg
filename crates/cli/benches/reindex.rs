#[path = "common/mod.rs"]
mod support;

use std::cell::Cell;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::indexer::{
    ReindexMode, reindex_repository_with_runtime_config,
    reindex_repository_with_runtime_config_and_dirty_paths,
};

fn bench_reindex(c: &mut Criterion) {
    let mut group = c.benchmark_group("reindex");

    group.bench_function(BenchmarkId::from_parameter("full"), |b| {
        b.iter_batched(
            || {
                let root = support::fresh_fixture_root("reindex-full");
                let config = support::native_search_config(&root);
                let db_path = support::benchmark_db_path(&root);
                (root, config, db_path)
            },
            |(root, config, db_path)| {
                let summary = reindex_repository_with_runtime_config(
                    "repo-001",
                    &root,
                    &db_path,
                    ReindexMode::Full,
                    &config.semantic_runtime,
                    &support::semantic_runtime_credentials(),
                )
                .expect("full reindex benchmark should succeed");
                criterion::black_box(summary.files_scanned);
            },
            BatchSize::SmallInput,
        );
    });

    let changed_root = support::fresh_fixture_root("reindex-changed");
    let changed_config = support::native_search_config(&changed_root);
    let changed_db_path = support::benchmark_db_path(&changed_root);
    reindex_repository_with_runtime_config(
        "repo-001",
        &changed_root,
        &changed_db_path,
        ReindexMode::Full,
        &changed_config.semantic_runtime,
        &support::semantic_runtime_credentials(),
    )
    .expect("changed-only reindex benchmark warmup should succeed");
    let hot_path = changed_root.join("src/module_000.rs");
    let revision = Cell::new(0usize);

    group.bench_function(BenchmarkId::from_parameter("changed_only_small_dirty_set"), |b| {
        b.iter(|| {
            let next_revision = revision.get().saturating_add(1);
            revision.set(next_revision);
            support::rewrite_file_with_new_mtime(
                &hot_path,
                &format!(
                    "pub struct Widget0;\n\
                     impl Widget0 {{\n\
                         pub fn handle_checkout_request(&self, user_id: usize) -> usize {{ user_id + {next_revision} }}\n\
                     }}\n"
                ),
            );
            let summary = reindex_repository_with_runtime_config_and_dirty_paths(
                "repo-001",
                &changed_root,
                &changed_db_path,
                ReindexMode::ChangedOnly,
                &changed_config.semantic_runtime,
                &support::semantic_runtime_credentials(),
                std::slice::from_ref(&hot_path),
            )
            .expect("changed-only reindex benchmark should succeed");
            criterion::black_box(summary.files_changed);
            criterion::black_box(summary.duration_ms);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_reindex);
criterion_main!(benches);
