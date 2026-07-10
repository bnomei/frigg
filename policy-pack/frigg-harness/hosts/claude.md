# Claude Code

## Adopt targets

| Flag | Path | Role |
| --- | --- | --- |
| `--target claude-md` | `CLAUDE.md` | Managed Frigg-first directive (default pair) |
| `--target agents-md` | `AGENTS.md` | Shared lightweight pointer (default pair) |
| `--target mcp-project` | `.mcp.json` | Project MCP (HTTP loopback) |
| `--target hook` | `.claude/settings.json` | **Opt-in** soft PreToolUse nudge only |

Hook is **not** included in `frigg adopt --all`. Use explicit `--target hook` when you want soft Grep/Bash/Read nudges (`frigg hook pretooluse`). Soft only — no hard deny.

Recommended Claude funnel:

```bash
frigg adopt --target agents-md --target claude-md --target mcp-project
# then decide:
frigg adopt --target hook
```

## Skill install (best-effort)

```bash
frigg adopt --skill-provider claude
```

| Scope | Parent dir (must already exist) | Dest |
| --- | --- | --- |
| **Personal (prefer)** | `~/.claude/skills/` | `~/.claude/skills/frigg-first-code-search/` |
| Project | `.claude/skills/` | `.claude/skills/frigg-first-code-search/` |

Frigg **does not** create `~/.claude/skills` or `.claude/skills`. Create the parent once (Claude skill tooling / manual), then re-run adopt. Source: workspace `skills/frigg-first-code-search` or `FRIGG_SKILL_SOURCE`.

`frigg adopt` without `--skill-provider` only writes managed markdown/MCP/hook — it does **not** copy the skill tree.

## Soft policy

- Keep `CLAUDE.md` / `AGENTS.md` lightweight: use
  [`../templates/AGENTS.snippet.md`](../templates/AGENTS.snippet.md).
- Optional PreToolUse: paste soft justification from
  [`../templates/shell-indexed-src-justification.md`](../templates/shell-indexed-src-justification.md)
  when Bash/`rg` targets indexed source — or rely on the opt-in hook nudge.
- Do not treat harness flukes as Frigg product bugs.
