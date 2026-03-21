use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::graph::{ScipResourceBudgets, SymbolGraph};
use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection,
};

use super::super::path_witness_projection::{
    StoredPathWitnessProjection, family_bits_for_projection,
};
use super::RETRIEVAL_PROJECTION_INPUT_MODE_SCIP;
use super::builders::{push_weighted_term, trim_excerpt};

const MAX_SCIP_ARTIFACTS: usize = 16;
const MAX_SCIP_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SCIP_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCIP_DOCUMENTS_PER_ARTIFACT: usize = 2_048;
const MAX_SCIP_ELAPSED_MS: u64 = 5_000;

#[derive(Debug, Clone)]
struct ScipArtifactInput {
    path: std::path::PathBuf,
    label: String,
    format: ScipArtifactInputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScipArtifactInputFormat {
    Json,
    Protobuf,
}

#[allow(clippy::ptr_arg)]
pub(super) fn apply_scip_projection_contributions(
    repository_id: &str,
    workspace_root: &Path,
    path_witness: &[StoredPathWitnessProjection],
    path_relations: &mut Vec<PathRelationProjection>,
    path_surface_terms: &mut Vec<PathSurfaceTermProjection>,
    path_anchor_sketches: &mut Vec<PathAnchorSketchProjection>,
    input_modes: &mut super::bundle::RetrievalProjectionInputModes,
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

fn symbol_projection_terms(name: &str) -> Vec<String> {
    let mut terms = super::super::query_terms::hybrid_query_exact_terms(name);
    let normalized_name = name.trim().to_ascii_lowercase();
    if !normalized_name.is_empty() && !terms.iter().any(|term| term == &normalized_name) {
        terms.push(normalized_name);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn find_path_surface_term_projection_mut<'a>(
    rows: &'a mut [PathSurfaceTermProjection],
    path: &str,
) -> Option<&'a mut PathSurfaceTermProjection> {
    rows.iter_mut().find(|projection| projection.path == path)
}

fn precise_relation_kind_name(kind: crate::graph::PreciseRelationshipKind) -> &'static str {
    match kind {
        crate::graph::PreciseRelationshipKind::Definition => "symbol_definition",
        crate::graph::PreciseRelationshipKind::Reference => "symbol_reference",
        crate::graph::PreciseRelationshipKind::Implementation => "symbol_implementation",
        crate::graph::PreciseRelationshipKind::TypeDefinition => "symbol_type_definition",
    }
}

fn precise_relation_score_hint(kind: crate::graph::PreciseRelationshipKind) -> usize {
    match kind {
        crate::graph::PreciseRelationshipKind::Implementation => 132,
        crate::graph::PreciseRelationshipKind::Reference => 124,
        crate::graph::PreciseRelationshipKind::TypeDefinition => 120,
        crate::graph::PreciseRelationshipKind::Definition => 112,
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
