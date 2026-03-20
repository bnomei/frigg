use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::FriggResult;
use crate::graph::{PreciseRelationshipKind, RelationKind, ScipResourceBudgets, SymbolGraph};
use crate::indexer::{
    SymbolDefinition, SymbolKind, extract_php_source_evidence_from_source,
    extract_symbols_for_paths,
};
use crate::languages::{
    SymbolLanguage, extract_blade_source_evidence_from_source, php_symbol_indices_by_lower_name,
    php_symbol_indices_by_name, resolve_blade_relation_evidence_edges,
    resolve_php_target_evidence_edges,
};
use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection,
    RetrievalProjectionBundle, RetrievalProjectionHeadRecord, SubtreeCoverageProjection,
};

use super::normalize_repository_relative_path;
use super::overlay_projection::{
    StoredEntrypointSurfaceProjection, StoredTestSubjectProjection,
    decode_entrypoint_surface_projection_records, decode_test_subject_projection_records,
};
use super::path_witness_projection::{
    GenericWitnessSurfaceFamily, PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
    StoredPathWitnessProjection, build_path_witness_projection_records_from_paths,
    decode_path_witness_projection_records, family_bits_for_projection,
    generic_surface_families_for_projection,
};
use super::query_terms::hybrid_query_exact_terms;
use super::{
    build_entrypoint_surface_projection_records_from_paths,
    build_test_subject_projection_records_from_paths,
};

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
pub(crate) const PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION: i64 = 1;

const MAX_RELATIONS_PER_SOURCE: usize = 8;
const MAX_ANCHOR_SKETCHES_PER_PATH: usize = 3;
const MAX_SURFACE_TERMS_PER_PATH: usize = 24;
const MAX_SCIP_ARTIFACTS: usize = 16;
const MAX_SCIP_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCIP_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCIP_DOCUMENTS_PER_ARTIFACT: usize = 2_048;
const MAX_SCIP_ELAPSED_MS: u64 = 5_000;

#[derive(Default)]
struct RetrievalProjectionInputModes {
    path_relation: BTreeSet<String>,
    path_surface_term: BTreeSet<String>,
    path_anchor_sketch: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ScipArtifactInput {
    path: PathBuf,
    label: String,
    format: ScipArtifactInputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScipArtifactInputFormat {
    Json,
    Protobuf,
}

pub(crate) fn build_retrieval_projection_bundle(
    repository_id: &str,
    workspace_root: &Path,
    manifest_paths: &[String],
) -> FriggResult<RetrievalProjectionBundle> {
    let path_witness = build_path_witness_projection_records_from_paths(manifest_paths)?;
    let stored_path_witness = decode_path_witness_projection_records(&path_witness)?;
    let test_subject = build_test_subject_projection_records_from_paths(manifest_paths)?;
    let stored_test_subject = decode_test_subject_projection_records(&test_subject)?;
    let entrypoint_surface =
        build_entrypoint_surface_projection_records_from_paths(manifest_paths)?;
    let stored_entrypoint_surface =
        decode_entrypoint_surface_projection_records(&entrypoint_surface)?;
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
    augment_path_relation_projection_records_with_ast_relation_evidence(
        workspace_root,
        &absolute_manifest_paths,
        &stored_path_witness,
        &mut path_relations,
    );
    if path_relations.len() > ast_relation_count_before {
        input_modes
            .path_relation
            .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
    }

    apply_ast_projection_contributions(
        workspace_root,
        &absolute_manifest_paths,
        &stored_path_witness,
        &mut path_relations,
        &mut path_surface_terms,
        &mut path_anchor_sketches,
        &mut input_modes,
    );
    apply_scip_projection_contributions(
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
                PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
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
) -> RetrievalProjectionHeadRecord {
    RetrievalProjectionHeadRecord {
        family: family.to_owned(),
        heuristic_version,
        input_modes: input_modes.to_vec(),
        row_count,
    }
}

pub(crate) fn build_path_relation_projection_records(
    path_witness: &[StoredPathWitnessProjection],
    test_subject: &[StoredTestSubjectProjection],
    entrypoint_surface: &[StoredEntrypointSurfaceProjection],
) -> Vec<PathRelationProjection> {
    let witness_by_path = path_witness
        .iter()
        .map(|projection| (projection.path.clone(), projection))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();

    for record in test_subject {
        rows.push(PathRelationProjection {
            src_path: record.test_path.clone(),
            dst_path: record.subject_path.clone(),
            relation_kind: "test_subject".to_owned(),
            evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned(),
            src_symbol_id: None,
            dst_symbol_id: None,
            src_family_bits: witness_by_path
                .get(&record.test_path)
                .map(|projection| family_bits_for_projection(projection))
                .unwrap_or_default(),
            dst_family_bits: witness_by_path
                .get(&record.subject_path)
                .map(|projection| family_bits_for_projection(projection))
                .unwrap_or_default(),
            shared_terms: record.shared_terms.clone(),
            score_hint: record.score_hint,
        });
    }

    for projection in entrypoint_surface
        .iter()
        .filter(|projection| projection.flags.is_runtime_entrypoint)
    {
        let Some(src_witness) = witness_by_path.get(&projection.path) else {
            continue;
        };
        let Some(subtree_root) = src_witness.subtree_root.as_deref() else {
            continue;
        };

        let mut per_source = path_witness
            .iter()
            .filter(|candidate| candidate.path != src_witness.path)
            .filter(|candidate| candidate.subtree_root.as_deref() == Some(subtree_root))
            .filter_map(|candidate| {
                let relation_kind = relation_kind_for_entrypoint_pair(candidate)?;
                let shared_terms = shared_terms_between(
                    &src_witness.path_terms,
                    &candidate.path_terms,
                    &src_witness.file_stem,
                    &candidate.file_stem,
                );
                let same_stem = src_witness.file_stem == candidate.file_stem
                    && !src_witness.file_stem.is_empty();
                if shared_terms.is_empty() && !same_stem {
                    return None;
                }
                let score_hint = 100
                    + shared_terms.len() * 12
                    + usize::from(same_stem) * 18
                    + usize::from(candidate.source_class == src_witness.source_class) * 6;
                Some(PathRelationProjection {
                    src_path: src_witness.path.clone(),
                    dst_path: candidate.path.clone(),
                    relation_kind: relation_kind.to_owned(),
                    evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned(),
                    src_symbol_id: None,
                    dst_symbol_id: None,
                    src_family_bits: family_bits_for_projection(src_witness),
                    dst_family_bits: family_bits_for_projection(candidate),
                    shared_terms,
                    score_hint,
                })
            })
            .collect::<Vec<_>>();
        per_source.sort_by(|left, right| {
            right
                .score_hint
                .cmp(&left.score_hint)
                .then(left.dst_path.cmp(&right.dst_path))
                .then(left.relation_kind.cmp(&right.relation_kind))
        });
        per_source.truncate(MAX_RELATIONS_PER_SOURCE);
        rows.extend(per_source);
    }

    let mut grouped_by_source = BTreeMap::<String, Vec<PathRelationProjection>>::new();
    for projection in path_witness {
        let Some(subtree_root) = projection.subtree_root.as_deref() else {
            continue;
        };
        if !projection.flags.is_runtime_companion_surface && !projection.flags.is_entrypoint_runtime
        {
            continue;
        }

        let mut relations = path_witness
            .iter()
            .filter(|candidate| candidate.path != projection.path)
            .filter(|candidate| candidate.subtree_root.as_deref() == Some(subtree_root))
            .filter(|candidate| {
                candidate.flags.is_test_support
                    || candidate.flags.is_test_harness
                    || candidate.flags.is_package_surface
                    || candidate.flags.is_build_config_surface
                    || candidate.flags.is_workspace_config_surface
                    || candidate.flags.is_entrypoint_build_workflow
                    || candidate.flags.is_runtime_config_artifact
            })
            .filter_map(|candidate| {
                let shared_terms = shared_terms_between(
                    &projection.path_terms,
                    &candidate.path_terms,
                    &projection.file_stem,
                    &candidate.file_stem,
                );
                let same_stem =
                    projection.file_stem == candidate.file_stem && !projection.file_stem.is_empty();
                if shared_terms.is_empty() && !same_stem {
                    return None;
                }
                Some(PathRelationProjection {
                    src_path: projection.path.clone(),
                    dst_path: candidate.path.clone(),
                    relation_kind: "companion_surface".to_owned(),
                    evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_PATH.to_owned(),
                    src_symbol_id: None,
                    dst_symbol_id: None,
                    src_family_bits: family_bits_for_projection(projection),
                    dst_family_bits: family_bits_for_projection(candidate),
                    shared_terms,
                    score_hint: 80 + usize::from(same_stem) * 20,
                })
            })
            .collect::<Vec<_>>();
        relations.sort_by(|left, right| {
            right
                .score_hint
                .cmp(&left.score_hint)
                .then(left.dst_path.cmp(&right.dst_path))
        });
        relations.truncate(MAX_RELATIONS_PER_SOURCE);
        grouped_by_source.insert(projection.path.clone(), relations);
    }
    rows.extend(grouped_by_source.into_values().flatten());

    rows.sort_by(|left, right| {
        left.src_path
            .cmp(&right.src_path)
            .then(left.dst_path.cmp(&right.dst_path))
            .then(left.relation_kind.cmp(&right.relation_kind))
    });
    rows.dedup_by(|left, right| {
        left.src_path == right.src_path
            && left.dst_path == right.dst_path
            && left.relation_kind == right.relation_kind
    });
    rows
}

pub(crate) fn augment_path_relation_projection_records_with_ast_relation_evidence(
    workspace_root: &Path,
    absolute_manifest_paths: &[PathBuf],
    path_witness: &[StoredPathWitnessProjection],
    path_relations: &mut Vec<PathRelationProjection>,
) {
    let extracted = extract_symbols_for_paths(absolute_manifest_paths);
    if extracted.symbols.is_empty() {
        return;
    }

    let witness_by_path = path_witness
        .iter()
        .map(|projection| (projection.path.clone(), projection))
        .collect::<BTreeMap<_, _>>();
    let relative_symbols = extracted
        .symbols
        .into_iter()
        .filter_map(|symbol| {
            let relative_path = normalize_repository_relative_path(workspace_root, &symbol.path);
            witness_by_path
                .contains_key(&relative_path)
                .then_some((relative_path, symbol))
        })
        .collect::<Vec<_>>();
    if relative_symbols.is_empty() {
        return;
    }

    let mut symbols = Vec::with_capacity(relative_symbols.len());
    let mut symbol_index_by_stable_id = BTreeMap::<String, usize>::new();
    let mut relative_path_by_stable_id = BTreeMap::<String, String>::new();
    let mut file_symbols_by_path = BTreeMap::<String, Vec<SymbolDefinition>>::new();
    for (index, (relative_path, symbol)) in relative_symbols.iter().enumerate() {
        symbol_index_by_stable_id.insert(symbol.stable_id.clone(), index);
        relative_path_by_stable_id.insert(symbol.stable_id.clone(), relative_path.clone());
        file_symbols_by_path
            .entry(relative_path.clone())
            .or_default()
            .push(symbol.clone());
        symbols.push(symbol.clone());
    }

    let symbol_indices_by_name = php_symbol_indices_by_name(&symbols);
    let symbol_indices_by_lower_name = php_symbol_indices_by_lower_name(&symbols);
    let mut php_canonical_names_by_stable_id = BTreeMap::<String, String>::new();
    let mut php_evidence = Vec::new();
    let mut blade_evidence = Vec::new();

    for absolute_path in absolute_manifest_paths {
        let relative_path = normalize_repository_relative_path(workspace_root, absolute_path);
        if !witness_by_path.contains_key(&relative_path) {
            continue;
        }

        match SymbolLanguage::from_path(absolute_path) {
            Some(SymbolLanguage::Php) => {
                let Ok(source) = fs::read_to_string(absolute_path) else {
                    continue;
                };
                let file_symbols = file_symbols_by_path
                    .get(&relative_path)
                    .cloned()
                    .unwrap_or_default();
                let Ok(evidence) =
                    extract_php_source_evidence_from_source(absolute_path, &source, &file_symbols)
                else {
                    continue;
                };
                php_canonical_names_by_stable_id
                    .extend(evidence.canonical_names_by_stable_id.clone().into_iter());
                php_evidence.push(evidence);
            }
            Some(SymbolLanguage::Blade) => {
                let Ok(source) = fs::read_to_string(absolute_path) else {
                    continue;
                };
                let file_symbols = file_symbols_by_path
                    .get(&relative_path)
                    .cloned()
                    .unwrap_or_default();
                blade_evidence.push(extract_blade_source_evidence_from_source(
                    &source,
                    &file_symbols,
                ));
            }
            _ => {}
        }
    }

    let mut symbol_indices_by_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    for (stable_id, canonical_name) in &php_canonical_names_by_stable_id {
        let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
            continue;
        };
        symbol_indices_by_canonical_name
            .entry(canonical_name.clone())
            .or_default()
            .push(symbol_index);
        symbol_indices_by_lower_canonical_name
            .entry(canonical_name.to_ascii_lowercase())
            .or_default()
            .push(symbol_index);
    }

    for evidence in &php_evidence {
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_php_target_evidence_edges(
                &symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_canonical_name,
                &symbol_indices_by_lower_canonical_name,
                evidence,
            )
        {
            push_ast_relation_projection(
                path_relations,
                &symbols,
                &witness_by_path,
                &relative_path_by_stable_id,
                source_symbol_index,
                target_symbol_index,
                relation,
            );
        }
    }

    for evidence in &blade_evidence {
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_blade_relation_evidence_edges(
                &symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_name,
                &symbol_indices_by_lower_name,
                evidence,
            )
        {
            push_ast_relation_projection(
                path_relations,
                &symbols,
                &witness_by_path,
                &relative_path_by_stable_id,
                source_symbol_index,
                target_symbol_index,
                relation,
            );
        }
    }
}

fn push_ast_relation_projection(
    path_relations: &mut Vec<PathRelationProjection>,
    symbols: &[SymbolDefinition],
    witness_by_path: &BTreeMap<String, &StoredPathWitnessProjection>,
    relative_path_by_stable_id: &BTreeMap<String, String>,
    source_symbol_index: usize,
    target_symbol_index: usize,
    relation: RelationKind,
) {
    let Some(source_symbol) = symbols.get(source_symbol_index) else {
        return;
    };
    let Some(target_symbol) = symbols.get(target_symbol_index) else {
        return;
    };
    let Some(src_path) = relative_path_by_stable_id
        .get(&source_symbol.stable_id)
        .cloned()
    else {
        return;
    };
    let Some(dst_path) = relative_path_by_stable_id
        .get(&target_symbol.stable_id)
        .cloned()
    else {
        return;
    };
    if src_path == dst_path {
        return;
    }
    let Some(src_projection) = witness_by_path.get(&src_path) else {
        return;
    };
    let Some(dst_projection) = witness_by_path.get(&dst_path) else {
        return;
    };

    path_relations.push(PathRelationProjection {
        src_path,
        dst_path,
        relation_kind: relation.as_str().to_owned(),
        evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned(),
        src_symbol_id: Some(source_symbol.stable_id.clone()),
        dst_symbol_id: Some(target_symbol.stable_id.clone()),
        src_family_bits: family_bits_for_projection(src_projection),
        dst_family_bits: family_bits_for_projection(dst_projection),
        shared_terms: symbol_projection_terms(&target_symbol.name),
        score_hint: ast_relation_score_hint(relation),
    });
}

fn ast_relation_score_hint(relation: RelationKind) -> usize {
    match relation {
        RelationKind::Calls => 116,
        RelationKind::RefersTo => 112,
        RelationKind::Implements | RelationKind::Extends => 108,
        RelationKind::Contains => 104,
        RelationKind::DefinedIn => 100,
    }
}

pub(crate) fn build_subtree_coverage_projection_records(
    path_witness: &[StoredPathWitnessProjection],
) -> Vec<SubtreeCoverageProjection> {
    let mut grouped = BTreeMap::<(String, String), Vec<&StoredPathWitnessProjection>>::new();
    for projection in path_witness {
        let Some(subtree_root) = projection.subtree_root.clone() else {
            continue;
        };
        for family in generic_surface_families_for_projection(projection) {
            grouped
                .entry((subtree_root.clone(), family_name(family).to_owned()))
                .or_default()
                .push(projection);
        }
    }

    let mut rows = grouped
        .into_iter()
        .map(|((subtree_root, family), mut projections)| {
            projections.sort_by(|left, right| {
                projection_score_hint(right)
                    .cmp(&projection_score_hint(left))
                    .then(left.path.cmp(&right.path))
            });
            let exemplar = projections[0];
            SubtreeCoverageProjection {
                subtree_root,
                family,
                path_count: projections.len(),
                exemplar_path: exemplar.path.clone(),
                exemplar_score_hint: projection_score_hint(exemplar),
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.subtree_root
            .cmp(&right.subtree_root)
            .then(left.family.cmp(&right.family))
    });
    rows
}

pub(crate) fn build_path_surface_term_projection_records(
    path_witness: &[StoredPathWitnessProjection],
    entrypoint_surface: &[StoredEntrypointSurfaceProjection],
) -> Vec<PathSurfaceTermProjection> {
    let entrypoint_by_path = entrypoint_surface
        .iter()
        .map(|projection| (projection.path.as_str(), projection))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();

    for projection in path_witness {
        let mut term_weights = BTreeMap::<String, u16>::new();
        let mut exact_terms = BTreeSet::<String>::new();
        for term in &projection.path_terms {
            push_weighted_term(&mut term_weights, term, 3);
            exact_terms.insert(term.clone());
        }
        if !projection.file_stem.is_empty() {
            push_weighted_term(&mut term_weights, &projection.file_stem, 4);
            exact_terms.insert(projection.file_stem.clone());
        }
        for alias in family_aliases(projection) {
            push_weighted_term(&mut term_weights, alias, 2);
            exact_terms.insert((*alias).to_owned());
        }
        if let Some(entrypoint) = entrypoint_by_path.get(projection.path.as_str()) {
            for term in &entrypoint.surface_terms {
                push_weighted_term(&mut term_weights, term, 3);
                exact_terms.insert(term.clone());
            }
        }

        while term_weights.len() > MAX_SURFACE_TERMS_PER_PATH {
            let Some(weakest_key) = term_weights
                .iter()
                .min_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
                .map(|(term, _)| term.clone())
            else {
                break;
            };
            term_weights.remove(&weakest_key);
            exact_terms.remove(&weakest_key);
        }

        rows.push(PathSurfaceTermProjection {
            path: projection.path.clone(),
            term_weights,
            exact_terms: exact_terms.into_iter().collect(),
        });
    }

    rows.sort_by(|left, right| left.path.cmp(&right.path));
    rows
}

pub(crate) fn build_path_anchor_sketch_projection_records(
    workspace_root: &Path,
    path_witness: &[StoredPathWitnessProjection],
    path_surface_terms: &[PathSurfaceTermProjection],
) -> Vec<PathAnchorSketchProjection> {
    let surface_terms_by_path = path_surface_terms
        .iter()
        .map(|projection| (projection.path.as_str(), projection))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();

    for projection in path_witness {
        let Some(surface_terms) = surface_terms_by_path.get(projection.path.as_str()) else {
            continue;
        };
        let file_path = workspace_root.join(&projection.path);
        let Ok(contents) = fs::read_to_string(&file_path) else {
            continue;
        };
        let ranked = contents
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }
                let lower = trimmed.to_ascii_lowercase();
                let matched_terms = surface_terms
                    .exact_terms
                    .iter()
                    .filter(|term| lower.contains(term.as_str()))
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>();
                let mut score = matched_terms.len() * 6;
                if !projection.file_stem.is_empty() && lower.contains(&projection.file_stem) {
                    score += 10;
                }
                if score == 0 && index > 0 {
                    return None;
                }
                Some((
                    score.max(1),
                    index + 1,
                    trim_excerpt(trimmed),
                    matched_terms,
                ))
            })
            .collect::<Vec<_>>();
        let mut ranked = ranked;
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
        });
        ranked.truncate(MAX_ANCHOR_SKETCHES_PER_PATH);
        for (anchor_rank, (score_hint, line, excerpt, matched_terms)) in
            ranked.into_iter().enumerate()
        {
            rows.push(PathAnchorSketchProjection {
                path: projection.path.clone(),
                anchor_rank,
                line,
                anchor_kind: "line_excerpt".to_owned(),
                excerpt,
                terms: matched_terms,
                score_hint,
            });
        }
    }

    rows.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.anchor_rank.cmp(&right.anchor_rank))
    });
    rows
}

fn relation_kind_for_entrypoint_pair(
    candidate: &StoredPathWitnessProjection,
) -> Option<&'static str> {
    if candidate.flags.is_entrypoint_build_workflow {
        return Some("entrypoint_workflow");
    }
    if candidate.flags.is_runtime_config_artifact || candidate.flags.is_build_config_surface {
        return Some("entrypoint_config");
    }
    if candidate.flags.is_package_surface {
        return Some("entrypoint_package");
    }
    if candidate.flags.is_workspace_config_surface {
        return Some("entrypoint_workspace");
    }
    if candidate.flags.is_runtime_companion_surface {
        return Some("companion_surface");
    }
    None
}

fn shared_terms_between(
    left_terms: &[String],
    right_terms: &[String],
    left_file_stem: &str,
    right_file_stem: &str,
) -> Vec<String> {
    let mut shared = left_terms
        .iter()
        .filter(|term| right_terms.iter().any(|candidate| candidate == *term))
        .cloned()
        .collect::<BTreeSet<_>>();
    if !left_file_stem.is_empty() && left_file_stem == right_file_stem {
        shared.insert(left_file_stem.to_owned());
    }
    shared.into_iter().take(8).collect()
}

fn push_weighted_term(term_weights: &mut BTreeMap<String, u16>, term: &str, weight: u16) {
    if term.is_empty() {
        return;
    }
    *term_weights.entry(term.to_owned()).or_insert(0) = term_weights
        .get(term)
        .copied()
        .unwrap_or_default()
        .saturating_add(weight);
}

fn family_aliases(projection: &StoredPathWitnessProjection) -> &'static [&'static str] {
    if projection.flags.is_entrypoint_runtime {
        return &["entrypoint", "main", "bootstrap", "startup"];
    }
    if projection.flags.is_runtime_companion_surface {
        return &["runtime", "service", "server"];
    }
    if projection.flags.is_package_surface {
        return &["package", "manifest", "dependency"];
    }
    if projection.flags.is_build_config_surface || projection.flags.is_entrypoint_build_workflow {
        return &["build", "config", "workflow"];
    }
    if projection.flags.is_workspace_config_surface {
        return &["workspace", "tooling", "monorepo"];
    }
    if projection.flags.is_test_support || projection.flags.is_test_harness {
        return &["test", "tests", "spec", "integration"];
    }
    &[]
}

fn family_name(family: GenericWitnessSurfaceFamily) -> &'static str {
    match family {
        GenericWitnessSurfaceFamily::Runtime => "runtime",
        GenericWitnessSurfaceFamily::Tests => "tests",
        GenericWitnessSurfaceFamily::PackageSurface => "package_surface",
        GenericWitnessSurfaceFamily::BuildConfig => "build_config",
        GenericWitnessSurfaceFamily::Entrypoint => "entrypoint",
        GenericWitnessSurfaceFamily::WorkspaceConfig => "workspace_config",
    }
}

fn projection_score_hint(projection: &StoredPathWitnessProjection) -> usize {
    let mut score = projection.path_terms.len() * 4;
    if projection.flags.is_entrypoint_runtime {
        score += 24;
    }
    if projection.flags.is_runtime_companion_surface {
        score += 16;
    }
    if projection.flags.is_test_support || projection.flags.is_test_harness {
        score += 14;
    }
    if projection.flags.is_package_surface {
        score += 12;
    }
    if projection.flags.is_build_config_surface || projection.flags.is_entrypoint_build_workflow {
        score += 10;
    }
    if projection.flags.is_workspace_config_surface {
        score += 8;
    }
    score
}

fn trim_excerpt(line: &str) -> String {
    const MAX_CHARS: usize = 160;
    if line.chars().count() <= MAX_CHARS {
        return line.to_owned();
    }
    let mut trimmed = line.chars().take(MAX_CHARS).collect::<String>();
    trimmed.push_str("...");
    trimmed
}

fn apply_ast_projection_contributions(
    workspace_root: &Path,
    absolute_manifest_paths: &[PathBuf],
    path_witness: &[StoredPathWitnessProjection],
    path_relations: &mut Vec<PathRelationProjection>,
    path_surface_terms: &mut Vec<PathSurfaceTermProjection>,
    path_anchor_sketches: &mut Vec<PathAnchorSketchProjection>,
    input_modes: &mut RetrievalProjectionInputModes,
) {
    let extracted = extract_symbols_for_paths(absolute_manifest_paths);
    if extracted.symbols.is_empty() {
        return;
    }

    let witness_by_path = path_witness
        .iter()
        .map(|projection| (projection.path.clone(), projection))
        .collect::<BTreeMap<_, _>>();
    let mut relative_symbols = extracted
        .symbols
        .into_iter()
        .filter_map(|symbol| {
            let relative_path = normalize_repository_relative_path(workspace_root, &symbol.path);
            witness_by_path
                .contains_key(&relative_path)
                .then_some((relative_path, symbol))
        })
        .collect::<Vec<_>>();
    if relative_symbols.is_empty() {
        return;
    }

    let mut anchor_candidates = path_anchor_sketches
        .drain(..)
        .map(|projection| (projection.path.clone(), projection))
        .fold(
            BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new(),
            |mut acc, (path, row)| {
                acc.entry(path).or_default().push(row);
                acc
            },
        );

    for (relative_path, symbol) in &relative_symbols {
        let Some(witness_projection) = witness_by_path.get(relative_path) else {
            continue;
        };
        let mut symbol_terms = symbol_projection_terms(&symbol.name);
        symbol_terms.extend(symbol_kind_projection_terms(symbol, witness_projection));
        symbol_terms.sort();
        symbol_terms.dedup();
        if symbol_terms.is_empty() {
            continue;
        }

        if let Some(projection) =
            find_path_surface_term_projection_mut(path_surface_terms, relative_path)
        {
            for term in &symbol_terms {
                push_weighted_term(&mut projection.term_weights, term, 2);
            }
            projection.exact_terms.extend(symbol_terms.iter().cloned());
            if !symbol.kind.as_str().is_empty() {
                push_weighted_term(&mut projection.term_weights, symbol.kind.as_str(), 1);
                projection.exact_terms.push(symbol.kind.as_str().to_owned());
            }
            input_modes
                .path_surface_term
                .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
        }

        anchor_candidates
            .entry(relative_path.clone())
            .or_default()
            .push(PathAnchorSketchProjection {
                path: relative_path.clone(),
                anchor_rank: 0,
                line: symbol.line.max(1),
                anchor_kind: ast_anchor_kind(symbol, witness_projection).to_owned(),
                excerpt: trim_excerpt(&format!("{} {}", symbol.kind.as_str(), symbol.name)),
                terms: symbol_terms.clone(),
                score_hint: 48,
            });
        input_modes
            .path_anchor_sketch
            .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
    }

    relative_symbols.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.line.cmp(&right.1.line))
            .then(left.1.name.cmp(&right.1.name))
            .then(left.1.stable_id.cmp(&right.1.stable_id))
    });
    let mut symbols_by_name = BTreeMap::<String, Vec<(String, SymbolDefinition)>>::new();
    for (relative_path, symbol) in relative_symbols {
        let normalized_name = symbol.name.trim().to_ascii_lowercase();
        if normalized_name.is_empty() {
            continue;
        }
        symbols_by_name
            .entry(normalized_name)
            .or_default()
            .push((relative_path, symbol));
    }

    for symbols in symbols_by_name.into_values() {
        if symbols.len() < 2 {
            continue;
        }
        let shared_terms = symbol_projection_terms(&symbols[0].1.name);
        for (relative_path, symbol) in &symbols {
            let Some(src_projection) = witness_by_path.get(relative_path) else {
                continue;
            };
            let Some(src_subtree_root) = src_projection.subtree_root.as_deref() else {
                continue;
            };
            let mut per_source = Vec::new();
            for (candidate_path, candidate_symbol) in &symbols {
                if candidate_path == relative_path {
                    continue;
                }
                let Some(dst_projection) = witness_by_path.get(candidate_path) else {
                    continue;
                };
                if dst_projection.subtree_root.as_deref() != Some(src_subtree_root) {
                    continue;
                }
                per_source.push(PathRelationProjection {
                    src_path: relative_path.clone(),
                    dst_path: candidate_path.clone(),
                    relation_kind: "symbol_overlap".to_owned(),
                    evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned(),
                    src_symbol_id: Some(symbol.stable_id.clone()),
                    dst_symbol_id: Some(candidate_symbol.stable_id.clone()),
                    src_family_bits: family_bits_for_projection(src_projection),
                    dst_family_bits: family_bits_for_projection(dst_projection),
                    shared_terms: shared_terms.clone(),
                    score_hint: 96 + usize::from(symbol.language == candidate_symbol.language) * 8,
                });
            }
            per_source.sort_by(|left, right| {
                right
                    .score_hint
                    .cmp(&left.score_hint)
                    .then(left.dst_path.cmp(&right.dst_path))
                    .then(left.dst_symbol_id.cmp(&right.dst_symbol_id))
            });
            per_source.truncate(MAX_RELATIONS_PER_SOURCE);
            if !per_source.is_empty() {
                path_relations.extend(per_source);
                input_modes
                    .path_relation
                    .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
            }
        }
    }

    *path_anchor_sketches = anchor_candidates.into_values().flatten().collect();
}

fn apply_scip_projection_contributions(
    repository_id: &str,
    workspace_root: &Path,
    path_witness: &[StoredPathWitnessProjection],
    path_relations: &mut Vec<PathRelationProjection>,
    path_surface_terms: &mut Vec<PathSurfaceTermProjection>,
    path_anchor_sketches: &mut Vec<PathAnchorSketchProjection>,
    input_modes: &mut RetrievalProjectionInputModes,
) {
    let scip_artifacts = collect_scip_artifact_inputs(workspace_root);
    if scip_artifacts.is_empty() {
        return;
    }

    let witness_by_path = path_witness
        .iter()
        .map(|projection| (projection.path.clone(), projection))
        .collect::<BTreeMap<_, _>>();
    let mut graph = SymbolGraph::default();
    let mut processed_total_bytes = 0usize;
    let mut ingested = false;

    for artifact in scip_artifacts.into_iter().take(MAX_SCIP_ARTIFACTS) {
        let Ok(payload) = fs::read(&artifact.path) else {
            continue;
        };
        if payload.len() > MAX_SCIP_ARTIFACT_BYTES {
            continue;
        }
        if processed_total_bytes.saturating_add(payload.len()) > MAX_SCIP_TOTAL_BYTES {
            break;
        }
        processed_total_bytes = processed_total_bytes.saturating_add(payload.len());
        let budgets = ScipResourceBudgets {
            max_payload_bytes: MAX_SCIP_ARTIFACT_BYTES,
            max_documents: MAX_SCIP_DOCUMENTS_PER_ARTIFACT,
            max_elapsed_ms: MAX_SCIP_ELAPSED_MS,
        };
        let result = match artifact.format {
            ScipArtifactInputFormat::Json => graph.overlay_scip_json_with_budgets(
                repository_id,
                &artifact.label,
                &payload,
                budgets,
            ),
            ScipArtifactInputFormat::Protobuf => graph.overlay_scip_protobuf_with_budgets(
                repository_id,
                &artifact.label,
                &payload,
                budgets,
            ),
        };
        if result.is_ok() {
            ingested = true;
        }
    }

    if !ingested {
        return;
    }

    let mut anchor_candidates = path_anchor_sketches
        .drain(..)
        .map(|projection| (projection.path.clone(), projection))
        .fold(
            BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new(),
            |mut acc, (path, row)| {
                acc.entry(path).or_default().push(row);
                acc
            },
        );

    let definitions_by_symbol = graph
        .precise_symbols_for_repository(repository_id)
        .into_iter()
        .filter_map(|symbol| {
            let definition =
                graph.precise_definition_occurrence_for_symbol(repository_id, &symbol.symbol)?;
            witness_by_path.get(&definition.path).map(|projection| {
                (
                    symbol.symbol.clone(),
                    (symbol, definition, family_bits_for_projection(projection)),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();

    for (symbol, definition, _) in definitions_by_symbol.values() {
        let symbol_terms = symbol_projection_terms(&symbol.display_name);
        if symbol_terms.is_empty() {
            continue;
        }
        if let Some(projection) =
            find_path_surface_term_projection_mut(path_surface_terms, &definition.path)
        {
            for term in &symbol_terms {
                push_weighted_term(&mut projection.term_weights, term, 3);
            }
            projection.exact_terms.extend(symbol_terms.iter().cloned());
            if !symbol.kind.is_empty() {
                push_weighted_term(&mut projection.term_weights, &symbol.kind, 1);
                projection.exact_terms.push(symbol.kind.clone());
            }
            input_modes
                .path_surface_term
                .insert(RETRIEVAL_PROJECTION_INPUT_MODE_SCIP.to_owned());
        }
        anchor_candidates
            .entry(definition.path.clone())
            .or_default()
            .push(PathAnchorSketchProjection {
                path: definition.path.clone(),
                anchor_rank: 0,
                line: definition.range.start_line.max(1),
                anchor_kind: "scip_definition".to_owned(),
                excerpt: trim_excerpt(&format!("{} {}", symbol.kind, symbol.display_name)),
                terms: symbol_terms,
                score_hint: 64,
            });
        input_modes
            .path_anchor_sketch
            .insert(RETRIEVAL_PROJECTION_INPUT_MODE_SCIP.to_owned());
    }

    for (symbol_id, (_, definition, src_family_bits)) in &definitions_by_symbol {
        for reference in graph.precise_references_for_symbol(repository_id, symbol_id) {
            let Some(dst_projection) = witness_by_path.get(&reference.path) else {
                continue;
            };
            if reference.path == definition.path {
                continue;
            }
            path_relations.push(PathRelationProjection {
                src_path: definition.path.clone(),
                dst_path: reference.path.clone(),
                relation_kind: "symbol_reference".to_owned(),
                evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_SCIP.to_owned(),
                src_symbol_id: Some(symbol_id.clone()),
                dst_symbol_id: None,
                src_family_bits: *src_family_bits,
                dst_family_bits: family_bits_for_projection(dst_projection),
                shared_terms: Vec::new(),
                score_hint: 120,
            });
            input_modes
                .path_relation
                .insert(RETRIEVAL_PROJECTION_INPUT_MODE_SCIP.to_owned());
        }

        for relation in graph.precise_relationships_from_symbol(repository_id, symbol_id) {
            let Some((_, target_definition, dst_family_bits)) =
                definitions_by_symbol.get(&relation.to_symbol)
            else {
                continue;
            };
            if target_definition.path == definition.path {
                continue;
            }
            path_relations.push(PathRelationProjection {
                src_path: definition.path.clone(),
                dst_path: target_definition.path.clone(),
                relation_kind: precise_relation_kind_name(relation.kind).to_owned(),
                evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_SCIP.to_owned(),
                src_symbol_id: Some(relation.from_symbol.clone()),
                dst_symbol_id: Some(relation.to_symbol.clone()),
                src_family_bits: *src_family_bits,
                dst_family_bits: *dst_family_bits,
                shared_terms: Vec::new(),
                score_hint: precise_relation_score_hint(relation.kind),
            });
            input_modes
                .path_relation
                .insert(RETRIEVAL_PROJECTION_INPUT_MODE_SCIP.to_owned());
        }
    }

    *path_anchor_sketches = anchor_candidates.into_values().flatten().collect();
}

pub(crate) fn normalize_path_relation_projection_records(rows: &mut Vec<PathRelationProjection>) {
    rows.sort_by(|left, right| {
        left.src_path
            .cmp(&right.src_path)
            .then(left.dst_path.cmp(&right.dst_path))
            .then(left.relation_kind.cmp(&right.relation_kind))
            .then(left.evidence_source.cmp(&right.evidence_source))
            .then(right.score_hint.cmp(&left.score_hint))
            .then(left.src_symbol_id.cmp(&right.src_symbol_id))
            .then(left.dst_symbol_id.cmp(&right.dst_symbol_id))
    });
    rows.dedup_by(|left, right| {
        left.src_path == right.src_path
            && left.dst_path == right.dst_path
            && left.relation_kind == right.relation_kind
            && left.evidence_source == right.evidence_source
            && left.src_symbol_id == right.src_symbol_id
            && left.dst_symbol_id == right.dst_symbol_id
    });

    let mut bounded = Vec::new();
    let mut current_src = None::<String>;
    let mut current_group = Vec::<PathRelationProjection>::new();
    for row in std::mem::take(rows) {
        if current_src.as_deref() != Some(row.src_path.as_str()) {
            flush_bounded_relations(&mut bounded, &mut current_group);
            current_src = Some(row.src_path.clone());
        }
        current_group.push(row);
    }
    flush_bounded_relations(&mut bounded, &mut current_group);
    bounded.sort_by(|left, right| {
        left.src_path
            .cmp(&right.src_path)
            .then(left.dst_path.cmp(&right.dst_path))
            .then(left.relation_kind.cmp(&right.relation_kind))
            .then(left.evidence_source.cmp(&right.evidence_source))
    });
    *rows = bounded;
}

fn flush_bounded_relations(
    output: &mut Vec<PathRelationProjection>,
    group: &mut Vec<PathRelationProjection>,
) {
    if group.is_empty() {
        return;
    }
    group.sort_by(|left, right| {
        right
            .score_hint
            .cmp(&left.score_hint)
            .then(left.dst_path.cmp(&right.dst_path))
            .then(left.relation_kind.cmp(&right.relation_kind))
            .then(left.evidence_source.cmp(&right.evidence_source))
    });
    group.truncate(MAX_RELATIONS_PER_SOURCE);
    output.append(group);
}

fn normalize_path_surface_term_projection_records(rows: &mut Vec<PathSurfaceTermProjection>) {
    for row in rows.iter_mut() {
        while row.term_weights.len() > MAX_SURFACE_TERMS_PER_PATH {
            let Some(weakest_key) = row
                .term_weights
                .iter()
                .min_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
                .map(|(term, _)| term.clone())
            else {
                break;
            };
            row.term_weights.remove(&weakest_key);
        }
        row.exact_terms.sort();
        row.exact_terms.dedup();
        row.exact_terms
            .retain(|term| row.term_weights.contains_key(term) || !term.is_empty());
    }
    rows.sort_by(|left, right| left.path.cmp(&right.path));
}

fn normalize_path_anchor_sketch_projection_records(rows: &mut Vec<PathAnchorSketchProjection>) {
    let mut grouped = BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new();
    for row in std::mem::take(rows) {
        grouped.entry(row.path.clone()).or_default().push(row);
    }

    let mut normalized = Vec::new();
    for (path, mut group) in grouped {
        group.sort_by(|left, right| {
            right
                .score_hint
                .cmp(&left.score_hint)
                .then(left.line.cmp(&right.line))
                .then(left.anchor_kind.cmp(&right.anchor_kind))
                .then(left.excerpt.cmp(&right.excerpt))
        });
        group.dedup_by(|left, right| {
            left.line == right.line
                && left.anchor_kind == right.anchor_kind
                && left.excerpt == right.excerpt
        });
        for (anchor_rank, mut row) in group
            .into_iter()
            .take(MAX_ANCHOR_SKETCHES_PER_PATH)
            .enumerate()
        {
            row.path = path.clone();
            row.anchor_rank = anchor_rank;
            row.terms.sort();
            row.terms.dedup();
            normalized.push(row);
        }
    }

    normalized.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.anchor_rank.cmp(&right.anchor_rank))
    });
    *rows = normalized;
}

fn symbol_projection_terms(name: &str) -> Vec<String> {
    let mut terms = hybrid_query_exact_terms(name);
    let normalized_name = name.trim().to_ascii_lowercase();
    if !normalized_name.is_empty() && !terms.iter().any(|term| term == &normalized_name) {
        terms.push(normalized_name);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn symbol_kind_projection_terms(
    symbol: &SymbolDefinition,
    witness_projection: &StoredPathWitnessProjection,
) -> Vec<String> {
    let mut terms = Vec::new();
    match symbol.kind {
        SymbolKind::Module => terms.push("module".to_owned()),
        SymbolKind::Component | SymbolKind::Section | SymbolKind::Slot => {
            terms.push("component".to_owned());
        }
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::EnumCase
        | SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::PhpEnum
        | SymbolKind::TypeAlias => {
            terms.push("type".to_owned());
        }
        SymbolKind::Trait | SymbolKind::PhpTrait => {
            terms.push("trait".to_owned());
            terms.push("interface".to_owned());
        }
        SymbolKind::Impl => {
            terms.push("impl".to_owned());
            terms.push("implementation".to_owned());
        }
        SymbolKind::Function => terms.push("function".to_owned()),
        SymbolKind::Method => {
            terms.push("function".to_owned());
            terms.push("method".to_owned());
        }
        SymbolKind::Property => {
            terms.push("property".to_owned());
            terms.push("field".to_owned());
        }
        SymbolKind::Const | SymbolKind::Static | SymbolKind::Constant => {
            terms.push("constant".to_owned());
        }
    }

    if symbol.name.eq_ignore_ascii_case("main") {
        terms.push("entrypoint".to_owned());
    }
    if witness_projection.flags.is_entrypoint_runtime {
        terms.push("runtime".to_owned());
    }
    if witness_projection.flags.is_test_support
        || witness_projection.flags.is_test_harness
        || symbol.name.to_ascii_lowercase().starts_with("test")
    {
        terms.push("test".to_owned());
    }

    terms.sort();
    terms.dedup();
    terms
}

fn ast_anchor_kind(
    symbol: &SymbolDefinition,
    witness_projection: &StoredPathWitnessProjection,
) -> &'static str {
    if symbol.name.eq_ignore_ascii_case("main") || witness_projection.flags.is_entrypoint_runtime {
        return "ast_entrypoint";
    }
    if witness_projection.flags.is_test_support || witness_projection.flags.is_test_harness {
        return "ast_test_symbol";
    }
    match symbol.kind {
        SymbolKind::Trait | SymbolKind::PhpTrait => "ast_trait",
        SymbolKind::Impl => "ast_impl",
        SymbolKind::Module => "ast_module",
        SymbolKind::Function => "ast_function",
        SymbolKind::Method => "ast_method",
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::TypeAlias
        | SymbolKind::PhpEnum => "ast_type",
        _ => "ast_symbol",
    }
}

fn find_path_surface_term_projection_mut<'a>(
    rows: &'a mut [PathSurfaceTermProjection],
    path: &str,
) -> Option<&'a mut PathSurfaceTermProjection> {
    rows.iter_mut().find(|projection| projection.path == path)
}

fn precise_relation_kind_name(kind: PreciseRelationshipKind) -> &'static str {
    match kind {
        PreciseRelationshipKind::Definition => "symbol_definition",
        PreciseRelationshipKind::Reference => "symbol_reference",
        PreciseRelationshipKind::Implementation => "symbol_implementation",
        PreciseRelationshipKind::TypeDefinition => "symbol_type_definition",
    }
}

fn precise_relation_score_hint(kind: PreciseRelationshipKind) -> usize {
    match kind {
        PreciseRelationshipKind::Implementation => 132,
        PreciseRelationshipKind::Reference => 124,
        PreciseRelationshipKind::TypeDefinition => 120,
        PreciseRelationshipKind::Definition => 112,
    }
}

fn collect_scip_artifact_inputs(workspace_root: &Path) -> Vec<ScipArtifactInput> {
    let scip_root = workspace_root.join(".frigg/scip");
    let Ok(read_dir) = fs::read_dir(&scip_root) else {
        return Vec::new();
    };
    let mut artifacts = read_dir
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let format = match path.extension().and_then(|ext| ext.to_str()) {
                Some("json") => ScipArtifactInputFormat::Json,
                Some("scip") => ScipArtifactInputFormat::Protobuf,
                _ => return None,
            };
            Some(ScipArtifactInput {
                label: format!("reindex:{}", path.display()),
                path,
                format,
            })
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    artifacts
}
