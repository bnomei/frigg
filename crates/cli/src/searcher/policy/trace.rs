//! Serializable policy trace types for path quality, path witness, selection, and post-selection
//! stages. Used by rule-trace helpers and optional post-selection diagnostics.

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
/// Stage label attached to each applied retrieval-policy rule.
pub(crate) enum PolicyStage {
    PathQuality,
    PathWitness,
    SelectionBase,
    SelectionContracts,
    SelectionNovelty,
    SelectionRuntimeWitness,
    SelectionRuntimeConfig,
    SelectionLaravelUi,
    SelectionTestWitness,
    SelectionNavigation,
    SelectionCiScriptsOps,
    SelectionEntrypoint,
    SelectionDiversification,
    SelectionTail,
    PostSelectionRuntime,
    PostSelectionMixedSupport,
    PostSelectionLaravel,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) enum PolicyEffect {
    Add(f32),
    Multiply(f32),
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct PolicyRuleTrace {
    pub rule_id: &'static str,
    pub stage: PolicyStage,
    pub predicate_ids: Vec<&'static str>,
    pub effect: PolicyEffect,
    pub before: f32,
    pub after: f32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub(crate) struct PolicyTrace {
    pub rules: Vec<PolicyRuleTrace>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct PolicyEvaluation {
    pub score: f32,
    pub trace: Option<PolicyTrace>,
}
