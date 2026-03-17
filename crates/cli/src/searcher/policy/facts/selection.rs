use super::super::super::HybridRankedEvidence;
use super::super::super::intent::HybridRankingIntent;
use super::super::super::laravel::LaravelUiSurfaceClass;
use super::super::super::surfaces::HybridSourceClass;
use super::super::hybrid_path_quality_multiplier_with_intent;
use super::shared::{SelectionCoverageState, SharedExcerptQueryMatch, SharedPathQueryMatch};
use super::{PolicyQueryContext, SharedIntentFacts, SharedPathFacts};

pub(crate) struct SelectionStaticFeatures {
    pub(crate) class: HybridSourceClass,
    pub(crate) path_depth: usize,
    pub(crate) path_match: SharedPathQueryMatch,
    pub(crate) excerpt_match: SharedExcerptQueryMatch,
    pub(crate) runtime_witness_path_overlap_multiplier: f32,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_example_support: bool,
    pub(crate) has_path_witness_source: bool,
    pub(crate) path_witness_subtree_affinity: usize,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
}

pub(crate) struct SelectionCandidate {
    pub(crate) evidence: HybridRankedEvidence,
    shared_path: SharedPathFacts,
    pub(crate) static_features: SelectionStaticFeatures,
}

impl SelectionCandidate {
    pub(crate) fn new(
        evidence: HybridRankedEvidence,
        intent: &HybridRankingIntent,
        query_context: &PolicyQueryContext,
    ) -> Self {
        let shared_intent = SharedIntentFacts::from_intent(intent);
        let shared_path = SharedPathFacts::from_path(&evidence.document.path);
        let path_match = query_context.match_path(&evidence.document.path);
        let excerpt_match = query_context.match_excerpt(&evidence.excerpt, &shared_intent);
        let class = shared_path.class;
        let path_overlap = path_match.path_overlap;
        let runtime_witness_path_overlap_multiplier = if shared_intent.wants_runtime_witnesses {
            shared_path.runtime_witness_path_overlap_multiplier(path_overlap)
        } else {
            1.0
        };
        let path_witness_paths = evidence
            .lexical_sources
            .iter()
            .filter_map(|source| parse_path_witness_source_path(source))
            .collect::<Vec<_>>();
        let has_path_witness_source = !path_witness_paths.is_empty();
        let normalized_candidate_path = evidence.document.path.trim_start_matches("./");
        let path_witness_subtree_affinity = path_witness_paths
            .iter()
            .map(|source_path| {
                let affinity = SharedPathFacts::workspace_subtree_affinity(
                    &evidence.document.path,
                    source_path,
                );
                if source_path.trim_start_matches("./") == normalized_candidate_path {
                    0
                } else {
                    affinity
                }
            })
            .max()
            .unwrap_or(0);
        let path_depth = shared_path.path_depth;
        let is_ci_workflow = shared_path.is_ci_workflow;
        let is_example_support = shared_path.is_example_support;
        let is_repo_root_runtime_config_artifact = shared_path.is_repo_root_runtime_config_artifact;
        let is_typescript_runtime_module_index = shared_path.is_typescript_runtime_module_index;

        Self {
            evidence,
            shared_path,
            static_features: SelectionStaticFeatures {
                class,
                path_depth,
                path_match,
                excerpt_match,
                runtime_witness_path_overlap_multiplier,
                is_ci_workflow,
                is_example_support,
                has_path_witness_source,
                path_witness_subtree_affinity,
                is_repo_root_runtime_config_artifact,
                is_typescript_runtime_module_index,
            },
        }
    }
}

fn parse_path_witness_source_path(source: &str) -> Option<String> {
    let raw = source.strip_prefix("path_witness:")?;
    let mut parts = raw.rsplitn(3, ':');
    let column = parts.next()?;
    let line = parts.next()?;
    let path = parts.next()?;
    if line.parse::<usize>().is_err() || column.parse::<usize>().is_err() {
        return None;
    }
    Some(path.to_owned())
}

#[derive(Debug, Default)]
pub(crate) struct SelectionState {
    coverage: SelectionCoverageState,
}

impl SelectionState {
    pub(crate) fn from_selected(
        selected: &[HybridRankedEvidence],
        intent: &HybridRankingIntent,
        query_context: &PolicyQueryContext,
    ) -> Self {
        let mut state = Self::default();
        for evidence in selected {
            let candidate = SelectionCandidate::new(evidence.clone(), intent, query_context);
            state.observe(&candidate);
        }
        state
    }

    pub(crate) fn observe(&mut self, candidate: &SelectionCandidate) {
        self.coverage.observe(
            &candidate.evidence.document.path,
            &candidate.shared_path,
            candidate.static_features.class,
            candidate.static_features.is_ci_workflow,
            candidate.static_features.is_example_support,
            candidate
                .static_features
                .is_repo_root_runtime_config_artifact,
            candidate.static_features.is_typescript_runtime_module_index,
        );
    }
}

pub(crate) struct SelectionFacts {
    pub(crate) base_score: f32,
    pub(crate) class: HybridSourceClass,
    pub(crate) path_depth: usize,
    pub(crate) path_overlap: usize,
    pub(crate) excerpt_overlap: usize,
    pub(crate) blade_specific_path_overlap: usize,
    pub(crate) specific_witness_path_overlap: usize,
    pub(crate) runtime_family_prefix_overlap: usize,
    pub(crate) seen_count: usize,
    pub(crate) runtime_seen: usize,
    pub(crate) seen_ci_workflows: usize,
    pub(crate) seen_example_support: usize,
    pub(crate) seen_bench_support: usize,
    pub(crate) seen_plain_test_support: usize,
    pub(crate) seen_repo_root_runtime_configs: usize,
    pub(crate) seen_typescript_runtime_module_indexes: usize,
    pub(crate) canonical_match_multiplier: f32,
    pub(crate) runtime_witness_path_overlap_multiplier: f32,
    pub(crate) excerpt_has_exact_identifier_anchor: bool,
    pub(crate) excerpt_has_build_flow_anchor: bool,
    pub(crate) excerpt_has_test_double_anchor: bool,
    pub(crate) has_exact_query_term_match: bool,
    pub(crate) has_path_witness_source: bool,
    pub(crate) is_examples_rs: bool,
    pub(crate) query_has_exact_terms: bool,
    pub(crate) query_mentions_cli: bool,
    pub(crate) query_has_identifier_anchor: bool,
    pub(crate) query_has_specific_blade_anchors: bool,
    pub(crate) wants_runtime_companion_tests: bool,
    pub(crate) prefer_runtime_anchor_tests: bool,
    pub(crate) wants_language_locality_bias: bool,
    pub(crate) wants_example_or_bench_witnesses: bool,
    pub(crate) penalize_generic_runtime_docs: bool,
    pub(crate) wants_python_witnesses: bool,
    pub(crate) wants_rust_workspace_config: bool,
    pub(crate) wants_python_workspace_config: bool,
    pub(crate) wants_contracts: bool,
    pub(crate) wants_error_taxonomy: bool,
    pub(crate) wants_tool_contracts: bool,
    pub(crate) wants_mcp_runtime_surface: bool,
    pub(crate) wants_class: bool,
    pub(crate) wants_runtime_witnesses: bool,
    pub(crate) wants_runtime_config_artifacts: bool,
    pub(crate) wants_laravel_ui_witnesses: bool,
    pub(crate) wants_blade_component_witnesses: bool,
    pub(crate) wants_laravel_form_action_witnesses: bool,
    pub(crate) wants_livewire_view_witnesses: bool,
    pub(crate) wants_commands_middleware_witnesses: bool,
    pub(crate) wants_jobs_listeners_witnesses: bool,
    pub(crate) wants_laravel_layout_witnesses: bool,
    pub(crate) wants_test_witness_recall: bool,
    pub(crate) wants_navigation_fallbacks: bool,
    pub(crate) wants_ci_workflow_witnesses: bool,
    pub(crate) wants_scripts_ops_witnesses: bool,
    pub(crate) wants_entrypoint_build_flow: bool,
    pub(crate) wants_examples: bool,
    pub(crate) wants_benchmarks: bool,
    pub(crate) candidate_language_known: bool,
    pub(crate) matches_query_language: bool,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_example_support: bool,
    pub(crate) is_runtime_config_artifact: bool,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
    pub(crate) is_entrypoint_runtime: bool,
    pub(crate) is_entrypoint_build_workflow: bool,
    pub(crate) is_entrypoint_reference_doc: bool,
    pub(crate) is_python_entrypoint_runtime: bool,
    pub(crate) is_python_runtime_config: bool,
    pub(crate) is_python_test_witness: bool,
    pub(crate) is_loose_python_test_module: bool,
    pub(crate) is_bench_support: bool,
    pub(crate) is_test_support: bool,
    pub(crate) is_cli_test_support: bool,
    pub(crate) is_runtime_anchor_test_support: bool,
    pub(crate) is_runtime_adjacent_python_test: bool,
    pub(crate) is_non_prefix_python_test_module: bool,
    pub(crate) is_test_harness: bool,
    pub(crate) is_non_code_test_doc: bool,
    pub(crate) is_generic_runtime_witness_doc: bool,
    pub(crate) is_repo_metadata: bool,
    pub(crate) has_generic_runtime_anchor_stem: bool,
    pub(crate) is_frontend_runtime_noise: bool,
    pub(crate) is_navigation_runtime: bool,
    pub(crate) is_navigation_reference_doc: bool,
    pub(crate) is_scripts_ops: bool,
    pub(crate) is_rust_workspace_config: bool,
    pub(crate) path_stem_is_server_or_cli: bool,
    pub(crate) path_stem_is_main: bool,
    pub(crate) is_laravel_non_livewire_blade_view: bool,
    pub(crate) is_laravel_livewire_view: bool,
    pub(crate) is_laravel_blade_component: bool,
    pub(crate) is_laravel_nested_blade_component: bool,
    pub(crate) is_laravel_form_action_blade: bool,
    pub(crate) is_laravel_livewire_component: bool,
    pub(crate) is_laravel_view_component_class: bool,
    pub(crate) is_laravel_command_or_middleware: bool,
    pub(crate) is_laravel_job_or_listener: bool,
    pub(crate) is_laravel_layout_blade_view: bool,
    pub(crate) is_laravel_core_provider: bool,
    pub(crate) is_laravel_provider: bool,
    pub(crate) is_laravel_route: bool,
    pub(crate) is_laravel_bootstrap_entrypoint: bool,
    pub(crate) laravel_surface: Option<LaravelUiSurfaceClass>,
    pub(crate) laravel_surface_seen: usize,
    pub(crate) runtime_subtree_affinity: usize,
}

impl SelectionFacts {
    pub(crate) fn from_candidate(
        candidate: &SelectionCandidate,
        intent: &HybridRankingIntent,
        query_context: &PolicyQueryContext,
        state: &SelectionState,
    ) -> Self {
        let shared_intent = SharedIntentFacts::from_intent(intent);
        let shared_path = &candidate.shared_path;
        let evidence = &candidate.evidence;
        let class = candidate.static_features.class;
        let laravel_surface = shared_path.laravel_surface;
        let coverage = state
            .coverage
            .snapshot_for(&evidence.document.path, class, laravel_surface);

        Self {
            base_score: evidence.blended_score
                * hybrid_path_quality_multiplier_with_intent(&evidence.document.path, intent),
            class,
            path_depth: candidate.static_features.path_depth,
            path_overlap: candidate.static_features.path_match.path_overlap,
            excerpt_overlap: candidate.static_features.excerpt_match.excerpt_overlap,
            blade_specific_path_overlap: candidate
                .static_features
                .path_match
                .blade_specific_path_overlap,
            specific_witness_path_overlap: candidate
                .static_features
                .path_match
                .specific_witness_path_overlap,
            runtime_family_prefix_overlap: coverage.runtime_family_prefix_overlap,
            seen_count: coverage.seen_count,
            runtime_seen: coverage.runtime_seen,
            seen_ci_workflows: coverage.seen_ci_workflows,
            seen_example_support: coverage.seen_example_support,
            seen_bench_support: coverage.seen_bench_support,
            seen_plain_test_support: coverage.seen_plain_test_support,
            seen_repo_root_runtime_configs: coverage.seen_repo_root_runtime_configs,
            seen_typescript_runtime_module_indexes: coverage.seen_typescript_runtime_module_indexes,
            canonical_match_multiplier: candidate
                .static_features
                .path_match
                .canonical_match_multiplier,
            runtime_witness_path_overlap_multiplier: candidate
                .static_features
                .runtime_witness_path_overlap_multiplier,
            excerpt_has_exact_identifier_anchor: candidate
                .static_features
                .excerpt_match
                .excerpt_has_exact_identifier_anchor,
            excerpt_has_build_flow_anchor: candidate
                .static_features
                .excerpt_match
                .excerpt_has_build_flow_anchor,
            excerpt_has_test_double_anchor: candidate
                .static_features
                .excerpt_match
                .excerpt_has_test_double_anchor,
            has_exact_query_term_match: candidate
                .static_features
                .path_match
                .has_exact_query_term_match,
            has_path_witness_source: candidate.static_features.has_path_witness_source,
            is_examples_rs: shared_path.is_examples_rs,
            query_has_exact_terms: query_context.has_exact_terms(),
            query_mentions_cli: query_context.query_mentions_cli,
            query_has_identifier_anchor: query_context.query_has_identifier_anchor,
            query_has_specific_blade_anchors: query_context.query_has_specific_blade_anchors,
            wants_runtime_companion_tests: shared_intent.wants_runtime_companion_tests,
            prefer_runtime_anchor_tests: shared_intent.prefer_runtime_anchor_tests,
            wants_language_locality_bias: shared_intent.wants_language_locality_bias,
            wants_example_or_bench_witnesses: shared_intent.wants_example_or_bench_witnesses,
            penalize_generic_runtime_docs: shared_intent.penalize_generic_runtime_docs,
            wants_python_witnesses: shared_intent.wants_python_witnesses,
            wants_rust_workspace_config: shared_intent.wants_rust_workspace_config,
            wants_python_workspace_config: shared_intent.wants_python_workspace_config,
            wants_contracts: shared_intent.wants_contracts,
            wants_error_taxonomy: shared_intent.wants_error_taxonomy,
            wants_tool_contracts: shared_intent.wants_tool_contracts,
            wants_mcp_runtime_surface: shared_intent.wants_mcp_runtime_surface,
            wants_class: intent.wants_class(class),
            wants_runtime_witnesses: shared_intent.wants_runtime_witnesses,
            wants_runtime_config_artifacts: shared_intent.wants_runtime_config_artifacts,
            wants_laravel_ui_witnesses: shared_intent.wants_laravel_ui_witnesses,
            wants_blade_component_witnesses: shared_intent.wants_blade_component_witnesses,
            wants_laravel_form_action_witnesses: shared_intent.wants_laravel_form_action_witnesses,
            wants_livewire_view_witnesses: shared_intent.wants_livewire_view_witnesses,
            wants_commands_middleware_witnesses: shared_intent.wants_commands_middleware_witnesses,
            wants_jobs_listeners_witnesses: shared_intent.wants_jobs_listeners_witnesses,
            wants_laravel_layout_witnesses: shared_intent.wants_laravel_layout_witnesses,
            wants_test_witness_recall: shared_intent.wants_test_witness_recall,
            wants_navigation_fallbacks: shared_intent.wants_navigation_fallbacks,
            wants_ci_workflow_witnesses: shared_intent.wants_ci_workflow_witnesses,
            wants_scripts_ops_witnesses: shared_intent.wants_scripts_ops_witnesses,
            wants_entrypoint_build_flow: shared_intent.wants_entrypoint_build_flow,
            wants_examples: shared_intent.wants_examples,
            wants_benchmarks: shared_intent.wants_benchmarks,
            candidate_language_known: shared_path.language.is_some(),
            matches_query_language: shared_path.matches_query_language(intent),
            is_ci_workflow: candidate.static_features.is_ci_workflow,
            is_example_support: shared_path.is_example_support,
            is_runtime_config_artifact: shared_path.is_runtime_config_artifact,
            is_repo_root_runtime_config_artifact: shared_path.is_repo_root_runtime_config_artifact,
            is_typescript_runtime_module_index: candidate
                .static_features
                .is_typescript_runtime_module_index,
            is_entrypoint_runtime: shared_path.is_entrypoint_runtime,
            is_entrypoint_build_workflow: shared_path.is_entrypoint_build_workflow,
            is_entrypoint_reference_doc: shared_path.is_entrypoint_reference_doc,
            is_python_entrypoint_runtime: shared_path.is_python_entrypoint_runtime,
            is_python_runtime_config: shared_path.is_python_runtime_config,
            is_python_test_witness: shared_path.is_python_test_witness,
            is_loose_python_test_module: shared_path.is_loose_python_test_module,
            is_bench_support: shared_path.is_bench_support,
            is_test_support: shared_path.is_test_support,
            is_cli_test_support: shared_path.is_cli_test_support,
            is_runtime_anchor_test_support: shared_path.is_runtime_anchor_test_support,
            is_runtime_adjacent_python_test: shared_path.is_runtime_adjacent_python_test,
            is_non_prefix_python_test_module: shared_path.is_non_prefix_python_test_module,
            is_test_harness: shared_path.is_test_harness,
            is_non_code_test_doc: shared_path.is_non_code_test_doc,
            is_generic_runtime_witness_doc: shared_path.is_generic_runtime_witness_doc,
            is_repo_metadata: shared_path.is_repo_metadata,
            has_generic_runtime_anchor_stem: shared_path.has_generic_runtime_anchor_stem,
            is_frontend_runtime_noise: shared_path.effective_frontend_runtime_noise(&shared_intent),
            is_navigation_runtime: shared_path.is_navigation_runtime,
            is_navigation_reference_doc: shared_path.is_navigation_reference_doc,
            is_scripts_ops: shared_path.is_scripts_ops,
            is_rust_workspace_config: shared_path.is_rust_workspace_config,
            path_stem_is_server_or_cli: shared_path.path_stem_is_server_or_cli,
            path_stem_is_main: shared_path.path_stem_is_main,
            is_laravel_non_livewire_blade_view: shared_path.is_laravel_non_livewire_blade_view,
            is_laravel_livewire_view: shared_path.is_laravel_livewire_view,
            is_laravel_blade_component: shared_path.is_laravel_blade_component,
            is_laravel_nested_blade_component: shared_path.is_laravel_nested_blade_component,
            is_laravel_form_action_blade: shared_path.is_laravel_form_action_blade,
            is_laravel_livewire_component: shared_path.is_laravel_livewire_component,
            is_laravel_view_component_class: shared_path.is_laravel_view_component_class,
            is_laravel_command_or_middleware: shared_path.is_laravel_command_or_middleware,
            is_laravel_job_or_listener: shared_path.is_laravel_job_or_listener,
            is_laravel_layout_blade_view: shared_path.is_laravel_layout_blade_view,
            is_laravel_core_provider: shared_path.is_laravel_core_provider,
            is_laravel_provider: shared_path.is_laravel_provider,
            is_laravel_route: shared_path.is_laravel_route,
            is_laravel_bootstrap_entrypoint: shared_path.is_laravel_bootstrap_entrypoint,
            laravel_surface,
            laravel_surface_seen: coverage.laravel_surface_seen,
            runtime_subtree_affinity: coverage
                .runtime_subtree_affinity
                .max(candidate.static_features.path_witness_subtree_affinity),
        }
    }
}

#[cfg(test)]
impl Default for SelectionFacts {
    fn default() -> Self {
        Self {
            base_score: 1.0,
            class: HybridSourceClass::Other,
            path_depth: 0,
            path_overlap: 0,
            excerpt_overlap: 0,
            blade_specific_path_overlap: 0,
            specific_witness_path_overlap: 0,
            runtime_family_prefix_overlap: 0,
            seen_count: 0,
            runtime_seen: 0,
            seen_ci_workflows: 0,
            seen_example_support: 0,
            seen_bench_support: 0,
            seen_plain_test_support: 0,
            seen_repo_root_runtime_configs: 0,
            seen_typescript_runtime_module_indexes: 0,
            canonical_match_multiplier: 1.0,
            runtime_witness_path_overlap_multiplier: 1.0,
            excerpt_has_exact_identifier_anchor: false,
            excerpt_has_build_flow_anchor: false,
            excerpt_has_test_double_anchor: false,
            has_exact_query_term_match: false,
            has_path_witness_source: false,
            is_examples_rs: false,
            query_has_exact_terms: false,
            query_mentions_cli: false,
            query_has_identifier_anchor: false,
            query_has_specific_blade_anchors: false,
            wants_runtime_companion_tests: false,
            prefer_runtime_anchor_tests: false,
            wants_language_locality_bias: false,
            wants_example_or_bench_witnesses: false,
            penalize_generic_runtime_docs: false,
            wants_python_witnesses: false,
            wants_rust_workspace_config: false,
            wants_python_workspace_config: false,
            wants_contracts: false,
            wants_error_taxonomy: false,
            wants_tool_contracts: false,
            wants_mcp_runtime_surface: false,
            wants_class: false,
            wants_runtime_witnesses: false,
            wants_runtime_config_artifacts: false,
            wants_laravel_ui_witnesses: false,
            wants_blade_component_witnesses: false,
            wants_laravel_form_action_witnesses: false,
            wants_livewire_view_witnesses: false,
            wants_commands_middleware_witnesses: false,
            wants_jobs_listeners_witnesses: false,
            wants_laravel_layout_witnesses: false,
            wants_test_witness_recall: false,
            wants_navigation_fallbacks: false,
            wants_ci_workflow_witnesses: false,
            wants_scripts_ops_witnesses: false,
            wants_entrypoint_build_flow: false,
            wants_examples: false,
            wants_benchmarks: false,
            candidate_language_known: false,
            matches_query_language: false,
            is_ci_workflow: false,
            is_example_support: false,
            is_runtime_config_artifact: false,
            is_repo_root_runtime_config_artifact: false,
            is_typescript_runtime_module_index: false,
            is_entrypoint_runtime: false,
            is_entrypoint_build_workflow: false,
            is_entrypoint_reference_doc: false,
            is_python_entrypoint_runtime: false,
            is_python_runtime_config: false,
            is_python_test_witness: false,
            is_loose_python_test_module: false,
            is_bench_support: false,
            is_test_support: false,
            is_cli_test_support: false,
            is_runtime_anchor_test_support: false,
            is_runtime_adjacent_python_test: false,
            is_non_prefix_python_test_module: false,
            is_test_harness: false,
            is_non_code_test_doc: false,
            is_generic_runtime_witness_doc: false,
            is_repo_metadata: false,
            has_generic_runtime_anchor_stem: false,
            is_frontend_runtime_noise: false,
            is_navigation_runtime: false,
            is_navigation_reference_doc: false,
            is_scripts_ops: false,
            is_rust_workspace_config: false,
            path_stem_is_server_or_cli: false,
            path_stem_is_main: false,
            is_laravel_non_livewire_blade_view: false,
            is_laravel_livewire_view: false,
            is_laravel_blade_component: false,
            is_laravel_nested_blade_component: false,
            is_laravel_form_action_blade: false,
            is_laravel_livewire_component: false,
            is_laravel_view_component_class: false,
            is_laravel_command_or_middleware: false,
            is_laravel_job_or_listener: false,
            is_laravel_layout_blade_view: false,
            is_laravel_core_provider: false,
            is_laravel_provider: false,
            is_laravel_route: false,
            is_laravel_bootstrap_entrypoint: false,
            laravel_surface: None,
            laravel_surface_seen: 0,
            runtime_subtree_affinity: 0,
        }
    }
}
