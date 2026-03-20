use std::path::Path;

use crate::domain::SourceClass;
use crate::languages::{LanguageCapability, supported_language_for_path};

use super::path_witness_projection::{self, StoredPathWitnessProjection};
use super::surfaces::{hybrid_source_class, is_frontend_runtime_noise_path, is_repo_metadata_path};

pub(crate) fn hybrid_match_source_class(path: &str) -> SourceClass {
    hybrid_source_class(path)
}

pub(crate) fn hybrid_match_surface_families(path: &str) -> Vec<String> {
    let projection = StoredPathWitnessProjection::from_path(path);
    path_witness_projection::generic_surface_families_for_projection(&projection)
        .into_iter()
        .map(|family| {
            match family {
                path_witness_projection::GenericWitnessSurfaceFamily::Runtime => "runtime",
                path_witness_projection::GenericWitnessSurfaceFamily::Tests => "tests",
                path_witness_projection::GenericWitnessSurfaceFamily::PackageSurface => {
                    "package_surface"
                }
                path_witness_projection::GenericWitnessSurfaceFamily::BuildConfig => "build_config",
                path_witness_projection::GenericWitnessSurfaceFamily::Entrypoint => "entrypoint",
                path_witness_projection::GenericWitnessSurfaceFamily::WorkspaceConfig => {
                    "workspace_config"
                }
            }
            .to_owned()
        })
        .collect()
}

pub(crate) fn hybrid_match_document_symbols_supported(path: &str) -> bool {
    supported_language_for_path(Path::new(path), LanguageCapability::DocumentSymbols).is_some()
}

pub(crate) fn hybrid_match_definition_navigation_supported(path: &str) -> bool {
    supported_language_for_path(Path::new(path), LanguageCapability::SymbolCorpus).is_some()
}

pub(crate) fn hybrid_match_is_live_navigation_pivot(path: &str) -> bool {
    if is_repo_metadata_path(path) || is_frontend_runtime_noise_path(path) {
        return false;
    }

    matches!(
        hybrid_source_class(path),
        SourceClass::Runtime | SourceClass::Support | SourceClass::Tests
    )
}
