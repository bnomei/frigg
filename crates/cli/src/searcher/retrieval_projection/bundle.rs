use std::path::Path;

use crate::domain::FriggResult;
use crate::storage::RetrievalProjectionBundle;

use super::{
    ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION,
    PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION, PATH_RELATION_PROJECTION_HEURISTIC_VERSION,
    PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION, RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE,
    RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH, RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION,
    RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM, RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
    RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE, RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT,
    RETRIEVAL_PROJECTION_INPUT_MODE_AST, RETRIEVAL_PROJECTION_INPUT_MODE_PATH,
    SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION, TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION,
    build_path_anchor_sketch_projection_records, build_path_relation_projection_records,
    build_path_surface_term_projection_records, build_subtree_coverage_projection_records,
    normalize_path_anchor_sketch_projection_records, normalize_path_relation_projection_records,
    normalize_path_surface_term_projection_records,
};

#[derive(Default)]
pub(super) struct RetrievalProjectionInputModes {
    pub(super) path_relation: std::collections::BTreeSet<String>,
    pub(super) path_surface_term: std::collections::BTreeSet<String>,
    pub(super) path_anchor_sketch: std::collections::BTreeSet<String>,
}

pub(crate) fn build_retrieval_projection_bundle(
    repository_id: &str,
    workspace_root: &Path,
    manifest_paths: &[String],
) -> FriggResult<RetrievalProjectionBundle> {
    let path_witness =
        super::super::path_witness_projection::build_path_witness_projection_records_from_paths(
            manifest_paths,
        )?;
    let stored_path_witness =
        super::super::path_witness_projection::decode_path_witness_projection_records(
            &path_witness,
        )?;
    let test_subject =
        super::super::overlay_projection::build_test_subject_projection_records(manifest_paths)?;
    let stored_test_subject =
        super::super::overlay_projection::decode_test_subject_projection_records(&test_subject)?;
    let entrypoint_surface =
        super::super::overlay_projection::build_entrypoint_surface_projection_records_from_paths(
            manifest_paths,
        )?;
    let stored_entrypoint_surface =
        super::super::overlay_projection::decode_entrypoint_surface_projection_records(
            &entrypoint_surface,
        )?;
    let path_relations = build_path_relation_projection_records(
        &stored_path_witness,
        &stored_test_subject,
        &stored_entrypoint_surface,
    );
    let subtree_coverage = build_subtree_coverage_projection_records(&stored_path_witness);
    let path_surface_terms = build_path_surface_term_projection_records(
        &stored_path_witness,
        &stored_entrypoint_surface,
    );
    let path_anchor_sketches = build_path_anchor_sketch_projection_records(
        workspace_root,
        &stored_path_witness,
        &path_surface_terms,
    );
    let absolute_manifest_paths = manifest_paths
        .iter()
        .map(|path| workspace_root.join(path))
        .collect::<Vec<_>>();

    let mut input_modes = RetrievalProjectionInputModes::default();
    input_modes
        .path_relation
        .insert(RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned());
    input_modes
        .path_surface_term
        .insert(RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned());
    input_modes
        .path_anchor_sketch
        .insert(RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned());

    let mut path_relations = path_relations;
    let mut path_surface_terms = path_surface_terms;
    let mut path_anchor_sketches = path_anchor_sketches;

    let ast_relation_count_before = path_relations.len();
    super::ast::apply_ast_bundle_contributions(
        workspace_root,
        &absolute_manifest_paths,
        &stored_path_witness,
        &mut path_relations,
        &mut path_surface_terms,
        &mut path_anchor_sketches,
        &mut input_modes,
    );
    if path_relations.len() > ast_relation_count_before {
        input_modes
            .path_relation
            .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
    }
    super::scip::apply_scip_projection_contributions(
        repository_id,
        workspace_root,
        &stored_path_witness,
        &mut path_relations,
        &mut path_surface_terms,
        &mut path_anchor_sketches,
        &mut input_modes,
    );

    normalize_path_relation_projection_records(&mut path_relations);
    normalize_path_surface_term_projection_records(&mut path_surface_terms);
    normalize_path_anchor_sketch_projection_records(&mut path_anchor_sketches);

    Ok(RetrievalProjectionBundle {
        heads: vec![
            head(
                RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
                super::super::path_witness_projection::PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
                path_witness.len(),
                &[RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned()],
            ),
            head(
                RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT,
                TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION,
                test_subject.len(),
                &[RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned()],
            ),
            head(
                RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE,
                ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION,
                entrypoint_surface.len(),
                &[RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned()],
            ),
            head(
                RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION,
                PATH_RELATION_PROJECTION_HEURISTIC_VERSION,
                path_relations.len(),
                &input_modes
                    .path_relation
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            head(
                RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE,
                SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION,
                subtree_coverage.len(),
                &[RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned()],
            ),
            head(
                RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM,
                PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION,
                path_surface_terms.len(),
                &input_modes
                    .path_surface_term
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            head(
                RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH,
                PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION,
                path_anchor_sketches.len(),
                &input_modes
                    .path_anchor_sketch
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
        ],
        path_witness,
        test_subject,
        entrypoint_surface,
        path_relations,
        subtree_coverage,
        path_surface_terms,
        path_anchor_sketches,
    })
}

fn head(
    family: &str,
    heuristic_version: i64,
    row_count: usize,
    input_modes: &[String],
) -> crate::storage::RetrievalProjectionHeadRecord {
    crate::storage::RetrievalProjectionHeadRecord {
        family: family.to_owned(),
        heuristic_version,
        input_modes: input_modes.to_vec(),
        row_count,
    }
}
