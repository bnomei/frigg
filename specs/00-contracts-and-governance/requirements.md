# Requirements — 00-contracts-and-governance

## Scope
Define stable public contracts and governance rules that all implementation specs consume.

## EARS requirements
- The Frigg platform shall publish one versioned input/output schema per MCP tool.
- When a tool receives invalid input, the Frigg platform shall return a typed deterministic error category and machine-readable detail payload.
- While new capabilities are added, the Frigg platform shall keep `docs/overview.md`, `docs/phases.md`, and `specs/index.md` synchronized in the same change set.
- If a change modifies public tool behavior, then the Frigg platform shall record contract impact in `contracts/changelog.md`.
- Where write-capable tools are enabled, the Frigg platform shall require explicit confirmation semantics before mutation.
