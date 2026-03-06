# Requirements — 01-storage-and-repo-state

## Scope
Create persistent repository/snapshot/storage foundations for indexing, retrieval, and provenance.

## EARS requirements
- The storage subsystem shall persist repository metadata, snapshots, file manifests, and provenance events in SQLite.
- When `init` is invoked, the system shall create required storage schema and verify extension/runtime compatibility checks.
- When repository snapshot state changes, the system shall persist deterministic identifiers for replay and diff operations.
- If schema migration fails, then the system shall return a typed startup error and abort serving MCP tools.
- While operating in local-first mode, the system shall keep all primary state on local disk by default.
