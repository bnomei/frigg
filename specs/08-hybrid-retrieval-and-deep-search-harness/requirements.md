# Requirements — 08-hybrid-retrieval-and-deep-search-harness

## Scope
Implement hybrid retrieval orchestration and replayable deep-search execution harness.

## EARS requirements
- When users submit natural-language questions, the retrieval subsystem shall combine lexical, graph, and semantic retrieval channels.
- When the same query and repository state are replayed, the retrieval subsystem shall produce deterministic ordered evidence sets.
- While composing final answers, the deep-search harness shall include explicit source citations derived from tool traces.
- If one retrieval channel fails, then the deep-search harness shall degrade gracefully and report partial-channel behavior.
- The deep-search harness shall persist replayable tool-call traces for benchmark and regression validation.
