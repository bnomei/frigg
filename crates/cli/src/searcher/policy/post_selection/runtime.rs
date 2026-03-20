use std::collections::BTreeSet;

use super::super::super::query_terms::hybrid_query_has_kotlin_android_ui_terms;
use super::super::super::query_terms::hybrid_query_mentions_cli_command;
use super::super::SelectionState;
use super::*;
use crate::searcher::policy::facts::SharedPathFacts;

#[path = "runtime/cli.rs"]
mod cli;
#[path = "runtime/companions.rs"]
mod companions;
#[path = "runtime/config.rs"]
mod config;
#[path = "runtime/shared.rs"]
mod shared;
#[path = "runtime/workflows.rs"]
mod workflows;

pub(super) use cli::{apply_cli_entrypoint_visibility, apply_cli_specific_test_visibility};
pub(super) use companions::{
    apply_mixed_support_visibility, apply_runtime_companion_surface_visibility,
    apply_runtime_companion_test_ordering, apply_runtime_companion_test_visibility,
    apply_runtime_witness_rescue_visibility,
};
pub(super) use config::{
    apply_runtime_config_surface_ordering, apply_runtime_config_surface_selection,
};
use shared::{
    cli_specific_test_guardrail_cmp, is_runtime_config_ordering_candidate_path,
    preserve_selected_build_workflow, query_mentions_cli_command,
    runtime_companion_surface_guardrail_cmp, runtime_companion_surface_supports_query,
    runtime_config_artifact_guardrail_cmp, runtime_config_ordering_cmp, selected_match_for_path,
};
pub(super) use workflows::{
    apply_ci_scripts_ops_visibility, apply_entrypoint_build_workflow_visibility,
    apply_runtime_entrypoint_visibility,
};
