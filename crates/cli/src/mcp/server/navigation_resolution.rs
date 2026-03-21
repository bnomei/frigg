use super::*;
use std::fs::File;
use std::io::{BufRead, BufReader};

impl FriggMcpServer {
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
        let normalized = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        normalized.trim_start_matches("./").to_owned()
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

    pub(in crate::mcp::server) fn build_ranked_symbol_match(
        corpus: &RepositorySymbolCorpus,
        symbol_index: usize,
        rank: u8,
        path_class_filter: Option<SearchSymbolPathClass>,
        path_regex: Option<&regex::Regex>,
    ) -> Option<RankedSymbolMatch> {
        let symbol = &corpus.symbols[symbol_index];
        let path = Self::relative_display_path(&corpus.root, &symbol.path);
        if let Some(path_class_filter) = path_class_filter {
            if Self::navigation_path_class(&path) != path_class_filter.as_str() {
                return None;
            }
        }
        if let Some(path_regex) = path_regex {
            if !path_regex.is_match(&path) {
                return None;
            }
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
                stable_symbol_id: Some(symbol.stable_id.clone()),
                repository_id: corpus.repository_id.clone(),
                symbol: symbol.name.clone(),
                kind: symbol.kind.as_str().to_owned(),
                path,
                line: symbol.line,
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

    pub(in crate::mcp::server) fn retain_bounded_ranked_symbol_match(
        ranked_matches: &mut Vec<RankedSymbolMatch>,
        limit: usize,
        candidate: RankedSymbolMatch,
    ) {
        if limit == 0 {
            return;
        }

        ranked_matches.push(candidate);
        Self::sort_ranked_symbol_matches(ranked_matches);
        if ranked_matches.len() > limit {
            ranked_matches.pop();
        }
    }

    fn resolve_navigation_symbol_target(
        corpora: &[Arc<RepositorySymbolCorpus>],
        symbol_query: &str,
        repository_id_hint: Option<&str>,
        location_relative_path: Option<&str>,
        rust_hint: Option<&crate::languages::RustNavigationQueryHint>,
        require_disambiguation: bool,
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
        if require_disambiguation && selected_rank_candidate_count > 1 {
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

    fn normalize_relative_input_path(raw_path: &str) -> String {
        raw_path
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_owned()
    }

    pub(in crate::mcp::server) fn requested_location_path_for_corpus(
        corpus: &RepositorySymbolCorpus,
        raw_path: &str,
    ) -> String {
        let requested_path = PathBuf::from(raw_path);
        if requested_path.is_absolute() {
            Self::relative_display_path(&corpus.root, &requested_path)
        } else {
            Self::normalize_relative_input_path(raw_path)
        }
    }

    fn navigation_symbol_context_ranks(
        corpora: &[Arc<RepositorySymbolCorpus>],
        candidate: &SymbolCandidate,
        location_relative_path: Option<&str>,
        rust_hint: Option<&crate::languages::RustNavigationQueryHint>,
    ) -> (u8, u8, u8, u8, u8) {
        let relative_path = Self::relative_display_path(&candidate.root, &candidate.symbol.path);
        let same_file_rank = rust_hint.map_or(1, |hint| {
            if hint.prefer_same_file && location_relative_path == Some(relative_path.as_str()) {
                0
            } else {
                1
            }
        });
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
                if let Some(column) = column {
                    if symbol.line == line && symbol.span.start_column > column {
                        break;
                    }
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
        raw_path: &str,
        line: usize,
        column: usize,
    ) -> Option<NavigationLocationTokenHint> {
        for corpus in corpora {
            let requested_path = Self::requested_location_path_for_corpus(corpus, raw_path);
            let absolute_path = corpus.root.join(&requested_path);
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
        let offset = offset.min(bytes.len().saturating_sub(1));
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
            Self::navigation_symbol_query_token_from_location(corpora, raw_path, line, column)
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

    pub(in crate::mcp::server) fn resolve_navigation_target_from_location_hint(
        corpora: &[Arc<RepositorySymbolCorpus>],
        raw_path: &str,
        line: usize,
        column: Option<usize>,
        repository_id_hint: Option<&str>,
        location_hint: Option<NavigationLocationTokenHint>,
    ) -> Result<ResolvedNavigationTarget, ErrorData> {
        if let Some(location_hint) = location_hint
            && let Ok(target) = Self::resolve_navigation_symbol_target(
                corpora,
                &location_hint.symbol_query,
                repository_id_hint,
                Some(location_hint.relative_path.as_str()),
                location_hint.rust_hint.as_ref(),
                false,
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
    use super::{FriggMcpServer, NavigationPhpHelperKind};
    use crate::indexer::byte_offset_for_line_column;

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
}
