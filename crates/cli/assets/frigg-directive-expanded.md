Frigg is the default for code discovery, file listing, navigation, exact code search, and bounded source reads.

Before using shell `rg`, `grep`, `find`, `fd`, `cat`, or `sed` for code exploration, use the matching Frigg MCP tool in attached or attachable code repositories.

Shell tools are fallback only for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, or when Frigg is unavailable.

Use the `frigg-first-code-search` skill for full scenario cards, hard anti-patterns, and done criteria. This expanded block is a compact policy pack for repos that want routing detail without loading the skill every turn.

## Compact scenario picker

```text
Known string or regex -> search_text
Known function/type/API name -> search_symbol
Vague "where is X?" -> search_hybrid, then exact search
Several guesses -> search_batch or parallel search_text
Need proof -> read_match, then read_file
Need impact -> find_references, incoming_calls, implementations
  (or impact_bundle when available)
Wrong repo, stale index, surprising zero -> workspace
Git/build/generated/unindexed/Frigg missing -> shell fallback
```

## Shell → Frigg (one-liners)

| Shell habit | Frigg call |
| --- | --- |
| `rg -n PATTERN` | `search_text`, `query=PATTERN` |
| `rg` with regex | `search_text`, `pattern_type=regex` |
| `rg --files` / `find` / `fd` | `list_files` |
| `cat path` | `read_file` |
| `sed -n 'A,Bp'` | `read_file`, `start_line` / `end_line` |

## Positive fallback boundary

Keep shell for git state and diffs, build/test output, generated or ignored paths, explicit live-disk checks, and when Frigg is unavailable. Do not run parallel shell grep on indexed source in the same turn as Frigg search.
