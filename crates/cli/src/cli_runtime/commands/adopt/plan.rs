//! Adopt planning types: per-repository target classification and roll-up counts for pending changes.

use std::path::PathBuf;

use crate::cli_args::AdoptTarget;

/// One adopt target under a repository root with the planned action and optional classification reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdoptPlanEntry {
    pub(crate) repository_id: String,
    pub(crate) root: PathBuf,
    pub(crate) target: AdoptTarget,
    pub(crate) path: PathBuf,
    pub(crate) action: AdoptPlanAction,
    pub(crate) reason: Option<String>,
}

/// Planned adopt outcome for one target file before any write is attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdoptPlanAction {
    Create,
    Update,
    Unchanged,
    Remove,
    Skipped,
    Error,
}

impl AdoptPlanAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
            Self::Remove => "remove",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }

    pub(crate) fn is_pending_change(self) -> bool {
        matches!(
            self,
            Self::Create | Self::Update | Self::Remove | Self::Error
        )
    }
}

/// Full adopt plan across repositories and targets, used for check, dry-run, and apply flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdoptPlan {
    pub(crate) entries: Vec<AdoptPlanEntry>,
}

impl AdoptPlan {
    pub(crate) fn new(entries: Vec<AdoptPlanEntry>) -> Self {
        Self { entries }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn pending_changes(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.action.is_pending_change())
            .count()
    }

    pub(crate) fn action_count(&self, action: AdoptPlanAction) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.action == action)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::{AdoptPlan, AdoptPlanAction, AdoptPlanEntry};
    use crate::cli_args::AdoptTarget;

    #[test]
    fn adopt_plan_counts_entries() {
        let plan = AdoptPlan::new(vec![AdoptPlanEntry {
            repository_id: "repo-001".to_owned(),
            root: "/workspace".into(),
            target: AdoptTarget::AgentsMd,
            path: "/workspace/AGENTS.md".into(),
            action: AdoptPlanAction::Create,
            reason: None,
        }]);

        assert_eq!(plan.len(), 1);
        assert!(!plan.is_empty());
        assert_eq!(plan.entries[0].action.as_str(), "create");
        assert_eq!(plan.pending_changes(), 1);
        assert_eq!(plan.action_count(AdoptPlanAction::Create), 1);
    }
}
