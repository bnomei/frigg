use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use frigg::domain::FriggError;
use frigg::storage::{ManifestEntry, Storage};
use serde_json::json;

const BENCH_REPOSITORY_ID: &str = "repo-001";
const BENCH_HOT_SNAPSHOT_ID: &str = "snapshot-hot-010";
const BENCH_COLD_SNAPSHOT_OLDER_ID: &str = "snapshot-cold-001";
const BENCH_COLD_SNAPSHOT_NEWER_ID: &str = "snapshot-cold-002";
const BENCH_PROVENANCE_TOOL_HOTSPOT: &str = "read_file";
const BENCH_OUTPUT_ROOT: &str = "target/criterion";

const WORKLOAD_MANIFEST_UPSERT: &str = "storage_hot_path_latency/manifest_upsert/hot-path-delta";
const WORKLOAD_PROVENANCE_QUERY: &str =
    "storage_hot_path_latency/provenance_query/hot-tool-contention";
const WORKLOAD_LOAD_LATEST_MANIFEST: &str =
    "storage_hot_path_latency/load_latest_manifest/cold-cache";

const BENCH_MANIFEST_ENTRY_COUNT: usize = 180;
const BENCH_PROVENANCE_ROW_COUNT: usize = 420;
const BENCH_PROVENANCE_QUERY_LIMIT: usize = 64;
const BENCH_SAMPLE_COUNT: usize = 30;
const BENCH_WARMUP_SAMPLES: usize = 5;

static BENCH_NONCE: AtomicU64 = AtomicU64::new(0);
static HOT_STATE: OnceLock<HotStorageState> = OnceLock::new();

fn main() {
    storage_hot_path_benchmarks();
}

fn storage_hot_path_benchmarks() {
    let hot_state = HOT_STATE.get_or_init(prepare_hot_state);
    assert_deterministic_hotspot_query(hot_state);
    assert_typed_invalid_input_contract(hot_state);

    run_workload(WORKLOAD_MANIFEST_UPSERT, BENCH_SAMPLE_COUNT, 1, || {
        hot_state
            .storage
            .upsert_manifest(
                BENCH_REPOSITORY_ID,
                BENCH_HOT_SNAPSHOT_ID,
                &hot_state.manifest_entries,
            )
            .expect("manifest hot-path benchmark upsert should succeed");
        let rows = hot_state
            .storage
            .load_manifest_for_snapshot(BENCH_HOT_SNAPSHOT_ID)
            .expect("manifest hot-path benchmark load should succeed");
        std::hint::black_box(rows);
    });

    run_workload(WORKLOAD_PROVENANCE_QUERY, BENCH_SAMPLE_COUNT, 6, || {
        let rows = hot_state
            .storage
            .load_provenance_events_for_tool(
                BENCH_PROVENANCE_TOOL_HOTSPOT,
                BENCH_PROVENANCE_QUERY_LIMIT,
            )
            .expect("provenance contention query benchmark should succeed");
        std::hint::black_box(rows);
    });

    run_workload(WORKLOAD_LOAD_LATEST_MANIFEST, BENCH_SAMPLE_COUNT, 1, || {
        let state = prepare_cold_cache_state("load-latest");
        let latest = state
            .storage
            .load_latest_manifest_for_repository(BENCH_REPOSITORY_ID)
            .expect("cold-cache latest manifest lookup should succeed")
            .expect("cold-cache state should include a manifest snapshot");
        assert_eq!(
            latest.snapshot_id, BENCH_COLD_SNAPSHOT_NEWER_ID,
            "cold-cache fixture should return deterministic latest snapshot"
        );
        std::hint::black_box(latest);
        state.cleanup();
    });
}

fn run_workload<F>(workload_id: &str, sample_count: usize, iters_per_sample: u64, mut workload: F)
where
    F: FnMut(),
{
    assert!(sample_count > 0, "sample_count must be greater than zero");
    assert!(
        iters_per_sample > 0,
        "iters_per_sample must be greater than zero"
    );

    for _ in 0..BENCH_WARMUP_SAMPLES {
        for _ in 0..iters_per_sample {
            workload();
        }
    }

    let mut iters = Vec::with_capacity(sample_count);
    let mut times = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let start = Instant::now();
        for _ in 0..iters_per_sample {
            workload();
        }
        let elapsed_ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        iters.push(iters_per_sample);
        times.push(elapsed_ns);
    }

    write_sample_file(workload_id, &iters, &times);
    println!("workload={workload_id} samples={sample_count} iters_per_sample={iters_per_sample}");
}

fn write_sample_file(workload_id: &str, iters: &[u64], times: &[u64]) {
    let sample_path = PathBuf::from(BENCH_OUTPUT_ROOT)
        .join(workload_id)
        .join("new")
        .join("sample.json");
    if let Some(parent) = sample_path.parent() {
        fs::create_dir_all(parent).expect("benchmark sample directory should be creatable");
    }

    let payload = json!({
        "iters": iters,
        "times": times,
    });
    let raw = serde_json::to_vec(&payload).expect("benchmark sample payload should serialize");
    fs::write(&sample_path, raw).expect("benchmark sample.json should be writable");
}

struct HotStorageState {
    storage: Storage,
    manifest_entries: Vec<ManifestEntry>,
}

fn prepare_hot_state() -> HotStorageState {
    let db_path = temp_db_path("hot-state");
    cleanup_db(&db_path);

    let storage = Storage::new(&db_path);
    storage
        .initialize()
        .expect("hot benchmark state should initialize sqlite schema");

    let manifest_entries = build_manifest_entries(BENCH_MANIFEST_ENTRY_COUNT);
    storage
        .upsert_manifest(
            BENCH_REPOSITORY_ID,
            BENCH_HOT_SNAPSHOT_ID,
            &manifest_entries,
        )
        .expect("hot benchmark state should seed manifest");

    seed_provenance_rows(&storage, BENCH_PROVENANCE_ROW_COUNT);

    HotStorageState {
        storage,
        manifest_entries,
    }
}

struct ColdCacheState {
    storage: Storage,
    db_path: PathBuf,
}

impl ColdCacheState {
    fn cleanup(self) {
        cleanup_db(&self.db_path);
    }
}

fn prepare_cold_cache_state(workload_id: &str) -> ColdCacheState {
    let db_path = temp_db_path(workload_id);
    cleanup_db(&db_path);

    let storage = Storage::new(&db_path);
    storage
        .initialize()
        .expect("cold-cache benchmark state should initialize sqlite schema");

    storage
        .upsert_manifest(
            BENCH_REPOSITORY_ID,
            BENCH_COLD_SNAPSHOT_OLDER_ID,
            &build_manifest_entries(48),
        )
        .expect("cold-cache benchmark state should seed older snapshot");
    storage
        .upsert_manifest(
            BENCH_REPOSITORY_ID,
            BENCH_COLD_SNAPSHOT_NEWER_ID,
            &build_manifest_entries(52),
        )
        .expect("cold-cache benchmark state should seed newer snapshot");

    ColdCacheState { storage, db_path }
}

fn build_manifest_entries(entry_count: usize) -> Vec<ManifestEntry> {
    let mut entries = Vec::with_capacity(entry_count);
    for idx in (0..entry_count).rev() {
        entries.push(ManifestEntry {
            path: format!("src/module_{:03}/file_{idx:04}.rs", idx % 24),
            sha256: format!("sha256-{idx:064x}"),
            size_bytes: ((idx + 1) * 128) as u64,
            mtime_ns: Some(1_000_000_000 + (idx as u64 * 10_000)),
        });
    }
    entries
}

fn seed_provenance_rows(storage: &Storage, row_count: usize) {
    for idx in 0..row_count {
        let tool_name = if idx % 9 == 0 {
            "search_text"
        } else {
            BENCH_PROVENANCE_TOOL_HOTSPOT
        };
        let trace_id = format!("trace-{tool_name}-{idx:05}");
        storage
            .append_provenance_event(
                &trace_id,
                tool_name,
                &json!({
                    "tool_name": tool_name,
                    "params": {
                        "path": format!("src/file_{:03}.rs", idx % 96),
                        "sequence": idx,
                    }
                }),
            )
            .expect("benchmark provenance fixture should append deterministically");
    }
}

fn assert_deterministic_hotspot_query(state: &HotStorageState) {
    let first = state
        .storage
        .load_provenance_events_for_tool(
            BENCH_PROVENANCE_TOOL_HOTSPOT,
            BENCH_PROVENANCE_QUERY_LIMIT,
        )
        .expect("hotspot query assertion should succeed");
    let second = state
        .storage
        .load_provenance_events_for_tool(
            BENCH_PROVENANCE_TOOL_HOTSPOT,
            BENCH_PROVENANCE_QUERY_LIMIT,
        )
        .expect("hotspot query assertion should be repeatable");
    assert_eq!(
        first, second,
        "repeated hotspot provenance queries must preserve deterministic ordering"
    );
    assert_eq!(
        first.len(),
        BENCH_PROVENANCE_QUERY_LIMIT,
        "hotspot provenance fixture should satisfy query limit exactly"
    );
}

fn assert_typed_invalid_input_contract(state: &HotStorageState) {
    let empty_trace = state
        .storage
        .append_provenance_event("", BENCH_PROVENANCE_TOOL_HOTSPOT, &json!({}))
        .expect_err("empty trace_id should keep typed invalid-input contract");
    assert!(
        matches!(
            empty_trace,
            FriggError::InvalidInput(ref message) if message == "trace_id must not be empty"
        ),
        "expected invalid_input error for empty trace id, got {empty_trace}"
    );

    let empty_tool = state
        .storage
        .load_provenance_events_for_tool("", 1)
        .expect_err("empty tool_name should keep typed invalid-input contract");
    assert!(
        matches!(
            empty_tool,
            FriggError::InvalidInput(ref message) if message == "tool_name must not be empty"
        ),
        "expected invalid_input error for empty tool name, got {empty_tool}"
    );
}

fn temp_db_path(workload_id: &str) -> PathBuf {
    let nonce = BENCH_NONCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "frigg-storage-bench-{workload_id}-{}-{nonce}.sqlite3",
        std::process::id()
    ))
}

fn cleanup_db(path: &PathBuf) {
    let _ = fs::remove_file(path);
}
