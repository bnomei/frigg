DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: crates/cli/src/languages/php/evidence.rs:76 | Slug: php-use-alias-global-context

# PHP Use Aliases Use A Global Namespace Context

## Finding

PHP source evidence builds one name-resolution context for the whole file, so `use` aliases from multiple namespace blocks share one alias map and one final namespace.

## Violated Invariant Or Contract

PHP `use` aliases are scoped to their namespace block. Target evidence for class-like references should resolve under the lexical namespace containing the expression.

## Oracle

A bracketed PHP file can contain multiple namespace blocks with different aliases for the same short class name.

## Counterexample

One file contains `namespace App\A { use Vendor\One\Target; new Target(); }` and `namespace App\B { use Vendor\Two\Target; }`. Evidence for `new Target()` in `App\A` can resolve through the global context that also collected the `App\B` alias.

## Why It Might Matter

Persisted path relation edges can point to the wrong target class or miss the correct target, degrading PHP relation-aware search and navigation.

## Proof

Contract mismatch: `extract_php_source_evidence_from_source` builds one context at `crates/cli/src/languages/php/evidence.rs:76`. `php_name_resolution_context_from_root` overwrites `context.namespace` and collects all namespace use declarations into that context at `crates/cli/src/languages/php/resolution.rs:98`. Later target collectors resolve references through that shared context rather than a namespace-local alias stack.

## Counterevidence Checked

Symbol names track `next_namespace`, and single-namespace files are unaffected. No per-namespace alias reset or stack was found for target resolution.

## Suggested Next Step

Carry namespace-local resolution contexts while walking PHP namespace bodies, and resolve target evidence with the context active at the reference node.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: fixed. Confirmed `php_name_resolution_context_from_root` overwrote `context.namespace` to the last block and merged every block's `use` declarations into one alias map, and `collect_source_evidence` resolved every reference through that single file-wide context. Added `php_name_resolution_context_for_namespace_node` (resolution.rs) which builds a context scoped to a single bracketed `namespace X { ... }` block (its name + only its body `use` aliases). In `collect_source_evidence`, a `namespace_definition` node with a body now builds that namespace-local context and passes it as the effective context for the block's subtree; non-bracketed namespaces (no body) and single-namespace files keep using the inherited file-wide context, so the common case is unchanged. Added regression test `php_source_evidence_resolves_aliases_per_namespace_block` (two bracketed namespaces aliasing `Target` to `Vendor\One\Target` and `Vendor\Two\Target`; each `new Target()` resolves under its own block — pre-fix both collapsed to the last alias). New test + existing 5 PHP evidence tests pass.

DEVANA-KEY: crates/cli/src/languages/php/evidence.rs:76 | P2 | php-use-alias-global-context
DEVANA-SUMMARY: Status=fixed | P2 medium crates/cli/src/languages/php/evidence.rs:76 - PHP evidence now resolves references under a namespace-block-local use-alias context for bracketed multi-namespace files instead of a merged file-wide context (regression test added).
