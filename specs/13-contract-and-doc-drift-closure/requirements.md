# Requirements — 13-contract-and-doc-drift-closure

## Scope
Close contract/documentation drift and evidence-shape inconsistencies identified by the review.

## EARS requirements
- If a public behavior or contract changes, then the Frigg platform shall append a deterministic entry to `contracts/changelog.md`.
- When config contract docs describe runtime keys, the docs shall only include keys that are implemented and validated by runtime code.
- While symbol-language support documentation is published, the docs shall match actual implemented language support.
- When deep-search citation payloads are composed from `search_text`, the system shall consume `excerpt` from `TextMatch` responses.
- Where repository IDs are generated, the contract docs shall explicitly state stability semantics and client expectations.
- Where deep-search harness is not exposed as a runtime public MCP tool, docs shall mark it as internal/test harness functionality.
