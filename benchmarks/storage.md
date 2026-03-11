# Storage Benchmark Methodology (`v1`)

## Scope

This benchmark plan tracks latency for `storage` crate hot paths in `Storage`:

- manifest upsert/read roundtrips
- provenance query for high-contention tool-name workloads
- cold-cache latest-manifest lookups after deterministic initialization
- semantic embedding delta advance and chunk-text lookup over deterministic vector batches
- semantic sqlite-vec top-k retrieval plus batched payload loading over deterministic local embeddings

Harness location:

- `crates/cli/benches/storage_hot_paths.rs`

Run command:

```bash
cargo bench -p frigg --bench storage_hot_paths -- --noplot
```

## Reproducibility Model

The harness is deterministic by construction:

- fixed repository id (`repo-001`)
- fixed manifest entry counts and deterministic path/hash/timestamp generation
- fixed provenance row volume and hotspot tool distribution
- fixed cold-cache snapshot ids and ordering expectations
- repeated-input deterministic assertions before measurement

## Workloads

1. `manifest_upsert/hot-path-delta`
- purpose: benchmark manifest upsert on a delta-sized deterministic snapshot plus roundtrip load.

2. `provenance_query/hot-tool-contention`
- purpose: benchmark deterministic provenance query latency for a high-cardinality hotspot tool name.

3. `load_latest_manifest/cold-cache`
- purpose: benchmark latest-manifest lookup from cold initialized storage state each iteration.

4. `semantic_embedding_advance/hot-delta-batch`
- purpose: benchmark hot semantic embedding delta advancement plus chunk-text lookup over deterministic 1536-dimension vectors and changed/deleted path batches.

5. `semantic_vector_topk/hot-query-batch`
- purpose: benchmark the healthy local sqlite-vec semantic retrieval path, including top-k neighbor selection and one bounded payload batch lookup for retained chunk ids.

## Budget Targets

Budgets are defined in the canonical budget source:

- `benchmarks/budgets.v1.json`

Current storage targets (ms):

- `manifest_upsert/hot-path-delta`: p50 <= 80, p95 <= 180, p99 <= 260
- `provenance_query/hot-tool-contention`: p50 <= 30, p95 <= 70, p99 <= 110
- `load_latest_manifest/cold-cache`: p50 <= 120, p95 <= 260, p99 <= 360
- `semantic_embedding_advance/hot-delta-batch`: p50 <= 80, p95 <= 160, p99 <= 240
- `semantic_vector_topk/hot-query-batch`: p50 <= 20, p95 <= 40, p99 <= 60

## Reporting

Generate consolidated benchmark report output:

```bash
python3 benchmarks/generate_latency_report.py
```
