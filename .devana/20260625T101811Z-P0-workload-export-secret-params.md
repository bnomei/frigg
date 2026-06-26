DEVANA-FINDING: v1
Priority: P0 | Confidence: medium | Security-sensitive: yes | Status: fixed
Location: crates/cli/src/cli_runtime/commands/workload_corpus.rs:44 | Slug: workload-export-secret-params

# Workload Export Preserves Secret-Bearing Params

## Finding

The workload corpus export sanitization bounds JSON shape and string length, but it does not redact secret-bearing parameter or source-reference text before writing JSON or JSONL output.

## Violated Invariant Or Contract

A "sanitized workload corpus" export should not preserve raw secret-like user text from MCP params or source refs.

## Oracle

The command help describes the export as sanitized, while provider diagnostics elsewhere include dedicated redaction behavior for API keys and raw source text.

## Counterexample

If a user searches for a token-like string, MCP provenance stores the bounded raw query under `params`. `export-workload-corpus` then copies that value into `parameter_summary` unchanged except for a 256-character cap.

## Why It Might Matter

The command can create durable corpus files containing secrets that users reasonably expect the sanitized export to remove before sharing or archiving.

## Proof

Dataflow trace: `provenance_payload` stores `params` and `source_refs` verbatim at `crates/cli/src/mcp/server/provenance.rs:31`. `sanitize_workload_corpus_value` only truncates depth, array length, object count, and strings at `crates/cli/src/cli_runtime/commands/workload_corpus.rs:44`. The export writes `parameter_summary`, `source_refs_summary`, and `normalized_workload` from those sanitized values at lines 142-153.

## Counterevidence Checked

Strings are bounded, malformed payloads are bounded, and provider HTTP diagnostics have separate redaction paths. None of those redaction paths are used by workload corpus export.

## Suggested Next Step

Add key-name and token-pattern redaction to workload corpus sanitization, and apply it to params, source refs, normalized workload, and decode-error fallbacks.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...` and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. Confirmed `sanitize_workload_corpus_value` only bounded shape/length with no redaction, and the decode-error fallback wrote raw payload JSON. The existing redactor (`embeddings/transport.rs::redact_key_bearing_url_parameters`) is URL-param specific and private, so added JSON-aware redaction in the corpus sanitizer: (1) object entries whose key contains a secret fragment (api_key, token, authorization, password, secret, credential, private_key, …) are redacted whole to `[REDACTED]`; (2) free-text string values are scanned for provider token prefixes (sk-, ghp_, xoxb-, AKIA, AIza, ya29., eyJ JWT, …), high-entropy 32+ char base64/hex-like tokens, inline `key=value`/`key:value` secret assignments, and `Bearer <token>`/introducer chains — non-secret words are preserved. Sanitizer now applies to params, outcome, source refs, normalized workload, and the decode-error fallback (`sanitized_workload_corpus_text` = redact then bound). Added unit tests for object-key, free-text-token, inline-assignment/bearer, and no-false-positive cases. All pass.

DEVANA-KEY: crates/cli/src/cli_runtime/commands/workload_corpus.rs:44 | P0 | workload-export-secret-params
DEVANA-SUMMARY: Status=fixed | P0 medium crates/cli/src/cli_runtime/commands/workload_corpus.rs:44 - Workload corpus export now redacts secret-bearing object keys and secret-like token text across params/source-refs/normalized-workload/decode-error before writing (tests added).
