DEVANA-FINDING: v1
DEVANA-STATE: wontfix | P1 | high | security=yes
DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/generation.rs:806 | precise-generator-extra-args

# Repository-local precise.json passes unvalidated argv to SCIP generators

## Finding

`load_workspace_precise_config` reads `.frigg/precise.json` from the adopted workspace. `generator_extra_args` are cloned verbatim into `run_precise_generator_command`, which appends them to the external generator subprocess after fixed generator args.

## Violated Invariant Or Contract

Repository-local configuration must not pass unvalidated argv to external SCIP generators executed under Frigg's OS identity without an explicit operator opt-in.

## Oracle

README documents `generator_extra_args` as intentional extensibility. Runtime has no allowlist or shell-metacharacter rejection before `command.args(request.generator_extra_args.iter())` (`generation.rs:806-807`). SCIP path ingest rejects `..` but generator argv is unconstrained.

## Counterexample

An adopted workspace contains `.frigg/precise.json` with `generator_extra_args: ["--some-flag", "../../../outside"]` or other attacker-influenced argv. `maybe_spawn_workspace_precise_generation` loads config and spawns `rust-analyzer` / `scip-go` with those args as Frigg.

## Why It Might Matter

Local write access to `.frigg/precise.json` (copied config, shared machine, or compromised repo tooling) becomes subprocess argument injection under Frigg privileges during attach or index precise warming.

## Proof

**Actor-to-resource trace:** adopt workspace → load `precise.json` → append `generator_extra_args` → `Command::output()` as Frigg user.

## Counterevidence Checked

`.frigg` is gitignored, so remote clone alone does not place the file; local influence is still realistic. Documented feature explains intent but provides no runtime guardrail.

## Suggested Next Step

Allowlist per generator, reject absolute paths and shell metacharacters in extra args, or require `FRIGG_ALLOW_PRECISE_EXTRA_ARGS=1` before honoring the field.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-07-05: open by Devana. Initial report written from static source inspection.
- 2026-07-05: wontfix by user decision; no code change made. The report remains as a durable record of the accepted risk around documented repository-local `generator_extra_args` extensibility.

DEVANA-KEY: crates/cli/src/mcp/server/precise_graph/generation.rs:806 | precise-generator-extra-args
DEVANA-SUMMARY: wontfix | P1 | high | Unvalidated generator_extra_args from .frigg/precise.json are intentionally left as documented repository-local SCIP generator extensibility.
