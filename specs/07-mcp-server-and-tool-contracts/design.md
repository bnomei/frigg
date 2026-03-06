# Design — 07-mcp-server-and-tool-contracts

## Normative excerpt (from `docs/overview.md`)
- Use rmcp as official MCP SDK.
- Default local transport should be stdio; HTTP optional with strict security constraints.
- Core tool surface is deterministic and evidence-producing.

## Architecture
- `crates/mcp/` owns tool handler definitions and server info/capabilities.
- `crates/cli/` owns runtime selection (`stdio` vs streamable HTTP) and bootstrapping.
- Tool contracts map 1:1 to docs contract schemas (`contracts/tools/v1`).

## Nereid alignment
- Use `#[tool_router]` + `#[tool_handler]` routing pattern.
- Keep server state explicit and clonable.
- Keep transport setup isolated from tool logic.
