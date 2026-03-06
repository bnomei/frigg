# Design — 30-citation-hygiene-gate

## Scope
- scripts/check-citation-hygiene.sh (new)
- scripts/check-doc-sync.sh
- scripts/check-release-readiness.sh
- Justfile
- docs/overview.md
- docs/security/release-readiness.md

## Approach
- Add a dedicated citation checker script that parses `docs/overview.md` and compares:
  - body URLs (before fact-check registry)
  - fact-check registry URLs
- Enforce placeholder-token bans with deterministic reporting.
- Wire the checker into docs-sync and release-readiness so regressions block merges/releases.
- Update `docs/overview.md` checkpoint wording once citation hygiene is enforced.

## Data flow and behavior
- `check-citation-hygiene.sh` emits one deterministic summary line:
  - `citation_hygiene body=<n> registry=<n> missing=<n> placeholders=<n>`
- Non-zero exit on missing registry URLs or placeholder tokens.
- Docs and release gates invoke this script and propagate failures.

## Risks
- Overly strict matching may require frequent registry updates for harmless text edits.
- URL extraction must avoid false positives from code blocks.

## Validation strategy
- bash scripts/check-citation-hygiene.sh
- bash scripts/check-doc-sync.sh
- just docs-sync
- bash scripts/check-release-readiness.sh
