#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PolicyEffect {
    Add(f32),
    Multiply(f32),
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolicyRuleTrace {
    pub rule_id: &'static str,
    pub stage: PolicyStage,
    pub effect: PolicyEffect,
    pub before: f32,
    pub after: f32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PolicyTrace {
    pub rules: Vec<PolicyRuleTrace>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolicyEvaluation {
    pub score: f32,
    pub trace: Option<PolicyTrace>,
}
