use super::*;
use crate::languages::rust_implementation_candidates_from_facts;

#[derive(Debug, Clone)]
pub(in crate::mcp::server) struct StructuralFollowUpSourceFile {
    language: SymbolLanguage,
    source: String,
}

impl FriggMcpServer {
    pub(in crate::mcp::server) fn generated_follow_up_structural_for_anchor(
        root: &Path,
        repository_id: &str,
        display_path: &str,
        line: usize,
        column: usize,
        file_cache: &mut BTreeMap<PathBuf, Option<StructuralFollowUpSourceFile>>,
    ) -> Vec<GeneratedStructuralFollowUp> {
        let relative_path = PathBuf::from(display_path);
        let absolute_path = if relative_path.is_absolute() {
            relative_path
        } else {
            root.join(&relative_path)
        };

        if !file_cache.contains_key(&absolute_path) {
            let source_file =
                supported_language_for_path(&absolute_path, LanguageCapability::StructuralSearch)
                    .and_then(|language| {
                        fs::read_to_string(&absolute_path)
                            .ok()
                            .map(|source| StructuralFollowUpSourceFile { language, source })
                    });
            file_cache.insert(absolute_path.clone(), source_file);
        }

        let Some(source_file) = file_cache.get(&absolute_path).and_then(Option::as_ref) else {
            return Vec::new();
        };

        generated_follow_up_structural_at_location_in_source(
            source_file.language,
            &absolute_path,
            display_path,
            &source_file.source,
            line,
            column,
            repository_id,
        )
        .unwrap_or_default()
    }

    pub(in crate::mcp::server) fn populate_navigation_location_follow_up_structural(
        root: &Path,
        matches: &mut [NavigationLocation],
    ) {
        let mut file_cache: BTreeMap<PathBuf, Option<StructuralFollowUpSourceFile>> =
            BTreeMap::new();
        for navigation_match in matches {
            navigation_match.follow_up_structural = Self::generated_follow_up_structural_for_anchor(
                root,
                &navigation_match.repository_id,
                &navigation_match.path,
                navigation_match.line,
                navigation_match.column,
                &mut file_cache,
            );
        }
    }

    pub(in crate::mcp::server) fn populate_reference_match_follow_up_structural(
        root: &Path,
        matches: &mut [ReferenceMatch],
    ) {
        let mut file_cache: BTreeMap<PathBuf, Option<StructuralFollowUpSourceFile>> =
            BTreeMap::new();
        for reference_match in matches {
            reference_match.follow_up_structural = Self::generated_follow_up_structural_for_anchor(
                root,
                &reference_match.repository_id,
                &reference_match.path,
                reference_match.line,
                reference_match.column,
                &mut file_cache,
            );
        }
    }

    pub(in crate::mcp::server) fn populate_implementation_match_follow_up_structural(
        root: &Path,
        matches: &mut [ImplementationMatch],
    ) {
        let mut file_cache: BTreeMap<PathBuf, Option<StructuralFollowUpSourceFile>> =
            BTreeMap::new();
        for implementation_match in matches {
            implementation_match.follow_up_structural =
                Self::generated_follow_up_structural_for_anchor(
                    root,
                    &implementation_match.repository_id,
                    &implementation_match.path,
                    implementation_match.line,
                    implementation_match.column,
                    &mut file_cache,
                );
        }
    }

    pub(in crate::mcp::server) fn populate_call_hierarchy_match_follow_up_structural(
        root: &Path,
        matches: &mut [CallHierarchyMatch],
    ) {
        let mut file_cache: BTreeMap<PathBuf, Option<StructuralFollowUpSourceFile>> =
            BTreeMap::new();
        for call_match in matches {
            call_match.follow_up_structural = Self::generated_follow_up_structural_for_anchor(
                root,
                &call_match.repository_id,
                &call_match.path,
                call_match.line,
                call_match.column,
                &mut file_cache,
            );
        }
    }

    pub(in crate::mcp::server) fn metadata_note_pair(
        metadata: Value,
    ) -> (Option<crate::mcp::types::MetadataObject>, Option<String>) {
        let note =
            Some(serde_json::to_string(&metadata).expect("metadata payload should serialize"));
        let metadata = crate::mcp::types::MetadataObject::try_from(metadata)
            .expect("metadata payload should be a JSON object");
        (Some(metadata), note)
    }

    pub(in crate::mcp::server) fn metadata_with_freshness_basis(
        mut metadata: Value,
        freshness_basis: &Value,
    ) -> Value {
        metadata
            .as_object_mut()
            .expect("metadata payload should be an object")
            .insert("freshness_basis".to_owned(), freshness_basis.clone());
        metadata
    }

    #[allow(clippy::type_complexity)]
    pub(in crate::mcp::server) fn precise_call_site_fields(
        root: &Path,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
    ) -> (
        Option<String>,
        Option<usize>,
        Option<usize>,
        Option<usize>,
        Option<usize>,
    ) {
        (
            Some(Self::canonicalize_navigation_path(root, &occurrence.path)),
            Some(occurrence.range.start_line),
            Some(occurrence.range.start_column),
            Some(occurrence.range.end_line),
            Some(occurrence.range.end_column),
        )
    }

    pub(in crate::mcp::server) fn is_heuristic_callable_kind(kind: &str) -> bool {
        matches!(
            kind.trim().to_ascii_lowercase().as_str(),
            "function" | "method"
        )
    }

    pub(in crate::mcp::server) fn navigation_target_selection_note(
        symbol_query: &str,
        target: &SymbolCandidate,
        candidate_count: usize,
        selected_rank_candidate_count: usize,
    ) -> serde_json::Value {
        json!({
            "query": symbol_query,
            "selected_symbol_id": target.symbol.stable_id,
            "selected_symbol": target.symbol.name,
            "selected_kind": target.symbol.kind,
            "selected_repository_id": target.repository_id,
            "selected_path": Self::relative_display_path(&target.root, &target.symbol.path),
            "selected_path_class": target.path_class,
            "selected_line": target.symbol.line,
            "selected_rank": target.rank,
            "candidate_count": candidate_count,
            "same_rank_candidate_count": selected_rank_candidate_count,
            "ambiguous_query": selected_rank_candidate_count > 1,
        })
    }

    fn symbol_match_from_symbol_candidate(
        corpora: &[Arc<RepositorySymbolCorpus>],
        candidate: &SymbolCandidate,
    ) -> SymbolMatch {
        let (container, signature) = corpora
            .iter()
            .find(|corpus| corpus.repository_id == candidate.repository_id)
            .map(|corpus| Self::symbol_context_for_stable_id(corpus, &candidate.symbol.stable_id))
            .unwrap_or((None, None));
        SymbolMatch {
            match_id: None,
            stable_symbol_id: Some(candidate.symbol.stable_id.clone()),
            repository_id: candidate.repository_id.clone(),
            symbol: candidate.symbol.name.clone(),
            kind: candidate.symbol.kind.as_str().to_owned(),
            path: Self::relative_display_path(&candidate.root, &candidate.symbol.path),
            line: candidate.symbol.line,
            container,
            signature,
        }
    }

    pub(in crate::mcp::server) fn navigation_target_selection_summary_for_resolved(
        symbol_query: &str,
        target: &ResolvedSymbolTarget,
    ) -> NavigationTargetSelectionSummary {
        NavigationTargetSelectionSummary {
            status: NavigationTargetSelectionStatus::Resolved,
            symbol_query: symbol_query.to_owned(),
            selected_stable_symbol_id: Some(target.candidate.symbol.stable_id.clone()),
            candidate_count: target.candidate_count,
            same_rank_candidate_count: target.selected_rank_candidate_count,
            ambiguous_query: target.selected_rank_candidate_count > 1,
            candidates: Vec::new(),
        }
    }

    pub(in crate::mcp::server) fn navigation_target_selection_summary_for_disambiguation(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol_query: &str,
        target: &DisambiguationRequiredSymbolTarget,
    ) -> NavigationTargetSelectionSummary {
        NavigationTargetSelectionSummary {
            status: NavigationTargetSelectionStatus::DisambiguationRequired,
            symbol_query: symbol_query.to_owned(),
            selected_stable_symbol_id: None,
            candidate_count: target.candidate_count,
            same_rank_candidate_count: target.selected_rank_candidate_count,
            ambiguous_query: true,
            candidates: target
                .candidates
                .iter()
                .map(|candidate| Self::symbol_match_from_symbol_candidate(corpora, candidate))
                .collect(),
        }
    }

    pub(in crate::mcp::server) fn navigation_target_selection_summary_for_selection(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol_query: &str,
        selection: &NavigationTargetSelection,
    ) -> NavigationTargetSelectionSummary {
        match selection {
            NavigationTargetSelection::Resolved(target) => {
                Self::navigation_target_selection_summary_for_resolved(symbol_query, target)
            }
            NavigationTargetSelection::DisambiguationRequired(target) => {
                Self::navigation_target_selection_summary_for_disambiguation(
                    corpora,
                    symbol_query,
                    target,
                )
            }
        }
    }

    pub(in crate::mcp::server) fn navigation_target_selection_summary_value(
        summary: &NavigationTargetSelectionSummary,
    ) -> Value {
        serde_json::to_value(summary).expect("target selection summary should serialize")
    }

    pub(in crate::mcp::server) fn precise_absence_reason(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
        precise_match_count: usize,
    ) -> &'static str {
        if stats.artifacts_discovered == 0 {
            return "no_scip_artifacts_discovered";
        }

        match coverage_mode {
            PreciseCoverageMode::Partial if precise_match_count == 0 => {
                return "precise_partial_non_authoritative_absence";
            }
            PreciseCoverageMode::None if stats.artifacts_failed > 0 => {
                return "scip_artifact_ingest_failed";
            }
            PreciseCoverageMode::Full | PreciseCoverageMode::Partial
                if stats.artifacts_ingested > 0 && precise_match_count == 0 =>
            {
                return "required_precise_matches_not_present_in_precise_graph";
            }
            PreciseCoverageMode::None => {
                return "no_usable_precise_data";
            }
            _ => {}
        }

        "precise_unavailable"
    }

    pub(in crate::mcp::server) fn call_hierarchy_availability(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
        precise_match_count: usize,
        heuristic_match_count: usize,
    ) -> NavigationAvailability {
        if precise_match_count > 0 {
            return NavigationAvailability {
                status: "available".to_owned(),
                reason: None,
                precise_required_for_complete_results: false,
            };
        }
        if heuristic_match_count > 0 {
            return NavigationAvailability {
                status: "heuristic".to_owned(),
                reason: Some(
                    Self::precise_absence_reason(coverage_mode, stats, precise_match_count)
                        .to_owned(),
                ),
                precise_required_for_complete_results: true,
            };
        }
        if coverage_mode == PreciseCoverageMode::Full {
            return NavigationAvailability {
                status: "available".to_owned(),
                reason: None,
                precise_required_for_complete_results: false,
            };
        }

        NavigationAvailability {
            status: "unavailable".to_owned(),
            reason: Some(
                Self::precise_absence_reason(coverage_mode, stats, precise_match_count).to_owned(),
            ),
            precise_required_for_complete_results: true,
        }
    }

    pub(in crate::mcp::server) fn navigation_mode_from_precision_label(
        label: Option<&str>,
    ) -> NavigationMode {
        match label {
            Some("precise") => NavigationMode::Precise,
            Some("precise_partial") => NavigationMode::PrecisePartial,
            Some("heuristic") => NavigationMode::HeuristicNoPrecise,
            _ => NavigationMode::UnavailableNoPrecise,
        }
    }

    pub(in crate::mcp::server) fn precise_coverage_mode(
        stats: &PreciseIngestStats,
    ) -> PreciseCoverageMode {
        if stats.artifacts_ingested == 0 {
            return PreciseCoverageMode::None;
        }
        if stats.artifacts_failed > 0 {
            return PreciseCoverageMode::Partial;
        }
        PreciseCoverageMode::Full
    }

    pub(in crate::mcp::server) fn precise_resolution_precision(
        coverage_mode: PreciseCoverageMode,
    ) -> &'static str {
        match coverage_mode {
            PreciseCoverageMode::Full => "precise",
            PreciseCoverageMode::Partial => "precise_partial",
            PreciseCoverageMode::None => "heuristic",
        }
    }

    pub(in crate::mcp::server) fn precise_match_precision(
        coverage_mode: PreciseCoverageMode,
    ) -> &'static str {
        match coverage_mode {
            PreciseCoverageMode::Full => "precise",
            PreciseCoverageMode::Partial => "precise_partial",
            PreciseCoverageMode::None => "heuristic",
        }
    }

    fn precise_note_metadata(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
    ) -> serde_json::Value {
        json!({
            "coverage": coverage_mode.as_str(),
            "candidate_directories": Self::bounded_text_values(&stats.candidate_directories),
            "discovered_artifacts": Self::bounded_text_values(&stats.discovered_artifacts),
            "artifacts_discovered": stats.artifacts_discovered,
            "artifacts_discovered_bytes": stats.artifacts_discovered_bytes,
            "artifacts_ingested": stats.artifacts_ingested,
            "artifacts_ingested_bytes": stats.artifacts_ingested_bytes,
            "artifacts_failed": stats.artifacts_failed,
            "artifacts_failed_bytes": stats.artifacts_failed_bytes,
            "failed_artifacts": Self::precise_failure_note_entries(stats),
        })
    }

    pub(in crate::mcp::server) fn precise_note_with_count(
        coverage_mode: PreciseCoverageMode,
        stats: &PreciseIngestStats,
        count_key: &str,
        count: usize,
    ) -> serde_json::Value {
        let mut precise = Self::precise_note_metadata(coverage_mode, stats);
        precise[count_key] = json!(count);
        precise
    }

    pub(in crate::mcp::server) fn push_precise_failure_sample(
        stats: &mut PreciseIngestStats,
        artifact_label: impl Into<String>,
        stage: &str,
        detail: impl AsRef<str>,
    ) {
        if stats.failed_artifacts.len() >= Self::PRECISE_FAILURE_SAMPLE_LIMIT {
            return;
        }

        let artifact_label = artifact_label.into();
        stats.failed_artifacts.push(PreciseArtifactFailureSample {
            artifact_label: Self::bounded_text(&artifact_label),
            stage: stage.to_owned(),
            detail: Self::bounded_text(detail.as_ref()),
        });
    }

    fn precise_failure_note_entries(stats: &PreciseIngestStats) -> Vec<Value> {
        stats
            .failed_artifacts
            .iter()
            .map(|sample| {
                json!({
                    "artifact_label": sample.artifact_label,
                    "stage": sample.stage,
                    "detail": sample.detail,
                })
            })
            .collect()
    }

    fn bounded_text_values(values: &[String]) -> Vec<String> {
        values
            .iter()
            .map(|value| Self::bounded_text(value))
            .collect::<Vec<_>>()
    }

    pub(in crate::mcp::server) fn heuristic_implementation_matches_from_symbols(
        target_symbol: &SymbolDefinition,
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
    ) -> Vec<ImplementationMatch> {
        match heuristic_implementation_strategy(target_symbol.language) {
            Some(HeuristicImplementationStrategy::RustImplBlocks) => {
                Self::heuristic_rust_implementation_matches_from_symbols(
                    target_symbol,
                    target_corpus,
                    target_root,
                )
            }
            Some(HeuristicImplementationStrategy::PhpDeclarationRelations) => {
                Self::heuristic_php_implementation_matches_from_symbols(
                    target_symbol,
                    target_corpus,
                    target_root,
                )
            }
            None => Vec::new(),
        }
    }

    fn heuristic_rust_implementation_matches_from_symbols(
        target_symbol: &SymbolDefinition,
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
    ) -> Vec<ImplementationMatch> {
        let candidates = if target_corpus.rust_implementation_facts.is_empty() {
            heuristic_rust_implementation_candidates(target_symbol, &target_corpus.symbols)
        } else {
            let indexed = rust_implementation_candidates_from_facts(
                target_symbol,
                &target_corpus.symbols,
                &target_corpus.rust_implementation_facts,
            );
            if indexed.is_empty() {
                heuristic_rust_implementation_candidates(target_symbol, &target_corpus.symbols)
            } else {
                indexed
            }
        };
        let matches = candidates
            .into_iter()
            .map(|candidate| {
                let (container, signature) = Self::symbol_context_for_stable_id(
                    target_corpus,
                    &candidate.source_symbol.stable_id,
                );
                ImplementationMatch {
                    match_id: None,
                    stable_symbol_id: Some(candidate.source_symbol.stable_id.clone()),
                    symbol: candidate.symbol,
                    kind: Self::display_symbol_kind(candidate.source_symbol.kind.as_str()),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(target_root, &candidate.source_symbol.path),
                    line: candidate.source_symbol.line,
                    column: 1,
                    relation: Some(candidate.relation.to_owned()),
                    container,
                    signature,
                    precision: Some("heuristic".to_owned()),
                    fallback_reason: Some(candidate.fallback_reason.to_owned()),
                    follow_up_structural: Vec::new(),
                }
            })
            .collect::<Vec<_>>();

        Self::dedup_sorted_implementation_matches(matches)
    }

    fn heuristic_php_implementation_matches_from_symbols(
        target_symbol: &SymbolDefinition,
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
    ) -> Vec<ImplementationMatch> {
        let candidate_files = target_corpus
            .source_paths
            .iter()
            .map(|path| (Self::relative_display_path(target_root, path), path.clone()))
            .collect::<Vec<_>>();
        let mut matches = Vec::new();
        for (source_symbol_index, relation) in php_heuristic_implementation_candidates_for_target(
            target_symbol,
            &candidate_files,
            &target_corpus.symbols,
            &target_corpus.symbols_by_relative_path,
            Some(&target_corpus.symbol_indices_by_name),
            Some(&target_corpus.symbol_indices_by_lower_name),
        ) {
            let source_symbol = &target_corpus.symbols[source_symbol_index];
            if source_symbol.stable_id == target_symbol.stable_id {
                continue;
            }

            let (container, signature) =
                Self::symbol_context_for_stable_id(target_corpus, &source_symbol.stable_id);
            matches.push(ImplementationMatch {
                match_id: None,
                stable_symbol_id: Some(source_symbol.stable_id.clone()),
                symbol: source_symbol.name.clone(),
                kind: Self::display_symbol_kind(source_symbol.kind.as_str()),
                repository_id: target_corpus.repository_id.clone(),
                path: Self::relative_display_path(target_root, &source_symbol.path),
                line: source_symbol.line,
                column: 1,
                relation: Some(RelationKind::as_str(relation).to_owned()),
                container,
                signature,
                precision: Some("heuristic".to_owned()),
                fallback_reason: Some("precise_absent".to_owned()),
                follow_up_structural: Vec::new(),
            });
        }

        Self::dedup_sorted_implementation_matches(matches)
    }

    fn dedup_sorted_implementation_matches(
        mut matches: Vec<ImplementationMatch>,
    ) -> Vec<ImplementationMatch> {
        Self::sort_implementation_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.symbol == right.symbol
                && left.kind == right.kind
                && left.relation == right.relation
                && left.precision == right.precision
                && left.fallback_reason == right.fallback_reason
        });
        matches
    }

    pub(in crate::mcp::server) fn sort_call_hierarchy_matches(matches: &mut [CallHierarchyMatch]) {
        matches.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.source_symbol.cmp(&right.source_symbol))
                .then(left.target_symbol.cmp(&right.target_symbol))
                .then(left.relation.cmp(&right.relation))
                .then(left.precision.cmp(&right.precision))
                .then(left.call_path.cmp(&right.call_path))
                .then(left.call_line.cmp(&right.call_line))
                .then(left.call_column.cmp(&right.call_column))
                .then(left.call_end_line.cmp(&right.call_end_line))
                .then(left.call_end_column.cmp(&right.call_end_column))
        });
    }

    pub(in crate::mcp::server) fn is_heuristic_call_relation(relation: RelationKind) -> bool {
        matches!(relation, RelationKind::Calls)
    }
}
