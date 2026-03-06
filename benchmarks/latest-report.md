# Benchmark Latency Report

- report_version: `v1`
- criterion_roots: `target/criterion, crates/cli/target/criterion`
- budget_file: `benchmarks/budgets.v1.json`
- summary: pass=37 fail=0 missing=0

| workload | status | p50 (ms) | p95 (ms) | p99 (ms) | budget p50/p95/p99 (ms) |
| --- | --- | ---: | ---: | ---: | --- |
| `graph_hot_path_latency/precise_references/hot-symbol-contention` | pass | 0.020 | 0.027 | 0.031 | 35.00/90.00/140.00 |
| `graph_hot_path_latency/relation_traversal/hot-fanout` | pass | 0.105 | 0.145 | 0.252 | 20.00/50.00/80.00 |
| `graph_hot_path_latency/scip_ingest/cold-cache` | pass | 0.367 | 0.439 | 0.500 | 80.00/180.00/280.00 |
| `mcp_tool_latency/deep_search_compose_citations/basic-playbook` | pass | 2.028 | 4.253 | 5.474 | 50.00/120.00/180.00 |
| `mcp_tool_latency/deep_search_replay/basic-playbook` | pass | 12.168 | 25.438 | 26.382 | 300.00/700.00/950.00 |
| `mcp_tool_latency/deep_search_run/basic-playbook` | pass | 13.122 | 25.261 | 32.533 | 260.00/600.00/850.00 |
| `mcp_tool_latency/document_symbols/single-rust-file` | pass | 4.592 | 6.473 | 8.201 | 90.00/220.00/320.00 |
| `mcp_tool_latency/find_declarations/precise-symbol` | pass | 2.840 | 4.647 | 4.825 | 140.00/300.00/420.00 |
| `mcp_tool_latency/find_implementations/precise-relationships` | pass | 2.255 | 3.554 | 5.064 | 160.00/320.00/460.00 |
| `mcp_tool_latency/find_references/heuristic` | pass | 4.305 | 7.057 | 13.381 | 120.00/260.00/360.00 |
| `mcp_tool_latency/find_references/precise` | pass | 2.199 | 2.974 | 3.345 | 150.00/300.00/420.00 |
| `mcp_tool_latency/go_to_definition/precise-symbol` | pass | 2.080 | 2.531 | 3.843 | 140.00/300.00/420.00 |
| `mcp_tool_latency/incoming_calls/precise-relationships` | pass | 5.743 | 8.276 | 8.815 | 160.00/320.00/460.00 |
| `mcp_tool_latency/list_repositories/default` | pass | 1.296 | 3.911 | 4.083 | 5.00/15.00/25.00 |
| `mcp_tool_latency/outgoing_calls/precise-relationships` | pass | 5.315 | 10.294 | 10.552 | 160.00/320.00/460.00 |
| `mcp_tool_latency/provenance_write_overhead/read-file-repeated-16x` | pass | 28.782 | 62.031 | 73.226 | 80.00/180.00/260.00 |
| `mcp_tool_latency/read_file/single-rust-file` | pass | 1.264 | 1.758 | 1.817 | 10.00/30.00/45.00 |
| `mcp_tool_latency/search_hybrid/semantic-degraded-missing-credentials` | pass | 4.237 | 7.381 | 8.451 | 30.00/75.00/110.00 |
| `mcp_tool_latency/search_hybrid/semantic-toggle-off` | pass | 8.462 | 12.458 | 12.489 | 25.00/60.00/90.00 |
| `mcp_tool_latency/search_structural/rust-function-scoped` | pass | 9.962 | 20.393 | 24.767 | 110.00/260.00/380.00 |
| `mcp_tool_latency/search_symbol/tree-sitter` | pass | 1.983 | 2.419 | 2.724 | 80.00/180.00/260.00 |
| `mcp_tool_latency/search_text/literal-scoped` | pass | 3.743 | 5.266 | 5.468 | 20.00/50.00/80.00 |
| `reindex_latency/reindex_repository/changed-only-delta` | pass | 17.396 | 22.971 | 23.026 | 60.00/130.00/200.00 |
| `reindex_latency/reindex_repository/changed-only-noop` | pass | 14.817 | 27.354 | 33.605 | 60.00/130.00/200.00 |
| `reindex_latency/reindex_repository/full-throughput` | pass | 20.306 | 36.750 | 45.324 | 60.00/120.00/180.00 |
| `search_latency/hybrid/semantic-degraded-missing-credentials` | pass | 3.768 | 4.890 | 6.633 | 18.00/40.00/60.00 |
| `search_latency/hybrid/semantic-toggle-off` | pass | 3.507 | 4.553 | 5.014 | 15.00/35.00/55.00 |
| `search_latency/literal/global` | pass | 3.452 | 4.603 | 7.190 | 15.00/40.00/60.00 |
| `search_latency/literal/global-low-limit` | pass | 7.126 | 15.512 | 16.355 | 10.00/25.00/40.00 |
| `search_latency/literal/global-low-limit-high-cardinality` | pass | 4.605 | 8.101 | 9.408 | 12.00/30.00/45.00 |
| `search_latency/literal/repo+path+lang` | pass | 1.046 | 1.481 | 2.178 | 10.00/25.00/40.00 |
| `search_latency/regex/global-no-hit-required-literal` | pass | 3.302 | 5.236 | 8.148 | 12.00/30.00/45.00 |
| `search_latency/regex/global-sparse-required-literal` | pass | 3.784 | 8.624 | 11.225 | 15.00/35.00/55.00 |
| `search_latency/regex/repo+path+lang` | pass | 1.187 | 2.096 | 2.600 | 20.00/55.00/80.00 |
| `storage_hot_path_latency/load_latest_manifest/cold-cache` | pass | 7.631 | 9.646 | 9.963 | 120.00/260.00/360.00 |
| `storage_hot_path_latency/manifest_upsert/hot-path-delta` | pass | 2.166 | 2.719 | 2.977 | 80.00/180.00/260.00 |
| `storage_hot_path_latency/provenance_query/hot-tool-contention` | pass | 0.915 | 1.329 | 2.668 | 30.00/70.00/110.00 |
