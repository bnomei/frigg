#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use frigg::mcp::FriggMcpServer;
use frigg::mcp::tool_surface::{
    ToolSurfaceProfile, active_runtime_tool_surface_profile, manifest_for_tool_surface_profile,
    tool_surface_profile_manifests,
};
use frigg::settings::FriggConfig;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repos/manifest-determinism")
}

fn build_server() -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![fixture_root()])
        .expect("fixture root must produce valid config");
    FriggMcpServer::new(config)
}

fn assert_sorted_and_unique(profile: ToolSurfaceProfile, tool_names: &[String]) {
    assert!(
        tool_names.windows(2).all(|window| window[0] <= window[1]),
        "tool manifest names must be sorted for profile={} names={tool_names:?}",
        profile.as_str()
    );

    let unique_count = tool_names.iter().cloned().collect::<BTreeSet<_>>().len();
    assert_eq!(
        unique_count,
        tool_names.len(),
        "tool manifest names must be unique for profile={} names={tool_names:?}",
        profile.as_str()
    );
}

#[test]
fn tool_surface_profile_manifests_are_deterministic() {
    let manifests = tool_surface_profile_manifests();
    let ordered_profiles = manifests
        .iter()
        .map(|manifest| manifest.profile)
        .collect::<Vec<_>>();
    assert_eq!(
        ordered_profiles,
        vec![ToolSurfaceProfile::Core, ToolSurfaceProfile::Extended],
        "tool-surface profiles must stay deterministic and explicit"
    );

    for manifest in manifests {
        assert!(
            !manifest.tool_names.is_empty(),
            "tool manifest must not be empty for profile={}",
            manifest.profile.as_str()
        );
        assert_sorted_and_unique(manifest.profile, &manifest.tool_names);
    }
}

#[test]
fn tool_surface_runtime_registration_matches_profile_manifests() {
    let server = build_server();
    let active_profile = active_runtime_tool_surface_profile();
    let diff = server.runtime_tool_surface_parity(active_profile);
    assert!(
        diff.is_empty(),
        "runtime tool surface drifted for profile={} missing_in_runtime={:?} unexpected_in_runtime={:?}",
        active_profile.as_str(),
        diff.missing_in_runtime,
        diff.unexpected_in_runtime
    );
}

#[test]
fn tool_surface_runtime_registration_matches_active_profile_order() {
    let server = build_server();
    let active_profile = active_runtime_tool_surface_profile();
    let runtime_names = server.runtime_registered_tool_names();
    let expected = manifest_for_tool_surface_profile(active_profile).tool_names;

    assert_eq!(
        runtime_names,
        expected,
        "runtime tools/list order drifted from profile={} manifest order",
        active_profile.as_str()
    );
}
