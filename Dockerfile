# syntax=docker/dockerfile:1

ARG FRIGG_VERSION=0.6.3
ARG FRIGG_REPOSITORY=bnomei/frigg
ARG FRIGG_RUNTIME_IMAGE=gcr.io/distroless/cc-debian13:nonroot

FROM --platform=$BUILDPLATFORM alpine:3.22 AS fetch
ARG FRIGG_VERSION
ARG FRIGG_REPOSITORY
ARG TARGETARCH

RUN apk add --no-cache ca-certificates curl

RUN set -eux; \
  case "$TARGETARCH" in \
    amd64) target=x86_64-unknown-linux-gnu ;; \
    arm64) target=aarch64-unknown-linux-gnu ;; \
    *) echo "unsupported Docker target architecture: $TARGETARCH" >&2; exit 1 ;; \
  esac; \
  tag="v${FRIGG_VERSION#v}"; \
  archive="frigg-${tag}-${target}.tar.gz"; \
  url="https://github.com/${FRIGG_REPOSITORY}/releases/download/${tag}/${archive}"; \
  curl -fsSL -o "/tmp/${archive}" "$url"; \
  curl -fsSL -o "/tmp/${archive}.sha256" "${url}.sha256"; \
  cd /tmp; \
  sha256sum -c "${archive}.sha256"; \
  tar -xzf "$archive"; \
  chmod 755 frigg; \
  mkdir -p /tmp/frigg-workspace/.cache

FROM ${FRIGG_RUNTIME_IMAGE}
ARG FRIGG_VERSION

ENV HOME=/workspace \
    XDG_CACHE_HOME=/workspace/.cache

LABEL org.opencontainers.image.title="Frigg"
LABEL org.opencontainers.image.description="Frigg gives AI agents local, source-backed code search and navigation without sending whole repositories through every prompt."
LABEL org.opencontainers.image.source="https://github.com/bnomei/frigg"
LABEL org.opencontainers.image.licenses="MIT AND MPL-2.0"
LABEL org.opencontainers.image.version="${FRIGG_VERSION}"

COPY --from=fetch --chown=65532:65532 /tmp/frigg /usr/local/bin/frigg
COPY --from=fetch --chown=65532:65532 /tmp/frigg-workspace /workspace

WORKDIR /workspace
USER 65532:65532

ENTRYPOINT ["/usr/local/bin/frigg"]
