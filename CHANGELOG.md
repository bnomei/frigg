# Changelog

## 0.4.2 - 2026-05-27

- Fixed semantic reindex recovery for deleted files recorded with absolute paths when indexing from a relative workspace root, allowing stale semantic chunks to be skipped or cleaned instead of failing on path canonicalization.
- Updated dependencies.

## 0.4.1 - 2026-05-24

- Fixed scoped MCP search to use runtime repository IDs for manifest and semantic storage lookups while preserving stable public repository IDs in responses.
- Fixed adjacent symbol/navigation manifest lookup, hybrid exact-pivot repository scoping, and root generated SCIP artifact watch churn.

## 0.4.0 - 2026-05-18

- Updated dependencies and removed the unused `gix` workspace dependency.

## 0.3.2 - 2026-04-17

- Upgraded dependencies

## 0.2.2 - 2026-04-17

- Replaced permissive `outputSchema.properties.metadata` boolean schemas with explicit object schemas for the affected MCP navigation and symbol-search tools, improving compatibility with strict clients such as Cursor.

## 0.2.1

- Upgraded `rmcp` from `1.2.0` to `1.4.0`.

## 0.2.0 - 2026-03-23

- Upgraded `rmcp` from `1.1.0` to `1.2.0`.
- Verified the `frigg` crate builds and its package test suite passes against `rmcp 1.2.0`.
- Restored bounded-SCIP coverage in max-file-bytes tool-handler tests by disabling `full_scip_ingest` in the test helper that exercises budgeted paths.
- Updated the `document_symbols` unsupported-extension expectation to include `.java`, matching the current language registry.
