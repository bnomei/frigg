use super::kernel::PolicyProgram;
use super::trace::{PolicyEffect, PolicyStage};

pub(crate) type PredicateFn<Ctx> = fn(&Ctx) -> bool;
pub(crate) type RuleFn<Ctx> = fn(&Ctx) -> Option<PolicyEffect>;

#[derive(Clone, Copy)]
pub(crate) struct PredicateLeaf<Ctx: 'static> {
    pub id: &'static str,
    pub eval: PredicateFn<Ctx>,
}

impl<Ctx: 'static> PredicateLeaf<Ctx> {
    pub(crate) const fn new(id: &'static str, eval: PredicateFn<Ctx>) -> Self {
        Self { id, eval }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Predicate<Ctx: 'static> {
    pub all: &'static [PredicateLeaf<Ctx>],
    pub any: &'static [PredicateLeaf<Ctx>],
    pub none: &'static [PredicateLeaf<Ctx>],
}

impl<Ctx: 'static> Predicate<Ctx> {
    pub(crate) const ALWAYS: Self = Self {
        all: &[],
        any: &[],
        none: &[],
    };

    pub(crate) const fn new(
        all: &'static [PredicateLeaf<Ctx>],
        any: &'static [PredicateLeaf<Ctx>],
        none: &'static [PredicateLeaf<Ctx>],
    ) -> Self {
        Self { all, any, none }
    }

    pub(crate) const fn all(all: &'static [PredicateLeaf<Ctx>]) -> Self {
        Self::new(all, &[], &[])
    }

    pub(crate) const fn any(any: &'static [PredicateLeaf<Ctx>]) -> Self {
        Self::new(&[], any, &[])
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScoreRule<Ctx: 'static> {
    pub id: &'static str,
    pub stage: PolicyStage,
    pub when: Predicate<Ctx>,
    pub eval: RuleFn<Ctx>,
}

impl<Ctx: 'static> ScoreRule<Ctx> {
    pub(crate) const fn when(
        id: &'static str,
        stage: PolicyStage,
        when: Predicate<Ctx>,
        eval: RuleFn<Ctx>,
    ) -> Self {
        Self {
            id,
            stage,
            when,
            eval,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScoreRuleSet<Ctx: 'static> {
    pub rules: &'static [ScoreRule<Ctx>],
}

impl<Ctx: 'static> ScoreRuleSet<Ctx> {
    pub(crate) const fn new(rules: &'static [ScoreRule<Ctx>]) -> Self {
        Self { rules }
    }
}

fn matched_predicate_ids<Ctx: 'static>(
    ctx: &Ctx,
    predicate: &Predicate<Ctx>,
) -> Option<Vec<&'static str>> {
    let mut matched = Vec::new();

    for leaf in predicate.all {
        if !(leaf.eval)(ctx) {
            return None;
        }
        matched.push(leaf.id);
    }

    if !predicate.any.is_empty() {
        let any_matches = predicate
            .any
            .iter()
            .filter_map(|leaf| (leaf.eval)(ctx).then_some(leaf.id))
            .collect::<Vec<_>>();
        if any_matches.is_empty() {
            return None;
        }
        matched.extend(any_matches);
    }

    if predicate.none.iter().any(|leaf| (leaf.eval)(ctx)) {
        return None;
    }

    Some(matched)
}

pub(crate) fn predicate_matches<Ctx: 'static>(ctx: &Ctx, predicate: Predicate<Ctx>) -> bool {
    matched_predicate_ids(ctx, &predicate).is_some()
}

pub(crate) fn apply_score_rules<Ctx: 'static>(
    program: &mut PolicyProgram,
    ctx: &Ctx,
    rules: &[ScoreRule<Ctx>],
) -> bool {
    let mut applied = false;
    for rule in rules {
        let Some(predicate_ids) = matched_predicate_ids(ctx, &rule.when) else {
            continue;
        };
        if let Some(effect) = (rule.eval)(ctx) {
            applied = true;
            program.apply_effect(rule.id, rule.stage, &predicate_ids, effect);
        }
    }
    applied
}

pub(crate) fn apply_score_rule_sets<Ctx: 'static>(
    program: &mut PolicyProgram,
    ctx: &Ctx,
    rule_sets: &[ScoreRuleSet<Ctx>],
) -> bool {
    let mut applied = false;
    for rule_set in rule_sets {
        applied |= apply_score_rules(program, ctx, rule_set.rules);
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    struct TestCtx {
        enabled: bool,
        multiplier: f32,
    }

    fn gate(ctx: &TestCtx) -> bool {
        ctx.enabled
    }

    const TEST_ENABLED: PredicateLeaf<TestCtx> = PredicateLeaf::new("test.enabled", gate);
    const TEST_GATE: PredicateLeaf<TestCtx> = PredicateLeaf::new("test.gate", gate);
    const TEST_ENABLED_PREDICATES: &[PredicateLeaf<TestCtx>] = &[TEST_ENABLED];
    const TEST_GATE_PREDICATES: &[PredicateLeaf<TestCtx>] = &[TEST_GATE];

    fn add_rule(ctx: &TestCtx) -> Option<PolicyEffect> {
        ctx.enabled.then_some(PolicyEffect::Add(2.0))
    }

    fn mul_rule(ctx: &TestCtx) -> Option<PolicyEffect> {
        ctx.enabled
            .then_some(PolicyEffect::Multiply(ctx.multiplier))
    }

    #[test]
    fn policy_rules_apply_in_declared_order() {
        let ctx = TestCtx {
            enabled: true,
            multiplier: 3.0,
        };
        let rules = &[
            ScoreRule::when(
                "test.add",
                PolicyStage::PathQuality,
                Predicate::all(TEST_ENABLED_PREDICATES),
                add_rule,
            ),
            ScoreRule::when(
                "test.mul",
                PolicyStage::PathQuality,
                Predicate::ALWAYS,
                mul_rule,
            ),
        ];
        let mut program = PolicyProgram::with_trace(1.0);
        apply_score_rules(&mut program, &ctx, rules);
        let evaluation = program.finish();
        let trace = evaluation.trace.expect("trace");
        assert_eq!(evaluation.score, 9.0);
        assert_eq!(trace.rules[0].predicate_ids, vec!["test.enabled"]);
        assert!(trace.rules[1].predicate_ids.is_empty());
    }

    #[test]
    fn policy_rules_predicates_are_deterministic() {
        let predicate = Predicate::all(TEST_GATE_PREDICATES);
        assert!(predicate_matches(
            &TestCtx {
                enabled: true,
                multiplier: 1.0,
            },
            predicate,
        ));
        assert!(!predicate_matches(
            &TestCtx {
                enabled: false,
                multiplier: 1.0,
            },
            predicate,
        ));
    }
}
