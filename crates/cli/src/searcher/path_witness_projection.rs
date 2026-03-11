use std::path::Path;

use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use crate::path_class::classify_repository_path;
use crate::storage::PathWitnessProjectionRecord;
use serde::{Deserialize, Serialize};

use super::{
    hybrid_path_overlap_tokens, hybrid_source_class, is_bench_support_path, is_ci_workflow_path,
    is_cli_test_support_path, is_entrypoint_build_workflow_path, is_entrypoint_runtime_path,
    is_example_support_path, is_frontend_runtime_noise_path, is_laravel_blade_component_path,
    is_laravel_bootstrap_entrypoint_path, is_laravel_command_or_middleware_path,
    is_laravel_core_provider_path, is_laravel_form_action_blade_path,
    is_laravel_job_or_listener_path, is_laravel_layout_blade_view_path,
    is_laravel_livewire_component_path, is_laravel_livewire_view_path,
    is_laravel_nested_blade_component_path, is_laravel_non_livewire_blade_view_path,
    is_laravel_provider_path, is_laravel_route_path, is_laravel_view_component_class_path,
    is_python_runtime_config_path, is_python_test_witness_path, is_runtime_config_artifact_path,
    is_scripts_ops_path, is_test_harness_path, is_test_support_path,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(super) struct PathWitnessProjectionFlags {
    pub(super) is_entrypoint_runtime: bool,
    pub(super) is_entrypoint_build_workflow: bool,
    pub(super) is_ci_workflow: bool,
    pub(super) is_runtime_config_artifact: bool,
    pub(super) is_python_runtime_config: bool,
    pub(super) is_python_test_witness: bool,
    pub(super) is_example_support: bool,
    pub(super) is_bench_support: bool,
    pub(super) is_cli_test_support: bool,
    pub(super) is_test_harness: bool,
    pub(super) is_scripts_ops: bool,
    pub(super) is_frontend_runtime_noise: bool,
    pub(super) is_test_support: bool,
    pub(super) is_laravel_non_livewire_blade_view: bool,
    pub(super) is_laravel_livewire_view: bool,
    pub(super) is_laravel_blade_component: bool,
    pub(super) is_laravel_nested_blade_component: bool,
    pub(super) is_laravel_form_action_blade: bool,
    pub(super) is_laravel_livewire_component: bool,
    pub(super) is_laravel_view_component_class: bool,
    pub(super) is_laravel_command_or_middleware: bool,
    pub(super) is_laravel_job_or_listener: bool,
    pub(super) is_laravel_layout_blade_view: bool,
    pub(super) is_laravel_route: bool,
    pub(super) is_laravel_bootstrap_entrypoint: bool,
    pub(super) is_laravel_core_provider: bool,
    pub(super) is_laravel_provider: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredPathWitnessProjection {
    pub(super) path: String,
    pub(super) path_class: PathClass,
    pub(super) source_class: SourceClass,
    pub(super) file_stem: String,
    pub(super) path_terms: Vec<String>,
    pub(super) flags: PathWitnessProjectionFlags,
}

impl StoredPathWitnessProjection {
    pub(super) fn from_path(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            path_class: classify_repository_path(path),
            source_class: hybrid_source_class(path),
            file_stem: file_stem_for_path(path),
            path_terms: hybrid_path_overlap_tokens(path),
            flags: build_path_witness_projection_flags(path),
        }
    }
}

pub(super) fn build_path_witness_projection_record(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
) -> FriggResult<PathWitnessProjectionRecord> {
    let projection = StoredPathWitnessProjection::from_path(path);
    let path_terms_json = serde_json::to_string(&projection.path_terms).map_err(|err| {
        FriggError::Internal(format!(
            "failed to encode path witness projection terms for '{path}': {err}"
        ))
    })?;
    let flags_json = serde_json::to_string(&projection.flags).map_err(|err| {
        FriggError::Internal(format!(
            "failed to encode path witness projection flags for '{path}': {err}"
        ))
    })?;

    Ok(PathWitnessProjectionRecord {
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        path_class: projection.path_class.as_str().to_owned(),
        source_class: projection.source_class.as_str().to_owned(),
        path_terms_json,
        flags_json,
    })
}

pub(super) fn decode_path_witness_projection_record(
    record: &PathWitnessProjectionRecord,
) -> FriggResult<StoredPathWitnessProjection> {
    let path_class = PathClass::from_str(&record.path_class).ok_or_else(|| {
        FriggError::Internal(format!(
            "invalid stored path witness path_class '{}' for '{}'",
            record.path_class, record.path
        ))
    })?;
    let source_class = SourceClass::from_str(&record.source_class).ok_or_else(|| {
        FriggError::Internal(format!(
            "invalid stored path witness source_class '{}' for '{}'",
            record.source_class, record.path
        ))
    })?;
    let path_terms = serde_json::from_str(&record.path_terms_json).map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode stored path witness terms for '{}': {err}",
            record.path
        ))
    })?;
    let flags = serde_json::from_str(&record.flags_json).map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode stored path witness flags for '{}': {err}",
            record.path
        ))
    })?;

    Ok(StoredPathWitnessProjection {
        path: record.path.clone(),
        path_class,
        source_class,
        file_stem: file_stem_for_path(&record.path),
        path_terms,
        flags,
    })
}

fn file_stem_for_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

fn build_path_witness_projection_flags(path: &str) -> PathWitnessProjectionFlags {
    PathWitnessProjectionFlags {
        is_entrypoint_runtime: is_entrypoint_runtime_path(path),
        is_entrypoint_build_workflow: is_entrypoint_build_workflow_path(path),
        is_ci_workflow: is_ci_workflow_path(path),
        is_runtime_config_artifact: is_runtime_config_artifact_path(path),
        is_python_runtime_config: is_python_runtime_config_path(path),
        is_python_test_witness: is_python_test_witness_path(path),
        is_example_support: is_example_support_path(path),
        is_bench_support: is_bench_support_path(path),
        is_cli_test_support: is_cli_test_support_path(path),
        is_test_harness: is_test_harness_path(path),
        is_scripts_ops: is_scripts_ops_path(path),
        is_frontend_runtime_noise: is_frontend_runtime_noise_path(path),
        is_test_support: is_test_support_path(path),
        is_laravel_non_livewire_blade_view: is_laravel_non_livewire_blade_view_path(path),
        is_laravel_livewire_view: is_laravel_livewire_view_path(path),
        is_laravel_blade_component: is_laravel_blade_component_path(path),
        is_laravel_nested_blade_component: is_laravel_nested_blade_component_path(path),
        is_laravel_form_action_blade: is_laravel_form_action_blade_path(path),
        is_laravel_livewire_component: is_laravel_livewire_component_path(path),
        is_laravel_view_component_class: is_laravel_view_component_class_path(path),
        is_laravel_command_or_middleware: is_laravel_command_or_middleware_path(path),
        is_laravel_job_or_listener: is_laravel_job_or_listener_path(path),
        is_laravel_layout_blade_view: is_laravel_layout_blade_view_path(path),
        is_laravel_route: is_laravel_route_path(path),
        is_laravel_bootstrap_entrypoint: is_laravel_bootstrap_entrypoint_path(path),
        is_laravel_core_provider: is_laravel_core_provider_path(path),
        is_laravel_provider: is_laravel_provider_path(path),
    }
}
