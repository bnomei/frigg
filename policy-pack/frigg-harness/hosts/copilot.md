# GitHub Copilot

Copilot is a first-class **CI / cloud-agent** surface for Frigg policy and skills.

## Adopt targets

- Managed instructions: `frigg adopt --target copilot`  
  → `.github/copilot-instructions.md`
- Optional shared AGENTS pointer: `frigg adopt --target agents-md`
- Project MCP (if your Copilot / VS Code setup reads it): `--target mcp-project`

## Skill install (best-effort)

```bash
# Prefer project skills so CI / cloud agent see the same tree as the repo
frigg adopt --skill-provider copilot
```

| Scope | Parent dir (must already exist) | Dest skill |
| --- | --- | --- |
| **Project (preferred for CI)** | `.github/skills/` | `.github/skills/frigg-first-code-search/` |
| Personal | `~/.copilot/skills/` | `~/.copilot/skills/frigg-first-code-search/` |

Frigg **does not** create `.github/skills` or `~/.copilot/skills`. Create the parent once (or let your host skill tooling do it), then re-run adopt.

Also accepted by Copilot docs: project `.claude/skills` / `.agents/skills`, personal `~/.agents/skills` — use those only if your team already standardizes on them; Frigg’s copilot provider targets the GitHub-native paths above.

## Operator checklist

1. `frigg adopt --target copilot` (and MCP if needed).
2. Ensure `.github/skills` exists if you want repo-shared skill install.
3. `frigg adopt --skill-provider copilot`.
4. In CI: `frigg adopt --check` for managed blocks; skill tree is optional filesystem state (not required for Frigg core tests).

## Soft policy

- Point instructions at the production skill; do not paste the full scenario skill into copilot-instructions.
- Soft shell justification template is optional; do not hard-block shell tools from Frigg product code.
