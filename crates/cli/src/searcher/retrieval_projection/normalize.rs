use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection,
};

const MAX_RELATIONS_PER_SOURCE: usize = 8;
const MAX_ANCHOR_SKETCHES_PER_PATH: usize = 3;
const MAX_SURFACE_TERMS_PER_PATH: usize = 24;

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

pub(crate) fn normalize_path_surface_term_projection_records(
    rows: &mut Vec<PathSurfaceTermProjection>,
) {
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

pub(crate) fn normalize_path_anchor_sketch_projection_records(
    rows: &mut Vec<PathAnchorSketchProjection>,
) {
    let mut grouped = std::collections::BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new();
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
