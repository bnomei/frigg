use std::collections::BTreeMap;

use crate::domain::{EvidenceAnchor, EvidenceChannel, FriggResult};

use super::{HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridRankedEvidence};

#[derive(Debug, Clone)]
struct HybridScoreAccumulator {
    document: HybridDocumentRef,
    anchor: EvidenceAnchor,
    excerpt: String,
    excerpt_channel_priority: usize,
    lexical_score: f32,
    witness_score: f32,
    graph_score: f32,
    semantic_score: f32,
    lexical_sources: Vec<String>,
    witness_sources: Vec<String>,
    graph_sources: Vec<String>,
    semantic_sources: Vec<String>,
}

fn blended_lexical_family_score(lexical_score: f32, witness_score: f32) -> f32 {
    let lexical_score = lexical_score.clamp(0.0, 1.0);
    let witness_score = witness_score.clamp(0.0, 1.0);
    if witness_score <= 0.0 {
        lexical_score
    } else {
        (lexical_score * 0.7 + witness_score * 0.3).clamp(0.0, 1.0)
    }
}

fn excerpt_channel_priority(channel: EvidenceChannel) -> usize {
    match channel {
        EvidenceChannel::LexicalManifest => 4,
        EvidenceChannel::PathSurfaceWitness => 3,
        EvidenceChannel::GraphPrecise => 2,
        EvidenceChannel::Semantic => 1,
    }
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
            blended_score: blended_lexical_family_score(state.lexical_score, state.witness_score)
                * weights.lexical,
            lexical_score: state.lexical_score,
            witness_score: state.witness_score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: state.lexical_sources,
            witness_sources: state.witness_sources,
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .blended_score
            .total_cmp(&left.blended_score)
            .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
            .then_with(|| right.witness_score.total_cmp(&left.witness_score))
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
            let blended_score =
                (blended_lexical_family_score(state.lexical_score, state.witness_score)
                    * weights.lexical)
                    + (state.graph_score * weights.graph)
                    + (state.semantic_score * weights.semantic);
            HybridRankedEvidence {
                document: state.document,
                anchor: state.anchor,
                excerpt: state.excerpt,
                blended_score,
                lexical_score: state.lexical_score,
                witness_score: state.witness_score,
                graph_score: state.graph_score,
                semantic_score: state.semantic_score,
                lexical_sources: state.lexical_sources,
                witness_sources: state.witness_sources,
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
            .then_with(|| right.witness_score.total_cmp(&left.witness_score))
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
    let mut grouped = group_all_hybrid_ranked_evidence(ranked_anchors, weights);
    grouped.truncate(limit);
    grouped
}

pub(super) fn group_all_hybrid_ranked_evidence(
    ranked_anchors: Vec<HybridRankedEvidence>,
    weights: HybridChannelWeights,
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
                let replace_representative = should_replace_group_representative(winner, &anchor);
                let representative_document = anchor.document.clone();
                let representative_anchor = anchor.anchor.clone();
                let representative_excerpt = anchor.excerpt.clone();
                winner.lexical_score = corroborate_channel_score(
                    winner.lexical_score,
                    anchor.lexical_score,
                    corroborating_anchor_count,
                );
                winner.witness_score = corroborate_channel_score(
                    winner.witness_score,
                    anchor.witness_score,
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
                winner.blended_score =
                    (blended_lexical_family_score(winner.lexical_score, winner.witness_score)
                        * weights.lexical)
                        + (winner.graph_score * weights.graph)
                        + (winner.semantic_score * weights.semantic);
                for source in anchor.lexical_sources {
                    insert_sorted_unique(&mut winner.lexical_sources, source);
                }
                for source in anchor.witness_sources {
                    insert_sorted_unique(&mut winner.witness_sources, source);
                }
                for source in anchor.graph_sources {
                    insert_sorted_unique(&mut winner.graph_sources, source);
                }
                for source in anchor.semantic_sources {
                    insert_sorted_unique(&mut winner.semantic_sources, source);
                }
                if replace_representative {
                    winner.document = representative_document;
                    winner.anchor = representative_anchor;
                    winner.excerpt = representative_excerpt;
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
            .then_with(|| right.witness_score.total_cmp(&left.witness_score))
            .then_with(|| right.graph_score.total_cmp(&left.graph_score))
            .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
            .then(left.document.cmp(&right.document))
            .then(left.anchor.cmp(&right.anchor))
            .then(left.excerpt.cmp(&right.excerpt))
    });
    grouped
}

fn should_replace_group_representative(
    current: &HybridRankedEvidence,
    candidate: &HybridRankedEvidence,
) -> bool {
    let current_anchor_priority = representative_anchor_priority(current);
    let candidate_anchor_priority = representative_anchor_priority(candidate);
    candidate_anchor_priority > current_anchor_priority
}

fn representative_anchor_priority(entry: &HybridRankedEvidence) -> (usize, usize, i32, i32, i32) {
    let has_witness = entry.witness_score > 0.0;
    let has_lexical = entry.lexical_score > 0.0;
    let has_graph = entry.graph_score > 0.0;
    let has_semantic = entry.semantic_score > 0.0;

    let channel_tier = if has_lexical {
        4
    } else if has_witness {
        3
    } else if has_graph {
        2
    } else if has_semantic {
        1
    } else {
        0
    };
    let corroboration_tier = usize::from(has_witness || has_lexical || has_graph);
    let excerpt_token_tier = representative_excerpt_token_count(&entry.excerpt) as i32;
    let excerpt_len_tier = entry.excerpt.len() as i32;
    let lexical_family_tier =
        ((entry.lexical_score.max(entry.witness_score)).clamp(0.0, 1.0) * 1000.0).round() as i32;
    let graph_tier = (entry.graph_score.clamp(0.0, 1.0) * 1000.0).round() as i32;

    (
        corroboration_tier,
        excerpt_token_tier.max(0) as usize,
        excerpt_len_tier,
        channel_tier,
        lexical_family_tier.max(graph_tier),
    )
}

fn representative_excerpt_token_count(excerpt: &str) -> usize {
    excerpt
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .count()
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
                excerpt_channel_priority: excerpt_channel_priority(hit.channel),
                lexical_score: 0.0,
                witness_score: 0.0,
                graph_score: 0.0,
                semantic_score: 0.0,
                lexical_sources: Vec::new(),
                witness_sources: Vec::new(),
                graph_sources: Vec::new(),
                semantic_sources: Vec::new(),
            });

        let hit_excerpt_priority = excerpt_channel_priority(hit.channel);
        if state.excerpt.is_empty()
            || hit_excerpt_priority > state.excerpt_channel_priority
            || (hit_excerpt_priority == state.excerpt_channel_priority
                && representative_excerpt_token_count(&hit.excerpt)
                    > representative_excerpt_token_count(&state.excerpt))
        {
            state.excerpt = hit.excerpt.clone();
            state.excerpt_channel_priority = hit_excerpt_priority;
        }

        match hit.channel {
            EvidenceChannel::LexicalManifest => {
                if normalized_score > state.lexical_score {
                    state.lexical_score = normalized_score;
                }
                for provenance_id in hit.provenance_ids {
                    insert_sorted_unique(&mut state.lexical_sources, provenance_id);
                }
            }
            EvidenceChannel::PathSurfaceWitness => {
                if normalized_score > state.witness_score {
                    state.witness_score = normalized_score;
                }
                for provenance_id in hit.provenance_ids {
                    insert_sorted_unique(&mut state.witness_sources, provenance_id);
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
    use crate::domain::{
        EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, EvidenceDocumentRef, EvidenceHit,
    };

    fn ranked_evidence(
        path: &str,
        line: usize,
        blended_score: f32,
        lexical_score: f32,
    ) -> HybridRankedEvidence {
        ranked_evidence_with_channels(path, line, blended_score, lexical_score, 0.0)
    }

    fn ranked_evidence_with_channels(
        path: &str,
        line: usize,
        blended_score: f32,
        lexical_score: f32,
        witness_score: f32,
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
            witness_score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: vec![format!("lexical:{path}:{line}")],
            witness_sources: if witness_score > 0.0 {
                vec![format!("witness:{path}:{line}")]
            } else {
                Vec::new()
            },
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        }
    }

    fn evidence_hit(
        path: &str,
        line: usize,
        channel: EvidenceChannel,
        raw_score: f32,
    ) -> EvidenceHit {
        EvidenceHit {
            channel,
            document: EvidenceDocumentRef {
                repository_id: "repo-001".to_owned(),
                path: path.to_owned(),
                line,
                column: 1,
            },
            anchor: EvidenceAnchor::new(EvidenceAnchorKind::TextSpan, line, 1, line, 24),
            raw_score,
            excerpt: format!("{path}:{line}"),
            provenance_ids: vec![format!("{}:{path}:{line}", channel.as_str())],
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

    #[test]
    fn hybrid_ranking_routes_path_surface_witness_hits_into_witness_channel() {
        let weights = HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        };
        let ranked = blend_hybrid_evidence(
            &[
                evidence_hit("src/server.rs", 12, EvidenceChannel::LexicalManifest, 0.8),
                evidence_hit(
                    "src/server.rs",
                    12,
                    EvidenceChannel::PathSurfaceWitness,
                    0.6,
                ),
            ],
            &[],
            &[],
            weights,
        )
        .expect("hybrid blend should succeed");

        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].lexical_score > 0.0);
        assert!(ranked[0].witness_score > 0.0);
        assert_eq!(
            ranked[0].lexical_sources,
            vec!["lexical_manifest:src/server.rs:12".to_owned()]
        );
        assert_eq!(
            ranked[0].witness_sources,
            vec!["path_surface_witness:src/server.rs:12".to_owned()]
        );
    }

    #[test]
    fn hybrid_ranking_without_witness_hits_preserves_lexical_only_behavior() {
        let weights = HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        };
        let lexical_only = rank_lexical_hybrid_hits(
            &[evidence_hit(
                "src/server.rs",
                12,
                EvidenceChannel::LexicalManifest,
                0.8,
            )],
            weights,
        )
        .expect("lexical ranking should succeed");
        let blended = blend_hybrid_evidence(
            &[evidence_hit(
                "src/server.rs",
                12,
                EvidenceChannel::LexicalManifest,
                0.8,
            )],
            &[],
            &[],
            weights,
        )
        .expect("hybrid blend should succeed");

        assert_eq!(blended.len(), 1);
        assert_eq!(blended[0].lexical_score, lexical_only[0].lexical_score);
        assert_eq!(blended[0].blended_score, lexical_only[0].blended_score);
        assert_eq!(blended[0].witness_score, 0.0);
        assert!(blended[0].witness_sources.is_empty());
    }

    #[test]
    fn hybrid_ranking_document_aggregation_corroborates_witness_channel() {
        let weights = HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        };
        let corroborated = ranked_evidence_with_channels("src/a.rs", 10, 0.68, 0.60, 0.40);
        let corroborating = ranked_evidence_with_channels("src/a.rs", 30, 0.65, 0.55, 0.35);

        let grouped = group_hybrid_ranked_evidence(
            vec![corroborated.clone(), corroborating.clone()],
            weights,
            10,
        );

        assert_eq!(grouped.len(), 1);
        assert!(grouped[0].witness_score > corroborated.witness_score);
        assert_eq!(
            grouped[0].witness_sources,
            vec![
                "witness:src/a.rs:10".to_owned(),
                "witness:src/a.rs:30".to_owned(),
            ]
        );
    }
}
