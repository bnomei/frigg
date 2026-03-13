use crate::domain::model::TextMatch;
use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, EvidenceHit};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{
    HybridChannelHit, HybridDocumentRef, HybridRankingIntent, SearchExecutionOutput,
    StoredPathWitnessProjection, hybrid_excerpt_has_build_flow_anchor,
    hybrid_excerpt_has_exact_identifier_anchor, hybrid_excerpt_has_test_double_anchor,
    hybrid_identifier_tokens, hybrid_overlap_count, hybrid_path_overlap_tokens,
    hybrid_query_overlap_terms,
    policy::{
        PathWitnessFacts,
        hybrid_path_quality_multiplier_with_intent as policy_path_quality_multiplier_with_intent,
        hybrid_path_witness_recall_score_from_context,
    },
    sort_matches_deterministically, sort_search_diagnostics_deterministically,
};

#[cfg(test)]
pub(super) fn build_hybrid_lexical_hits(matches: &[TextMatch]) -> Vec<HybridChannelHit> {
    build_hybrid_lexical_hits_with_intent(matches, &HybridRankingIntent::default(), "")
}

#[cfg(test)]
pub(super) fn build_hybrid_lexical_hits_for_query(
    matches: &[TextMatch],
    query_text: &str,
) -> Vec<HybridChannelHit> {
    let intent = HybridRankingIntent::from_query(query_text);
    build_hybrid_lexical_hits_with_intent(matches, &intent, query_text)
}

pub(super) fn build_hybrid_lexical_hits_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
    query_text: &str,
) -> Vec<HybridChannelHit> {
    build_hybrid_hits_from_matches_with_intent(
        matches,
        intent,
        query_text,
        EvidenceChannel::LexicalManifest,
        EvidenceAnchorKind::TextSpan,
    )
}

pub(super) fn build_hybrid_path_witness_hits_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
    query_text: &str,
) -> Vec<HybridChannelHit> {
    let mut hits = build_hybrid_hits_from_matches_with_intent(
        matches,
        intent,
        query_text,
        EvidenceChannel::PathSurfaceWitness,
        EvidenceAnchorKind::PathWitness,
    );
    for (hit, found) in hits.iter_mut().zip(matches.iter()) {
        hit.provenance_ids = vec![format!(
            "path_witness:{}:{}:{}",
            hit.document.path, hit.document.line, hit.document.column
        )];
        if let Some(score_hint_millis) = found.witness_score_hint_millis {
            hit.raw_score = score_hint_millis as f32 / 1000.0;
        }
        if let Some(extra_provenance_ids) = &found.witness_provenance_ids {
            hit.provenance_ids
                .extend(extra_provenance_ids.iter().cloned());
            hit.provenance_ids.sort();
            hit.provenance_ids.dedup();
        }
    }
    hits
}

fn build_hybrid_hits_from_matches_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
    query_text: &str,
    channel: EvidenceChannel,
    anchor_kind: EvidenceAnchorKind,
) -> Vec<EvidenceHit> {
    let mut frequency_by_document: BTreeMap<(String, String), f32> = BTreeMap::new();
    for found in matches {
        let key = (found.repository_id.clone(), found.path.clone());
        *frequency_by_document.entry(key).or_insert(0.0) += 1.0;
    }

    matches
        .iter()
        .map(|found| {
            let key = (found.repository_id.clone(), found.path.clone());
            let frequency = *frequency_by_document.get(&key).unwrap_or(&1.0);
            let computed_raw_score = frequency.sqrt()
                * hybrid_path_quality_multiplier_with_intent(&found.path, intent)
                * hybrid_excerpt_alignment_multiplier(&found.excerpt, intent, query_text);
            let raw_score = if matches!(channel, EvidenceChannel::PathSurfaceWitness) {
                found
                    .witness_score_hint_millis
                    .map(|millis| millis as f32 / 1000.0)
                    .unwrap_or(computed_raw_score)
            } else {
                computed_raw_score
            };
            let anchor = EvidenceAnchor::new(
                anchor_kind,
                found.line,
                found.column,
                found.line,
                found.column,
            );
            let anchor = match anchor_kind {
                EvidenceAnchorKind::PathWitness => anchor.with_detail(found.path.clone()),
                _ => anchor,
            };
            HybridChannelHit {
                channel,
                document: HybridDocumentRef {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: found.column,
                },
                anchor,
                raw_score,
                excerpt: found.excerpt.clone(),
                provenance_ids: vec![format!(
                    "text:{}:{}:{}",
                    found.path, found.line, found.column
                )],
            }
        })
        .collect()
}

fn hybrid_excerpt_alignment_multiplier(
    excerpt: &str,
    intent: &HybridRankingIntent,
    query_text: &str,
) -> f32 {
    let query_terms = hybrid_query_overlap_terms(query_text);
    if query_terms.is_empty() {
        return 1.0;
    }

    let excerpt_terms = hybrid_identifier_tokens(excerpt);
    let overlap = hybrid_overlap_count(&excerpt_terms, &query_terms);
    let mut multiplier = match overlap {
        0 => 1.0,
        1 => 1.05,
        2 => 1.14,
        _ => 1.24,
    };
    if hybrid_excerpt_has_exact_identifier_anchor(excerpt, query_text) {
        multiplier *= 1.18;
    }

    if intent.wants_entrypoint_build_flow {
        if hybrid_excerpt_has_build_flow_anchor(excerpt, &query_terms) {
            multiplier *= 1.24;
        }
        if hybrid_excerpt_has_test_double_anchor(excerpt) {
            multiplier *= 0.72;
        }
    }

    multiplier
}

pub(super) fn hybrid_path_quality_multiplier_with_intent(
    path: &str,
    intent: &HybridRankingIntent,
) -> f32 {
    policy_path_quality_multiplier_with_intent(path, intent)
}

pub(super) use super::policy::PathWitnessQueryContext as HybridPathWitnessQueryContext;

fn score_path_witness_anchor_line(
    line: &str,
    path_terms: &[String],
    query_context: &HybridPathWitnessQueryContext,
) -> usize {
    let normalized_line = line.to_ascii_lowercase();
    let line_terms = hybrid_identifier_tokens(&normalized_line);
    let mut score = hybrid_overlap_count(&line_terms, &query_context.query_overlap_terms) * 4;
    score += hybrid_overlap_count(&line_terms, path_terms) * 2;
    if query_context
        .exact_terms
        .iter()
        .any(|term: &String| normalized_line.contains(term.as_str()))
    {
        score += 8;
    }

    score
}

fn max_path_witness_anchor_score(
    path_terms: &[String],
    query_context: &HybridPathWitnessQueryContext,
) -> usize {
    query_context.query_overlap_terms.len().saturating_mul(4)
        + path_terms.len().saturating_mul(2)
        + if query_context.exact_terms.is_empty() {
            0
        } else {
            8
        }
}

pub(super) fn best_path_witness_anchor_in_file(
    path: &str,
    file_path: &Path,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<(usize, String)> {
    let file = File::open(file_path).ok()?;
    best_path_witness_anchor_in_reader(path, BufReader::new(file), query_context)
}

fn best_path_witness_anchor_in_reader<R: BufRead>(
    path: &str,
    mut reader: R,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<(usize, String)> {
    let path_terms = hybrid_path_overlap_tokens(path);
    let max_score = max_path_witness_anchor_score(&path_terms, query_context);
    let mut buffer = String::new();
    let mut line_number = 0usize;
    let mut first_non_empty: Option<(usize, String)> = None;
    let mut best_excerpt: Option<(usize, String)> = None;
    let mut best_score = 0usize;

    loop {
        buffer.clear();
        let bytes_read = reader.read_line(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }

        line_number += 1;
        let line = buffer.trim();
        if line.is_empty() {
            continue;
        }
        if first_non_empty.is_none() {
            first_non_empty = Some((line_number, line.to_owned()));
        }

        let score = score_path_witness_anchor_line(line, &path_terms, query_context);
        if score > best_score {
            best_score = score;
            best_excerpt = Some((line_number, line.to_owned()));
            if best_score >= max_score {
                break;
            }
        }
    }

    best_excerpt.or(first_non_empty)
}

pub(super) fn hybrid_path_witness_recall_score(
    path: &str,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<f32> {
    let projection = StoredPathWitnessProjection::from_path(path);
    hybrid_path_witness_recall_score_for_projection(path, &projection, intent, query_context)
}

pub(super) fn hybrid_path_witness_recall_score_for_projection(
    path: &str,
    projection: &StoredPathWitnessProjection,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<f32> {
    if !intent.wants_path_witness_recall() {
        return None;
    }

    let ctx = PathWitnessFacts::from_projection(path, projection, intent, query_context);

    hybrid_path_witness_recall_score_from_context(&ctx)
}

pub(super) fn merge_hybrid_lexical_search_output(
    base: &mut SearchExecutionOutput,
    supplement: SearchExecutionOutput,
    limit: usize,
) {
    let mut merged_by_key: BTreeMap<(String, String, usize, usize, String), TextMatch> =
        BTreeMap::new();
    for found in &base.matches {
        merged_by_key.insert(
            (
                found.repository_id.clone(),
                found.path.clone(),
                found.line,
                found.column,
                found.excerpt.clone(),
            ),
            found.clone(),
        );
    }
    for found in supplement.matches {
        merged_by_key
            .entry((
                found.repository_id.clone(),
                found.path.clone(),
                found.line,
                found.column,
                found.excerpt.clone(),
            ))
            .or_insert(found);
    }

    base.matches = merged_by_key.into_values().collect::<Vec<_>>();
    sort_matches_deterministically(&mut base.matches);
    base.matches.truncate(limit);

    base.diagnostics
        .entries
        .extend(supplement.diagnostics.entries);
    sort_search_diagnostics_deterministically(&mut base.diagnostics.entries);
    base.diagnostics.entries.dedup();
}

pub(super) fn semantic_excerpt(content_text: &str, fallback_path: &str) -> String {
    content_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback_path.to_owned())
}

pub(super) fn hybrid_path_has_exact_stem_match(path: &str, exact_terms: &[String]) -> bool {
    super::query_terms::hybrid_path_has_exact_stem_match(path, exact_terms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn witness_anchor_reader_keeps_best_matching_line() {
        let query_context = HybridPathWitnessQueryContext::new("build flow entrypoint");
        let source = Cursor::new(
            "\nplain header\nsetup unrelated values\nbuild entrypoint wires workflow\n",
        );

        let anchor = best_path_witness_anchor_in_reader("scripts/build.rs", source, &query_context);

        assert_eq!(
            anchor,
            Some((4, "build entrypoint wires workflow".to_owned()))
        );
    }

    #[test]
    fn witness_anchor_reader_falls_back_to_first_non_empty_line() {
        let query_context = HybridPathWitnessQueryContext::new("jobs listeners queue");
        let source = Cursor::new("\nheader line\nanother unrelated value\n");

        let anchor = best_path_witness_anchor_in_reader("docs/overview.md", source, &query_context);

        assert_eq!(anchor, Some((2, "header line".to_owned())));
    }

    #[test]
    fn path_witness_hits_preserve_score_hints_and_overlay_provenance() {
        let hits = build_hybrid_path_witness_hits_with_intent(
            &[TextMatch {
                repository_id: "repo-001".to_owned(),
                path: "tests/unit/user_service_test.rs".to_owned(),
                line: 7,
                column: 1,
                excerpt: "fn user_service_test() {}".to_owned(),
                witness_score_hint_millis: Some(4_200),
                witness_provenance_ids: Some(vec![
                    "overlay:test_subject:tests/unit/user_service_test.rs->src/user_service.rs"
                        .to_owned(),
                ]),
            }],
            &HybridRankingIntent::from_query("user service tests"),
            "user service tests",
        );

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw_score, 4.2);
        assert!(
            hits[0]
                .provenance_ids
                .iter()
                .any(|id| id == "path_witness:tests/unit/user_service_test.rs:7:1")
        );
        assert!(hits[0].provenance_ids.iter().any(|id| {
            id == "overlay:test_subject:tests/unit/user_service_test.rs->src/user_service.rs"
        }));
    }
}
