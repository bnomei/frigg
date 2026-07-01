use std::path::PathBuf;

use crate::cli_args::AdoptTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdoptPlanEntry {
    pub(crate) repository_id: String,
    pub(crate) root: PathBuf,
    pub(crate) target: AdoptTarget,
    pub(crate) path: PathBuf,
    pub(crate) action: AdoptPlanAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdoptPlanAction {
    PlanInstall,
    PlanUninstall,
}

impl AdoptPlanAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PlanInstall => "plan-install",
            Self::PlanUninstall => "plan-uninstall",
        }
    }
}

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
            action: AdoptPlanAction::PlanInstall,
        }]);

        assert_eq!(plan.len(), 1);
        assert!(!plan.is_empty());
        assert_eq!(plan.entries[0].action.as_str(), "plan-install");
    }
}
