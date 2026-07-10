# Host notes (optional)

These notes describe where to paste templates and which `frigg adopt` targets map
to each host. **None are required for Frigg CI** except when you deliberately
gate managed blocks with `frigg adopt --check`.

## Install triangle (operator rule)

| Surface | Role | SSOT? |
| --- | --- | --- |
| Production skill (`skills/frigg-first-code-search/`) | Scenario routing / Frigg-first loops | **Yes — behavior** |
| Live MCP `tools/list` / `runtime.tools_exposed` | What tools exist **this process** | **Yes — existence** |
| `frigg adopt` | Install plumbing (managed markdown, MCP JSON, opt-in hook, best-effort skill copy) | No |
| This pack (`hosts/` + `templates/`) | Optional paste guidance only | No |

Rule of thumb: **skill wins for scenarios; `tools/list` wins for existence; adopt is install plumbing; policy-pack is optional host paste.**

## Adopt target matrix

| Adopt target | Path written | Host note | Notes |
| --- | --- | --- | --- |
| `agents-md` | `AGENTS.md` | [codex.md](codex.md) (+ Generic) | Default detect + default pair |
| `claude-md` | `CLAUDE.md` | [claude.md](claude.md) | Default detect + default pair |
| `copilot` | `.github/copilot-instructions.md` | [copilot.md](copilot.md) | Useful for CI / Copilot cloud agent |
| `cursor` | `.cursor/rules/frigg.mdc` | [cursor.md](cursor.md) | Detects `.cursor/rules/` |
| `mcp-project` | `.mcp.json` | Generic MCP row below | Project-level MCP (many hosts) |
| `mcp-cursor` | `.cursor/mcp.json` | [cursor.md](cursor.md) | Prefer project MCP over flaky global |
| `hook` | `.claude/settings.json` | [claude.md](claude.md) | **Opt-in only** — not in `--all` |

Gemini (`GEMINI.md`) is **not** an adopt target. Grok is **not** a host note (no 3C checklist).

## Skill install (`--skill-provider`, best-effort)

```bash
frigg adopt --skill-provider claude
frigg adopt --skill-provider copilot --skill-provider cursor
```

| Provider | Preferred skills parent (must already exist) | Fallback |
| --- | --- | --- |
| `claude` | `~/.claude/skills` | project `.claude/skills` |
| `codex` | `~/.codex/skills` | — |
| `cursor` | project `.cursor/skills` | `~/.cursor/skills` |
| `copilot` | project `.github/skills` (CI-friendly) | `~/.copilot/skills` |

**Never creates a missing parent `…/skills` directory.** If the parent is missing, adopt skips skill copy with `skills-parent-missing`. Source is workspace `skills/frigg-first-code-search` or `FRIGG_SKILL_SOURCE`.

## Host overview

| Host | Typical policy surface | Suggestion |
| --- | --- | --- |
| Claude Code | project `CLAUDE.md` / opt-in hooks | AGENTS snippet + optional PreToolUse; skill via `--skill-provider claude` |
| Codex | `AGENTS.md` | AGENTS snippet; skill via `--skill-provider codex` |
| Cursor | project rules / `.cursor` | Rules + project `.cursor/mcp.json`; skill via `--skill-provider cursor` |
| Copilot | `.github/copilot-instructions.md` + `.github/skills` | CI/cloud agent; skill via `--skill-provider copilot` |
| Generic MCP | MCP server config + skill | Register Frigg; load `frigg-first-code-search` skill |

See sibling files for slightly longer host-specific notes.
