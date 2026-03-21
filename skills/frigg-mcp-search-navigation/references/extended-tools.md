# Extended Tools

These tools are only available when the Frigg runtime exposes the extended tool surface.

## `explore`

`explore` is the bounded follow-up file tool. Use it after discovery when you need:
- probe/zoom/refine within one artifact
- continuation cursors
- anchored windows instead of repeated full reads

Presentation defaults:
- `zoom` is text-first by default, with compact metadata and `presentation_mode=json` as the structured compatibility escape hatch
- `probe` and `refine` stay structured by default

See [discovery-and-evidence.md](discovery-and-evidence.md) for the detailed input and output shape.

## Deep Search Tools

- `deep_search_run`
- `deep_search_replay`
- `deep_search_compose_citations`

These tools are for explicit trace-oriented search workflows, not normal first-line repo navigation.

Use them when the task explicitly needs:
- a replayable multi-step search trace
- diffing a replay against an expected trace
- citation payload composition from an existing trace

Do not reach for deep-search tools when normal `search_hybrid`, `search_symbol`, navigation, and bounded reads will answer the question more simply.
