# Futura synthetic fixtures (`frigg-futura-bench`)

Tiny trees used by `cargo test -p frigg --test futura_bench` to force edge cases
that are awkward to stage cleanly on the full dogfood tree:

- regex-looking literal zero (`QUERY_LOOKS_LIKE_REGEX`)
- `count_only` shape
- zero-hit recovery fields
- handle / `read_match` chain

## Layout

- `seed/` — source content copied into a temp workspace with a `.git` marker
  before each synth scenario. The harness does not mutate this directory.

## Adding a fixture

1. Add files under `seed/` (or a sibling scenario folder if isolation is needed).
2. Wire a scenario in `crates/cli/tests/futura_bench/synth.rs` tagged `synth`.
3. Prefer unique sentinel strings so probes cannot accidentally hit dogfood roots.
4. Document the forced `zero_hit_reason` / recovery code in the scenario name.

See `docs/futura.md` §21 and `docs/futura-roadmap.md` Phase 7.
