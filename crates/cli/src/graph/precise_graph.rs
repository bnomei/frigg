use super::*;

impl SymbolGraph {
    pub fn precise_counts(&self) -> PreciseGraphCounts {
        PreciseGraphCounts {
            symbols: self.precise_symbols.len(),
            occurrences: self.precise_occurrences.len(),
            relationships: self.precise_relationships.len(),
        }
    }

    pub fn clear_precise_data(&mut self) {
        self.precise_symbols.clear();
        self.precise_symbol_keys_by_repository.clear();
        self.precise_symbols_by_file.clear();
        self.precise_symbol_ref_counts.clear();
        self.precise_occurrences.clear();
        self.precise_occurrence_keys_by_file.clear();
        self.precise_occurrence_keys_by_symbol.clear();
        self.precise_relationships.clear();
        self.precise_relationship_keys_by_from_symbol.clear();
        self.precise_relationship_keys_by_to_symbol.clear();
        self.precise_relationships_by_file.clear();
        self.precise_relationship_ref_counts.clear();
    }

    pub fn precise_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Option<&PreciseSymbolRecord> {
        self.precise_symbols
            .get(&(repository_id.to_owned(), symbol.to_owned()))
    }

    pub fn precise_symbols_for_repository(&self, repository_id: &str) -> Vec<PreciseSymbolRecord> {
        let mut symbols = self
            .precise_symbol_keys_by_repository
            .get(repository_id)
            .into_iter()
            .flat_map(|symbol_ids| symbol_ids.iter())
            .filter_map(|symbol_id| {
                self.precise_symbols
                    .get(&(repository_id.to_owned(), symbol_id.clone()))
                    .cloned()
            })
            .collect::<Vec<_>>();
        symbols.sort_by(precise_symbol_order);
        symbols
    }

    pub fn precise_occurrences_for_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Vec<PreciseOccurrenceRecord> {
        let mut occurrences = self
            .precise_occurrence_keys_by_symbol
            .get(&precise_symbol_key(repository_id, symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_occurrences.get(key).cloned())
            .collect::<Vec<_>>();
        occurrences.sort_by(precise_occurrence_order);
        occurrences
    }

    pub fn precise_definition_occurrence_for_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Option<PreciseOccurrenceRecord> {
        self.precise_occurrence_keys_by_symbol
            .get(&precise_symbol_key(repository_id, symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_occurrences.get(key))
            .find(|occurrence| occurrence.is_definition())
            .cloned()
    }

    pub fn precise_references_for_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Vec<PreciseOccurrenceRecord> {
        self.precise_occurrences_for_symbol(repository_id, symbol)
            .into_iter()
            .filter(|occurrence| !occurrence.is_definition())
            .collect()
    }

    pub fn precise_occurrences_for_file(
        &self,
        repository_id: &str,
        path: &str,
    ) -> Vec<PreciseOccurrenceRecord> {
        let mut occurrences = self
            .precise_occurrence_keys_by_file
            .get(&precise_file_key(repository_id, path))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_occurrences.get(key).cloned())
            .collect::<Vec<_>>();
        occurrences.sort_by(precise_occurrence_order);
        occurrences
    }

    pub fn select_precise_symbol_for_location(
        &self,
        repository_id: &str,
        path: &str,
        line: usize,
        column: Option<usize>,
    ) -> Option<PreciseSymbolRecord> {
        let mut ranked = self
            .precise_occurrences_for_file(repository_id, path)
            .into_iter()
            .filter(|occurrence| occurrence.range.start_line <= line)
            .filter(|occurrence| {
                column.is_none_or(|value| {
                    occurrence.range.start_line < line || occurrence.range.start_column <= value
                })
            })
            .filter_map(|occurrence| {
                let symbol = self
                    .precise_symbol(repository_id, &occurrence.symbol)?
                    .clone();
                let line_distance = line.saturating_sub(occurrence.range.start_line);
                let column_distance = if line_distance == 0 {
                    column
                        .map(|value| value.saturating_sub(occurrence.range.start_column))
                        .unwrap_or(0)
                } else {
                    0
                };
                let containment_rank = if occurrence.contains_location(line, column) {
                    0u8
                } else {
                    1u8
                };
                Some((
                    containment_rank,
                    line_distance,
                    column_distance,
                    occurrence.range.start_line,
                    occurrence.range.start_column,
                    occurrence.symbol.clone(),
                    symbol,
                ))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(right.3.cmp(&left.3))
                .then(right.4.cmp(&left.4))
                .then(left.5.cmp(&right.5))
        });
        ranked
            .into_iter()
            .next()
            .map(|(_, _, _, _, _, _, symbol)| symbol)
    }

    pub fn precise_relationships_from_symbol(
        &self,
        repository_id: &str,
        from_symbol: &str,
    ) -> Vec<PreciseRelationshipRecord> {
        let mut relationships = self
            .precise_relationship_keys_by_from_symbol
            .get(&precise_symbol_key(repository_id, from_symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_relationships.get(key).cloned())
            .collect::<Vec<_>>();
        relationships.sort_by(precise_relationship_order);
        relationships
    }

    pub fn precise_relationships_to_symbol_by_kinds(
        &self,
        repository_id: &str,
        to_symbol: &str,
        kinds: &[PreciseRelationshipKind],
    ) -> Vec<PreciseRelationshipRecord> {
        let mut relationships = self
            .precise_relationship_keys_by_to_symbol
            .get(&precise_symbol_key(repository_id, to_symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_relationships.get(key))
            .filter(|relationship| kinds.contains(&relationship.kind))
            .cloned()
            .collect::<Vec<_>>();
        relationships.sort_by(precise_relationship_order);
        relationships
    }

    pub fn select_precise_symbol_for_navigation(
        &self,
        repository_id: &str,
        symbol_query: &str,
        fallback_symbol_name: &str,
    ) -> Option<PreciseSymbolRecord> {
        self.matching_precise_symbols_for_navigation(
            repository_id,
            symbol_query,
            fallback_symbol_name,
        )
        .into_iter()
        .next()
    }

    pub fn matching_precise_symbols_for_navigation(
        &self,
        repository_id: &str,
        symbol_query: &str,
        fallback_symbol_name: &str,
    ) -> Vec<PreciseSymbolRecord> {
        let mut ranked = self
            .precise_symbols_for_repository(repository_id)
            .into_iter()
            .filter_map(|precise_symbol| {
                precise_navigation_symbol_rank(&precise_symbol, symbol_query, fallback_symbol_name)
                    .map(|rank| (rank, precise_symbol))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.symbol.cmp(&right.1.symbol))
                .then(left.1.display_name.cmp(&right.1.display_name))
                .then(left.1.kind.cmp(&right.1.kind))
        });
        ranked
            .into_iter()
            .map(|(_, precise_symbol)| precise_symbol)
            .collect()
    }
}
