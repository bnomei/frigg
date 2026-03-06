# Requirements — 34-readonly-ide-navigation-tools

## Goal
Add deterministic read-only MCP navigation/refactoring primitives aligned with IDE workflows without adding in-server AI planning.

## Functional requirements (EARS)
- WHEN a client calls `go_to_definition` with a resolvable symbol or source position THE SYSTEM SHALL return canonical repository-relative definition locations in deterministic order.
- IF both precise SCIP-backed and heuristic definition candidates exist THEN THE SYSTEM SHALL return precise results first and include precision metadata in the response note.
- WHEN a client calls `find_declarations` for a resolvable symbol THE SYSTEM SHALL return declaration/definition anchor locations with deterministic ordering and typed empty-result behavior.
- WHEN a client calls `find_implementations` for a resolvable symbol THE SYSTEM SHALL return implementation targets from precise relationships when available and deterministic heuristic graph fallbacks otherwise.
- WHEN a client calls `incoming_calls` or `outgoing_calls` for a resolvable symbol THE SYSTEM SHALL return call-adjacency results with relation metadata and deterministic ordering.
- WHEN a client calls `document_symbols` for a supported source file THE SYSTEM SHALL return a deterministic symbol outline for that file using current Rust/PHP extraction support.
- WHEN a client calls `search_structural` with a supported language and valid structural query THE SYSTEM SHALL return deterministic structural matches scoped by repository/path filters.
- IF a new read-only navigation tool receives invalid input, unsupported language, or unsafe pattern parameters THEN THE SYSTEM SHALL return typed deterministic errors aligned to the public error taxonomy.
- WHILE these tools are exposed in MCP `tools/list` THE SYSTEM SHALL keep them read-only (`read_only_hint=true`) and side-effect free except deterministic provenance emission.
- WHEN runtime instructions are generated for MCP clients THE SYSTEM SHALL preserve delegated agency (client orchestrates tool loops; server only exposes deterministic tools).

## Non-functional requirements
- New tool responses must preserve canonical repository-relative path contract semantics.
- Result ordering must be deterministic across repeated identical requests.
- Core hot-path latency budgets must be defined and benchmarked for each new tool workload.
- Existing public tools (`list_repositories`, `read_file`, `search_text`, `search_symbol`, `find_references`) must remain backward-compatible.
