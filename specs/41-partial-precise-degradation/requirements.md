# Requirements — 41-partial-precise-degradation

## Goal
Partial Precise Degradation Instead Of Corpus-Wide Drop

## Functional requirements (EARS)
- WHEN some SCIP artifacts ingest successfully and others fail THE SYSTEM SHALL retain precise data from the successful artifacts instead of clearing precise data for the entire repository corpus.
- WHEN a read-only navigation or reference request executes against partial precise state THE SYSTEM SHALL distinguish between authoritative precise hits and non-authoritative precise absence.
- IF partial precise state does not provide an authoritative answer for the requested symbol or relation THEN THE SYSTEM SHALL fall back to the existing deterministic heuristic path for that request.
- WHEN a response uses retained partial precise data THE SYSTEM SHALL emit deterministic note metadata describing the partial/degraded mode and the successful versus failed artifact counts.
- IF all precise artifacts fail or no usable precise state exists for the requested symbol THE SYSTEM SHALL preserve the current heuristic fallback behavior and typed diagnostics.
- IF this change alters public precision metadata, contracts, or benchmark guidance THEN THE SYSTEM SHALL update those artifacts in the same change set.

## Non-functional requirements
- Failed or partial SCIP ingest must not corrupt existing precise graph state for successful artifacts.
- Deterministic ordering and typed error behavior must remain stable for successful precise and heuristic fallback results.
- Validation must include mixed-success SCIP ingest coverage for references and navigation handlers, plus contract coverage for the updated note semantics.
