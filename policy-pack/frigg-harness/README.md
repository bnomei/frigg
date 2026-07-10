# Optional harness policy pack

**Status:** optional templates only. **Not required** for Frigg core correctness or CI.

Frigg owns evidence (workspace, search, proof handles, recovery). Host harnesses
(Claude Code, Codex, Cursor, Copilot, …) own tool ordering and shell policy. This
pack ships soft templates you can copy into a host repo when you want harness
guidance that prefers Frigg for indexed source without making Frigg depend on
any particular host.

## Install triangle

| Surface | Role |
| --- | --- |
| Production skill | Behavior SSOT (scenarios) |
| Live `tools/list` | Existence SSOT |
| `frigg adopt` | Install plumbing |
| This pack | Optional host paste |

See [`hosts/README.md`](hosts/README.md) for the full adopt-target matrix and skill-provider paths.

## What this is

| Asset | Purpose |
| --- | --- |
| [`templates/shell-indexed-src-justification.md`](templates/shell-indexed-src-justification.md) | Soft justification when shell `rg`/grep hits indexed `src/**` (or runtime roots) while Frigg is registered |
| [`templates/AGENTS.snippet.md`](templates/AGENTS.snippet.md) | Lightweight `AGENTS.md` pointer at the production skill |
| [`hosts/`](hosts/) | Short notes for Claude, Codex, Cursor, Copilot + matrix |

## Soft shell vs hard block

| Mechanism | Frigg product? | Blocks shell? |
| --- | --- | --- |
| Skill / AGENTS text | Yes | No |
| Soft justification template | Optional pack | No |
| Claude PreToolUse (`frigg adopt --target hook`) | Yes, **opt-in** | **No** — `additionalContext` only |
| Host Grep deprioritization / hard deny | **Host only** | Host decides |

Do **not** file product bugs for “agent still grepped.” Soft policy does not reorder tools.
Success = better Frigg paths when registered (`search_batch`, recovery), not shell deny %.

## What this is not

- Not a Frigg runtime dependency
- Not enforced by `cargo test` or Frigg MCP
- Not a replacement for `skills/frigg-first-code-search/SKILL.md`
- Not a place to hardcode host tool-order hacks or Grep deny into Frigg product code

## Install (optional)

1. Use `frigg adopt` for managed directives / MCP / opt-in Claude hook.
2. Optionally install the production skill into a host skills dir that **already exists**:
   `frigg adopt --skill-provider claude|codex|cursor|copilot` (never creates the parent `…/skills` folder).
3. Optionally copy snippets from `templates/` into host policy files.
4. Prefer the production skill as the single scenario-first policy home.

## Related

- Skill: `skills/frigg-first-code-search/SKILL.md`
