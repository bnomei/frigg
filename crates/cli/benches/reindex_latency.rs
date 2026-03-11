use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::indexer::{ReindexMode, reindex_repository};

const BENCH_REPOSITORY_ID: &str = "repo-001";
const BENCH_FILE_COUNT: usize = 120;
const BENCH_LINES_PER_FILE: usize = 48;
const BENCH_MODIFIED_FILE_COUNT: usize = 12;
const BENCH_ADDED_RELATIVE_PATH: &str = "src/module_99/new_added.rs";

static BENCH_NONCE: AtomicU64 = AtomicU64::new(0);

fn reindex_latency_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("reindex_latency");
    group.sample_size(20);

    group.bench_function(
        BenchmarkId::new("reindex_repository", "full-throughput"),
        |b| {
            b.iter_batched(
                || prepare_state("full-throughput"),
                |state| {
                    let summary = reindex_repository(
                        BENCH_REPOSITORY_ID,
                        &state.workspace_root,
                        &state.db_path,
                        ReindexMode::Full,
                    )
                    .expect("full reindex benchmark should not fail");
                    assert_eq!(
                        summary.files_scanned, BENCH_FILE_COUNT,
                        "full reindex benchmark fixture should scan every file"
                    );
                    assert_eq!(
                        summary.files_changed, BENCH_FILE_COUNT,
                        "full reindex benchmark fixture should mark all files changed"
                    );
                    assert_eq!(
                        summary.files_deleted, 0,
                        "full reindex benchmark fixture should not report deletions"
                    );
                    criterion::black_box(summary);
                },
                BatchSize::SmallInput,
            );
        },
    );

    group.bench_function(
        BenchmarkId::new("reindex_repository", "changed-only-noop"),
        |b| {
            b.iter_batched(
                prepare_changed_only_noop_state,
                |state| {
                    let summary = reindex_repository(
                        BENCH_REPOSITORY_ID,
                        &state.workspace_root,
                        &state.db_path,
                        ReindexMode::ChangedOnly,
                    )
                    .expect("changed-only no-op benchmark should not fail");
                    assert_eq!(
                        summary.files_scanned, BENCH_FILE_COUNT,
                        "changed-only no-op benchmark should still metadata-scan the fixture"
                    );
                    assert_eq!(
                        summary.files_changed, 0,
                        "changed-only no-op benchmark should detect zero changed files"
                    );
                    assert_eq!(
                        summary.files_deleted, 0,
                        "changed-only no-op benchmark should detect zero deleted files"
                    );
                    criterion::black_box(summary);
                },
                BatchSize::SmallInput,
            );
        },
    );

    group.bench_function(
        BenchmarkId::new("reindex_repository", "changed-only-delta"),
        |b| {
            b.iter_batched(
                prepare_changed_only_delta_state,
                |state| {
                    let summary = reindex_repository(
                        BENCH_REPOSITORY_ID,
                        &state.workspace_root,
                        &state.db_path,
                        ReindexMode::ChangedOnly,
                    )
                    .expect("changed-only delta benchmark should not fail");
                    assert_eq!(
                        summary.files_scanned, BENCH_FILE_COUNT,
                        "changed-only delta benchmark should metadata-scan the deterministic fixture size"
                    );
                    assert_eq!(
                        summary.files_changed,
                        BENCH_MODIFIED_FILE_COUNT + 1,
                        "changed-only delta benchmark should include modified + added files"
                    );
                    assert_eq!(
                        summary.files_deleted, 1,
                        "changed-only delta benchmark should detect a deterministic deletion"
                    );
                    criterion::black_box(summary);
                },
                BatchSize::SmallInput,
            );
        },
    );

    group.finish();
}

struct BenchState {
    workspace_root: PathBuf,
    db_path: PathBuf,
}

impl BenchState {
    fn new(workload_id: &str) -> Self {
        let nonce = BENCH_NONCE.fetch_add(1, Ordering::Relaxed);
        let workspace_root = std::env::temp_dir().join(format!(
            "frigg-index-bench-workspace-{workload_id}-{}-{nonce}",
            std::process::id()
        ));
        let db_path = std::env::temp_dir().join(format!(
            "frigg-index-bench-db-{workload_id}-{}-{nonce}.sqlite3",
            std::process::id()
        ));

        if workspace_root.exists() {
            let _ = fs::remove_dir_all(&workspace_root);
        }
        let _ = fs::remove_file(&db_path);

        populate_workspace_fixture(&workspace_root);
        Self {
            workspace_root,
            db_path,
        }
    }
}

impl Drop for BenchState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.workspace_root);
        let _ = fs::remove_file(&self.db_path);
    }
}

fn prepare_state(workload_id: &str) -> BenchState {
    BenchState::new(workload_id)
}

fn prepare_changed_only_noop_state() -> BenchState {
    let state = prepare_state("changed-only-noop");
    let summary = reindex_repository(
        BENCH_REPOSITORY_ID,
        &state.workspace_root,
        &state.db_path,
        ReindexMode::Full,
    )
    .expect("changed-only no-op setup full reindex should not fail");
    assert_eq!(
        summary.files_scanned, BENCH_FILE_COUNT,
        "changed-only no-op setup should index the full fixture"
    );
    state
}

fn prepare_changed_only_delta_state() -> BenchState {
    let state = prepare_state("changed-only-delta");
    let summary = reindex_repository(
        BENCH_REPOSITORY_ID,
        &state.workspace_root,
        &state.db_path,
        ReindexMode::Full,
    )
    .expect("changed-only delta setup full reindex should not fail");
    assert_eq!(
        summary.files_scanned, BENCH_FILE_COUNT,
        "changed-only delta setup should index the full fixture"
    );
    apply_workspace_delta(&state.workspace_root);
    state
}

fn populate_workspace_fixture(root: &Path) {
    fs::create_dir_all(root).expect("benchmark fixture root should be creatable");

    for file_idx in 0..BENCH_FILE_COUNT {
        let relative_path = file_relative_path(file_idx);
        let file_path = root.join(relative_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .expect("benchmark fixture parent directory should be creatable");
        }
        fs::write(&file_path, build_file_content(file_idx))
            .expect("benchmark fixture file should be writable");
    }
}

fn apply_workspace_delta(root: &Path) {
    for file_idx in 0..BENCH_MODIFIED_FILE_COUNT {
        let path = root.join(file_relative_path(file_idx));
        let mut content =
            fs::read_to_string(&path).expect("benchmark delta file should be readable for update");
        content.push_str(&format!(
            "\n// deterministic changed-only delta marker {file_idx}\n"
        ));
        fs::write(&path, content).expect("benchmark delta file should be writable");
    }

    let deleted_path = root.join(file_relative_path(BENCH_FILE_COUNT - 1));
    fs::remove_file(&deleted_path).expect("benchmark delta should remove deterministic file");

    let added_path = root.join(BENCH_ADDED_RELATIVE_PATH);
    if let Some(parent) = added_path.parent() {
        fs::create_dir_all(parent).expect("benchmark added file parent should be creatable");
    }
    fs::write(
        &added_path,
        "pub fn bench_added_file() -> usize { 99 }\n// deterministic delta addition\n",
    )
    .expect("benchmark delta added file should be writable");
}

fn file_relative_path(file_idx: usize) -> PathBuf {
    PathBuf::from(format!(
        "src/module_{:02}/file_{file_idx:03}.rs",
        file_idx % 8
    ))
}

fn build_file_content(file_idx: usize) -> String {
    let mut lines = Vec::with_capacity(BENCH_LINES_PER_FILE + 2);
    lines.push(format!("pub struct Entity{file_idx:03};"));
    lines.push(format!(
        "pub fn make_{file_idx:03}() -> Entity{file_idx:03} {{ Entity{file_idx:03} }}"
    ));
    for line_idx in 0..BENCH_LINES_PER_FILE {
        lines.push(format!(
            "pub fn marker_{file_idx:03}_{line_idx:03}() -> usize {{ {} }}",
            file_idx + line_idx
        ));
    }
    lines.join("\n")
}

criterion_group!(
    name = benches;
    config = Criterion::default().configure_from_args();
    targets = reindex_latency_benchmarks
);
criterion_main!(benches);
