# Requirements — 44-integrated-local-watch-mode

## Goal
Integrated Local Watch Mode

## Functional requirements (EARS)
- WHEN Frigg starts in stdio transport and `watch_mode` resolves to `auto` THEN THE SYSTEM SHALL start a built-in background watcher for each configured workspace root.
- WHEN Frigg starts in loopback HTTP transport and `watch_mode` resolves to `auto` THEN THE SYSTEM SHALL start a built-in background watcher for each configured workspace root.
- WHEN `watch_mode` resolves to `on` THEN THE SYSTEM SHALL start the built-in watcher regardless of transport.
- WHEN `watch_mode` resolves to `off`, or `watch_mode=auto` resolves against non-loopback HTTP, THEN THE SYSTEM SHALL not start the built-in watcher.
- WHILE filesystem events arrive for a watched root THE SYSTEM SHALL debounce trigger scheduling per root and serialize changed-only reindex execution to one in-flight background refresh across the process.
- IF additional filesystem events arrive for a root while that root already has an in-flight changed-only reindex THEN THE SYSTEM SHALL coalesce them into one follow-on rerun after the active refresh completes.
- WHEN a watched root has no valid latest manifest snapshot at startup THEN THE SYSTEM SHALL enqueue one immediate background changed-only reindex for that root.
- WHEN a watched root already has a valid latest manifest snapshot at startup THEN THE SYSTEM SHALL begin watching without forcing a startup refresh.
- IF a watch-triggered changed-only reindex fails THEN THE SYSTEM SHALL keep the affected root dirty, schedule one retry after the configured backoff, and keep the MCP server available for subsequent requests.
- WHEN watch-trigger filtering is evaluated THE SYSTEM SHALL ignore `.frigg/`, `.git/`, and `target/` changes without depending on Git ignore rules for correctness.
- THE SYSTEM SHALL expose watch configuration through global CLI flags and env vars for `watch_mode`, debounce, and retry backoff.

## Non-functional requirements
- Default watch settings must be `watch_mode=auto`, `watch_debounce_ms=750`, and `watch_retry_ms=5000`.
- Watch mode v1 must add no MCP tool, RPC, or status endpoint; observability is tracing/logs only.
- Built-in watch mode must remain a local-development convenience and must not silently enable itself for non-loopback HTTP by default.
