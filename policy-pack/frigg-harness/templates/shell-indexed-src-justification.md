# Soft shell justification (indexed source while Frigg registered)

Use this template in host pre-tool hooks, reviewer checklists, or agent
self-critique prompts. It does **not** block shell; it asks for a one-line reason
when shell search touches indexed application source while Frigg MCP is available.

**Product stance (FUT-021):** Frigg’s Claude PreToolUse hook (`frigg hook pretooluse`) is
**soft only** — `additionalContext` checklist, never `permissionDecision` deny.
Hard deny of Grep/`rg` on indexed roots is a **host-side experiment**, not Frigg core.
Prefer measuring Frigg tool mix (`FRIGG_ROUTING_STATS`) over shell-deny rates.
The live hook text (`HOOK_NUDGE`) is a compact subset of the checklist below.

## When to apply

- Frigg MCP tools are registered in the session (`workspace`, `search_text`, …)
- The path class is indexed runtime/project source (examples: `src/**`,
  `crates/**`, `app/**`, `lib/**` — adjust to the repo)
- The agent chose shell `rg` / `grep` / `find` / `fd` / bulk `cat` instead of Frigg

## Prompt / checklist text

```text
Shell search hit indexed source while Frigg is registered.
One-line justification required (pick one):
- Frigg unavailable / tools/list missing search tools
- Path is ignored, generated, or outside index
- Live-disk / dirty worktree path Frigg advised via recommended_action
- Non-source artifact (build log, binary, lockfile)
- Other: <reason>

Preferred Frigg path for this habit:
- exact string → search_text
- several guesses → search_batch (or parallel Frigg probes if batch missing)
- known symbol → search_symbol → navigation
- broad discovery → search_hybrid → exact proof (not rank-1 alone)
```

## Positive fallbacks (do not shame)

Shell remains correct for: git, build/test runners, generated output, gitignored
docs, unindexed trees, Frigg-unavailable sessions, and operator-direct reads.
