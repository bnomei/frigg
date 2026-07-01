# Design: Install And Distribution

## Objective

Make Frigg installable from prebuilt GitHub Release assets through a claim-safe Unix installer, document GitHub Actions usage with and without cache, and dogfood the documented install/cache path in this repository.

## Scope

In scope:

- `scripts/install.sh` and optional shell test helpers under `scripts/`.
- README install ordering, manual release asset notes, and GitHub Actions examples.
- `.github/workflows/frigg-install-cache.yml`.
- `frigg hash`, a read-only cache fingerprint command that emits one machine output.

Out of scope:

- npm, Docker, Scoop, or new registry publishing.
- Windows PowerShell installer.
- `frigg adopt` implementation or any docs claiming adopt behavior.
- New release asset naming, versionless aliases, artifact attestations, or musl/Alpine support.

## Current State

- README currently leads with `cargo install frigg` at `README.md:31`; Homebrew is already documented at `README.md:35`.
- Release workflow builds five targets at `.github/workflows/release.yml:25`; the Unix installer should cover the four Unix targets and leave Windows as manual release-asset download.
- Linux release targets are GNU libc targets at `.github/workflows/release.yml:28` and `.github/workflows/release.yml:31`.
- Release workflow uploads `dist/*` at `.github/workflows/release.yml:103`.
- Unix archive names are `frigg-v<VERSION>-<TARGET>.tar.gz` from `scripts/package-release.sh:44`; `.sha256` files are produced at `scripts/package-release.sh:48`.
- `scripts/resolve-version.sh:6` strips a leading `v`, so release tags are `v0.5.0` while archive names include `frigg-v0.5.0-...`.
- `frigg init`, `frigg verify`, and `frigg reindex --changed` are CLI subcommands in `crates/cli/src/cli_args.rs`.
- There is no `Adopt` CLI command variant, so docs must not claim one-command registration.
- Utility commands default to the current directory when no workspace root is provided.
- `frigg reindex --changed` falls back to full indexing when no previous manifest exists.
- Storage schema migrations live in `crates/cli/src/storage/schema.rs`; retrieval projection heuristic versions live in searcher/projection modules and are checked during reindex.

## Architecture

### Installer

Add `scripts/install.sh` as POSIX `sh`, not Bash. It supports:

- `FRIGG_VERSION`, accepting `0.5.0` or `v0.5.0`.
- `FRIGG_INSTALL_DIR`, defaulting to `$HOME/.local/bin`.
- `FRIGG_REPOSITORY`, defaulting to `bnomei/frigg`.
- `FRIGG_INSTALL_DRY_RUN`.
- test-only target overrides: `FRIGG_INSTALL_OS`, `FRIGG_INSTALL_ARCH`, `FRIGG_INSTALL_TARGET`.

The script detects the Unix target, resolves latest release version when `FRIGG_VERSION` is unset, constructs the existing versioned asset URL, downloads the archive plus `.sha256`, verifies before extracting, and installs only the `frigg` binary. It uses strict HTTPS curl flags:

```sh
curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error
```

Verification supports `sha256sum --check`, `shasum -a 256`, and `openssl dgst -sha256`. If no verifier exists or verification fails, the script exits without installing.

The script avoids `sudo`, fails clearly when the install directory is not writable, and appends the install directory to `$GITHUB_PATH` when that file exists.

### Cache Fingerprint

Add `frigg hash` as a normal utility subcommand. It is read-only, requires no workspace root, and never writes `.frigg/`.

Output contract:

```text
frigg-hash=<hex>
```

If `$GITHUB_OUTPUT` is set, write the same key/value there. Do not emit paths, debug fields, runner data, restore prefixes, or human explanations.

The hash is derived from Frigg-owned stability inputs only, such as:

- package version from `env!("CARGO_PKG_VERSION")`;
- latest storage schema version from `MIGRATIONS`;
- retrieval projection/index contract versions;
- index-affecting defaults or feature/config knobs that change persisted `.frigg/` compatibility.

Prefer a small library-owned helper for the stable hash material so the binary layer does not hardcode storage or projection internals.

### Documentation

README install docs should lead with the installer and Homebrew before `cargo install`. The install section must separate installation from repository setup. The current setup flow remains:

```bash
frigg init
frigg verify
frigg reindex
frigg serve
```

GitHub Actions docs should lead with no cache. Optional `.frigg/` cache docs must always restore then run:

```bash
frigg init
frigg reindex --changed
frigg verify
```

Binary cache can be documented as optional, exact-key only, and de-emphasized because installing a prebuilt binary is cheap.

### Dogfood Workflow

Add `.github/workflows/frigg-install-cache.yml` to exercise the documented path. It should install through `scripts/install.sh`, use `frigg hash` for Frigg-owned cache material, restore optional `.frigg/`, run `init`/`reindex --changed`/`verify`, and save caches only on trusted events such as `push` to `main` or manual dispatch.

## Decisions

- Use current release asset naming instead of changing release workflow output.
- Keep checksum sidecars as the v1 verification source.
- Keep the installer binary-only and workspace-side-effect-free.
- Use `frigg hash` for one Frigg-owned cache fingerprint; workflow YAML owns OS/arch/SHA/cache path decisions.
- Cache `.frigg/` only as an optional optimization and always refresh/verify after restore.

## Traceability

| Requirement | Task(s) | Validation | Risk/Open Decision |
| --- | --- | --- | --- |
| R001, R002, R008 | T001 | installer dry-run and fixture checksum tests | latest release resolution must stay testable without live GitHub |
| R010 | T002 | `cargo test -p frigg hash` | hash inputs must remain Frigg-owned, not workflow-specific |
| R003, R004, R005, R006, R007 | T003 | README snippet review | docs must not overclaim adopt |
| R005, R006, R007, R009, R010 | T004 | workflow syntax/review and cache guard checks | GitHub cache hit branch is stateful in real CI |

## Verification Plan

- `sh scripts/install.sh --help`
- `FRIGG_INSTALL_DRY_RUN=1 sh scripts/install.sh`
- `FRIGG_VERSION=v0.5.0 FRIGG_INSTALL_DRY_RUN=1 FRIGG_INSTALL_TARGET=x86_64-unknown-linux-gnu sh scripts/install.sh`
- `dash scripts/install.sh --help` when `dash` is available.
- `cargo test -p frigg hash`
- Review README examples for pinned versions, post-cache refresh, and trusted-trigger cache saves.
- Validate `.github/workflows/frigg-install-cache.yml` syntax where tooling is available and inspect trusted-trigger guards.

## Risks

- `curl | sh` is inherently trust-sensitive. Mitigate with inspectable/version-pinnable examples, strict HTTPS flags, mandatory checksum verification, and binary-only behavior.
- Cached executables can become stale or untrusted. Mitigate by de-emphasizing binary cache and requiring exact keys if documented.
- Restored `.frigg/` state can be stale. Mitigate by always running `reindex --changed` and `verify` after restore.
- The cache fingerprint can drift if workers omit an index-affecting version input. Mitigate by centralizing hash material in a library helper and testing deterministic output.
