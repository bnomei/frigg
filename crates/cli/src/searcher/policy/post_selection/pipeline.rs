use super::super::super::HybridRankedEvidence;
use super::super::super::query_terms::hybrid_query_mentions_cli_command;
use super::super::dsl::PredicateLeaf;
use super::PostSelectionContext;
use super::PostSelectionRuleMeta;

pub(super) type TransformFn = for<'a> fn(
    Vec<HybridRankedEvidence>,
    &PostSelectionContext<'a>,
    PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence>;

#[derive(Clone, Copy)]
pub(super) struct PostSelectionPipelineFacts {
    wants_runtime_config_artifacts: bool,
    wants_entrypoint_build_flow: bool,
    wants_test_witness_recall: bool,
    wants_examples: bool,
    wants_benchmarks: bool,
    wants_laravel_ui_witnesses: bool,
    wants_ci_workflow_witnesses: bool,
    wants_scripts_ops_witnesses: bool,
    query_mentions_cli: bool,
    has_specific_witness_terms: bool,
}

impl PostSelectionPipelineFacts {
    pub(super) fn from_context(ctx: &PostSelectionContext<'_>) -> Self {
        Self {
            wants_runtime_config_artifacts: ctx.intent.wants_runtime_config_artifacts,
            wants_entrypoint_build_flow: ctx.intent.wants_entrypoint_build_flow,
            wants_test_witness_recall: ctx.intent.wants_test_witness_recall,
            wants_examples: ctx.intent.wants_examples,
            wants_benchmarks: ctx.intent.wants_benchmarks,
            wants_laravel_ui_witnesses: ctx.intent.wants_laravel_ui_witnesses,
            wants_ci_workflow_witnesses: ctx.intent.wants_ci_workflow_witnesses,
            wants_scripts_ops_witnesses: ctx.intent.wants_scripts_ops_witnesses,
            query_mentions_cli: hybrid_query_mentions_cli_command(ctx.query_text),
            has_specific_witness_terms: !ctx
                .selection_query_context
                .specific_witness_terms
                .is_empty(),
        }
    }
}

fn wants_runtime_config_artifacts(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_runtime_config_artifacts
}

fn wants_entrypoint_build_flow(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_entrypoint_build_flow
}

fn wants_test_witness_recall(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_test_witness_recall
}

fn wants_examples(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_examples
}

fn wants_benchmarks(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_benchmarks
}

fn wants_laravel_ui_witnesses(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_laravel_ui_witnesses
}

fn wants_ci_workflow_witnesses(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_ci_workflow_witnesses
}

fn wants_scripts_ops_witnesses(facts: &PostSelectionPipelineFacts) -> bool {
    facts.wants_scripts_ops_witnesses
}

fn query_mentions_cli(facts: &PostSelectionPipelineFacts) -> bool {
    facts.query_mentions_cli
}

fn has_specific_witness_terms(facts: &PostSelectionPipelineFacts) -> bool {
    facts.has_specific_witness_terms
}

pub(super) const WANTS_RUNTIME_CONFIG_ARTIFACTS: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new(
        "intent.runtime_config_artifacts",
        wants_runtime_config_artifacts,
    );
pub(super) const WANTS_ENTRYPOINT_BUILD_FLOW: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.entrypoint_build_flow", wants_entrypoint_build_flow);
pub(super) const WANTS_TEST_WITNESS_RECALL: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.test_witness_recall", wants_test_witness_recall);
pub(super) const WANTS_EXAMPLES: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.examples", wants_examples);
pub(super) const WANTS_BENCHMARKS: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.benchmarks", wants_benchmarks);
pub(super) const WANTS_LARAVEL_UI_WITNESSES: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.laravel_ui_witnesses", wants_laravel_ui_witnesses);
pub(super) const WANTS_CI_WORKFLOW_WITNESSES: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.ci_workflow_witnesses", wants_ci_workflow_witnesses);
pub(super) const WANTS_SCRIPTS_OPS_WITNESSES: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("intent.scripts_ops_witnesses", wants_scripts_ops_witnesses);
pub(super) const QUERY_MENTIONS_CLI: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new("query.mentions_cli", query_mentions_cli);
pub(super) const HAS_SPECIFIC_WITNESS_TERMS: PredicateLeaf<PostSelectionPipelineFacts> =
    PredicateLeaf::new(
        "query.has_specific_witness_terms",
        has_specific_witness_terms,
    );
