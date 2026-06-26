DEVANA-FINDING: v1
Priority: P0 | Confidence: high | Security-sensitive: yes | Status: fixed
Location: crates/cli/src/http_runtime.rs:48 | Slug: serve-default-remote-auth-bypass

# Serve Default Port Bypasses Remote Auth Guard

## Finding

The `serve` command's default-port path returns an HTTP runtime before it parses or enforces the remote-bind authentication guards used by the explicit-port path.

## Violated Invariant Or Contract

Non-loopback HTTP binds must require the explicit remote override and bearer authentication, and a supplied bearer token must be honored.

## Oracle

The explicit-port path parses the auth token and rejects non-loopback hosts without both `--allow-remote-http` and a token. The HTTP middleware only enforces bearer auth when `runtime.auth_token` is present.

## Counterexample

Start `serve` with a non-loopback host and no explicit HTTP port. The early branch builds `0.0.0.0:37444` or equivalent, stores `auth_token: None`, and disables host allowlisting for an unspecified bind.

## Why It Might Matter

This can expose the MCP endpoint on a network interface without bearer authentication, making repository read/search/navigation tools reachable outside the local loopback boundary.

## Proof

Cross-entry guard mismatch: `resolve_http_runtime_config` returns from the no-port `serve_requested` branch at lines 48-58. The auth parsing and non-loopback checks live later in the explicit-port path at lines 74-93, so they never run for default-port `serve`.

## Counterevidence Checked

The non-serve no-port path rejects HTTP flags. The explicit-port remote path enforces the guard. The bypass is specific to `serve` with the default port.

## Suggested Next Step

Route `serve` default-port resolution through the same host/auth validation used for explicit HTTP ports, preserving the default port only after validation.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. Confirmed the early `serve_requested` no-port branch built a bind from `cli.mcp_http_host` (could be non-loopback), hardcoded `auth_token: None`, and returned before the non-loopback override check and auth-token requirement, so `serve --mcp-http-host 0.0.0.0` bound publicly without auth and any `--mcp-http-auth-token` was silently dropped. Refactored `resolve_http_runtime_config` to only choose the port early (explicit `--mcp-http-port`, else 37444 for serve, else reject stray HTTP flags / return None) and then run the shared host/auth validation for every HTTP path. Added regression tests: serve default port rejects non-loopback bind without `--allow-remote-http`, rejects remote bind without an auth token, and honors a supplied auth token on the default loopback port. `cargo test --bin frigg transport`/`serve_command` all pass (15 + 4).

DEVANA-KEY: crates/cli/src/http_runtime.rs:48 | P0 | serve-default-remote-auth-bypass
DEVANA-SUMMARY: Status=fixed | P0 high crates/cli/src/http_runtime.rs:48 - The `serve` default-port branch bypassed remote-bind/bearer validation; default-port resolution now flows through the shared host/auth guard (regression tests added).
