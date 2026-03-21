set shell := ["bash", "-euo", "pipefail", "-c"]

help:
  @just --list

fmt:
  CARGO_HOME=.codex-cache/cargo-home cargo fmt --all

test:
  CARGO_HOME=.codex-cache/cargo-home cargo test -p frigg

bench target="":
  if [[ -n "{{target}}" ]]; then \
    CARGO_HOME=.codex-cache/cargo-home cargo bench -p frigg --bench {{target}}; \
  else \
    CARGO_HOME=.codex-cache/cargo-home cargo bench -p frigg; \
  fi

build:
  CARGO_HOME=.codex-cache/cargo-home cargo build -p frigg

build-release:
  CARGO_HOME=.codex-cache/cargo-home cargo build --release -p frigg

serve port="37444" host="127.0.0.1" token="":
  if [[ -n "{{token}}" ]]; then \
    CARGO_HOME=.codex-cache/cargo-home cargo run -p frigg -- serve --mcp-http-port {{port}} --mcp-http-host {{host}} --mcp-http-auth-token '{{token}}'; \
  else \
    CARGO_HOME=.codex-cache/cargo-home cargo run -p frigg -- serve --mcp-http-port {{port}} --mcp-http-host {{host}}; \
  fi

init root=".":
  CARGO_HOME=.codex-cache/cargo-home cargo run -p frigg -- init --workspace-root {{root}}

verify root=".":
  CARGO_HOME=.codex-cache/cargo-home cargo run -p frigg -- verify --workspace-root {{root}}

reindex root=".":
  CARGO_HOME=.codex-cache/cargo-home cargo run -p frigg -- reindex --workspace-root {{root}}

reindex-changed root=".":
  CARGO_HOME=.codex-cache/cargo-home cargo run -p frigg -- reindex --changed --workspace-root {{root}}
