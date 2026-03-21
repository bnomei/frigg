use std::path::Path;

use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use crate::path_class::classify_repository_path;
use crate::storage::PathWitnessProjection;
use serde::{Deserialize, Serialize};

use super::{
    coverage_subtree_root, hybrid_path_overlap_tokens, hybrid_source_class, is_bench_support_path,
    is_build_config_surface_path, is_ci_workflow_path, is_cli_test_support_path,
    is_entrypoint_build_workflow_path, is_entrypoint_runtime_path, is_example_support_path,
    is_frontend_runtime_noise_path, is_kotlin_android_ui_runtime_surface_path,
    is_laravel_blade_component_path, is_laravel_bootstrap_entrypoint_path,
    is_laravel_command_or_middleware_path, is_laravel_core_provider_path,
    is_laravel_form_action_blade_path, is_laravel_job_or_listener_path,
    is_laravel_layout_blade_view_path, is_laravel_livewire_component_path,
    is_laravel_livewire_view_path, is_laravel_nested_blade_component_path,
    is_laravel_non_livewire_blade_view_path, is_laravel_provider_path, is_laravel_route_path,
    is_laravel_view_component_class_path, is_package_surface_path, is_python_runtime_config_path,
    is_python_test_witness_path, is_runtime_companion_surface_path,
    is_runtime_config_artifact_path, is_scripts_ops_path, is_test_harness_path,
    is_test_support_path, is_workspace_config_surface_path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum GenericWitnessSurfaceFamily {
    Runtime,
    Tests,
    PackageSurface,
    BuildConfig,
    Entrypoint,
    WorkspaceConfig,
}

pub(crate) const PATH_WITNESS_PROJECTION_HEURISTIC_VERSION: i64 = 1;
const FAMILY_RUNTIME_BIT: u64 = 1 << 0;
const FAMILY_TESTS_BIT: u64 = 1 << 1;
const FAMILY_PACKAGE_SURFACE_BIT: u64 = 1 << 2;
const FAMILY_BUILD_CONFIG_BIT: u64 = 1 << 3;
const FAMILY_ENTRYPOINT_BIT: u64 = 1 << 4;
const FAMILY_WORKSPACE_CONFIG_BIT: u64 = 1 << 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(super) struct PathWitnessProjectionFlags {
    pub(super) is_entrypoint_runtime: bool,
    pub(super) is_entrypoint_build_workflow: bool,
    pub(super) is_ci_workflow: bool,
    pub(super) is_runtime_config_artifact: bool,
    pub(super) is_package_surface: bool,
    pub(super) is_build_config_surface: bool,
    pub(super) is_runtime_companion_surface: bool,
    pub(super) is_workspace_config_surface: bool,
    pub(super) is_kotlin_android_ui_runtime_surface: bool,
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
pub(crate) struct StoredPathWitnessProjection {
    pub(super) path: String,
    pub(super) path_class: PathClass,
    pub(super) source_class: SourceClass,
    pub(super) file_stem: String,
    pub(super) path_terms: Vec<String>,
    pub(super) subtree_root: Option<String>,
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
            subtree_root: coverage_subtree_root(path),
            flags: build_path_witness_projection_flags(path),
        }
    }
}

pub(crate) fn generic_surface_families_for_projection(
    projection: &StoredPathWitnessProjection,
) -> Vec<GenericWitnessSurfaceFamily> {
    let mut families = Vec::new();
    if projection.flags.is_runtime_companion_surface {
        families.push(GenericWitnessSurfaceFamily::Runtime);
    }
    if projection.flags.is_test_support || projection.flags.is_test_harness {
        families.push(GenericWitnessSurfaceFamily::Tests);
    }
    if projection.flags.is_package_surface {
        families.push(GenericWitnessSurfaceFamily::PackageSurface);
    }
    if projection.flags.is_runtime_config_artifact {
        families.push(GenericWitnessSurfaceFamily::BuildConfig);
    }
    if projection.flags.is_build_config_surface || projection.flags.is_entrypoint_build_workflow {
        families.push(GenericWitnessSurfaceFamily::BuildConfig);
    }
    if projection.flags.is_entrypoint_runtime {
        families.push(GenericWitnessSurfaceFamily::Entrypoint);
    }
    if projection.flags.is_workspace_config_surface {
        families.push(GenericWitnessSurfaceFamily::WorkspaceConfig);
    }
    families.sort();
    families.dedup();
    families
}

pub(crate) fn family_bits_for_projection(projection: &StoredPathWitnessProjection) -> u64 {
    generic_surface_families_for_projection(projection)
        .into_iter()
        .fold(0u64, |bits, family| bits | family_bit(family))
}

#[allow(dead_code)]
pub(crate) fn generic_surface_families_from_bits(bits: u64) -> Vec<GenericWitnessSurfaceFamily> {
    let mut families = Vec::new();
    for family in [
        GenericWitnessSurfaceFamily::Runtime,
        GenericWitnessSurfaceFamily::Tests,
        GenericWitnessSurfaceFamily::PackageSurface,
        GenericWitnessSurfaceFamily::BuildConfig,
        GenericWitnessSurfaceFamily::Entrypoint,
        GenericWitnessSurfaceFamily::WorkspaceConfig,
    ] {
        if bits & family_bit(family) != 0 {
            families.push(family);
        }
    }
    families
}

pub(crate) fn generic_surface_family_from_name(value: &str) -> Option<GenericWitnessSurfaceFamily> {
    match value.trim() {
        "runtime" => Some(GenericWitnessSurfaceFamily::Runtime),
        "tests" => Some(GenericWitnessSurfaceFamily::Tests),
        "package_surface" => Some(GenericWitnessSurfaceFamily::PackageSurface),
        "build_config" => Some(GenericWitnessSurfaceFamily::BuildConfig),
        "entrypoint" => Some(GenericWitnessSurfaceFamily::Entrypoint),
        "workspace_config" => Some(GenericWitnessSurfaceFamily::WorkspaceConfig),
        _ => None,
    }
}

pub(super) fn build_path_witness_projection(path: &str) -> FriggResult<PathWitnessProjection> {
    let projection = StoredPathWitnessProjection::from_path(path);
    let family_bits = family_bits_for_projection(&projection);
    let flags_json = serde_json::to_string(&projection.flags).map_err(|err| {
        FriggError::Internal(format!(
            "failed to encode path witness projection flags for '{path}': {err}"
        ))
    })?;

    Ok(PathWitnessProjection {
        path: path.to_owned(),
        path_class: projection.path_class,
        source_class: projection.source_class,
        file_stem: projection.file_stem,
        path_terms: projection.path_terms,
        subtree_root: projection.subtree_root,
        family_bits,
        flags_json,
        heuristic_version: PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
    })
}

pub(crate) fn build_path_witness_projection_records_from_paths(
    paths: &[String],
) -> FriggResult<Vec<PathWitnessProjection>> {
    let mut rows = paths
        .iter()
        .map(|path| build_path_witness_projection(path))
        .collect::<FriggResult<Vec<_>>>()?;
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    rows.dedup_by(|left, right| left.path == right.path);
    Ok(rows)
}

pub(super) fn decode_path_witness_projection(
    record: &PathWitnessProjection,
) -> FriggResult<StoredPathWitnessProjection> {
    let path_class = record.path_class;
    let source_class = record.source_class;
    let source_class = match source_class {
        // Legacy rows may still carry the old FRIGG-specific playbook class. Normalize those
        // projections to the generic path-based class so ranking behavior does not depend on
        // historical storage state.
        SourceClass::Playbooks => SourceClass::Project,
        other => other,
    };
    let stored_flags: PathWitnessProjectionFlags = serde_json::from_str(&record.flags_json)
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode stored path witness flags for '{}': {err}",
                record.path
            ))
        })?;
    let trust_stored = record.heuristic_version == PATH_WITNESS_PROJECTION_HEURISTIC_VERSION
        && !record.file_stem.is_empty();
    let path_terms = if trust_stored {
        record.path_terms.clone()
    } else {
        hybrid_path_overlap_tokens(&record.path)
    };
    let flags = if trust_stored {
        stored_flags
    } else {
        build_path_witness_projection_flags(&record.path)
    };

    Ok(StoredPathWitnessProjection {
        path: record.path.clone(),
        path_class,
        source_class,
        file_stem: if trust_stored {
            record.file_stem.clone()
        } else {
            file_stem_for_path(&record.path)
        },
        path_terms,
        subtree_root: if trust_stored {
            record.subtree_root.clone()
        } else {
            coverage_subtree_root(&record.path)
        },
        flags,
    })
}

pub(crate) fn decode_path_witness_projection_records(
    rows: &[PathWitnessProjection],
) -> FriggResult<Vec<StoredPathWitnessProjection>> {
    rows.iter().map(decode_path_witness_projection).collect()
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
        is_package_surface: is_package_surface_path(path),
        is_build_config_surface: is_build_config_surface_path(path),
        is_runtime_companion_surface: is_runtime_companion_surface_path(path),
        is_workspace_config_surface: is_workspace_config_surface_path(path),
        is_kotlin_android_ui_runtime_surface: is_kotlin_android_ui_runtime_surface_path(path),
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

fn family_bit(family: GenericWitnessSurfaceFamily) -> u64 {
    match family {
        GenericWitnessSurfaceFamily::Runtime => FAMILY_RUNTIME_BIT,
        GenericWitnessSurfaceFamily::Tests => FAMILY_TESTS_BIT,
        GenericWitnessSurfaceFamily::PackageSurface => FAMILY_PACKAGE_SURFACE_BIT,
        GenericWitnessSurfaceFamily::BuildConfig => FAMILY_BUILD_CONFIG_BIT,
        GenericWitnessSurfaceFamily::Entrypoint => FAMILY_ENTRYPOINT_BIT,
        GenericWitnessSurfaceFamily::WorkspaceConfig => FAMILY_WORKSPACE_CONFIG_BIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::PathWitnessProjection;

    #[test]
    fn decode_path_witness_projection_record_recomputes_live_terms_for_stale_rows() {
        let path = "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py";
        let projection = StoredPathWitnessProjection::from_path(path);
        let stale_terms = vec![
            "autogpt_platform".to_owned(),
            "autogpt".to_owned(),
            "platform".to_owned(),
            "backend".to_owned(),
            "blocks".to_owned(),
            "test_helpers".to_owned(),
        ];
        let record = PathWitnessProjection {
            path: path.to_owned(),
            path_class: projection.path_class,
            source_class: projection.source_class,
            file_stem: projection.file_stem.clone(),
            path_terms: stale_terms.clone(),
            subtree_root: projection.subtree_root.clone(),
            family_bits: family_bits_for_projection(&projection),
            flags_json: serde_json::to_string(&projection.flags).expect("flags json"),
            heuristic_version: 0,
        };

        let decoded = decode_path_witness_projection(&record).expect("decode should succeed");

        assert!(
            decoded.path_terms.iter().any(|term| term == "helpers"),
            "live decoding should recover split helper token: {:?}",
            decoded.path_terms
        );
        assert_ne!(
            decoded.path_terms, stale_terms,
            "decoded path terms should not trust stale stored tokenization"
        );
    }

    #[test]
    fn decode_path_witness_projection_record_recomputes_live_flags_for_runtime_config_artifacts() {
        let path = "app/src/main/AndroidManifest.xml";
        let record = PathWitnessProjection {
            path: path.to_owned(),
            path_class: PathClass::Project,
            source_class: SourceClass::Project,
            file_stem: "androidmanifest".to_owned(),
            path_terms: Vec::new(),
            subtree_root: None,
            family_bits: 0,
            flags_json: serde_json::to_string(&PathWitnessProjectionFlags::default())
                .expect("flags json"),
            heuristic_version: 0,
        };

        let decoded = decode_path_witness_projection(&record).expect("decode should succeed");

        assert!(
            decoded.flags.is_runtime_config_artifact,
            "live decoding should recover Android and Gradle runtime config flags"
        );
    }

    #[test]
    fn path_witness_projection_derives_subtree_root_and_companion_surface_families() {
        let runtime = StoredPathWitnessProjection::from_path(
            "packages/editor-ui/src/components/canvas/NodeDetails.vue",
        );
        let tests = StoredPathWitnessProjection::from_path(
            "packages/editor-ui/tests/canvas/node_details.spec.ts",
        );
        let package_surface =
            StoredPathWitnessProjection::from_path("packages/editor-ui/package.json");
        let workspace_config =
            StoredPathWitnessProjection::from_path("packages/editor-ui/tsconfig.base.json");

        assert_eq!(
            runtime.subtree_root.as_deref(),
            Some("packages/editor-ui/src")
        );
        assert_eq!(
            tests.subtree_root.as_deref(),
            Some("packages/editor-ui/tests")
        );
        assert_eq!(
            package_surface.subtree_root.as_deref(),
            Some("packages/editor-ui")
        );
        assert_eq!(
            workspace_config.subtree_root.as_deref(),
            Some("packages/editor-ui")
        );
        assert!(
            runtime.flags.is_runtime_companion_surface,
            "runtime companion surface flag should be set for runtime siblings"
        );
        assert!(package_surface.flags.is_package_surface);
        assert!(workspace_config.flags.is_workspace_config_surface);
        assert_eq!(
            generic_surface_families_for_projection(&package_surface),
            vec![
                GenericWitnessSurfaceFamily::PackageSurface,
                GenericWitnessSurfaceFamily::BuildConfig,
            ]
        );
    }

    #[test]
    fn path_witness_projection_keeps_top_level_runtime_and_tests_in_same_workspace_root() {
        let runtime = StoredPathWitnessProjection::from_path("desktop/src/messages/layout.rs");
        let tests = StoredPathWitnessProjection::from_path("desktop/tests/layout.rs");

        assert_eq!(runtime.subtree_root.as_deref(), Some("desktop"));
        assert_eq!(tests.subtree_root.as_deref(), Some("desktop"));
    }
}
