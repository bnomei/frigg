# Requirements — 45-watch-driven-changed-reindex-correctness

## Goal
Watch-Driven Changed Reindex Correctness

## Functional requirements (EARS)
- WHEN built-in watch mode triggers a repository refresh THEN THE SYSTEM SHALL execute the existing changed-only reindex pipeline instead of introducing a separate dirty-path indexer.
- WHEN a watch-triggered changed-only reindex detects repository changes THEN THE SYSTEM SHALL persist a new full manifest snapshot for that root and advance semantic rows so unchanged records move to the new snapshot while changed and deleted paths are replaced.
- WHEN MCP queries arrive while a watch-triggered changed-only reindex is still in flight THEN THE SYSTEM SHALL continue serving results from the latest successful snapshot until the new snapshot commits.
- IF a persisted manifest snapshot is stale or invalid during watch-triggered operation THEN THE SYSTEM SHALL reject snapshot-backed reuse and fall back to live discovery as already required by manifest freshness validation.
- WHEN files under `docs/` are gitignored THEN THE SYSTEM SHALL continue indexing and searching them during watch-triggered refreshes.
- WHEN paths under `.git/`, `.frigg/`, or `target/` change THEN THE SYSTEM SHALL not ingest them into the active search corpus and shall not treat them as watch-triggered refresh inputs.
- IF a watched file is modified, deleted, or renamed THEN THE SYSTEM SHALL stop returning stale manifest-backed or semantic-backed results for the superseded path after the replacement snapshot commits.
- IF a watch-triggered refresh fails for one workspace root THEN THE SYSTEM SHALL preserve correctness and refresh progress for other roots.

## Non-functional requirements
- This spec must not add a new path-level incremental manifest patcher; it reuses the current manifest-refresh plus semantic-delta `--changed` pipeline.
- Watch-triggered background refreshes must preserve deterministic latest-successful-snapshot semantics for query serving.
- Validation must include targeted regressions for docs visibility, ignored-path exclusion, delete/rename freshness, and end-to-end watcher-triggered hybrid playbook behavior.
