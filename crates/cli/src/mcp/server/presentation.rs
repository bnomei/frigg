use super::*;
use crate::domain::model::TextMatch;
use crate::mcp::types::DocumentSymbolItem;

impl FriggMcpServer {
    pub(super) fn response_mode(mode: Option<ResponseMode>) -> ResponseMode {
        mode.unwrap_or(ResponseMode::Compact)
    }

    fn should_return_full_response(mode: Option<ResponseMode>) -> bool {
        matches!(Self::response_mode(mode), ResponseMode::Full)
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

    pub(super) fn session_result_handle_match(
        &self,
        result_handle: &str,
        match_id: &str,
    ) -> Option<crate::mcp::server_cache::ResultHandleMatchAnchor> {
        let now = Instant::now();
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_session_result_handles(&mut cache, now);
        cache
            .entries
            .get(result_handle)?
            .matches
            .get(match_id)
            .cloned()
    }

    fn assign_result_handle_for_text_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [TextMatch],
    ) -> Option<String> {
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
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
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: None,
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
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
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
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
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
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
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
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
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
        let mut stored = BTreeMap::new();
        for (index, found) in matches.iter_mut().enumerate() {
            let match_id = format!("m{}", index + 1);
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
        fn visit(
            symbols: &mut [DocumentSymbolItem],
            next_id: &mut usize,
            stored: &mut BTreeMap<String, crate::mcp::server_cache::ResultHandleMatchAnchor>,
        ) {
            for symbol in symbols {
                let match_id = format!("m{}", *next_id);
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
                visit(&mut symbol.children, next_id, stored);
            }
        }

        let mut stored = BTreeMap::new();
        let mut next_id = 1usize;
        visit(symbols, &mut next_id, &mut stored);
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
        let canonical_path = workspace.root.join(&found.path);
        let snapshot = self.file_content_snapshot_for_workspace(&workspace, &canonical_path)?;
        let line_start = found.line.saturating_sub(context_lines).max(1);
        let line_end = found.line.saturating_add(context_lines);
        let slice = snapshot
            .read_line_slice_lossy(line_start, Some(line_end), self.config.max_file_bytes)
            .map_err(|err| Self::map_lossy_line_slice_error(&canonical_path, err))?;
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

        let per_file_limit = if params.collapse_by_file == Some(true) {
            1usize
        } else {
            params.max_matches_per_file.unwrap_or(usize::MAX)
        };
        if per_file_limit != usize::MAX {
            let mut retained = Vec::with_capacity(response.matches.len());
            let mut counts = BTreeMap::<(String, String), usize>::new();
            for found in response.matches {
                let key = (found.repository_id.clone(), found.path.clone());
                let count = counts.entry(key).or_insert(0);
                if *count >= per_file_limit {
                    continue;
                }
                *count += 1;
                retained.push(found);
                if retained.len() >= requested_limit {
                    break;
                }
            }
            response.matches = retained;
        } else if response.matches.len() > requested_limit {
            response.matches.truncate(requested_limit);
        }

        response.result_handle =
            self.assign_result_handle_for_text_matches("search_text", &mut response.matches);
        if !Self::should_return_full_response(params.response_mode) {
            response.metadata = None;
        }
        Ok(response)
    }

    pub(super) fn present_search_hybrid_response(
        &self,
        mut response: SearchHybridResponse,
        response_mode: Option<ResponseMode>,
    ) -> SearchHybridResponse {
        response.result_handle =
            self.assign_result_handle_for_hybrid_matches("search_hybrid", &mut response.matches);
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_search_symbol_response(
        &self,
        mut response: SearchSymbolResponse,
        response_mode: Option<ResponseMode>,
    ) -> SearchSymbolResponse {
        response.result_handle =
            self.assign_result_handle_for_symbol_matches("search_symbol", &mut response.matches);
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_find_references_response(
        &self,
        mut response: FindReferencesResponse,
        response_mode: Option<ResponseMode>,
    ) -> FindReferencesResponse {
        response.result_handle = self
            .assign_result_handle_for_reference_matches("find_references", &mut response.matches);
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
        if !Self::should_return_full_response(response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }

    pub(super) fn present_document_symbols_response(
        &self,
        mut response: DocumentSymbolsResponse,
        params: &DocumentSymbolsParams,
    ) -> DocumentSymbolsResponse {
        if params.top_level_only == Some(true) {
            for symbol in &mut response.symbols {
                symbol.children.clear();
            }
        }
        response.result_handle = self
            .assign_result_handle_for_document_symbols("document_symbols", &mut response.symbols);
        if !Self::should_return_full_response(params.response_mode) {
            response.metadata = None;
            response.note = None;
        }
        response
    }
}
