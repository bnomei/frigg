# Claude Code

- Install skill via `frigg adopt` (copies / registers skill assets as configured).
- Keep `CLAUDE.md` / `AGENTS.md` lightweight: use
  [`../templates/AGENTS.snippet.md`](../templates/AGENTS.snippet.md).
- Optional PreToolUse hook: paste soft justification from
  [`../templates/shell-indexed-src-justification.md`](../templates/shell-indexed-src-justification.md)
  when Bash/`rg` targets indexed source.
- Do not treat harness flukes as Frigg product bugs (`FUT-003`).
