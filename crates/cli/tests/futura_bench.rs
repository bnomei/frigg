#![allow(clippy::panic)]

//! # frigg-futura-bench (FUT-019)
//!
//! Evaluation harness that exercises **shipped** Frigg MCP server code
//! (`FriggMcpServer` tool handlers) — not a reimplementation.
//!
//! Surfaces:
//! - **dogfood** — this Frigg repository (override with `FUTURA_BENCH_DOGFOOD_ROOT`)
//! - **synth** — `tests/fixtures/futura_synth/`
//! - **lang** — `tests/fixtures/futura_lang/{php,ts,python}/`
//!
//! Run:
//! ```text
//! cargo test -p frigg --test futura_bench -- --nocapture
//! ```
//!
//! Machine-readable summary:
//! - JSON lines prefixed with `FUTURA_BENCH` on stdout
//! - Final summary object after `FUTURA_BENCH_SUMMARY`
//! - Optional write path via env `FUTURA_BENCH_OUT`

#[path = "futura_bench/dogfood.rs"]
mod dogfood;
#[path = "futura_bench/harness.rs"]
mod harness;
#[path = "futura_bench/lang.rs"]
mod lang;
#[path = "futura_bench/synth.rs"]
mod synth;

use std::sync::Mutex;

/// Primary CI entrypoint for `frigg-futura-bench`.
#[tokio::test(flavor = "multi_thread")]
async fn frigg_futura_bench() {
    let report = Mutex::new(harness::BenchReport::default());

    println!("FUTURA_BENCH harness=frigg-futura-bench starting");
    dogfood::run_all(&report).await;
    synth::run_all(&report).await;
    lang::run_all(&report).await;

    report
        .into_inner()
        .expect("bench report mutex")
        .emit_and_assert();
}
