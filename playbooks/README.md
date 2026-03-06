# Frigg Playbooks

These markdown playbooks are human-readable search scenarios for Frigg MCP. They now carry an executable metadata header for hybrid-search regressions, while the broader multi-tool narrative remains descriptive until we freeze full params, traces, and expected outputs into deterministic JSON playbooks or replay artifacts.

## Format

Each playbook follows the same structure:

1. Executable metadata header (`<!-- frigg-playbook ... -->`) with the hybrid regression query, witness groups, and target paths
1. Search goal
2. Why the search matters
3. Scope and assumptions
4. Expected tool flow
5. Expected return cues
6. Recording ledger for the eventual real run

The hybrid metadata is intentionally smaller than the prose. It captures the minimum release-gating contract:

- the `search_hybrid` query we expect a developer to ask
- allowed semantic states (`ok`, `disabled`, `degraded`)
- `required_witness_groups` that define the current "can do" expectation
- `target_witness_groups` that define the stronger "should do" expectation

The executable suite currently gates the required witness groups and can optionally enforce target paths in a live semantic run.

## Executable Harness

Run the hybrid markdown playbook harness with:

```bash
cargo test -p frigg --test playbook_hybrid_suite -- --nocapture
```

That reads the metadata header from each markdown playbook, runs `search_hybrid` against the current repo, and asserts the required witness groups. To turn the stricter target witness groups into a failing contract too:

```bash
FRIGG_PLAYBOOK_ENFORCE_TARGETS=1 cargo test -p frigg --test playbook_hybrid_suite -- --nocapture
```

## Why These Scenarios

These playbooks are designed around a few recurring code-understanding expectations:

- precise search over files, symbols, and patterns
- code navigation that still explains itself when precise data is missing
- multi-step source-backed workflows instead of vague summaries
- retrieval/ranking flows that surface why an answer degraded or narrowed

Those themes map cleanly onto Frigg's current surface:

- search and ranking: `search_text`, `search_hybrid`, `search_symbol`
- navigation and fallbacks: `go_to_definition`, `find_references`, `find_implementations`, `incoming_calls`, `outgoing_calls`, `document_symbols`, `search_structural`
- source-backed multi-step flows: `deep_search_run`, `deep_search_replay`, `deep_search_compose_citations`
- deterministic typed failures: canonical `error_code` mappings in `contracts/errors.md`

## Design Rules

- Prefer questions a developer would actually ask during onboarding, debugging, or impact analysis.
- Keep each flow narrow enough that it can become deterministic later.
- Make fallbacks explicit when precise index data may be missing.
- Record both happy-path and typed-failure expectations where those behaviors are part of the product promise.
- Bias toward source-backed answers over vague summaries.

## Current Playbooks

- `http-auth-entrypoint-trace.md`
- `tool-surface-gating.md`
- `hybrid-search-context-retrieval.md`
- `implementation-fallback-navigation.md`
- `error-contract-alignment.md`
- `deep-search-replay-and-citations.md`
