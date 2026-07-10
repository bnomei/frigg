//! Core versus extended MCP tool-surface profiles and runtime parity checks against the manifest.

use std::collections::BTreeSet;

use crate::mcp::types::PUBLIC_TOOL_NAMES;

/// Environment variable selecting the registered MCP tool subset for this process.
pub const TOOL_SURFACE_PROFILE_ENV: &str = "FRIGG_MCP_TOOL_SURFACE_PROFILE";

/// Registered MCP tool subset exposed by the running server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolSurfaceProfile {
    /// Default product surface: Futura primary loop + in-file `explore` (no playbook tools).
    Core,
    /// Product core plus optional playbook tools when compiled with `--features playbook`.
    ///
    /// Without the playbook feature, core and extended expose the same public tool names.
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

/// Expected tool-name manifest for one `ToolSurfaceProfile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceManifest {
    pub profile: ToolSurfaceProfile,
    pub tool_names: Vec<String>,
}

/// Tools registered only on the extended profile.
///
/// `explore` is **core** (agent product). Playbook tools are **dev/trace tooling**:
/// compile-time opt-in (`--features playbook`) and extended-profile only — not on
/// default cargo features, and not on `core` even when the feature is compiled in.
#[cfg(feature = "playbook")]
const EXTENDED_ONLY_TOOL_NAMES: &[&str] = &[
    "playbook_compose_citations",
    "playbook_replay",
    "playbook_run",
];
#[cfg(not(feature = "playbook"))]
const EXTENDED_ONLY_TOOL_NAMES: &[&str] = &[];

/// Resolves the active tool-surface profile from `FRIGG_MCP_TOOL_SURFACE_PROFILE`.
pub fn active_runtime_tool_surface_profile() -> ToolSurfaceProfile {
    runtime_tool_surface_profile_from_env(std::env::var(TOOL_SURFACE_PROFILE_ENV).ok())
}

fn runtime_tool_surface_profile_from_env(raw: Option<String>) -> ToolSurfaceProfile {
    let Some(raw) = raw else {
        return ToolSurfaceProfile::Extended;
    };

    match raw.trim().to_ascii_lowercase().as_str() {
        "core" => ToolSurfaceProfile::Core,
        "extended" => ToolSurfaceProfile::Extended,
        _ => ToolSurfaceProfile::Extended,
    }
}

fn profile_tool_names(profile: ToolSurfaceProfile) -> Vec<String> {
    let mut names = PUBLIC_TOOL_NAMES
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

/// Builds the expected sorted tool-name manifest for one runtime profile.
pub fn manifest_for_tool_surface_profile(profile: ToolSurfaceProfile) -> ToolSurfaceManifest {
    ToolSurfaceManifest {
        profile,
        tool_names: profile_tool_names(profile),
    }
}

/// Returns manifests for every supported tool-surface profile.
pub fn tool_surface_profile_manifests() -> [ToolSurfaceManifest; 2] {
    [
        manifest_for_tool_surface_profile(ToolSurfaceProfile::Core),
        manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended),
    ]
}

/// Drift between the runtime-registered tool router and a profile manifest.
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

/// Compares a registered tool router against the manifest for a target profile.
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
    fn runtime_tool_surface_profile_from_env_defaults_to_extended() {
        assert_eq!(
            runtime_tool_surface_profile_from_env(None),
            ToolSurfaceProfile::Extended
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("".to_owned())),
            ToolSurfaceProfile::Extended
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("invalid".to_owned())),
            ToolSurfaceProfile::Extended
        );
    }

    #[test]
    fn runtime_tool_surface_profile_from_env_accepts_profiles_case_insensitively() {
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("core".to_owned())),
            ToolSurfaceProfile::Core
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some(" CoRe ".to_owned())),
            ToolSurfaceProfile::Core
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some("extended".to_owned())),
            ToolSurfaceProfile::Extended
        );
        assert_eq!(
            runtime_tool_surface_profile_from_env(Some(" ExTeNdEd ".to_owned())),
            ToolSurfaceProfile::Extended
        );
    }

    #[test]
    fn explore_is_on_core_surface_playbook_is_not() {
        use super::manifest_for_tool_surface_profile;

        let core = manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
        assert!(
            core.tool_names.iter().any(|name| name == "explore"),
            "explore is product tooling and belongs on core"
        );
        assert!(
            !core.tool_names.iter().any(|name| name.starts_with("playbook_")),
            "playbook tools must not appear on core"
        );

        let extended = manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended);
        assert!(
            extended.tool_names.iter().any(|name| name == "explore"),
            "explore remains available on extended"
        );
        #[cfg(feature = "playbook")]
        {
            for playbook in [
                "playbook_run",
                "playbook_replay",
                "playbook_compose_citations",
            ] {
                assert!(
                    extended.tool_names.iter().any(|name| name == playbook),
                    "playbook tool {playbook} should be extended-only when feature is on"
                );
                assert!(
                    !core.tool_names.iter().any(|name| name == playbook),
                    "playbook tool {playbook} must stay off core"
                );
            }
        }
        #[cfg(not(feature = "playbook"))]
        {
            assert_eq!(
                core.tool_names, extended.tool_names,
                "without playbook feature, core and extended expose the same public tools"
            );
        }
    }
}
