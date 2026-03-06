# Design — 03-text-search-engine

## Normative excerpt (from `docs/overview.md`)
- Deterministic code search should prioritize literal/regex accuracy.
- Trigram-style prefiltering is the robust base for code-like substring and regex search.
- Safe regex constraints are required as a security gate.

## Architecture
- `crates/search/` owns query parsing, literal execution, regex execution, and ranking baseline.
- `crates/index/` provides file candidate sets and manifest metadata.
- Path/language filter enforcement occurs before expensive match verification.

## Query model
- `patternType`: `literal` | `regexp` (structural deferred to later workstream).
- Common filters: repository id, path regex, language, limit.
- Output: deterministic sorted matches by `(repository_id, path, line, column)`.

## Performance model
- Baseline literal path uses Aho-Corasick/multi-pattern prefilter.
- Regex path uses bounded candidate set and finite-automata-safe regex engine.
