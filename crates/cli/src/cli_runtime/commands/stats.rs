//! CLI `stats` command: live opt-in routing stats surface.
//!
//! Reads routing counters from the running loopback/HTTP MCP server process, where tool-call
//! telemetry is recorded. Never sends telemetry off-machine.

use std::error::Error;
use std::io;
use std::time::Duration;

use frigg::mcp::routing_stats::{
    ROUTING_STATS_ENV, ROUTING_STATS_RESOURCE_URI, RoutingStatsSnapshot,
};
use reqwest::header;

/// Fetches and prints routing stats from the running MCP HTTP server.
pub(crate) async fn run_stats_command(
    json: bool,
    endpoint_url: &str,
    auth_token: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let body = fetch_live_routing_stats_json(endpoint_url, auth_token).await?;
    if json {
        println!("{body}");
        return Ok(());
    }

    let snap: RoutingStatsSnapshot = serde_json::from_str(&body).map_err(|err| {
        io::Error::other(format!(
            "live routing stats response from {endpoint_url} was not a valid stats snapshot: {err}"
        ))
    })?;
    if !snap.enabled {
        println!(
            "routing stats disabled on live server (set {ROUTING_STATS_ENV}=1 before `frigg serve`)"
        );
        println!("endpoint: {endpoint_url}");
        println!("resource: {ROUTING_STATS_RESOURCE_URI}");
        println!("privacy: local process only; no cloud telemetry");
        return Ok(());
    }

    println!(
        "routing stats enabled on live server — tools={} zero_hits={} recovery={} handle_failures={} workspace_gates={}",
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
        println!("tool_calls: (none recorded yet)");
    }
    println!("endpoint: {endpoint_url}");
    println!("resource: {ROUTING_STATS_RESOURCE_URI}");
    println!("privacy: local process only; no cloud telemetry");
    Ok(())
}

async fn fetch_live_routing_stats_json(
    endpoint_url: &str,
    auth_token: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut request = client
        .get(endpoint_url)
        .header(header::ACCEPT, "application/json");
    if let Some(token) = auth_token {
        request = request.bearer_auth(token);
    }

    let response = request.send().await.map_err(|err| {
        io::Error::other(format!(
            "failed to read live routing stats from {endpoint_url}: {err}"
        ))
    })?;
    let status = response.status();
    let body = response.text().await.map_err(|err| {
        io::Error::other(format!(
            "failed to read live routing stats body from {endpoint_url}: {err}"
        ))
    })?;
    if !status.is_success() {
        return Err(Box::new(io::Error::other(format!(
            "live routing stats request to {endpoint_url} failed with HTTP {status}: {body}"
        ))));
    }
    Ok(body)
}
