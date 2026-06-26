DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: crates/cli/src/mcp/server/navigation_resolution.rs:704 | Slug: php-location-token-multibyte-slice-panic

# PHP/Blade location-token resolution panics slicing at a non-char boundary on a multibyte line

## Finding

When a navigation request resolves a target by `line`/`column` for a PHP or Blade
file, the helper `php_helper_string_token_around_offset` slices the source string
with `source[..offset]` and `source[offset..]`, where `offset` is derived from the
client-supplied `column`. The offset is clamped only to the line's byte length and
then re-clamped to `bytes.len() - 1`, neither of which respects UTF-8 char
boundaries. If the target line contains a multibyte character at the clamped
position, the slice indexes into a continuation byte and panics
("byte index N is not a char boundary").

## Violated Invariant Or Contract

Rust `&str` range indexing requires the index to fall on a UTF-8 char boundary;
otherwise it panics. The offset feeding `source[..offset]` is a byte offset that
may land mid-character.

## Oracle

Rust stdlib `str` `Index<Range<usize>>` contract. The sibling identifier helper in
the same file (`identifier_token_around_offset`) only slices on ASCII identifier
byte boundaries and is panic-free, showing the intended safety the PHP helper
violates.

## Counterexample

PHP file, single target line `echo café` (bytes: `e c h o ␠ c a f é`, where `é`
is `0xC3 0xA9` at byte indices 8,9; total length 10).
Navigation request: `language = php`, `line = 1`, `column = 999`.
- `byte_offset_for_line_column("echo café", 1, 999)`
  (`crates/cli/src/indexer/symbols/spans.rs:55,64`):
  `line_len = 10`, `column_offset = min(998, 10) = 10` → returns `10`.
- `php_helper_string_token_around_offset(source, 10)`
  (`navigation_resolution.rs:703`): `offset = 10.min(10-1) = 9`.
- `navigation_resolution.rs:704`: `source[..9]` — byte 9 is the `0xA9`
  continuation byte of `é` → panic.

A moderate column pointing directly into a multibyte character (e.g. `column = 5`
on a line beginning with `é`) panics at `source[..offset]` without needing the
overshoot re-clamp.

## Why It Might Matter

A client-controlled `column` (the request field is `Option<usize>`,
`crates/cli/src/mcp/types/navigation.rs:16`) triggers a panic in an MCP request
handler whenever the addressed PHP/Blade line contains a multibyte character (very
common in comments and string literals). At minimum the navigation request aborts;
depending on unwind handling it can degrade the handling task. It is a reachable,
deterministic logic defect (the workspace lints `clippy::panic = warn`).

## Proof

Control-flow + dataflow trace from user input to panicking slice:
- `navigation_resolution.rs:638` `byte_offset_for_line_column(&line_source, 1, column)`
  forwards the unvalidated request `column`.
- `spans.rs:55/65` clamps `column_offset` to the line's **byte** length, not a
  char boundary; returns a byte offset that can equal the line byte length.
- `navigation_resolution.rs:703` re-clamps to `bytes.len() - 1` (a continuation
  byte when the last char is multibyte).
- `navigation_resolution.rs:704` `source[..offset].rfind('\n')` slices at the
  non-boundary index → panic. (`:708` `source[offset..]` is a second instance,
  unreached because 704 fires first.)

## Counterevidence Checked

- `bytes.is_empty()` guard (`:700`) handles empty source but not multibyte mid-char.
- The path is gated to `matches!(language, Php | Blade)` (`:641`) but is reachable
  for any such file whose addressed line ends in / contains a multibyte char.
- The ASCII-quote helper `php_helper_token_for_quote_span` slices on ASCII quote
  indices and is safe; the identifier helper is safe; the defect is specific to
  this PHP string-token helper.
- `read_source_line_for_navigation` strips the trailing newline, so `source` is a
  single line and `line_start = 0`, matching the counterexample.

## Suggested Next Step

Snap `offset` down to a char boundary before slicing (e.g.
`while !source.is_char_boundary(offset) { offset -= 1; }`, or use
`source.floor_char_boundary`), or operate on `char_indices` instead of raw byte
slicing in `php_helper_string_token_around_offset`.

## Status Notes

- 2026-06-26: fixed. Confirmed `php_helper_string_token_around_offset` clamped `offset` to `bytes.len()-1` (a byte index that can land on a UTF-8 continuation byte) and then sliced `source[..offset]`/`source[offset..]`, panicking on a multibyte line. Added a snap-down loop (`while offset > 0 && !source.is_char_boundary(offset) { offset -= 1; }`) after the clamp so both slices fall on char boundaries. Added regression tests: a column overshoot on `echo café` (returns None without panic) and a sweep of every column over `é route('x')` (never panics). All 5 helper tests pass.

DEVANA-KEY: crates/cli/src/mcp/server/navigation_resolution.rs:704 | P2 | php-location-token-multibyte-slice-panic
DEVANA-SUMMARY: Status=fixed | P2 high crates/cli/src/mcp/server/navigation_resolution.rs:704 - PHP/Blade location-token helper now snaps the byte offset down to a UTF-8 char boundary before slicing, so a client-supplied column no longer panics on a multibyte line (regression tests added).
