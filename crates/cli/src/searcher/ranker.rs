use std::collections::BTreeMap;

use crate::domain::FriggResult;

use super::{
    HybridChannel, HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridRankedEvidence,
};

#[derive(Debug, Clone)]
struct HybridScoreAccumulator {
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

    let mut ranked = blend_hybrid_evidence(lexical_hits, graph_hits, semantic_hits, weights)?;
    ranked.truncate(limit);

    Ok(ranked)
}

pub(super) fn blend_hybrid_evidence(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    let weights = weights.validate()?;
    let mut by_document: BTreeMap<HybridDocumentRef, HybridScoreAccumulator> = BTreeMap::new();

    apply_hybrid_channel_hits(lexical_hits, HybridChannel::Lexical, &mut by_document);
    apply_hybrid_channel_hits(graph_hits, HybridChannel::Graph, &mut by_document);
    apply_hybrid_channel_hits(semantic_hits, HybridChannel::Semantic, &mut by_document);

    let mut ranked = by_document
        .into_iter()
        .map(|(document, state)| {
            let blended_score = (state.lexical_score * weights.lexical)
                + (state.graph_score * weights.graph)
                + (state.semantic_score * weights.semantic);
            HybridRankedEvidence {
                document,
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
            .then(left.excerpt.cmp(&right.excerpt))
    });

    Ok(ranked)
}

fn apply_hybrid_channel_hits(
    hits: &[HybridChannelHit],
    channel: HybridChannel,
    by_document: &mut BTreeMap<HybridDocumentRef, HybridScoreAccumulator>,
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
            .then_with(|| right.raw_score.total_cmp(&left.raw_score))
            .then(left.provenance_id.cmp(&right.provenance_id))
            .then(left.excerpt.cmp(&right.excerpt))
    });

    for hit in ordered_hits {
        let normalized_score = normalize_channel_score(hit.raw_score, max_raw_score);
        let state =
            by_document
                .entry(hit.document.clone())
                .or_insert_with(|| HybridScoreAccumulator {
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

        match channel {
            HybridChannel::Lexical => {
                if normalized_score > state.lexical_score {
                    state.lexical_score = normalized_score;
                }
                insert_sorted_unique(&mut state.lexical_sources, hit.provenance_id);
            }
            HybridChannel::Graph => {
                if normalized_score > state.graph_score {
                    state.graph_score = normalized_score;
                }
                insert_sorted_unique(&mut state.graph_sources, hit.provenance_id);
            }
            HybridChannel::Semantic => {
                if normalized_score > state.semantic_score {
                    state.semantic_score = normalized_score;
                }
                insert_sorted_unique(&mut state.semantic_sources, hit.provenance_id);
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
