//! Dogfood-shaped: tool surface filtering by profile.
use super::server::ToolSurfaceProfile;
use super::types::PUBLIC_TOOL_NAMES;

pub fn filter_tools(profile: ToolSurfaceProfile) -> Vec<&'static str> {
    match profile {
        ToolSurfaceProfile::Core => PUBLIC_TOOL_NAMES
            .iter()
            .copied()
            .filter(|name| *name != "explore")
            .collect(),
        ToolSurfaceProfile::Extended => PUBLIC_TOOL_NAMES.to_vec(),
    }
}
