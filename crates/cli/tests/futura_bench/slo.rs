//! Small-fixture `search_text` latency sample for FUT-023 posture.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use frigg::mcp::types::{ResponseMode, SearchPatternType, SearchTextParams};
use rmcp::handler::server::wrapper::Parameters;

use crate::harness::{
    self, Surface, cleanup_workspace_root, materialize_fixture_workspace, require, server_for_root,
};

pub async fn run_search_text_latency(report: &Mutex<harness::BenchReport>, fixture: &Path) {
    let started = Instant::now();
    let root = materialize_fixture_workspace(fixture, "synth-slo-search-text");
    let outcome = async {
        let server = server_for_root(&root).await;
        const N: usize = 12;
        let mut samples_ms = Vec::with_capacity(N);
        for _ in 0..N {
            let t0 = Instant::now();
            let response = server
                .search_text(Parameters(SearchTextParams {
                    query: "synth_greeting".to_owned(),
                    pattern_type: Some(SearchPatternType::Literal),
                    repository_id: None,
                    path_regex: Some("^src/".to_owned()),
                    limit: Some(20),
                    response_mode: Some(ResponseMode::Compact),
                    ..Default::default()
                }))
                .await
                .map_err(|e| format!("search_text latency sample failed: {e}"))?
                .0;
            let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
            require(
                response.total_matches > 0,
                format!(
                    "expected synth_greeting hits, total_matches={}",
                    response.total_matches
                ),
            )?;
            samples_ms.push(elapsed);
        }
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95 = samples_ms[N - 1]; // max as conservative upper bound with small N
        let p50 = samples_ms[N / 2];
        // Soft posture on tiny fixture: warm/cold Frigg search must stay interactive.
        // Full monorepo p95 vs rg is recorded separately in crates/cli/assets/futura-slo-snapshot.md.
        require(
            p95 < 2000.0,
            format!("search_text p95_ms={p95:.2} exceeded 2000ms soft budget; samples={samples_ms:?}"),
        )?;
        Ok(format!(
            "search_text latency p50_ms={p50:.2} p95_ms={p95:.2} n={N} (meets interactive soft budget)"
        ))
    }
    .await
    .map(|_| ());
    cleanup_workspace_root(&root);
    report.lock().unwrap().record(
        "slo_search_text_latency",
        Surface::Synth,
        started,
        outcome,
    );
}
