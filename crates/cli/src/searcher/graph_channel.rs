use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{FriggResult, model::TextMatch};
use crate::graph::{HeuristicConfidence, RelationKind, SymbolGraph};
use crate::indexer::{
    SymbolDefinition, extract_blade_source_evidence_from_source,
    extract_php_source_evidence_from_source, extract_symbols_for_paths,
    php_declaration_relation_edges_for_file, register_symbol_definitions,
    resolve_blade_relation_evidence_edges, resolve_php_target_evidence_edges,
};
use crate::language_support::SymbolLanguage;

use super::{
    HYBRID_GRAPH_CANDIDATE_POOL_MIN, HYBRID_GRAPH_CANDIDATE_POOL_MULTIPLIER,
    HYBRID_GRAPH_MAX_ANCHORS, HYBRID_GRAPH_MAX_NEIGHBORS_PER_ANCHOR, HybridChannelHit,
    HybridDocumentRef, SearchExecutionDiagnostics, SearchFilters, SearchTextQuery, TextSearcher,
    hybrid_path_has_exact_stem_match, hybrid_query_exact_terms, normalize_repository_relative_path,
    normalize_search_filters,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HybridGraphAnchorKind {
    SymbolNameExact,
    FileStemExact,
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

pub(super) fn search_graph_channel_hits(
    searcher: &TextSearcher,
    query_text: &str,
    filters: &SearchFilters,
    lexical_matches: &[TextMatch],
    limit: usize,
) -> FriggResult<Vec<HybridChannelHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let exact_terms = hybrid_query_exact_terms(query_text);
    if exact_terms.is_empty() {
        return Ok(Vec::new());
    }

    let normalized_filters = normalize_search_filters(filters.clone())?;
    let graph_candidate_limit = limit
        .saturating_mul(HYBRID_GRAPH_CANDIDATE_POOL_MULTIPLIER)
        .max(HYBRID_GRAPH_CANDIDATE_POOL_MIN);
    let mut graph_hits = Vec::new();

    let mut repositories = searcher.config.repositories();
    repositories.sort_by(|left, right| {
        left.repository_id
            .cmp(&right.repository_id)
            .then(left.root_path.cmp(&right.root_path))
    });
    for repo in &repositories {
        if normalized_filters
            .repository_id
            .as_ref()
            .is_some_and(|repository_id| repository_id != &repo.repository_id.0)
        {
            continue;
        }

        let repository_id = repo.repository_id.0.clone();
        let repository_root = Path::new(&repo.root_path);
        let mut candidates_by_relative_path = BTreeMap::new();
        for matched in lexical_matches {
            if matched.repository_id != repository_id {
                continue;
            }

            let absolute_path = repository_root.join(&matched.path);
            if SymbolLanguage::from_path(&absolute_path).is_none() || !absolute_path.exists() {
                continue;
            }

            candidates_by_relative_path.insert(matched.path.clone(), absolute_path);
        }

        if candidates_by_relative_path.len() < HYBRID_GRAPH_MAX_ANCHORS {
            let mut diagnostics = SearchExecutionDiagnostics::default();
            let candidate_files = searcher.candidate_files_for_repository(
                &repository_id,
                repository_root,
                &SearchTextQuery {
                    query: query_text.to_owned(),
                    path_regex: None,
                    limit: graph_candidate_limit,
                },
                &normalized_filters,
                &mut diagnostics,
            );
            for (relative_path, absolute_path) in candidate_files {
                if SymbolLanguage::from_path(&absolute_path).is_none() {
                    continue;
                }
                if !hybrid_path_has_exact_stem_match(&relative_path, &exact_terms) {
                    continue;
                }

                candidates_by_relative_path
                    .entry(relative_path)
                    .or_insert(absolute_path);
                if candidates_by_relative_path.len() >= graph_candidate_limit {
                    break;
                }
            }
        }

        if candidates_by_relative_path.is_empty() {
            continue;
        }

        let candidate_files = candidates_by_relative_path
            .into_iter()
            .collect::<Vec<(String, PathBuf)>>();
        let candidate_paths = candidate_files
            .iter()
            .map(|entry: &(String, PathBuf)| entry.1.clone())
            .collect::<Vec<PathBuf>>();
        let extracted = extract_symbols_for_paths(&candidate_paths);
        if extracted.symbols.is_empty() {
            continue;
        }

        let mut graph = SymbolGraph::default();
        register_symbol_definitions(&mut graph, &repository_id, &extracted.symbols);
        register_search_php_declaration_relations(&mut graph, &candidate_files, &extracted.symbols);
        register_search_php_target_evidence_relations(
            &mut graph,
            &candidate_files,
            &extracted.symbols,
        );
        register_search_blade_relation_evidence(&mut graph, &candidate_files, &extracted.symbols);

        let anchors = select_hybrid_graph_anchors(&extracted.symbols, &exact_terms);
        if anchors.is_empty() {
            continue;
        }

        graph_hits.extend(build_hybrid_graph_channel_hits(
            &repository_id,
            repository_root,
            &graph,
            &anchors,
            graph_candidate_limit,
        ));
    }

    Ok(graph_hits)
}

fn select_hybrid_graph_anchors(
    symbols: &[SymbolDefinition],
    exact_terms: &[String],
) -> Vec<HybridGraphAnchor> {
    let mut by_symbol = BTreeMap::<String, HybridGraphAnchor>::new();

    for symbol in symbols {
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
    anchors
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
    }
}

fn build_hybrid_graph_channel_hits(
    repository_id: &str,
    root: &Path,
    graph: &SymbolGraph,
    anchors: &[HybridGraphAnchor],
    limit: usize,
) -> Vec<HybridChannelHit> {
    let mut hits = Vec::new();

    for anchor in anchors.iter().take(HYBRID_GRAPH_MAX_ANCHORS) {
        let anchor_relative_path =
            normalize_repository_relative_path(root, Path::new(&anchor.symbol_path));
        hits.push(HybridChannelHit {
            document: HybridDocumentRef {
                repository_id: repository_id.to_owned(),
                path: anchor_relative_path.clone(),
                line: 1,
                column: 1,
            },
            raw_score: hybrid_graph_anchor_kind_score(anchor.kind),
            excerpt: anchor.symbol_name.clone(),
            provenance_id: format!(
                "graph:{}:anchor:{}:{}",
                anchor.term, anchor_relative_path, anchor.symbol_line
            ),
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
            let raw_score = hybrid_graph_anchor_kind_score(anchor.kind)
                * hybrid_graph_relation_score(adjacent.relation)
                * if direction_rank == 0 { 1.0 } else { 0.95 };
            hits.push(HybridChannelHit {
                document: HybridDocumentRef {
                    repository_id: repository_id.to_owned(),
                    path: relative_path.clone(),
                    line: 1,
                    column: 1,
                },
                raw_score,
                excerpt: adjacent.symbol.display_name.clone(),
                provenance_id: format!(
                    "graph:{}:{}:{}:{}",
                    anchor.term,
                    adjacent.relation.as_str(),
                    relative_path,
                    adjacent.symbol.line
                ),
            });
        }
    }

    hits.sort_by(|left, right| {
        right
            .raw_score
            .total_cmp(&left.raw_score)
            .then(left.document.cmp(&right.document))
            .then(left.provenance_id.cmp(&right.provenance_id))
            .then(left.excerpt.cmp(&right.excerpt))
    });
    hits.truncate(limit);
    hits
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

fn register_search_php_declaration_relations(
    graph: &mut SymbolGraph,
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
) {
    let mut symbols_by_relative_path = BTreeMap::<String, Vec<usize>>::new();
    for (index, symbol) in symbols.iter().enumerate() {
        let relative_path = candidate_files
            .iter()
            .find(|(_, absolute_path)| *absolute_path == symbol.path)
            .map(|(relative_path, _)| relative_path.clone());
        let Some(relative_path) = relative_path else {
            continue;
        };
        symbols_by_relative_path
            .entry(relative_path)
            .or_default()
            .push(index);
    }

    for (relative_path, absolute_path) in candidate_files {
        let Ok(edges) = php_declaration_relation_edges_for_file(
            relative_path,
            absolute_path,
            symbols,
            &symbols_by_relative_path,
            None,
            None,
        ) else {
            continue;
        };

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
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
) {
    let mut symbols_by_relative_path = BTreeMap::<String, Vec<usize>>::new();
    for (index, symbol) in symbols.iter().enumerate() {
        let relative_path = candidate_files
            .iter()
            .find(|(_, absolute_path)| *absolute_path == symbol.path)
            .map(|(relative_path, _)| relative_path.clone());
        let Some(relative_path) = relative_path else {
            continue;
        };
        symbols_by_relative_path
            .entry(relative_path)
            .or_default()
            .push(index);
    }

    let symbol_index_by_stable_id = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| (symbol.stable_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut canonical_symbol_name_by_stable_id = BTreeMap::new();

    for (relative_path, absolute_path) in candidate_files {
        if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Php) {
            continue;
        }
        let file_symbols = symbols_by_relative_path
            .get(relative_path)
            .into_iter()
            .flatten()
            .map(|index| symbols[*index].clone())
            .collect::<Vec<_>>();
        let Ok(source) = fs::read_to_string(absolute_path) else {
            continue;
        };
        let Ok(evidence) =
            extract_php_source_evidence_from_source(absolute_path, &source, &file_symbols)
        else {
            continue;
        };
        canonical_symbol_name_by_stable_id.extend(evidence.canonical_names_by_stable_id.clone());
    }

    let mut symbol_indices_by_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    for (stable_id, canonical_name) in &canonical_symbol_name_by_stable_id {
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

    for (relative_path, absolute_path) in candidate_files {
        if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Php) {
            continue;
        }
        let file_symbols = symbols_by_relative_path
            .get(relative_path)
            .into_iter()
            .flatten()
            .map(|index| symbols[*index].clone())
            .collect::<Vec<_>>();
        let Ok(source) = fs::read_to_string(absolute_path) else {
            continue;
        };
        let Ok(evidence) =
            extract_php_source_evidence_from_source(absolute_path, &source, &file_symbols)
        else {
            continue;
        };
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_php_target_evidence_edges(
                symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_canonical_name,
                &symbol_indices_by_lower_canonical_name,
                &evidence,
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
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
) {
    let mut symbols_by_relative_path = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_index_by_stable_id = BTreeMap::<String, usize>::new();
    let mut symbol_indices_by_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_name = BTreeMap::<String, Vec<usize>>::new();
    for (index, symbol) in symbols.iter().enumerate() {
        let relative_path = candidate_files
            .iter()
            .find(|(_, absolute_path)| *absolute_path == symbol.path)
            .map(|(relative_path, _)| relative_path.clone());
        let Some(relative_path) = relative_path else {
            continue;
        };
        symbols_by_relative_path
            .entry(relative_path)
            .or_default()
            .push(index);
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

    for (relative_path, absolute_path) in candidate_files {
        if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Blade) {
            continue;
        }
        let file_symbols = symbols_by_relative_path
            .get(relative_path)
            .into_iter()
            .flatten()
            .map(|index| symbols[*index].clone())
            .collect::<Vec<_>>();
        let Ok(source) = fs::read_to_string(absolute_path) else {
            continue;
        };
        let evidence =
            extract_blade_source_evidence_from_source(absolute_path, &source, &file_symbols);
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_blade_relation_evidence_edges(
                symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_name,
                &symbol_indices_by_lower_name,
                &evidence,
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
