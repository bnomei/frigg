# MCP Tool Benchmark Methodology (`v1`)

## Scope

This benchmark plan tracks MCP tool-call latency for:

- core read-only MCP runtime tools
- feature-gated extended MCP runtime tools (`extended` profile)

- `list_repositories`
- `read_file`
- `search_text`
- `search_hybrid`
- `search_symbol`
- `find_references`
- `go_to_definition`
- `find_declarations`
- `find_implementations`
- `incoming_calls`
- `outgoing_calls`
- `document_symbols`
- `search_structural`
- `explore` (`extended` profile only)
- `deep_search_run` (`extended` profile only)
- `deep_search_replay` (`extended` profile only)
- `deep_search_compose_citations` (`extended` profile only)

Harness location:

- `crates/cli/benches/tool_latency.rs`

Run command:

```bash
cargo bench -p frigg --bench tool_latency
```

## Reproducibility Model

The harness is deterministic by construction:

- fixed fixture size (`BENCH_FILES`, `BENCH_LINES_PER_FILE`)
- fixed file naming (`src/file_{idx}.rs`)
- fixed symbol/reference patterns in file contents
- fixed precise-reference fixture (`fixtures/scip/mcp-bench-precise-references.json`)
- fixed query payloads and limits per tool workload
- fixed deep-search playbook payload (`BENCH_DEEP_SEARCH_PLAYBOOK_ID`)
- stable repository scope (`repo-001`)

## Workloads

1. `list_repositories/default`
- purpose: baseline MCP control-path latency

2. `read_file/single-rust-file`
- purpose: direct file read path latency

3. `search_text/literal-scoped`
- purpose: text search latency through MCP wrapper

4. `explore/probe`
- purpose: single-artifact streaming probe latency via MCP

5. `explore/zoom`
- purpose: single-artifact bounded window extraction latency via MCP

6. `explore/refine`
- purpose: single-artifact anchor-scoped search latency via MCP

7. `search_symbol/tree-sitter`
- purpose: symbol extraction + indexed exact/prefix ranking latency via MCP, with bounded infix fallback only when higher-ranked buckets underfill the limit

8. `find_references/heuristic`
- purpose: heuristic reference resolution latency via MCP

9. `find_references/precise`
- purpose: precise SCIP-backed reference resolution latency via MCP

10. `go_to_definition/precise-symbol`
- purpose: precise symbol definition resolution latency via MCP

11. `find_declarations/precise-symbol`
- purpose: precise declaration-anchor (v1 definition-anchor) latency via MCP

12. `find_implementations/precise-relationships`
- purpose: precise implementation relationship traversal latency via MCP

13. `incoming_calls/precise-relationships`
- purpose: precise incoming call-hierarchy traversal latency via MCP

14. `outgoing_calls/precise-relationships`
- purpose: precise outgoing call-hierarchy traversal latency via MCP

15. `document_symbols/single-rust-file`
- purpose: deterministic per-file symbol-outline extraction latency via MCP for in-budget Rust/PHP files; over-budget requests are excluded because they now fail before whole-file reads

16. `search_structural/rust-function-scoped`
- purpose: deterministic Rust tree-sitter structural search latency via MCP

17. `deep_search_run/basic-playbook`
- purpose: deterministic deep-search playbook execution latency through MCP runtime handler

18. `search_hybrid/semantic-toggle-off`
- purpose: deterministic MCP hybrid retrieval path with semantic channel explicitly disabled by request toggle

19. `search_hybrid/semantic-degraded-missing-credentials`
- purpose: deterministic MCP hybrid retrieval fallback path where semantic channel degrades from semantic startup-validation failure in non-strict mode

20. `deep_search_replay/basic-playbook`
- purpose: deterministic deep-search replay + diff latency through MCP runtime handler

21. `deep_search_compose_citations/basic-playbook`
- purpose: deterministic deep-search citation payload composition latency through MCP runtime handler

22. `provenance_write_overhead/read-file-repeated-16x`
- purpose: repeated `read_file` path including deterministic provenance write overhead

## Budget Targets

Budgets are defined in the canonical budget source:

- `benchmarks/budgets.v1.json`

Current MCP targets (ms):

- `list_repositories/default`: p50 <= 5, p95 <= 15, p99 <= 25
- `read_file/single-rust-file`: p50 <= 10, p95 <= 30, p99 <= 45
- `search_text/literal-scoped`: p50 <= 20, p95 <= 50, p99 <= 80
- `explore/probe`: p50 <= 25, p95 <= 60, p99 <= 90
- `explore/zoom`: p50 <= 20, p95 <= 45, p99 <= 70
- `explore/refine`: p50 <= 25, p95 <= 60, p99 <= 90
- `search_symbol/tree-sitter`: p50 <= 80, p95 <= 180, p99 <= 260
- `find_references/heuristic`: p50 <= 120, p95 <= 260, p99 <= 360
- `find_references/precise`: p50 <= 150, p95 <= 300, p99 <= 420
- `go_to_definition/precise-symbol`: p50 <= 140, p95 <= 300, p99 <= 420
- `find_declarations/precise-symbol`: p50 <= 140, p95 <= 300, p99 <= 420
- `find_implementations/precise-relationships`: p50 <= 160, p95 <= 320, p99 <= 460
- `incoming_calls/precise-relationships`: p50 <= 160, p95 <= 320, p99 <= 460
- `outgoing_calls/precise-relationships`: p50 <= 160, p95 <= 320, p99 <= 460
- `document_symbols/single-rust-file`: p50 <= 90, p95 <= 220, p99 <= 320
- `search_structural/rust-function-scoped`: p50 <= 110, p95 <= 260, p99 <= 380
- `search_hybrid/semantic-toggle-off`: p50 <= 25, p95 <= 60, p99 <= 90
- `search_hybrid/semantic-degraded-missing-credentials`: p50 <= 30, p95 <= 75, p99 <= 110
- `deep_search_run/basic-playbook`: p50 <= 260, p95 <= 600, p99 <= 850
- `deep_search_replay/basic-playbook`: p50 <= 300, p95 <= 700, p99 <= 950
- `deep_search_compose_citations/basic-playbook`: p50 <= 50, p95 <= 120, p99 <= 180
- `provenance_write_overhead/read-file-repeated-16x`: p50 <= 80, p95 <= 180, p99 <= 260

## Reporting

Generate a consolidated report (search + MCP workloads):

```bash
python3 benchmarks/generate_latency_report.py
```

Output contract:

- deterministic key-value lines to stdout:
  - `benchmark_report_version=...`
  - per-workload `status`, `p50_ms`, `p95_ms`, `p99_ms`, and budget fields
  - `summary pass=... fail=... missing=...`
  - `report_path=...`
- markdown report file:
  - `benchmarks/latest-report.md`
