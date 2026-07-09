# Optional harness policy pack

**Status:** optional templates only. **Not required** for Frigg core correctness or CI.

Frigg owns evidence (workspace, search, proof handles, recovery). Host harnesses
(Claude Code, Codex, Cursor, Grok, …) own tool ordering and shell policy. This
pack ships soft templates you can copy into a host repo when you want harness
guidance that prefers Frigg for indexed source without making Frigg depend on
any particular host.

## What this is

| Asset | Purpose |
| --- | --- |
| [`templates/shell-indexed-src-justification.md`](templates/shell-indexed-src-justification.md) | Soft justification when shell `rg`/grep hits indexed `src/**` (or runtime roots) while Frigg is registered |
| [`templates/AGENTS.snippet.md`](templates/AGENTS.snippet.md) | Lightweight `AGENTS.md` pointer at the production skill |
| [`hosts/`](hosts/) | Short notes for common hosts |

## What this is not

- Not a Frigg runtime dependency
- Not enforced by `cargo test` or Frigg MCP
- Not a replacement for `skills/frigg-first-code-search/SKILL.md`
- Not a place to hardcode Grok/Cursor tool-order hacks into Frigg product code

## Install (optional)

1. Keep using `frigg adopt` for the default lightweight directive + skill install.
2. Optionally copy snippets from `templates/` into your host policy files.
3. Prefer the production skill as the single scenario-first policy home.

## Related

- Skill: `skills/frigg-first-code-search/SKILL.md`
