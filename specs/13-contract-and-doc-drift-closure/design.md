# Design — 13-contract-and-doc-drift-closure

## Normative excerpt
- Public contract docs are source-of-truth and must be synchronized with implementation.

## Architecture
- `contracts/`
  - add `changelog.md` and keep deterministic reverse-chronological entries.
  - align `config.md` with actual `FriggConfig` fields and defaults.
- `specs/`
  - align language-coverage wording with implemented Rust/PHP support.
- `crates/mcp/src/mcp/deep_search.rs`
  - switch citation extraction from `snippet` fallback to `excerpt` primary field.
- benchmark/security/release docs
  - clarify deep-search harness status as internal test harness unless separately exposed.

## Acceptance signals
- Contracts directory contains changelog and release gate checks it.
- No stale claims for unsupported config keys/languages.
- Citation payload tests pass with live response shape (`excerpt`).
