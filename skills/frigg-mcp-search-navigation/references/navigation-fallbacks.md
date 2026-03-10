# Navigation Fallbacks

## Default Expectation

Frigg navigation is precise-first and may degrade to deterministic heuristic behavior when precise SCIP data is absent or partial.

Affected tools:

- `find_references`
- `go_to_definition`
- `find_declarations`
- `find_implementations`
- `incoming_calls`
- `outgoing_calls`

## What To Do When Navigation Underfills

- Keep the selected anchor concrete: prefer source position or exact symbol where possible.
- Use `search_symbol` first when the name is known but the navigation target is ambiguous.
- Use `document_symbols` when you need the file outline and enclosing scopes.
- Use `search_structural` when syntax shape matters more than symbol metadata.
- Use a shell read when you already have a concrete local file path and only need a quick inspection.
- Use `read_file` to confirm the final answer only when you need repository-aware evidence; otherwise use a shell read.

## Practical Fallback Patterns

- `find_implementations` underfills: try `search_structural` for impl or class-shape queries, then confirm with `document_symbols`, a shell read, or `read_file` when canonical paths matter.
- call hierarchy underfills: use `find_references` plus a shell read or `read_file`.
- overloaded names: add `repository_id`, `path_class`, or `path_regex` before retrying.

The note and metadata payloads explain whether the result was precise or heuristic. Read the contract docs when the distinction matters to the answer.

## When Precise Data Is Missing Entirely

If note metadata reports `precise_absence_reason=no_scip_artifacts_discovered`, Frigg likely has no external SCIP artifacts to ingest for that repository.

What to check:

- the repository root contains `.frigg/scip/`
- the directory contains `.scip` or `.json` artifacts
- the artifacts were generated from the repository root so document paths match Frigg's canonical repository-relative paths

Typical generators:

- Rust: `rust-analyzer scip . > .frigg/scip/rust.scip`
- PHP: run `vendor/bin/scip-php`, then move `index.scip` into `.frigg/scip/`
- TypeScript / TSX: run `scip-typescript index`, then move `index.scip` into `.frigg/scip/`
- Python: run `scip-python index . --project-name=<repo-name>`, then move `index.scip` into `.frigg/scip/`

Be explicit that SCIP generation is outside Frigg itself.
Frigg consumes those artifacts to improve precise-first navigation; it does not ship language indexers.
