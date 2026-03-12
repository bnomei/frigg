use super::kernel::PolicyProgram;
use super::trace::{PolicyEffect, PolicyStage};

pub(crate) type GateFn<Ctx> = fn(&Ctx) -> bool;
pub(crate) type RuleFn<Ctx> = fn(&Ctx) -> Option<PolicyEffect>;

#[derive(Clone, Copy)]
pub(crate) struct GateRule<Ctx> {
    #[allow(dead_code)]
    pub id: &'static str,
    pub when: GateFn<Ctx>,
}

impl<Ctx> GateRule<Ctx> {
    pub(crate) const fn new(id: &'static str, when: GateFn<Ctx>) -> Self {
        Self { id, when }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScoreRule<Ctx> {
    pub id: &'static str,
    pub stage: PolicyStage,
    pub eval: RuleFn<Ctx>,
}

impl<Ctx> ScoreRule<Ctx> {
    pub(crate) const fn new(id: &'static str, stage: PolicyStage, eval: RuleFn<Ctx>) -> Self {
        Self { id, stage, eval }
    }
}

pub(crate) fn any_gate_matches<Ctx>(ctx: &Ctx, rules: &[GateRule<Ctx>]) -> bool {
    rules.iter().any(|rule| (rule.when)(ctx))
}

pub(crate) fn apply_score_rules<Ctx>(
    program: &mut PolicyProgram,
    ctx: &Ctx,
    rules: &[ScoreRule<Ctx>],
) {
    for rule in rules {
        if let Some(effect) = (rule.eval)(ctx) {
            program.apply_effect(rule.id, rule.stage, effect);
        }
    }
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
            ScoreRule::new("test.add", PolicyStage::PathQuality, add_rule),
            ScoreRule::new("test.mul", PolicyStage::PathQuality, mul_rule),
        ];
        let mut program = PolicyProgram::new(1.0);
        apply_score_rules(&mut program, &ctx, rules);
        assert_eq!(program.finish().score, 9.0);
    }

    #[test]
    fn policy_rules_gate_sets_are_deterministic() {
        let rules = &[GateRule::new("test.gate", gate)];
        assert!(any_gate_matches(
            &TestCtx {
                enabled: true,
                multiplier: 1.0,
            },
            rules,
        ));
        assert!(!any_gate_matches(
            &TestCtx {
                enabled: false,
                multiplier: 1.0,
            },
            rules,
        ));
    }
}
