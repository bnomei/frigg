# Scripts

Operational and validation scripts used by specs and CI.

- `check-tool-surface-parity.py`: profile-aware parity check for MCP tool surface claims across runtime profile manifests (`crates/cli/src/mcp/tool_surface.rs` + `crates/cli/src/mcp/types.rs`), schema files (`contracts/tools/v1/*.v1.schema.json`), and docs markers in `contracts/tools/v1/README.md` + `docs/overview.md`.
