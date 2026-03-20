use super::super::super::dsl::{ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;

mod baseline;
mod language_navigation;
mod locality;
mod support;

const RULE_SETS: &[ScoreRuleSet<SelectionFacts>] = &[
    baseline::RULE_SET,
    language_navigation::RULE_SET,
    locality::RULE_SET,
    support::RULE_SET,
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, RULE_SETS);
}
