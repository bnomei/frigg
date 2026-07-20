//! Runtime profile resolution from transport and watch settings.
//!
//! Maps stdio versus HTTP transport and watch attachment into persistence and freshness profiles
//! that gate manifest reuse, watch supervisor startup, and long-lived service behavior.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// MCP transport shape that drives persistence and watch defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTransportKind {
    Stdio,
    LoopbackHttp,
    RemoteHttp,
}

impl RuntimeTransportKind {
    /// Snake-case transport id used in status, profile mapping, and operator diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::LoopbackHttp => "loopback_http",
            Self::RemoteHttp => "remote_http",
        }
    }
}

/// Resolved runtime shape that drives persistence and freshness behavior for a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProfile {
    StdioEphemeral,
    StdioAttached,
    HttpLoopbackService,
    HttpRemoteService,
}

impl RuntimeProfile {
    /// Snake-case profile id exposed in workspace status and runtime policy surfaces.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StdioEphemeral => "stdio_ephemeral",
            Self::StdioAttached => "stdio_attached",
            Self::HttpLoopbackService => "http_loopback_service",
            Self::HttpRemoteService => "http_remote_service",
        }
    }

    /// Whether durable storage and watch-driven index are expected for this profile.
    pub fn persistent_state_available(self) -> bool {
        matches!(
            self,
            Self::StdioAttached | Self::HttpLoopbackService | Self::HttpRemoteService
        )
    }
}

/// Maps transport and watch state to the runtime profile used for storage lifecycle.
pub fn runtime_profile_for_transport(
    transport: RuntimeTransportKind,
    watch_enabled: bool,
) -> RuntimeProfile {
    match transport {
        RuntimeTransportKind::Stdio if watch_enabled => RuntimeProfile::StdioAttached,
        RuntimeTransportKind::Stdio => RuntimeProfile::StdioEphemeral,
        RuntimeTransportKind::LoopbackHttp => RuntimeProfile::HttpLoopbackService,
        RuntimeTransportKind::RemoteHttp => RuntimeProfile::HttpRemoteService,
    }
}
