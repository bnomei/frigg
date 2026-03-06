# Configuration Contract (`v1`)

This document defines the public runtime configuration contract for Frigg.

## Scope

- Contract type: `FriggConfig` in `crates/cli/src/settings/mod.rs`.
- This `v1` contract includes only runtime keys implemented in `FriggConfig`.

## Defaults policy

- Defaults are defined in `FriggConfig::default()`.
- `FriggConfig::from_workspace_roots(...)` applies provided `workspace_roots`; when empty, it falls back to `["."]`.
- Any change to a documented default value must be treated as a public behavior change and updated in this file in the same change set.

## Keys, defaults, validation

| Key | Type | Default | Validation behavior |
| --- | --- | --- | --- |
| `workspace_roots` | `Vec<PathBuf>` | `["."]` | Must contain at least one entry; each path must exist. |
| `max_search_results` | `usize` | `200` | Must be greater than `0`. |
| `max_file_bytes` | `usize` | `2097152` (`2 * 1024 * 1024`) | Must be greater than `0`. |
| `watch` | `WatchConfig` | `{ mode: "auto", debounce_ms: 750, retry_ms: 5000 }` | `debounce_ms` and `retry_ms` must both be greater than `0`. |
| `semantic_runtime` | `SemanticRuntimeConfig` | `{ enabled: false, provider: null, model: null, strict_mode: false }` | When `enabled=true`: `provider` is required; `model` is optional and falls back to the provider default; if `model` is provided explicitly, `model.trim()` must be non-empty. |

## Runtime override wiring

- `max_file_bytes` can be overridden at process startup via CLI flag `--max-file-bytes <BYTES>` or env `FRIGG_MAX_FILE_BYTES`.
- `watch.mode` can be overridden via CLI flag `--watch-mode <auto|on|off>` or env `FRIGG_WATCH_MODE`.
- `watch.debounce_ms` can be overridden via CLI flag `--watch-debounce-ms <MILLISECONDS>` or env `FRIGG_WATCH_DEBOUNCE_MS`.
- `watch.retry_ms` can be overridden via CLI flag `--watch-retry-ms <MILLISECONDS>` or env `FRIGG_WATCH_RETRY_MS`.
- Overrides are validated with the same `FriggConfig::validate()` contract (`> 0`).
- The override applies to both MCP serving mode and utility commands (`init`, `verify`, `reindex`) because all runtime paths share the same base config resolution.

## Watch key details

- `watch.mode` accepted values: `auto`, `on`, `off`.
- `watch.mode=auto` enables the built-in watcher for stdio and loopback HTTP, and disables it for non-loopback HTTP.
- `watch.mode=on` forces the built-in watcher for any transport.
- `watch.mode=off` disables the built-in watcher for any transport.
- Built-in watch mode is logs-only in `v1`; there is no separate MCP status tool or RPC surface for watcher state.
- Built-in watch mode remains a local-development convenience over the existing changed-only reindex path; external watchers remain supported for multi-repo orchestration and editor-owned scheduling.

## Semantic runtime key details

- `semantic_runtime.provider` accepted values: `openai`, `google`.
- `semantic_runtime.model` provider defaults are:
  - `openai` -> `text-embedding-3-small`
  - `google` -> `gemini-embedding-001`
- `semantic_runtime.strict_mode` controls query-time strict semantic behavior and defaults to `false`.
- `semantic_runtime` credential startup checks are environment-sourced and deterministic:
  - `OPENAI_API_KEY` is required for `provider=openai`.
  - `GEMINI_API_KEY` is required for `provider=google`.
- Missing/blank required key maps to deterministic semantic startup failure code `invalid_params` and aborts startup when semantic runtime is enabled.

## Derived repository-id behavior

- `FriggConfig::repositories()` assigns IDs as `repo-001`, `repo-002`, ... in current `workspace_roots` order.
- IDs are stable only while `workspace_roots` order and membership stay unchanged.
- Reordering, adding, or removing workspace roots can renumber IDs.
- Clients must refresh repository IDs from `list_repositories` for each runtime config snapshot.
- `FriggConfig::root_by_repository_id(...)` resolves IDs only within the same active config snapshot.

## Validation failure semantics

- Validation entrypoint: `FriggConfig::validate()`.
- On failure, validation returns `FriggError::InvalidInput` with a deterministic message describing the violated key/constraint.
