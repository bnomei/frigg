//! Watch and index freshness policy for long-lived MCP runtimes.
//!
//! Resolves watch enablement against transport defaults and exposes debounce, retry, and
//! concurrency knobs that the supervisor uses to schedule manifest-fast and semantic-followup work.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::domain::{FriggError, FriggResult};

use super::RuntimeTransportKind;

/// Default debounce interval before watch-triggered index refresh runs.
pub const DEFAULT_WATCH_DEBOUNCE_MS: u64 = 2_000;
/// Default retry interval after a watch-driven index refresh failure.
pub const DEFAULT_WATCH_RETRY_MS: u64 = 5_000;
/// Default manifest-fast refreshes allowed to run concurrently.
pub const DEFAULT_WATCH_MANIFEST_FAST_CONCURRENCY: usize = 1;
/// Default semantic-followup refreshes allowed to run concurrently.
pub const DEFAULT_WATCH_SEMANTIC_FOLLOWUP_CONCURRENCY: usize = 1;

fn default_manifest_fast_concurrency() -> usize {
    DEFAULT_WATCH_MANIFEST_FAST_CONCURRENCY
}

fn default_semantic_followup_concurrency() -> usize {
    DEFAULT_WATCH_SEMANTIC_FOLLOWUP_CONCURRENCY
}

/// Watch enablement mode resolved against transport defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WatchMode {
    #[default]
    Auto,
    On,
    Off,
}

impl WatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

impl std::fmt::Display for WatchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for WatchMode {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "auto" => Ok(Self::Auto),
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            _ => Err(format!(
                "watch mode must be one of: auto, on, off (received: {normalized})"
            )),
        }
    }
}

/// Freshness policy for background watch-driven index after manifest changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchConfig {
    pub mode: WatchMode,
    pub debounce_ms: u64,
    pub retry_ms: u64,
    #[serde(default = "default_manifest_fast_concurrency")]
    pub manifest_fast_concurrency: usize,
    #[serde(default = "default_semantic_followup_concurrency")]
    pub semantic_followup_concurrency: usize,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            mode: WatchMode::Auto,
            debounce_ms: DEFAULT_WATCH_DEBOUNCE_MS,
            retry_ms: DEFAULT_WATCH_RETRY_MS,
            manifest_fast_concurrency: DEFAULT_WATCH_MANIFEST_FAST_CONCURRENCY,
            semantic_followup_concurrency: DEFAULT_WATCH_SEMANTIC_FOLLOWUP_CONCURRENCY,
        }
    }
}

impl WatchConfig {
    pub(crate) fn validate(&self) -> FriggResult<()> {
        if self.debounce_ms == 0 {
            return Err(FriggError::InvalidInput(
                "watch.debounce_ms must be greater than zero".to_owned(),
            ));
        }

        if self.retry_ms == 0 {
            return Err(FriggError::InvalidInput(
                "watch.retry_ms must be greater than zero".to_owned(),
            ));
        }

        if self.manifest_fast_concurrency == 0 {
            return Err(FriggError::InvalidInput(
                "watch.manifest_fast_concurrency must be greater than zero".to_owned(),
            ));
        }

        if self.semantic_followup_concurrency == 0 {
            return Err(FriggError::InvalidInput(
                "watch.semantic_followup_concurrency must be greater than zero".to_owned(),
            ));
        }

        Ok(())
    }

    /// Applies transport-specific defaults, disabling watch for stdio unless overridden.
    pub fn default_for_transport(transport: RuntimeTransportKind) -> Self {
        let mut watch = Self::default();
        if transport == RuntimeTransportKind::Stdio {
            watch.mode = WatchMode::Off;
        }
        watch
    }

    /// Resolves whether watch should run for the given transport and mode.
    pub fn enabled_for_transport(&self, transport: RuntimeTransportKind) -> bool {
        match self.mode {
            WatchMode::On => true,
            WatchMode::Off => false,
            WatchMode::Auto => matches!(
                transport,
                RuntimeTransportKind::Stdio | RuntimeTransportKind::LoopbackHttp
            ),
        }
    }
}
