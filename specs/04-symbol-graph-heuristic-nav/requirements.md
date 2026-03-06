# Requirements — 04-symbol-graph-heuristic-nav

## Scope
Deliver L1/L2 code navigation using symbol extraction and heuristic references.

## EARS requirements
- When repositories are indexed, the navigation subsystem shall extract symbol definitions for Rust and PHP as first-class targets.
- While precise SCIP data is unavailable, the navigation subsystem shall provide best-effort heuristic references with explicit confidence limitations.
- When `search_symbol` is called, the system shall return symbol kind, path, and definition location.
- When `find_references` is called without precise data, the system shall return heuristic references flagged as heuristic.
- If symbol extraction fails for a file, then indexing shall continue and emit typed extraction diagnostics.
