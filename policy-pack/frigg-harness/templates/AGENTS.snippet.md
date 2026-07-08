# AGENTS.md snippet — lightweight Frigg pointer

Copy into the managed or hand-maintained agent instructions block. Keep this
short; scenario detail lives in the skill.

```markdown
## Frigg (code evidence)

Prefer Frigg MCP for indexed source discovery and proof:
- session: `workspace` first (gate / `recommended_action`)
- exact: `search_text` · multi-probe: `search_batch` · symbol: `search_symbol`
- discovery: `search_hybrid` (pivots only — confirm with exact tools)
- proof: `read_match` / `read_file` via handles

Skill (scenario-first policy home):
`skills/frigg-first-code-search/SKILL.md`

Do not use shell `rg`/grep as a quick check on indexed source when Frigg is up.
Shell stays correct for git, build/test, generated, ignored, and unindexed paths.
```

For Frigg-managed adopt defaults, prefer:

```bash
frigg adopt
# opt-in larger policy block:
frigg adopt --policy expanded
```
