//! Retrieval projection assembly for manifest-driven search heuristics.

mod ast;
mod builders;
mod bundle;
mod normalize;
mod scip;

pub(crate) const RETRIEVAL_PROJECTION_INPUT_MODE_PATH: &str = "path";
pub(crate) const RETRIEVAL_PROJECTION_INPUT_MODE_AST: &str = "ast";
pub(crate) const RETRIEVAL_PROJECTION_INPUT_MODE_SCIP: &str = "scip";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS: &str = "path_witness";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT: &str = "test_subject";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE: &str = "entrypoint_surface";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION: &str = "path_relation";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE: &str = "subtree_coverage";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM: &str = "path_surface_term";
pub(crate) const RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH: &str = "path_anchor_sketch";
pub(crate) const TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION: i64 = 1;
pub(crate) const ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION: i64 = 1;
pub(crate) const PATH_RELATION_PROJECTION_HEURISTIC_VERSION: i64 = 1;
pub(crate) const SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION: i64 = 1;
pub(crate) const PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION: i64 = 1;
pub(crate) const PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION: i64 = 2;

/// The required projection families paired with the heuristic version the current
/// runtime produces for each. This is the single source of truth used both when
/// building bundles and when deciding whether a reused snapshot's persisted heads
/// are still current. Consumers reject heads whose version does not match these.
pub(crate) fn required_retrieval_projection_versions() -> [(&'static str, i64); 7] {
    [
        (
            RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
            super::path_witness_projection::PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
        ),
        (
            RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT,
            TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION,
        ),
        (
            RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE,
            ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION,
        ),
        (
            RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION,
            PATH_RELATION_PROJECTION_HEURISTIC_VERSION,
        ),
        (
            RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE,
            SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION,
        ),
        (
            RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM,
            PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION,
        ),
        (
            RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH,
            PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION,
        ),
    ]
}

pub(crate) use ast::augment_path_relation_projection_records_with_ast_relation_evidence;
pub(crate) use builders::{
    build_path_anchor_sketch_projection_records, build_path_relation_projection_records,
    build_path_surface_term_projection_records, build_subtree_coverage_projection_records,
};
pub(crate) use bundle::build_retrieval_projection_bundle;
pub(crate) use normalize::{
    normalize_path_anchor_sketch_projection_records, normalize_path_relation_projection_records,
    normalize_path_surface_term_projection_records,
};
