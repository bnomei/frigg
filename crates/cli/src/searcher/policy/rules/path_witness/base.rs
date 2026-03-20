use super::*;

pub(super) fn path_witness_entrypoint_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(4.0))
}

pub(super) fn path_witness_build_flow_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(3.2))
}

pub(super) fn path_witness_workflow_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_entrypoint_build_workflow
        .then_some(PolicyEffect::Add(if ctx.path_overlap == 0 {
            10.4
        } else {
            7.2
        }))
}

pub(super) fn path_witness_ci_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(6.2))
}
