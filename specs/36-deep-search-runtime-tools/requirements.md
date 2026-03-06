# Requirements — 36-deep-search-runtime-tools

## Goal
Promote the existing deep-search harness APIs from internal library usage to an optional MCP runtime tool surface.

## Functional requirements (EARS)
- WHEN deep-search runtime tools are enabled THE SYSTEM SHALL expose deterministic MCP tools for playbook execution, replay diffing, and citation payload composition.
- WHEN a client calls `deep_search_run` with a valid playbook THE SYSTEM SHALL execute only allowlisted read-only tools and return a deterministic trace artifact.
- WHEN a client calls `deep_search_replay` with a valid playbook and expected trace THE SYSTEM SHALL return deterministic replay comparison output (`matches`, `diff`, `replayed`).
- WHEN a client calls `deep_search_compose_citations` with a valid trace artifact THE SYSTEM SHALL return deterministic citation payloads with stable claim and citation IDs.
- IF a playbook step references unsupported tools or invalid params THEN THE SYSTEM SHALL return typed deterministic `invalid_params` failures.
- WHILE deep-search runtime tools are disabled THE SYSTEM SHALL NOT expose them in `tools/list`.

## Non-functional requirements
- Deep-search runtime tools must remain read-only and side-effect free for MCP clients.
- Existing resource budgets and provenance behavior must apply to tool executions inside deep-search playbooks.
- Replay behavior must remain deterministic under repeated identical inputs.
