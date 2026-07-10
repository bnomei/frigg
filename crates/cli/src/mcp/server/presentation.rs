//! Compact versus full response shaping, text-first read surfaces, and session `result_handle` allocation.
//!
//! Shapes compact versus full tool payloads and allocates session-local `result_handle` ids for
//! deferred `read_match` follow-ups.

use super::*;
use crate::domain::model::TextMatch;
use crate::mcp::types::{
    DocumentSymbolItem, HybridPivotMatchSource, SearchHybridDiagnosticsSummary,
    SearchHybridMatch, SearchHybridMetadata, SearchHybridRankReason,
};

/// Outcome of resolving a session `result_handle` + `match_id` pair for `read_match`.
#[derive(Debug, Clone)]
pub(in crate::mcp::server) enum SessionResultHandleLookup {
    Found(crate::mcp::server_cache::ResultHandleMatchAnchor),
    /// `result_handle` is missing (expired, never issued, or invalidated).
    StaleHandle,
    /// Handle exists but `match_id` does not belong to it (often mixed across calls).
    MixedHandle {
        foreign_handle_has_match: bool,
        foreign_handle: Option<String>,
    },
}

impl FriggMcpServer {
    pub(super) fn response_mode(mode: Option<ResponseMode>) -> ResponseMode {
        mode.unwrap_or(ResponseMode::Compact)
    }

    fn should_return_full_response(mode: Option<ResponseMode>) -> bool {
        matches!(Self::response_mode(mode), ResponseMode::Full)
    }

    pub(super) fn hybrid_pivot_match_sources(
        matches: &[SearchHybridMatch],
    ) -> Vec<HybridPivotMatchSource<'_>> {
        matches
            .iter()
            .map(|matched| HybridPivotMatchSource {
                path: matched.path.as_str(),
                excerpt: matched.excerpt.as_str(),
                // Prefer post-guardrail exact/strong rank_reasons when present; otherwise use
                // live lexical sources so pre-guardrail exact-pivot probe selection still
                // boosts strong-lexical rows (rank_reasons are empty until after assist).
                prefers_exact: matched.rank_reasons.iter().any(|reason| {
                    matches!(
                        reason,
                        SearchHybridRankReason::ExactSymbolMatch
                            | SearchHybridRankReason::ExactTextMatch
                            | SearchHybridRankReason::StrongLexicalAnchor
                    )
                }) || Self::search_hybrid_match_has_strong_lexical_anchor(matched),
            })
            .collect()
    }

    fn attach_recovery_index_if_missing(
        recovery: &mut RecoveryFields,
        index: Option<ZeroHitIndex>,
    ) {
        if recovery.index.is_none()
            && let Some(index) = index
        {
            recovery.index = Some(index);
        }
    }

    fn store_session_result_handle(
        &self,
        _tool_name: &'static str,
        matches: BTreeMap<String, crate::mcp::server_cache::ResultHandleMatchAnchor>,
    ) -> Option<String> {
        if matches.is_empty() {
            return None;
        }

        let now = Instant::now();
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_session_result_handles(&mut cache, now);
        cache.next_id = cache.next_id.saturating_add(1);
        let handle = format!("result-{:06}", cache.next_id);
        cache.insertion_order.push_back(handle.clone());
        cache.entries.insert(
            handle.clone(),
            SessionResultHandleEntry {
                generated_at: now,
                matches,
            },
        );
        while cache.entries.len() > Self::SESSION_RESULT_HANDLE_MAX_ENTRIES {
            if let Some(oldest) = cache.insertion_order.pop_front() {
                cache.entries.remove(&oldest);
            } else {
                break;
            }
        }
        Some(handle)
    }

    fn prune_session_result_handles(cache: &mut SessionResultHandleCache, now: Instant) {
        while let Some(oldest) = cache.insertion_order.front().cloned() {
            let Some(entry) = cache.entries.get(&oldest) else {
                cache.insertion_order.pop_front();
                continue;
            };
            if now.duration_since(entry.generated_at) < Self::SESSION_RESULT_HANDLE_TTL {
                break;
            }
            cache.insertion_order.pop_front();
            cache.entries.remove(&oldest);
        }
    }

    #[cfg(test)]
    pub(super) fn session_result_handle_match(
        &self,
        result_handle: &str,
        match_id: &str,
    ) -> Option<crate::mcp::server_cache::ResultHandleMatchAnchor> {
        match self.session_result_handle_lookup(result_handle, match_id) {
            SessionResultHandleLookup::Found(anchor) => Some(anchor),
            SessionResultHandleLookup::StaleHandle
            | SessionResultHandleLookup::MixedHandle { .. } => None,
        }
    }

    /// Classifies `read_match` handle failures as stale (missing handle) vs mixed (wrong match_id).
    pub(super) fn session_result_handle_lookup(
        &self,
        result_handle: &str,
        match_id: &str,
    ) -> SessionResultHandleLookup {
        let now = Instant::now();
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_session_result_handles(&mut cache, now);
        let Some(entry) = cache.entries.get(result_handle) else {
            return SessionResultHandleLookup::StaleHandle;
        };
        if let Some(anchor) = entry.matches.get(match_id).cloned() {
            return SessionResultHandleLookup::Found(anchor);
        }
        let foreign_handle = cache.entries.iter().find_map(|(handle, other)| {
            if handle == result_handle {
                return None;
            }
            other.matches.contains_key(match_id).then(|| handle.clone())
        });
        SessionResultHandleLookup::MixedHandle {
            foreign_handle_has_match: foreign_handle.is_some(),
            foreign_handle,
        }
    }

    pub(super) fn invalidate_session_result_handles_for_repository_ids<'a>(
        &self,
        repository_ids: impl IntoIterator<Item = &'a str>,
    ) {
        let repository_ids = repository_ids.into_iter().collect::<Vec<_>>();
        if repository_ids.is_empty() {
            return;
        }
        self.for_each_session_result_handle_cache(|cache| {
            cache.entries.retain(|_, entry| {
                !entry.matches.values().any(|anchor| {
                    repository_ids
                        .iter()
                        .any(|repository_id| anchor.repository_id == *repository_id)
                })
            });
            let retained_handles = cache.entries.keys().cloned().collect::<BTreeSet<_>>();
            cache
                .insertion_order
                .retain(|handle| retained_handles.contains(handle));
        });
    }

    /// Drop only anchors whose paths are in `dirty_paths` (EXP-handle-inval D).
    ///
    /// Handles with remaining clean-path matches stay valid. Handles that lose all matches
    /// are removed (subsequent `read_match` → StaleHandle). **Empty** `dirty_paths` is a
    /// known-empty set (noop refresh) and skips handle invalidation — use
    /// [`Self::invalidate_session_result_handles_for_repository_ids`] for unknown sets.
    ///
    /// Fans out to every live session handle cache (HTTP multi-session).
    pub(super) fn invalidate_session_result_handles_for_paths(
        &self,
        repository_ids: &[&str],
        dirty_paths: &[String],
    ) {
        if repository_ids.is_empty() || dirty_paths.is_empty() {
            return;
        }
        let dirty: BTreeSet<String> = dirty_paths
            .iter()
            .map(|path| Self::normalize_handle_path(path))
            .filter(|path| !path.is_empty())
            .collect();
        if dirty.is_empty() {
            return;
        }

        self.for_each_session_result_handle_cache(|cache| {
            let mut empty_handles = Vec::new();
            for (handle, entry) in cache.entries.iter_mut() {
                entry.matches.retain(|_, anchor| {
                    let repo_hit = repository_ids
                        .iter()
                        .any(|repository_id| anchor.repository_id == *repository_id);
                    if !repo_hit {
                        return true;
                    }
                    let anchor_path = Self::normalize_handle_path(&anchor.path);
                    !Self::handle_path_is_dirty(&anchor_path, &dirty)
                });
                if entry.matches.is_empty() {
                    empty_handles.push(handle.clone());
                }
            }
            for handle in empty_handles {
                cache.entries.remove(&handle);
                cache.insertion_order.retain(|h| h != &handle);
            }
        });
    }

    /// Normalize paths for handle dirty-matching: slash style, strip `./`, leading `/`,
    /// trailing `/`. Does not treat bare basenames as universal matches.
    fn normalize_handle_path(path: &str) -> String {
        let mut normalized = path.replace('\\', "/");
        while normalized.starts_with("./") {
            normalized = normalized[2..].to_owned();
        }
        normalized = normalized.trim_start_matches('/').to_owned();
        while normalized.ends_with('/') && !normalized.is_empty() {
            normalized.pop();
        }
        normalized
    }

    /// True when the anchor path is the dirty path, under a dirty directory, or when an
    /// absolute dirty path ends with `/{repo-relative-anchor}` (absolute must have a `/`).
    fn handle_path_is_dirty(anchor_path: &str, dirty: &BTreeSet<String>) -> bool {
        if dirty.contains(anchor_path) {
            return true;
        }
        dirty.iter().any(|dirty_path| {
            if dirty_path == anchor_path {
                return true;
            }
            // Directory dirty path invalidates all anchors under it.
            if !dirty_path.is_empty() && anchor_path.starts_with(&format!("{dirty_path}/")) {
                return true;
            }
            // Absolute dirty path → match repo-relative anchor as a full suffix path.
            // Require a `/` in dirty so bare basenames never over-match nested files.
            dirty_path.contains('/')
                && dirty_path.starts_with('/')
                && dirty_path.ends_with(&format!("/{anchor_path}"))
        })
    }

    /// Apply `f` to every live session's result-handle cache (prunes dead Weak entries).
    fn for_each_session_result_handle_cache(
        &self,
        mut f: impl FnMut(&mut SessionResultHandleCache),
    ) {
        let mut registry = self
            .runtime_state
            .session_result_handle_caches
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.retain(|weak| {
            if let Some(cache) = weak.upgrade() {
                let mut guard = cache
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                f(&mut guard);
                true
            } else {
                false
            }
        });
    }

    /// Drop one session handle entry (used when a composer discards nested tool handles).
    pub(super) fn drop_session_result_handle(&self, result_handle: &str) {
        if result_handle.is_empty() {
            return;
        }
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.entries.remove(result_handle);
        cache
            .insertion_order
            .retain(|handle| handle != result_handle);
    }

    /// Maps tool names to short `match_id` scope prefixes (`search:m1`, `nav:m1`, …).
    pub(super) fn result_handle_scope_for_tool(tool_name: &str) -> &'static str {
        match tool_name {
            "search_text" => "search",
            "search_symbol" | "document_symbols" => "symbols",
            "search_hybrid" => "hybrid",
            "search_batch" => "batch",
            "find_references"
            | "go_to_definition"
            | "find_declarations"
            | "find_implementations"
            | "incoming_calls"
            | "outgoing_calls" => "nav",
            _ => "search",
        }
    }

    fn scoped_match_id(scope: &str, index: usize) -> String {
        format!("{scope}:m{}", index + 1)
    }

    fn attach_handle_metadata(
        result_handle: &Option<String>,
        tool_name: &str,
    ) -> (Option<String>, Option<String>) {
        if result_handle.is_some() {
            (
                Some(Self::result_handle_scope_for_tool(tool_name).to_owned()),
                Some("session".to_owned()),
            )
        } else {
            (None, None)
        }
    }

    fn assign_result_handle_for_text_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [TextMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: Some(found.column),
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_symbol_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [SymbolMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: found.column,
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    pub(super) fn assign_result_handle_for_batch_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [crate::mcp::types::SearchBatchMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: found.column,
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_hybrid_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [SearchHybridMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: Some(found.column),
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_reference_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [ReferenceMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: Some(found.column),
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_navigation_locations(
        &self,
        tool_name: &'static str,
        matches: &mut [NavigationLocation],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: Some(found.column),
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_implementation_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [ImplementationMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: Some(found.column),
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_call_hierarchy_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [CallHierarchyMatch],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: Some(found.column),
                },
            );
            found.match_id = Some(match_id);
        }
        self.store_session_result_handle(tool_name, stored)
    }

    fn assign_result_handle_for_document_symbols(
        &self,
        tool_name: &'static str,
        symbols: &mut [DocumentSymbolItem],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        fn visit(
            scope: &str,
            symbols: &mut [DocumentSymbolItem],
            next_id: &mut usize,
            stored: &mut BTreeMap<String, crate::mcp::server_cache::ResultHandleMatchAnchor>,
        ) {
            for symbol in symbols {
                let match_id = format!("{scope}:m{}", *next_id);
                *next_id = next_id.saturating_add(1);
                stored.insert(
                    match_id.clone(),
                    crate::mcp::server_cache::ResultHandleMatchAnchor {
                        repository_id: symbol.repository_id.clone(),
                        path: symbol.path.clone(),
                        line: symbol.line,
                        column: Some(symbol.column),
                    },
                );
                symbol.match_id = Some(match_id);
                visit(scope, &mut symbol.children, next_id, stored);
            }
        }

        let mut stored = BTreeMap::new();
        let mut next_id = 1usize;
        visit(scope, symbols, &mut next_id, &mut stored);
        self.store_session_result_handle(tool_name, stored)
    }

    fn search_text_requested_limit(&self, params: &SearchTextParams) -> usize {
        params
            .limit
            .unwrap_or(self.config.max_search_results)
            .min(self.config.max_search_results.max(1))
    }

    fn expand_text_match_excerpt(
        &self,
        found: &mut TextMatch,
        context_lines: usize,
    ) -> Result<(), ErrorData> {
        let workspace = self
            .attached_workspaces_for_repository(Some(found.repository_id.as_str()))?
            .into_iter()
            .find(|workspace| workspace.repository_id == found.repository_id)
            .ok_or_else(|| {
                Self::resource_not_found(
                    "repository_id not found",
                    Some(json!({ "repository_id": found.repository_id })),
                )
            })?;
        let read_params = ReadFileParams {
            path: found.path.clone(),
            repository_id: Some(found.repository_id.clone()),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        };
        let (_, canonical_path, _) = self.resolve_file_path(&read_params)?;
        let snapshot = self.file_content_snapshot_for_workspace(&workspace, &canonical_path)?;
        let line_start = found.line.saturating_sub(context_lines).max(1);
        let line_end = found.line.saturating_add(context_lines);
        let slice = snapshot
            .read_line_slice_lossy(line_start, Some(line_end), self.config.max_file_bytes)
            .map_err(|err| Self::map_lossy_line_slice_error(&canonical_path, err))?;
        if slice.bytes > self.config.max_file_bytes {
            return Err(Self::invalid_params(
                format!(
                    "selected line range exceeds max_file_bytes={}",
                    self.config.max_file_bytes
                ),
                Some(json!({
                    "path": found.path.clone(),
                    "bytes": slice.bytes,
                    "max_bytes": self.config.max_file_bytes,
                    "start_line": line_start,
                    "end_line": line_end,
                    "total_lines": slice.total_lines,
                })),
            ));
        }
        found.excerpt = slice.content;
        Ok(())
    }

    pub(super) fn present_search_text_response(
        &self,
        mut response: SearchTextResponse,
        params: &SearchTextParams,
    ) -> Result<SearchTextResponse, ErrorData> {
        let requested_limit = self.search_text_requested_limit(params);
        let context_lines = params.context_lines.unwrap_or(0).min(MAX_CONTEXT_LINES);
        if context_lines > 0 {
            for found in &mut response.matches {
                self.expand_text_match_excerpt(found, context_lines)?;
            }
        }

        if params.count_only == Some(true) {
            response.matches.clear();
            response.result_handle = None;
            response.handle_scope = None;
            response.handle_expires = None;
            response.count_only = Some(true);
            if response.latency_class.is_none() {
                response.latency_class = Some(crate::mcp::types::LatencyClass::Hot);
            }
            if !Self::should_return_full_response(params.response_mode) {
                response.metadata = None;
            }
            return Ok(response);
        }

        let per_file_limit =
            if params.files_with_matches == Some(true) || params.collapse_by_file == Some(true) {
                1usize
            } else {
                params.max_count_per_file.unwrap_or(usize::MAX)
            };
        if requested_limit == 0 {
            response.matches.clear();
            response.total_matches = 0;
        } else if per_file_limit != usize::MAX {
            let mut retained = Vec::with_capacity(response.matches.len());
            let mut counts = BTreeMap::<(String, String), usize>::new();
            for found in response.matches {
                if retained.len() >= requested_limit {
                    break;
                }
                let key = (found.repository_id.clone(), found.path.clone());
                let count = counts.entry(key).or_insert(0);
                if *count >= per_file_limit {
                    continue;
                }
                *count += 1;
                retained.push(found);
            }
            response.matches = retained;
        } else if response.matches.len() > requested_limit {
            response.matches.truncate(requested_limit);
        }

        response.result_handle =
            self.assign_result_handle_for_text_matches("search_text", &mut response.matches);
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "search_text");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.latency_class.is_none() {
            response.latency_class = Some(Self::search_text_latency_class(
                params,
                response.total_matches,
            ));
        }
        if response.total_matches == 0 {
            let repository_ids = params.repository_id.clone().into_iter().collect::<Vec<_>>();
            let index = self.zero_hit_index_for_repositories(&repository_ids);
            if response.recovery.is_empty() {
                let pattern_type_is_literal =
                    !matches!(params.pattern_type, Some(SearchPatternType::Regex));
                let mut scope = ZeroHitScope::default();
                if let Some(path_regex) = params.path_regex.as_ref() {
                    scope = scope.with_path_regex(path_regex.clone());
                }
                if let Some(glob) = params.glob.as_ref() {
                    scope = scope.with_glob(glob.clone());
                }
                if let Some(repository_id) = params.repository_id.as_ref() {
                    scope = scope.with_repository_id(repository_id.clone());
                }
                response.recovery = RecoveryFields::for_zero_hit(ZeroHitInput {
                    tool: "search_text",
                    query: Some(params.query.as_str()),
                    pattern_type_is_literal: Some(pattern_type_is_literal),
                    scope: Some(scope).filter(|scope| !scope.is_empty()),
                    index,
                    reason_override: None,
                });
                if let Some(glob) = params.glob.as_deref() {
                    response.recovery = response
                        .recovery
                        .with_non_recursive_glob_hint(params.query.as_str(), glob);
                }
                crate::mcp::routing_stats::record_zero_hit();
            } else {
                // Live search paths may have composed recovery before index was available.
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        } else if response.recovery.scope.is_none() {
            // Scope echo on non-empty hits when filters were applied.
            let mut scope = ZeroHitScope::default();
            if let Some(path_regex) = params.path_regex.as_ref() {
                scope = scope.with_path_regex(path_regex.clone());
            }
            if let Some(glob) = params.glob.as_ref() {
                scope = scope.with_glob(glob.clone());
            }
            if let Some(repository_id) = params.repository_id.as_ref() {
                scope = scope.with_repository_id(repository_id.clone());
            }
            if !scope.is_empty() {
                response.recovery.scope = Some(scope);
            }
        }
        if !Self::should_return_full_response(params.response_mode) {
            response.metadata = None;
        }
        Ok(response)
    }

    fn search_text_latency_class(
        params: &SearchTextParams,
        total_matches: usize,
    ) -> crate::mcp::types::LatencyClass {
        use crate::mcp::types::LatencyClass;
        if params.count_only == Some(true) || params.files_with_matches == Some(true) {
            return LatencyClass::Hot;
        }
        let scoped =
            params.path_regex.is_some() || params.glob.is_some() || params.repository_id.is_some();
        if scoped && total_matches < 50 {
            LatencyClass::Hot
        } else if scoped {
            LatencyClass::Warm
        } else {
            LatencyClass::Cold
        }
    }

    pub(super) fn present_search_hybrid_response(
        &self,
        mut response: SearchHybridResponse,
        response_mode: Option<ResponseMode>,
        query: Option<&str>,
    ) -> SearchHybridResponse {
        response.result_handle =
            self.assign_result_handle_for_hybrid_matches("search_hybrid", &mut response.matches);
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "search_hybrid");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.latency_class.is_none() {
            // Hybrid is discovery-oriented and allowed slower than exact search.
            response.latency_class = Some(if response.matches.is_empty() {
                crate::mcp::types::LatencyClass::Warm
            } else {
                crate::mcp::types::LatencyClass::Cold
            });
        }
        // Always-on discovery contract (compact + full). EXP-semantic-default A:
        // product default is semantic-off; compact strips readiness dumps but must
        // not hide lexical-only mode. Encode the cliff in short ranking_note only —
        // do not re-open long metadata.warning / semantic_status in compact.
        let lexical_only = response
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lexical_only_mode)
            .unwrap_or(false);
        response.ranking_note = Some(if lexical_only {
            "discovery_only; lexical_only (semantic not contributing); confirm with exact search"
                .to_owned()
        } else {
            "discovery_only; confirm with exact search".to_owned()
        });
        if response.best_pivot_path.is_none() {
            response.best_pivot_path = response
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.utility.as_ref())
                .and_then(|utility| utility.best_pivot_path.clone())
                .or_else(|| {
                    response
                        .matches
                        .iter()
                        .find(|matched| {
                            matched
                                .navigation_hint
                                .as_ref()
                                .is_some_and(|hint| hint.pivotable)
                        })
                        .or_else(|| response.matches.first())
                        .map(|matched| matched.path.clone())
                });
        }
        if response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = RecoveryFields::for_zero_hit(ZeroHitInput {
                    tool: "search_hybrid",
                    query,
                    pattern_type_is_literal: Some(true),
                    scope: None,
                    index,
                    reason_override: None,
                });
                crate::mcp::routing_stats::record_zero_hit();
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        } else if !response.matches.is_empty() && response.recovery.suggested_next.is_empty() {
            let pivot_sources = Self::hybrid_pivot_match_sources(&response.matches);
            response.recovery = RecoveryFields::hybrid_discovery_exact_pivot(
                query.unwrap_or(""),
                response.best_pivot_path.as_deref(),
                &pivot_sources,
            );
            crate::mcp::routing_stats::record_recovery_issued();
        }
        if !Self::should_return_full_response(response_mode) {
            // Agent compact must not dump semantic readiness telemetry (status, hit
            // counts, long warnings). Mode cliff is already in ranking_note above.
            // Full mode keeps warnings and channel health for operators/debug.
            // Compact keeps metadata only when context_efficiency was explicitly requested.
            response.metadata = response.metadata.and_then(|mut metadata| {
                let context_efficiency = metadata.context_efficiency.take();
                if context_efficiency.is_none() {
                    return None;
                }
                Some(SearchHybridMetadata {
                    channels: BTreeMap::new(),
                    lexical_backend: None,
                    lexical_backend_note: None,
                    semantic_requested: None,
                    semantic_enabled: None,
                    semantic_status: None,
                    semantic_reason: None,
                    semantic_candidate_count: None,
                    semantic_hit_count: None,
                    semantic_match_count: None,
                    lexical_only_mode: None,
                    query_shape: None,
                    warning: None,
                    exact_pivot_assistance: None,
                    witness_demotion_applied: None,
                    diagnostics_count: 0,
                    diagnostics: SearchHybridDiagnosticsSummary {
                        walk: 0,
                        read: 0,
                        total: 0,
                    },
                    stage_attribution: None,
                    semantic_capability: None,
                    utility: None,
                    context_efficiency,
                    cache_debug: None,
                })
            });
        }
        response
    }

    pub(super) fn present_search_symbol_response(
        &self,
        mut response: SearchSymbolResponse,
        response_mode: Option<ResponseMode>,
        params: Option<&SearchSymbolParams>,
    ) -> SearchSymbolResponse {
        response.result_handle =
            self.assign_result_handle_for_symbol_matches("search_symbol", &mut response.matches);
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "search_symbol");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.latency_class.is_none() {
            // Runtime-first known-name lookup is the hot symbol path.
            let scoped = params.is_some_and(|params| {
                params.path_regex.is_some()
                    || params.repository_id.is_some()
                    || params
                        .path_class
                        .is_none_or(|class| class != SearchSymbolPathClass::Any)
            });
            response.latency_class = Some(if scoped {
                crate::mcp::types::LatencyClass::Hot
            } else {
                crate::mcp::types::LatencyClass::Warm
            });
        }
        if response.matches.is_empty() {
            let repository_ids = params
                .and_then(|params| params.repository_id.clone())
                .into_iter()
                .collect::<Vec<_>>();
            let index = self.zero_hit_index_for_repositories(&repository_ids);
            if response.recovery.is_empty() {
                let query = params.map(|params| params.query.as_str());
                let mut scope = ZeroHitScope::default();
                let effective_path_class = params
                    .and_then(|params| params.path_class)
                    .unwrap_or(SearchSymbolPathClass::Runtime);
                if let Some(params) = params {
                    if let Some(path_regex) = params.path_regex.as_ref() {
                        scope = scope.with_path_regex(path_regex.clone());
                    }
                    scope = scope.with_path_class(effective_path_class.as_str());
                    if let Some(repository_id) = params.repository_id.as_ref() {
                        scope = scope.with_repository_id(repository_id.clone());
                    }
                } else {
                    scope = scope.with_path_class(effective_path_class.as_str());
                }
                // Runtime-first empty recovery for known names.
                let scope = Some(scope).filter(|scope| !scope.is_empty());
                response.recovery = if effective_path_class == SearchSymbolPathClass::Runtime {
                    if let Some(name) = query {
                        RecoveryFields::runtime_zero_name_known(name)
                            .with_diagnostics(ZeroHitDiagnostics { scope, index })
                    } else {
                        RecoveryFields::for_zero_hit(ZeroHitInput {
                            tool: "search_symbol",
                            query,
                            pattern_type_is_literal: None,
                            scope,
                            index,
                            reason_override: None,
                        })
                    }
                } else {
                    RecoveryFields::for_zero_hit(ZeroHitInput {
                        tool: "search_symbol",
                        query,
                        pattern_type_is_literal: None,
                        scope,
                        index,
                        reason_override: None,
                    })
                };
                crate::mcp::routing_stats::record_zero_hit();
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    /// Prefer disambiguation recovery over SCIP-missing when target_selection says so.
    fn navigation_zero_hit_recovery(
        tool: &'static str,
        mode: NavigationMode,
        target_selection: Option<&crate::mcp::types::NavigationTargetSelectionSummary>,
        index: Option<ZeroHitIndex>,
    ) -> RecoveryFields {
        let query = target_selection.map(|selection| selection.symbol_query.as_str());
        let disambiguation_required = target_selection.is_some_and(|selection| {
            selection.status
                == crate::mcp::types::NavigationTargetSelectionStatus::DisambiguationRequired
        });
        if disambiguation_required {
            let mut recovery = RecoveryFields::disambiguation_required(query);
            Self::attach_recovery_index_if_missing(&mut recovery, index);
            return recovery;
        }
        let reason_override = matches!(mode, NavigationMode::UnavailableNoPrecise)
            .then_some(ZeroHitReason::PreciseGraphUnavailable);
        RecoveryFields::for_zero_hit(ZeroHitInput {
            tool,
            query,
            pattern_type_is_literal: None,
            scope: None,
            index,
            reason_override,
        })
    }

    pub(super) fn present_find_references_response(
        &self,
        mut response: FindReferencesResponse,
        response_mode: Option<ResponseMode>,
    ) -> FindReferencesResponse {
        response.result_handle = self
            .assign_result_handle_for_reference_matches("find_references", &mut response.matches);
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "find_references");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.total_matches == 0 || response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = Self::navigation_zero_hit_recovery(
                    "find_references",
                    response.mode,
                    response.target_selection.as_ref(),
                    index,
                );
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_go_to_definition_response(
        &self,
        mut response: GoToDefinitionResponse,
        response_mode: Option<ResponseMode>,
    ) -> GoToDefinitionResponse {
        response.result_handle = self.assign_result_handle_for_navigation_locations(
            "go_to_definition",
            &mut response.matches,
        );
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "go_to_definition");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = Self::navigation_zero_hit_recovery(
                    "go_to_definition",
                    response.mode,
                    response.target_selection.as_ref(),
                    index,
                );
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_find_declarations_response(
        &self,
        mut response: FindDeclarationsResponse,
        response_mode: Option<ResponseMode>,
    ) -> FindDeclarationsResponse {
        response.result_handle = self.assign_result_handle_for_navigation_locations(
            "find_declarations",
            &mut response.matches,
        );
        if response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = Self::navigation_zero_hit_recovery(
                    "find_declarations",
                    response.mode,
                    response.target_selection.as_ref(),
                    index,
                );
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_find_implementations_response(
        &self,
        mut response: FindImplementationsResponse,
        response_mode: Option<ResponseMode>,
    ) -> FindImplementationsResponse {
        response.result_handle = self.assign_result_handle_for_implementation_matches(
            "find_implementations",
            &mut response.matches,
        );
        if response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = Self::navigation_zero_hit_recovery(
                    "find_implementations",
                    response.mode,
                    response.target_selection.as_ref(),
                    index,
                );
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_incoming_calls_response(
        &self,
        mut response: IncomingCallsResponse,
        response_mode: Option<ResponseMode>,
    ) -> IncomingCallsResponse {
        response.result_handle = self.assign_result_handle_for_call_hierarchy_matches(
            "incoming_calls",
            &mut response.matches,
        );
        if response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = Self::navigation_zero_hit_recovery(
                    "incoming_calls",
                    response.mode,
                    response.target_selection.as_ref(),
                    index,
                );
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_outgoing_calls_response(
        &self,
        mut response: OutgoingCallsResponse,
        response_mode: Option<ResponseMode>,
    ) -> OutgoingCallsResponse {
        response.result_handle = self.assign_result_handle_for_call_hierarchy_matches(
            "outgoing_calls",
            &mut response.matches,
        );
        // EXP-nav-outgoing-honesty B: always-on machine honesty (not stripped in compact).
        response = response.with_provisional_honesty();
        if response.matches.is_empty() {
            let index = self.zero_hit_index_for_repositories(&[]);
            if response.recovery.is_empty() {
                response.recovery = Self::navigation_zero_hit_recovery(
                    "outgoing_calls",
                    response.mode,
                    response.target_selection.as_ref(),
                    index,
                );
            } else {
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            // Full-mode diagnostic `note` only; keep always-on `trust` / `trust_note`.
            response.note = None;
        }
        response
    }

    pub(super) fn present_document_symbols_response(
        &self,
        mut response: DocumentSymbolsResponse,
        params: &DocumentSymbolsParams,
    ) -> DocumentSymbolsResponse {
        const DEFAULT_DOCUMENT_SYMBOLS_LIMIT: usize = 200;
        const MAX_DOCUMENT_SYMBOLS_LIMIT: usize = 1000;

        // Default top_level_only=true ( outline).
        let top_level_only = params.top_level_only.unwrap_or(true);
        response.top_level_only = top_level_only;
        if top_level_only {
            for symbol in &mut response.symbols {
                symbol.children.clear();
            }
        }

        let total_symbols = response.symbols.len();
        response.total_symbols = total_symbols;
        let resume_offset = params.resume_from.unwrap_or(0);
        let limit = params
            .limit
            .unwrap_or(DEFAULT_DOCUMENT_SYMBOLS_LIMIT)
            .clamp(1, MAX_DOCUMENT_SYMBOLS_LIMIT);
        let page = if resume_offset >= total_symbols {
            Vec::new()
        } else {
            response
                .symbols
                .into_iter()
                .skip(resume_offset)
                .take(limit.saturating_add(1))
                .collect::<Vec<_>>()
        };
        let truncated = page.len() > limit;
        response.symbols = page.into_iter().take(limit).collect();
        response.returned = response.symbols.len();
        response.truncated = truncated;
        response.resume_from = truncated.then_some(resume_offset.saturating_add(response.returned));

        response.result_handle = self
            .assign_result_handle_for_document_symbols("document_symbols", &mut response.symbols);
        if !Self::should_return_full_response(params.response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::types::SearchPatternType;
    use crate::settings::FriggConfig;
    use rmcp::model::ErrorCode;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "frigg-presentation-{test_name}-{nonce}-{}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    #[test]
    fn search_text_excerpt_rejects_symlink_escape_hit() {
        let workspace_root = temp_workspace_root("search-text-symlink-escape");
        let repo_root = workspace_root.join("repo");
        let src_root = repo_root.join("src");
        fs::create_dir_all(repo_root.join(".git")).expect("repo git marker should be creatable");
        fs::create_dir_all(&src_root).expect("fixture src root should be creatable");
        fs::write(src_root.join("lib.rs"), "pub fn safe() {}\n")
            .expect("safe source fixture should be writable");
        let outside_path = workspace_root.join("outside_secret.rs");
        fs::write(&outside_path, "pub fn secret_token() {}\n")
            .expect("outside source fixture should be writable");
        std::os::unix::fs::symlink(&outside_path, src_root.join("leak.rs"))
            .expect("symlink fixture should be creatable");

        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![repo_root.clone()])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        server
            .adopt_workspace(&workspace, true)
            .expect("server should adopt fixture workspace");

        let error = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 1,
                    matches: vec![TextMatch {
                        match_id: None,
                        repository_id: workspace.repository_id.clone(),
                        path: "src/leak.rs".to_owned(),
                        line: 1,
                        column: 8,
                        excerpt: "secret_token".to_owned(),
                        witness_score_hint_millis: None,
                        witness_provenance_ids: None,
                    }],
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "secret_token".to_owned(),
                    pattern_type: Some(SearchPatternType::Literal),
                    repository_id: Some(workspace.repository_id),
                    context_lines: Some(2),
                    limit: Some(5),
                    ..Default::default()
                },
            )
            .expect_err("search_text excerpt expansion should reject symlink escapes");

        assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
        assert!(
            error.message.contains("outside workspace roots"),
            "unexpected symlink escape error message: {}",
            error.message
        );

        let _ = fs::remove_dir_all(workspace_root);
    }

    fn presentation_test_server() -> FriggMcpServer {
        let workspace_root = temp_workspace_root("limit-zero-presentation");
        fs::create_dir_all(workspace_root.join(".git"))
            .expect("fixture git marker should be creatable");
        FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root])
                .expect("workspace root must produce valid config"),
        )
    }

    fn sample_text_match(repository_id: &str, path: &str) -> TextMatch {
        TextMatch {
            match_id: None,
            repository_id: repository_id.to_owned(),
            path: path.to_owned(),
            line: 1,
            column: 1,
            excerpt: "needle".to_owned(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        }
    }

    #[test]
    fn search_text_limit_zero_with_collapse_by_file_returns_no_matches() {
        let server = presentation_test_server();
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 2,
                    matches: vec![
                        sample_text_match("repo-001", "src/a.rs"),
                        sample_text_match("repo-001", "src/b.rs"),
                    ],
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "needle".to_owned(),
                    limit: Some(0),
                    collapse_by_file: Some(true),
                    ..Default::default()
                },
            )
            .expect("limit-zero collapse_by_file shaping should succeed");

        assert_eq!(response.matches.len(), 0);
        assert_eq!(response.total_matches, 0);
        assert!(response.result_handle.is_none());
    }

    #[test]
    fn search_text_limit_zero_with_files_with_matches_returns_no_matches() {
        let server = presentation_test_server();
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 1,
                    matches: vec![sample_text_match("repo-001", "src/a.rs")],
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "needle".to_owned(),
                    limit: Some(0),
                    files_with_matches: Some(true),
                    ..Default::default()
                },
            )
            .expect("limit-zero files_with_matches shaping should succeed");

        assert_eq!(response.matches.len(), 0);
        assert_eq!(response.total_matches, 0);
        assert!(response.result_handle.is_none());
    }

    #[test]
    fn search_text_zero_hit_serializes_recovery_at_top_level() {
        let server = presentation_test_server();
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 0,
                    matches: Vec::new(),
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "zzznomatch_unique_token".to_owned(),
                    path_regex: Some("^src/".to_owned()),
                    ..Default::default()
                },
            )
            .expect("zero-hit search_text should shape");

        assert!(!response.recovery.is_empty());
        assert!(response.recovery.zero_hit_reason.is_some());
        let value = serde_json::to_value(&response).expect("serialize");
        assert!(value.get("zero_hit_reason").is_some());
        assert!(value.get("message").is_some());
        assert!(value.get("correction_hint").is_some());
        assert!(value.get("suggested_next").is_some());
        assert_eq!(value["scope"]["path_regex"], "^src/");
        // Index block is filled from workspace storage when a repo is known.
        assert!(
            value.get("index").is_some(),
            "zero-hit should serialize index block when workspace signals exist: {value}"
        );
        assert!(value["index"].get("index_state").is_some());
        assert!(
            value.get("recovery").is_none(),
            "recovery must be flattened"
        );
    }

    #[test]
    fn search_text_zero_hit_index_includes_dirty_changed_paths() {
        let server = presentation_test_server();
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("startup workspace");
        server.test_record_gate_dirty_paths(
            &workspace.repository_id,
            &[String::from("src/lib.rs")],
            &[],
        );

        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 0,
                    matches: Vec::new(),
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "zzznomatch_dirty_index_token".to_owned(),
                    repository_id: Some(workspace.repository_id.clone()),
                    ..Default::default()
                },
            )
            .expect("zero-hit search_text with dirty paths should shape");

        let index = response.recovery.index.as_ref().expect("index block");
        assert_eq!(index.working_tree_dirty, Some(true));
        assert!(
            index
                .changed_paths_since_snapshot
                .iter()
                .any(|path| path == "src/lib.rs"),
            "changed paths should surface on zero-hit index: {:?}",
            index.changed_paths_since_snapshot
        );
        assert!(index.stale_warning.is_some());
        assert_eq!(
            response.recovery.zero_hit_reason,
            Some(ZeroHitReason::IndexStalePossible)
        );

        let value = serde_json::to_value(&response).expect("serialize");
        assert_eq!(value["index"]["working_tree_dirty"], true);
        assert!(
            value["index"]["changed_paths_since_snapshot"]
                .as_array()
                .is_some_and(|paths| paths.iter().any(|path| path == "src/lib.rs"))
        );
    }

    #[test]
    fn scoped_match_ids_and_handle_metadata_are_assigned() {
        let server = presentation_test_server();
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 1,
                    matches: vec![sample_text_match("repo-001", "src/a.rs")],
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "needle".to_owned(),
                    limit: Some(5),
                    ..Default::default()
                },
            )
            .expect("search_text with matches should shape");

        assert_eq!(response.matches[0].match_id.as_deref(), Some("search:m1"));
        assert_eq!(response.handle_scope.as_deref(), Some("search"));
        assert_eq!(response.handle_expires.as_deref(), Some("session"));
        let handle = response.result_handle.expect("handle");

        assert!(matches!(
            server.session_result_handle_lookup(&handle, "search:m1"),
            SessionResultHandleLookup::Found(_)
        ));
        assert!(matches!(
            server.session_result_handle_lookup("result-missing", "search:m1"),
            SessionResultHandleLookup::StaleHandle
        ));
        assert!(matches!(
            server.session_result_handle_lookup(&handle, "nav:m9"),
            SessionResultHandleLookup::MixedHandle { .. }
        ));
    }

    #[test]
    fn hybrid_compact_ranking_note_signals_lexical_only_without_metadata_dump() {
        use crate::mcp::types::{ResponseMode, SearchHybridMetadata};

        let server = presentation_test_server();
        let with_lexical_only = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                ranking_note: Some("discovery_only; confirm with exact search".to_owned()),
                best_pivot_path: None,
                latency_class: None,
                metadata: Some(SearchHybridMetadata {
                    channels: BTreeMap::new(),
                    lexical_backend: None,
                    lexical_backend_note: None,
                    semantic_requested: Some(false),
                    semantic_enabled: Some(false),
                    semantic_status: None,
                    semantic_reason: None,
                    semantic_candidate_count: None,
                    semantic_hit_count: None,
                    semantic_match_count: None,
                    lexical_only_mode: Some(true),
                    query_shape: None,
                    warning: Some(
                        "semantic retrieval is disabled; results are ranked from lexical and graph signals only"
                            .to_owned(),
                    ),
                    exact_pivot_assistance: None,
                    witness_demotion_applied: None,
                    diagnostics_count: 0,
                    diagnostics: crate::mcp::types::SearchHybridDiagnosticsSummary {
                        walk: 0,
                        read: 0,
                        total: 0,
                    },
                    stage_attribution: None,
                    semantic_capability: None,
                    utility: None,
                    context_efficiency: None,
                    cache_debug: None,
                }),
                recovery: RecoveryFields::default(),
            },
            Some(ResponseMode::Compact),
            Some("where is catalog"),
        );
        assert_eq!(
            with_lexical_only.ranking_note.as_deref(),
            Some(
                "discovery_only; lexical_only (semantic not contributing); confirm with exact search"
            ),
            "compact must surface lexical-only mode via ranking_note"
        );
        assert!(
            with_lexical_only.metadata.is_none(),
            "compact must still strip readiness metadata dump: {:?}",
            with_lexical_only.metadata
        );

        let multi_channel = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                ranking_note: None,
                best_pivot_path: None,
                latency_class: None,
                metadata: Some(SearchHybridMetadata {
                    channels: BTreeMap::new(),
                    lexical_backend: None,
                    lexical_backend_note: None,
                    semantic_requested: Some(true),
                    semantic_enabled: Some(true),
                    semantic_status: None,
                    semantic_reason: None,
                    semantic_candidate_count: None,
                    semantic_hit_count: Some(3),
                    semantic_match_count: Some(2),
                    lexical_only_mode: Some(false),
                    query_shape: None,
                    warning: None,
                    exact_pivot_assistance: None,
                    witness_demotion_applied: None,
                    diagnostics_count: 0,
                    diagnostics: crate::mcp::types::SearchHybridDiagnosticsSummary {
                        walk: 0,
                        read: 0,
                        total: 0,
                    },
                    stage_attribution: None,
                    semantic_capability: None,
                    utility: None,
                    context_efficiency: None,
                    cache_debug: None,
                }),
                recovery: RecoveryFields::default(),
            },
            Some(ResponseMode::Compact),
            Some("where is catalog"),
        );
        assert_eq!(
            multi_channel.ranking_note.as_deref(),
            Some("discovery_only; confirm with exact search"),
            "when semantic contributes, ranking_note stays the short discovery form"
        );
    }

    #[test]
    fn hybrid_and_symbol_zero_hit_include_recovery() {
        let server = presentation_test_server();
        let hybrid = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                ranking_note: None,
                best_pivot_path: None,
                latency_class: None,
                metadata: None,
                recovery: RecoveryFields::default(),
            },
            None,
            Some("where is catalog"),
        );
        assert!(!hybrid.recovery.is_empty());
        assert_eq!(
            hybrid.ranking_note.as_deref(),
            Some("discovery_only; confirm with exact search"),
            "no metadata → no lexical_only signal"
        );
        let hybrid_value = serde_json::to_value(&hybrid).expect("serialize hybrid");
        assert!(hybrid_value.get("zero_hit_reason").is_some());
        assert!(
            hybrid_value.get("index").is_some(),
            "hybrid zero-hit should include index block: {hybrid_value}"
        );

        let symbol = server.present_search_symbol_response(
            SearchSymbolResponse {
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                latency_class: None,
                metadata: None,
                note: None,
                recovery: RecoveryFields::default(),
            },
            None,
            Some(&SearchSymbolParams {
                query: "MissingSymbol".to_owned(),
                path_class: Some(SearchSymbolPathClass::Runtime),
                ..Default::default()
            }),
        );
        assert!(!symbol.recovery.is_empty());
        let symbol_value = serde_json::to_value(&symbol).expect("serialize symbol");
        assert_eq!(symbol_value["scope"]["path_class"], "runtime");
        assert!(symbol_value.get("suggested_next").is_some());
        assert!(
            symbol_value.get("index").is_some(),
            "symbol zero-hit should include index block: {symbol_value}"
        );
    }

    #[test]
    fn find_references_and_go_to_definition_zero_hit_include_recovery() {
        let server = presentation_test_server();
        let refs = server.present_find_references_response(
            FindReferencesResponse {
                total_matches: 0,
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                mode: NavigationMode::UnavailableNoPrecise,
                target_selection: None,
                metadata: None,
                note: None,
                recovery: RecoveryFields::default(),
            },
            None,
        );
        assert_eq!(
            refs.recovery.zero_hit_reason,
            Some(ZeroHitReason::PreciseGraphUnavailable)
        );
        let refs_value = serde_json::to_value(&refs).expect("serialize refs");
        assert!(refs_value.get("zero_hit_reason").is_some());
        assert!(refs_value.get("correction_hint").is_some());

        let defs = server.present_go_to_definition_response(
            GoToDefinitionResponse {
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                mode: NavigationMode::HeuristicNoPrecise,
                target_selection: None,
                metadata: None,
                note: None,
                location_warning: None,
                ambiguous_location: None,
                recovery: RecoveryFields::default(),
            },
            None,
        );
        assert!(!defs.recovery.is_empty());
        let defs_value = serde_json::to_value(&defs).expect("serialize defs");
        assert!(defs_value.get("zero_hit_reason").is_some());
    }

    #[test]
    fn drop_session_result_handle_removes_entry() {
        let server = presentation_test_server();
        let mut matches = vec![crate::domain::model::TextMatch {
            match_id: None,
            repository_id: "repo".to_owned(),
            path: "src/lib.rs".to_owned(),
            line: 1,
            column: 1,
            excerpt: "fn x() {}".to_owned(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        }];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle should be stored");
        assert!(matches!(
            server.session_result_handle_lookup(
                &handle,
                matches[0].match_id.as_deref().expect("match id")
            ),
            SessionResultHandleLookup::Found(_)
        ));
        server.drop_session_result_handle(&handle);
        assert!(matches!(
            server.session_result_handle_lookup(
                &handle,
                matches[0].match_id.as_deref().expect("match id")
            ),
            SessionResultHandleLookup::StaleHandle
        ));
    }

    /// EXP-handle-inval D: dirty-path anchors drop; clean-path anchors on the same
    /// handle (or other handles) remain valid for `read_match`.
    #[test]
    fn path_scoped_handle_invalidation_drops_only_dirty_anchors() {
        let server = presentation_test_server();
        let mut dirty_matches = vec![sample_text_match("repo-001", "src/dirty.rs")];
        let mut clean_matches = vec![sample_text_match("repo-001", "src/clean.rs")];
        let mut mixed_matches = vec![
            sample_text_match("repo-001", "src/dirty.rs"),
            sample_text_match("repo-001", "src/other.rs"),
        ];
        let dirty_handle = server
            .assign_result_handle_for_text_matches("search_text", &mut dirty_matches)
            .expect("dirty handle");
        let clean_handle = server
            .assign_result_handle_for_text_matches("search_text", &mut clean_matches)
            .expect("clean handle");
        let mixed_handle = server
            .assign_result_handle_for_text_matches("search_text", &mut mixed_matches)
            .expect("mixed handle");
        let dirty_mid = dirty_matches[0].match_id.clone().expect("dirty match_id");
        let clean_mid = clean_matches[0].match_id.clone().expect("clean match_id");
        let mixed_dirty_mid = mixed_matches[0].match_id.clone().expect("mixed dirty match_id");
        let mixed_clean_mid = mixed_matches[1].match_id.clone().expect("mixed clean match_id");

        server.invalidate_session_result_handles_for_paths(
            &["repo-001"],
            &["src/dirty.rs".to_owned()],
        );

        assert!(
            matches!(
                server.session_result_handle_lookup(&dirty_handle, &dirty_mid),
                SessionResultHandleLookup::StaleHandle
            ),
            "handle with only dirty anchors must be removed"
        );
        assert!(
            matches!(
                server.session_result_handle_lookup(&clean_handle, &clean_mid),
                SessionResultHandleLookup::Found(_)
            ),
            "untouched-path handle must survive path-scoped invalidation"
        );
        assert!(
            matches!(
                server.session_result_handle_lookup(&mixed_handle, &mixed_clean_mid),
                SessionResultHandleLookup::Found(_)
            ),
            "clean anchors on a multi-path handle must survive"
        );
        // Dirty match removed from the handle; handle still exists → MixedHandle
        // (not Found). Agents should re-search rather than re-use the old match_id.
        assert!(
            matches!(
                server.session_result_handle_lookup(&mixed_handle, &mixed_dirty_mid),
                SessionResultHandleLookup::MixedHandle { .. }
            ),
            "dirty match_id on a surviving handle must not resolve"
        );
    }

    #[test]
    fn empty_dirty_paths_skips_handle_invalidation() {
        let server = presentation_test_server();
        let mut matches = vec![sample_text_match("repo-001", "src/a.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        // Known-empty dirty set (noop refresh) must not whole-wipe handles.
        server.invalidate_session_result_handles_for_paths(&["repo-001"], &[]);

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::Found(_)
        ));
    }

    #[test]
    fn whole_repo_invalidation_drops_all_repo_handles() {
        let server = presentation_test_server();
        let mut matches = vec![sample_text_match("repo-001", "src/a.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        server.invalidate_session_result_handles_for_repository_ids(["repo-001"]);

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::StaleHandle
        ));
    }

    #[test]
    fn directory_dirty_path_invalidates_nested_anchors() {
        let server = presentation_test_server();
        let mut matches = vec![sample_text_match("repo-001", "src/nested/lib.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        server.invalidate_session_result_handles_for_paths(
            &["repo-001"],
            &["src".to_owned()],
        );

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::StaleHandle
        ));
    }

    #[test]
    fn basename_dirty_does_not_overmatch_nested_paths() {
        let server = presentation_test_server();
        let mut matches = vec![sample_text_match("repo-001", "src/nested/foo.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        // Bare basename must not treat every */foo.rs as dirty.
        server.invalidate_session_result_handles_for_paths(
            &["repo-001"],
            &["foo.rs".to_owned()],
        );

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::Found(_)
        ));
    }

    #[test]
    fn path_scoped_invalidation_does_not_touch_other_repositories() {
        let server = presentation_test_server();
        let mut repo_a = vec![sample_text_match("repo-a", "src/lib.rs")];
        let mut repo_b = vec![sample_text_match("repo-b", "src/lib.rs")];
        let handle_a = server
            .assign_result_handle_for_text_matches("search_text", &mut repo_a)
            .expect("handle a");
        let handle_b = server
            .assign_result_handle_for_text_matches("search_text", &mut repo_b)
            .expect("handle b");
        let mid_a = repo_a[0].match_id.clone().expect("mid a");
        let mid_b = repo_b[0].match_id.clone().expect("mid b");

        server.invalidate_session_result_handles_for_paths(
            &["repo-a"],
            &["src/lib.rs".to_owned()],
        );

        assert!(matches!(
            server.session_result_handle_lookup(&handle_a, &mid_a),
            SessionResultHandleLookup::StaleHandle
        ));
        assert!(matches!(
            server.session_result_handle_lookup(&handle_b, &mid_b),
            SessionResultHandleLookup::Found(_)
        ));
    }

    #[test]
    fn find_references_disambiguation_does_not_claim_precise_graph_unavailable() {
        let server = presentation_test_server();
        let refs = server.present_find_references_response(
            FindReferencesResponse {
                total_matches: 0,
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                mode: NavigationMode::UnavailableNoPrecise,
                target_selection: Some(NavigationTargetSelectionSummary {
                    status: NavigationTargetSelectionStatus::DisambiguationRequired,
                    symbol_query: "Handler".to_owned(),
                    selected_stable_symbol_id: None,
                    candidate_count: 2,
                    same_rank_candidate_count: 2,
                    ambiguous_query: true,
                    candidates: Vec::new(),
                }),
                metadata: None,
                note: None,
                recovery: RecoveryFields::default(),
            },
            None,
        );
        assert_eq!(
            refs.recovery.error_code.as_deref(),
            Some("DISAMBIGUATION_REQUIRED")
        );
        assert_ne!(
            refs.recovery.zero_hit_reason,
            Some(ZeroHitReason::PreciseGraphUnavailable),
            "disambiguation must not be presented as SCIP absence: {:?}",
            refs.recovery
        );
    }

    #[test]
    fn go_to_definition_disambiguation_does_not_claim_precise_graph_unavailable() {
        let server = presentation_test_server();
        let defs = server.present_go_to_definition_response(
            GoToDefinitionResponse {
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                mode: NavigationMode::UnavailableNoPrecise,
                target_selection: Some(NavigationTargetSelectionSummary {
                    status: NavigationTargetSelectionStatus::DisambiguationRequired,
                    symbol_query: "Handler".to_owned(),
                    selected_stable_symbol_id: None,
                    candidate_count: 2,
                    same_rank_candidate_count: 2,
                    ambiguous_query: true,
                    candidates: Vec::new(),
                }),
                metadata: None,
                note: None,
                location_warning: None,
                ambiguous_location: None,
                recovery: RecoveryFields::default(),
            },
            None,
        );
        assert_eq!(
            defs.recovery.error_code.as_deref(),
            Some("DISAMBIGUATION_REQUIRED")
        );
        assert_ne!(
            defs.recovery.zero_hit_reason,
            Some(ZeroHitReason::PreciseGraphUnavailable)
        );
        assert!(
            defs.recovery
                .correction_hint
                .as_ref()
                .is_some_and(|hint| !hint.to_ascii_lowercase().contains("scip")
                    || hint.contains("not a missing SCIP")),
            "disambiguation must not be presented as SCIP absence: {:?}",
            defs.recovery.correction_hint
        );
    }
}
