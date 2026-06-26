DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: wontfix
Location: crates/cli/src/mcp/workspace_registry.rs:77 | Slug: dynamic-cli-reindex-repo-fork

# Dynamic Attach And CLI Reindex Fork Repository Identity

## Finding

Dynamically attached MCP workspaces use the stable root hash as both public and runtime repository id, while CLI commands for the same physical root write snapshots under positional ids such as `repo-001`.

## Violated Invariant Or Contract

One physical workspace database should use one repository identity for manifest snapshots across MCP and CLI entry points.

## Oracle

Startup workspaces explicitly bridge stable public ids to legacy runtime ids, and MCP reindex writes using `workspace.runtime_repository_id`. CLI reindex iterates `FriggConfig::repositories()`, which emits legacy positional ids.

## Counterexample

Start the MCP server without startup roots, attach a repository dynamically, and run `workspace_reindex`. The DB receives snapshots under the stable hash id. Later run CLI `reindex --workspace-root` for the same root. The CLI writes a new snapshot under `repo-001`, so the dynamic MCP session continues reading the stale stable-hash snapshot.

## Why It Might Matter

Users can refresh the same workspace through the CLI and still see stale MCP search/navigation state because the two entry points update different repository partitions in the same DB.

## Proof

Cross-entry persistent identity mismatch: dynamic `get_or_insert` passes the same stable id as both public and runtime ids through `insert_with_repository_id` at `crates/cli/src/mcp/workspace_registry.rs:77`. MCP reindex writes `workspace.runtime_repository_id` at `crates/cli/src/mcp/server.rs:1177`. CLI repositories are generated from `legacy_repository_id_for_workspace_index` at `crates/cli/src/settings/frigg_config.rs:143`.

## Counterevidence Checked

Startup-configured roots avoid this by keeping a stable public id and a legacy runtime id. The existing runtime-id report covers concurrent task aliasing for startup roots, not CLI-vs-dynamic persistent snapshot forking.

## Suggested Next Step

Give dynamically attached workspaces a runtime id compatible with CLI resolution, or make CLI commands resolve and update the stable repository id used by dynamic attachments.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-26: confirmed real, deferred (wontfix as a point patch — needs a dedicated identity-unification change). Verified: dynamic `get_or_insert` sets public == runtime == `stable_repository_id_for_root` (workspace_registry.rs:92-101); MCP reindex writes manifests under `workspace.runtime_repository_id` (server.rs:1178) = the stable hash; CLI `run_reindex_command` writes under `config.repositories()[i].repository_id` = `legacy_repository_id_for_workspace_index` = `repo-00N` (reindex.rs:38, frigg_config.rs:44,148). So a root attached dynamically via MCP and later reindexed by the CLI lands its snapshot in a different repository_id partition of the same DB, and the MCP session keeps reading the stale stable-hash partition. Startup-configured roots are unaffected (both MCP runtime id and CLI id are the positional `repo-00N`).

  Why no point fix here: neither narrow option is safe. (a) Giving dynamic attach a "CLI-compatible" runtime id is impossible — CLI ids are positional (`repo-{index}`) and depend on the CLI invocation's root ordering, so they are non-deterministic and would collide across dynamic attaches. (b) Switching CLI to the stable id would break the *startup* path, where MCP reads/writes under the legacy positional id, trading one divergence for another. The correct fix is to unify every entry point on `stable_repository_id_for_root`, i.e. make `FriggConfig::repositories()` emit the stable id (which also flips startup `runtime_repository_id` to stable, converging all three paths). But that ripples into: the watch scheduler, which keys repositories positionally as `repo-{index:03}` (scheduler.rs:219) and routes events by that id; searcher legacy-id resolution (searcher/mod.rs:359); and ~42 tests hardcoding `repo-00N`. It also strands existing on-disk snapshots written under legacy ids (stale until a full reindex) unless a re-key migration is added. That is a deliberate, separately-scoped migration with its own test matrix, not a safe batch-loop patch. Recommend a dedicated PR: unify on the stable id end-to-end (config + watch scheduler keying + searcher resolution + tests) with a one-time legacy→stable snapshot re-key on open. Interim guidance: drive a given root's reindex through one entry point (all-MCP or all-CLI), or include the root in startup config so both sides use the positional id.

DEVANA-KEY: crates/cli/src/mcp/workspace_registry.rs:77 | P1 | dynamic-cli-reindex-repo-fork
DEVANA-SUMMARY: Status=wontfix | P1 high crates/cli/src/mcp/workspace_registry.rs:77 - Confirmed real: dynamic-attach (stable hash id) and CLI reindex (legacy positional id) fork the snapshot partition for one DB. Deferred — a safe fix is a cross-cutting identity unification (config + watch scheduler + searcher + ~42 tests + on-disk re-key), not a point patch; narrow fixes introduce new divergences.
