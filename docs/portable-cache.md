# Portable release caches

Frigg can move its local repository index between checkout paths without operating a remote
database. `frigg cache make` creates a consistent `frigg-cache.zip`; `frigg cache load` validates
and installs that archive in another checkout. Follow a load with `frigg index --changed` so Frigg
quickly reconciles an older release snapshot with the checked-out source.

The archive contains a SQLite online-backup snapshot, semantic rows when they exist, and
`.frigg/scip/` artifacts when they exist. Its manifest records the full source commit, exact Frigg
version, storage schema and compatibility fingerprint, source repository partition, semantic
provider/model partitions, payload sizes, and a BLAKE3 digest for every payload. On load, Frigg
rebases the source partition to the destination checkout's stable identity so independently loaded
workspaces do not collide. WAL/SHM files, context logs, temporary files, model caches, and the Frigg
binary are never included.

## Commands

```sh
frigg cache make                 # writes frigg-cache.zip
frigg cache load                 # reads frigg-cache.zip
frigg cache make custom.zip      # optional positional path
frigg cache load custom.zip
```

There are intentionally no packaging flags. SCIP and semantic state are included automatically.
Loading initially requires the exact Frigg version that created the archive. Frigg bounds and
validates every ZIP entry, checks SQLite integrity, relational invariants, and semantic vector
membership, then installs through SQLite's online-backup API. A checksum, compatibility, schema,
integrity, or version mismatch fails before current database and SCIP state are replaced; an
installation failure rolls both back together.

## Publish one cache with each GitHub Release

Build the cache at the validated release commit with the released Frigg binary, then attach the
fixed filename to that release. This minimal job assumes an earlier job made `frigg` available:

```yaml
- name: Build release cache
  env:
    FRIGG_SEMANTIC_RUNTIME_ENABLED: "false"
  run: |
    frigg index
    frigg cache make

- name: Attach release cache
  uses: softprops/action-gh-release@v3
  with:
    files: frigg-cache.zip
```

Frigg's own release workflow is the complete working example:
[`.github/workflows/release.yml`](../.github/workflows/release.yml). It builds a lexical-only cache,
loads it in a different checkout path, runs `index --changed`, and uses one final publishing job to
attach exactly one `frigg-cache.zip` asset per release. Projects that enable semantic indexing can
use their existing `FRIGG_SEMANTIC_RUNTIME_*` environment variables; remote providers require their
API secret in the trusted release job, while the local provider does not.

## Seed a fresh environment or Amp orb

```sh
curl -fsSL \
  https://github.com/OWNER/REPOSITORY/releases/latest/download/frigg-cache.zip \
  -o frigg-cache.zip
frigg cache load
rm -f frigg-cache.zip
frigg index --changed
```

Put those commands in `.agents/setup` for an Amp project and install the matching latest Frigg
release before loading. Treat a missing or incompatible cache as an optional seed and continue with
`frigg index --changed`. Amp captures the refreshed `.frigg/`
directory in its project snapshot, so later orbs begin from the release index and only reconcile
changes since that release. If a project has no cache asset yet, skip the load and run
`frigg index --changed`; the index remains disposable, local build output.
