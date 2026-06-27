DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: crates/cli/src/searcher/lexical_channel.rs:271 | Slug: path-witness-anchor-skips-markdown-comment-scrub

# Path-witness anchor channel surfaces leading markdown HTML comments the lexical channel deliberately scrubs

## Finding

For markdown files, the lexical search channel deliberately routes content through
`scrub_search_content`, which blanks a leading `<!-- ... -->` HTML comment (while
preserving line numbers). The path-witness anchor channel reads the same file
bytes raw and selects the first non-empty line — including a leading HTML comment —
as the excerpt, with no scrub applied. The two channels thus disagree: the same
markdown file's hidden leading comment is redacted on one excerpt-producing path
and surfaced verbatim on the other, reaching the user-visible `TextMatch.excerpt`.

## Violated Invariant Or Contract

All search excerpts derived from a markdown file must have the leading HTML comment
redacted, as enforced by `content_scrub::scrub_search_content` /
`should_scrub_leading_markdown_comment` and the routing that sends markdown to the
native scanner specifically so the scrub runs. The path-witness anchor channel is
a parallel excerpt source that omits this redaction.

## Oracle

- `crates/cli/src/searcher/content_scrub.rs` — `scrub_search_content` applies
  `scrub_leading_html_comment` for `.md/.markdown/.mdown`.
- `crates/cli/src/searcher/scan_engine.rs:199,244-258` — the native lexical scan
  scrubs before producing excerpts.
- `crates/cli/src/text_sanitization.rs` test `scrub_leading_html_comment_preserves_line_numbers`
  asserts the scrubbed output does not contain the leading-comment body — i.e. the
  redaction is intentional, not incidental.
The path-witness anchor builder applies none of this, contradicting the established
contract.

## Counterexample

`README.md`:
```
<!-- frigg-internal: hidden note -->
# Project
```
A query with path-witness recall intent over a generic runtime-witness doc
(`README.md` qualifies). In `best_path_witness_anchor_in_bytes` the first non-empty
line `<!-- frigg-internal: hidden note -->` becomes `first_non_empty`; if no later
line out-scores it, it is returned as the excerpt and flows into `TextMatch.excerpt`
unredacted — whereas the lexical channel would have blanked that line.

## Why It Might Matter

Content the system goes out of its way to redact from markdown search excerpts
(hidden leading HTML comments — a hidden-instruction / metadata hygiene measure) is
disclosed through a second channel, undermining the redaction. Impact is bounded to
leading markdown comments, hence P2 rather than higher.

## Proof

Cross-entry mismatch + dataflow trace:
- Lexical path scrubs: `scan_engine.rs:199` `scrub_search_content(rel_path, &content)`;
  markdown routed to native scanner so this runs.
- Path-witness path does not: `crates/cli/src/searcher/lexical_channel.rs:267`
  `fs::read(file_path)` → `:271 best_path_witness_anchor_in_bytes` →
  `:281-286` keeps first non-empty line as excerpt; no scrub call.
- Excerpt reaches output: consumed at
  `crates/cli/src/searcher/path_witness_search.rs` where the `(line, excerpt)` pair
  is placed into `TextMatch { excerpt, .. }` and returned, with no downstream scrub.

## Counterevidence Checked

- Verified no downstream scrub exists between the path-witness excerpt and final
  ranking/output in the hybrid pipeline.
- Ripgrep routing scrub protects only the lexical path, not the witness path.
- The fallback `(line, rel_path)` (path string, not content) is only used when no
  anchor exists; with a leading comment present an anchor always exists, so the raw
  line is returned.
- Markdown docs are exactly the generic runtime-witness docs this channel targets,
  so the bypass is reachable, not theoretical.

## Suggested Next Step

Apply `scrub_search_content(path, ...)` (or skip the leading-comment line) in
`best_path_witness_anchor_in_bytes` / `best_path_witness_anchor_in_file` before
selecting/returning an excerpt, matching the lexical channel.

## Status Notes

- 2026-06-26: fixed. Confirmed `best_path_witness_anchor_in_bytes` selected excerpts from raw file bytes with no scrub, while the lexical channel routes markdown through `scrub_search_content`. Added the same scrub at the top of `best_path_witness_anchor_in_bytes`: for paths matching `should_scrub_leading_markdown_comment`, the UTF-8 content is run through `scrub_search_content` (leading HTML comment blanked, line numbers preserved) and the scanner operates on the scrubbed bytes; non-markdown or non-UTF-8 input falls back to the raw bytes unchanged. This covers both `best_path_witness_anchor_in_file` and the test reader. Added regression tests: a README.md with a leading `<!-- frigg-internal ... -->` now yields the heading on line 2 (comment scrubbed, line numbers preserved), and a non-markdown `.txt` leading comment is left intact. All 4 anchor tests pass.
- 2026-06-27: reopened after fixed-status audit. The direct file-anchor path is fixed, but the persisted `path_anchor_sketch` projection path remains unsanitized: `build_path_anchor_sketch_projection_records` reads markdown with `fs::read_to_string`, iterates raw `contents.lines()`, and stores `trim_excerpt(trimmed)` without `scrub_search_content`. `PathWitnessSearcher` prefers `projected_anchor` over the now-scrubbed `best_path_witness_anchor_in_file` fallback unless a build-flow upgrade is needed, so a persisted sketch for `README.md` can still surface a leading `<!-- ... -->` excerpt. Remaining fix should apply the same markdown leading-comment scrub before ranking/storing projected path-anchor sketches, or scrub projected anchors before returning them.
- 2026-06-27: fixed. The persisted anchor-sketch path now applies `content_scrub::scrub_search_content` immediately after `fs::read_to_string` and before `.lines()` ranking/storing, so markdown leading comments are blanked with line numbers preserved before any `PathAnchorSketchProjection.excerpt` can be written. Bumped `PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION` from 1 to 2 so existing pre-fix v1 sketches are treated as stale and rebuilt instead of being trusted by read-only loaders/reused snapshots. Added focused regression coverage: `path_anchor_sketch_scrubs_leading_markdown_html_comment` verifies `README.md` persists the visible heading on line 2 instead of the hidden comment, `path_anchor_sketch_keeps_non_markdown_leading_comment` verifies non-markdown files keep their leading comment behavior, and the projection-version test verifies an old v1 path-anchor-sketch head is stale against the current runtime version.

DEVANA-KEY: crates/cli/src/searcher/lexical_channel.rs:271 | P2 | path-witness-anchor-skips-markdown-comment-scrub
DEVANA-SUMMARY: Status=fixed | P2 medium crates/cli/src/searcher/lexical_channel.rs:271 - Direct file anchors and persisted path_anchor_sketch excerpts now both apply markdown leading-comment scrubbing, and path_anchor_sketch v2 forces stale pre-fix sketches to rebuild.
