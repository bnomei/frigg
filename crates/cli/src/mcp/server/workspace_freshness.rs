//! Pure workspace-freshness state derivation.
//!
//! This module intentionally receives an already-captured view of runtime state.  It does not
//! inspect storage, acquire a watch lease, or otherwise touch process state, so callers can use
//! the same transition table for `workspace` and zero-hit diagnostics.

use crate::mcp::types::{
    WatchStatusReason, WatchStatusSummary, WorkspaceContinuousFreshnessState,
    WorkspaceContinuousFreshnessSummary, WorkspaceDirtyScope, WorkspaceFreshnessSummary,
    WorkspacePostEditStrategy, WorkspacePostEditSummary, WorkspaceSnapshotFreshnessState,
    WorkspaceSnapshotSummary, WorkspaceToolFreshnessAvailability, WorkspaceToolFreshnessCapability,
    WorkspaceToolFreshnessPathScope, WorkspaceToolFreshnessSourceBasis,
};
use crate::settings::{RuntimeProfile, RuntimeTransportKind, WatchMode};

/// Complete captured input to the workspace freshness transition table.
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceFreshnessInput {
    pub(crate) snapshot_state: WorkspaceSnapshotFreshnessState,
    pub(crate) storage_available: Option<bool>,
    pub(crate) dirty_scope: WorkspaceDirtyScope,
    pub(crate) changed_paths_since_snapshot: Vec<String>,
    pub(crate) transport: RuntimeTransportKind,
    pub(crate) runtime_profile: RuntimeProfile,
    pub(crate) configured_watch_mode: WatchMode,
    pub(crate) watch_status: WatchStatusSummary,
    /// The live router surface captured with the rest of the response. Capability projection is
    /// added by the follow-up integration task; retaining it here prevents a second state input.
    pub(crate) live_tool_names: Vec<String>,
}

/// Derives the authoritative freshness axes without IO, locking, lease changes, or indexing.
pub(crate) fn derive_workspace_freshness(
    input: &WorkspaceFreshnessInput,
) -> WorkspaceFreshnessSummary {
    let continuous_state = continuous_state(input.watch_status.reason);
    // The captured transport/profile/mode and router list deliberately travel with this input.
    // The watch snapshot is authoritative for the current continuous state; these values are
    // consumed by handler integration when it captures that snapshot and capabilities.
    let _runtime_context = (
        input.transport,
        input.runtime_profile,
        input.configured_watch_mode,
        &input.live_tool_names,
    );
    let has_leased_progress = input.watch_status.lease_count > 0
        && matches!(
            input.watch_status.reason,
            WatchStatusReason::Debouncing | WatchStatusReason::Refreshing
        );
    let can_converge_by_waiting = has_leased_progress;
    let changed_paths_since_snapshot = match input.dirty_scope {
        WorkspaceDirtyScope::KnownChangedPaths => {
            sorted_unique(&input.changed_paths_since_snapshot)
        }
        WorkspaceDirtyScope::Clean | WorkspaceDirtyScope::UnknownRepositoryDirtiness => Vec::new(),
    };
    let strategy = match input.snapshot_state {
        WorkspaceSnapshotFreshnessState::Detached => WorkspacePostEditStrategy::AdoptRepo,
        WorkspaceSnapshotFreshnessState::Missing
        | WorkspaceSnapshotFreshnessState::Uninitialized
        | WorkspaceSnapshotFreshnessState::Error => WorkspacePostEditStrategy::RunCliIndex,
        WorkspaceSnapshotFreshnessState::Unavailable => WorkspacePostEditStrategy::FriggUnavailable,
        WorkspaceSnapshotFreshnessState::Ready => match input.dirty_scope {
            WorkspaceDirtyScope::Clean => WorkspacePostEditStrategy::UseSnapshot,
            WorkspaceDirtyScope::KnownChangedPaths
            | WorkspaceDirtyScope::UnknownRepositoryDirtiness
                if has_leased_progress =>
            {
                WorkspacePostEditStrategy::WaitForRefresh
            }
            WorkspaceDirtyScope::KnownChangedPaths
            | WorkspaceDirtyScope::UnknownRepositoryDirtiness => {
                WorkspacePostEditStrategy::UseLiveDiskForTouchedFiles
            }
        },
    };

    WorkspaceFreshnessSummary {
        snapshot: WorkspaceSnapshotSummary {
            state: input.snapshot_state,
            storage_available: input.storage_available,
        },
        continuous: WorkspaceContinuousFreshnessSummary {
            state: continuous_state,
            can_converge_by_waiting,
        },
        post_edit: WorkspacePostEditSummary { strategy },
        dirty_scope: input.dirty_scope,
        changed_paths_since_snapshot,
        tool_capabilities: derive_tool_capabilities(input),
    }
}

/// Classifies the source consulted by each public tool. Keep this registry keyed by tool name:
/// response rows are intentionally driven by the live router, never by this table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolFreshnessClass {
    SnapshotIndex,
    LiveManifest,
    LiveFile,
    HandleBoundLiveContent,
    Mixed,
    NotApplicable,
}

const UNKNOWN_TOOL_NAME_DIAGNOSTIC_MAX_CHARS: usize = 128;

fn bounded_unknown_tool_name(tool_name: &str) -> String {
    tool_name
        .chars()
        .take(UNKNOWN_TOOL_NAME_DIAGNOSTIC_MAX_CHARS)
        .collect()
}

fn classify_tool(tool_name: &str) -> Option<ToolFreshnessClass> {
    Some(match tool_name {
        // `workspace` reports current control-plane state rather than source evidence.
        "workspace" | "playbook_run" | "playbook_replay" | "playbook_compose_citations" => {
            ToolFreshnessClass::NotApplicable
        }
        "list_files" => ToolFreshnessClass::LiveManifest,
        "read_file" | "explore" => ToolFreshnessClass::LiveFile,
        "read_match" => ToolFreshnessClass::HandleBoundLiveContent,
        // These paths combine source parsing with index-backed resolution; report the more
        // conservative mixed basis rather than claiming an index-only or live-only guarantee.
        "document_symbols" | "inspect_syntax_tree" | "search_structural" | "impact_bundle" => {
            ToolFreshnessClass::Mixed
        }
        "search_text"
        | "search_hybrid"
        | "search_symbol"
        | "search_batch"
        | "find_references"
        | "go_to_definition"
        | "find_declarations"
        | "find_implementations"
        | "incoming_calls"
        | "outgoing_calls" => ToolFreshnessClass::SnapshotIndex,
        _ => return None,
    })
}

fn derive_tool_capabilities(
    input: &WorkspaceFreshnessInput,
) -> Vec<WorkspaceToolFreshnessCapability> {
    let mut tool_names = input.live_tool_names.clone();
    tool_names.sort();
    tool_names.dedup();

    tool_names
        .into_iter()
        .map(|tool_name| capability_for_tool(input, tool_name))
        .collect()
}

fn capability_for_tool(
    input: &WorkspaceFreshnessInput,
    tool_name: String,
) -> WorkspaceToolFreshnessCapability {
    let Some(class) = classify_tool(&tool_name) else {
        // A router/registry mismatch must not silently make a new tool look trustworthy.
        tracing::warn!(
            tool_name = %bounded_unknown_tool_name(&tool_name),
            "workspace freshness registry has no classification for a live tool; failing closed"
        );
        return unavailable_capability(
            tool_name,
            WorkspaceToolFreshnessSourceBasis::Mixed,
            WorkspaceToolFreshnessPathScope::RepositoryWide,
            recovery_for(input.snapshot_state),
        );
    };

    if class == ToolFreshnessClass::NotApplicable {
        return WorkspaceToolFreshnessCapability {
            tool_name,
            source_basis: WorkspaceToolFreshnessSourceBasis::NotApplicable,
            availability: WorkspaceToolFreshnessAvailability::NotApplicable,
            path_scope: WorkspaceToolFreshnessPathScope::NotApplicable,
            required_recovery: None,
        };
    }

    let source_basis = match class {
        ToolFreshnessClass::SnapshotIndex => WorkspaceToolFreshnessSourceBasis::SnapshotIndex,
        ToolFreshnessClass::LiveManifest => WorkspaceToolFreshnessSourceBasis::LiveManifest,
        ToolFreshnessClass::LiveFile => WorkspaceToolFreshnessSourceBasis::LiveFile,
        ToolFreshnessClass::HandleBoundLiveContent => {
            WorkspaceToolFreshnessSourceBasis::HandleBoundLiveContent
        }
        ToolFreshnessClass::Mixed => WorkspaceToolFreshnessSourceBasis::Mixed,
        ToolFreshnessClass::NotApplicable => unreachable!("handled above"),
    };

    if matches!(
        input.snapshot_state,
        WorkspaceSnapshotFreshnessState::Detached | WorkspaceSnapshotFreshnessState::Unavailable
    ) {
        return unavailable_capability(
            tool_name,
            source_basis,
            WorkspaceToolFreshnessPathScope::RepositoryWide,
            recovery_for(input.snapshot_state),
        );
    }

    if matches!(
        class,
        ToolFreshnessClass::SnapshotIndex | ToolFreshnessClass::Mixed
    ) && input.snapshot_state != WorkspaceSnapshotFreshnessState::Ready
    {
        return unavailable_capability(
            tool_name,
            source_basis,
            WorkspaceToolFreshnessPathScope::RepositoryWide,
            recovery_for(input.snapshot_state),
        );
    }

    match class {
        ToolFreshnessClass::LiveManifest | ToolFreshnessClass::LiveFile => fully_fresh_capability(
            tool_name,
            source_basis,
            WorkspaceToolFreshnessPathScope::RepositoryWide,
        ),
        ToolFreshnessClass::HandleBoundLiveContent => WorkspaceToolFreshnessCapability {
            tool_name,
            source_basis,
            // The bytes are read live, but the anchor belongs to a previous producer result.
            // Never let the legacy full-fresh projection include this replay-sensitive tool.
            availability: WorkspaceToolFreshnessAvailability::StalePossible,
            path_scope: WorkspaceToolFreshnessPathScope::HandleGenerationBound,
            required_recovery: (input.dirty_scope != WorkspaceDirtyScope::Clean)
                .then_some(WorkspacePostEditStrategy::UseLiveDiskForTouchedFiles),
        },
        ToolFreshnessClass::SnapshotIndex | ToolFreshnessClass::Mixed => match input.dirty_scope {
            WorkspaceDirtyScope::Clean => fully_fresh_capability(
                tool_name,
                source_basis,
                WorkspaceToolFreshnessPathScope::RepositoryWide,
            ),
            WorkspaceDirtyScope::KnownChangedPaths => WorkspaceToolFreshnessCapability {
                tool_name,
                source_basis,
                availability: WorkspaceToolFreshnessAvailability::StalePossible,
                path_scope: WorkspaceToolFreshnessPathScope::TouchedPaths,
                required_recovery: Some(input_recovery(input)),
            },
            WorkspaceDirtyScope::UnknownRepositoryDirtiness => WorkspaceToolFreshnessCapability {
                tool_name,
                source_basis,
                availability: WorkspaceToolFreshnessAvailability::StalePossible,
                path_scope: WorkspaceToolFreshnessPathScope::RepositoryWide,
                required_recovery: Some(input_recovery(input)),
            },
        },
        ToolFreshnessClass::NotApplicable => unreachable!("handled above"),
    }
}

fn fully_fresh_capability(
    tool_name: String,
    source_basis: WorkspaceToolFreshnessSourceBasis,
    path_scope: WorkspaceToolFreshnessPathScope,
) -> WorkspaceToolFreshnessCapability {
    WorkspaceToolFreshnessCapability {
        tool_name,
        source_basis,
        availability: WorkspaceToolFreshnessAvailability::FullyFresh,
        path_scope,
        required_recovery: None,
    }
}

fn unavailable_capability(
    tool_name: String,
    source_basis: WorkspaceToolFreshnessSourceBasis,
    path_scope: WorkspaceToolFreshnessPathScope,
    required_recovery: Option<WorkspacePostEditStrategy>,
) -> WorkspaceToolFreshnessCapability {
    WorkspaceToolFreshnessCapability {
        tool_name,
        source_basis,
        availability: WorkspaceToolFreshnessAvailability::Unavailable,
        path_scope,
        required_recovery,
    }
}

const fn recovery_for(
    snapshot_state: WorkspaceSnapshotFreshnessState,
) -> Option<WorkspacePostEditStrategy> {
    match snapshot_state {
        WorkspaceSnapshotFreshnessState::Detached => Some(WorkspacePostEditStrategy::AdoptRepo),
        WorkspaceSnapshotFreshnessState::Missing
        | WorkspaceSnapshotFreshnessState::Uninitialized
        | WorkspaceSnapshotFreshnessState::Error => Some(WorkspacePostEditStrategy::RunCliIndex),
        WorkspaceSnapshotFreshnessState::Unavailable => {
            Some(WorkspacePostEditStrategy::FriggUnavailable)
        }
        WorkspaceSnapshotFreshnessState::Ready => None,
    }
}

fn input_recovery(input: &WorkspaceFreshnessInput) -> WorkspacePostEditStrategy {
    match input.snapshot_state {
        WorkspaceSnapshotFreshnessState::Ready
            if input.watch_status.lease_count > 0
                && matches!(
                    input.watch_status.reason,
                    WatchStatusReason::Debouncing | WatchStatusReason::Refreshing
                ) =>
        {
            WorkspacePostEditStrategy::WaitForRefresh
        }
        WorkspaceSnapshotFreshnessState::Ready => {
            WorkspacePostEditStrategy::UseLiveDiskForTouchedFiles
        }
        state => recovery_for(state).expect("non-ready snapshot has a recovery"),
    }
}

fn sorted_unique(paths: &[String]) -> Vec<String> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    paths
}

const fn continuous_state(reason: WatchStatusReason) -> WorkspaceContinuousFreshnessState {
    match reason {
        WatchStatusReason::ModeOff => WorkspaceContinuousFreshnessState::ModeOff,
        WatchStatusReason::NoLease => WorkspaceContinuousFreshnessState::NoLease,
        WatchStatusReason::Active => WorkspaceContinuousFreshnessState::Active,
        WatchStatusReason::Debouncing => WorkspaceContinuousFreshnessState::Debouncing,
        WatchStatusReason::Refreshing => WorkspaceContinuousFreshnessState::Refreshing,
        WatchStatusReason::RetryBackoff => WorkspaceContinuousFreshnessState::RetryBackoff,
        WatchStatusReason::Blocked => WorkspaceContinuousFreshnessState::Blocked,
        WatchStatusReason::NotifyDegraded => WorkspaceContinuousFreshnessState::NotifyDegraded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOTS: [WorkspaceSnapshotFreshnessState; 6] = [
        WorkspaceSnapshotFreshnessState::Detached,
        WorkspaceSnapshotFreshnessState::Missing,
        WorkspaceSnapshotFreshnessState::Uninitialized,
        WorkspaceSnapshotFreshnessState::Ready,
        WorkspaceSnapshotFreshnessState::Error,
        WorkspaceSnapshotFreshnessState::Unavailable,
    ];
    const DIRTY_SCOPES: [WorkspaceDirtyScope; 3] = [
        WorkspaceDirtyScope::Clean,
        WorkspaceDirtyScope::KnownChangedPaths,
        WorkspaceDirtyScope::UnknownRepositoryDirtiness,
    ];
    const TRANSPORTS: [RuntimeTransportKind; 3] = [
        RuntimeTransportKind::Stdio,
        RuntimeTransportKind::LoopbackHttp,
        RuntimeTransportKind::RemoteHttp,
    ];
    const PROFILES: [RuntimeProfile; 4] = [
        RuntimeProfile::StdioEphemeral,
        RuntimeProfile::StdioAttached,
        RuntimeProfile::HttpLoopbackService,
        RuntimeProfile::HttpRemoteService,
    ];
    const WATCH_MODES: [WatchMode; 3] = [WatchMode::Auto, WatchMode::On, WatchMode::Off];
    const REASONS: [WatchStatusReason; 8] = [
        WatchStatusReason::ModeOff,
        WatchStatusReason::NoLease,
        WatchStatusReason::Active,
        WatchStatusReason::Debouncing,
        WatchStatusReason::Refreshing,
        WatchStatusReason::RetryBackoff,
        WatchStatusReason::Blocked,
        WatchStatusReason::NotifyDegraded,
    ];

    fn input(
        snapshot_state: WorkspaceSnapshotFreshnessState,
        dirty_scope: WorkspaceDirtyScope,
        transport: RuntimeTransportKind,
        runtime_profile: RuntimeProfile,
        configured_watch_mode: WatchMode,
        reason: WatchStatusReason,
        lease_count: usize,
    ) -> WorkspaceFreshnessInput {
        WorkspaceFreshnessInput {
            snapshot_state,
            storage_available: Some(true),
            dirty_scope,
            changed_paths_since_snapshot: vec![
                "z.rs".to_owned(),
                "a.rs".to_owned(),
                "a.rs".to_owned(),
            ],
            transport,
            runtime_profile,
            configured_watch_mode,
            watch_status: WatchStatusSummary {
                reason,
                lease_count,
                repository_id: None,
                detail: None,
                refresh_queue_depth: None,
                pending_dirty_path_count: None,
                oldest_pending_age_ms: None,
            },
            live_tool_names: vec!["read_file".to_owned()],
        }
    }

    #[test]
    fn workspace_freshness_cartesian_transition_table() {
        for snapshot in SNAPSHOTS {
            for dirty_scope in DIRTY_SCOPES {
                for transport in TRANSPORTS {
                    for profile in PROFILES {
                        for mode in WATCH_MODES {
                            for reason in REASONS {
                                for lease_count in [0, 1] {
                                    let result = derive_workspace_freshness(&input(
                                        snapshot,
                                        dirty_scope,
                                        transport,
                                        profile,
                                        mode,
                                        reason,
                                        lease_count,
                                    ));
                                    let leased_progress = lease_count > 0
                                        && matches!(
                                            reason,
                                            WatchStatusReason::Debouncing
                                                | WatchStatusReason::Refreshing
                                        );
                                    assert_eq!(
                                        result.continuous.can_converge_by_waiting,
                                        leased_progress
                                    );
                                    let expected = match snapshot {
                                        WorkspaceSnapshotFreshnessState::Detached => {
                                            WorkspacePostEditStrategy::AdoptRepo
                                        }
                                        WorkspaceSnapshotFreshnessState::Missing
                                        | WorkspaceSnapshotFreshnessState::Uninitialized
                                        | WorkspaceSnapshotFreshnessState::Error => {
                                            WorkspacePostEditStrategy::RunCliIndex
                                        }
                                        WorkspaceSnapshotFreshnessState::Unavailable => {
                                            WorkspacePostEditStrategy::FriggUnavailable
                                        }
                                        WorkspaceSnapshotFreshnessState::Ready
                                            if dirty_scope == WorkspaceDirtyScope::Clean =>
                                        {
                                            WorkspacePostEditStrategy::UseSnapshot
                                        }
                                        WorkspaceSnapshotFreshnessState::Ready
                                            if leased_progress =>
                                        {
                                            WorkspacePostEditStrategy::WaitForRefresh
                                        }
                                        WorkspaceSnapshotFreshnessState::Ready => {
                                            WorkspacePostEditStrategy::UseLiveDiskForTouchedFiles
                                        }
                                    };
                                    assert_eq!(result.post_edit.strategy, expected);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn no_state_projects_wait_without_active_leased_progress() {
        for reason in REASONS {
            for lease_count in [0, 1] {
                let result = derive_workspace_freshness(&input(
                    WorkspaceSnapshotFreshnessState::Ready,
                    WorkspaceDirtyScope::KnownChangedPaths,
                    RuntimeTransportKind::LoopbackHttp,
                    RuntimeProfile::HttpLoopbackService,
                    WatchMode::On,
                    reason,
                    lease_count,
                ));
                if result.post_edit.strategy == WorkspacePostEditStrategy::WaitForRefresh {
                    assert!(lease_count > 0);
                    assert!(matches!(
                        reason,
                        WatchStatusReason::Debouncing | WatchStatusReason::Refreshing
                    ));
                    assert!(result.continuous.can_converge_by_waiting);
                }
            }
        }
    }

    #[test]
    fn clean_ready_snapshot_uses_snapshot_under_stdio_and_watch_off_http() {
        for (transport, profile, mode) in [
            (
                RuntimeTransportKind::Stdio,
                RuntimeProfile::StdioEphemeral,
                WatchMode::Off,
            ),
            (
                RuntimeTransportKind::LoopbackHttp,
                RuntimeProfile::HttpLoopbackService,
                WatchMode::Off,
            ),
        ] {
            let result = derive_workspace_freshness(&input(
                WorkspaceSnapshotFreshnessState::Ready,
                WorkspaceDirtyScope::Clean,
                transport,
                profile,
                mode,
                WatchStatusReason::NoLease,
                0,
            ));
            assert_eq!(
                result.post_edit.strategy,
                WorkspacePostEditStrategy::UseSnapshot
            );
            assert!(!result.continuous.can_converge_by_waiting);
        }
    }

    #[test]
    fn only_known_paths_are_retained_and_sorted() {
        let known = derive_workspace_freshness(&input(
            WorkspaceSnapshotFreshnessState::Ready,
            WorkspaceDirtyScope::KnownChangedPaths,
            RuntimeTransportKind::Stdio,
            RuntimeProfile::StdioAttached,
            WatchMode::On,
            WatchStatusReason::Active,
            1,
        ));
        assert_eq!(known.changed_paths_since_snapshot, ["a.rs", "z.rs"]);
        let unknown = derive_workspace_freshness(&input(
            WorkspaceSnapshotFreshnessState::Ready,
            WorkspaceDirtyScope::UnknownRepositoryDirtiness,
            RuntimeTransportKind::Stdio,
            RuntimeProfile::StdioAttached,
            WatchMode::On,
            WatchStatusReason::Active,
            1,
        ));
        assert!(unknown.changed_paths_since_snapshot.is_empty());
    }

    #[test]
    fn capability_rows_follow_live_router_names_and_fail_closed() {
        let mut captured = input(
            WorkspaceSnapshotFreshnessState::Ready,
            WorkspaceDirtyScope::KnownChangedPaths,
            RuntimeTransportKind::LoopbackHttp,
            RuntimeProfile::HttpLoopbackService,
            WatchMode::On,
            WatchStatusReason::Active,
            1,
        );
        captured.live_tool_names = vec![
            "unknown_live_tool".to_owned(),
            "read_match".to_owned(),
            "read_file".to_owned(),
            "search_text".to_owned(),
            "list_files".to_owned(),
        ];

        let capabilities = derive_workspace_freshness(&captured).tool_capabilities;
        assert_eq!(
            capabilities
                .iter()
                .map(|capability| capability.tool_name.as_str())
                .collect::<Vec<_>>(),
            [
                "list_files",
                "read_file",
                "read_match",
                "search_text",
                "unknown_live_tool"
            ]
        );

        let read_file = capabilities
            .iter()
            .find(|capability| capability.tool_name == "read_file")
            .expect("read_file row");
        assert_eq!(
            read_file.source_basis,
            WorkspaceToolFreshnessSourceBasis::LiveFile
        );
        assert_eq!(
            read_file.availability,
            WorkspaceToolFreshnessAvailability::FullyFresh
        );

        let read_match = capabilities
            .iter()
            .find(|capability| capability.tool_name == "read_match")
            .expect("read_match row");
        assert_eq!(
            read_match.source_basis,
            WorkspaceToolFreshnessSourceBasis::HandleBoundLiveContent
        );
        assert_eq!(
            read_match.availability,
            WorkspaceToolFreshnessAvailability::StalePossible
        );
        assert_eq!(
            read_match.path_scope,
            WorkspaceToolFreshnessPathScope::HandleGenerationBound
        );

        let unknown = capabilities
            .iter()
            .find(|capability| capability.tool_name == "unknown_live_tool")
            .expect("unknown row");
        assert_eq!(
            unknown.availability,
            WorkspaceToolFreshnessAvailability::Unavailable
        );
        assert!(unknown.required_recovery.is_none());
    }

    #[test]
    fn registry_covers_every_public_core_and_extended_tool() {
        for manifest in crate::mcp::tool_surface::tool_surface_profile_manifests() {
            for tool_name in manifest.tool_names {
                assert!(
                    classify_tool(&tool_name).is_some(),
                    "{} profile is missing a freshness classification for {tool_name}",
                    manifest.profile.as_str(),
                );
            }
        }
    }

    #[test]
    fn playbook_tools_are_not_applicable_when_compiled_and_exposed() {
        #[cfg(feature = "playbook")]
        for tool_name in [
            "playbook_run",
            "playbook_replay",
            "playbook_compose_citations",
        ] {
            let capability = capability_for_tool(
                &input(
                    WorkspaceSnapshotFreshnessState::Ready,
                    WorkspaceDirtyScope::Clean,
                    RuntimeTransportKind::LoopbackHttp,
                    RuntimeProfile::HttpLoopbackService,
                    WatchMode::On,
                    WatchStatusReason::Active,
                    1,
                ),
                tool_name.to_owned(),
            );
            assert_eq!(
                capability.availability,
                WorkspaceToolFreshnessAvailability::NotApplicable
            );
        }
    }

    #[test]
    fn unknown_tool_diagnostics_bound_untrusted_router_names() {
        let long_name = "x".repeat(UNKNOWN_TOOL_NAME_DIAGNOSTIC_MAX_CHARS + 1);
        assert_eq!(
            bounded_unknown_tool_name(&long_name).chars().count(),
            UNKNOWN_TOOL_NAME_DIAGNOSTIC_MAX_CHARS
        );
        assert_eq!(bounded_unknown_tool_name("known_tool"), "known_tool");
    }
}
