//! Dogfood-shaped anchor: Frigg MCP server type (fixture excerpt).
//! where is the MCP tool surface filtered by profile?

/// MCP server orchestration type used by agents over streamable HTTP or stdio.
pub struct FriggMcpServer {
    pub tool_surface_profile: ToolSurfaceProfile,
}

/// Core vs extended tool surface profile.
pub enum ToolSurfaceProfile {
    Core,
    Extended,
}

impl FriggMcpServer {
    pub fn new() -> Self {
        Self {
            tool_surface_profile: ToolSurfaceProfile::Core,
        }
    }

    pub fn manifest_for_tool_surface_profile(&self) -> &'static [&'static str] {
        // Filtered by profile for discovery probes.
        &["workspace", "search_text", "search_hybrid", "search_symbol", "search_batch"]
    }
}

pub fn stable_repository_id_for_root(_root: &str) -> String {
    "dogfood-repo".to_owned()
}
