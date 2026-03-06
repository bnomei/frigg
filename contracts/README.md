# Contracts

This directory stores versioned public contracts for Frigg:

- MCP tool schemas (`contracts/tools/`)
- error taxonomy (`contracts/errors.md`)
- runtime config keys/defaults (`contracts/config.md`)
- semantic embedding provider/runtime contract (`contracts/semantic.md`)
- storage schema/runtime contract (`contracts/storage.md`)
- compatibility changelog (`contracts/changelog.md`)

Any public behavior or contract change must append a deterministic entry to `contracts/changelog.md`.

## Repository ID stability semantics

`list_repositories` IDs are generated from `FriggConfig.workspace_roots` in positional order (`repo-001`, `repo-002`, ...).
These IDs are not globally stable identifiers.
Reordering, inserting, or removing roots can change IDs, so clients must refresh IDs from `list_repositories` and treat them as config-snapshot scoped.
