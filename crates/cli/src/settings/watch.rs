use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::RuntimeTransportKind;

pub const DEFAULT_WATCH_DEBOUNCE_MS: u64 = 2_000;
pub const DEFAULT_WATCH_RETRY_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchMode {
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

impl Default for WatchMode {
    fn default() -> Self {
        Self::Auto
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Freshness policy for long-lived runtimes that may want incremental reindexing in the
/// background.
pub struct WatchConfig {
    pub mode: WatchMode,
    pub debounce_ms: u64,
    pub retry_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            mode: WatchMode::Auto,
            debounce_ms: DEFAULT_WATCH_DEBOUNCE_MS,
            retry_ms: DEFAULT_WATCH_RETRY_MS,
        }
    }
}

impl WatchConfig {
    pub fn default_for_transport(transport: RuntimeTransportKind) -> Self {
        let mut watch = Self::default();
        if transport == RuntimeTransportKind::Stdio {
            watch.mode = WatchMode::Off;
        }
        watch
    }

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
