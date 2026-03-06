# Design — 44-integrated-local-watch-mode

## Scope
- `crates/cli/src/main.rs`
- `crates/cli/src/settings/`
- `crates/cli/src/watch.rs`
- `README.md`
- `specs/44-integrated-local-watch-mode/`
- `specs/index.md`

## Problem statement
Frigg currently requires an explicit `reindex` command or an external watcher to refresh snapshots after local file changes. That is safe, but it leaves stdio and loopback HTTP workflows without a built-in local refresh loop and forces users to wire their own debounce, restart, and retry behavior around `reindex --changed`.

The expensive part of the workflow is still reindexing, not file event detection. This spec therefore adds an integrated watcher as a scheduling layer on top of the existing changed-only reindex path instead of introducing a new indexer.

## Approach
Add a built-in watcher subsystem that is enabled by configuration and transport policy:

1. Resolve watch settings from global CLI flags and env vars during startup.
2. Decide whether watch mode is active from `watch_mode` plus the resolved transport.
3. Start one watcher supervisor for all configured workspace roots when watch mode is active.
4. Translate filesystem events into root-level dirty signals.
5. Debounce each root independently, but run only one background changed-only reindex at a time across the process.
6. If a root changes while its reindex is already in flight, mark `rerun_requested=true` and run exactly one follow-on refresh after the active run completes.
7. Keep the MCP transport loop alive even if a watch-triggered refresh fails.

## Configuration surface
- Add `WatchMode` with values `auto`, `on`, and `off`.
- Add `WatchConfig` with `mode`, `debounce_ms`, and `retry_ms`.
- Resolve these globals in the same layer that currently resolves semantic runtime and HTTP config.
- `auto` means:
  - enabled for stdio
  - enabled for loopback HTTP (`127.0.0.1`, `::1`, `localhost`)
  - disabled for non-loopback HTTP
- `on` forces watch mode for any transport.
- `off` disables watch mode for any transport.

## Runtime architecture

### Watch supervisor
Add a new `watch.rs` module that owns:
- the `notify` watcher
- a root registry for configured workspace roots
- per-root dirty state
- a serialized background reindex executor

Each root keeps the minimum state required to drive scheduling:
- `pending`
- `last_event_at`
- `in_flight`
- `rerun_requested`
- a small sample of recent changed paths for logs only

### Trigger filtering
Trigger filtering is root-local and path-based:
- ignore `.frigg/`
- ignore `.git/`
- ignore `target/`
- do not rely on Git ignore rules for correctness decisions

The built-in watcher should never trigger on provenance DB churn, build outputs, or Git housekeeping. `docs/` remains visible to indexing/search correctness; this spec only excludes the specific self-noise roots above from trigger scheduling.

### Startup behavior
When watch mode is active:
- if a root has no valid latest manifest snapshot, enqueue one immediate changed-only refresh
- if a root has a valid latest manifest snapshot, start watching and wait for events

This startup check is metadata-based and should reuse the same storage/validation concepts that already protect persisted manifest reuse.

### Failure and retry behavior
- A failed background refresh does not stop the watcher or the MCP server.
- The failed root stays dirty.
- The supervisor schedules one retry after `watch_retry_ms`.
- New events received before the retry fires should collapse into that same pending rerun rather than creating parallel jobs.

## Logging and observability
Watch mode v1 is logs-only. Emit structured tracing for:
- watch mode enabled/disabled and the activation reason
- watched roots registered
- filesystem events accepted or ignored
- debounce scheduled and debounce fired
- reindex started
- reindex succeeded with root and summary metadata
- reindex failed with root, retry delay, and rerun state

## Risks
- A watcher tied too tightly to transport startup can create shutdown or stdio framing hazards if it logs incorrectly or blocks the main runtime.
- Over-eager event handling can create self-trigger loops from `.frigg/` or `target/` churn if path filtering is incomplete.
- Treating every root independently without a process-wide executor can create overlapping semantic jobs and storage contention.

## Validation strategy
- Config resolution tests for `auto`, `on`, `off`, debounce, and retry defaults.
- Transport gating tests for stdio, loopback HTTP, and non-loopback HTTP.
- State-machine tests for per-root debounce, serialized execution, rerun coalescing, and retry after failure.
- Integration tests showing startup initial sync for missing manifests and no forced sync for valid manifests.
