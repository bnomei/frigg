# Codex

## Adopt targets

- Prefer managed `AGENTS.md`: `frigg adopt` (default) or `--target agents-md`.
- Expanded policy: `frigg adopt --policy expanded`.
- Project MCP when needed: `--target mcp-project`.

## Skill install (best-effort)

```bash
frigg adopt --skill-provider codex
```

| Scope | Parent dir (must already exist) | Dest |
| --- | --- | --- |
| Personal | `~/.codex/skills/` | `~/.codex/skills/frigg-first-code-search/` |

OpenAI ships system skills under `~/.codex/skills/.system`; drop custom skills as sibling directories with `SKILL.md`. Frigg never creates `~/.codex/skills` — enable/create skills support first, then re-run adopt.

Repo-vendored skill path (no global copy): `skills/frigg-first-code-search/SKILL.md`.

## Soft policy

- Soft shell justification template is optional; Codex policy should not re-order tools inside Frigg itself.
