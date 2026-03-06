# Design — 08-hybrid-retrieval-and-deep-search-harness

## Normative excerpt (from `docs/overview.md`)
- Hybrid retrieval should combine lexical exactness, graph grounding, and semantic recall.
- Deep Search is an agentic tool loop with auditable sources.
- Determinism and replay are mandatory for validation.

## Architecture
- `crates/search/` hosts hybrid retrieval orchestration and ranking composition.
- `crates/mcp/` provides provenance-rich tool calls consumed by harness runs.
- `fixtures/playbooks/` stores replayable deep-search scenarios and expected outputs.

## Retrieval policy
1. Run lexical candidate fetch.
2. Expand with symbol graph traversal.
3. Add semantic nearest chunks.
4. Re-rank and emit evidence bundle with provenance IDs.
5. Compose answer scaffold with evidence links.
