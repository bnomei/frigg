use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use crate::languages::SymbolLanguage;
use crate::path_class::classify_repository_path;
use crate::storage::{
    EntrypointSurfaceProjection, PathRelationProjection, PathSurfaceTermProjection,
    TestSubjectProjection,
};
use serde::{Deserialize, Serialize};

use super::path_witness_projection::{
    GenericWitnessSurfaceFamily, StoredPathWitnessProjection,
    generic_surface_families_for_projection, generic_surface_families_from_bits,
};
use super::{
    HybridPathWitnessQueryContext, HybridRankingIntent, hybrid_identifier_tokens,
    hybrid_overlap_count, hybrid_path_overlap_tokens, hybrid_source_class, is_ci_workflow_path,
    is_entrypoint_build_workflow_path, is_entrypoint_runtime_path, is_runtime_config_artifact_path,
    is_scripts_ops_path, is_test_harness_path, is_test_support_path,
};

const MAX_TEST_SUBJECT_LINKS_PER_TEST: usize = 6;
const MAX_TEST_SUBJECT_LINKS_PER_SUBJECT: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct PathOverlayBoost {
    pub(super) bonus_millis: u32,
    pub(super) provenance_ids: Vec<String>,
}

impl PathOverlayBoost {
    pub(super) fn merge(&mut self, other: Self) {
        self.bonus_millis = self.bonus_millis.saturating_add(other.bonus_millis);
        self.provenance_ids.extend(other.provenance_ids);
        self.provenance_ids.sort();
        self.provenance_ids.dedup();
    }

    pub(super) fn bonus_score(&self) -> f32 {
        self.bonus_millis as f32 / 1000.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(super) struct TestSubjectProjectionFlags {
    pub(super) exact_stem_match: bool,
    pub(super) same_language: bool,
    pub(super) runtime_subject: bool,
    pub(super) support_subject: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredTestSubjectProjection {
    pub(super) test_path: String,
    pub(super) subject_path: String,
    pub(super) shared_terms: Vec<String>,
    pub(super) score_hint: usize,
    pub(super) flags: TestSubjectProjectionFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(super) struct EntrypointSurfaceProjectionFlags {
    pub(super) is_runtime_entrypoint: bool,
    pub(super) is_build_workflow: bool,
    pub(super) is_runtime_config_artifact: bool,
    pub(super) is_ci_workflow: bool,
    pub(super) is_scripts_ops: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredEntrypointSurfaceProjection {
    pub(super) path: String,
    pub(super) path_class: PathClass,
    pub(super) source_class: SourceClass,
    pub(super) path_terms: Vec<String>,
    pub(super) surface_terms: Vec<String>,
    pub(super) flags: EntrypointSurfaceProjectionFlags,
}

pub(crate) fn build_test_subject_projection_records(
    paths: &[String],
) -> FriggResult<Vec<TestSubjectProjection>> {
    let candidates = paths
        .iter()
        .map(|path| TestSubjectCandidate::from_path(path))
        .collect::<Vec<_>>();
    let test_candidates = candidates
        .iter()
        .filter(|candidate| candidate.is_testish)
        .collect::<Vec<_>>();
    let subject_candidates = candidates
        .iter()
        .filter(|candidate| candidate.is_subjectish)
        .collect::<Vec<_>>();

    let mut by_subject = BTreeMap::<String, Vec<StoredTestSubjectProjection>>::new();
    for test_candidate in test_candidates {
        let mut subject_links = subject_candidates
            .iter()
            .filter_map(|subject_candidate| {
                build_test_subject_projection_from_candidates(test_candidate, subject_candidate)
            })
            .collect::<Vec<_>>();
        subject_links.sort_by(test_subject_projection_order);
        subject_links.truncate(MAX_TEST_SUBJECT_LINKS_PER_TEST);

        for projection in subject_links {
            by_subject
                .entry(projection.subject_path.clone())
                .or_default()
                .push(projection);
        }
    }

    let mut retained = by_subject
        .into_values()
        .flat_map(|mut projections| {
            projections.sort_by(test_subject_projection_order);
            projections.truncate(MAX_TEST_SUBJECT_LINKS_PER_SUBJECT);
            projections
        })
        .collect::<Vec<_>>();
    retained.sort_by(test_subject_projection_order);
    retained.dedup_by(|left, right| {
        left.test_path == right.test_path && left.subject_path == right.subject_path
    });

    retained
        .into_iter()
        .map(|projection| encode_test_subject_projection_record(&projection))
        .collect()
}

pub(crate) fn decode_test_subject_projection_records(
    rows: &[TestSubjectProjection],
) -> FriggResult<Vec<StoredTestSubjectProjection>> {
    rows.iter()
        .map(decode_test_subject_projection_record)
        .collect()
}

pub(super) fn decode_test_subject_projection_record(
    record: &TestSubjectProjection,
) -> FriggResult<StoredTestSubjectProjection> {
    let _stored_terms = &record.shared_terms;
    let _stored_flags: TestSubjectProjectionFlags = serde_json::from_str(&record.flags_json)
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode stored test subject projection flags for '{} -> {}': {err}",
                record.test_path, record.subject_path
            ))
        })?;

    let test_candidate = TestSubjectCandidate::from_path(&record.test_path);
    let subject_candidate = TestSubjectCandidate::from_path(&record.subject_path);
    build_test_subject_projection_from_candidates(&test_candidate, &subject_candidate).ok_or_else(
        || {
            FriggError::Internal(format!(
                "stored test subject projection '{} -> {}' is no longer valid under live generic heuristics",
                record.test_path, record.subject_path
            ))
        },
    )
}

pub(super) fn build_entrypoint_surface_projection_record(
    path: &str,
) -> FriggResult<Option<EntrypointSurfaceProjection>> {
    let Some(projection) = StoredEntrypointSurfaceProjection::from_path(path) else {
        return Ok(None);
    };
    let flags_json = serde_json::to_string(&projection.flags).map_err(|err| {
        FriggError::Internal(format!(
            "failed to encode entrypoint surface flags for '{path}': {err}"
        ))
    })?;

    Ok(Some(EntrypointSurfaceProjection {
        path: path.to_owned(),
        path_class: projection.path_class,
        source_class: projection.source_class,
        path_terms: projection.path_terms,
        surface_terms: projection.surface_terms,
        flags_json,
    }))
}

pub(crate) fn build_entrypoint_surface_projection_records_from_paths(
    paths: &[String],
) -> FriggResult<Vec<EntrypointSurfaceProjection>> {
    let mut rows = paths
        .iter()
        .filter_map(|path| build_entrypoint_surface_projection_record(path).transpose())
        .collect::<FriggResult<Vec<_>>>()?;
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    rows.dedup_by(|left, right| left.path == right.path);
    Ok(rows)
}

pub(super) fn decode_entrypoint_surface_projection_record(
    record: &EntrypointSurfaceProjection,
) -> FriggResult<StoredEntrypointSurfaceProjection> {
    let path_class = record.path_class.clone();
    let source_class = record.source_class.clone();
    let _stored_terms = &record.path_terms;
    let _stored_surface_terms = &record.surface_terms;
    let _stored_flags: EntrypointSurfaceProjectionFlags = serde_json::from_str(&record.flags_json)
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode stored entrypoint surface flags for '{}': {err}",
                record.path
            ))
        })?;

    let Some(projection) = StoredEntrypointSurfaceProjection::from_path(&record.path) else {
        return Err(FriggError::Internal(format!(
            "stored entrypoint surface projection '{}' is no longer valid under live generic heuristics",
            record.path
        )));
    };
    if projection.path_class != path_class || projection.source_class != source_class {
        return Err(FriggError::Internal(format!(
            "stored entrypoint surface projection '{}' no longer matches live path/source classification",
            record.path
        )));
    }

    Ok(projection)
}

pub(crate) fn decode_entrypoint_surface_projection_records(
    rows: &[EntrypointSurfaceProjection],
) -> FriggResult<Vec<StoredEntrypointSurfaceProjection>> {
    rows.iter()
        .map(decode_entrypoint_surface_projection_record)
        .collect()
}

pub(super) fn entrypoint_surface_overlay_boost(
    projection: &StoredEntrypointSurfaceProjection,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<PathOverlayBoost> {
    if !intent.wants_entrypoint_build_flow
        && !intent.wants_runtime_config_artifacts
        && !intent.wants_ci_workflow_witnesses
        && !intent.wants_scripts_ops_witnesses
    {
        return None;
    }

    let term_overlap = hybrid_overlap_count(
        &projection.surface_terms,
        &query_context.query_overlap_terms,
    ) as u32;
    let path_overlap =
        hybrid_overlap_count(&projection.path_terms, &query_context.query_overlap_terms) as u32;
    let exact_term_match = query_context.exact_terms.iter().any(|term| {
        projection
            .surface_terms
            .iter()
            .any(|candidate| candidate == term)
            || projection
                .path_terms
                .iter()
                .any(|candidate| candidate == term)
    });

    let mut boost = PathOverlayBoost::default();
    if term_overlap > 0 {
        boost.bonus_millis = boost
            .bonus_millis
            .saturating_add(term_overlap.saturating_mul(140));
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:terms:{}",
            projection.path
        ));
    }
    if path_overlap > 0 {
        boost.bonus_millis = boost
            .bonus_millis
            .saturating_add(path_overlap.saturating_mul(80));
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:path:{}",
            projection.path
        ));
    }
    if exact_term_match {
        boost.bonus_millis = boost.bonus_millis.saturating_add(160);
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:exact:{}",
            projection.path
        ));
    }

    if intent.wants_entrypoint_build_flow && projection.flags.is_runtime_entrypoint {
        boost.bonus_millis = boost.bonus_millis.saturating_add(420);
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:runtime:{}",
            projection.path
        ));
    }
    if intent.wants_entrypoint_build_flow && projection.flags.is_build_workflow {
        boost.bonus_millis = boost.bonus_millis.saturating_add(360);
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:build:{}",
            projection.path
        ));
    }
    if intent.wants_runtime_config_artifacts && projection.flags.is_runtime_config_artifact {
        boost.bonus_millis = boost.bonus_millis.saturating_add(380);
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:config:{}",
            projection.path
        ));
    }
    if intent.wants_ci_workflow_witnesses && projection.flags.is_ci_workflow {
        boost.bonus_millis = boost.bonus_millis.saturating_add(320);
        boost
            .provenance_ids
            .push(format!("overlay:entrypoint_surface:ci:{}", projection.path));
    }
    if intent.wants_scripts_ops_witnesses && projection.flags.is_scripts_ops {
        boost.bonus_millis = boost.bonus_millis.saturating_add(320);
        boost.provenance_ids.push(format!(
            "overlay:entrypoint_surface:scripts:{}",
            projection.path
        ));
    }

    (boost.bonus_millis > 0).then_some(boost)
}

pub(super) fn accumulate_test_subject_overlay_boosts(
    projections: &[StoredTestSubjectProjection],
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> BTreeMap<String, PathOverlayBoost> {
    if !intent.wants_tests && !intent.wants_test_witness_recall {
        return BTreeMap::new();
    }

    let mut boosts = BTreeMap::<String, PathOverlayBoost>::new();
    for projection in projections {
        let shared_overlap =
            hybrid_overlap_count(&projection.shared_terms, &query_context.query_overlap_terms)
                as u32;
        let exact_term_match = query_context.exact_terms.iter().any(|term| {
            projection
                .shared_terms
                .iter()
                .any(|candidate| candidate == term)
        });
        if shared_overlap == 0 && !exact_term_match {
            continue;
        }

        let capped_score_hint = projection.score_hint.min(24) as u32;
        let mut test_boost = PathOverlayBoost {
            bonus_millis: shared_overlap
                .saturating_mul(180)
                .saturating_add(capped_score_hint.saturating_mul(10)),
            provenance_ids: vec![format!(
                "overlay:test_subject:test:{}:{}",
                projection.test_path, projection.subject_path
            )],
        };
        if exact_term_match {
            test_boost.bonus_millis = test_boost.bonus_millis.saturating_add(160);
        }
        if projection.flags.exact_stem_match {
            test_boost.bonus_millis = test_boost.bonus_millis.saturating_add(120);
        }
        if projection.flags.same_language {
            test_boost.bonus_millis = test_boost.bonus_millis.saturating_add(60);
        }
        if projection.flags.runtime_subject {
            test_boost.bonus_millis = test_boost.bonus_millis.saturating_add(120);
        }
        if projection.flags.support_subject {
            test_boost.bonus_millis = test_boost.bonus_millis.saturating_add(40);
        }
        boosts
            .entry(projection.test_path.clone())
            .or_default()
            .merge(test_boost);

        let mut subject_boost = PathOverlayBoost {
            bonus_millis: shared_overlap
                .saturating_mul(90)
                .saturating_add(capped_score_hint.saturating_mul(6)),
            provenance_ids: vec![format!(
                "overlay:test_subject:subject:{}:{}",
                projection.subject_path, projection.test_path
            )],
        };
        if exact_term_match {
            subject_boost.bonus_millis = subject_boost.bonus_millis.saturating_add(80);
        }
        if projection.flags.exact_stem_match {
            subject_boost.bonus_millis = subject_boost.bonus_millis.saturating_add(60);
        }
        if projection.flags.runtime_subject {
            subject_boost.bonus_millis = subject_boost.bonus_millis.saturating_add(90);
        }
        if projection.flags.runtime_subject
            && intent.wants_test_witness_recall
            && !intent.wants_entrypoint_build_flow
        {
            subject_boost.bonus_millis = subject_boost.bonus_millis.saturating_add(360);
            subject_boost.provenance_ids.push(format!(
                "overlay:test_subject:reverse_focus:{}:{}",
                projection.subject_path, projection.test_path
            ));
        }
        boosts
            .entry(projection.subject_path.clone())
            .or_default()
            .merge(subject_boost);
    }

    boosts
}

pub(super) fn accumulate_companion_surface_overlay_boosts(
    projections: &[StoredPathWitnessProjection],
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> BTreeMap<String, PathOverlayBoost> {
    if !wants_companion_surface_overlay(intent) {
        return BTreeMap::new();
    }

    let anchors = projections
        .iter()
        .filter_map(|projection| companion_surface_anchor(projection, query_context))
        .collect::<Vec<_>>();
    if anchors.is_empty() {
        return BTreeMap::new();
    }

    let mut boosts = BTreeMap::<String, PathOverlayBoost>::new();
    for projection in projections {
        let Some(subtree_root) = projection.subtree_root.as_deref() else {
            continue;
        };
        let families = generic_surface_families_for_projection(projection);
        if families.is_empty() {
            continue;
        }

        let query_match =
            query_context.match_projection_path(&projection.path, &projection.path_terms);
        let mut best_boost = PathOverlayBoost::default();
        for anchor in &anchors {
            if anchor.path == projection.path {
                continue;
            }
            if !subtree_roots_related(subtree_root, &anchor.subtree_root) {
                continue;
            }

            let family_bonus = companion_surface_family_bonus(&families, &anchor.families, intent);
            if family_bonus == 0 {
                continue;
            }

            let same_family = families
                .iter()
                .any(|family| anchor.families.contains(family));
            let mut bonus_millis = 120_u32.saturating_add(family_bonus);
            if same_family {
                bonus_millis = bonus_millis.saturating_add(70);
            }
            let term_overlap = (query_match.path_overlap.min(3) as u32)
                .saturating_add(anchor.path_overlap.min(3) as u32)
                .min(4);
            bonus_millis = bonus_millis.saturating_add(term_overlap.saturating_mul(45));
            if query_match.has_exact_query_term_match || anchor.exact_term_match {
                bonus_millis = bonus_millis.saturating_add(80);
            }
            if query_match.specific_witness_path_overlap > 0 || anchor.specific_witness_overlap > 0
            {
                bonus_millis = bonus_millis.saturating_add(120);
            }

            let mut provenance_ids = vec![format!(
                "overlay:companion_surface:subtree:{}:{}",
                anchor.path, projection.path
            )];
            if same_family {
                provenance_ids.push(format!(
                    "overlay:companion_surface:family:{}:{}",
                    anchor.path, projection.path
                ));
            } else {
                provenance_ids.push(format!(
                    "overlay:companion_surface:companion:{}:{}",
                    anchor.path, projection.path
                ));
            }
            if query_match.specific_witness_path_overlap > 0 || anchor.specific_witness_overlap > 0
            {
                provenance_ids.push(format!(
                    "overlay:companion_surface:specific:{}:{}",
                    anchor.path, projection.path
                ));
            }
            if query_match.has_exact_query_term_match || anchor.exact_term_match {
                provenance_ids.push(format!(
                    "overlay:companion_surface:exact:{}:{}",
                    anchor.path, projection.path
                ));
            }

            if bonus_millis > best_boost.bonus_millis {
                best_boost = PathOverlayBoost {
                    bonus_millis,
                    provenance_ids,
                };
            }
        }

        if best_boost.bonus_millis > 0 {
            boosts.insert(projection.path.clone(), best_boost);
        }
    }

    boosts
}

pub(super) fn accumulate_relation_overlay_boosts(
    relations: &[PathRelationProjection],
    surface_terms_by_path: &BTreeMap<String, PathSurfaceTermProjection>,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> BTreeMap<String, PathOverlayBoost> {
    if !wants_companion_surface_overlay(intent) {
        return BTreeMap::new();
    }

    let mut boosts = BTreeMap::<String, PathOverlayBoost>::new();
    for relation in relations {
        let relation_bonus = relation_kind_bonus(relation, intent);
        if relation_bonus == 0 {
            continue;
        }

        let src_match = surface_term_match_score(
            surface_terms_by_path.get(&relation.src_path),
            &relation.shared_terms,
            query_context,
        );
        let dst_match = surface_term_match_score(
            surface_terms_by_path.get(&relation.dst_path),
            &relation.shared_terms,
            query_context,
        );
        if src_match.total == 0 && dst_match.total == 0 {
            continue;
        }

        if src_match.total >= dst_match.total && src_match.total > 0 {
            let boost = relation_overlay_boost(
                relation,
                &relation.dst_path,
                "dst",
                src_match,
                relation_bonus,
            );
            boosts
                .entry(relation.dst_path.clone())
                .or_default()
                .merge(boost);
        }
        if dst_match.total >= src_match.total && dst_match.total > 0 {
            let boost = relation_overlay_boost(
                relation,
                &relation.src_path,
                "src",
                dst_match,
                relation_bonus,
            );
            boosts
                .entry(relation.src_path.clone())
                .or_default()
                .merge(boost);
        }
    }

    boosts
}

#[derive(Debug, Clone)]
struct CompanionSurfaceAnchor {
    path: String,
    subtree_root: String,
    families: Vec<GenericWitnessSurfaceFamily>,
    path_overlap: usize,
    specific_witness_overlap: usize,
    exact_term_match: bool,
}

fn wants_companion_surface_overlay(intent: &HybridRankingIntent) -> bool {
    intent.wants_runtime_witnesses
        || intent.wants_tests
        || intent.wants_test_witness_recall
        || intent.wants_entrypoint_build_flow
        || intent.wants_runtime_config_artifacts
}

fn companion_surface_anchor(
    projection: &StoredPathWitnessProjection,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<CompanionSurfaceAnchor> {
    let subtree_root = projection.subtree_root.clone()?;
    let families = generic_surface_families_for_projection(projection);
    if families.is_empty() {
        return None;
    }

    let query_match = query_context.match_projection_path(&projection.path, &projection.path_terms);
    if query_match.path_overlap == 0
        && query_match.specific_witness_path_overlap == 0
        && !query_match.has_exact_query_term_match
    {
        return None;
    }

    Some(CompanionSurfaceAnchor {
        path: projection.path.clone(),
        subtree_root,
        families,
        path_overlap: query_match.path_overlap,
        specific_witness_overlap: query_match.specific_witness_path_overlap,
        exact_term_match: query_match.has_exact_query_term_match,
    })
}

fn companion_surface_family_bonus(
    left: &[GenericWitnessSurfaceFamily],
    right: &[GenericWitnessSurfaceFamily],
    intent: &HybridRankingIntent,
) -> u32 {
    let mut best = 0_u32;
    for &left_family in left {
        for &right_family in right {
            let bonus = family_pair_bonus(left_family, right_family, intent);
            if bonus > best {
                best = bonus;
            }
        }
    }
    best
}

fn family_pair_bonus(
    left: GenericWitnessSurfaceFamily,
    right: GenericWitnessSurfaceFamily,
    intent: &HybridRankingIntent,
) -> u32 {
    use GenericWitnessSurfaceFamily::{
        BuildConfig, Entrypoint, PackageSurface, Runtime, Tests, WorkspaceConfig,
    };

    if left == right && left == Tests {
        return 0;
    }
    if left == right {
        return 140;
    }

    match (left, right) {
        (Runtime, Tests) | (Tests, Runtime)
            if intent.wants_runtime_witnesses
                || intent.wants_tests
                || intent.wants_test_witness_recall =>
        {
            120
        }
        (Runtime, Entrypoint) | (Entrypoint, Runtime)
            if intent.wants_runtime_witnesses || intent.wants_entrypoint_build_flow =>
        {
            110
        }
        (Runtime, PackageSurface)
        | (PackageSurface, Runtime)
        | (Runtime, BuildConfig)
        | (BuildConfig, Runtime)
        | (Runtime, WorkspaceConfig)
        | (WorkspaceConfig, Runtime)
            if intent.wants_runtime_witnesses || intent.wants_runtime_config_artifacts =>
        {
            100
        }
        (Entrypoint, BuildConfig)
        | (BuildConfig, Entrypoint)
        | (Entrypoint, WorkspaceConfig)
        | (WorkspaceConfig, Entrypoint)
        | (Entrypoint, PackageSurface)
        | (PackageSurface, Entrypoint)
            if intent.wants_entrypoint_build_flow || intent.wants_runtime_config_artifacts =>
        {
            95
        }
        (PackageSurface, BuildConfig)
        | (BuildConfig, PackageSurface)
        | (PackageSurface, WorkspaceConfig)
        | (WorkspaceConfig, PackageSurface)
        | (BuildConfig, WorkspaceConfig)
        | (WorkspaceConfig, BuildConfig)
            if intent.wants_runtime_config_artifacts =>
        {
            90
        }
        (Tests, Entrypoint) | (Entrypoint, Tests)
            if intent.wants_test_witness_recall || intent.wants_entrypoint_build_flow =>
        {
            85
        }
        _ => 0,
    }
}

fn subtree_roots_related(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[derive(Debug, Clone, Copy, Default)]
struct SurfaceTermMatch {
    total: u32,
    weighted_overlap: u32,
    shared_overlap: u32,
    exact_term_match: bool,
}

fn surface_term_match_score(
    projection: Option<&PathSurfaceTermProjection>,
    shared_terms: &[String],
    query_context: &HybridPathWitnessQueryContext,
) -> SurfaceTermMatch {
    let Some(projection) = projection else {
        return SurfaceTermMatch::default();
    };
    let weighted_overlap = query_context
        .query_overlap_terms
        .iter()
        .filter_map(|term| projection.term_weights.get(term))
        .map(|weight| *weight as u32)
        .sum::<u32>()
        .min(20);
    let shared_overlap =
        hybrid_overlap_count(shared_terms, &query_context.query_overlap_terms).min(4) as u32;
    let exact_term_match = query_context.exact_terms.iter().any(|term| {
        projection
            .exact_terms
            .iter()
            .any(|candidate| candidate == term)
    });
    let total = weighted_overlap
        .saturating_add(shared_overlap.saturating_mul(3))
        .saturating_add(u32::from(exact_term_match).saturating_mul(6));
    SurfaceTermMatch {
        total,
        weighted_overlap,
        shared_overlap,
        exact_term_match,
    }
}

fn relation_kind_bonus(relation: &PathRelationProjection, intent: &HybridRankingIntent) -> u32 {
    match relation.relation_kind.as_str() {
        "companion_surface" if wants_companion_surface_overlay(intent) => 220,
        "entrypoint_workflow" if intent.wants_entrypoint_build_flow => 240,
        "entrypoint_config"
            if intent.wants_runtime_config_artifacts || intent.wants_entrypoint_build_flow =>
        {
            230
        }
        "entrypoint_package"
            if intent.wants_runtime_config_artifacts || intent.wants_runtime_witnesses =>
        {
            215
        }
        "entrypoint_workspace"
            if intent.wants_runtime_config_artifacts || intent.wants_entrypoint_build_flow =>
        {
            205
        }
        "test_subject" if intent.wants_tests || intent.wants_test_witness_recall => 200,
        _ => {
            let left = generic_surface_families_from_bits(relation.src_family_bits);
            let right = generic_surface_families_from_bits(relation.dst_family_bits);
            companion_surface_family_bonus(&left, &right, intent)
        }
    }
}

fn relation_overlay_boost(
    relation: &PathRelationProjection,
    target_path: &str,
    direction: &str,
    surface_match: SurfaceTermMatch,
    relation_bonus: u32,
) -> PathOverlayBoost {
    let bonus_millis = relation_bonus
        .saturating_add(surface_match.weighted_overlap.saturating_mul(20))
        .saturating_add(surface_match.shared_overlap.saturating_mul(35))
        .saturating_add(u32::from(surface_match.exact_term_match).saturating_mul(90))
        .saturating_add((relation.score_hint.min(32) as u32).saturating_mul(4));
    let mut provenance_ids = vec![format!(
        "overlay:path_relation:{}:{}:{}",
        relation.relation_kind, direction, target_path
    )];
    if surface_match.exact_term_match {
        provenance_ids.push(format!(
            "overlay:path_relation:exact:{}:{}",
            relation.src_path, relation.dst_path
        ));
    }
    if surface_match.shared_overlap > 0 {
        provenance_ids.push(format!(
            "overlay:path_relation:shared:{}:{}",
            relation.src_path, relation.dst_path
        ));
    }
    PathOverlayBoost {
        bonus_millis,
        provenance_ids,
    }
}

fn encode_test_subject_projection_record(
    projection: &StoredTestSubjectProjection,
) -> FriggResult<TestSubjectProjection> {
    let flags_json = serde_json::to_string(&projection.flags).map_err(|err| {
        FriggError::Internal(format!(
            "failed to encode test subject projection flags for '{} -> {}': {err}",
            projection.test_path, projection.subject_path
        ))
    })?;

    Ok(TestSubjectProjection {
        test_path: projection.test_path.clone(),
        subject_path: projection.subject_path.clone(),
        shared_terms: projection.shared_terms.clone(),
        score_hint: projection.score_hint,
        flags_json,
    })
}

fn build_test_subject_projection_from_candidates(
    test_candidate: &TestSubjectCandidate,
    subject_candidate: &TestSubjectCandidate,
) -> Option<StoredTestSubjectProjection> {
    if test_candidate.path == subject_candidate.path {
        return None;
    }
    if !test_candidate.is_testish || !subject_candidate.is_subjectish {
        return None;
    }
    if !test_subject_languages_compatible(test_candidate.language, subject_candidate.language) {
        return None;
    }

    let shared_terms = test_candidate
        .subject_terms
        .iter()
        .filter(|term| subject_candidate.subject_terms.contains(*term))
        .cloned()
        .collect::<Vec<_>>();
    if shared_terms.is_empty() {
        return None;
    }

    let exact_stem_match = test_candidate.file_stem_subject == subject_candidate.file_stem_subject
        && !test_candidate.file_stem_subject.is_empty();
    let same_language =
        test_candidate.language == subject_candidate.language && test_candidate.language.is_some();
    let runtime_subject = subject_candidate.path_class == PathClass::Runtime;
    let support_subject = subject_candidate.path_class == PathClass::Support;
    let score_hint = shared_terms.len().saturating_mul(10)
        + usize::from(exact_stem_match).saturating_mul(6)
        + usize::from(same_language).saturating_mul(3)
        + usize::from(runtime_subject).saturating_mul(2)
        + usize::from(support_subject);

    Some(StoredTestSubjectProjection {
        test_path: test_candidate.path.clone(),
        subject_path: subject_candidate.path.clone(),
        shared_terms,
        score_hint,
        flags: TestSubjectProjectionFlags {
            exact_stem_match,
            same_language,
            runtime_subject,
            support_subject,
        },
    })
}

fn test_subject_projection_order(
    left: &StoredTestSubjectProjection,
    right: &StoredTestSubjectProjection,
) -> std::cmp::Ordering {
    right
        .score_hint
        .cmp(&left.score_hint)
        .then_with(|| left.test_path.cmp(&right.test_path))
        .then_with(|| left.subject_path.cmp(&right.subject_path))
}

#[derive(Debug, Clone)]
struct TestSubjectCandidate {
    path: String,
    path_class: PathClass,
    language: Option<SymbolLanguage>,
    subject_terms: BTreeSet<String>,
    file_stem_subject: String,
    is_testish: bool,
    is_subjectish: bool,
}

impl TestSubjectCandidate {
    fn from_path(path: &str) -> Self {
        let path_class = classify_repository_path(path);
        let source_class = hybrid_source_class(path);
        let language = SymbolLanguage::from_path(Path::new(path));
        let subject_terms = normalized_test_subject_terms(path);
        let file_stem_subject = normalized_subject_stem(path);
        let is_testish = matches!(source_class, SourceClass::Tests)
            || is_test_support_path(path)
            || is_test_harness_path(path)
            || path_contains_test_signal(path);
        let is_subjectish = !is_testish
            && !matches!(
                source_class,
                SourceClass::Documentation
                    | SourceClass::Readme
                    | SourceClass::Fixtures
                    | SourceClass::BenchmarkDocs
                    | SourceClass::Playbooks
                    | SourceClass::Specs
                    | SourceClass::Other
            );

        Self {
            path: path.to_owned(),
            path_class,
            language,
            subject_terms,
            file_stem_subject,
            is_testish,
            is_subjectish,
        }
    }
}

impl StoredEntrypointSurfaceProjection {
    pub(super) fn from_path(path: &str) -> Option<Self> {
        let flags = build_entrypoint_surface_projection_flags(path);
        if !flags.is_runtime_entrypoint
            && !flags.is_build_workflow
            && !flags.is_runtime_config_artifact
            && !flags.is_ci_workflow
            && !flags.is_scripts_ops
        {
            return None;
        }

        Some(Self {
            path: path.to_owned(),
            path_class: classify_repository_path(path),
            source_class: hybrid_source_class(path),
            path_terms: hybrid_path_overlap_tokens(path),
            surface_terms: build_entrypoint_surface_terms(path, &flags),
            flags,
        })
    }
}

fn build_entrypoint_surface_projection_flags(path: &str) -> EntrypointSurfaceProjectionFlags {
    EntrypointSurfaceProjectionFlags {
        is_runtime_entrypoint: is_entrypoint_runtime_path(path),
        is_build_workflow: is_entrypoint_build_workflow_path(path),
        is_runtime_config_artifact: is_runtime_config_artifact_path(path),
        is_ci_workflow: is_ci_workflow_path(path),
        is_scripts_ops: is_scripts_ops_path(path),
    }
}

fn build_entrypoint_surface_terms(
    path: &str,
    flags: &EntrypointSurfaceProjectionFlags,
) -> Vec<String> {
    let mut terms = hybrid_path_overlap_tokens(path)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if flags.is_runtime_entrypoint {
        terms.extend(
            [
                "entrypoint",
                "startup",
                "bootstrap",
                "runtime",
                "main",
                "app",
            ]
            .into_iter()
            .map(str::to_owned),
        );
    }
    if flags.is_build_workflow {
        terms.extend(
            ["build", "workflow", "compile", "pipeline"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    if flags.is_runtime_config_artifact {
        terms.extend(
            ["config", "manifest", "runtime", "settings"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    if flags.is_ci_workflow {
        terms.extend(
            ["ci", "workflow", "automation"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    if flags.is_scripts_ops {
        terms.extend(
            ["script", "ops", "automation"]
                .into_iter()
                .map(str::to_owned),
        );
    }

    terms.into_iter().collect()
}

fn normalized_test_subject_terms(path: &str) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for token in hybrid_path_overlap_tokens(path) {
        if !is_low_signal_test_subject_term(&token) {
            terms.insert(token);
        }
    }
    for token in hybrid_identifier_tokens(&normalized_subject_stem(path)) {
        if !is_low_signal_test_subject_term(&token) {
            terms.insert(token);
        }
    }
    terms
}

fn normalized_subject_stem(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut normalized = stem.clone();
    for prefix in [
        "test_",
        "tests_",
        "spec_",
        "specs_",
        "integration_",
        "unit_",
        "e2e_",
    ] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.to_owned();
            break;
        }
    }
    for suffix in [
        "_test",
        "_tests",
        "_spec",
        "_specs",
        "_integration",
        "_unit",
        "_e2e",
        "_suite",
        "_case",
    ] {
        if let Some(stripped) = normalized.strip_suffix(suffix) {
            normalized = stripped.to_owned();
            break;
        }
    }
    normalized
}

fn is_low_signal_test_subject_term(term: &str) -> bool {
    matches!(
        term,
        "test"
            | "tests"
            | "spec"
            | "specs"
            | "integration"
            | "unit"
            | "e2e"
            | "fixture"
            | "fixtures"
            | "helper"
            | "helpers"
            | "harness"
            | "support"
            | "case"
            | "cases"
            | "suite"
            | "suites"
            | "snapshot"
            | "smoke"
            | "bench"
            | "benches"
            | "benchmark"
    )
}

fn path_contains_test_signal(path: &str) -> bool {
    path.split('/').any(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            "test" | "tests" | "spec" | "specs"
        )
    })
}

fn test_subject_languages_compatible(
    test_language: Option<SymbolLanguage>,
    subject_language: Option<SymbolLanguage>,
) -> bool {
    match (test_language, subject_language) {
        (Some(SymbolLanguage::Php), Some(SymbolLanguage::Blade))
        | (Some(SymbolLanguage::Blade), Some(SymbolLanguage::Php)) => true,
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subject_projection_pairs_generic_test_and_runtime_subject_paths() {
        let rows = build_test_subject_projection_records(&[
            "tests/unit/user_service_test.rs".to_owned(),
            "src/user_service.rs".to_owned(),
            "src/other.rs".to_owned(),
        ])
        .expect("projection rows should build");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].test_path, "tests/unit/user_service_test.rs");
        assert_eq!(rows[0].subject_path, "src/user_service.rs");
        assert!(rows[0].score_hint > 0);
    }

    #[test]
    fn entrypoint_surface_projection_marks_runtime_and_build_artifacts() {
        let runtime = StoredEntrypointSurfaceProjection::from_path("src/main.rs")
            .expect("runtime entrypoint should project");
        assert!(runtime.flags.is_runtime_entrypoint);
        assert!(
            runtime
                .surface_terms
                .iter()
                .any(|term| term == "entrypoint")
        );

        let workflow = StoredEntrypointSurfaceProjection::from_path(".github/workflows/ci.yml")
            .expect("workflow surface should project");
        assert!(workflow.flags.is_build_workflow || workflow.flags.is_ci_workflow);
        assert!(workflow.surface_terms.iter().any(|term| term == "workflow"));
    }

    #[test]
    fn decode_test_subject_projection_record_recomputes_live_terms_and_flags() {
        let record = TestSubjectProjection {
            test_path: "tests/unit/user_service_test.rs".to_owned(),
            subject_path: "src/user_service.rs".to_owned(),
            shared_terms: vec!["stale".to_owned()],
            score_hint: 1,
            flags_json: serde_json::to_string(&TestSubjectProjectionFlags::default())
                .expect("flags json"),
        };

        let decoded =
            decode_test_subject_projection_record(&record).expect("decode should succeed");

        assert!(
            decoded.shared_terms.iter().any(|term| term == "user"),
            "live decode should recover current shared subject terms: {:?}",
            decoded.shared_terms
        );
        assert!(decoded.flags.exact_stem_match);
    }

    #[test]
    fn decode_entrypoint_surface_projection_record_recomputes_live_surface_terms() {
        let record = EntrypointSurfaceProjection {
            path: "Cargo.toml".to_owned(),
            path_class: PathClass::Project,
            source_class: SourceClass::Project,
            path_terms: Vec::new(),
            surface_terms: Vec::new(),
            flags_json: serde_json::to_string(&EntrypointSurfaceProjectionFlags::default())
                .expect("flags json"),
        };

        let decoded = decode_entrypoint_surface_projection_record(&record)
            .expect("decode should succeed for runtime config artifacts");

        assert!(decoded.flags.is_runtime_config_artifact);
        assert!(
            decoded.surface_terms.iter().any(|term| term == "config"),
            "live decode should recover generic config surface terms"
        );
    }

    #[test]
    fn entrypoint_surface_overlay_boost_rewards_matching_runtime_config_surfaces() {
        let projection = StoredEntrypointSurfaceProjection::from_path("Cargo.toml")
            .expect("runtime config artifact should project");
        let intent = HybridRankingIntent::from_query("runtime config manifest settings");
        let query_context =
            HybridPathWitnessQueryContext::from_query_text("runtime config manifest settings");

        let boost = entrypoint_surface_overlay_boost(&projection, &intent, &query_context)
            .expect("matching config query should produce an overlay boost");

        assert!(boost.bonus_millis >= 500);
        assert!(
            boost
                .provenance_ids
                .iter()
                .any(|id| id.starts_with("overlay:entrypoint_surface:config:"))
        );
    }

    #[test]
    fn test_subject_overlay_boosts_promote_related_tests_and_subjects() {
        let projections = vec![StoredTestSubjectProjection {
            test_path: "tests/unit/user_service_test.rs".to_owned(),
            subject_path: "src/user_service.rs".to_owned(),
            shared_terms: vec!["user".to_owned(), "service".to_owned()],
            score_hint: 23,
            flags: TestSubjectProjectionFlags {
                exact_stem_match: true,
                same_language: true,
                runtime_subject: true,
                support_subject: false,
            },
        }];
        let intent = HybridRankingIntent::from_query("user service tests");
        let query_context = HybridPathWitnessQueryContext::from_query_text("user service tests");

        let boosts = accumulate_test_subject_overlay_boosts(&projections, &intent, &query_context);

        assert!(
            boosts
                .get("tests/unit/user_service_test.rs")
                .is_some_and(|boost| boost.bonus_millis > 0)
        );
        assert!(
            boosts
                .get("src/user_service.rs")
                .is_some_and(|boost| boost.bonus_millis > 0)
        );
    }

    #[test]
    fn companion_surface_overlay_boosts_promote_same_subtree_runtime_and_config_surfaces() {
        let projections = vec![
            StoredPathWitnessProjection::from_path("packages/editor-ui/src/main.ts"),
            StoredPathWitnessProjection::from_path("packages/editor-ui/package.json"),
            StoredPathWitnessProjection::from_path("packages/editor-ui/tsconfig.base.json"),
            StoredPathWitnessProjection::from_path("packages/worker/src/main.ts"),
        ];
        let intent = HybridRankingIntent::from_query("editor ui runtime config main tsconfig");
        let query_context = HybridPathWitnessQueryContext::from_query_text(
            "editor ui runtime config main tsconfig",
        );

        let boosts =
            accumulate_companion_surface_overlay_boosts(&projections, &intent, &query_context);

        assert!(
            boosts
                .get("packages/editor-ui/package.json")
                .is_some_and(|boost| boost.bonus_millis > 0),
            "same-subtree package surface should receive a companion boost"
        );
        assert!(
            boosts
                .get("packages/editor-ui/tsconfig.base.json")
                .is_some_and(|boost| boost.bonus_millis > 0),
            "same-subtree workspace config should receive a companion boost"
        );
        assert!(
            boosts
                .get("packages/worker/src/main.ts")
                .is_none_or(|boost| boost.bonus_millis == 0),
            "sibling workspace runtime should not inherit an editor-ui companion boost"
        );
    }
}
