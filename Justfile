set shell := ["bash", "-euo", "pipefail", "-c"]

help:
  @just --list

fmt:
  cargo fmt --all

clippy:
  cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
  cargo test --workspace

quality: fmt clippy test

build:
  cargo build -p frigg

build-release:
  cargo build --release -p frigg

run +args:
  cargo run -p frigg -- {{args}}

run-http port="37444" host="127.0.0.1" token="":
  if [[ -n "{{token}}" ]]; then \
    cargo run -p frigg -- --mcp-http-port {{port}} --mcp-http-host {{host}} --mcp-http-auth-token '{{token}}'; \
  else \
    cargo run -p frigg -- --mcp-http-port {{port}} --mcp-http-host {{host}}; \
  fi

init root=".":
  cargo run -p frigg -- init --workspace-root {{root}}

verify root=".":
  cargo run -p frigg -- verify --workspace-root {{root}}

reindex root=".":
  cargo run -p frigg -- reindex --workspace-root {{root}}

reindex-changed root=".":
  cargo run -p frigg -- reindex --changed --workspace-root {{root}}

ops-cli-roundtrip root:
  cargo run -p frigg -- init --workspace-root {{root}}
  cargo run -p frigg -- reindex --workspace-root {{root}}
  cargo run -p frigg -- reindex --changed --workspace-root {{root}}
  cargo run -p frigg -- verify --workspace-root {{root}}

test-security:
  cargo test -p frigg --test security
  cargo test -p frigg security

test-mcp-tool-handlers:
  cargo test -p frigg --test tool_handlers

test-mcp-provenance:
  cargo test -p frigg --test provenance

smoke-ops:
  bash scripts/smoke-ops.sh

docs-sync:
  bash scripts/check-doc-sync.sh

release-ready:
  bash scripts/check-release-readiness.sh

bench-mcp:
  cargo bench -p frigg --bench tool_latency -- --noplot

bench-search:
  cargo bench -p frigg --bench search_latency -- --noplot

bench-index:
  cargo bench -p frigg --bench reindex_latency -- --noplot

bench-core-latency:
  just bench-mcp
  just bench-search
  just bench-index

bench-report output="benchmarks/latest-report.md":
  python3 benchmarks/generate_latency_report.py --output {{output}}

bench-report-gate output="benchmarks/latest-report.md":
  python3 benchmarks/generate_latency_report.py --fail-on-budget --output {{output}}
