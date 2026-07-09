//! Opt-in local routing stats for operator health.
//!
//! Session/process-local counters only — no cloud telemetry. Enable with
//! `FRIGG_ROUTING_STATS=1` (or `true` / `yes` / `on`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Environment variable that enables local routing-stats recording.
pub const ROUTING_STATS_ENV: &str = "FRIGG_ROUTING_STATS";

/// MCP resource URI for the live routing-stats snapshot.
pub const ROUTING_STATS_RESOURCE_URI: &str = "frigg://stats/routing";

/// Aggregated local routing counters for the current process.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RoutingStatsSnapshot {
    /// Whether recording is currently enabled for this process.
    pub enabled: bool,
    /// Tool call counts keyed by tool name.
    pub tool_calls: BTreeMap<String, u64>,
    /// Responses that carried a structured zero-hit recovery.
    pub zero_hit_count: u64,
    /// Responses/errors that issued recovery fields (`suggested_next` / zero-hit).
    pub recovery_issued: u64,
    /// Stale or mixed `read_match` handle failures.
    pub handle_failures: u64,
    /// Calls that used the workspace gate surface.
    pub workspace_gate_uses: u64,
}

impl RoutingStatsSnapshot {
    /// Total recorded tool calls across all names.
    pub fn total_tool_calls(&self) -> u64 {
        self.tool_calls.values().copied().sum()
    }
}

#[derive(Debug, Default)]
struct RoutingStatsState {
    tool_calls: BTreeMap<String, u64>,
    zero_hit_count: u64,
    recovery_issued: u64,
    handle_failures: u64,
    workspace_gate_uses: u64,
}

fn state() -> &'static Mutex<RoutingStatsState> {
    static STATE: OnceLock<Mutex<RoutingStatsState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(RoutingStatsState::default()))
}

/// Test-only override so unit tests do not need process-wide env mutation under
/// `unsafe_code = "deny"`.
static TEST_FORCE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Returns true when `FRIGG_ROUTING_STATS` opts into local recording.
pub fn routing_stats_enabled() -> bool {
    if TEST_FORCE_ENABLED.load(Ordering::Relaxed) {
        return true;
    }
    match std::env::var(ROUTING_STATS_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        }
        Err(_) => false,
    }
}

/// Records one completed MCP tool call by name when stats are enabled.
pub fn record_tool_call(tool_name: &str) {
    if !routing_stats_enabled() {
        return;
    }
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard.tool_calls.entry(tool_name.to_owned()).or_insert(0) += 1;
}

/// Records a structured zero-hit recovery emission when stats are enabled.
pub fn record_zero_hit() {
    if !routing_stats_enabled() {
        return;
    }
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.zero_hit_count = guard.zero_hit_count.saturating_add(1);
    guard.recovery_issued = guard.recovery_issued.saturating_add(1);
}

/// Records a recovery payload issuance (non-zero-hit) when stats are enabled.
pub fn record_recovery_issued() {
    if !routing_stats_enabled() {
        return;
    }
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.recovery_issued = guard.recovery_issued.saturating_add(1);
}

/// Records a stale/mixed handle failure when stats are enabled.
pub fn record_handle_failure() {
    if !routing_stats_enabled() {
        return;
    }
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.handle_failures = guard.handle_failures.saturating_add(1);
}

/// Records a workspace gate use when stats are enabled.
pub fn record_workspace_gate_use() {
    if !routing_stats_enabled() {
        return;
    }
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.workspace_gate_uses = guard.workspace_gate_uses.saturating_add(1);
}

/// Returns a point-in-time snapshot of local routing stats.
pub fn snapshot() -> RoutingStatsSnapshot {
    let enabled = routing_stats_enabled();
    let guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    RoutingStatsSnapshot {
        enabled,
        tool_calls: guard.tool_calls.clone(),
        zero_hit_count: guard.zero_hit_count,
        recovery_issued: guard.recovery_issued,
        handle_failures: guard.handle_failures,
        workspace_gate_uses: guard.workspace_gate_uses,
    }
}

/// Pretty JSON body for the `frigg://stats/routing` resource.
pub fn snapshot_json() -> String {
    let mut value = serde_json::to_value(snapshot()).expect("routing stats should serialize");
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "schema_id".to_owned(),
            serde_json::json!("frigg.stats.routing.v1"),
        );
        object.insert(
            "privacy".to_owned(),
            serde_json::json!("local process only; no cloud telemetry"),
        );
        object.insert(
            "enable_env".to_owned(),
            serde_json::json!(ROUTING_STATS_ENV),
        );
    }
    serde_json::to_string_pretty(&value).expect("routing stats JSON should serialize")
}

/// Clears counters (test helper).
#[cfg(test)]
pub fn reset_for_tests() {
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = RoutingStatsState::default();
    TEST_FORCE_ENABLED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that mutate the shared process counters.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn recording_is_noop_when_disabled() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        reset_for_tests();
        record_tool_call("search_text");
        record_zero_hit();
        record_handle_failure();
        record_workspace_gate_use();
        let snap = snapshot();
        // Without FRIGG_ROUTING_STATS in the ambient env, force flag is off.
        if !snap.enabled {
            assert!(snap.tool_calls.is_empty());
            assert_eq!(snap.zero_hit_count, 0);
            assert_eq!(snap.handle_failures, 0);
            assert_eq!(snap.workspace_gate_uses, 0);
        }
        reset_for_tests();
    }

    #[test]
    fn recording_increments_when_enabled() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        reset_for_tests();
        TEST_FORCE_ENABLED.store(true, Ordering::Relaxed);
        record_tool_call("search_text");
        record_tool_call("search_text");
        record_tool_call("workspace");
        record_zero_hit();
        record_recovery_issued();
        record_handle_failure();
        record_workspace_gate_use();
        let snap = snapshot();
        assert!(snap.enabled);
        assert_eq!(snap.tool_calls.get("search_text"), Some(&2));
        assert_eq!(snap.tool_calls.get("workspace"), Some(&1));
        assert_eq!(snap.total_tool_calls(), 3);
        assert_eq!(snap.zero_hit_count, 1);
        // zero_hit also increments recovery_issued, plus explicit recovery
        assert_eq!(snap.recovery_issued, 2);
        assert_eq!(snap.handle_failures, 1);
        assert_eq!(snap.workspace_gate_uses, 1);
        reset_for_tests();
    }

    #[test]
    fn snapshot_json_includes_schema_and_privacy() {
        let _guard = TEST_LOCK.lock().expect("test lock");
        reset_for_tests();
        TEST_FORCE_ENABLED.store(true, Ordering::Relaxed);
        record_tool_call("search_symbol");
        let body = snapshot_json();
        let value: serde_json::Value =
            serde_json::from_str(&body).expect("snapshot JSON should parse");
        assert_eq!(value["schema_id"], "frigg.stats.routing.v1");
        assert_eq!(value["privacy"], "local process only; no cloud telemetry");
        assert_eq!(value["enabled"], true);
        assert_eq!(value["tool_calls"]["search_symbol"], 1);
        reset_for_tests();
    }
}
