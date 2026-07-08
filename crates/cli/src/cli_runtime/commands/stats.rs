//! CLI `stats` command: local opt-in routing stats surface (`FUT-024`).
//!
//! Prints process-local routing counters when `FRIGG_ROUTING_STATS` is enabled for this
//! process, or documents how to enable/read stats from a running MCP server. Never sends
//! telemetry off-machine.

use std::error::Error;

use frigg::mcp::routing_stats::{
    ROUTING_STATS_ENV, ROUTING_STATS_RESOURCE_URI, routing_stats_enabled, snapshot, snapshot_json,
};

/// Prints local routing-stats guidance and any in-process counters.
pub(crate) fn run_stats_command(json: bool) -> Result<(), Box<dyn Error>> {
    if json {
        println!("{}", snapshot_json());
        return Ok(());
    }

    let snap = snapshot();
    if !routing_stats_enabled() {
        println!(
            "routing stats disabled (set {ROUTING_STATS_ENV}=1 on `frigg serve` / stdio MCP process)"
        );
        println!("read live snapshot via MCP resource `{ROUTING_STATS_RESOURCE_URI}`");
        println!("privacy: local process only; no cloud telemetry");
        return Ok(());
    }

    println!(
        "routing stats enabled — tools={} zero_hits={} recovery={} handle_failures={} workspace_gates={}",
        snap.total_tool_calls(),
        snap.zero_hit_count,
        snap.recovery_issued,
        snap.handle_failures,
        snap.workspace_gate_uses,
    );
    if !snap.tool_calls.is_empty() {
        println!("tool_calls:");
        for (name, count) in &snap.tool_calls {
            println!("  {name}: {count}");
        }
    } else {
        println!("tool_calls: (none recorded in this process yet)");
    }
    println!("resource: {ROUTING_STATS_RESOURCE_URI}");
    println!("privacy: local process only; no cloud telemetry");
    Ok(())
}
