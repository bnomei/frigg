use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTransportKind {
    Stdio,
    LoopbackHttp,
    RemoteHttp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Resolved runtime shape that drives persistence and freshness behavior for a process.
pub enum RuntimeProfile {
    StdioEphemeral,
    StdioAttached,
    HttpLoopbackService,
    HttpRemoteService,
}

impl RuntimeProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StdioEphemeral => "stdio_ephemeral",
            Self::StdioAttached => "stdio_attached",
            Self::HttpLoopbackService => "http_loopback_service",
            Self::HttpRemoteService => "http_remote_service",
        }
    }

    pub fn persistent_state_available(self) -> bool {
        matches!(
            self,
            Self::StdioAttached | Self::HttpLoopbackService | Self::HttpRemoteService
        )
    }
}

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
