# Requirements — 42-manifest-freshness-validation

## Goal
Manifest Snapshot Freshness Validation

## Functional requirements (EARS)
- WHEN symbol-corpus or manifest-backed search fast paths reuse a persisted manifest snapshot THE SYSTEM SHALL validate stored digest metadata against current filesystem metadata instead of trusting path existence alone.
- IF any snapshot entry is missing, size-mismatched, mtime-mismatched, or otherwise unverifiable THEN THE SYSTEM SHALL reject snapshot fast-path reuse for that repository and rebuild from live manifest discovery.
- WHEN validated manifest snapshot metadata is reused for cache signatures or candidate selection THE SYSTEM SHALL derive those signatures from the validated metadata set.
- WHEN live fallback occurs because snapshot metadata is stale THE SYSTEM SHALL preserve deterministic diagnostics behavior and avoid reusing stale in-memory corpus state for that repository.
- IF freshness validation changes public operator guidance, benchmark assumptions, or contract notes THEN THE SYSTEM SHALL update the corresponding contracts artifacts in the same change set.

## Non-functional requirements
- Snapshot freshness checks must remain metadata-only and must not read full file contents.
- Fast-path reuse must not return stale symbol/search results after on-disk source metadata diverges from the persisted snapshot.
- Validation must include targeted stale-snapshot regression coverage for symbol-corpus reuse and manifest-backed search candidate discovery.
