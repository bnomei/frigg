use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::domain::{
    EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, FriggResult, model::TextMatch,
};
use crate::graph::{HeuristicConfidence, RelationKind, SymbolGraph};
use crate::indexer::{
    PhpDeclarationRelation, SymbolDefinition, extract_php_graph_analysis_from_source,
    extract_symbols_for_paths, php_declaration_relation_edges_for_relations,
    php_heuristic_implementation_candidates_for_target, register_symbol_definitions,
    resolve_php_target_evidence_edges,
};
use crate::languages::{
    BladeSourceEvidence, PhpSourceEvidence, SymbolLanguage,
    extract_blade_source_evidence_from_source, php_symbol_indices_by_lower_name,
    php_symbol_indices_by_name, resolve_blade_relation_evidence_edges,
};
use blake3::Hasher as SignatureHasher;

use super::{
    HYBRID_GRAPH_CANDIDATE_POOL_MIN, HYBRID_GRAPH_CANDIDATE_POOL_MULTIPLIER,
    HYBRID_GRAPH_MAX_ANCHORS, HYBRID_GRAPH_MAX_NEIGHBORS_PER_ANCHOR, HybridChannelHit,
    HybridDocumentRef, HybridGraphFileAnalysis, HybridGraphFileAnalysisCacheKey,
    SearchCandidateUniverse, TextSearcher, hybrid_path_has_exact_stem_match,
    hybrid_query_exact_terms, normalize_repository_relative_path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HybridGraphAnchorKind {
    SymbolNameExact,
    FileStemExact,
    CanonicalFileSymbol,
}

#[derive(Debug, Clone)]
struct HybridGraphAnchor {
    symbol_id: String,
    symbol_name: String,
    symbol_path: String,
    symbol_line: usize,
    kind: HybridGraphAnchorKind,
    term: String,
}

#[derive(Debug, Clone, Default)]
struct HybridGraphSymbolIndex {
    symbols_by_relative_path: BTreeMap<String, Vec<usize>>,
    symbol_index_by_stable_id: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
struct HybridGraphCandidateExtraction {
    symbols: Vec<SymbolDefinition>,
    php_declaration_relations_by_relative_path: BTreeMap<String, Vec<PhpDeclarationRelation>>,
    php_evidence_by_relative_path: BTreeMap<String, PhpSourceEvidence>,
    blade_evidence_by_relative_path: BTreeMap<String, BladeSourceEvidence>,
}

pub(super) const HYBRID_GRAPH_ARTIFACT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct HybridGraphArtifactCacheKey {
    pub(super) repository_id: String,
    pub(super) snapshot_id: Option<String>,
    candidate_signature: String,
    artifact_version: u32,
}

#[derive(Debug, Clone)]
pub(super) struct HybridGraphArtifact {
    graph: Arc<SymbolGraph>,
    candidate_files: Vec<(String, PathBuf)>,
    symbols: Vec<SymbolDefinition>,
    symbol_index: HybridGraphSymbolIndex,
}

pub(super) fn search_graph_channel_hits(
    searcher: &TextSearcher,
    query_text: &str,
    candidate_universe: &SearchCandidateUniverse,
    lexical_matches: &[TextMatch],
    limit: usize,
) -> FriggResult<Vec<HybridChannelHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let exact_terms = hybrid_query_exact_terms(query_text);

    let graph_candidate_limit = limit
        .saturating_mul(HYBRID_GRAPH_CANDIDATE_POOL_MULTIPLIER)
        .max(HYBRID_GRAPH_CANDIDATE_POOL_MIN);
    let mut graph_hits = Vec::new();

    for repository in &candidate_universe.repositories {
        let allowed_paths = select_hybrid_graph_candidate_paths(
            repository,
            lexical_matches,
            &exact_terms,
            graph_candidate_limit,
        );
        if allowed_paths.is_empty() {
            continue;
        }

        let Some(artifact) = hybrid_graph_artifact(searcher, repository) else {
            continue;
        };
        let non_exact_seed_paths =
            select_non_exact_graph_seed_paths(repository, lexical_matches, &allowed_paths);

        let anchors = select_hybrid_graph_anchors(
            &artifact.symbols,
            &artifact.symbol_index.symbols_by_relative_path,
            &allowed_paths,
            &exact_terms,
            &non_exact_seed_paths,
        );
        if anchors.is_empty() {
            continue;
        }
        let selected_candidate_files = artifact
            .candidate_files
            .iter()
            .filter(|(relative_path, _)| allowed_paths.contains(relative_path))
            .cloned()
            .collect::<Vec<_>>();
        if selected_candidate_files.is_empty() {
            continue;
        }

        graph_hits.extend(build_hybrid_graph_channel_hits(
            &repository.repository_id,
            repository.root.as_path(),
            artifact.graph.as_ref(),
            &anchors,
            &selected_candidate_files,
            &artifact.symbols,
            &artifact.symbol_index,
            graph_candidate_limit,
            Some(&allowed_paths),
        ));
    }

    Ok(graph_hits)
}

fn language_supports_relation_evidence(language: SymbolLanguage) -> bool {
    matches!(language, SymbolLanguage::Php | SymbolLanguage::Blade)
}

fn select_hybrid_graph_anchors(
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    allowed_paths: &BTreeSet<String>,
    exact_terms: &[String],
    non_exact_seed_paths: &[String],
) -> Vec<HybridGraphAnchor> {
    let mut by_symbol = BTreeMap::<String, HybridGraphAnchor>::new();

    for symbol in allowed_paths.iter().flat_map(|relative_path| {
        symbols_by_relative_path
            .get(relative_path)
            .into_iter()
            .flatten()
            .map(|index| &symbols[*index])
    }) {
        let normalized_name = symbol.name.trim().to_ascii_lowercase();
        if normalized_name.is_empty() {
            continue;
        }
        let file_stem = symbol
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let matched_term = exact_terms
            .iter()
            .find(|term| **term == normalized_name)
            .cloned()
            .map(|term| (HybridGraphAnchorKind::SymbolNameExact, term))
            .or_else(|| {
                exact_terms
                    .iter()
                    .find(|term| **term == file_stem)
                    .cloned()
                    .map(|term| (HybridGraphAnchorKind::FileStemExact, term))
            });
        let Some((kind, term)) = matched_term else {
            continue;
        };

        let anchor = HybridGraphAnchor {
            symbol_id: symbol.stable_id.clone(),
            symbol_name: symbol.name.clone(),
            symbol_path: symbol.path.to_string_lossy().into_owned(),
            symbol_line: symbol.line,
            kind,
            term,
        };
        let replace = by_symbol
            .get(&symbol.stable_id)
            .is_none_or(|existing| hybrid_graph_anchor_order(&anchor, existing).is_lt());
        if replace {
            by_symbol.insert(symbol.stable_id.clone(), anchor);
        }
    }

    let mut anchors = by_symbol.into_values().collect::<Vec<_>>();
    anchors.sort_by(hybrid_graph_anchor_order);
    anchors.truncate(HYBRID_GRAPH_MAX_ANCHORS);
    if !anchors.is_empty() {
        return anchors;
    }

    let mut fallback_anchors = Vec::new();
    for relative_path in non_exact_seed_paths
        .iter()
        .filter(|relative_path| allowed_paths.contains(*relative_path))
        .take(HYBRID_GRAPH_MAX_ANCHORS)
    {
        fallback_anchors.extend(select_canonical_file_symbol_anchors(
            symbols,
            symbols_by_relative_path,
            relative_path,
        ));
        if fallback_anchors.len() >= HYBRID_GRAPH_MAX_ANCHORS {
            break;
        }
    }

    fallback_anchors.sort_by(hybrid_graph_anchor_order);
    fallback_anchors.dedup_by(|left, right| left.symbol_id == right.symbol_id);
    fallback_anchors.truncate(HYBRID_GRAPH_MAX_ANCHORS);
    fallback_anchors
}

fn select_canonical_file_symbol_anchors(
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    relative_path: &str,
) -> Vec<HybridGraphAnchor> {
    let Some(symbol_indices) = symbols_by_relative_path.get(relative_path) else {
        return Vec::new();
    };
    let file_stem = Path::new(relative_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if file_stem.is_empty() {
        return Vec::new();
    }

    let anchors = symbol_indices
        .iter()
        .filter_map(|index| {
            let symbol = symbols.get(*index)?;
            (symbol.name.trim().to_ascii_lowercase() == file_stem).then(|| HybridGraphAnchor {
                symbol_id: symbol.stable_id.clone(),
                symbol_name: symbol.name.clone(),
                symbol_path: symbol.path.to_string_lossy().into_owned(),
                symbol_line: symbol.line,
                kind: HybridGraphAnchorKind::CanonicalFileSymbol,
                term: file_stem.clone(),
            })
        })
        .collect::<Vec<_>>();
    if !anchors.is_empty() {
        return anchors;
    }

    if symbol_indices.len() != 1 {
        return Vec::new();
    }
    let Some(symbol) = symbols.get(symbol_indices[0]) else {
        return Vec::new();
    };
    vec![HybridGraphAnchor {
        symbol_id: symbol.stable_id.clone(),
        symbol_name: symbol.name.clone(),
        symbol_path: symbol.path.to_string_lossy().into_owned(),
        symbol_line: symbol.line,
        kind: HybridGraphAnchorKind::CanonicalFileSymbol,
        term: file_stem,
    }]
}

fn hybrid_graph_anchor_order(
    left: &HybridGraphAnchor,
    right: &HybridGraphAnchor,
) -> std::cmp::Ordering {
    hybrid_graph_anchor_kind_rank(left.kind)
        .cmp(&hybrid_graph_anchor_kind_rank(right.kind))
        .then(left.symbol_name.cmp(&right.symbol_name))
        .then(left.symbol_path.cmp(&right.symbol_path))
        .then(left.symbol_line.cmp(&right.symbol_line))
        .then(left.term.cmp(&right.term))
        .then(left.symbol_id.cmp(&right.symbol_id))
}

fn hybrid_graph_anchor_kind_rank(kind: HybridGraphAnchorKind) -> u8 {
    match kind {
        HybridGraphAnchorKind::SymbolNameExact => 0,
        HybridGraphAnchorKind::FileStemExact => 1,
        HybridGraphAnchorKind::CanonicalFileSymbol => 2,
    }
}

fn build_hybrid_graph_channel_hits(
    repository_id: &str,
    root: &Path,
    graph: &SymbolGraph,
    anchors: &[HybridGraphAnchor],
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
    symbol_index: &HybridGraphSymbolIndex,
    limit: usize,
    allowed_paths: Option<&BTreeSet<String>>,
) -> Vec<HybridChannelHit> {
    let mut hits = Vec::new();

    for anchor in anchors.iter().take(HYBRID_GRAPH_MAX_ANCHORS) {
        let anchor_relative_path =
            normalize_repository_relative_path(root, Path::new(&anchor.symbol_path));
        hits.push(HybridChannelHit {
            channel: EvidenceChannel::GraphPrecise,
            document: HybridDocumentRef {
                repository_id: repository_id.to_owned(),
                path: anchor_relative_path.clone(),
                line: anchor.symbol_line,
                column: 1,
            },
            anchor: EvidenceAnchor::new(
                EvidenceAnchorKind::Symbol,
                anchor.symbol_line,
                1,
                anchor.symbol_line,
                1,
            )
            .with_detail(anchor.symbol_name.clone()),
            raw_score: hybrid_graph_anchor_kind_score(anchor.kind),
            excerpt: anchor.symbol_name.clone(),
            provenance_ids: vec![format!(
                "graph:{}:anchor:{}:{}",
                anchor.term, anchor_relative_path, anchor.symbol_line
            )],
        });

        let mut adjacency = graph
            .incoming_adjacency(&anchor.symbol_id)
            .into_iter()
            .map(|adjacent| (0_u8, adjacent))
            .chain(
                graph
                    .outgoing_adjacency(&anchor.symbol_id)
                    .into_iter()
                    .map(|adjacent| (1_u8, adjacent)),
            )
            .collect::<Vec<_>>();
        adjacency.sort_by(|(left_direction, left), (right_direction, right)| {
            hybrid_graph_neighbor_order((*left_direction, left), (*right_direction, right))
        });
        adjacency.truncate(HYBRID_GRAPH_MAX_NEIGHBORS_PER_ANCHOR);

        for (direction_rank, adjacent) in adjacency {
            let adjacent_path = Path::new(&adjacent.symbol.path);
            let relative_path = normalize_repository_relative_path(root, adjacent_path);
            if allowed_paths.is_some_and(|paths| !paths.contains(&relative_path)) {
                continue;
            }
            let raw_score = hybrid_graph_anchor_kind_score(anchor.kind)
                * hybrid_graph_relation_score(adjacent.relation)
                * if direction_rank == 0 { 1.0 } else { 0.95 };
            hits.push(HybridChannelHit {
                channel: EvidenceChannel::GraphPrecise,
                document: HybridDocumentRef {
                    repository_id: repository_id.to_owned(),
                    path: relative_path.clone(),
                    line: adjacent.symbol.line,
                    column: 1,
                },
                anchor: EvidenceAnchor::new(
                    EvidenceAnchorKind::Symbol,
                    adjacent.symbol.line,
                    1,
                    adjacent.symbol.line,
                    1,
                )
                .with_detail(adjacent.symbol.display_name.clone()),
                raw_score,
                excerpt: adjacent.symbol.display_name.clone(),
                provenance_ids: vec![format!(
                    "graph:{}:{}:{}:{}",
                    anchor.term,
                    adjacent.relation.as_str(),
                    relative_path,
                    adjacent.symbol.line
                )],
            });
        }

        hits.extend(build_hybrid_graph_heuristic_implementation_hits(
            repository_id,
            root,
            anchor,
            candidate_files,
            symbols,
            symbol_index,
            allowed_paths,
        ));
    }

    hits.sort_by(|left, right| {
        right
            .raw_score
            .total_cmp(&left.raw_score)
            .then(left.document.cmp(&right.document))
            .then(left.provenance_ids.cmp(&right.provenance_ids))
            .then(left.excerpt.cmp(&right.excerpt))
    });
    hits.truncate(limit);
    hits
}

fn build_hybrid_graph_heuristic_implementation_hits(
    repository_id: &str,
    root: &Path,
    anchor: &HybridGraphAnchor,
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
    symbol_index: &HybridGraphSymbolIndex,
    allowed_paths: Option<&BTreeSet<String>>,
) -> Vec<HybridChannelHit> {
    if anchor.kind != HybridGraphAnchorKind::SymbolNameExact {
        return Vec::new();
    }

    let Some(target_symbol_index) = symbol_index
        .symbol_index_by_stable_id
        .get(&anchor.symbol_id)
        .copied()
    else {
        return Vec::new();
    };
    let target_symbol = &symbols[target_symbol_index];
    let implementation_candidates = php_heuristic_implementation_candidates_for_target(
        target_symbol,
        candidate_files,
        symbols,
        &symbol_index.symbols_by_relative_path,
        None,
        None,
    );
    if implementation_candidates.is_empty() {
        return Vec::new();
    }

    implementation_candidates
        .into_iter()
        .filter_map(|(source_symbol_index, relation)| {
            let symbol = symbols.get(source_symbol_index)?;
            let relative_path = normalize_repository_relative_path(root, &symbol.path);
            if allowed_paths.is_some_and(|paths| !paths.contains(&relative_path)) {
                return None;
            }
            Some(HybridChannelHit {
                channel: EvidenceChannel::GraphPrecise,
                document: HybridDocumentRef {
                    repository_id: repository_id.to_owned(),
                    path: relative_path.clone(),
                    line: symbol.line,
                    column: 1,
                },
                anchor: EvidenceAnchor::new(
                    EvidenceAnchorKind::Symbol,
                    symbol.line,
                    1,
                    symbol.line,
                    1,
                )
                .with_detail(symbol.name.clone()),
                raw_score: hybrid_graph_anchor_kind_score(anchor.kind)
                    * hybrid_graph_relation_score(relation)
                    * 0.92,
                excerpt: symbol.name.clone(),
                provenance_ids: vec![format!(
                    "graph:{}:heuristic:{}:{}:{}",
                    anchor.term,
                    relation.as_str(),
                    relative_path,
                    symbol.line
                )],
            })
        })
        .collect()
}

fn hybrid_graph_neighbor_order(
    left: (u8, &crate::graph::AdjacentSymbol),
    right: (u8, &crate::graph::AdjacentSymbol),
) -> std::cmp::Ordering {
    hybrid_graph_relation_rank(right.1.relation)
        .cmp(&hybrid_graph_relation_rank(left.1.relation))
        .then(left.0.cmp(&right.0))
        .then(left.1.symbol.path.cmp(&right.1.symbol.path))
        .then(left.1.symbol.line.cmp(&right.1.symbol.line))
        .then(left.1.symbol.display_name.cmp(&right.1.symbol.display_name))
}

fn hybrid_graph_anchor_kind_score(kind: HybridGraphAnchorKind) -> f32 {
    match kind {
        HybridGraphAnchorKind::SymbolNameExact => 1.0,
        HybridGraphAnchorKind::FileStemExact => 0.82,
        HybridGraphAnchorKind::CanonicalFileSymbol => 0.76,
    }
}

fn hybrid_graph_relation_score(relation: RelationKind) -> f32 {
    match HeuristicConfidence::from_relation(relation) {
        HeuristicConfidence::High => 1.0,
        HeuristicConfidence::Medium => 0.72,
        HeuristicConfidence::Low => 0.48,
    }
}

fn hybrid_graph_relation_rank(relation: RelationKind) -> u8 {
    match HeuristicConfidence::from_relation(relation) {
        HeuristicConfidence::High => 3,
        HeuristicConfidence::Medium => 2,
        HeuristicConfidence::Low => 1,
    }
}

fn build_hybrid_graph_symbol_index(
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
) -> HybridGraphSymbolIndex {
    let relative_path_by_absolute_path = candidate_files
        .iter()
        .map(|(relative_path, absolute_path)| (absolute_path.clone(), relative_path.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut index = HybridGraphSymbolIndex::default();

    for (symbol_index, symbol) in symbols.iter().enumerate() {
        if let Some(relative_path) = relative_path_by_absolute_path.get(&symbol.path) {
            index
                .symbols_by_relative_path
                .entry(relative_path.clone())
                .or_default()
                .push(symbol_index);
        }
        index
            .symbol_index_by_stable_id
            .insert(symbol.stable_id.clone(), symbol_index);
    }

    index
}

fn select_hybrid_graph_candidate_paths(
    repository: &super::RepositoryCandidateUniverse,
    lexical_matches: &[TextMatch],
    exact_terms: &[String],
    graph_candidate_limit: usize,
) -> BTreeSet<String> {
    let repository_id = repository.repository_id.as_str();
    let repository_root = repository.root.as_path();
    let mut candidates_by_relative_path = BTreeMap::new();
    let mut relation_candidate_count = 0usize;
    let mut exact_stem_relation_candidate_count = 0usize;

    for matched in lexical_matches {
        if matched.repository_id != repository_id {
            continue;
        }

        let absolute_path = repository_root.join(&matched.path);
        let Some(language) = SymbolLanguage::from_path(&absolute_path) else {
            continue;
        };
        if !absolute_path.exists() {
            continue;
        }

        if candidates_by_relative_path
            .insert(matched.path.clone(), absolute_path)
            .is_none()
            && language_supports_relation_evidence(language)
        {
            relation_candidate_count += 1;
            if hybrid_path_has_exact_stem_match(&matched.path, exact_terms) {
                exact_stem_relation_candidate_count += 1;
            }
        }
    }

    if exact_stem_relation_candidate_count == 0 || relation_candidate_count < 2 {
        for candidate in &repository.candidates {
            let relative_path = &candidate.relative_path;
            let absolute_path = &candidate.absolute_path;
            let Some(language) = SymbolLanguage::from_path(absolute_path) else {
                continue;
            };
            if !hybrid_path_has_exact_stem_match(relative_path, exact_terms) {
                continue;
            }

            if candidates_by_relative_path
                .insert(relative_path.clone(), absolute_path.clone())
                .is_none()
                && language_supports_relation_evidence(language)
            {
                relation_candidate_count += 1;
                exact_stem_relation_candidate_count += 1;
            }
            if candidates_by_relative_path.len() >= graph_candidate_limit
                || (exact_stem_relation_candidate_count > 0 && relation_candidate_count >= 2)
            {
                break;
            }
        }
    }

    candidates_by_relative_path.into_keys().collect()
}

fn select_non_exact_graph_seed_paths(
    repository: &super::RepositoryCandidateUniverse,
    lexical_matches: &[TextMatch],
    allowed_paths: &BTreeSet<String>,
) -> Vec<String> {
    let repository_root = repository.root.as_path();
    let mut seed_paths = Vec::new();
    let mut seen = BTreeSet::new();

    for matched in lexical_matches {
        if matched.repository_id != repository.repository_id {
            continue;
        }
        if !allowed_paths.contains(&matched.path) || !seen.insert(matched.path.clone()) {
            continue;
        }

        let absolute_path = repository_root.join(&matched.path);
        let Some(language) = SymbolLanguage::from_path(&absolute_path) else {
            continue;
        };
        if !language_supports_relation_evidence(language) {
            continue;
        }

        seed_paths.push(matched.path.clone());
        if seed_paths.len() >= HYBRID_GRAPH_MAX_ANCHORS {
            break;
        }
    }

    seed_paths
}

fn hybrid_graph_artifact(
    searcher: &TextSearcher,
    repository: &super::RepositoryCandidateUniverse,
) -> Option<Arc<HybridGraphArtifact>> {
    let (cache_key, candidate_files) = hybrid_graph_artifact_cache_material(repository)?;
    if let Some(cached) = searcher
        .hybrid_graph_artifact_cache
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&cache_key)
        .cloned()
    {
        return Some(cached);
    }

    let extracted = extract_graph_candidate_data(searcher, &candidate_files);
    if extracted.symbols.is_empty() {
        return None;
    }
    let symbol_index = build_hybrid_graph_symbol_index(&candidate_files, &extracted.symbols);

    let mut graph = SymbolGraph::default();
    register_symbol_definitions(&mut graph, &repository.repository_id, &extracted.symbols);
    register_search_php_declaration_relations(
        &mut graph,
        &extracted.php_declaration_relations_by_relative_path,
        &extracted.symbols,
        &symbol_index.symbols_by_relative_path,
    );
    register_search_php_target_evidence_relations(
        &mut graph,
        &extracted.symbols,
        &symbol_index,
        &extracted.php_evidence_by_relative_path,
    );
    register_search_blade_relation_evidence(
        &mut graph,
        &extracted.symbols,
        &symbol_index.symbols_by_relative_path,
        &extracted.blade_evidence_by_relative_path,
    );

    let artifact = Arc::new(HybridGraphArtifact {
        graph: Arc::new(graph),
        candidate_files,
        symbols: extracted.symbols,
        symbol_index,
    });
    let mut cache = searcher
        .hybrid_graph_artifact_cache
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|existing_key, _| {
        if existing_key.repository_id != cache_key.repository_id {
            return true;
        }
        match (&existing_key.snapshot_id, &cache_key.snapshot_id) {
            (Some(existing_snapshot), Some(current_snapshot)) => {
                existing_snapshot == current_snapshot
            }
            (None, None) => existing_key.candidate_signature == cache_key.candidate_signature,
            _ => false,
        }
    });
    cache.insert(cache_key, artifact.clone());
    Some(artifact)
}

fn hybrid_graph_artifact_cache_material(
    repository: &super::RepositoryCandidateUniverse,
) -> Option<(HybridGraphArtifactCacheKey, Vec<(String, PathBuf)>)> {
    let candidate_files = repository
        .candidates
        .iter()
        .filter_map(|candidate| {
            let language = SymbolLanguage::from_path(&candidate.absolute_path)?;
            if !language_supports_relation_evidence(language) {
                return None;
            }
            Some((
                candidate.relative_path.clone(),
                candidate.absolute_path.clone(),
            ))
        })
        .collect::<Vec<_>>();
    if candidate_files.is_empty() {
        return None;
    }

    let candidate_signature =
        hybrid_graph_candidate_signature(repository.snapshot_id.as_deref(), &candidate_files)?;
    Some((
        HybridGraphArtifactCacheKey {
            repository_id: repository.repository_id.clone(),
            snapshot_id: repository.snapshot_id.clone(),
            candidate_signature,
            artifact_version: HYBRID_GRAPH_ARTIFACT_VERSION,
        },
        candidate_files,
    ))
}

fn hybrid_graph_candidate_signature(
    snapshot_id: Option<&str>,
    candidate_files: &[(String, PathBuf)],
) -> Option<String> {
    let mut hasher = SignatureHasher::new();
    hasher.update(&HYBRID_GRAPH_ARTIFACT_VERSION.to_le_bytes());
    if let Some(snapshot_id) = snapshot_id {
        hasher.update(snapshot_id.as_bytes());
        hasher.update(&[0xff]);
    }
    for (relative_path, absolute_path) in candidate_files {
        hasher.update(relative_path.as_bytes());
        hasher.update(&[0x00]);
        if snapshot_id.is_none() {
            let metadata = hybrid_graph_file_analysis_cache_key(absolute_path)?;
            hasher.update(relative_path.as_bytes());
            hasher.update(&[0x01]);
            hasher.update(&metadata.modified_unix_nanos.to_le_bytes());
            hasher.update(&metadata.size_bytes.to_le_bytes());
        }
        hasher.update(&[0xfe]);
    }
    Some(hasher.finalize().to_hex().to_string())
}

fn extract_graph_candidate_data(
    searcher: &TextSearcher,
    candidate_files: &[(String, PathBuf)],
) -> HybridGraphCandidateExtraction {
    let mut extraction = HybridGraphCandidateExtraction::default();

    for (relative_path, absolute_path) in candidate_files {
        let Some(file_analysis) = hybrid_graph_file_analysis(searcher, absolute_path) else {
            continue;
        };
        extraction
            .symbols
            .extend(file_analysis.symbols.iter().cloned());
        if let Some(relations) = file_analysis.php_declaration_relations.as_ref() {
            extraction
                .php_declaration_relations_by_relative_path
                .insert(relative_path.clone(), relations.clone());
        }
        if let Some(evidence) = file_analysis.php_evidence.as_ref() {
            extraction
                .php_evidence_by_relative_path
                .insert(relative_path.clone(), evidence.clone());
        }
        if let Some(evidence) = file_analysis.blade_evidence.as_ref() {
            extraction
                .blade_evidence_by_relative_path
                .insert(relative_path.clone(), evidence.clone());
        }
    }

    extraction
        .symbols
        .sort_by(symbol_definition_order_in_graph_search);
    extraction
}

fn hybrid_graph_file_analysis(
    searcher: &TextSearcher,
    absolute_path: &Path,
) -> Option<std::sync::Arc<HybridGraphFileAnalysis>> {
    let cache_key = hybrid_graph_file_analysis_cache_key(absolute_path);
    if let Some(key) = cache_key.as_ref() {
        if let Some(cached) = searcher
            .hybrid_graph_file_analysis_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
        {
            return Some(cached);
        }
    }

    let analysis = std::sync::Arc::new(build_hybrid_graph_file_analysis(absolute_path)?);
    if let Some(key) = cache_key {
        let mut cache = searcher
            .hybrid_graph_file_analysis_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|existing_key, _| existing_key.path != key.path || *existing_key == key);
        cache.insert(key, analysis.clone());
    }
    Some(analysis)
}

fn hybrid_graph_file_analysis_cache_key(
    absolute_path: &Path,
) -> Option<HybridGraphFileAnalysisCacheKey> {
    let metadata = fs::metadata(absolute_path).ok()?;
    let modified_unix_nanos = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(HybridGraphFileAnalysisCacheKey {
        path: absolute_path.to_path_buf(),
        modified_unix_nanos,
        size_bytes: metadata.len(),
    })
}

fn build_hybrid_graph_file_analysis(absolute_path: &Path) -> Option<HybridGraphFileAnalysis> {
    match SymbolLanguage::from_path(absolute_path) {
        Some(SymbolLanguage::Php) => {
            let source = fs::read_to_string(absolute_path).ok()?;
            let analysis = extract_php_graph_analysis_from_source(absolute_path, &source).ok()?;
            Some(HybridGraphFileAnalysis {
                symbols: analysis.symbols,
                php_declaration_relations: Some(analysis.declaration_relations),
                php_evidence: Some(analysis.source_evidence),
                blade_evidence: None,
            })
        }
        Some(SymbolLanguage::Blade) => {
            let source = fs::read_to_string(absolute_path).ok()?;
            let mut extracted = extract_symbols_for_paths(&[absolute_path.to_path_buf()]);
            let blade_evidence =
                extract_blade_source_evidence_from_source(&source, &extracted.symbols);
            Some(HybridGraphFileAnalysis {
                symbols: std::mem::take(&mut extracted.symbols),
                php_declaration_relations: None,
                php_evidence: None,
                blade_evidence: Some(blade_evidence),
            })
        }
        Some(_) => {
            let mut extracted = extract_symbols_for_paths(&[absolute_path.to_path_buf()]);
            Some(HybridGraphFileAnalysis {
                symbols: std::mem::take(&mut extracted.symbols),
                php_declaration_relations: None,
                php_evidence: None,
                blade_evidence: None,
            })
        }
        None => None,
    }
}

fn symbol_definition_order_in_graph_search(
    left: &SymbolDefinition,
    right: &SymbolDefinition,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.span.start_byte.cmp(&right.span.start_byte))
        .then(left.span.end_byte.cmp(&right.span.end_byte))
        .then(left.kind.cmp(&right.kind))
        .then(left.name.cmp(&right.name))
        .then(left.stable_id.cmp(&right.stable_id))
}

fn clone_file_symbols(
    relative_path: &str,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
) -> Vec<SymbolDefinition> {
    symbols_by_relative_path
        .get(relative_path)
        .into_iter()
        .flatten()
        .map(|index| symbols[*index].clone())
        .collect()
}

fn register_search_php_declaration_relations(
    graph: &mut SymbolGraph,
    php_declaration_relations_by_relative_path: &BTreeMap<String, Vec<PhpDeclarationRelation>>,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
) {
    let symbol_indices_by_name = php_symbol_indices_by_name(symbols);
    let symbol_indices_by_lower_name = php_symbol_indices_by_lower_name(symbols);
    for (relative_path, relations) in php_declaration_relations_by_relative_path {
        let edges = php_declaration_relation_edges_for_relations(
            relative_path,
            symbols,
            symbols_by_relative_path,
            Some(&symbol_indices_by_name),
            Some(&symbol_indices_by_lower_name),
            relations,
        );
        if edges.is_empty() {
            continue;
        }

        for (source_symbol_index, target_symbol_index, relation) in edges {
            let source_symbol = &symbols[source_symbol_index];
            let target_symbol = &symbols[target_symbol_index];
            if source_symbol.stable_id == target_symbol.stable_id {
                continue;
            }
            let _ =
                graph.add_relation(&source_symbol.stable_id, &target_symbol.stable_id, relation);
        }
    }
}

fn register_search_php_target_evidence_relations(
    graph: &mut SymbolGraph,
    symbols: &[SymbolDefinition],
    symbol_index: &HybridGraphSymbolIndex,
    php_evidence_by_relative_path: &BTreeMap<String, PhpSourceEvidence>,
) {
    let mut canonical_symbol_name_by_stable_id = BTreeMap::new();

    for evidence in php_evidence_by_relative_path.values() {
        canonical_symbol_name_by_stable_id.extend(evidence.canonical_names_by_stable_id.clone());
    }

    let mut symbol_indices_by_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    for (stable_id, canonical_name) in &canonical_symbol_name_by_stable_id {
        let Some(symbol_index) = symbol_index
            .symbol_index_by_stable_id
            .get(stable_id)
            .copied()
        else {
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

    for evidence in php_evidence_by_relative_path.values() {
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_php_target_evidence_edges(
                symbols,
                &symbol_index.symbol_index_by_stable_id,
                &symbol_indices_by_canonical_name,
                &symbol_indices_by_lower_canonical_name,
                evidence,
            )
        {
            let source_symbol = &symbols[source_symbol_index];
            let target_symbol = &symbols[target_symbol_index];
            if source_symbol.stable_id == target_symbol.stable_id {
                continue;
            }
            let _ =
                graph.add_relation(&source_symbol.stable_id, &target_symbol.stable_id, relation);
        }
    }
}

fn register_search_blade_relation_evidence(
    graph: &mut SymbolGraph,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    blade_evidence_by_relative_path: &BTreeMap<String, BladeSourceEvidence>,
) {
    let mut symbol_index_by_stable_id = BTreeMap::<String, usize>::new();
    let mut symbol_indices_by_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_name = BTreeMap::<String, Vec<usize>>::new();
    for (index, symbol) in symbols.iter().enumerate() {
        symbol_index_by_stable_id.insert(symbol.stable_id.clone(), index);
        symbol_indices_by_name
            .entry(symbol.name.clone())
            .or_default()
            .push(index);
        symbol_indices_by_lower_name
            .entry(symbol.name.to_ascii_lowercase())
            .or_default()
            .push(index);
    }

    for (relative_path, evidence) in blade_evidence_by_relative_path {
        if clone_file_symbols(relative_path, symbols, symbols_by_relative_path).is_empty() {
            continue;
        }
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_blade_relation_evidence_edges(
                symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_name,
                &symbol_indices_by_lower_name,
                evidence,
            )
        {
            let source_symbol = &symbols[source_symbol_index];
            let target_symbol = &symbols[target_symbol_index];
            if source_symbol.stable_id == target_symbol.stable_id {
                continue;
            }
            let _ =
                graph.add_relation(&source_symbol.stable_id, &target_symbol.stable_id, relation);
        }
    }
}
