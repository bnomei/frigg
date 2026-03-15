use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::FrameworkHint;

use super::super::super::intent::HybridRankingIntent;
use super::super::super::laravel::{LaravelUiSurfaceClass, laravel_ui_surface_class};
use super::super::super::query_terms::{
    hybrid_canonical_match_multiplier, hybrid_excerpt_has_build_flow_anchor,
    hybrid_excerpt_has_exact_identifier_anchor, hybrid_excerpt_has_test_double_anchor,
    hybrid_identifier_tokens, hybrid_overlap_count, hybrid_path_overlap_count_with_terms,
    hybrid_query_exact_terms, hybrid_query_has_kotlin_android_ui_terms,
    hybrid_query_mentions_cli_command, hybrid_query_overlap_terms,
    hybrid_specific_witness_query_terms, path_has_exact_query_term_match,
};
use super::super::super::surfaces::{
    HybridSourceClass, has_generic_runtime_anchor_stem, hybrid_source_class,
    is_bench_support_path, is_ci_workflow_path, is_cli_test_support_path,
    is_entrypoint_build_workflow_path, is_entrypoint_reference_doc_path,
    is_entrypoint_runtime_path, is_example_support_path, is_frontend_runtime_noise_path,
    is_generic_runtime_witness_doc_path, is_kotlin_android_ui_runtime_surface_path,
    is_loose_python_test_module_path, is_navigation_reference_doc_path,
    is_navigation_runtime_path, is_non_code_test_doc_path, is_python_entrypoint_runtime_path,
    is_python_runtime_config_path, is_python_test_witness_path, is_repo_metadata_path,
    is_root_scoped_runtime_config_path, is_runtime_adjacent_python_test_path,
    is_runtime_anchor_test_support_path, is_runtime_config_artifact_path,
    is_rust_workspace_config_path, is_scripts_ops_path, is_test_harness_path,
    is_test_support_path, is_typescript_runtime_module_index_path,
};
use super::super::super::{
    is_laravel_blade_component_path, is_laravel_bootstrap_entrypoint_path,
    is_laravel_command_or_middleware_path, is_laravel_core_provider_path,
    is_laravel_form_action_blade_path, is_laravel_job_or_listener_path,
    is_laravel_layout_blade_view_path, is_laravel_livewire_component_path,
    is_laravel_livewire_view_path, is_laravel_nested_blade_component_path,
    is_laravel_non_livewire_blade_view_path, is_laravel_provider_path, is_laravel_route_path,
    is_laravel_view_component_class_path,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SharedIntentFacts {
    pub(crate) wants_docs: bool,
    pub(crate) wants_readme: bool,
    pub(crate) wants_onboarding: bool,
    pub(crate) wants_contracts: bool,
    pub(crate) wants_error_taxonomy: bool,
    pub(crate) wants_tool_contracts: bool,
    pub(crate) wants_mcp_runtime_surface: bool,
    pub(crate) wants_examples: bool,
    pub(crate) wants_benchmarks: bool,
    pub(crate) wants_tests: bool,
    pub(crate) wants_fixtures: bool,
    pub(crate) wants_runtime: bool,
    pub(crate) wants_runtime_witnesses: bool,
    pub(crate) wants_runtime_config_artifacts: bool,
    pub(crate) wants_entrypoint_build_flow: bool,
    pub(crate) wants_navigation_fallbacks: bool,
    pub(crate) wants_laravel_ui_witnesses: bool,
    pub(crate) wants_laravel_form_action_witnesses: bool,
    pub(crate) wants_livewire_view_witnesses: bool,
    pub(crate) wants_laravel_layout_witnesses: bool,
    pub(crate) wants_blade_component_witnesses: bool,
    pub(crate) wants_commands_middleware_witnesses: bool,
    pub(crate) wants_jobs_listeners_witnesses: bool,
    pub(crate) wants_ci_workflow_witnesses: bool,
    pub(crate) wants_scripts_ops_witnesses: bool,
    pub(crate) wants_test_witness_recall: bool,
    pub(crate) wants_example_or_bench_witnesses: bool,
    pub(crate) penalize_generic_runtime_docs: bool,
    pub(crate) wants_python_witnesses: bool,
    pub(crate) wants_rust_workspace_config: bool,
    pub(crate) wants_python_workspace_config: bool,
    pub(crate) wants_runtime_companion_tests: bool,
    pub(crate) prefer_runtime_anchor_tests: bool,
}

impl SharedIntentFacts {
    pub(crate) fn from_intent(intent: &HybridRankingIntent) -> Self {
        let wants_example_or_bench_witnesses = intent.wants_example_or_bench_witnesses();
        let penalize_generic_runtime_docs = intent.penalizes_generic_runtime_docs();
        let wants_python_witnesses = intent.has_framework_hint(FrameworkHint::Python);
        let wants_rust_workspace_config = intent.has_framework_hint(FrameworkHint::Rust);
        let wants_python_workspace_config = wants_python_witnesses;
        let wants_runtime_companion_tests = intent.wants_test_witness_recall
            || intent.wants_entrypoint_build_flow
            || intent.wants_runtime_config_artifacts;
        let prefer_runtime_anchor_tests =
            wants_runtime_companion_tests && !intent.wants_test_witness_recall;

        Self {
            wants_docs: intent.wants_docs,
            wants_readme: intent.wants_readme,
            wants_onboarding: intent.wants_onboarding,
            wants_contracts: intent.wants_contracts,
            wants_error_taxonomy: intent.wants_error_taxonomy,
            wants_tool_contracts: intent.wants_tool_contracts,
            wants_mcp_runtime_surface: intent.wants_mcp_runtime_surface,
            wants_examples: intent.wants_examples,
            wants_benchmarks: intent.wants_benchmarks,
            wants_tests: intent.wants_tests,
            wants_fixtures: intent.wants_fixtures,
            wants_runtime: intent.wants_runtime,
            wants_runtime_witnesses: intent.wants_runtime_witnesses,
            wants_runtime_config_artifacts: intent.wants_runtime_config_artifacts,
            wants_entrypoint_build_flow: intent.wants_entrypoint_build_flow,
            wants_navigation_fallbacks: intent.wants_navigation_fallbacks,
            wants_laravel_ui_witnesses: intent.wants_laravel_ui_witnesses,
            wants_laravel_form_action_witnesses: intent.wants_laravel_form_action_witnesses,
            wants_livewire_view_witnesses: intent.wants_livewire_view_witnesses,
            wants_laravel_layout_witnesses: intent.wants_laravel_layout_witnesses,
            wants_blade_component_witnesses: intent.wants_blade_component_witnesses,
            wants_commands_middleware_witnesses: intent.wants_commands_middleware_witnesses,
            wants_jobs_listeners_witnesses: intent.wants_jobs_listeners_witnesses,
            wants_ci_workflow_witnesses: intent.wants_ci_workflow_witnesses,
            wants_scripts_ops_witnesses: intent.wants_scripts_ops_witnesses,
            wants_test_witness_recall: intent.wants_test_witness_recall,
            wants_example_or_bench_witnesses,
            penalize_generic_runtime_docs,
            wants_python_witnesses,
            wants_rust_workspace_config,
            wants_python_workspace_config,
            wants_runtime_companion_tests,
            prefer_runtime_anchor_tests,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PolicyQueryContext {
    pub(crate) exact_terms: Vec<String>,
    pub(crate) query_overlap_terms: Vec<String>,
    pub(crate) specific_witness_terms: Vec<String>,
    pub(crate) blade_component_specific_terms: Vec<String>,
    pub(crate) query_mentions_cli: bool,
    pub(crate) query_has_identifier_anchor: bool,
    pub(crate) query_has_specific_blade_anchors: bool,
    pub(crate) wants_kotlin_android_ui_witnesses: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SharedPathQueryMatch {
    pub(crate) path_overlap: usize,
    pub(crate) specific_witness_path_overlap: usize,
    pub(crate) blade_specific_path_overlap: usize,
    pub(crate) has_exact_query_term_match: bool,
    pub(crate) canonical_match_multiplier: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SharedExcerptQueryMatch {
    pub(crate) excerpt_overlap: usize,
    pub(crate) excerpt_has_exact_identifier_anchor: bool,
    pub(crate) excerpt_has_build_flow_anchor: bool,
    pub(crate) excerpt_has_test_double_anchor: bool,
}

impl PolicyQueryContext {
    pub(crate) fn new(intent: &HybridRankingIntent, query_text: &str) -> Self {
        let shared_intent = SharedIntentFacts::from_intent(intent);
        let exact_terms = hybrid_query_exact_terms(query_text);
        let query_overlap_terms = hybrid_query_overlap_terms(query_text);
        let specific_witness_terms = hybrid_specific_witness_query_terms(query_text);
        let blade_component_specific_terms = blade_component_specific_query_terms(query_text);
        let query_mentions_cli = hybrid_query_mentions_cli_command(query_text);
        let query_has_identifier_anchor = query_overlap_terms.len() > exact_terms.len();
        let query_has_specific_blade_anchors =
            shared_intent.wants_laravel_ui_witnesses && !specific_witness_terms.is_empty();

        Self {
            exact_terms,
            query_overlap_terms,
            specific_witness_terms,
            blade_component_specific_terms,
            query_mentions_cli,
            query_has_identifier_anchor,
            query_has_specific_blade_anchors,
            wants_kotlin_android_ui_witnesses: hybrid_query_has_kotlin_android_ui_terms(query_text),
        }
    }

    pub(crate) fn from_query_text(query_text: &str) -> Self {
        let intent = HybridRankingIntent::from_query(query_text);
        Self::new(&intent, query_text)
    }

    pub(crate) fn has_exact_terms(&self) -> bool {
        !self.exact_terms.is_empty()
    }

    pub(crate) fn has_specific_witness_terms(&self) -> bool {
        !self.specific_witness_terms.is_empty()
    }

    pub(crate) fn match_excerpt(
        &self,
        excerpt: &str,
        intent: &SharedIntentFacts,
    ) -> SharedExcerptQueryMatch {
        SharedExcerptQueryMatch {
            excerpt_overlap: if self.query_has_identifier_anchor
                && (intent.wants_runtime_witnesses || intent.wants_entrypoint_build_flow)
            {
                hybrid_overlap_count(&hybrid_identifier_tokens(excerpt), &self.query_overlap_terms)
            } else {
                0
            },
            excerpt_has_exact_identifier_anchor: self.has_exact_terms()
                && hybrid_excerpt_has_exact_identifier_anchor(
                    excerpt,
                    &self.exact_terms.join(" "),
                ),
            excerpt_has_build_flow_anchor: intent.wants_entrypoint_build_flow
                && hybrid_excerpt_has_build_flow_anchor(excerpt, &self.query_overlap_terms),
            excerpt_has_test_double_anchor: intent.wants_entrypoint_build_flow
                && hybrid_excerpt_has_test_double_anchor(excerpt),
        }
    }

    pub(crate) fn match_projection_path(
        &self,
        path: &str,
        path_terms: &[String],
    ) -> SharedPathQueryMatch {
        SharedPathQueryMatch {
            path_overlap: hybrid_overlap_count(path_terms, &self.query_overlap_terms),
            specific_witness_path_overlap: hybrid_overlap_count(
                path_terms,
                &self.specific_witness_terms,
            ),
            blade_specific_path_overlap: blade_component_specific_path_overlap_count(
                path,
                &self.blade_component_specific_terms,
            ),
            has_exact_query_term_match: path_has_exact_query_term_match(path, &self.exact_terms),
            canonical_match_multiplier: hybrid_canonical_match_multiplier(path, &self.exact_terms),
        }
    }

    pub(crate) fn match_path(&self, path: &str) -> SharedPathQueryMatch {
        SharedPathQueryMatch {
            path_overlap: hybrid_path_overlap_count_with_terms(path, &self.query_overlap_terms),
            specific_witness_path_overlap: hybrid_path_overlap_count_with_terms(
                path,
                &self.specific_witness_terms,
            ),
            blade_specific_path_overlap: blade_component_specific_path_overlap_count(
                path,
                &self.blade_component_specific_terms,
            ),
            has_exact_query_term_match: path_has_exact_query_term_match(path, &self.exact_terms),
            canonical_match_multiplier: hybrid_canonical_match_multiplier(path, &self.exact_terms),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SharedPathFacts {
    pub(crate) class: HybridSourceClass,
    pub(crate) path_depth: usize,
    pub(crate) is_root_readme: bool,
    pub(crate) is_entrypoint_runtime: bool,
    pub(crate) is_entrypoint_build_workflow: bool,
    pub(crate) is_navigation_runtime: bool,
    pub(crate) is_navigation_reference_doc: bool,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
    pub(crate) is_runtime_config_artifact: bool,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_example_support: bool,
    pub(crate) is_bench_support: bool,
    pub(crate) is_test_support: bool,
    pub(crate) is_runtime_anchor_test_support: bool,
    pub(crate) is_generic_runtime_witness_doc: bool,
    pub(crate) is_python_runtime_config: bool,
    pub(crate) is_entrypoint_reference_doc: bool,
    pub(crate) is_repo_metadata: bool,
    pub(crate) is_python_entrypoint_runtime: bool,
    pub(crate) is_python_test_witness: bool,
    pub(crate) is_loose_python_test_module: bool,
    pub(crate) is_runtime_adjacent_python_test: bool,
    pub(crate) is_non_prefix_python_test_module: bool,
    pub(crate) is_cli_test_support: bool,
    pub(crate) is_test_harness: bool,
    pub(crate) is_non_code_test_doc: bool,
    pub(crate) is_scripts_ops: bool,
    pub(crate) is_rust_workspace_config: bool,
    pub(crate) has_generic_runtime_anchor_stem: bool,
    pub(crate) is_frontend_runtime_noise: bool,
    pub(crate) is_kotlin_android_ui_runtime_surface: bool,
    pub(crate) path_stem_is_server_or_cli: bool,
    pub(crate) path_stem_is_main: bool,
    pub(crate) is_examples_rs: bool,
    pub(crate) is_laravel_non_livewire_blade_view: bool,
    pub(crate) is_laravel_livewire_view: bool,
    pub(crate) is_laravel_partial_view: bool,
    pub(crate) is_laravel_top_level_blade_view: bool,
    pub(crate) is_laravel_blade_component: bool,
    pub(crate) is_laravel_nested_blade_component: bool,
    pub(crate) is_laravel_form_action_blade: bool,
    pub(crate) is_laravel_livewire_component: bool,
    pub(crate) is_laravel_view_component_class: bool,
    pub(crate) is_laravel_command_or_middleware: bool,
    pub(crate) is_laravel_job_or_listener: bool,
    pub(crate) is_laravel_layout_blade_view: bool,
    pub(crate) is_laravel_route: bool,
    pub(crate) is_laravel_bootstrap_entrypoint: bool,
    pub(crate) is_laravel_core_provider: bool,
    pub(crate) is_laravel_provider: bool,
    pub(crate) laravel_surface: Option<LaravelUiSurfaceClass>,
}

impl SharedPathFacts {
    pub(crate) fn from_path(path: &str) -> Self {
        let normalized_path = path.trim_start_matches("./");
        let file_name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let path_stem = Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let is_test_support = is_test_support_path(path);
        let is_laravel_non_livewire_blade_view = is_laravel_non_livewire_blade_view_path(path);
        let is_laravel_layout_blade_view = is_laravel_layout_blade_view_path(path);
        let is_laravel_partial_view = path.contains("/parts/") || path.contains("/partials/");

        Self {
            class: hybrid_source_class(path),
            path_depth: path_depth(path),
            is_root_readme: normalized_path.eq_ignore_ascii_case("README.md"),
            is_entrypoint_runtime: is_entrypoint_runtime_path(path),
            is_entrypoint_build_workflow: is_entrypoint_build_workflow_path(path),
            is_navigation_runtime: is_navigation_runtime_path(path),
            is_navigation_reference_doc: is_navigation_reference_doc_path(path),
            is_ci_workflow: is_ci_workflow_path(path),
            is_typescript_runtime_module_index: is_typescript_runtime_module_index_path(path),
            is_runtime_config_artifact: is_runtime_config_artifact_path(path),
            is_repo_root_runtime_config_artifact: is_root_scoped_runtime_config_path(path),
            is_example_support: is_example_support_path(path),
            is_bench_support: is_bench_support_path(path),
            is_test_support,
            is_runtime_anchor_test_support: is_runtime_anchor_test_support_path(path),
            is_generic_runtime_witness_doc: is_generic_runtime_witness_doc_path(path),
            is_python_runtime_config: is_python_runtime_config_path(path),
            is_entrypoint_reference_doc: is_entrypoint_reference_doc_path(path),
            is_repo_metadata: is_repo_metadata_path(path),
            is_python_entrypoint_runtime: is_python_entrypoint_runtime_path(path),
            is_python_test_witness: is_python_test_witness_path(path),
            is_loose_python_test_module: is_loose_python_test_module_path(path),
            is_runtime_adjacent_python_test: is_runtime_adjacent_python_test_path(path),
            is_non_prefix_python_test_module: is_loose_python_test_module_path(path),
            is_cli_test_support: is_cli_test_support_path(path),
            is_test_harness: is_test_harness_path(path),
            is_non_code_test_doc: is_non_code_test_doc_path(path),
            is_scripts_ops: is_scripts_ops_path(path),
            is_rust_workspace_config: is_rust_workspace_config_path(path),
            has_generic_runtime_anchor_stem: has_generic_runtime_anchor_stem(path),
            is_frontend_runtime_noise: is_frontend_runtime_noise_path(path),
            is_kotlin_android_ui_runtime_surface: is_kotlin_android_ui_runtime_surface_path(path),
            path_stem_is_server_or_cli: matches!(path_stem.as_str(), "server" | "cli"),
            path_stem_is_main: path_stem == "main",
            is_examples_rs: is_test_support && file_name.eq_ignore_ascii_case("examples.rs"),
            is_laravel_non_livewire_blade_view,
            is_laravel_livewire_view: is_laravel_livewire_view_path(path),
            is_laravel_partial_view,
            is_laravel_top_level_blade_view: is_laravel_non_livewire_blade_view
                && !is_laravel_layout_blade_view
                && !is_laravel_partial_view,
            is_laravel_blade_component: is_laravel_blade_component_path(path),
            is_laravel_nested_blade_component: is_laravel_nested_blade_component_path(path),
            is_laravel_form_action_blade: is_laravel_form_action_blade_path(path),
            is_laravel_livewire_component: is_laravel_livewire_component_path(path),
            is_laravel_view_component_class: is_laravel_view_component_class_path(path),
            is_laravel_command_or_middleware: is_laravel_command_or_middleware_path(path),
            is_laravel_job_or_listener: is_laravel_job_or_listener_path(path),
            is_laravel_layout_blade_view,
            is_laravel_route: is_laravel_route_path(path),
            is_laravel_bootstrap_entrypoint: is_laravel_bootstrap_entrypoint_path(path),
            is_laravel_core_provider: is_laravel_core_provider_path(path),
            is_laravel_provider: is_laravel_provider_path(path),
            laravel_surface: laravel_ui_surface_class(path),
        }
    }

    pub(crate) fn path_quality_base_multiplier(&self, path: &str) -> f32 {
        if self.class == HybridSourceClass::Other {
            return match Path::new(path).extension().and_then(|ext| ext.to_str()) {
                Some(
                    "rs" | "php" | "go" | "py" | "ts" | "tsx" | "js" | "jsx" | "java"
                    | "kt" | "kts",
                ) => 1.0,
                _ => 0.9,
            };
        }

        match self.class {
            HybridSourceClass::ErrorContracts => 1.0,
            HybridSourceClass::ToolContracts => 1.0,
            HybridSourceClass::BenchmarkDocs => 0.98,
            HybridSourceClass::Documentation => 0.88,
            HybridSourceClass::Readme => 0.78,
            HybridSourceClass::Specs => 0.82,
            HybridSourceClass::Fixtures => 0.92,
            HybridSourceClass::Project => 0.94,
            HybridSourceClass::Support => 0.78,
            HybridSourceClass::Tests => 0.97,
            HybridSourceClass::Runtime => 1.0,
            _ => 0.94,
        }
    }

    pub(crate) fn effective_frontend_runtime_noise(&self, intent: &SharedIntentFacts) -> bool {
        self.is_frontend_runtime_noise
            && !(self.is_repo_root_runtime_config_artifact
                && (intent.wants_entrypoint_build_flow || intent.wants_runtime_config_artifacts))
    }

    pub(crate) fn runtime_witness_path_overlap_multiplier(&self, overlap: usize) -> f32 {
        match self.class {
            HybridSourceClass::Runtime => match overlap {
                0 => 1.0,
                1 => 1.32,
                2 => 1.58,
                _ => 1.84,
            },
            HybridSourceClass::Support | HybridSourceClass::Tests => match overlap {
                0 => 1.0,
                1 => 1.18,
                2 => 1.30,
                _ => 1.42,
            },
            _ => 1.0,
        }
    }

    pub(crate) fn shared_prefix_segments(left: &str, right: &str) -> usize {
        left.trim_start_matches("./")
            .split('/')
            .zip(right.trim_start_matches("./").split('/'))
            .take_while(|(left, right)| left == right)
            .count()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SelectionCoverageSnapshot {
    pub(crate) seen_count: usize,
    pub(crate) runtime_seen: usize,
    pub(crate) seen_ci_workflows: usize,
    pub(crate) seen_example_support: usize,
    pub(crate) seen_bench_support: usize,
    pub(crate) seen_plain_test_support: usize,
    pub(crate) seen_repo_root_runtime_configs: usize,
    pub(crate) seen_typescript_runtime_module_indexes: usize,
    pub(crate) laravel_surface_seen: usize,
    pub(crate) runtime_family_prefix_overlap: usize,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SelectionCoverageState {
    seen_classes: BTreeMap<HybridSourceClass, usize>,
    seen_laravel_ui_surfaces: BTreeMap<LaravelUiSurfaceClass, usize>,
    runtime_anchor_paths: Vec<String>,
    seen_ci_workflows: usize,
    seen_example_support: usize,
    seen_bench_support: usize,
    seen_plain_test_support: usize,
    seen_repo_root_runtime_configs: usize,
    seen_typescript_runtime_module_indexes: usize,
}

impl SelectionCoverageState {
    pub(crate) fn observe(
        &mut self,
        path: &str,
        shared_path: &SharedPathFacts,
        class: HybridSourceClass,
        is_ci_workflow: bool,
        is_example_support: bool,
        is_repo_root_runtime_config_artifact: bool,
        is_typescript_runtime_module_index: bool,
    ) {
        *self.seen_classes.entry(class).or_insert(0) += 1;
        if let Some(surface) = shared_path.laravel_surface {
            *self.seen_laravel_ui_surfaces.entry(surface).or_insert(0) += 1;
        }
        if is_ci_workflow {
            self.seen_ci_workflows += 1;
        }
        if shared_path.is_entrypoint_runtime || shared_path.is_runtime_config_artifact {
            self.runtime_anchor_paths.push(path.to_owned());
        }
        if is_example_support {
            self.seen_example_support += 1;
        }
        if shared_path.is_bench_support {
            self.seen_bench_support += 1;
        }
        if shared_path.is_test_support && !is_example_support && !shared_path.is_bench_support {
            self.seen_plain_test_support += 1;
        }
        if is_repo_root_runtime_config_artifact {
            self.seen_repo_root_runtime_configs += 1;
        }
        if is_typescript_runtime_module_index {
            self.seen_typescript_runtime_module_indexes += 1;
        }
    }

    pub(crate) fn snapshot_for(
        &self,
        path: &str,
        class: HybridSourceClass,
        laravel_surface: Option<LaravelUiSurfaceClass>,
    ) -> SelectionCoverageSnapshot {
        SelectionCoverageSnapshot {
            seen_count: self.seen_classes.get(&class).copied().unwrap_or(0),
            runtime_seen: self
                .seen_classes
                .get(&HybridSourceClass::Runtime)
                .copied()
                .unwrap_or(0),
            seen_ci_workflows: self.seen_ci_workflows,
            seen_example_support: self.seen_example_support,
            seen_bench_support: self.seen_bench_support,
            seen_plain_test_support: self.seen_plain_test_support,
            seen_repo_root_runtime_configs: self.seen_repo_root_runtime_configs,
            seen_typescript_runtime_module_indexes: self.seen_typescript_runtime_module_indexes,
            laravel_surface_seen: laravel_surface
                .and_then(|surface| self.seen_laravel_ui_surfaces.get(&surface).copied())
                .unwrap_or(0),
            runtime_family_prefix_overlap: self
                .runtime_anchor_paths
                .iter()
                .map(|anchor| SharedPathFacts::shared_prefix_segments(path, anchor))
                .max()
                .unwrap_or(0),
        }
    }
}

fn blade_component_specific_query_terms(query_text: &str) -> Vec<String> {
    const GENERIC_TERMS: &[&str] = &[
        "app",
        "application",
        "blade",
        "component",
        "components",
        "company",
        "create",
        "creates",
        "footer",
        "header",
        "layout",
        "layouts",
        "page",
        "pages",
        "pest",
        "render",
        "report",
        "reports",
        "resources",
        "section",
        "slot",
        "test",
        "tests",
        "view",
        "views",
    ];

    hybrid_query_exact_terms(query_text)
        .into_iter()
        .filter(|term| {
            !GENERIC_TERMS
                .iter()
                .any(|generic| generic == &term.as_str())
        })
        .collect()
}

fn path_depth(path: &str) -> usize {
    path.trim_start_matches("./")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

fn blade_component_specific_path_overlap_count(path: &str, specific_terms: &[String]) -> usize {
    if specific_terms.is_empty() {
        return 0;
    }

    let path_terms = hybrid_identifier_tokens(path);
    specific_terms
        .iter()
        .filter(|term| path_terms.iter().any(|path_term| path_term == *term))
        .count()
}

#[cfg(test)]
mod tests {
    use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceDocumentRef};

    use super::*;
    use super::super::path_quality::PathQualityFacts;
    use super::super::path_witness::PathWitnessFacts;
    use super::super::selection::{SelectionCandidate, SelectionFacts, SelectionState};
    use crate::searcher::path_witness_projection::StoredPathWitnessProjection;
    use crate::searcher::types::HybridRankedEvidence;

    fn make_ranked(path: &str, score: f32) -> HybridRankedEvidence {
        HybridRankedEvidence {
            document: EvidenceDocumentRef {
                repository_id: "repo".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            anchor: EvidenceAnchor::new(EvidenceAnchorKind::PathWitness, 1, 1, 1, 1),
            excerpt: path.to_owned(),
            blended_score: score,
            lexical_score: score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: Vec::new(),
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        }
    }

    #[test]
    fn shared_intent_facts_normalize_test_and_config_biases() {
        let intent = HybridRankingIntent::from_query(
            "config examples benches benchmark pyproject requirements tests",
        );
        let facts = SharedIntentFacts::from_intent(&intent);

        assert!(facts.wants_example_or_bench_witnesses);
        assert!(facts.penalize_generic_runtime_docs);
        assert!(facts.wants_python_witnesses);
        assert!(facts.wants_python_workspace_config);
        assert!(facts.wants_runtime_companion_tests);
        assert!(!facts.prefer_runtime_anchor_tests);
    }

    #[test]
    fn policy_query_context_normalizes_cli_blade_and_android_signals() {
        let intent = HybridRankingIntent::from_query(
            "blade component invoice view cli android activity screen viewmodel",
        );
        let context = PolicyQueryContext::new(
            &intent,
            "blade component invoice view cli android activity screen viewmodel",
        );

        assert!(context.query_mentions_cli);
        assert!(context.query_has_specific_blade_anchors);
        assert!(context.wants_kotlin_android_ui_witnesses);
        assert!(context
            .blade_component_specific_terms
            .iter()
            .any(|term| term == "invoice"));
    }

    #[test]
    fn policy_query_context_excerpt_matching_tracks_identifier_and_build_anchors() {
        struct Case {
            query: &'static str,
            excerpt: &'static str,
            expect_overlap: bool,
            expect_exact_identifier_anchor: bool,
            expect_build_anchor: bool,
            expect_test_double_anchor: bool,
        }

        let cases = [
            Case {
                query: "server_main entrypoint",
                excerpt: "server_main entrypoint helper",
                expect_overlap: true,
                expect_exact_identifier_anchor: true,
                expect_build_anchor: false,
                expect_test_double_anchor: false,
            },
            Case {
                query: "entrypoint build workflow server",
                excerpt: "build server runner = build_ mock contract",
                expect_overlap: false,
                expect_exact_identifier_anchor: false,
                expect_build_anchor: true,
                expect_test_double_anchor: true,
            },
            Case {
                query: "docs readme onboarding",
                excerpt: "general documentation without anchor terms",
                expect_overlap: false,
                expect_exact_identifier_anchor: false,
                expect_build_anchor: false,
                expect_test_double_anchor: false,
            },
        ];

        for case in cases {
            let intent = HybridRankingIntent::from_query(case.query);
            let shared_intent = SharedIntentFacts::from_intent(&intent);
            let context = PolicyQueryContext::new(&intent, case.query);
            let match_result = context.match_excerpt(case.excerpt, &shared_intent);

            assert_eq!(match_result.excerpt_overlap > 0, case.expect_overlap, "query={}", case.query);
            assert_eq!(
                match_result.excerpt_has_exact_identifier_anchor,
                case.expect_exact_identifier_anchor,
                "query={}",
                case.query
            );
            assert_eq!(
                match_result.excerpt_has_build_flow_anchor,
                case.expect_build_anchor,
                "query={}",
                case.query
            );
            assert_eq!(
                match_result.excerpt_has_test_double_anchor,
                case.expect_test_double_anchor,
                "query={}",
                case.query
            );
        }
    }

    #[test]
    fn shared_path_facts_normalize_runtime_config_and_laravel_shape_flags() {
        let config = SharedPathFacts::from_path("gradle/wrapper/gradle-wrapper.properties");
        let laravel = SharedPathFacts::from_path("resources/views/components/invoice-table.blade.php");

        assert!(config.is_runtime_config_artifact);
        assert!(config.is_repo_root_runtime_config_artifact);
        assert!(laravel.is_laravel_blade_component);
        assert!(laravel.laravel_surface.is_some());
    }

    #[test]
    fn shared_core_keeps_runtime_config_flags_consistent_across_stages() {
        let query = "config pyproject requirements tests";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = PolicyQueryContext::new(&intent, query);
        let path = "gradle/wrapper/gradle-wrapper.properties";
        let projection = StoredPathWitnessProjection::from_path(path);
        let path_quality = PathQualityFacts::from_path(path, &intent);
        let path_witness = PathWitnessFacts::from_projection(path, &projection, &intent, &query_context);
        let candidate = SelectionCandidate::new(make_ranked(path, 1.0), &intent, &query_context);
        let selection = SelectionFacts::from_candidate(&candidate, &intent, &query_context, &SelectionState::default());

        assert!(path_quality.is_runtime_config_artifact);
        assert!(path_witness.is_config_artifact);
        assert!(selection.is_runtime_config_artifact);
        assert!(path_quality.is_repo_root_runtime_config_artifact);
        assert!(path_witness.is_repo_root_runtime_config_artifact);
        assert!(selection.is_repo_root_runtime_config_artifact);
        assert_eq!(path_quality.wants_example_or_bench_witnesses, path_witness.wants_example_or_bench_witnesses);
        assert_eq!(path_witness.wants_example_or_bench_witnesses, selection.wants_example_or_bench_witnesses);
    }

    #[test]
    fn shared_core_keeps_laravel_component_flags_consistent_across_stages() {
        let query = "blade component invoice layout slot view render";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = PolicyQueryContext::new(&intent, query);
        let path = "resources/views/components/invoice-table.blade.php";
        let projection = StoredPathWitnessProjection::from_path(path);
        let shared_path = SharedPathFacts::from_path(path);
        let path_quality = PathQualityFacts::from_path(path, &intent);
        let path_witness = PathWitnessFacts::from_projection(path, &projection, &intent, &query_context);
        let candidate = SelectionCandidate::new(make_ranked(path, 1.0), &intent, &query_context);
        let selection = SelectionFacts::from_candidate(&candidate, &intent, &query_context, &SelectionState::default());

        assert!(path_quality.is_laravel_blade_component);
        assert!(path_witness.is_laravel_blade_component);
        assert!(selection.is_laravel_blade_component);
        assert_eq!(
            path_quality.is_laravel_non_livewire_blade_view,
            path_witness.is_laravel_non_livewire_blade_view
        );
        assert_eq!(
            path_witness.is_laravel_non_livewire_blade_view,
            selection.is_laravel_non_livewire_blade_view
        );
        assert_eq!(
            shared_path.is_laravel_top_level_blade_view,
            path_witness.is_laravel_top_level_blade_view
        );
        assert!(!path_witness.is_laravel_top_level_blade_view);
        assert_eq!(path_quality.is_laravel_layout_blade_view, selection.is_laravel_layout_blade_view);
    }

    #[test]
    fn selection_coverage_state_snapshots_accumulate_expected_state() {
        let runtime_path = "src/main.rs";
        let runtime_shared = SharedPathFacts::from_path(runtime_path);
        let support_path = "tests/integration/main_test.rs";
        let support_shared = SharedPathFacts::from_path(support_path);
        let blade_path = "resources/views/components/invoice-table.blade.php";
        let blade_shared = SharedPathFacts::from_path(blade_path);

        let mut coverage = SelectionCoverageState::default();
        coverage.observe(
            runtime_path,
            &runtime_shared,
            runtime_shared.class,
            runtime_shared.is_ci_workflow,
            runtime_shared.is_example_support,
            runtime_shared.is_repo_root_runtime_config_artifact,
            runtime_shared.is_typescript_runtime_module_index,
        );
        coverage.observe(
            support_path,
            &support_shared,
            support_shared.class,
            support_shared.is_ci_workflow,
            support_shared.is_example_support,
            support_shared.is_repo_root_runtime_config_artifact,
            support_shared.is_typescript_runtime_module_index,
        );

        let runtime_snapshot =
            coverage.snapshot_for(runtime_path, runtime_shared.class, runtime_shared.laravel_surface);
        let blade_snapshot =
            coverage.snapshot_for(blade_path, blade_shared.class, blade_shared.laravel_surface);

        assert_eq!(runtime_snapshot.seen_count, 1);
        assert_eq!(runtime_snapshot.runtime_seen, 1);
        assert_eq!(runtime_snapshot.seen_plain_test_support, 1);
        assert_eq!(runtime_snapshot.runtime_family_prefix_overlap, 2);
        assert_eq!(blade_snapshot.seen_count, 0);
        assert_eq!(blade_snapshot.runtime_seen, 1);
        assert_eq!(blade_snapshot.seen_plain_test_support, 1);
        assert_eq!(blade_snapshot.laravel_surface_seen, 0);
        assert_eq!(blade_snapshot.runtime_family_prefix_overlap, 0);
    }
}
