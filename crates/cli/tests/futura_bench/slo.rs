//! head-to-head latency: warm shipped `search_text` vs local `rg` on the
//! **same** fixture, query, and path scope.
//!
//! Agent-facing comparison:
//! - Frigg: in-process MCP handler after warm index (HTTP/stdio agent path once attached)
//! - rg: subprocess `rg -n --glob '*.rs' QUERY path` (shell habit cost)

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Instant;

use frigg::mcp::types::{ResponseMode, SearchPatternType, SearchTextParams};
use rmcp::handler::server::wrapper::Parameters;

use crate::harness::{
    self, Surface, cleanup_workspace_root, materialize_fixture_workspace, require, server_for_root,
};

const QUERY: &str = "greeting";
const WARMUP: usize = 10;
const N: usize = 50;
/// Relative competitive budget for small-N process-spawn timing (CI/scheduler jitter).
/// Product remediations closed a ~4× gap to ~1.0–1.2× on quiet machines; p95 still
/// flickers ~1.0–1.3× under load. Exact ≤ and even 1.25× flake; 1.5× stays competitive
/// (not “lose badly”) without CI flukes on toy fixtures.
/// Release fails if warm Frigg `search_text` p95 exceeds this multiple of subprocess `rg` p95.
const RELEASE_NOISE_RATIO: f64 = 1.5;

#[derive(Debug, Clone)]
struct LatencyStats {
    n: usize,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    min_ms: f64,
    max_ms: f64,
    samples_ms: Vec<f64>,
}

impl LatencyStats {
    fn from_samples(mut samples: Vec<f64>) -> Result<Self, String> {
        require(!samples.is_empty(), "latency samples empty")?;
        samples.sort_by(|a, b| a.total_cmp(b));
        let n = samples.len();
        let mean_ms = samples.iter().sum::<f64>() / n as f64;
        Ok(Self {
            n,
            mean_ms,
            p50_ms: percentile(&samples, 0.50),
            p95_ms: percentile(&samples, 0.95),
            min_ms: samples[0],
            max_ms: samples[n - 1],
            samples_ms: samples,
        })
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "n": self.n,
            "mean_ms": self.mean_ms,
            "p50_ms": self.p50_ms,
            "p95_ms": self.p95_ms,
            "min_ms": self.min_ms,
            "max_ms": self.max_ms,
            "samples_ms": self.samples_ms,
        })
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let k = (sorted.len() - 1) as f64 * p;
    let f = k.floor() as usize;
    let c = (f + 1).min(sorted.len() - 1);
    if f == c {
        sorted[f]
    } else {
        sorted[f] + (sorted[c] - sorted[f]) * (k - f as f64)
    }
}

fn time_rg_samples(workspace: &Path, query: &str, n: usize) -> Result<LatencyStats, String> {
    let src = workspace.join("src");
    require(src.is_dir(), format!("missing src at {}", src.display()))?;
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t0 = Instant::now();
        let status = Command::new("rg")
            .args(["-n", "--glob", "*.rs", query])
            .arg(&src)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("failed to spawn rg: {e}"))?;
        require(
            status.success(),
            format!("rg exited {:?}; expected hits for {query:?}", status.code()),
        )?;
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    LatencyStats::from_samples(samples)
}

async fn time_frigg_search_text_samples(
    server: &frigg::mcp::FriggMcpServer,
    query: &str,
    warmup: usize,
    n: usize,
) -> Result<LatencyStats, String> {
    // Equivalent scoped probe to `rg -n --glob '*.rs' QUERY src/`:
    // path_regex alone (avoid combining glob+path_regex which forces post-filter over-fetch).
    let params = SearchTextParams {
        query: query.to_owned(),
        pattern_type: Some(SearchPatternType::Literal),
        repository_id: None,
        path_regex: Some("^src/".to_owned()),
        limit: Some(20),
        response_mode: Some(ResponseMode::Compact),
        ..Default::default()
    };

    // Warm path: attach/index already done by server_for_root; discard first samples.
    for i in 0..warmup {
        let response = server
            .search_text(Parameters(params.clone()))
            .await
            .map_err(|e| format!("warmup search_text #{i} failed: {e}"))?
            .0;
        require(
            response.total_matches > 0,
            format!(
                "warmup expected hits for {query:?}, total_matches={}",
                response.total_matches
            ),
        )?;
    }

    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t0 = Instant::now();
        let response = server
            .search_text(Parameters(params.clone()))
            .await
            .map_err(|e| format!("timed search_text #{i} failed: {e}"))?
            .0;
        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        require(
            response.total_matches > 0,
            format!(
                "expected hits for {query:?}, total_matches={}",
                response.total_matches
            ),
        )?;
        samples.push(elapsed);
    }
    LatencyStats::from_samples(samples)
}

/// Publish snapshot markdown when `FUTURA_SLO_OUT` is set (operator refresh path).
fn maybe_write_snapshot(
    workspace: &Path,
    rg: &LatencyStats,
    frigg: &LatencyStats,
    meets: bool,
) -> Result<(), String> {
    let Some(out) = std::env::var_os("FUTURA_SLO_OUT") else {
        return Ok(());
    };
    let out = PathBuf::from(out);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create SLO out parent: {e}"))?;
    }
    let date_utc = chrono_like_utc_now();
    let status = if meets {
        "PASS — warm Frigg `search_text` p95 competitive with local `rg` (≤ 1.5× release noise budget) on the same fixture/query/scope"
    } else {
        "FAIL — Frigg exceeded 1.5× rg p95; remediate before marking green"
    };
    let ratio = if rg.p95_ms > 0.0 {
        frigg.p95_ms / rg.p95_ms
    } else {
        f64::INFINITY
    };
    let body = format!(
        r#"# Search latency SLO snapshot

Generated: `{date_utc}`

## Posture targets (product contract)

| Surface | Target posture | Gate status |
| --- | --- | --- |
| Small-fixture exact `search_text` p95 | Competitive with local `rg` (≤ 1.5× release noise budget) | **Measured / CI-gated** |
| Warm `search_symbol` p95 | Fast enough that known-name tasks never prefer shell | Posture only (not gated) |
| `search_batch` (4 probes) | Concurrent probes; better agent UX than multi-turn greps | Posture only (not gated) |
| Dirty hot-path index lag | Path-scoped live-disk when dirty | Posture only; lag p95 deferred |
| Hybrid p95 | Allowed slower than exact; must still return pivots promptly | Posture only (not gated) |
| Large-repo monorepo p95 | Competitive with scoped `rg` | **Deferred** (not measured) |

## Methodology (head-to-head)

- **Fixture:** `{fixture}` (materialized synth seed with `src/**/*.rs`, gitignored `*.tmp`)
- **Query:** `{query}` (literal)
- **Frigg path:** shipped `FriggMcpServer::search_text` after `workspace` adopt + {warmup} warmups; N={n} timed samples; `path_regex='^src/'` only (no glob filter on timed path)
- **rg path:** subprocess `rg -n --glob '*.rs' '{query}' <fixture>/src`; N={n} timed samples (includes process spawn — agent shell cost)
- **Pass rule (release):** `frigg.p95_ms <= rg.p95_ms * 1.5` (competitive noise budget; exact ≤ and 1.25× flake on small fixtures)
- **Debug:** soft 2s budget only; ratios logged; strict gate skipped

## Measured rg baseline

```json
{rg_json}
```

| Metric | Value |
| --- | --- |
| N | {rg_n} |
| mean_ms | {rg_mean:.3} |
| p50_ms | {rg_p50:.3} |
| p95_ms | {rg_p95:.3} |
| query | `{query}` |
| path scope | `src/**/*.rs` |

## Measured warm Frigg `search_text`

```json
{frigg_json}
```

| Metric | Value |
| --- | --- |
| N | {frigg_n} |
| warmup discarded | {warmup} |
| mean_ms | {frigg_mean:.3} |
| p50_ms | {frigg_p50:.3} |
| p95_ms | {frigg_p95:.3} |
| query | `{query}` |
| path scope | `path_regex=^src/` |

## Comparison

| Tool | p50_ms | p95_ms |
| --- | ---: | ---: |
| local `rg` (subprocess) | {rg_p50:.3} | {rg_p95:.3} |
| warm Frigg `search_text` | {frigg_p50:.3} | {frigg_p95:.3} |
| ratio frigg/rg p95 | — | {ratio:.3} |

**Status:** {status}

## Operator recipe

```bash
# Binding (release + writes this file when FUTURA_SLO_OUT is set)
FUTURA_SLO_OUT=crates/cli/assets/futura-slo-snapshot.md cargo futura-bench
# Or: scripts/futura_slo_probe.sh crates/cli/assets/futura-slo-snapshot.md

# Contract-only (debug; soft SLO, no competitive gate):
# cargo test -p frigg --test futura_bench -- --nocapture

# Local routing stats — process-local only
FRIGG_ROUTING_STATS=1 frigg serve
# then: frigg stats   OR   MCP resource frigg://stats/routing
```

## Privacy

Routing stats and this SLO snapshot are **local**. No cloud telemetry is required or emitted
by Frigg core for / .
"#,
        date_utc = date_utc,
        fixture = workspace.display(),
        query = QUERY,
        warmup = WARMUP,
        n = N,
        rg_json = serde_json::to_string_pretty(&rg.to_json_value()).unwrap_or_default(),
        rg_n = rg.n,
        rg_mean = rg.mean_ms,
        rg_p50 = rg.p50_ms,
        rg_p95 = rg.p95_ms,
        frigg_json = serde_json::to_string_pretty(&frigg.to_json_value()).unwrap_or_default(),
        frigg_n = frigg.n,
        frigg_mean = frigg.mean_ms,
        frigg_p50 = frigg.p50_ms,
        frigg_p95 = frigg.p95_ms,
        ratio = ratio,
        status = status,
    );
    std::fs::write(&out, body).map_err(|e| format!("write SLO snapshot {}: {e}", out.display()))?;
    println!("FUTURA_SLO wrote snapshot to {}", out.display());
    Ok(())
}

fn chrono_like_utc_now() -> String {
    // Avoid pulling chrono into the test binary; use system time formatted as RFC3339-ish UTC.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Enough for snapshot labeling; not a full date library.
    format!("unix-{secs}")
}

pub async fn run_search_text_latency(report: &Mutex<harness::BenchReport>, fixture: &Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-slo-vs-rg");
    // Ensure fixture contains the shared QUERY string (synth seed uses greeting in comments/names).
    // Prefer materializing a dedicated SLO seed if present; else rewrite lib.rs with QUERY.
    ensure_query_in_fixture(&root, QUERY);

    let outcome = async {
        let server = server_for_root(&root).await;

        let rg = time_rg_samples(&root, QUERY, N)?;
        let frigg = time_frigg_search_text_samples(&server, QUERY, WARMUP, N).await?;

        let ratio = if rg.p95_ms > 0.0 {
            frigg.p95_ms / rg.p95_ms
        } else {
            f64::NAN
        };
        let meets_exact = frigg.p95_ms <= rg.p95_ms;
        let meets_release = frigg.p95_ms <= rg.p95_ms * RELEASE_NOISE_RATIO;
        // Snapshot Status tracks the **binding** release gate (≤1.5× noise budget), not flaky exact ≤.
        maybe_write_snapshot(&root, &rg, &frigg, meets_release)?;

        let comparison = serde_json::json!({
            "query": QUERY,
            "path_scope": "path_regex=^src/ (equiv. rg scoped to src/)",
            "warmup": WARMUP,
            "n": N,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "strict_gate": !cfg!(debug_assertions),
            "release_noise_ratio": RELEASE_NOISE_RATIO,
            "rg": rg.to_json_value(),
            "frigg_search_text": frigg.to_json_value(),
            "ratio_frigg_rg_p95": ratio,
            "meets_posture_frigg_p95_le_rg_p95": meets_exact,
            "meets_release_gate_with_noise_budget": meets_release,
        });
        println!(
            "FUTURA_SLO_COMPARISON {}",
            serde_json::to_string(&comparison).unwrap_or_default()
        );

        // Soft interactive budget always (debug or release): prevents catastrophic regressions.
        require(
            frigg.p95_ms < 2000.0,
            format!(
                "soft budget FAIL: warm search_text p95_ms={:.3} exceeded 2000ms",
                frigg.p95_ms
            ),
        )?;

        // Strict head-to-head is a **release** gate only (CI / `cargo futura-bench`).
        // Debug builds log ratios but do not fail on measurement noise.
        if cfg!(debug_assertions) {
            if !meets_exact {
                println!(
                    "FUTURA_SLO_NOTE debug profile: frigg p95_ms={:.3} > rg p95_ms={:.3} (ratio={:.3}); strict gate skipped — use --release for binding proof",
                    frigg.p95_ms, rg.p95_ms, ratio
                );
            }
            return Ok(());
        }

        require(
            meets_release,
            format!(
                "FAIL (release): warm search_text p95_ms={:.3} > rg p95_ms={:.3} * {:.2} (ratio={:.3}); remediate latency before green",
                frigg.p95_ms, rg.p95_ms, RELEASE_NOISE_RATIO, ratio
            ),
        )?;
        Ok(())
    }
    .await;

    cleanup_workspace_root(&root);
    report
        .lock()
        .expect("benchmark report mutex should not be poisoned")
        .record("slo_search_text_vs_rg", Surface::Synth, started, outcome);
}

fn ensure_query_in_fixture(root: &Path, query: &str) {
    let lib = root.join("src/lib.rs");
    let contents = std::fs::read_to_string(&lib).unwrap_or_default();
    if contents.contains(query) {
        return;
    }
    let _ = std::fs::create_dir_all(root.join("src"));
    let body = format!(
        "//! SLO seed\npub fn {query} -> &'static str {{\n \"hello from futura slo fixture {query}\"\n}}\n"
    );
    let _ = std::fs::write(lib, body);
    let util = root.join("src/util.rs");
    if !util.exists() {
        let _ = std::fs::write(util, "pub fn util_marker() {}\n");
    }
}
