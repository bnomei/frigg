//! Symbol and cursor target resolution for navigation tools across corpora and source evidence.
//!
//! Resolves cursor and symbol targets across corpora, source evidence, and precise graph when
//! multiple candidates compete.

use super::*;
use crate::mcp::server::presentation::SessionResultHandleLookup;
use crate::mcp::types::TargetRef;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Component;

impl FriggMcpServer {
    /// Resolve either an issued target reference or legacy direct navigation inputs.
    /// Target references are exclusive with direct identity fields and never fall back to name
    /// ranking. A top-level repository id remains legal only as an equality assertion.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::mcp::server) fn resolve_navigation_request(
        &self,
        corpora: &[Arc<RepositorySymbolCorpus>],
        target: Option<&TargetRef>,
        symbol: Option<&str>,
        path: Option<&str>,
        line: Option<usize>,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        if let Some(target) = target {
            Self::validate_navigation_target_inputs(Some(target), symbol, path, line, column)?;
            return match repository_id_hint {
                Some(repository_id) => self.resolve_target_ref_with_repository_assertion(
                    corpora,
                    target,
                    Some(repository_id),
                ),
                None => self.resolve_target_ref(corpora, target),
            };
        }
        Self::resolve_navigation_target(corpora, symbol, path, line, column, repository_id_hint)
    }

    pub(in crate::mcp::server) fn validate_navigation_target_inputs(
        target: Option<&TargetRef>,
        symbol: Option<&str>,
        path: Option<&str>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Result<(), ErrorData> {
        if target.is_some()
            && (symbol.is_some() || path.is_some() || line.is_some() || column.is_some())
        {
            return Err(Self::target_invalid_params(
                "CONFLICTING_TARGET_INPUT",
                "target cannot be combined with symbol, path, line, or column",
                "Remove the direct symbol/location fields and send the issued target unchanged.",
            ));
        }
        Ok(())
    }

    /// Derive the only repository that an issued target is allowed to address. This preflight is
    /// used before corpus/freshness collection so a target without a redundant repository
    /// assertion cannot accidentally inherit the session default repository.
    pub(in crate::mcp::server) fn navigation_target_repository_hint(
        &self,
        target: Option<&TargetRef>,
        repository_id_assertion: Option<&str>,
    ) -> Result<Option<String>, ErrorData> {
        let Some(target) = target else {
            return Ok(repository_id_assertion.map(str::to_owned));
        };
        let repository_id = match target {
            TargetRef::StableSymbol { repository_id, .. } => repository_id.clone(),
            TargetRef::ResultMatch {
                result_handle,
                match_id,
                target_scope,
            } => {
                if target_scope != &self.session_state.display_session_id() {
                    return Err(Self::target_invalid_params(
                        "TARGET_SCOPE_MISMATCH",
                        "result target belongs to a different session",
                        "Use a target issued by this session.",
                    ));
                }
                match self.session_result_handle_lookup(result_handle, match_id) {
                    SessionResultHandleLookup::Found(anchor) => anchor.repository_id.clone(),
                    SessionResultHandleLookup::StaleHandle => {
                        return Err(Self::target_not_found(
                            "STALE_HANDLE",
                            "result target is stale",
                            "Issue a fresh search or navigation request.",
                        ));
                    }
                    SessionResultHandleLookup::MixedHandle { .. } => {
                        return Err(Self::target_not_found(
                            "MIXED_HANDLE",
                            "result target does not belong to this handle",
                            "Use the target with its original result handle.",
                        ));
                    }
                    SessionResultHandleLookup::TargetScopeMismatch => unreachable!(),
                }
            }
        };
        if repository_id_assertion.is_some_and(|asserted| asserted != repository_id) {
            return Err(Self::target_invalid_params(
                "TARGET_REPOSITORY_MISMATCH",
                "repository assertion does not match the target",
                "Remove the repository assertion or use the target's original repository.",
            ));
        }
        if self
            .attached_workspaces_for_repository(Some(&repository_id))
            .is_err()
        {
            return Err(Self::target_not_found(
                "REPOSITORY_NOT_FOUND",
                "target repository is not available",
                "Attach the target repository and issue a fresh result.",
            ));
        }
        Ok(Some(repository_id))
    }

    fn target_error_detail(error_code: &'static str, correction_hint: &'static str) -> Value {
        serde_json::json!({
            "error_code": error_code,
            "correction_hint": correction_hint,
            "related_tools": ["search_symbol", "search_text"],
            "next_actions": [],
            "suggested_next": [],
        })
    }

    pub(in crate::mcp::server) fn target_invalid_params(
        error_code: &'static str,
        message: &'static str,
        correction_hint: &'static str,
    ) -> ErrorData {
        Self::invalid_params(
            message,
            Some(Self::target_error_detail(error_code, correction_hint)),
        )
    }

    pub(in crate::mcp::server) fn target_not_found(
        error_code: &'static str,
        message: &'static str,
        correction_hint: &'static str,
    ) -> ErrorData {
        Self::resource_not_found(
            message,
            Some(Self::target_error_detail(error_code, correction_hint)),
        )
    }

    /// Resolve an issued target without falling back to name ranking.  This is the single
    /// target path used by navigation consumers: result handles are checked for session scope
    /// before the cache is consulted, while stable symbols are looked up directly in the adopted
    /// corpus snapshot.
    pub(in crate::mcp::server) fn resolve_target_ref(
        &self,
        corpora: &[Arc<RepositorySymbolCorpus>],
        target: &TargetRef,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        self.resolve_target_ref_with_repository_assertion(corpora, target, None)
    }

    fn resolve_target_ref_with_repository_assertion(
        &self,
        corpora: &[Arc<RepositorySymbolCorpus>],
        target: &TargetRef,
        repository_id_assertion: Option<&str>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        match target {
            TargetRef::ResultMatch {
                result_handle,
                match_id,
                target_scope,
            } => {
                if target_scope != &self.session_state.display_session_id() {
                    return Err(Self::target_invalid_params(
                        "TARGET_SCOPE_MISMATCH",
                        "result target belongs to a different session",
                        "Use a target issued by this session.",
                    ));
                }
                let anchor = match self.session_result_handle_lookup(result_handle, match_id) {
                    SessionResultHandleLookup::Found(anchor) => anchor,
                    SessionResultHandleLookup::StaleHandle => {
                        return Err(Self::target_not_found(
                            "STALE_HANDLE",
                            "result target is stale",
                            "Issue a fresh search or navigation request.",
                        ));
                    }
                    SessionResultHandleLookup::MixedHandle { .. } => {
                        return Err(Self::target_not_found(
                            "MIXED_HANDLE",
                            "result target does not belong to this handle",
                            "Use the target with its original result handle.",
                        ));
                    }
                    SessionResultHandleLookup::TargetScopeMismatch => unreachable!(),
                };
                if repository_id_assertion.is_some_and(|asserted| asserted != anchor.repository_id)
                {
                    return Err(Self::target_invalid_params(
                        "TARGET_REPOSITORY_MISMATCH",
                        "repository assertion does not match the result target",
                        "Remove the repository assertion or use the target's original repository.",
                    ));
                }
                self.verify_result_target_anchor_freshness(result_handle, match_id, &anchor)?;
                let corpus = corpora
                    .iter()
                    .find(|c| c.repository_id == anchor.repository_id)
                    .ok_or_else(|| {
                        Self::target_not_found(
                            "REPOSITORY_NOT_FOUND",
                            "target repository is not available",
                            "Attach the target repository and issue a fresh result.",
                        )
                    })?;
                let index = if let Some(stable_symbol_id) = anchor.stable_symbol_id.as_deref() {
                    corpus
                        .symbol_index_by_stable_id
                        .get(stable_symbol_id)
                        .copied()
                        .ok_or_else(|| {
                            Self::target_not_found(
                                "TARGET_NOT_FOUND",
                                "target symbol was not found",
                                "Issue a fresh symbol-bearing result.",
                            )
                        })?
                } else {
                    Self::unique_coordinate_symbol_index(
                        corpus,
                        &anchor.path,
                        anchor.line,
                        anchor.column,
                    )
                    .ok_or_else(|| {
                        Self::target_invalid_params(
                            "TARGET_ANCHOR_INSUFFICIENT",
                            "target anchor cannot be resolved exactly",
                            "Issue a fresh result with an exact symbol target.",
                        )
                    })?
                };
                let mut candidates = Vec::new();
                Self::push_symbol_candidate(&mut candidates, corpus, index, 0);
                Ok(ResolvedNavigationTarget {
                    symbol_query: corpus.symbols[index].name.clone(),
                    selection: NavigationTargetSelection::Resolved(ResolvedSymbolTarget {
                        candidate: candidates.remove(0),
                        corpus: Arc::clone(corpus),
                        candidate_count: 1,
                        selected_rank_candidate_count: 1,
                    }),
                    resolution_source: "result_match",
                })
            }
            TargetRef::StableSymbol {
                repository_id,
                stable_symbol_id,
                snapshot_token,
            } => {
                if repository_id_assertion.is_some_and(|asserted| asserted != repository_id) {
                    return Err(Self::target_invalid_params(
                        "TARGET_REPOSITORY_MISMATCH",
                        "repository assertion does not match the stable-symbol target",
                        "Remove the repository assertion or use the target's embedded repository.",
                    ));
                }
                let corpus = corpora
                    .iter()
                    .find(|c| &c.repository_id == repository_id)
                    .ok_or_else(|| {
                        Self::target_not_found(
                            "REPOSITORY_NOT_FOUND",
                            "target repository is not available",
                            "Attach the target repository and issue a fresh symbol result.",
                        )
                    })?;
                if &corpus.root_signature != snapshot_token {
                    return Err(Self::target_not_found(
                        "STALE_TARGET_SNAPSHOT",
                        "target snapshot is stale",
                        "Issue a fresh symbol result from the current repository snapshot.",
                    ));
                }
                let index = corpus
                    .symbol_index_by_stable_id
                    .get(stable_symbol_id)
                    .copied()
                    .ok_or_else(|| {
                        Self::target_not_found(
                            "TARGET_NOT_FOUND",
                            "target symbol was not found",
                            "Issue a fresh symbol result.",
                        )
                    })?;
                let mut candidates = Vec::new();
                Self::push_symbol_candidate(&mut candidates, corpus, index, 0);
                Ok(ResolvedNavigationTarget {
                    symbol_query: corpus.symbols[index].name.clone(),
                    selection: NavigationTargetSelection::Resolved(ResolvedSymbolTarget {
                        candidate: candidates.remove(0),
                        corpus: Arc::clone(corpus),
                        candidate_count: 1,
                        selected_rank_candidate_count: 1,
                    }),
                    resolution_source: "stable_symbol",
                })
            }
        }
    }

    fn unique_coordinate_symbol_index(
        corpus: &RepositorySymbolCorpus,
        path: &str,
        line: usize,
        column: Option<usize>,
    ) -> Option<usize> {
        let column = column?;
        let key = Self::normalize_relative_input_path(path);
        let indexes = corpus.symbols_by_relative_path.get(&key)?;
        let exact = indexes
            .iter()
            .copied()
            .filter(|i| {
                let symbol = &corpus.symbols[*i];
                symbol.span.start_line == line && symbol.span.start_column == column
            })
            .collect::<Vec<_>>();
        if exact.len() == 1 {
            return exact.first().copied();
        }
        if !exact.is_empty() {
            return None;
        }
        let mut containing: Vec<usize> = indexes
            .iter()
            .copied()
            .filter(|i| {
                let s = &corpus.symbols[*i];
                let start_ok = (s.span.start_line < line)
                    || (s.span.start_line == line && s.span.start_column <= column);
                let end_ok = s.span.end_line > line
                    || (s.span.end_line == line && column < s.span.end_column);
                start_ok && end_ok
            })
            .collect();
        containing.sort_by_key(|i| {
            let s = &corpus.symbols[*i];
            s.span.end_byte.saturating_sub(s.span.start_byte)
        });
        let first = *containing.first()?;
        let first_span = &corpus.symbols[first].span;
        let first_size = first_span.end_byte.saturating_sub(first_span.start_byte);
        let tied = containing.get(1).is_some_and(|second| {
            let span = &corpus.symbols[*second].span;
            span.end_byte.saturating_sub(span.start_byte) == first_size
        });
        (!tied).then_some(first)
    }
    fn php_helper_prefixes() -> &'static [(&'static str, NavigationPhpHelperKind)] {
        &[
            ("__(", NavigationPhpHelperKind::Translation),
            ("trans(", NavigationPhpHelperKind::Translation),
            ("route(", NavigationPhpHelperKind::Route),
            ("to_route(", NavigationPhpHelperKind::Route),
            ("config(", NavigationPhpHelperKind::Config),
            ("env(", NavigationPhpHelperKind::Env),
            ("Lang::get(", NavigationPhpHelperKind::Translation),
            ("->route(", NavigationPhpHelperKind::Route),
            ("routeIs(", NavigationPhpHelperKind::Route),
            ("->routeIs(", NavigationPhpHelperKind::Route),
        ]
    }

    pub(in crate::mcp::server) fn relative_display_path(root: &Path, path: &Path) -> String {
        match path.strip_prefix(root) {
            Ok(relative_path) => {
                Self::normalize_relative_input_path(&relative_path.to_string_lossy())
            }
            Err(_) if path.is_relative() => {
                Self::normalize_relative_input_path(&path.to_string_lossy())
            }
            Err(_) => path
                .to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_owned(),
        }
    }

    pub(in crate::mcp::server) fn symbol_name_match_rank(
        symbol_name: &str,
        query: &str,
        query_lower: &str,
    ) -> Option<u8> {
        if symbol_name == query {
            return Some(0);
        }

        let symbol_lower = symbol_name.to_ascii_lowercase();
        if symbol_lower == query_lower {
            return Some(1);
        }
        if symbol_lower.starts_with(query_lower) {
            return Some(2);
        }
        if symbol_lower.contains(query_lower) {
            return Some(3);
        }

        None
    }

    fn push_symbol_candidate(
        candidates: &mut Vec<SymbolCandidate>,
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
        rank: u8,
    ) {
        let symbol = corpus.symbols[symbol_index].clone();
        let relative_path = Self::relative_display_path(&corpus.root, &symbol.path);
        let path_class = Self::navigation_path_class(&relative_path);
        candidates.push(SymbolCandidate {
            rank,
            path_class_rank: Self::navigation_path_class_rank(path_class),
            path_class,
            repository_id: corpus.repository_id.clone(),
            root: corpus.root.clone(),
            symbol,
        });
    }

    pub(in crate::mcp::server) fn source_span_strictly_contains(
        parent: &SourceSpan,
        child: &SourceSpan,
    ) -> bool {
        let starts_before = parent.start_line < child.start_line
            || (parent.start_line == child.start_line && parent.start_column <= child.start_column);
        let ends_after = parent.end_line > child.end_line
            || (parent.end_line == child.end_line && parent.end_column >= child.end_column);
        starts_before
            && ends_after
            && (parent.start_line != child.start_line
                || parent.start_column != child.start_column
                || parent.end_line != child.end_line
                || parent.end_column != child.end_column)
    }

    pub(in crate::mcp::server) fn symbol_context_for_index(
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
    ) -> (Option<String>, Option<String>) {
        let Some(symbol) = corpus.symbols.get(symbol_index) else {
            return (None, None);
        };
        let container = corpus
            .container_symbol_index_by_index
            .get(symbol_index)
            .and_then(|container_index| {
                container_index
                    .and_then(|index| corpus.symbols.get(index).map(|symbol| symbol.name.clone()))
            });
        let signature = corpus
            .canonical_symbol_name_by_stable_id
            .get(symbol.stable_id.as_str())
            .cloned();
        (container, signature)
    }

    pub(in crate::mcp::server) fn symbol_context_for_stable_id(
        corpus: &RepositorySymbolCorpus,
        stable_id: &str,
    ) -> (Option<String>, Option<String>) {
        corpus
            .symbol_index_by_stable_id
            .get(stable_id)
            .map(|symbol_index| Self::symbol_context_for_index(corpus, *symbol_index))
            .unwrap_or((None, None))
    }

    /// Rank one corpus symbol for navigation/search. `column` is set only for real 1-based spans
    /// (`start_column > 0`); never invents column 1 as a dense-line disambiguator.
    pub(in crate::mcp::server) fn build_ranked_symbol_match(
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
        rank: u8,
        path_class_filter: Option<SearchSymbolPathClass>,
        path_regex: Option<&regex::Regex>,
    ) -> Option<RankedSymbolMatch> {
        let symbol = &corpus.symbols[symbol_index];
        let path = Self::relative_display_path(&corpus.root, &symbol.path);
        if let Some(path_class_filter) = path_class_filter
            && path_class_filter.is_concrete_filter()
            && Self::navigation_path_class(&path) != path_class_filter.as_str()
        {
            return None;
        }
        if let Some(path_regex) = path_regex
            && !path_regex.is_match(&path)
        {
            return None;
        }
        let rust_context = corpus
            .rust_symbol_context_by_index
            .get(symbol_index)
            .and_then(Option::as_ref);
        if path_class_filter == Some(SearchSymbolPathClass::Runtime)
            && rust_context.is_some_and(crate::languages::RustSymbolContext::is_test_context)
        {
            return None;
        }
        let path_class = Self::navigation_path_class(&path);
        let (container, signature) = Self::symbol_context_for_index(corpus, symbol_index);
        let column = (symbol.span.start_column > 0).then_some(symbol.span.start_column);
        let excerpt = signature
            .clone()
            .or_else(|| Some(format!("{} {}", symbol.kind.as_str(), symbol.name)));
        Some(RankedSymbolMatch {
            rank,
            path_class_rank: Self::navigation_path_class_rank(path_class),
            context_rank: if rust_context
                .is_some_and(crate::languages::RustSymbolContext::is_test_context)
            {
                1
            } else {
                0
            },
            matched: SymbolMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: Some(symbol.stable_id.clone()),
                repository_id: corpus.repository_id.clone(),
                symbol: symbol.name.clone(),
                kind: symbol.kind.as_str().to_owned(),
                path,
                line: symbol.line,
                column,
                excerpt,
                path_class: Some(path_class.to_owned()),
                container,
                signature,
            },
        })
    }

    pub(in crate::mcp::server) fn sort_ranked_symbol_matches(
        ranked_matches: &mut [RankedSymbolMatch],
    ) {
        ranked_matches.sort_by(|left, right| {
            left.rank
                .cmp(&right.rank)
                .then(left.path_class_rank.cmp(&right.path_class_rank))
                .then(left.context_rank.cmp(&right.context_rank))
                .then(left.matched.repository_id.cmp(&right.matched.repository_id))
                .then(left.matched.path.cmp(&right.matched.path))
                .then(left.matched.line.cmp(&right.matched.line))
                .then(left.matched.kind.cmp(&right.matched.kind))
                .then(left.matched.symbol.cmp(&right.matched.symbol))
        });
    }

    pub(in crate::mcp::server) fn dedup_ranked_symbol_matches(
        ranked_matches: &mut Vec<RankedSymbolMatch>,
    ) {
        ranked_matches.dedup_by(|left, right| {
            left.matched.repository_id == right.matched.repository_id
                && left.matched.path == right.matched.path
                && left.matched.line == right.matched.line
                && left.matched.kind == right.matched.kind
                && left.matched.symbol == right.matched.symbol
        });
    }

    fn resolve_navigation_symbol_target(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol_query: &str,
        repository_id_hint: Option<&str>,
        location_relative_path: Option<&str>,
        rust_hint: Option<&crate::languages::RustNavigationQueryHint>,
        require_disambiguation: bool,
        path_class_filter: Option<SearchSymbolPathClass>,
    ) -> Result<NavigationTargetSelection, ErrorData> {
        let mut candidates = Vec::new();
        let query_lower = symbol_query.to_ascii_lowercase();
        let query_looks_canonical = symbol_query.contains('\\')
            || symbol_query.contains("::")
            || symbol_query.contains('$');
        for corpus in corpora {
            if let Some(symbol_index) = corpus.symbol_index_by_stable_id.get(symbol_query) {
                Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 0);
            }
            if query_looks_canonical {
                if let Some(symbol_indices) =
                    corpus.symbol_indices_by_canonical_name.get(symbol_query)
                {
                    for symbol_index in symbol_indices {
                        Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 1);
                    }
                }
                if let Some(symbol_indices) = corpus
                    .symbol_indices_by_lower_canonical_name
                    .get(&query_lower)
                {
                    for symbol_index in symbol_indices {
                        let Some(canonical_name) = corpus
                            .canonical_symbol_name_by_stable_id
                            .get(corpus.symbols[*symbol_index].stable_id.as_str())
                        else {
                            continue;
                        };
                        if canonical_name != symbol_query {
                            Self::push_symbol_candidate(&mut candidates, corpus, *symbol_index, 2);
                        }
                    }
                }
            }
            let name_rank_offset = if query_looks_canonical { 3 } else { 1 };
            if let Some(symbol_indices) = corpus.symbol_indices_by_name.get(symbol_query) {
                for symbol_index in symbol_indices {
                    let symbol = &corpus.symbols[*symbol_index];
                    if navigation_symbol_target_rank(symbol, symbol_query) == Some(1) {
                        Self::push_symbol_candidate(
                            &mut candidates,
                            corpus,
                            *symbol_index,
                            name_rank_offset,
                        );
                    }
                }
            }
            if let Some(symbol_indices) = corpus.symbol_indices_by_lower_name.get(&query_lower) {
                for symbol_index in symbol_indices {
                    let symbol = &corpus.symbols[*symbol_index];
                    if navigation_symbol_target_rank(symbol, symbol_query) == Some(2) {
                        Self::push_symbol_candidate(
                            &mut candidates,
                            corpus,
                            *symbol_index,
                            name_rank_offset + 1,
                        );
                    }
                }
            }
        }

        if let Some(path_class_filter) = path_class_filter {
            candidates.retain(|candidate| {
                corpora.iter().any(|corpus| {
                    corpus.repository_id == candidate.repository_id
                        && corpus
                            .symbol_index_by_stable_id
                            .get(candidate.symbol.stable_id.as_str())
                            .is_some_and(|symbol_index| {
                                Self::build_ranked_symbol_match(
                                    corpus,
                                    *symbol_index,
                                    candidate.rank,
                                    Some(path_class_filter),
                                    None,
                                )
                                .is_some()
                            })
                })
            });
        }

        candidates.sort_by(|left, right| {
            let left_context = Self::navigation_symbol_context_ranks(
                corpora,
                left,
                location_relative_path,
                rust_hint,
            );
            let right_context = Self::navigation_symbol_context_ranks(
                corpora,
                right,
                location_relative_path,
                rust_hint,
            );
            left.rank
                .cmp(&right.rank)
                .then(left_context.cmp(&right_context))
                .then(left.path_class_rank.cmp(&right.path_class_rank))
                .then(left.repository_id.cmp(&right.repository_id))
                .then(left.symbol.path.cmp(&right.symbol.path))
                .then(left.symbol.line.cmp(&right.symbol.line))
                .then(left.symbol.stable_id.cmp(&right.symbol.stable_id))
        });
        let candidate_count = candidates.len();
        let candidate = candidates.first().cloned().ok_or_else(|| {
            Self::resource_not_found(
                "symbol not found",
                Some(json!({
                    "symbol": symbol_query,
                    "repository_id": repository_id_hint,
                })),
            )
        })?;
        let corpus = corpora
            .iter()
            .find(|corpus| corpus.repository_id == candidate.repository_id)
            .cloned()
            .ok_or_else(|| {
                Self::internal(
                    "target symbol repository was not present in corpus set",
                    Some(json!({
                        "repository_id": candidate.repository_id.clone(),
                        "symbol_id": candidate.symbol.stable_id.clone(),
                    })),
                )
            })?;
        let selected_rank_candidate_count = candidates
            .iter()
            .take_while(|resolved| resolved.rank == candidate.rank)
            .count();
        let multi_repo_same_rank = repository_id_hint.is_none()
            && require_disambiguation
            && selected_rank_candidate_count > 1
            && {
                let mut repos = candidates
                    .iter()
                    .take(selected_rank_candidate_count)
                    .map(|c| c.repository_id.as_str());
                let first = repos.next();
                first.is_some_and(|first_repo| repos.any(|repo| repo != first_repo))
            };
        if (require_disambiguation && selected_rank_candidate_count > 1) || multi_repo_same_rank {
            let same_rank_candidates = candidates
                .into_iter()
                .take(selected_rank_candidate_count)
                .collect::<Vec<_>>();
            return Ok(NavigationTargetSelection::DisambiguationRequired(
                DisambiguationRequiredSymbolTarget {
                    candidates: same_rank_candidates,
                    candidate_count,
                    selected_rank_candidate_count,
                },
            ));
        }

        Ok(NavigationTargetSelection::Resolved(ResolvedSymbolTarget {
            candidate,
            corpus,
            candidate_count,
            selected_rank_candidate_count,
        }))
    }

    pub(in crate::mcp::server) fn navigation_path_class(relative_path: &str) -> &'static str {
        repository_path_class(relative_path)
    }

    pub(in crate::mcp::server) fn navigation_path_class_rank(path_class: &str) -> u8 {
        repository_path_class_rank(path_class)
    }

    fn collapse_consecutive_slashes(path: &str) -> String {
        let mut collapsed = String::with_capacity(path.len());
        let mut previous_was_slash = false;
        for ch in path.chars() {
            if ch == '/' {
                if !previous_was_slash {
                    collapsed.push(ch);
                    previous_was_slash = true;
                }
            } else {
                collapsed.push(ch);
                previous_was_slash = false;
            }
        }
        collapsed
    }

    fn normalize_relative_input_path(raw_path: &str) -> String {
        let normalized = Self::collapse_consecutive_slashes(&raw_path.replace('\\', "/"));
        let path = Path::new(&normalized);
        if path.is_absolute() {
            return Self::collapse_consecutive_slashes(normalized.trim_start_matches("./"));
        }

        let mut components = Vec::<String>::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => {
                    let part = part.to_string_lossy();
                    if !part.is_empty() {
                        components.push(part.into_owned());
                    }
                }
                Component::ParentDir => {
                    if components.last().is_some_and(|part| part != "..") {
                        components.pop();
                    } else {
                        components.push("..".to_owned());
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Self::collapse_consecutive_slashes(normalized.trim_start_matches("./"));
                }
            }
        }

        if components.is_empty() {
            Self::collapse_consecutive_slashes(normalized.trim_start_matches("./"))
        } else {
            components.join("/")
        }
    }

    pub(in crate::mcp::server) fn navigation_path_within_root(
        root: &Path,
        candidate: &Path,
    ) -> bool {
        let Ok(root_canonical) = root.canonicalize() else {
            return false;
        };
        let Ok(candidate_canonical) = candidate.canonicalize() else {
            return false;
        };
        candidate_canonical.starts_with(&root_canonical)
    }

    pub(in crate::mcp::server) fn requested_location_path_for_corpus(
        corpus: &RepositorySymbolCorpus,
        raw_path: &str,
    ) -> String {
        Self::canonicalize_navigation_path(&corpus.root, raw_path)
    }

    fn navigation_symbol_context_ranks(
        corpora: &[Arc<RepositorySymbolCorpus>],
        candidate: &SymbolCandidate,
        location_relative_path: Option<&str>,
        rust_hint: Option<&crate::languages::RustNavigationQueryHint>,
    ) -> (u8, u8, u8, u8, u8) {
        let relative_path = Self::relative_display_path(&candidate.root, &candidate.symbol.path);
        let same_file_rank = match rust_hint {
            Some(hint) if hint.prefer_same_file => {
                if location_relative_path == Some(relative_path.as_str()) {
                    0
                } else {
                    1
                }
            }
            Some(_) => 1,
            None => {
                if location_relative_path == Some(relative_path.as_str()) {
                    0
                } else {
                    1
                }
            }
        };
        let method_rank = rust_hint.map_or(0, |hint| {
            if hint.prefer_method && candidate.symbol.kind != crate::indexer::SymbolKind::Method {
                1
            } else {
                0
            }
        });
        let module_rank = rust_hint.map_or(0, |hint| {
            Self::rust_navigation_module_affinity_rank(&hint.module_path_segments, &relative_path)
        });
        let impl_rank = rust_hint.map_or(0, |hint| {
            if hint.enclosing_impl_type.is_none() {
                return 0;
            }
            let Some(corpus) = corpora
                .iter()
                .find(|corpus| corpus.repository_id == candidate.repository_id)
            else {
                return 1;
            };
            let context = rust_enclosing_symbol_context(&candidate.symbol, &corpus.symbols);
            if context
                .impl_type
                .as_deref()
                .zip(hint.enclosing_impl_type.as_deref())
                .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
            {
                0
            } else {
                1
            }
        });
        let trait_rank = rust_hint.map_or(0, |hint| {
            if hint.enclosing_trait.is_none() {
                return 0;
            }
            let Some(corpus) = corpora
                .iter()
                .find(|corpus| corpus.repository_id == candidate.repository_id)
            else {
                return 1;
            };
            let context = rust_enclosing_symbol_context(&candidate.symbol, &corpus.symbols);
            let target_trait = hint.enclosing_trait.as_deref().unwrap_or_default();
            if context
                .trait_name
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(target_trait))
                || context
                    .impl_trait
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(target_trait))
            {
                0
            } else {
                1
            }
        });

        (
            same_file_rank,
            method_rank,
            module_rank,
            impl_rank,
            trait_rank,
        )
    }

    fn rust_navigation_module_affinity_rank(hint_segments: &[String], relative_path: &str) -> u8 {
        if hint_segments.is_empty() {
            return 0;
        }
        let candidate_segments = rust_relative_path_module_segments(relative_path);
        if candidate_segments.is_empty() {
            return 3;
        }
        if candidate_segments == hint_segments {
            return 0;
        }
        if candidate_segments.starts_with(hint_segments)
            || candidate_segments.ends_with(hint_segments)
        {
            return 0;
        }
        if hint_segments
            .iter()
            .all(|segment| candidate_segments.contains(segment))
        {
            return 1;
        }
        if hint_segments
            .iter()
            .any(|segment| candidate_segments.contains(segment))
        {
            return 2;
        }
        3
    }

    fn navigation_location_absolute_paths(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        repository_id_hint: Option<&str>,
    ) -> Vec<PathBuf> {
        corpora
            .iter()
            .filter(|corpus| {
                repository_id_hint.is_none_or(|hint| {
                    corpus.repository_id == hint || corpus.runtime_repository_id == hint
                })
            })
            .filter_map(|corpus| {
                let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
                let absolute_path = corpus.root.join(&requested_path);
                (Self::navigation_path_within_root(&corpus.root, &absolute_path)
                    && absolute_path.is_file())
                .then_some(absolute_path)
            })
            .collect()
    }

    pub(in crate::mcp::server) fn validate_navigation_location_line_bounds(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        line: usize,
        repository_id_hint: Option<&str>,
    ) -> Result<(), ErrorData> {
        if line == 0 {
            return Ok(());
        }

        let matching_paths =
            Self::navigation_location_absolute_paths(corpora, raw_path, repository_id_hint);
        if matching_paths.is_empty() {
            return Ok(());
        }

        let mut first_bounds_error = None;
        for absolute_path in &matching_paths {
            let snapshot = FileContentSnapshot::from_path(absolute_path).map_err(|err| {
                Self::map_lossy_line_slice_error(absolute_path, LossyLineSliceError::Io(err))
            })?;
            match snapshot.read_line_slice_lossy(line, Some(line), usize::MAX) {
                Ok(_) => return Ok(()),
                Err(error) => {
                    first_bounds_error.get_or_insert_with(|| {
                        Self::map_lossy_line_slice_error(absolute_path, error)
                    });
                }
            }
        }

        if let Some(error) = first_bounds_error {
            return Err(error);
        }

        Ok(())
    }

    fn resolve_navigation_symbol_query_from_location(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
    ) -> Result<String, ErrorData> {
        if line == 0 {
            return Err(Self::invalid_params(
                "line must be greater than zero",
                Some(json!({
                    "line": line,
                })),
            ));
        }
        if column == Some(0) {
            return Err(Self::invalid_params(
                "column must be greater than zero when provided",
                Some(json!({
                    "column": column,
                })),
            ));
        }

        let mut candidates: Vec<(usize, usize, String, String, usize, usize, String)> = Vec::new();
        for corpus in corpora {
            let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
            let Some(symbol_indices) = corpus.symbols_by_relative_path.get(&requested_path) else {
                continue;
            };
            for symbol_index in symbol_indices {
                let symbol = &corpus.symbols[*symbol_index];
                let symbol_path = Self::relative_display_path(&corpus.root, &symbol.path);
                if symbol.line > line {
                    break;
                }
                if let Some(column) = column
                    && symbol.line == line
                    && symbol.span.start_column > column
                {
                    break;
                }

                let line_distance = line.saturating_sub(symbol.line);
                let column_distance = if line_distance == 0 {
                    column
                        .map(|value| value.saturating_sub(symbol.span.start_column))
                        .unwrap_or(0)
                } else {
                    0
                };
                candidates.push((
                    line_distance,
                    column_distance,
                    corpus.repository_id.clone(),
                    symbol_path,
                    symbol.line,
                    symbol.span.start_column,
                    symbol.stable_id.clone(),
                ));
            }
        }

        let candidate_repository_ids = candidates
            .iter()
            .map(|candidate| candidate.2.as_str())
            .collect::<BTreeSet<_>>();
        if repository_id_hint.is_none() && candidate_repository_ids.len() > 1 {
            return Err(Self::invalid_params(
                "location path is ambiguous across adopted repositories; pass repository_id",
                Some(json!({
                    "path": raw_path,
                    "line": line,
                    "column": column,
                    "repository_ids": candidate_repository_ids
                        .into_iter()
                        .collect::<Vec<_>>(),
                })),
            ));
        }

        candidates.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(left.3.cmp(&right.3))
                .then(right.4.cmp(&left.4))
                .then(right.5.cmp(&left.5))
                .then(left.6.cmp(&right.6))
        });

        candidates
            .first()
            .map(|candidate| candidate.6.clone())
            .ok_or_else(|| {
                Self::resource_not_found(
                    "symbol not found at location",
                    Some(json!({
                        "path": raw_path,
                        "line": line,
                        "column": column,
                        "repository_id": repository_id_hint,
                    })),
                )
            })
    }

    pub(in crate::mcp::server) fn navigation_symbol_query_token_from_location(
        corpora: &[Arc<RepositorySymbolCorpus>],
        repository_id_hint: Option<&str>,
        raw_path: &str,
        line: usize,
        column: usize,
    ) -> Option<NavigationLocationTokenHint> {
        if repository_id_hint.is_none()
            && corpora
                .iter()
                .filter(|corpus| {
                    let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
                    let absolute_path = corpus.root.join(&requested_path);
                    Self::navigation_path_within_root(&corpus.root, &absolute_path)
                        && absolute_path.is_file()
                })
                .take(2)
                .count()
                > 1
        {
            return None;
        }

        for corpus in corpora {
            if let Some(repository_id_hint) = repository_id_hint
                && corpus.repository_id != repository_id_hint
                && corpus.runtime_repository_id != repository_id_hint
            {
                continue;
            }
            let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
            let absolute_path = corpus.root.join(&requested_path);
            if !Self::navigation_path_within_root(&corpus.root, &absolute_path) {
                continue;
            }
            let language =
                supported_language_for_path(&absolute_path, LanguageCapability::StructuralSearch);
            if language == Some(SymbolLanguage::Rust) {
                let Ok(source) = fs::read_to_string(&absolute_path) else {
                    continue;
                };
                if let Some(rust_hint) =
                    rust_navigation_query_hint_from_source(&absolute_path, &source, line, column)
                    && !rust_hint.symbol_query.is_empty()
                {
                    return Some(NavigationLocationTokenHint {
                        symbol_query: rust_hint.symbol_query.clone(),
                        relative_path: requested_path,
                        resolution_source: "location_token_rust",
                        helper_kind: None,
                        rust_hint: Some(rust_hint),
                    });
                }
                let Some(offset) = byte_offset_for_line_column(&source, line, column) else {
                    continue;
                };
                let Some(token) = Self::identifier_token_around_offset(&source, offset) else {
                    continue;
                };
                if !token.is_empty() {
                    return Some(NavigationLocationTokenHint {
                        symbol_query: token,
                        relative_path: requested_path,
                        resolution_source: "location_token",
                        helper_kind: None,
                        rust_hint: None,
                    });
                }
                continue;
            }

            let Ok(line_source) = Self::read_source_line_for_navigation(&absolute_path, line)
            else {
                continue;
            };
            let Some(offset) = byte_offset_for_line_column(&line_source, 1, column) else {
                continue;
            };
            if matches!(language, Some(SymbolLanguage::Php | SymbolLanguage::Blade))
                && let Some((token, helper_kind)) =
                    Self::php_helper_string_token_around_offset(&line_source, offset)
            {
                return Some(NavigationLocationTokenHint {
                    symbol_query: token,
                    relative_path: requested_path,
                    resolution_source: "location_token_php_helper",
                    helper_kind: Some(helper_kind),
                    rust_hint: None,
                });
            }
            let Some(token) = Self::identifier_token_around_offset(&line_source, offset) else {
                continue;
            };
            if !token.is_empty() {
                return Some(NavigationLocationTokenHint {
                    symbol_query: token,
                    relative_path: requested_path,
                    resolution_source: "location_token",
                    helper_kind: None,
                    rust_hint: None,
                });
            }
        }
        None
    }

    fn read_source_line_for_navigation(path: &Path, line: usize) -> std::io::Result<String> {
        if line == 0 {
            return Ok(String::new());
        }
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut buffer = String::new();
        for current_line in 1..=line {
            buffer.clear();
            let bytes_read = reader.read_line(&mut buffer)?;
            if bytes_read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "line out of range",
                ));
            }
            if current_line == line {
                while buffer.ends_with('\n') || buffer.ends_with('\r') {
                    buffer.pop();
                }
                return Ok(buffer);
            }
        }
        Ok(String::new())
    }

    pub(in crate::mcp::server) fn php_helper_string_token_around_offset(
        source: &str,
        offset: usize,
    ) -> Option<(String, NavigationPhpHelperKind)> {
        let bytes = source.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let mut offset = offset.min(bytes.len().saturating_sub(1));
        while offset > 0 && !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let line_start = source[..offset]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let line_end = source[offset..]
            .find('\n')
            .map(|delta| offset + delta)
            .unwrap_or(source.len());
        if let Some((quote_start, quote_end)) =
            Self::enclosing_simple_quoted_span_in_line(source, line_start, line_end, offset)
            && let Some(helper_match) =
                Self::php_helper_token_for_quote_span(source, line_start, quote_start, quote_end)
        {
            return Some(helper_match);
        }

        Self::php_helper_token_for_helper_call_span(source, line_start, line_end, offset)
    }

    fn php_helper_token_for_quote_span(
        source: &str,
        line_start: usize,
        quote_start: usize,
        quote_end: usize,
    ) -> Option<(String, NavigationPhpHelperKind)> {
        let literal = source[quote_start + 1..quote_end].trim();
        if literal.is_empty() || literal.contains('\n') || literal.contains('\r') {
            return None;
        }

        let prefix = source[line_start..quote_start].trim_end();
        Self::php_helper_prefixes()
            .iter()
            .find_map(|(suffix, kind)| {
                prefix
                    .ends_with(suffix)
                    .then(|| (literal.to_owned(), *kind))
            })
    }

    fn php_helper_token_for_helper_call_span(
        source: &str,
        line_start: usize,
        line_end: usize,
        offset: usize,
    ) -> Option<(String, NavigationPhpHelperKind)> {
        let line = &source[line_start..line_end];
        let bytes = source.as_bytes();
        let mut best_match: Option<(usize, usize, String, NavigationPhpHelperKind)> = None;

        for (suffix, _kind) in Self::php_helper_prefixes() {
            for (match_offset, _) in line.match_indices(suffix) {
                let helper_start = line_start + match_offset;
                let search_start = helper_start + suffix.len();
                let Some((quote_start, quote_end)) =
                    Self::first_simple_quoted_span_in_line(source, search_start, line_end)
                else {
                    continue;
                };
                let Some((literal, helper_kind)) = Self::php_helper_token_for_quote_span(
                    source,
                    helper_start,
                    quote_start,
                    quote_end,
                ) else {
                    continue;
                };
                let mut helper_end = quote_end.saturating_add(1);
                while helper_end < line_end && bytes[helper_end].is_ascii_whitespace() {
                    helper_end += 1;
                }
                if helper_end < line_end && bytes[helper_end] == b')' {
                    helper_end += 1;
                }
                if offset < helper_start || offset >= helper_end {
                    continue;
                }

                let candidate = (
                    helper_end - helper_start,
                    helper_start,
                    literal,
                    helper_kind,
                );
                if best_match
                    .as_ref()
                    .is_none_or(|current| (candidate.0, candidate.1) < (current.0, current.1))
                {
                    best_match = Some(candidate);
                }
            }
        }

        best_match.map(|(_, _, literal, kind)| (literal, kind))
    }

    fn first_simple_quoted_span_in_line(
        source: &str,
        search_start: usize,
        line_end: usize,
    ) -> Option<(usize, usize)> {
        let bytes = source.as_bytes();
        let mut start = search_start;
        while start < line_end {
            let quote = bytes[start];
            if (quote == b'\'' || quote == b'"') && !Self::is_escaped_byte(bytes, start) {
                let mut end = start + 1;
                while end < line_end {
                    if bytes[end] == quote && !Self::is_escaped_byte(bytes, end) {
                        return Some((start, end));
                    }
                    end += 1;
                }
                return None;
            }
            start += 1;
        }
        None
    }

    fn enclosing_simple_quoted_span_in_line(
        source: &str,
        line_start: usize,
        line_end: usize,
        offset: usize,
    ) -> Option<(usize, usize)> {
        let bytes = source.as_bytes();
        let mut best_span: Option<(usize, usize)> = None;
        for start in line_start..line_end {
            let quote = bytes[start];
            if quote != b'\'' && quote != b'"' {
                continue;
            }
            if Self::is_escaped_byte(bytes, start) {
                continue;
            }

            let mut end = start + 1;
            while end < line_end {
                if bytes[end] == quote && !Self::is_escaped_byte(bytes, end) {
                    if offset > start
                        && offset < end
                        && best_span.as_ref().is_none_or(|(best_start, best_end)| {
                            end.saturating_sub(start) < best_end.saturating_sub(*best_start)
                        })
                    {
                        best_span = Some((start, end));
                    }
                    break;
                }
                end += 1;
            }
        }
        best_span
    }

    fn is_escaped_byte(bytes: &[u8], index: usize) -> bool {
        if index == 0 {
            return false;
        }
        let mut backslash_count = 0usize;
        let mut probe = index;
        while probe > 0 {
            probe -= 1;
            if bytes[probe] != b'\\' {
                break;
            }
            backslash_count += 1;
        }
        backslash_count % 2 == 1
    }

    fn identifier_token_around_offset(source: &str, offset: usize) -> Option<String> {
        fn is_identifier_byte(byte: u8) -> bool {
            byte.is_ascii_alphanumeric() || byte == b'_'
        }

        let bytes = source.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let mut index = offset.min(bytes.len().saturating_sub(1));
        if !is_identifier_byte(bytes[index]) {
            if index > 0 && is_identifier_byte(bytes[index - 1]) {
                index -= 1;
            } else {
                let mut probe = index;
                while probe < bytes.len()
                    && !is_identifier_byte(bytes[probe])
                    && bytes[probe] != b'\n'
                {
                    probe += 1;
                }
                if probe >= bytes.len() || !is_identifier_byte(bytes[probe]) {
                    return None;
                }
                index = probe;
            }
        }

        let mut start = index;
        while start > 0 && is_identifier_byte(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = index + 1;
        while end < bytes.len() && is_identifier_byte(bytes[end]) {
            end += 1;
        }
        (start < end).then(|| source[start..end].to_owned())
    }

    pub(in crate::mcp::server) fn resolve_navigation_target(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol: Option<&str>,
        path: Option<&str>,
        line: Option<usize>,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        if let Some(symbol) = symbol {
            let query = symbol.trim();
            if query.is_empty() {
                return Err(Self::invalid_params("symbol must not be empty", None));
            }
            let target = Self::resolve_navigation_symbol_target(
                corpora,
                query,
                repository_id_hint,
                None,
                None,
                true,
                None,
            )?;
            return Ok(ResolvedNavigationTarget {
                symbol_query: query.to_owned(),
                selection: target,
                resolution_source: "symbol",
            });
        }

        let raw_path = path.ok_or_else(|| {
            Self::invalid_params("either `symbol` or (`path` + `line`) is required", None)
        })?;
        if raw_path.trim().is_empty() {
            return Err(Self::invalid_params(
                "path must not be empty when provided",
                None,
            ));
        }
        let line = line
            .ok_or_else(|| Self::invalid_params("line is required when resolving by path", None))?;
        let location_hint = column.and_then(|column| {
            Self::navigation_symbol_query_token_from_location(
                corpora,
                repository_id_hint,
                raw_path,
                line,
                column,
            )
        });
        Self::resolve_navigation_target_from_location_hint(
            corpora,
            raw_path,
            line,
            column,
            repository_id_hint,
            location_hint,
        )
    }

    /// Resolve a legacy symbol request using the same path-class boundary as `search_symbol`.
    /// This keeps target selection and the visible candidate rows in one filtered universe.
    pub(in crate::mcp::server) fn resolve_navigation_symbol_request_with_path_class(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol: &str,
        path_class: SearchSymbolPathClass,
        repository_id_hint: Option<&str>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        let query = symbol.trim();
        if query.is_empty() {
            return Err(Self::invalid_params("symbol must not be empty", None));
        }
        let target = Self::resolve_navigation_symbol_target(
            corpora,
            query,
            repository_id_hint,
            None,
            None,
            true,
            Some(path_class),
        )?;
        Ok(ResolvedNavigationTarget {
            symbol_query: query.to_owned(),
            selection: target,
            resolution_source: "symbol",
        })
    }

    pub(in crate::mcp::server) fn resolve_navigation_target_from_location_hint(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
        location_hint: Option<NavigationLocationTokenHint>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        Self::validate_navigation_location_line_bounds(
            corpora,
            raw_path,
            line,
            repository_id_hint,
        )?;
        if let Some(location_hint) = location_hint
            && let Ok(target) = Self::resolve_navigation_symbol_target(
                corpora,
                &location_hint.symbol_query,
                repository_id_hint,
                Some(location_hint.relative_path.as_str()),
                location_hint.rust_hint.as_ref(),
                false,
                None,
            )
        {
            return Ok(ResolvedNavigationTarget {
                symbol_query: location_hint.symbol_query,
                selection: target,
                resolution_source: location_hint.resolution_source,
            });
        }
        let symbol_query = Self::resolve_navigation_symbol_query_from_location(
            corpora,
            raw_path,
            line,
            column,
            repository_id_hint,
        )?;
        let target = Self::resolve_navigation_symbol_target(
            corpora,
            &symbol_query,
            repository_id_hint,
            None,
            None,
            false,
            None,
        )?;
        Ok(ResolvedNavigationTarget {
            symbol_query,
            selection: target,
            resolution_source: "location_enclosing_symbol",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FriggMcpServer, NavigationPhpHelperKind, RepositorySymbolCorpus};
    use crate::indexer::{SourceSpan, SymbolDefinition, SymbolKind, byte_offset_for_line_column};
    use crate::languages::SymbolLanguage;
    use crate::mcp::server_state::RepositoryDiagnosticsSummary;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn coordinate_corpus(spans: &[(usize, usize, usize, usize)]) -> RepositorySymbolCorpus {
        let symbols = spans
            .iter()
            .enumerate()
            .map(
                |(index, (start_line, start_column, end_line, end_column))| SymbolDefinition {
                    stable_id: format!("symbol-{index}"),
                    language: SymbolLanguage::Rust,
                    kind: SymbolKind::Function,
                    name: format!("symbol_{index}"),
                    path: PathBuf::from("src/lib.rs"),
                    line: *start_line,
                    span: SourceSpan {
                        start_byte: start_line.saturating_mul(1_000) + start_column,
                        end_byte: end_line.saturating_mul(1_000) + end_column,
                        start_line: *start_line,
                        start_column: *start_column,
                        end_line: *end_line,
                        end_column: *end_column,
                    },
                },
            )
            .collect::<Vec<_>>();
        RepositorySymbolCorpus {
            repository_id: "repo".to_owned(),
            runtime_repository_id: "repo".to_owned(),
            root: PathBuf::from("/repo"),
            root_signature: "snapshot".to_owned(),
            source_paths: vec![PathBuf::from("src/lib.rs")],
            container_symbol_index_by_index: vec![None; symbols.len()],
            symbols_by_relative_path: BTreeMap::from([(
                "src/lib.rs".to_owned(),
                (0..symbols.len()).collect(),
            )]),
            symbol_index_by_stable_id: symbols
                .iter()
                .enumerate()
                .map(|(index, symbol)| (symbol.stable_id.clone(), index))
                .collect(),
            symbol_indices_by_name: BTreeMap::new(),
            symbol_indices_by_lower_name: BTreeMap::new(),
            canonical_symbol_name_by_stable_id: BTreeMap::new(),
            symbol_indices_by_canonical_name: BTreeMap::new(),
            symbol_indices_by_lower_canonical_name: BTreeMap::new(),
            rust_symbol_context_by_index: vec![None; symbols.len()],
            rust_implementation_facts: Vec::new(),
            php_evidence_by_relative_path: BTreeMap::new(),
            blade_evidence_by_relative_path: BTreeMap::new(),
            diagnostics: RepositoryDiagnosticsSummary::default(),
            symbols,
        }
    }

    #[test]
    fn coordinate_target_prefers_exact_start_and_rejects_missing_or_tied_anchors() {
        let exact = coordinate_corpus(&[(1, 1, 20, 1), (5, 8, 5, 14)]);
        assert_eq!(
            FriggMcpServer::unique_coordinate_symbol_index(&exact, "src/lib.rs", 5, Some(8),),
            Some(1),
            "an exact symbol start must win over an enclosing symbol"
        );
        assert_eq!(
            FriggMcpServer::unique_coordinate_symbol_index(&exact, "src/lib.rs", 5, Some(10),),
            Some(1),
            "the unique smallest enclosing symbol must be selected"
        );
        assert_eq!(
            FriggMcpServer::unique_coordinate_symbol_index(&exact, "src/lib.rs", 5, Some(14),),
            Some(0),
            "a symbol's exclusive end column must not count as containment"
        );
        assert_eq!(
            FriggMcpServer::unique_coordinate_symbol_index(&exact, "src/lib.rs", 5, None),
            None,
            "a coordinate target without a column is insufficient"
        );

        let tied = coordinate_corpus(&[(5, 2, 5, 12), (5, 4, 5, 14)]);
        assert_eq!(
            FriggMcpServer::unique_coordinate_symbol_index(&tied, "src/lib.rs", 5, Some(8),),
            None,
            "equal-smallest enclosing candidates must not select the first row"
        );
    }

    #[test]
    fn navigation_relative_input_path_collapses_double_slash_segments() {
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path("src//lib.rs"),
            "src/lib.rs"
        );
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path("src///lib.rs"),
            "src/lib.rs"
        );
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path("./src//lib.rs"),
            "src/lib.rs"
        );
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path("crates//cli//src//lib.rs"),
            "crates/cli/src/lib.rs"
        );
    }

    #[test]
    fn navigation_relative_input_path_collapses_internal_parent_segments() {
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path(
                r".\crates\cli\src\..\src\navigation_resolution.rs"
            ),
            "crates/cli/src/navigation_resolution.rs"
        );
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path(
                "./crates/cli/src/../src/navigation_resolution.rs"
            ),
            "crates/cli/src/navigation_resolution.rs"
        );
        assert_eq!(
            FriggMcpServer::normalize_relative_input_path("../frigg/src/lib.rs"),
            "../frigg/src/lib.rs"
        );
    }

    #[test]
    fn relative_display_path_collapses_internal_parent_segments_under_root() {
        let root = Path::new("/workspace/frigg");
        let path = root.join("crates/cli/src/../src/navigation_resolution.rs");

        assert_eq!(
            FriggMcpServer::relative_display_path(root, &path),
            "crates/cli/src/navigation_resolution.rs"
        );
    }

    #[test]
    fn php_helper_string_token_extracts_laravel_translation_literal() {
        let source = "{{ __('Settings') }}\n";
        let offset = byte_offset_for_line_column(source, 1, 10).expect("offset should resolve");
        assert_eq!(
            FriggMcpServer::php_helper_string_token_around_offset(source, offset),
            Some(("Settings".to_owned(), NavigationPhpHelperKind::Translation))
        );
    }

    #[test]
    fn php_helper_string_token_extracts_blade_attribute_route_literal() {
        let source = r#"<x-nav-link href="{{ route('dashboard') }}">Dashboard</x-nav-link>"#;
        let offset = byte_offset_for_line_column(source, 1, 31).expect("offset should resolve");
        assert_eq!(
            FriggMcpServer::php_helper_string_token_around_offset(source, offset),
            Some(("dashboard".to_owned(), NavigationPhpHelperKind::Route))
        );
    }

    #[test]
    fn php_helper_string_token_extracts_route_literal_from_helper_prefix_offset() {
        let source = r#"<x-nav-link href="{{ route('dashboard') }}">Dashboard</x-nav-link>"#;
        let route_column = source.find("route(").expect("helper should exist") + 3;
        let offset =
            byte_offset_for_line_column(source, 1, route_column).expect("offset should resolve");
        assert_eq!(
            FriggMcpServer::php_helper_string_token_around_offset(source, offset),
            Some(("dashboard".to_owned(), NavigationPhpHelperKind::Route))
        );
    }

    #[test]
    fn php_helper_string_token_does_not_panic_on_multibyte_line() {
        let source = "echo café";
        let offset = byte_offset_for_line_column(source, 1, 999).expect("offset should resolve");
        assert_eq!(
            FriggMcpServer::php_helper_string_token_around_offset(source, offset),
            None
        );
    }

    #[test]
    fn php_helper_string_token_handles_column_inside_multibyte_char() {
        let source = "é route('x')";
        for column in 1..=source.chars().count() {
            let offset =
                byte_offset_for_line_column(source, 1, column).expect("offset should resolve");
            let _ = FriggMcpServer::php_helper_string_token_around_offset(source, offset);
        }
    }

    #[cfg(unix)]
    #[test]
    fn navigation_path_within_root_rejects_symlink_escape() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let workspace_root = std::env::temp_dir().join(format!(
            "frigg-nav-path-symlink-{nonce}-{}",
            std::process::id()
        ));
        let repo_root = workspace_root.join("repo");
        let src_root = repo_root.join("src");
        fs::create_dir_all(&src_root).expect("src root");
        let outside = workspace_root.join("outside_secret.rs");
        let inside = src_root.join("safe.rs");
        let leak = src_root.join("leak.rs");
        fs::write(&outside, "fn secret() {}\n").expect("outside");
        fs::write(&inside, "fn safe() {}\n").expect("inside");
        std::os::unix::fs::symlink(&outside, &leak).expect("symlink");

        assert!(FriggMcpServer::navigation_path_within_root(
            &repo_root, &inside
        ));
        assert!(!FriggMcpServer::navigation_path_within_root(
            &repo_root, &leak
        ));

        let _ = fs::remove_dir_all(workspace_root);
    }
}
