# Design — 00-contracts-and-governance

## Normative excerpt (from `docs/overview.md`)
- Define tool contract versioning.
- Define config contract with defaults and validation behavior.
- Define deterministic error categories per tool.
- Enforce security gates and replayable traces.

## Architecture decisions
- Contract artifacts live in `contracts/`.
- Tool schemas are grouped by version under `contracts/tools/v1/`.
- Error taxonomy is centralized in `contracts/errors.md` and mapped to rmcp `ErrorData` categories.
- Config contract is documented in `contracts/config.md` and mirrored in typed structs in `crates/config/`.
- Changelog for externally visible contract changes is maintained in `contracts/changelog.md`.

## Deliverables
- Tool schema versioning policy doc.
- Public error taxonomy doc with examples.
- Config contract doc (keys, defaults, validation, compatibility policy).
- Lightweight docs sync check script in `scripts/`.
