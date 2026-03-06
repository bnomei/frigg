# Requirements — 03-text-search-engine

## Scope
Implement deterministic text search for literal and safe-regex code queries.

## EARS requirements
- When users submit a literal query, the search subsystem shall return deterministic path/line/column matches.
- When users submit a regex query, the search subsystem shall evaluate with safe-regex constraints and bounded runtime behavior.
- While search results are returned, the search subsystem shall support repository/path/language filters and explicit limits.
- If regex input is invalid or unsafe, then the search subsystem shall return typed invalid-params errors without crashing.
- The search subsystem shall report p50/p95 latency metrics for standard fixture queries.
