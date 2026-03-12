use super::super::intent::HybridRankingIntent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PathWitnessFrontierPlan {
    pub(crate) top_k: usize,
    pub(crate) materialized_limit: usize,
}

pub(crate) fn plan_path_witness_frontier(
    intent: &HybridRankingIntent,
    limit: usize,
) -> PathWitnessFrontierPlan {
    let widen_runtime_config_witness_pool = intent.wants_runtime_config_artifacts;
    let widen_surface_witness_pool = intent.wants_laravel_ui_witnesses
        || intent.wants_test_witness_recall
        || intent.wants_runtime_config_artifacts
        || intent.wants_entrypoint_build_flow;
    let top_k = if widen_runtime_config_witness_pool
        && (intent.wants_test_witness_recall || intent.wants_entrypoint_build_flow)
    {
        limit.saturating_mul(12).max(128)
    } else if widen_runtime_config_witness_pool {
        limit.saturating_mul(8).max(80)
    } else if intent.wants_test_witness_recall || intent.wants_entrypoint_build_flow {
        limit.saturating_mul(10).max(96)
    } else if widen_surface_witness_pool {
        limit.saturating_mul(6).max(64)
    } else {
        limit.saturating_mul(2).max(16)
    };
    let materialized_limit = if widen_runtime_config_witness_pool || widen_surface_witness_pool {
        // Surface-heavy queries rely on downstream selection and guardrail repair passes.
        top_k
    } else {
        limit.saturating_add(2).max(8).min(top_k)
    };

    PathWitnessFrontierPlan {
        top_k,
        materialized_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_witness_frontier_expands_for_runtime_config_and_test_queries() {
        let mut intent = HybridRankingIntent::default();
        intent.wants_runtime_config_artifacts = true;
        intent.wants_test_witness_recall = true;
        let plan = plan_path_witness_frontier(&intent, 5);

        assert_eq!(plan.top_k, 128);
        assert_eq!(plan.materialized_limit, 128);
    }

    #[test]
    fn path_witness_frontier_materializes_guardrail_pool_for_surface_queries() {
        let mut intent = HybridRankingIntent::default();
        intent.wants_laravel_ui_witnesses = true;
        let plan = plan_path_witness_frontier(&intent, 6);

        assert_eq!(plan.top_k, 64);
        assert_eq!(plan.materialized_limit, 64);
    }

    #[test]
    fn path_witness_frontier_stays_tight_for_plain_queries() {
        let intent = HybridRankingIntent::default();
        let plan = plan_path_witness_frontier(&intent, 5);

        assert_eq!(plan.top_k, 16);
        assert_eq!(plan.materialized_limit, 8);
    }
}
