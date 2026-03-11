use std::collections::BTreeSet;

use crate::mcp::types::PUBLIC_READ_ONLY_TOOL_NAMES;

pub const TOOL_SURFACE_PROFILE_ENV: &str = "FRIGG_MCP_TOOL_SURFACE_PROFILE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolSurfaceProfile {
    /// Stable default read-only runtime surface.
    Core,
    /// Advanced-consumer runtime surface that layers deep-search tools on top of the stable profile.
    Extended,
}

impl ToolSurfaceProfile {
    pub const ALL: [Self; 2] = [Self::Core, Self::Extended];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Extended => "extended",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceManifest {
    pub profile: ToolSurfaceProfile,
    pub tool_names: Vec<String>,
}

const EXTENDED_ONLY_TOOL_NAMES: [&str; 4] = [
    "explore",
    "deep_search_compose_citations",
    "deep_search_replay",
    "deep_search_run",
];

pub fn active_runtime_tool_surface_profile() -> ToolSurfaceProfile {
    runtime_tool_surface_profile_from_env(std::env::var(TOOL_SURFACE_PROFILE_ENV).ok())
}

fn runtime_tool_surface_profile_from_env(raw: Option<String>) -> ToolSurfaceProfile {
    let Some(raw) = raw else {
        return ToolSurfaceProfile::Core;
    };

    match raw.trim().to_ascii_lowercase().as_str() {
        "extended" => ToolSurfaceProfile::Extended,
        _ => ToolSurfaceProfile::Core,
    }
}

fn profile_tool_names(profile: ToolSurfaceProfile) -> Vec<String> {
    let mut names = PUBLIC_READ_ONLY_TOOL_NAMES
        .iter()
        .copied()
        .filter(|tool_name| {
            profile == ToolSurfaceProfile::Extended || !EXTENDED_ONLY_TOOL_NAMES.contains(tool_name)
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

pub fn manifest_for_tool_surface_profile(profile: ToolSurfaceProfile) -> ToolSurfaceManifest {
    ToolSurfaceManifest {
        profile,
        tool_names: profile_tool_names(profile),
    }
}

pub fn tool_surface_profile_manifests() -> [ToolSurfaceManifest; 2] {
    [
        manifest_for_tool_surface_profile(ToolSurfaceProfile::Core),
        manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceParityDiff {
    pub profile: ToolSurfaceProfile,
    pub missing_in_runtime: Vec<String>,
    pub unexpected_in_runtime: Vec<String>,
}

impl ToolSurfaceParityDiff {
    pub fn is_empty(&self) -> bool {
        self.missing_in_runtime.is_empty() && self.unexpected_in_runtime.is_empty()
    }
}

pub fn diff_runtime_against_profile_manifest(
    profile: ToolSurfaceProfile,
    runtime_registered_tool_names: &[String],
) -> ToolSurfaceParityDiff {
    let expected_names = manifest_for_tool_surface_profile(profile)
        .tool_names
        .into_iter()
        .collect::<BTreeSet<_>>();
    let runtime_names = runtime_registered_tool_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    ToolSurfaceParityDiff {
        profile,
        missing_in_runtime: expected_names
            .difference(&runtime_names)
            .cloned()
            .collect::<Vec<_>>(),
        unexpected_in_runtime: runtime_names
            .difference(&expected_names)
            .cloned()
            .collect::<Vec<_>>(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolSurfaceProfile, runtime_tool_surface_profile_from_env};

    #[test]
    fn runtime_tool_surface_profile_from_env_defaults_to_core() {
        assert_eq!(
            runtime_tool_surface_profile_from_env(None),
            ToolSurfaceProfile::Core
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("".to_owned())),
            ToolSurfaceProfile::Core
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("invalid".to_owned())),
            ToolSurfaceProfile::Core
        );
    }

    #[test]
    fn runtime_tool_surface_profile_from_env_accepts_extended_case_insensitively() {
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("extended".to_owned())),
            ToolSurfaceProfile::Extended
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some(" ExTeNdEd ".to_owned())),
            ToolSurfaceProfile::Extended
        );
    }
}
