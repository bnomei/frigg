use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use frigg::domain::FriggError;
use frigg::storage::{
    DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, SemanticChunkEmbeddingRecord, Storage,
};
use serde_json::json;

const BENCH_REPOSITORY_ID: &str = "repo-001";
const BENCH_HOT_SNAPSHOT_ID: &str = "snapshot-hot-010";
const BENCH_COLD_SNAPSHOT_OLDER_ID: &str = "snapshot-cold-001";
const BENCH_COLD_SNAPSHOT_NEWER_ID: &str = "snapshot-cold-002";
const BENCH_PROVENANCE_TOOL_HOTSPOT: &str = "read_file";
const BENCH_SEMANTIC_PREVIOUS_SNAPSHOT_ID: &str = "snapshot-semantic-009";
const BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID: &str = "snapshot-semantic-010";
const BENCH_SEMANTIC_PROVIDER: &str = "openai";
const BENCH_SEMANTIC_MODEL: &str = "text-embedding-3-small";
const BENCH_OUTPUT_ROOT: &str = "target/criterion";

const WORKLOAD_MANIFEST_UPSERT: &str = "storage_hot_path_latency/manifest_upsert/hot-path-delta";
const WORKLOAD_PROVENANCE_QUERY: &str =
    "storage_hot_path_latency/provenance_query/hot-tool-contention";
const WORKLOAD_LOAD_LATEST_MANIFEST: &str =
    "storage_hot_path_latency/load_latest_manifest/cold-cache";
const WORKLOAD_SEMANTIC_EMBEDDING_ADVANCE: &str =
    "storage_hot_path_latency/semantic_embedding_advance/hot-delta-batch";
const WORKLOAD_SEMANTIC_VECTOR_TOPK: &str =
    "storage_hot_path_latency/semantic_vector_topk/hot-query-batch";

const BENCH_MANIFEST_ENTRY_COUNT: usize = 180;
const BENCH_PROVENANCE_ROW_COUNT: usize = 420;
const BENCH_PROVENANCE_QUERY_LIMIT: usize = 64;
const BENCH_SEMANTIC_PATH_COUNT: usize = 48;
const BENCH_SEMANTIC_CHUNKS_PER_PATH: usize = 2;
const BENCH_SEMANTIC_CHANGED_PATH_COUNT: usize = 12;
const BENCH_SEMANTIC_DELETED_PATH_COUNT: usize = 6;
const BENCH_SEMANTIC_VECTOR_TOPK_LIMIT: usize = 8;
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
    assert_semantic_delta_is_deterministic(hot_state);
    assert_semantic_vector_topk_is_deterministic(hot_state);
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

    run_workload(
        WORKLOAD_SEMANTIC_EMBEDDING_ADVANCE,
        BENCH_SAMPLE_COUNT,
        1,
        || {
            hot_state
                .storage
                .advance_semantic_embeddings_for_repository(
                    BENCH_REPOSITORY_ID,
                    Some(BENCH_SEMANTIC_PREVIOUS_SNAPSHOT_ID),
                    BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
                    &hot_state.semantic_delta.changed_paths,
                    &hot_state.semantic_delta.deleted_paths,
                    &hot_state.semantic_delta.delta_records,
                )
                .expect("semantic embedding delta benchmark advance should succeed");
            let count = hot_state
                .storage
                .count_semantic_embeddings_for_repository_snapshot_model(
                    BENCH_REPOSITORY_ID,
                    BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
                    BENCH_SEMANTIC_PROVIDER,
                    BENCH_SEMANTIC_MODEL,
                )
                .expect("semantic embedding delta benchmark count should succeed");
            assert_eq!(
                count, hot_state.semantic_delta.expected_record_count,
                "semantic embedding delta benchmark should preserve deterministic record count"
            );
            let texts = hot_state
                .storage
                .load_semantic_chunk_texts_for_repository_snapshot(
                    BENCH_REPOSITORY_ID,
                    BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
                    &hot_state.semantic_delta.lookup_chunk_ids,
                )
                .expect("semantic embedding delta benchmark lookup should succeed");
            assert_eq!(
                texts.len(),
                hot_state.semantic_delta.lookup_chunk_ids.len(),
                "semantic embedding delta benchmark should roundtrip every lookup chunk"
            );
            std::hint::black_box((count, texts));
        },
    );

    run_workload(WORKLOAD_SEMANTIC_VECTOR_TOPK, BENCH_SAMPLE_COUNT, 1, || {
        let matches = hot_state
            .storage
            .load_semantic_vector_topk_for_repository_snapshot_model(
                BENCH_REPOSITORY_ID,
                BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
                BENCH_SEMANTIC_PROVIDER,
                BENCH_SEMANTIC_MODEL,
                &hot_state.semantic_delta.query_embedding,
                BENCH_SEMANTIC_VECTOR_TOPK_LIMIT,
                Some("rust"),
            )
            .expect("semantic vector top-k benchmark query should succeed");
        assert_eq!(
            matches.len(),
            BENCH_SEMANTIC_VECTOR_TOPK_LIMIT,
            "semantic vector top-k benchmark should honor the deterministic top-k limit"
        );
        assert_eq!(
            matches[0].chunk_id, hot_state.semantic_delta.expected_topk_first_chunk_id,
            "semantic vector top-k benchmark should preserve deterministic nearest-neighbor ordering"
        );

        let chunk_ids = matches
            .iter()
            .map(|entry| entry.chunk_id.clone())
            .collect::<Vec<_>>();
        let payloads = hot_state
            .storage
            .load_semantic_chunk_payloads_for_repository_snapshot(
                BENCH_REPOSITORY_ID,
                BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
                &chunk_ids,
            )
            .expect("semantic vector top-k benchmark payload load should succeed");
        assert_eq!(
            payloads.len(),
            chunk_ids.len(),
            "semantic vector top-k benchmark should batch-load one payload per retained hit"
        );
        std::hint::black_box((matches, payloads));
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
    semantic_delta: SemanticDeltaState,
}

struct SemanticDeltaState {
    changed_paths: Vec<String>,
    deleted_paths: Vec<String>,
    delta_records: Vec<SemanticChunkEmbeddingRecord>,
    lookup_chunk_ids: Vec<String>,
    expected_record_count: usize,
    query_embedding: Vec<f32>,
    expected_topk_first_chunk_id: String,
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
    let semantic_delta = seed_semantic_delta_state(&storage);

    let state = HotStorageState {
        storage,
        manifest_entries,
        semantic_delta,
    };
    apply_semantic_delta_and_assert(&state);
    state
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

fn seed_semantic_delta_state(storage: &Storage) -> SemanticDeltaState {
    let base_records = build_semantic_records(
        BENCH_SEMANTIC_PREVIOUS_SNAPSHOT_ID,
        0..BENCH_SEMANTIC_PATH_COUNT,
        false,
    );
    storage
        .replace_semantic_embeddings_for_repository(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_PREVIOUS_SNAPSHOT_ID,
            &base_records,
        )
        .expect("benchmark semantic fixture should seed base snapshot");

    let changed_paths = (0..BENCH_SEMANTIC_CHANGED_PATH_COUNT)
        .map(semantic_path)
        .collect::<Vec<_>>();
    let deleted_paths = (BENCH_SEMANTIC_CHANGED_PATH_COUNT
        ..BENCH_SEMANTIC_CHANGED_PATH_COUNT + BENCH_SEMANTIC_DELETED_PATH_COUNT)
        .map(semantic_path)
        .collect::<Vec<_>>();
    let delta_records = build_semantic_records(
        BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
        0..BENCH_SEMANTIC_CHANGED_PATH_COUNT,
        true,
    );
    let lookup_chunk_ids = delta_records
        .iter()
        .map(|record| record.chunk_id.clone())
        .collect::<Vec<_>>();
    let expected_record_count =
        base_records.len() - (BENCH_SEMANTIC_DELETED_PATH_COUNT * BENCH_SEMANTIC_CHUNKS_PER_PATH);
    let query_embedding = build_embedding(10_000);
    let expected_topk_first_chunk_id = "chunk-000-00".to_owned();

    SemanticDeltaState {
        changed_paths,
        deleted_paths,
        delta_records,
        lookup_chunk_ids,
        expected_record_count,
        query_embedding,
        expected_topk_first_chunk_id,
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

fn assert_semantic_delta_is_deterministic(state: &HotStorageState) {
    apply_semantic_delta_and_assert(state);
    let first_count = state
        .storage
        .count_semantic_embeddings_for_repository_snapshot_model(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            BENCH_SEMANTIC_PROVIDER,
            BENCH_SEMANTIC_MODEL,
        )
        .expect("semantic delta deterministic count should succeed");
    let first_texts = state
        .storage
        .load_semantic_chunk_texts_for_repository_snapshot(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            &state.semantic_delta.lookup_chunk_ids,
        )
        .expect("semantic delta deterministic lookup should succeed");

    apply_semantic_delta_and_assert(state);
    let second_count = state
        .storage
        .count_semantic_embeddings_for_repository_snapshot_model(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            BENCH_SEMANTIC_PROVIDER,
            BENCH_SEMANTIC_MODEL,
        )
        .expect("semantic delta repeated count should succeed");
    let second_texts = state
        .storage
        .load_semantic_chunk_texts_for_repository_snapshot(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            &state.semantic_delta.lookup_chunk_ids,
        )
        .expect("semantic delta repeated lookup should succeed");

    assert_eq!(
        first_count, second_count,
        "semantic delta count must remain deterministic across repeated advances"
    );
    assert_eq!(
        first_texts, second_texts,
        "semantic delta lookup payloads must remain deterministic across repeated advances"
    );
}

fn assert_semantic_vector_topk_is_deterministic(state: &HotStorageState) {
    let first = state
        .storage
        .load_semantic_vector_topk_for_repository_snapshot_model(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            BENCH_SEMANTIC_PROVIDER,
            BENCH_SEMANTIC_MODEL,
            &state.semantic_delta.query_embedding,
            BENCH_SEMANTIC_VECTOR_TOPK_LIMIT,
            Some("rust"),
        )
        .expect("semantic vector top-k deterministic assertion should succeed");
    let second = state
        .storage
        .load_semantic_vector_topk_for_repository_snapshot_model(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            BENCH_SEMANTIC_PROVIDER,
            BENCH_SEMANTIC_MODEL,
            &state.semantic_delta.query_embedding,
            BENCH_SEMANTIC_VECTOR_TOPK_LIMIT,
            Some("rust"),
        )
        .expect("semantic vector top-k repeated assertion should succeed");
    assert_eq!(
        first, second,
        "semantic vector top-k queries must preserve deterministic ordering"
    );
    assert_eq!(
        first.len(),
        BENCH_SEMANTIC_VECTOR_TOPK_LIMIT,
        "semantic vector top-k assertion should honor the deterministic top-k limit"
    );
    assert_eq!(
        first[0].chunk_id, state.semantic_delta.expected_topk_first_chunk_id,
        "semantic vector top-k assertion should return the nearest deterministic chunk first"
    );

    let chunk_ids = first
        .iter()
        .map(|entry| entry.chunk_id.clone())
        .collect::<Vec<_>>();
    let payloads = state
        .storage
        .load_semantic_chunk_payloads_for_repository_snapshot(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            &chunk_ids,
        )
        .expect("semantic vector top-k payload assertion should succeed");
    assert_eq!(
        payloads.len(),
        chunk_ids.len(),
        "semantic vector top-k payload assertion should materialize one payload per hit"
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

    let empty_provider = state
        .storage
        .count_semantic_embeddings_for_repository_snapshot_model(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            "",
            BENCH_SEMANTIC_MODEL,
        )
        .expect_err("empty provider should keep typed invalid-input contract");
    assert!(
        matches!(
            empty_provider,
            FriggError::InvalidInput(ref message) if message == "provider must not be empty"
        ),
        "expected invalid_input error for empty semantic provider, got {empty_provider}"
    );
}

fn apply_semantic_delta_and_assert(state: &HotStorageState) {
    state
        .storage
        .advance_semantic_embeddings_for_repository(
            BENCH_REPOSITORY_ID,
            Some(BENCH_SEMANTIC_PREVIOUS_SNAPSHOT_ID),
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            &state.semantic_delta.changed_paths,
            &state.semantic_delta.deleted_paths,
            &state.semantic_delta.delta_records,
        )
        .expect("semantic delta assertion advance should succeed");

    let count = state
        .storage
        .count_semantic_embeddings_for_repository_snapshot_model(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            BENCH_SEMANTIC_PROVIDER,
            BENCH_SEMANTIC_MODEL,
        )
        .expect("semantic delta assertion count should succeed");
    assert_eq!(
        count, state.semantic_delta.expected_record_count,
        "semantic delta assertion should preserve deterministic record count"
    );

    let texts = state
        .storage
        .load_semantic_chunk_texts_for_repository_snapshot(
            BENCH_REPOSITORY_ID,
            BENCH_SEMANTIC_CURRENT_SNAPSHOT_ID,
            &state.semantic_delta.lookup_chunk_ids,
        )
        .expect("semantic delta assertion lookup should succeed");
    assert_eq!(
        texts.len(),
        state.semantic_delta.lookup_chunk_ids.len(),
        "semantic delta assertion should return one text per lookup chunk"
    );
    assert!(
        texts
            .values()
            .all(|text| text.contains("delta semantic chunk") && text.contains("benchmark")),
        "semantic delta assertion should roundtrip delta payload text deterministically: {texts:?}"
    );
}

fn build_semantic_records(
    snapshot_id: &str,
    path_indices: std::ops::Range<usize>,
    delta_variant: bool,
) -> Vec<SemanticChunkEmbeddingRecord> {
    let mut records = Vec::with_capacity(path_indices.len() * BENCH_SEMANTIC_CHUNKS_PER_PATH);
    for path_idx in path_indices {
        for chunk_idx in 0..BENCH_SEMANTIC_CHUNKS_PER_PATH {
            let path = semantic_path(path_idx);
            let chunk_id = format!("chunk-{path_idx:03}-{chunk_idx:02}");
            let variant_offset = if delta_variant { 10_000 } else { 0 };
            let content_text = if delta_variant {
                format!(
                    "delta semantic chunk path={path_idx} chunk={chunk_idx} benchmark hotspot payload"
                )
            } else {
                format!("base semantic chunk path={path_idx} chunk={chunk_idx} benchmark payload")
            };
            records.push(SemanticChunkEmbeddingRecord {
                chunk_id: chunk_id.clone(),
                repository_id: BENCH_REPOSITORY_ID.to_owned(),
                snapshot_id: snapshot_id.to_owned(),
                path,
                language: "rust".to_owned(),
                chunk_index: chunk_idx,
                start_line: (chunk_idx * 24) + 1,
                end_line: (chunk_idx * 24) + 18,
                provider: BENCH_SEMANTIC_PROVIDER.to_owned(),
                model: BENCH_SEMANTIC_MODEL.to_owned(),
                trace_id: Some(format!("trace-semantic-{snapshot_id}-{path_idx:03}")),
                content_hash_blake3: format!(
                    "blake3-{:064x}",
                    variant_offset + (path_idx * 64) + chunk_idx
                ),
                content_text,
                embedding: build_embedding(variant_offset + (path_idx * 64) + chunk_idx),
            });
        }
    }
    records
}

fn semantic_path(path_idx: usize) -> String {
    format!(
        "src/semantic/module_{:03}/chunk_{path_idx:03}.rs",
        path_idx % 12
    )
}

fn build_embedding(seed: usize) -> Vec<f32> {
    let mut embedding = Vec::with_capacity(DEFAULT_VECTOR_DIMENSIONS);
    for dimension in 0..DEFAULT_VECTOR_DIMENSIONS {
        let value = (((seed + 1) * 31) + (dimension * 17)) % 1024;
        embedding.push(value as f32 / 1024.0);
    }
    embedding
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
