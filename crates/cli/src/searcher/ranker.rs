use std::collections::BTreeMap;

use crate::domain::{EvidenceAnchor, EvidenceChannel, FriggResult};

use super::{HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridRankedEvidence};

#[derive(Debug, Clone)]
struct HybridScoreAccumulator {
    document: HybridDocumentRef,
    anchor: EvidenceAnchor,
    excerpt: String,
    lexical_score: f32,
    graph_score: f32,
    semantic_score: f32,
    lexical_sources: Vec<String>,
    graph_sources: Vec<String>,
    semantic_sources: Vec<String>,
}

pub fn rank_hybrid_evidence(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
    limit: usize,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let ranked_anchors = blend_hybrid_evidence(lexical_hits, graph_hits, semantic_hits, weights)?;
    Ok(group_hybrid_ranked_evidence(ranked_anchors, weights, limit))
}

pub(super) fn rank_lexical_hybrid_hits(
    lexical_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    let weights = weights.validate()?;
    let mut by_anchor: BTreeMap<(HybridDocumentRef, EvidenceAnchor), HybridScoreAccumulator> =
        BTreeMap::new();
    apply_hybrid_channel_hits(lexical_hits, &mut by_anchor);

    let mut ranked = by_anchor
        .into_values()
        .map(|state| HybridRankedEvidence {
            document: state.document,
            anchor: state.anchor,
            excerpt: state.excerpt,
            blended_score: state.lexical_score * weights.lexical,
            lexical_score: state.lexical_score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: state.lexical_sources,
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .blended_score
            .total_cmp(&left.blended_score)
            .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
            .then(left.document.cmp(&right.document))
            .then(left.anchor.cmp(&right.anchor))
            .then(left.excerpt.cmp(&right.excerpt))
    });

    Ok(ranked)
}

pub(super) fn blend_hybrid_evidence(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    let weights = weights.validate()?;
    let mut by_anchor: BTreeMap<(HybridDocumentRef, EvidenceAnchor), HybridScoreAccumulator> =
        BTreeMap::new();

    apply_hybrid_channel_hits(lexical_hits, &mut by_anchor);
    apply_hybrid_channel_hits(graph_hits, &mut by_anchor);
    apply_hybrid_channel_hits(semantic_hits, &mut by_anchor);

    let mut ranked = by_anchor
        .into_values()
        .map(|state| {
            let blended_score = (state.lexical_score * weights.lexical)
                + (state.graph_score * weights.graph)
                + (state.semantic_score * weights.semantic);
            HybridRankedEvidence {
                document: state.document,
                anchor: state.anchor,
                excerpt: state.excerpt,
                blended_score,
                lexical_score: state.lexical_score,
                graph_score: state.graph_score,
                semantic_score: state.semantic_score,
                lexical_sources: state.lexical_sources,
                graph_sources: state.graph_sources,
                semantic_sources: state.semantic_sources,
            }
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .blended_score
            .total_cmp(&left.blended_score)
            .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
            .then_with(|| right.graph_score.total_cmp(&left.graph_score))
            .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
            .then(left.document.cmp(&right.document))
            .then(left.anchor.cmp(&right.anchor))
            .then(left.excerpt.cmp(&right.excerpt))
    });

    Ok(ranked)
}

pub(super) fn group_hybrid_ranked_evidence(
    ranked_anchors: Vec<HybridRankedEvidence>,
    weights: HybridChannelWeights,
    limit: usize,
) -> Vec<HybridRankedEvidence> {
    #[derive(Clone)]
    struct GroupedEvidence {
        winner: HybridRankedEvidence,
        corroborating_anchor_count: usize,
    }

    let mut grouped_by_document = BTreeMap::<(String, String), GroupedEvidence>::new();
    for anchor in ranked_anchors {
        let key = (
            anchor.document.repository_id.clone(),
            anchor.document.path.clone(),
        );
        match grouped_by_document.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(GroupedEvidence {
                    winner: anchor,
                    corroborating_anchor_count: 0,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let grouped = entry.get_mut();
                let corroborating_anchor_count = grouped.corroborating_anchor_count;
                let winner = &mut grouped.winner;
                winner.lexical_score = corroborate_channel_score(
                    winner.lexical_score,
                    anchor.lexical_score,
                    corroborating_anchor_count,
                );
                winner.graph_score = corroborate_channel_score(
                    winner.graph_score,
                    anchor.graph_score,
                    corroborating_anchor_count,
                );
                winner.semantic_score = corroborate_channel_score(
                    winner.semantic_score,
                    anchor.semantic_score,
                    corroborating_anchor_count,
                );
                winner.blended_score = (winner.lexical_score * weights.lexical)
                    + (winner.graph_score * weights.graph)
                    + (winner.semantic_score * weights.semantic);
                for source in anchor.lexical_sources {
                    insert_sorted_unique(&mut winner.lexical_sources, source);
                }
                for source in anchor.graph_sources {
                    insert_sorted_unique(&mut winner.graph_sources, source);
                }
                for source in anchor.semantic_sources {
                    insert_sorted_unique(&mut winner.semantic_sources, source);
                }
                grouped.corroborating_anchor_count =
                    grouped.corroborating_anchor_count.saturating_add(1);
            }
        }
    }

    let mut grouped = grouped_by_document
        .into_values()
        .map(|grouped| grouped.winner)
        .collect::<Vec<_>>();
    grouped.sort_by(|left, right| {
        right
            .blended_score
            .total_cmp(&left.blended_score)
            .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
            .then_with(|| right.graph_score.total_cmp(&left.graph_score))
            .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
            .then(left.document.cmp(&right.document))
            .then(left.anchor.cmp(&right.anchor))
            .then(left.excerpt.cmp(&right.excerpt))
    });
    grouped.truncate(limit);
    grouped
}

fn corroborate_channel_score(
    current: f32,
    supporting: f32,
    corroborating_anchor_count: usize,
) -> f32 {
    let current = current.clamp(0.0, 1.0);
    let supporting = supporting.clamp(0.0, 1.0);
    if supporting <= 0.0 {
        return current;
    }

    let corroboration_weight =
        (0.35_f32 / (corroborating_anchor_count as f32 + 1.0)).clamp(0.05, 0.35);
    (1.0 - ((1.0 - current) * (1.0 - supporting * corroboration_weight))).clamp(current, 1.0)
}

fn apply_hybrid_channel_hits(
    hits: &[HybridChannelHit],
    by_anchor: &mut BTreeMap<(HybridDocumentRef, EvidenceAnchor), HybridScoreAccumulator>,
) {
    if hits.is_empty() {
        return;
    }

    let max_raw_score = hits
        .iter()
        .map(|hit| hit.raw_score.max(0.0))
        .fold(0.0_f32, f32::max);
    let mut ordered_hits = hits.to_vec();
    ordered_hits.sort_by(|left, right| {
        left.document
            .cmp(&right.document)
            .then(left.anchor.cmp(&right.anchor))
            .then_with(|| right.raw_score.total_cmp(&left.raw_score))
            .then(left.provenance_ids.cmp(&right.provenance_ids))
            .then(left.excerpt.cmp(&right.excerpt))
    });

    for hit in ordered_hits {
        let normalized_score = normalize_channel_score(hit.raw_score, max_raw_score);
        let state = by_anchor
            .entry((hit.document.clone(), hit.anchor.clone()))
            .or_insert_with(|| HybridScoreAccumulator {
                document: hit.document.clone(),
                anchor: hit.anchor.clone(),
                excerpt: hit.excerpt.clone(),
                lexical_score: 0.0,
                graph_score: 0.0,
                semantic_score: 0.0,
                lexical_sources: Vec::new(),
                graph_sources: Vec::new(),
                semantic_sources: Vec::new(),
            });

        if state.excerpt.is_empty() {
            state.excerpt = hit.excerpt.clone();
        }

        match hit.channel {
            EvidenceChannel::LexicalManifest | EvidenceChannel::PathSurfaceWitness => {
                if normalized_score > state.lexical_score {
                    state.lexical_score = normalized_score;
                }
                for provenance_id in hit.provenance_ids {
                    insert_sorted_unique(&mut state.lexical_sources, provenance_id);
                }
            }
            EvidenceChannel::GraphPrecise => {
                if normalized_score > state.graph_score {
                    state.graph_score = normalized_score;
                }
                for provenance_id in hit.provenance_ids {
                    insert_sorted_unique(&mut state.graph_sources, provenance_id);
                }
            }
            EvidenceChannel::Semantic => {
                if normalized_score > state.semantic_score {
                    state.semantic_score = normalized_score;
                }
                for provenance_id in hit.provenance_ids {
                    insert_sorted_unique(&mut state.semantic_sources, provenance_id);
                }
            }
        }
    }
}

fn normalize_channel_score(raw_score: f32, max_raw_score: f32) -> f32 {
    if max_raw_score <= 0.0 {
        return 0.0;
    }

    (raw_score.max(0.0) / max_raw_score).clamp(0.0, 1.0)
}

fn insert_sorted_unique(values: &mut Vec<String>, value: String) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(index) => values.insert(index, value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceDocumentRef};

    fn ranked_evidence(
        path: &str,
        line: usize,
        blended_score: f32,
        lexical_score: f32,
    ) -> HybridRankedEvidence {
        HybridRankedEvidence {
            document: EvidenceDocumentRef {
                repository_id: "repo-001".to_owned(),
                path: path.to_owned(),
                line,
                column: 1,
            },
            anchor: EvidenceAnchor::new(EvidenceAnchorKind::TextSpan, line, 1, line, 24),
            excerpt: format!("{path}:{line}"),
            blended_score,
            lexical_score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: vec![format!("lexical:{path}:{line}")],
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        }
    }

    #[test]
    fn hybrid_ranking_document_aggregation_promotes_corroborating_anchors_without_replacing_winner()
    {
        let corroborated = ranked_evidence("src/a.rs", 10, 0.68, 0.68);
        let corroborating = ranked_evidence("src/a.rs", 30, 0.65, 0.65);
        let competing = ranked_evidence("src/b.rs", 20, 0.72, 0.72);
        let weights = HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        };

        let grouped = group_hybrid_ranked_evidence(
            vec![
                competing.clone(),
                corroborated.clone(),
                corroborating.clone(),
            ],
            weights,
            10,
        );

        assert_eq!(grouped[0].document.path, "src/a.rs");
        assert_eq!(grouped[0].anchor.start_line, corroborated.anchor.start_line);
        assert!(
            grouped[0].blended_score > competing.blended_score,
            "corroborating anchors should be able to lift the winning document above a single-peak competitor: {grouped:?}"
        );
        assert_eq!(
            grouped[0].lexical_sources,
            vec![
                "lexical:src/a.rs:10".to_owned(),
                "lexical:src/a.rs:30".to_owned(),
            ]
        );
    }
}
