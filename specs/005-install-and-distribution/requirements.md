# Requirements: Install And Distribution

## Functional Requirements

R001: WHEN a Unix user runs the installer THE SYSTEM SHALL download the matching GitHub Release archive for their platform.

R002: WHEN the installer downloads an archive THE SYSTEM SHALL verify the matching `.sha256` before installing or fail without installing.

R003: WHEN README presents installation THE DOCUMENTATION SHALL lead with prebuilt install paths before `cargo install`.

R004: WHEN docs mention client setup THE DOCUMENTATION SHALL not claim `frigg adopt` behavior until that command exists.

R005: WHEN docs show GitHub Actions usage THE DOCUMENTATION SHALL include a no-cache workflow that installs a pinned Frigg version and refreshes/verifies repository state.

R006: WHEN docs show cached GitHub Actions usage THE DOCUMENTATION SHALL key caches by Frigg version and runner platform and refresh/verify Frigg state after restore.

R007: WHEN docs discuss caching `.frigg/` THE DOCUMENTATION SHALL warn that cache contents are not a secret store and SHALL save caches only from trusted triggers.

R008: WHEN the installer runs THE SYSTEM SHALL only install the `frigg` binary and SHALL NOT run workspace setup, indexing, serving, client registration, or adopt-like behavior.

R009: WHEN Frigg's own CI runs install/cache dogfood THE WORKFLOW SHALL exercise the documented install plus `init`/`reindex`/`verify` path and SHALL save caches only from trusted events.

R010: WHEN the cache fingerprint command runs THE SYSTEM SHALL emit exactly one Frigg-owned stable hash output without mutating repository state or owning installer paths.

## Acceptance Anchors

- Installer dry-run and target-mapping tests cover Linux GNU x86_64/aarch64 and macOS x86_64/aarch64.
- Installer fixture tests cover checksum success and checksum failure.
- `frigg hash` tests prove one deterministic `frigg-hash=<hex>` output and `$GITHUB_OUTPUT` behavior.
- README snippets show no-cache first and post-restore refresh for `.frigg/` caches.
- Dogfood workflow has trusted-trigger cache save guards.
