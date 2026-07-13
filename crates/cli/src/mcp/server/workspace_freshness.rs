//! Pure workspace-freshness state derivation.
//!
//! This module intentionally receives an already-captured view of runtime state.  It does not
//! inspect storage, acquire a watch lease, or otherwise touch process state, so callers can use
//! the same transition table for `workspace` and zero-hit diagnostics.

use crate::mcp::types::{
    WatchStatusReason, WatchStatusSummary, WorkspaceContinuousFreshnessState,
    WorkspaceContinuousFreshnessSummary, WorkspaceDirtyScope, WorkspaceFreshnessSummary,
    WorkspacePostEditStrategy, WorkspacePostEditSummary, WorkspaceSnapshotFreshnessState,
    WorkspaceSnapshotSummary,
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
        tool_capabilities: Vec::new(),
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
}
