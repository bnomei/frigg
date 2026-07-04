# Frigg Packaging

Frigg's release assets are the source of truth for binary installers:

- `cargo binstall frigg` uses `[package.metadata.binstall]` in `crates/cli/Cargo.toml`.
- `npx @bnomei/frigg` is published from `npm/frigg`; the wrapper downloads the matching GitHub Release asset and verifies its `.sha256`.
- `docker run ghcr.io/bnomei/frigg:<version>` is built from `Dockerfile`, which downloads the GNU/glibc Linux release asset for the target image architecture so the default local semantic provider remains available.
- Scoop bucket metadata lives in the sibling `scoop-frigg` repository. Its manifest consumes the Windows `.zip` release asset and published `.sha256` checksum.

The npm publish job is skipped unless the release environment provides `NPM_TOKEN`.

The Docker image keeps using GNU/glibc Linux assets so the default local semantic
provider remains available. Its runtime base must provide glibc/libstdc++
compatible with the pinned Linux release runner; the default is
`gcr.io/distroless/cc-debian13:nonroot`.

Default Docker image assets:

```bash
VERSION=0.6.0 TARGET=x86_64-unknown-linux-gnu scripts/build-release.sh
VERSION=0.6.0 TARGET=x86_64-unknown-linux-gnu scripts/package-release.sh
VERSION=0.6.0 TARGET=x86_64-unknown-linux-gnu scripts/smoke-release.sh

VERSION=0.6.0 TARGET=aarch64-unknown-linux-gnu scripts/build-release.sh
VERSION=0.6.0 TARGET=aarch64-unknown-linux-gnu scripts/package-release.sh
VERSION=0.6.0 TARGET=aarch64-unknown-linux-gnu scripts/smoke-release.sh
```

Optional static musl assets omit the default local ONNX/FastEmbed provider because `ort-sys` does not provide prebuilt ONNX Runtime binaries for musl:

```bash
VERSION=0.6.0 TARGET=x86_64-unknown-linux-musl scripts/build-release.sh
VERSION=0.6.0 TARGET=x86_64-unknown-linux-musl scripts/package-release.sh
VERSION=0.6.0 TARGET=x86_64-unknown-linux-musl scripts/smoke-release.sh

VERSION=0.6.0 TARGET=aarch64-unknown-linux-musl scripts/build-release.sh
VERSION=0.6.0 TARGET=aarch64-unknown-linux-musl scripts/package-release.sh
VERSION=0.6.0 TARGET=aarch64-unknown-linux-musl scripts/smoke-release.sh
```

Use `cross` for these targets unless the matching musl C toolchain is installed on the host.

The Intel macOS release asset also omits the default local ONNX/FastEmbed provider because
`ort-sys` does not provide prebuilt ONNX Runtime binaries for `x86_64-apple-darwin`:

```bash
VERSION=0.6.0 TARGET=x86_64-apple-darwin scripts/build-release.sh
VERSION=0.6.0 TARGET=x86_64-apple-darwin scripts/package-release.sh
VERSION=0.6.0 TARGET=x86_64-apple-darwin scripts/smoke-release.sh
```
