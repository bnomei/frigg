use super::*;

impl FriggMcpServer {
    pub(in crate::mcp::server) fn try_precise_definition_fast_path(
        &self,
        repository_id_hint: Option<&str>,
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        include_follow_up_structural: bool,
        limit: usize,
    ) -> Result<Option<(Json<GoToDefinitionResponse>, String, String, String)>, ErrorData> {
        let scoped_roots = self.roots_for_repository(repository_id_hint)?;
        if repository_id_hint.is_none() && scoped_roots.len() != 1 {
            return Ok(None);
        }

        let mut scoped_roots = scoped_roots;
        scoped_roots.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        for (repository_id, root) in scoped_roots {
            let Ok(cached_precise_graph) = self.precise_graph_for_repository_root(
                &repository_id,
                &root,
                self.find_references_resource_budgets(),
            ) else {
                continue;
            };
            let relative_path = Self::canonicalize_navigation_path(&root, raw_path);
            let graph = cached_precise_graph.graph;
            let Some(precise_target) = graph.select_precise_symbol_for_location(
                &repository_id,
                &relative_path,
                line,
                column,
            ) else {
                continue;
            };

            let mut precise_matches = graph
                .precise_occurrences_for_symbol(&repository_id, &precise_target.symbol)
                .into_iter()
                .filter(|occurrence| occurrence.is_definition())
                .map(|occurrence| NavigationLocation {
                    symbol: if precise_target.display_name.is_empty() {
                        precise_target.symbol.clone()
                    } else {
                        precise_target.display_name.clone()
                    },
                    repository_id: repository_id.clone(),
                    path: Self::canonicalize_navigation_path(&root, &occurrence.path),
                    line: occurrence.range.start_line,
                    column: occurrence.range.start_column,
                    kind: Self::display_symbol_kind(&precise_target.kind),
                    precision: Some(
                        Self::precise_match_precision(cached_precise_graph.coverage_mode)
                            .to_owned(),
                    ),
                    follow_up_structural: Vec::new(),
                })
                .collect::<Vec<_>>();
            Self::sort_navigation_locations(&mut precise_matches);
            if precise_matches.is_empty() {
                continue;
            }
            if precise_matches.len() > limit {
                precise_matches.truncate(limit);
            }
            if include_follow_up_structural {
                Self::populate_navigation_location_follow_up_structural(
                    &root,
                    &mut precise_matches,
                );
            }

            let precision =
                Self::precise_resolution_precision(cached_precise_graph.coverage_mode).to_owned();
            let metadata = json!({
                "precision": precision,
                "heuristic": false,
                "target_precise_symbol": precise_target.symbol.clone(),
                "resolution_source": "location_precise_cache",
                "precise": Self::precise_note_with_count(
                    cached_precise_graph.coverage_mode,
                    &cached_precise_graph.ingest_stats,
                    "definition_count",
                    precise_matches.len(),
                )
            });
            let (metadata, note) = Self::metadata_note_pair(metadata);
            return Ok(Some((
                Json(GoToDefinitionResponse {
                    matches: precise_matches,
                    mode: Self::navigation_mode_from_precision_label(Some(&precision)),
                    metadata,
                    note,
                }),
                repository_id,
                precise_target.symbol,
                precision,
            )));
        }

        Ok(None)
    }

    pub(in crate::mcp::server) fn canonicalize_navigation_path(
        root: &Path,
        raw_path: &str,
    ) -> String {
        let path = PathBuf::from(raw_path);
        let absolute_path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        Self::relative_display_path(root, &absolute_path)
    }

    pub(in crate::mcp::server) fn precise_definition_occurrence_for_symbol(
        graph: &SymbolGraph,
        repository_id: &str,
        symbol: &str,
    ) -> Option<crate::graph::PreciseOccurrenceRecord> {
        graph.precise_definition_occurrence_for_symbol(repository_id, symbol)
    }

    fn precise_navigation_candidate_anchor_rank(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        target_symbol: &SymbolDefinition,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> (u8, String, usize, usize) {
        let Some(definition) = Self::precise_definition_occurrence_for_symbol(
            graph,
            repository_id,
            &precise_target.symbol,
        ) else {
            return (4, String::new(), usize::MAX, usize::MAX);
        };

        let target_path = Self::relative_display_path(root, &target_symbol.path);
        let definition_path = Self::canonicalize_navigation_path(root, &definition.path);
        let rank = if definition_path == target_path
            && definition.range.start_line == target_symbol.line
            && definition.range.start_column == target_symbol.span.start_column
        {
            0
        } else if definition_path == target_path
            && definition.range.start_line == target_symbol.line
        {
            1
        } else if definition_path == target_path {
            2
        } else {
            3
        };

        (
            rank,
            definition_path,
            definition.range.start_line,
            definition.range.start_column,
        )
    }

    pub(in crate::mcp::server) fn matching_precise_symbols_for_resolved_target(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        symbol_query: &str,
        target_symbol: &SymbolDefinition,
    ) -> Vec<crate::graph::PreciseSymbolRecord> {
        let mut candidates = graph.matching_precise_symbols_for_navigation(
            repository_id,
            symbol_query,
            &target_symbol.name,
        );
        candidates.sort_by(|left, right| {
            Self::precise_navigation_candidate_anchor_rank(
                graph,
                repository_id,
                root,
                target_symbol,
                left,
            )
            .cmp(&Self::precise_navigation_candidate_anchor_rank(
                graph,
                repository_id,
                root,
                target_symbol,
                right,
            ))
            .then(left.symbol.cmp(&right.symbol))
            .then(left.display_name.cmp(&right.display_name))
            .then(left.kind.cmp(&right.kind))
        });
        candidates
    }

    pub(in crate::mcp::server) fn select_precise_symbol_for_resolved_target(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        symbol_query: &str,
        target_symbol: &SymbolDefinition,
    ) -> Option<crate::graph::PreciseSymbolRecord> {
        Self::matching_precise_symbols_for_resolved_target(
            graph,
            repository_id,
            root,
            symbol_query,
            target_symbol,
        )
        .into_iter()
        .next()
    }

    fn precise_relationships_to_symbol_by_kind(
        graph: &SymbolGraph,
        repository_id: &str,
        to_symbol: &str,
        kinds: &[PreciseRelationshipKind],
    ) -> Vec<crate::graph::PreciseRelationshipRecord> {
        graph.precise_relationships_to_symbol_by_kinds(repository_id, to_symbol, kinds)
    }

    pub(in crate::mcp::server) fn sort_navigation_locations(matches: &mut [NavigationLocation]) {
        matches.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.symbol.cmp(&right.symbol))
                .then(left.kind.cmp(&right.kind))
                .then(left.precision.cmp(&right.precision))
        });
    }

    pub(in crate::mcp::server) fn sort_implementation_matches(matches: &mut [ImplementationMatch]) {
        matches.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.path.cmp(&right.path))
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.symbol.cmp(&right.symbol))
                .then(left.kind.cmp(&right.kind))
                .then(left.relation.cmp(&right.relation))
                .then(left.precision.cmp(&right.precision))
                .then(left.fallback_reason.cmp(&right.fallback_reason))
        });
    }

    pub(in crate::mcp::server) fn precise_implementation_matches_for_symbol(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> Vec<ImplementationMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let mut matches = Self::precise_relationships_to_symbol_by_kind(
            graph,
            repository_id,
            &precise_target.symbol,
            &[
                PreciseRelationshipKind::Implementation,
                PreciseRelationshipKind::TypeDefinition,
            ],
        )
        .into_iter()
        .filter_map(|relationship| {
            let implementation_symbol = graph
                .precise_symbol(repository_id, &relationship.from_symbol)?
                .clone();
            let definition = Self::precise_definition_occurrence_for_symbol(
                graph,
                repository_id,
                &relationship.from_symbol,
            )?;
            Some(ImplementationMatch {
                symbol: if implementation_symbol.display_name.is_empty() {
                    implementation_symbol.symbol
                } else {
                    implementation_symbol.display_name
                },
                kind: Self::display_symbol_kind(&implementation_symbol.kind),
                repository_id: repository_id.to_owned(),
                path: Self::canonicalize_navigation_path(root, &definition.path),
                line: definition.range.start_line,
                column: definition.range.start_column,
                relation: Some(relationship.kind.as_str().to_owned()),
                precision: Some(precision.clone()),
                fallback_reason: None,
                follow_up_structural: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
        Self::sort_implementation_matches(&mut matches);
        matches
    }

    pub(in crate::mcp::server) fn precise_implementation_matches_from_occurrences(
        graph: &SymbolGraph,
        target_corpus: &RepositorySymbolCorpus,
        root: &Path,
        target_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> Vec<ImplementationMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let target_name = if precise_target.display_name.is_empty() {
            target_symbol_name
        } else {
            precise_target.display_name.as_str()
        };

        let mut matches = graph
            .precise_references_for_symbol(&target_corpus.repository_id, &precise_target.symbol)
            .into_iter()
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    None,
                )?;
                if enclosing_symbol.kind.as_str() != "impl" {
                    return None;
                }

                let (implemented_trait, implementing_type) =
                    parse_rust_impl_signature(enclosing_symbol.name.as_str())?;
                let (symbol, kind, path, line, column, relation) =
                    if let Some(implemented_trait) = implemented_trait {
                        if implemented_trait.eq_ignore_ascii_case(target_name) {
                            let implementing_symbol = graph.select_precise_symbol_for_navigation(
                                &target_corpus.repository_id,
                                implementing_type,
                                implementing_type,
                            )?;
                            let definition = Self::precise_definition_occurrence_for_symbol(
                                graph,
                                &target_corpus.repository_id,
                                &implementing_symbol.symbol,
                            )?;
                            (
                                if implementing_symbol.display_name.is_empty() {
                                    implementing_symbol.symbol
                                } else {
                                    implementing_symbol.display_name
                                },
                                Self::display_symbol_kind(&implementing_symbol.kind),
                                Self::canonicalize_navigation_path(root, &definition.path),
                                definition.range.start_line,
                                definition.range.start_column,
                                Some("implementation".to_owned()),
                            )
                        } else if implementing_type.eq_ignore_ascii_case(target_name) {
                            (
                                enclosing_symbol.name.clone(),
                                Self::display_symbol_kind(enclosing_symbol.kind.as_str()),
                                Self::relative_display_path(root, &enclosing_symbol.path),
                                enclosing_symbol.line,
                                enclosing_symbol.span.start_column,
                                Some("type_definition".to_owned()),
                            )
                        } else {
                            return None;
                        }
                    } else if implementing_type.eq_ignore_ascii_case(target_name) {
                        (
                            enclosing_symbol.name.clone(),
                            Self::display_symbol_kind(enclosing_symbol.kind.as_str()),
                            Self::relative_display_path(root, &enclosing_symbol.path),
                            enclosing_symbol.line,
                            enclosing_symbol.span.start_column,
                            Some("type_definition".to_owned()),
                        )
                    } else {
                        return None;
                    };

                Some(ImplementationMatch {
                    symbol,
                    kind,
                    repository_id: target_corpus.repository_id.clone(),
                    path,
                    line,
                    column,
                    relation,
                    precision: Some(precision.clone()),
                    fallback_reason: None,
                    follow_up_structural: Vec::new(),
                })
            })
            .collect::<Vec<_>>();
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

    pub(in crate::mcp::server) fn precise_incoming_matches_from_relationships(
        graph: &SymbolGraph,
        repository_id: &str,
        root: &Path,
        target_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
    ) -> Vec<CallHierarchyMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let mut matches = Self::precise_relationships_to_symbol_by_kind(
            graph,
            repository_id,
            &precise_target.symbol,
            &[PreciseRelationshipKind::Reference],
        )
        .into_iter()
        .filter_map(|relationship| {
            let caller_symbol = graph
                .precise_symbol(repository_id, &relationship.from_symbol)?
                .clone();
            let caller_definition = Self::precise_definition_occurrence_for_symbol(
                graph,
                repository_id,
                &relationship.from_symbol,
            )?;
            Some(CallHierarchyMatch {
                source_symbol: if caller_symbol.display_name.is_empty() {
                    caller_symbol.symbol
                } else {
                    caller_symbol.display_name
                },
                target_symbol: if precise_target.display_name.is_empty() {
                    target_symbol_name.to_owned()
                } else {
                    precise_target.display_name.clone()
                },
                repository_id: repository_id.to_owned(),
                path: Self::canonicalize_navigation_path(root, &caller_definition.path),
                line: caller_definition.range.start_line,
                column: caller_definition.range.start_column,
                relation: "calls".to_owned(),
                precision: Some(precision.clone()),
                call_path: None,
                call_line: None,
                call_column: None,
                call_end_line: None,
                call_end_column: None,
                follow_up_structural: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
        Self::sort_call_hierarchy_matches(&mut matches);
        matches
    }

    fn precise_enclosing_symbol_for_occurrence<'a>(
        target_corpus: &'a RepositorySymbolCorpus,
        root: &Path,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
        exclude_symbol_id: Option<&str>,
    ) -> Option<&'a SymbolDefinition> {
        let occurrence_path = Self::canonicalize_navigation_path(root, &occurrence.path);
        target_corpus
            .symbols_by_relative_path
            .get(&occurrence_path)
            .into_iter()
            .flat_map(|indices| indices.iter())
            .map(|index| &target_corpus.symbols[*index])
            .filter(|symbol| {
                exclude_symbol_id
                    .map(|exclude| symbol.stable_id != exclude)
                    .unwrap_or(true)
            })
            .filter(|symbol| {
                Self::source_span_contains_precise_range(&symbol.span, &occurrence.range)
            })
            .min_by(|left, right| {
                let left_span = left.span.end_line.saturating_sub(left.span.start_line);
                let right_span = right.span.end_line.saturating_sub(right.span.start_line);
                let left_column_span = if left_span == 0 {
                    left.span.end_column.saturating_sub(left.span.start_column)
                } else {
                    usize::MAX
                };
                let right_column_span = if right_span == 0 {
                    right
                        .span
                        .end_column
                        .saturating_sub(right.span.start_column)
                } else {
                    usize::MAX
                };
                left_span
                    .cmp(&right_span)
                    .then(left_column_span.cmp(&right_column_span))
                    .then(left.span.start_line.cmp(&right.span.start_line))
                    .then(left.span.start_column.cmp(&right.span.start_column))
                    .then(left.stable_id.cmp(&right.stable_id))
            })
    }

    pub(in crate::mcp::server) fn precise_incoming_matches_from_occurrences(
        graph: &SymbolGraph,
        target_corpus: &RepositorySymbolCorpus,
        root: &Path,
        target_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
        exclude_symbol_id: &str,
    ) -> Vec<CallHierarchyMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let mut source_cache: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut matches = graph
            .precise_references_for_symbol(&target_corpus.repository_id, &precise_target.symbol)
            .into_iter()
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    Some(exclude_symbol_id),
                )?;
                let relation = Self::classify_precise_incoming_occurrence_relation(
                    root,
                    precise_target,
                    &occurrence,
                    &mut source_cache,
                );
                let (call_path, call_line, call_column, call_end_line, call_end_column) =
                    Self::precise_call_site_fields(root, &occurrence);
                Some(CallHierarchyMatch {
                    source_symbol: enclosing_symbol.name.clone(),
                    target_symbol: if precise_target.display_name.is_empty() {
                        target_symbol_name.to_owned()
                    } else {
                        precise_target.display_name.clone()
                    },
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::relative_display_path(root, &enclosing_symbol.path),
                    line: enclosing_symbol.line,
                    column: enclosing_symbol.span.start_column,
                    relation: relation.to_owned(),
                    precision: Some(precision.clone()),
                    call_path,
                    call_line,
                    call_column,
                    call_end_line,
                    call_end_column,
                    follow_up_structural: Vec::new(),
                })
            })
            .collect::<Vec<_>>();
        Self::sort_call_hierarchy_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.source_symbol == right.source_symbol
                && left.target_symbol == right.target_symbol
                && left.relation == right.relation
                && left.precision == right.precision
                && left.call_path == right.call_path
                && left.call_line == right.call_line
                && left.call_column == right.call_column
                && left.call_end_line == right.call_end_line
                && left.call_end_column == right.call_end_column
        });
        matches
    }

    fn classify_precise_incoming_occurrence_relation(
        root: &Path,
        precise_target: &crate::graph::PreciseSymbolRecord,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
        source_cache: &mut BTreeMap<String, Option<String>>,
    ) -> &'static str {
        if Self::precise_occurrence_has_call_like_source(
            root,
            precise_target,
            occurrence,
            source_cache,
        ) {
            "calls"
        } else {
            "refers_to"
        }
    }

    fn precise_occurrence_has_call_like_source(
        root: &Path,
        precise_target: &crate::graph::PreciseSymbolRecord,
        occurrence: &crate::graph::PreciseOccurrenceRecord,
        source_cache: &mut BTreeMap<String, Option<String>>,
    ) -> bool {
        let source = source_cache
            .entry(occurrence.path.clone())
            .or_insert_with(|| {
                let occurrence_path = Path::new(&occurrence.path);
                let absolute_path = if occurrence_path.is_absolute() {
                    occurrence_path.to_path_buf()
                } else {
                    root.join(occurrence_path)
                };
                fs::read_to_string(absolute_path).ok()
            })
            .as_deref();
        let Some(source) = source else {
            return false;
        };
        let Some(line) = Self::source_line_for_precise_range(source, &occurrence.range) else {
            return false;
        };
        let target_name = Self::precise_target_call_name(precise_target);
        line.match_indices(target_name.as_str()).any(|(index, _)| {
            let suffix_start = index.saturating_add(target_name.len()).min(line.len());
            line.get(suffix_start..)
                .map(rust_source_suffix_looks_like_call)
                .unwrap_or(false)
        })
    }

    pub(in crate::mcp::server) fn precise_symbol_label(
        precise_symbol: &crate::graph::PreciseSymbolRecord,
    ) -> String {
        crate::graph::precise_navigation_identifier(&precise_symbol.display_name)
            .or_else(|| crate::graph::precise_navigation_identifier(&precise_symbol.symbol))
            .unwrap_or_else(|| precise_symbol.symbol.clone())
    }

    fn precise_target_call_name(precise_target: &crate::graph::PreciseSymbolRecord) -> String {
        Self::precise_symbol_label(precise_target)
    }

    fn source_line_for_precise_range<'a>(
        source: &'a str,
        range: &crate::graph::PreciseRange,
    ) -> Option<&'a str> {
        source.lines().nth(range.start_line.saturating_sub(1))
    }

    pub(in crate::mcp::server) fn precise_outgoing_matches_from_occurrences(
        graph: &SymbolGraph,
        target_corpus: &RepositorySymbolCorpus,
        root: &Path,
        source_symbol_name: &str,
        coverage_mode: PreciseCoverageMode,
        precise_target: &crate::graph::PreciseSymbolRecord,
        enclosing_symbol_id: &str,
    ) -> Vec<CallHierarchyMatch> {
        let precision = Self::precise_match_precision(coverage_mode).to_owned();
        let source_definition = match Self::precise_definition_occurrence_for_symbol(
            graph,
            &target_corpus.repository_id,
            &precise_target.symbol,
        ) {
            Some(definition) => definition,
            None => return Vec::new(),
        };
        let source_path = Self::canonicalize_navigation_path(root, &source_definition.path);
        let mut source_cache: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut matches = graph
            .precise_occurrences_for_file(&target_corpus.repository_id, &source_path)
            .into_iter()
            .filter(|occurrence| !occurrence.is_definition())
            .filter(|occurrence| occurrence.symbol != precise_target.symbol)
            .filter_map(|occurrence| {
                let enclosing_symbol = Self::precise_enclosing_symbol_for_occurrence(
                    target_corpus,
                    root,
                    &occurrence,
                    None,
                )?;
                if enclosing_symbol.stable_id != enclosing_symbol_id {
                    return None;
                }

                let callee_symbol = graph
                    .precise_symbol(&target_corpus.repository_id, &occurrence.symbol)?
                    .clone();
                if !Self::is_precise_callable_kind(&callee_symbol.kind)
                    && !Self::precise_occurrence_has_call_like_source(
                        root,
                        &callee_symbol,
                        &occurrence,
                        &mut source_cache,
                    )
                {
                    return None;
                }
                let callee_definition = Self::precise_definition_occurrence_for_symbol(
                    graph,
                    &target_corpus.repository_id,
                    &occurrence.symbol,
                )?;
                let (call_path, call_line, call_column, call_end_line, call_end_column) =
                    Self::precise_call_site_fields(root, &occurrence);
                Some(CallHierarchyMatch {
                    source_symbol: if precise_target.display_name.is_empty() {
                        source_symbol_name.to_owned()
                    } else {
                        precise_target.display_name.clone()
                    },
                    target_symbol: Self::precise_symbol_label(&callee_symbol),
                    repository_id: target_corpus.repository_id.clone(),
                    path: Self::canonicalize_navigation_path(root, &callee_definition.path),
                    line: callee_definition.range.start_line,
                    column: callee_definition.range.start_column,
                    relation: "calls".to_owned(),
                    precision: Some(precision.clone()),
                    call_path,
                    call_line,
                    call_column,
                    call_end_line,
                    call_end_column,
                    follow_up_structural: Vec::new(),
                })
            })
            .collect::<Vec<_>>();
        Self::sort_call_hierarchy_matches(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.source_symbol == right.source_symbol
                && left.target_symbol == right.target_symbol
                && left.relation == right.relation
                && left.precision == right.precision
                && left.call_path == right.call_path
                && left.call_line == right.call_line
                && left.call_column == right.call_column
                && left.call_end_line == right.call_end_line
                && left.call_end_column == right.call_end_column
        });
        matches
    }

    fn position_leq(
        left_line: usize,
        left_column: usize,
        right_line: usize,
        right_column: usize,
    ) -> bool {
        (left_line, left_column) <= (right_line, right_column)
    }

    fn source_span_contains_precise_range(
        span: &SourceSpan,
        range: &crate::graph::PreciseRange,
    ) -> bool {
        Self::position_leq(
            span.start_line,
            span.start_column,
            range.start_line,
            range.start_column,
        ) && Self::position_leq(
            range.end_line,
            range.end_column,
            span.end_line,
            span.end_column,
        )
    }

    pub(in crate::mcp::server) fn precise_kind_numeric_value(kind: &str) -> Option<i32> {
        kind.strip_prefix("kind_")
            .unwrap_or(kind)
            .parse::<i32>()
            .ok()
    }

    pub(in crate::mcp::server) fn display_symbol_kind(kind: &str) -> Option<String> {
        let normalized = kind.trim();
        if normalized.is_empty() {
            return None;
        }

        if let Some(value) = Self::precise_kind_numeric_value(normalized) {
            if let Some(kind) = ScipSymbolKind::from_i32(value) {
                return Some(Self::camel_to_snake_case(&format!("{kind:?}")));
            }
        }

        Some(Self::camel_to_snake_case(normalized))
    }

    fn camel_to_snake_case(raw: &str) -> String {
        let mut output = String::with_capacity(raw.len());
        let mut previous_was_separator = false;
        let mut previous_was_lower_or_digit = false;

        for character in raw.chars() {
            if matches!(character, '_' | '-' | ' ' | '\t') {
                if !output.ends_with('_') && !output.is_empty() {
                    output.push('_');
                }
                previous_was_separator = true;
                previous_was_lower_or_digit = false;
                continue;
            }

            if character.is_ascii_uppercase()
                && !output.is_empty()
                && !previous_was_separator
                && previous_was_lower_or_digit
            {
                output.push('_');
            }

            output.push(character.to_ascii_lowercase());
            previous_was_separator = false;
            previous_was_lower_or_digit =
                character.is_ascii_lowercase() || character.is_ascii_digit();
        }

        output
    }
}
