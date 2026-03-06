# Requirements — 48-typescript-tsx-runtime-symbol-surface

## Goal
Promote TypeScript and TSX from roadmap target to first-class runtime L1/L2 languages across Frigg's symbol and read-only navigation surfaces.

## Functional requirements (EARS)
- WHEN repository manifests include `.ts` or `.tsx` files THE SYSTEM SHALL treat them as supported source files for symbol-corpus construction, warm-corpus rebuilds, and runtime symbol queries.
- WHEN a supported TypeScript or TSX file is indexed THE SYSTEM SHALL extract deterministic symbol definitions for namespaces/modules, classes, interfaces, enums, type aliases, functions, methods, fields/properties, `const` bindings, and identifier-bound mutable variable declarations needed for common application and TSX component patterns.
- WHEN a client calls `search_symbol`, `document_symbols`, or `search_structural` against supported TypeScript/TSX sources THE SYSTEM SHALL return canonical repository-relative results with deterministic ordering and note metadata aligned to current Rust/PHP behavior.
- WHEN a client calls `find_references`, `go_to_definition`, `find_declarations`, `find_implementations`, `incoming_calls`, or `outgoing_calls` for a TypeScript/TSX-backed symbol or source position THE SYSTEM SHALL reuse the existing symbol-corpus and fallback precedence flows without language-specific contract drift.
- WHEN a client supplies a TypeScript-oriented runtime `language` filter THE SYSTEM SHALL accept `typescript`, `ts`, and `tsx` as aliases for one logical TypeScript family.
- IF TypeScript/TSX parser configuration or symbol extraction fails for a file THEN indexing SHALL continue and SHALL emit typed diagnostics without corrupting cached corpus state.
- WHILE identical repository state and query inputs are replayed THE SYSTEM SHALL preserve stable TypeScript/TSX symbol IDs and deterministic byte-equivalent ordering.

## Non-functional requirements
- TypeScript/TSX onboarding SHALL NOT regress existing Rust/PHP output ordering, schema identity, or typed-error behavior.
- TSX component definitions expressed as functions, classes, or exported `const` bindings SHALL be discoverable without a JSX-specific public tool contract.
- Public docs/contracts SHALL describe TypeScript/TSX support only for the query shapes and tools implemented by this spec.
