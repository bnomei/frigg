# Extended / opt-in tools

## Profile model

- **Core** — Futura primary loop + in-file `explore`. This is the default product surface for agents.
- **Extended** — core tools plus **playbook** tools when the binary was built with `--features playbook`.
- On default builds (no playbook feature), core and extended register the **same** tool names.
- Trust `runtime.tools_exposed` / live `tools/list` for this process.

## `explore` (core)

`explore` is **not** extended-only. Full docs live in [discovery-and-evidence.md](discovery-and-evidence.md).

Use it after discovery when you need:
- probe/zoom/refine within one artifact
- continuation cursors
- anchored windows instead of repeated full reads

Presentation defaults:
- `zoom` is text-first by default, with compact metadata and `presentation_mode=json` as the structured compatibility escape hatch
- `probe` and `refine` stay structured by default
- `presentation_mode=text` is invalid for `probe` and `refine`

## Playbook tools (dev / trace; not default)

- `playbook_run`
- `playbook_replay`
- `playbook_compose_citations`

These require:

1. Compile with `--features playbook` (**not** in default cargo features)
2. Extended tool-surface profile (`FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`, the process default when playbook is compiled in)

They are for **explicit** trace-oriented search workflows, not normal first-line repo navigation.

Use them when the task explicitly needs:
- a replayable multi-step search trace
- diffing a replay against an expected trace
- citation payload composition from an existing trace

Do not reach for playbook tools when normal `search_hybrid`, `search_symbol`, navigation, and bounded reads will answer the question more simply.
