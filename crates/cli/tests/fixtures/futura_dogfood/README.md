# Dogfood-shaped fixture for `frigg-futura-bench`

Mirrors the Frigg monorepo layout (`crates/cli/src/mcp/…`, `skills/…`) with
stable public anchors (`PUBLIC_TOOL_NAMES`, `FriggMcpServer`, hybrid discovery
phrases) so CI can score the dogfood board without indexing the entire live
checkout on every run.

## Live dogfood pin

```bash
FUTURA_BENCH_DOGFOOD_ROOT=/path/to/frigg \
  cargo test -p frigg --test futura_bench -- --nocapture
```

Live mode adopts the real repository root under test (slower; may write
`.frigg/` state which is gitignored).
