# Host notes (optional)

These notes describe where to paste templates for common coding agents. None are
required for Frigg CI.

| Host | Typical policy surface | Suggestion |
| --- | --- | --- |
| Claude Code | project `CLAUDE.md` / hooks | AGENTS snippet + soft shell justification in PreToolUse |
| Codex | `AGENTS.md` | AGENTS snippet; skill path in repo |
| Cursor | project rules / `.cursor` | AGENTS snippet in rules; skill as always-on ref |
| Generic MCP | MCP server config + skill | Register Frigg; load `frigg-first-code-search` skill |

See sibling files for slightly longer host-specific notes.
