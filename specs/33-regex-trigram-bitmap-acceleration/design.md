# Design — 33-regex-trigram-bitmap-acceleration

## Scope
- crates/search/src/lib.rs
- crates/search/benches/search_latency.rs
- benchmarks/search.md
- benchmarks/budgets.v1.json
- contracts/changelog.md
- docs/overview.md

## Approach
- Add a regex prefilter plan builder that extracts required literals and builds trigram bitmap constraints.
- Apply file-level prefilter checks before per-line regex matching.
- Keep current sorting, diagnostics, and fallback semantics unchanged.
- Extend regex benchmarks for sparse/no-hit scenarios and update budgets/docs.

## Data flow and behavior
- `search_regex_with_filters_diagnostics` compiles safe regex and optional prefilter plan.
- For each file candidate:
  - read content
  - evaluate prefilter; skip line regex scan when impossible to match
  - otherwise run existing per-line regex match flow.

## Risks
- Literal extraction may be too conservative or too permissive; implementation must avoid false negatives.
- Benchmarks may need fixture tuning to show meaningful improvements.

## Validation strategy
- cargo test -p searcher --all-targets
- cargo bench -p searcher --bench search_latency -- --noplot
- python3 benchmarks/generate_latency_report.py
