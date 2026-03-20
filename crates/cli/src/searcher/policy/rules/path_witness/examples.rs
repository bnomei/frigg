use super::*;

pub(super) fn examples_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    let delta = if ctx.specific_path_overlap >= 2 {
        5.8
    } else if ctx.specific_path_overlap == 1 {
        4.2
    } else if ctx.has_exact_query_term_match {
        3.4
    } else {
        1.8
    };

    Some(PolicyEffect::Add(delta))
}

pub(super) fn benchmarks_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    let delta = if ctx.specific_path_overlap >= 2 {
        6.4
    } else if ctx.specific_path_overlap == 1 {
        4.8
    } else if ctx.has_exact_query_term_match {
        3.8
    } else {
        2.0
    };

    Some(PolicyEffect::Add(delta))
}

pub(super) fn examples_unwanted_example_support_penalty(
    _ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-3.8))
}
