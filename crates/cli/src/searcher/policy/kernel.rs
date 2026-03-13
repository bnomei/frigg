use super::trace::{PolicyEffect, PolicyEvaluation, PolicyRuleTrace, PolicyStage, PolicyTrace};

pub(crate) struct PolicyProgram {
    score: f32,
    trace: Option<Vec<PolicyRuleTrace>>,
}

impl PolicyProgram {
    pub(crate) fn new(score: f32) -> Self {
        Self { score, trace: None }
    }

    pub(crate) fn with_trace(score: f32) -> Self {
        Self {
            score,
            trace: Some(Vec::new()),
        }
    }

    pub(crate) fn with_optional_trace(score: f32, enabled: bool) -> Self {
        if enabled {
            Self::with_trace(score)
        } else {
            Self::new(score)
        }
    }

    pub(crate) fn apply_effect(
        &mut self,
        rule_id: &'static str,
        stage: PolicyStage,
        predicate_ids: &[&'static str],
        effect: PolicyEffect,
    ) {
        let before = self.score;
        match effect {
            PolicyEffect::Add(delta) => self.score += delta,
            PolicyEffect::Multiply(multiplier) => self.score *= multiplier,
        }
        if let Some(trace) = &mut self.trace {
            trace.push(PolicyRuleTrace {
                rule_id,
                stage,
                predicate_ids: predicate_ids.to_vec(),
                effect,
                before,
                after: self.score,
            });
        }
    }

    pub(crate) fn finish(self) -> PolicyEvaluation {
        PolicyEvaluation {
            score: self.score,
            trace: self.trace.map(|rules| PolicyTrace { rules }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_trace_records_add_and_mul_effects() {
        let mut program = PolicyProgram::with_trace(2.0);
        program.apply_effect(
            "test.add",
            PolicyStage::PathQuality,
            &[],
            PolicyEffect::Add(3.0),
        );
        program.apply_effect(
            "test.mul",
            PolicyStage::PathQuality,
            &[],
            PolicyEffect::Multiply(2.0),
        );
        let evaluation = program.finish();
        let trace = evaluation.trace.expect("trace");
        assert_eq!(evaluation.score, 10.0);
        assert_eq!(trace.rules.len(), 2);
        assert_eq!(trace.rules[0].rule_id, "test.add");
        assert!(trace.rules[0].predicate_ids.is_empty());
        assert_eq!(trace.rules[1].rule_id, "test.mul");
    }
}
