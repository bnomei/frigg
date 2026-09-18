# Frigg Packaging

Frigg's release assets are the source of truth for binary installers:

- `cargo binstall frigg` uses `[package.metadata.binstall]` in `crates/cli/Cargo.toml`.
- `npx @bnomei/frigg` is published from `npm/frigg`; the wrapper downloads the matching GitHub Release asset and verifies its `.sha256`.
- `docker run ghcr.io/bnomei/frigg:<version>` is built from `Dockerfile`, which downloads the GNU/glibc Linux release asset for the target image architecture so the default local semantic provider remains available.
- Scoop bucket metadata lives in the sibling `scoop-frigg` repository. Its manifest consumes the Windows `.zip` release asset and published `.sha256` checksum.

The npm publish job is skipped unless the release environment provides `NPM_TOKEN`.

GNU Linux artifacts are cross-built against an old glibc baseline and bundle the official ONNX
Runtime 1.24 shared library (glibc 2.27 baseline) for local embeddings. Release smoke coverage
runs them on Debian Bookworm. The Docker image uses these same assets; its default runtime is
`gcr.io/distroless/cc-debian13:nonroot` and contains no shell. Bind-mounted workspaces must be
writable by uid 65532, or callers must pass a matching `--user <uid>:<gid>`.
Source-installed GNU Linux binaries can point to a compatible ONNX Runtime shared library with
`ORT_DYLIB_PATH`; packaged binaries prefer the bundled library beside the Frigg executable.
Packaged GNU and musl MCP smoke tests also build a semantic fixture through a loopback-only,
dummy-key OpenAI-compatible stub and require a healthy full `search_hybrid` response. The harness
removes ambient OpenAI and Gemini credentials before starting Frigg.

Default Docker image assets:

```bash
VERSION=0.10.2 TARGET=x86_64-unknown-linux-gnu scripts/build-release.sh
VERSION=0.10.2 TARGET=x86_64-unknown-linux-gnu scripts/package-release.sh
VERSION=0.10.2 TARGET=x86_64-unknown-linux-gnu scripts/smoke-release.sh

VERSION=0.10.2 TARGET=aarch64-unknown-linux-gnu scripts/build-release.sh
VERSION=0.10.2 TARGET=aarch64-unknown-linux-gnu scripts/package-release.sh
VERSION=0.10.2 TARGET=aarch64-unknown-linux-gnu scripts/smoke-release.sh
```

Static musl assets omit the default local ONNX/FastEmbed provider because ONNX Runtime does not
publish musl binaries. The npm wrapper detects Node's runtime libc and selects these assets on musl
instead of hard-coding GNU Linux:

```bash
VERSION=0.10.2 TARGET=x86_64-unknown-linux-musl scripts/build-release.sh
VERSION=0.10.2 TARGET=x86_64-unknown-linux-musl scripts/package-release.sh
VERSION=0.10.2 TARGET=x86_64-unknown-linux-musl scripts/smoke-release.sh

VERSION=0.10.2 TARGET=aarch64-unknown-linux-musl scripts/build-release.sh
VERSION=0.10.2 TARGET=aarch64-unknown-linux-musl scripts/package-release.sh
VERSION=0.10.2 TARGET=aarch64-unknown-linux-musl scripts/smoke-release.sh
```

Use `cross` for these targets unless the matching musl C toolchain is installed on the host.

The Intel macOS release asset also omits the default local ONNX/FastEmbed provider because
`ort-sys` does not provide prebuilt ONNX Runtime binaries for `x86_64-apple-darwin`:

```bash
VERSION=0.10.2 TARGET=x86_64-apple-darwin scripts/build-release.sh
VERSION=0.10.2 TARGET=x86_64-apple-darwin scripts/package-release.sh
VERSION=0.10.2 TARGET=x86_64-apple-darwin scripts/smoke-release.sh
```
