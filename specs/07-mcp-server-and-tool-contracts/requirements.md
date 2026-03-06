# Requirements — 07-mcp-server-and-tool-contracts

## Scope
Build the MCP server surface and transport behavior using rmcp patterns aligned with Nereid-style implementation.

## EARS requirements
- The MCP server shall expose `list_repositories`, `read_file`, `search_text`, `search_symbol`, and `find_references` tools.
- When running locally, the MCP server shall support stdio transport by default.
- Where HTTP mode is enabled, the MCP server shall run on localhost and enforce origin/auth safety controls.
- When tools are called, the MCP server shall emit provenance events sufficient for replay and citation.
- If a tool call fails, then the MCP server shall return typed errors consistent with the public error contract.
