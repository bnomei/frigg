use std::collections::BTreeSet;
use std::time::Instant;

use super::*;

pub(super) fn replace_precise_occurrences_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    occurrences: &[PreciseOccurrenceRecord],
) {
    let keys = graph
        .precise_occurrence_keys_by_file
        .remove(&precise_file_key(repository_id, path))
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    for key in keys {
        remove_precise_occurrence(graph, &key);
    }
    for occurrence in occurrences {
        upsert_precise_occurrence(graph, occurrence);
    }
}

pub(super) fn overlay_precise_occurrences_for_file(
    graph: &mut SymbolGraph,
    _repository_id: &str,
    _path: &str,
    occurrences: &[PreciseOccurrenceRecord],
) {
    for occurrence in occurrences {
        upsert_precise_occurrence(graph, occurrence);
    }
}

pub(super) fn replace_precise_symbols_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    symbols: &[PreciseSymbolRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let previous_symbols = graph
        .precise_symbols_by_file
        .remove(&file_key)
        .unwrap_or_default();

    for previous_symbol in previous_symbols {
        decrement_precise_symbol_ref_count(graph, repository_id, &previous_symbol);
    }

    let mut next_symbols = BTreeSet::new();
    for symbol in symbols {
        let symbol_key = (symbol.repository_id.clone(), symbol.symbol.clone());
        upsert_precise_symbol_record(graph, &symbol_key, symbol);
        increment_precise_symbol_ref_count(graph, &symbol_key);
        next_symbols.insert(symbol.symbol.clone());
    }

    graph.precise_symbols_by_file.insert(file_key, next_symbols);
}

pub(super) fn overlay_precise_symbols_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    symbols: &[PreciseSymbolRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let mut newly_referenced_symbols = Vec::new();
    for symbol in symbols {
        let symbol_key = (symbol.repository_id.clone(), symbol.symbol.clone());
        let is_new_for_file = {
            let file_symbols = graph
                .precise_symbols_by_file
                .entry(file_key.clone())
                .or_default();
            file_symbols.insert(symbol.symbol.clone())
        };
        upsert_precise_symbol_record(graph, &symbol_key, symbol);
        if is_new_for_file {
            newly_referenced_symbols.push(symbol_key);
        }
    }

    for symbol_key in newly_referenced_symbols {
        increment_precise_symbol_ref_count(graph, &symbol_key);
    }
}

pub(super) fn replace_precise_relationships_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    relationships: &[PreciseRelationshipRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let previous_relationship_keys = graph
        .precise_relationships_by_file
        .remove(&file_key)
        .unwrap_or_default();

    for relationship_key in previous_relationship_keys {
        decrement_precise_relationship_ref_count(graph, &relationship_key);
    }

    let mut next_relationship_keys = BTreeSet::new();
    for relationship in relationships {
        let relationship_key = PreciseRelationshipKey::from(relationship);
        upsert_precise_relationship(graph, relationship);
        increment_precise_relationship_ref_count(graph, &relationship_key);
        next_relationship_keys.insert(relationship_key);
    }

    graph
        .precise_relationships_by_file
        .insert(file_key, next_relationship_keys);
}

pub(super) fn overlay_precise_relationships_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    relationships: &[PreciseRelationshipRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let mut newly_referenced_relationships = Vec::new();
    for relationship in relationships {
        let relationship_key = PreciseRelationshipKey::from(relationship);
        let is_new_for_file = {
            let file_relationships = graph
                .precise_relationships_by_file
                .entry(file_key.clone())
                .or_default();
            file_relationships.insert(relationship_key.clone())
        };
        upsert_precise_relationship(graph, relationship);
        if is_new_for_file {
            newly_referenced_relationships.push(relationship_key);
        }
    }

    for relationship_key in newly_referenced_relationships {
        increment_precise_relationship_ref_count(graph, &relationship_key);
    }
}

pub(super) fn upsert_precise_symbol_record(
    graph: &mut SymbolGraph,
    symbol_key: &(String, String),
    symbol: &PreciseSymbolRecord,
) {
    graph
        .precise_symbols
        .insert(symbol_key.clone(), symbol.clone());
    graph
        .precise_symbol_keys_by_repository
        .entry(symbol_key.0.clone())
        .or_default()
        .insert(symbol_key.1.clone());
}

pub(super) fn upsert_precise_occurrence(
    graph: &mut SymbolGraph,
    occurrence: &PreciseOccurrenceRecord,
) {
    let key = PreciseOccurrenceKey::from(occurrence);
    if let Some(previous) = graph.precise_occurrences.get(&key).cloned() {
        remove_precise_occurrence_indexes(graph, &key, &previous);
    }
    graph
        .precise_occurrences
        .insert(key.clone(), occurrence.clone());
    insert_precise_occurrence_indexes(graph, &key, occurrence);
}

pub(super) fn remove_precise_occurrence(graph: &mut SymbolGraph, key: &PreciseOccurrenceKey) {
    if let Some(previous) = graph.precise_occurrences.remove(key) {
        remove_precise_occurrence_indexes(graph, key, &previous);
    }
}

pub(super) fn insert_precise_occurrence_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseOccurrenceKey,
    occurrence: &PreciseOccurrenceRecord,
) {
    graph
        .precise_occurrence_keys_by_file
        .entry(precise_file_key(
            &occurrence.repository_id,
            &occurrence.path,
        ))
        .or_default()
        .insert(key.clone());
    graph
        .precise_occurrence_keys_by_symbol
        .entry(precise_symbol_key(
            &occurrence.repository_id,
            &occurrence.symbol,
        ))
        .or_default()
        .insert(key.clone());
}

pub(super) fn remove_precise_occurrence_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseOccurrenceKey,
    occurrence: &PreciseOccurrenceRecord,
) {
    let file_key = precise_file_key(&occurrence.repository_id, &occurrence.path);
    let remove_file_entry =
        if let Some(keys) = graph.precise_occurrence_keys_by_file.get_mut(&file_key) {
            keys.remove(key);
            keys.is_empty()
        } else {
            false
        };
    if remove_file_entry {
        graph.precise_occurrence_keys_by_file.remove(&file_key);
    }

    let symbol_key = precise_symbol_key(&occurrence.repository_id, &occurrence.symbol);
    let remove_symbol_entry =
        if let Some(keys) = graph.precise_occurrence_keys_by_symbol.get_mut(&symbol_key) {
            keys.remove(key);
            keys.is_empty()
        } else {
            false
        };
    if remove_symbol_entry {
        graph.precise_occurrence_keys_by_symbol.remove(&symbol_key);
    }
}

pub(super) fn upsert_precise_relationship(
    graph: &mut SymbolGraph,
    relationship: &PreciseRelationshipRecord,
) {
    let key = PreciseRelationshipKey::from(relationship);
    if let Some(previous) = graph.precise_relationships.get(&key).cloned() {
        remove_precise_relationship_indexes(graph, &key, &previous);
    }
    graph
        .precise_relationships
        .insert(key.clone(), relationship.clone());
    insert_precise_relationship_indexes(graph, &key, relationship);
}

pub(super) fn insert_precise_relationship_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseRelationshipKey,
    relationship: &PreciseRelationshipRecord,
) {
    graph
        .precise_relationship_keys_by_from_symbol
        .entry(precise_symbol_key(
            &relationship.repository_id,
            &relationship.from_symbol,
        ))
        .or_default()
        .insert(key.clone());
    graph
        .precise_relationship_keys_by_to_symbol
        .entry(precise_symbol_key(
            &relationship.repository_id,
            &relationship.to_symbol,
        ))
        .or_default()
        .insert(key.clone());
}

pub(super) fn remove_precise_relationship_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseRelationshipKey,
    relationship: &PreciseRelationshipRecord,
) {
    let from_symbol_key =
        precise_symbol_key(&relationship.repository_id, &relationship.from_symbol);
    let remove_from_entry = if let Some(keys) = graph
        .precise_relationship_keys_by_from_symbol
        .get_mut(&from_symbol_key)
    {
        keys.remove(key);
        keys.is_empty()
    } else {
        false
    };
    if remove_from_entry {
        graph
            .precise_relationship_keys_by_from_symbol
            .remove(&from_symbol_key);
    }

    let to_symbol_key = precise_symbol_key(&relationship.repository_id, &relationship.to_symbol);
    let remove_to_entry = if let Some(keys) = graph
        .precise_relationship_keys_by_to_symbol
        .get_mut(&to_symbol_key)
    {
        keys.remove(key);
        keys.is_empty()
    } else {
        false
    };
    if remove_to_entry {
        graph
            .precise_relationship_keys_by_to_symbol
            .remove(&to_symbol_key);
    }
}

pub(super) fn increment_precise_symbol_ref_count(
    graph: &mut SymbolGraph,
    symbol_key: &(String, String),
) {
    let next = graph
        .precise_symbol_ref_counts
        .get(symbol_key)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    graph
        .precise_symbol_ref_counts
        .insert(symbol_key.clone(), next);
}

pub(super) fn decrement_precise_symbol_ref_count(
    graph: &mut SymbolGraph,
    repository_id: &str,
    symbol: &str,
) {
    let symbol_key = precise_symbol_key(repository_id, symbol);
    let current = graph
        .precise_symbol_ref_counts
        .get(&symbol_key)
        .copied()
        .unwrap_or(0);
    match current {
        0 | 1 => {
            graph.precise_symbol_ref_counts.remove(&symbol_key);
            graph.precise_symbols.remove(&symbol_key);
            let remove_repository_entry = if let Some(symbols) = graph
                .precise_symbol_keys_by_repository
                .get_mut(repository_id)
            {
                symbols.remove(symbol);
                symbols.is_empty()
            } else {
                false
            };
            if remove_repository_entry {
                graph
                    .precise_symbol_keys_by_repository
                    .remove(repository_id);
            }
        }
        count => {
            graph
                .precise_symbol_ref_counts
                .insert(symbol_key, count - 1);
        }
    }
}

pub(super) fn increment_precise_relationship_ref_count(
    graph: &mut SymbolGraph,
    relationship_key: &PreciseRelationshipKey,
) {
    let next = graph
        .precise_relationship_ref_counts
        .get(relationship_key)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    graph
        .precise_relationship_ref_counts
        .insert(relationship_key.clone(), next);
}

pub(super) fn decrement_precise_relationship_ref_count(
    graph: &mut SymbolGraph,
    relationship_key: &PreciseRelationshipKey,
) {
    let current = graph
        .precise_relationship_ref_counts
        .get(relationship_key)
        .copied()
        .unwrap_or(0);
    match current {
        0 | 1 => {
            graph
                .precise_relationship_ref_counts
                .remove(relationship_key);
            if let Some(relationship) = graph.precise_relationships.remove(relationship_key) {
                remove_precise_relationship_indexes(graph, relationship_key, &relationship);
            }
        }
        count => {
            graph
                .precise_relationship_ref_counts
                .insert(relationship_key.clone(), count - 1);
        }
    }
}

pub(super) fn precise_symbol_key(repository_id: &str, symbol: &str) -> (String, String) {
    (repository_id.to_owned(), symbol.to_owned())
}

pub(super) fn precise_file_key(repository_id: &str, path: &str) -> (String, String) {
    (repository_id.to_owned(), path.to_owned())
}

pub(super) fn precise_symbol_order(
    left: &PreciseSymbolRecord,
    right: &PreciseSymbolRecord,
) -> std::cmp::Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.symbol.cmp(&right.symbol))
        .then(left.display_name.cmp(&right.display_name))
        .then(left.kind.cmp(&right.kind))
}

pub(super) fn precise_occurrence_order(
    left: &PreciseOccurrenceRecord,
    right: &PreciseOccurrenceRecord,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.range.start_line.cmp(&right.range.start_line))
        .then(left.range.start_column.cmp(&right.range.start_column))
        .then(left.range.end_line.cmp(&right.range.end_line))
        .then(left.range.end_column.cmp(&right.range.end_column))
        .then(left.symbol.cmp(&right.symbol))
        .then(left.symbol_roles.cmp(&right.symbol_roles))
}

pub(super) fn precise_relationship_order(
    left: &PreciseRelationshipRecord,
    right: &PreciseRelationshipRecord,
) -> std::cmp::Ordering {
    left.from_symbol
        .cmp(&right.from_symbol)
        .then(left.to_symbol.cmp(&right.to_symbol))
        .then(left.kind.cmp(&right.kind))
}

pub(super) fn precise_navigation_symbol_rank(
    precise_symbol: &PreciseSymbolRecord,
    symbol_query: &str,
    fallback_symbol_name: &str,
) -> Option<u8> {
    if precise_symbol.symbol == symbol_query {
        return Some(0);
    }
    if precise_symbol.display_name == symbol_query {
        return Some(1);
    }
    if precise_symbol
        .display_name
        .eq_ignore_ascii_case(symbol_query)
    {
        return Some(2);
    }
    if precise_symbol.display_name == fallback_symbol_name {
        return Some(3);
    }
    if precise_symbol
        .display_name
        .eq_ignore_ascii_case(fallback_symbol_name)
    {
        return Some(4);
    }

    None
}

pub(super) fn invalid_input(
    artifact_label: &str,
    code: ScipInvalidInputCode,
    message: impl Into<String>,
) -> ScipIngestError {
    ScipIngestError::InvalidInput {
        diagnostic: ScipInvalidInputDiagnostic {
            artifact_label: artifact_label.to_owned(),
            code,
            message: message.into(),
            line: None,
            column: None,
        },
    }
}

pub(super) fn resource_budget_exceeded(
    artifact_label: &str,
    code: ScipResourceBudgetCode,
    message: impl Into<String>,
    limit: u64,
    actual: u64,
) -> ScipIngestError {
    ScipIngestError::ResourceBudgetExceeded {
        diagnostic: ScipResourceBudgetDiagnostic {
            artifact_label: artifact_label.to_owned(),
            code,
            message: message.into(),
            limit,
            actual,
        },
    }
}

pub(super) fn enforce_elapsed_budget(
    artifact_label: &str,
    started_at: Instant,
    budgets: ScipResourceBudgets,
    phase: &str,
) -> ScipIngestResult<()> {
    if budgets.max_elapsed_ms == u64::MAX {
        return Ok(());
    }

    let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    if elapsed_ms > budgets.max_elapsed_ms {
        return Err(resource_budget_exceeded(
            artifact_label,
            ScipResourceBudgetCode::ElapsedMs,
            format!("scip ingest elapsed time exceeded while {phase}"),
            budgets.max_elapsed_ms,
            elapsed_ms,
        ));
    }

    Ok(())
}

pub(super) fn symbol_relation_order(
    left: &SymbolRelation,
    right: &SymbolRelation,
) -> std::cmp::Ordering {
    left.from_symbol
        .cmp(&right.from_symbol)
        .then(left.to_symbol.cmp(&right.to_symbol))
        .then(left.relation.cmp(&right.relation))
}

pub(super) fn adjacent_symbol_order(
    left: &AdjacentSymbol,
    right: &AdjacentSymbol,
) -> std::cmp::Ordering {
    left.relation
        .cmp(&right.relation)
        .then(left.symbol.symbol_id.cmp(&right.symbol.symbol_id))
        .then(left.symbol.path.cmp(&right.symbol.path))
        .then(left.symbol.line.cmp(&right.symbol.line))
}

pub(super) fn heuristic_relation_hint_order(
    left: &HeuristicRelationHint,
    right: &HeuristicRelationHint,
) -> std::cmp::Ordering {
    right
        .confidence
        .rank()
        .cmp(&left.confidence.rank())
        .then(left.source_symbol.path.cmp(&right.source_symbol.path))
        .then(left.source_symbol.line.cmp(&right.source_symbol.line))
        .then(
            left.source_symbol
                .symbol_id
                .cmp(&right.source_symbol.symbol_id),
        )
        .then(
            left.target_symbol
                .symbol_id
                .cmp(&right.target_symbol.symbol_id),
        )
        .then(left.relation.cmp(&right.relation))
}
