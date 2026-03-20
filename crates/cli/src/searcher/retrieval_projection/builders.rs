use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection,
    SubtreeCoverageProjection,
};

use super::super::overlay_projection::{
    StoredEntrypointSurfaceProjection, StoredTestSubjectProjection,
};
use super::super::path_witness_projection::{
    GenericWitnessSurfaceFamily, StoredPathWitnessProjection, family_bits_for_projection,
    generic_surface_families_for_projection,
};
use super::RETRIEVAL_PROJECTION_INPUT_MODE_PATH;

const MAX_RELATIONS_PER_SOURCE: usize = 8;
const MAX_ANCHOR_SKETCHES_PER_PATH: usize = 3;
const MAX_SURFACE_TERMS_PER_PATH: usize = 24;

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
                exact_terms.insert(term.to_owned());
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

pub(crate) fn push_weighted_term(
    term_weights: &mut BTreeMap<String, u16>,
    term: &str,
    weight: u16,
) {
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

pub(crate) fn trim_excerpt(line: &str) -> String {
    const MAX_CHARS: usize = 160;
    if line.chars().count() <= MAX_CHARS {
        return line.to_owned();
    }
    let mut trimmed = line.chars().take(MAX_CHARS).collect::<String>();
    trimmed.push_str("...");
    trimmed
}
