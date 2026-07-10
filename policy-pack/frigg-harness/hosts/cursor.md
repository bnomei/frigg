# Cursor

## Adopt targets

| Flag | Path | Role |
| --- | --- | --- |
| `--target cursor` | `.cursor/rules/frigg.mdc` | Project rules (managed markdown) |
| `--target mcp-cursor` | `.cursor/mcp.json` | **Preferred** project MCP entry |
| `--target mcp-project` | `.mcp.json` | Generic project MCP (shared with other hosts) |

Detect: a `.cursor/rules/` directory (or the frigg rule file) opts the repo into the cursor target.

## MCP (project vs global)

| Config | Path | Guidance |
| --- | --- | --- |
| **Project (prefer)** | `.cursor/mcp.json` | Use `frigg adopt --target mcp-cursor`. Reliable for agent tool injection. |
| Global | `~/.cursor/mcp.json` | May show in Settings → Tools & MCP yet be **ignored by the agent** on some Cursor builds. Prefer project MCP for Frigg. |

Managed adopt entries use loopback HTTP (`type: http`, default `http://127.0.0.1:37444/mcp`). Keep `frigg serve` running.

Confirm live `tools/list` includes `search_text`, `search_batch`, `workspace` before routing.

## Skills (best-effort)

```bash
frigg adopt --skill-provider cursor
```

| Scope | Parent dir (must already exist) | Dest |
| --- | --- | --- |
| **Project (prefer)** | `.cursor/skills/` | `.cursor/skills/frigg-first-code-search/` |
| Personal | `~/.cursor/skills/` | `~/.cursor/skills/frigg-first-code-search/` |

Frigg never creates a missing skills parent. Project skills are the common Cursor layout; personal `~/.cursor/skills` is optional fallback when present.

## Hooks (Cursor-native — not `frigg adopt --target hook`)

Cursor hooks are **separate** from Claude’s opt-in PreToolUse hook:

| Scope | Path | Notes |
| --- | --- | --- |
| Project | `.cursor/hooks.json` | Cloud agents load repo hooks from this path |
| User | `~/.cursor/hooks.json` | IDE / local; **not** available to cloud agents |

`frigg adopt --target hook` only writes **Claude** `.claude/settings.json`. Frigg does **not** generate Cursor `hooks.json` yet (shape and event names differ: e.g. `beforeReadFile`, `afterFileEdit`, `sessionStart`). Soft shell justification can live in a user/project rule or a hand-written Cursor hook if shell tools compete with MCP on indexed paths.

## Soft policy

- Point project rules at the AGENTS snippet template (`templates/AGENTS.snippet.md`).
- Soft shell justification is optional; do not re-order tools inside Frigg product code.
