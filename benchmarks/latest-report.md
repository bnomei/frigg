# Benchmark Latency Report

- report_version: `v1`
- criterion_roots: `target/criterion, crates/cli/target/criterion`
- budget_file: `benchmarks/budgets.v1.json`
- summary: pass=51 fail=0 missing=0

| workload | status | p50 (ms) | p95 (ms) | p99 (ms) | budget p50/p95/p99 (ms) |
| --- | --- | ---: | ---: | ---: | --- |
| `graph_hot_path_latency/precise_navigation/location-aware-selection` | pass | 0.266 | 0.283 | 0.299 | 8.00/20.00/30.00 |
| `graph_hot_path_latency/precise_references/hot-symbol-contention` | pass | 0.035 | 0.037 | 0.038 | 35.00/90.00/140.00 |
| `graph_hot_path_latency/relation_traversal/hot-fanout` | pass | 0.102 | 0.111 | 0.114 | 20.00/50.00/80.00 |
| `graph_hot_path_latency/scip_ingest/cold-cache` | pass | 0.463 | 0.487 | 0.493 | 80.00/180.00/280.00 |
| `graph_hot_path_latency/scip_ingest_protobuf/cold-cache` | pass | 0.557 | 0.605 | 0.613 | 90.00/200.00/320.00 |
| `mcp_tool_latency/deep_search_compose_citations/basic-playbook` | pass | 0.174 | 0.181 | 0.190 | 50.00/120.00/180.00 |
| `mcp_tool_latency/deep_search_replay/basic-playbook` | pass | 3.285 | 3.326 | 3.331 | 300.00/700.00/950.00 |
| `mcp_tool_latency/deep_search_run/basic-playbook` | pass | 3.336 | 3.532 | 3.584 | 260.00/600.00/850.00 |
| `mcp_tool_latency/document_symbols/single-rust-file` | pass | 0.243 | 0.316 | 0.323 | 90.00/220.00/320.00 |
| `mcp_tool_latency/explore/probe` | pass | 4.084 | 4.398 | 4.411 | 25.00/60.00/90.00 |
| `mcp_tool_latency/explore/refine` | pass | 3.620 | 3.759 | 4.211 | 25.00/60.00/90.00 |
| `mcp_tool_latency/explore/zoom` | pass | 2.550 | 2.748 | 3.122 | 20.00/45.00/70.00 |
| `mcp_tool_latency/find_declarations/precise-symbol` | pass | 0.112 | 0.115 | 0.117 | 140.00/300.00/420.00 |
| `mcp_tool_latency/find_implementations/precise-relationships` | pass | 2.939 | 3.053 | 3.121 | 160.00/320.00/460.00 |
| `mcp_tool_latency/find_references/heuristic` | pass | 1.511 | 1.550 | 1.556 | 120.00/260.00/360.00 |
| `mcp_tool_latency/find_references/precise` | pass | 1.510 | 1.529 | 1.555 | 150.00/300.00/420.00 |
| `mcp_tool_latency/go_to_definition/precise-symbol` | pass | 0.119 | 0.132 | 0.159 | 140.00/300.00/420.00 |
| `mcp_tool_latency/incoming_calls/precise-relationships` | pass | 2.927 | 3.060 | 3.108 | 160.00/320.00/460.00 |
| `mcp_tool_latency/list_repositories/default` | pass | 0.149 | 0.163 | 0.172 | 5.00/15.00/25.00 |
| `mcp_tool_latency/outgoing_calls/precise-relationships` | pass | 2.933 | 3.104 | 3.189 | 160.00/320.00/460.00 |
| `mcp_tool_latency/provenance_write_overhead/read-file-repeated-16x` | pass | 3.624 | 3.744 | 3.811 | 80.00/180.00/260.00 |
| `mcp_tool_latency/read_file/single-rust-file` | pass | 0.225 | 0.231 | 0.234 | 10.00/30.00/45.00 |
| `mcp_tool_latency/search_hybrid/semantic-degraded-missing-credentials` | pass | 0.141 | 0.148 | 0.148 | 30.00/75.00/110.00 |
| `mcp_tool_latency/search_hybrid/semantic-toggle-off` | pass | 0.158 | 0.175 | 0.253 | 25.00/60.00/90.00 |
| `mcp_tool_latency/search_structural/rust-function-scoped` | pass | 3.365 | 3.683 | 3.882 | 110.00/260.00/380.00 |
| `mcp_tool_latency/search_symbol/tree-sitter` | pass | 0.851 | 0.868 | 0.900 | 80.00/180.00/260.00 |
| `mcp_tool_latency/search_text/literal-scoped` | pass | 0.134 | 0.141 | 0.144 | 20.00/50.00/80.00 |
| `reindex_latency/reindex_repository/changed-only-delta` | pass | 11.121 | 11.657 | 11.676 | 60.00/130.00/200.00 |
| `reindex_latency/reindex_repository/changed-only-noop` | pass | 9.325 | 10.420 | 14.530 | 60.00/130.00/200.00 |
| `reindex_latency/reindex_repository/full-throughput` | pass | 16.022 | 18.858 | 21.213 | 60.00/120.00/180.00 |
| `search_latency/hybrid/benchmark-witness-recall` | pass | 3.878 | 5.359 | 10.902 | 420.00/650.00/750.00 |
| `search_latency/hybrid/graph-php-target-evidence` | pass | 1.647 | 1.902 | 1.935 | 30.00/45.00/60.00 |
| `search_latency/hybrid/path-witness-build-flow` | pass | 5.478 | 6.993 | 8.148 | 550.00/1100.00/1200.00 |
| `search_latency/hybrid/semantic-degraded-missing-credentials` | pass | 0.915 | 1.014 | 1.047 | 130.00/160.00/190.00 |
| `search_latency/hybrid/semantic-toggle-off` | pass | 0.889 | 1.217 | 1.246 | 130.00/160.00/190.00 |
| `search_latency/literal/global` | pass | 3.553 | 3.913 | 3.928 | 15.00/40.00/60.00 |
| `search_latency/literal/global-low-limit` | pass | 3.513 | 4.968 | 4.978 | 10.00/25.00/40.00 |
| `search_latency/literal/global-low-limit-high-cardinality` | pass | 3.642 | 4.843 | 5.378 | 12.00/30.00/45.00 |
| `search_latency/literal/indexed-manifest-low-limit-high-cardinality` | pass | 4.694 | 5.575 | 5.631 | 40.00/60.00/80.00 |
| `search_latency/literal/repo+path+lang` | pass | 1.092 | 1.334 | 1.543 | 10.00/25.00/40.00 |
| `search_latency/regex/global-no-hit-required-literal` | pass | 3.548 | 5.443 | 5.828 | 12.00/30.00/45.00 |
| `search_latency/regex/global-sparse-required-literal` | pass | 3.416 | 3.821 | 5.194 | 15.00/35.00/55.00 |
| `search_latency/regex/repo+path+lang` | pass | 1.190 | 1.290 | 1.356 | 20.00/55.00/80.00 |
| `semantic_chunk_hot_paths/file/markdown-heading-fanout` | pass | 0.041 | 0.042 | 0.042 | 1.00/3.00/5.00 |
| `semantic_chunk_hot_paths/file/rust-large-split` | pass | 0.100 | 0.102 | 0.103 | 2.00/5.00/8.00 |
| `semantic_chunk_hot_paths/manifest/mixed-language-batch` | pass | 2.447 | 2.502 | 2.562 | 60.00/90.00/120.00 |
| `storage_hot_path_latency/load_latest_manifest/cold-cache` | pass | 8.748 | 10.632 | 10.982 | 120.00/260.00/360.00 |
| `storage_hot_path_latency/manifest_upsert/hot-path-delta` | pass | 0.615 | 0.669 | 0.674 | 80.00/180.00/260.00 |
| `storage_hot_path_latency/provenance_query/hot-tool-contention` | pass | 0.172 | 0.194 | 0.208 | 30.00/70.00/110.00 |
| `storage_hot_path_latency/semantic_embedding_advance/hot-delta-batch` | pass | 3.663 | 4.749 | 4.790 | 80.00/160.00/240.00 |
| `storage_hot_path_latency/semantic_vector_topk/hot-query-batch` | pass | 2.712 | 3.507 | 3.792 | 20.00/40.00/60.00 |

## Search Stage Attribution

- source: `benchmarks/search-stage-attribution.latest.json`

| workload | candidate intake (ms) | freshness (ms) | scan (ms) | witness (ms) | graph (ms) | semantic (ms) | anchor blend (ms) | doc corroboration (ms) | final diversify (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `search_latency/hybrid/benchmark-witness-recall` | 0.104 | 1.751 | 0.881 | 0.601 | 0.000 | 0.000 | 0.013 | 0.005 | 0.104 |
| `search_latency/hybrid/graph-php-target-evidence` | 0.758 | 0.000 | 0.718 | 0.452 | 0.200 | 0.000 | 0.010 | 0.004 | 0.049 |
| `search_latency/hybrid/path-witness-build-flow` | 0.120 | 1.933 | 1.372 | 1.003 | 0.061 | 0.000 | 0.013 | 0.005 | 0.252 |
| `search_latency/hybrid/semantic-degraded-missing-credentials` | 0.734 | 0.000 | 0.092 | 0.000 | 0.000 | 0.000 | 0.005 | 0.003 | 0.014 |
| `search_latency/hybrid/semantic-toggle-off` | 0.769 | 0.000 | 0.089 | 0.000 | 0.000 | 0.000 | 0.005 | 0.003 | 0.015 |
