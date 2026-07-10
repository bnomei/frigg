# Claude Code

## Adopt targets

| Flag | Path | Role |
| --- | --- | --- |
| `--target claude-md` | `CLAUDE.md` | Managed Frigg-first directive (default pair) |
| `--target agents-md` | `AGENTS.md` | Shared lightweight pointer (default pair) |
| `--target mcp-project` | `.mcp.json` | Project MCP (HTTP loopback) |
| `--target hook` | `.claude/settings.json` | **Opt-in** soft PreToolUse nudge only |

Hook is **not** included in `frigg adopt --all`. Use explicit `--target hook` when you want soft Grep/Bash/Read nudges (`frigg hook pretooluse`).

**Soft only — product never hard-denies Grep/shell.** The hook injects `additionalContext` (richer next-step checklist: `search_text` / `search_batch` / …). It does **not** set `permissionDecision` allow/deny. There is no `FRIGG_HOOK_STRICT` deny mode in Frigg core. Hosts may experiment with harder shell preference **outside** Frigg; measure Frigg mix via opt-in `FRIGG_ROUTING_STATS` / `frigg stats`, not shell-deny rates.

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

`frigg adopt` without `--skill-provider` only writes managed markdown/MCP/hook — it does **not** copy the skill tree. `--skill-provider` is additive (normal targets still run). Skill uninstall requires `frigg adopt --uninstall --skill-provider claude`.

## Soft policy

- Keep `CLAUDE.md` / `AGENTS.md` lightweight: use
  [`../templates/AGENTS.snippet.md`](../templates/AGENTS.snippet.md).
- Optional PreToolUse: opt-in hook nudge is aligned with
  [`../templates/shell-indexed-src-justification.md`](../templates/shell-indexed-src-justification.md)
  (preferred Frigg path + when shell is still OK). You can also paste that template into host checklists.
- **Hard block / Grep hide / tool-order hacks are host experiments**, not Frigg product (FUT-001/021). Soft nudge does not reorder the tool menu.
- Do not treat harness flukes (Grep-first, flaky MCP bridge) as Frigg ranking bugs.
