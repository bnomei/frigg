//! Compact versus full response shaping, text-first read surfaces, and session `result_handle` allocation.
//!
//! Shapes compact versus full tool payloads and allocates session-local `result_handle` ids for
//! deferred `read_match` follow-ups.

use super::*;
use crate::domain::model::TextMatch;
use crate::mcp::types::{DocumentSymbolItem, SearchHybridDiagnosticsSummary, SearchHybridMetadata};

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

    pub(super) fn invalidate_session_result_handles_for_repository_ids<'a>(
        &self,
        repository_ids: impl IntoIterator<Item = &'a str>,
    ) {
        let repository_ids = repository_ids.into_iter().collect::<Vec<_>>();
        if repository_ids.is_empty() {
            return;
        }
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            response.metadata = response.metadata.and_then(|mut metadata| {
                let lexical_only_mode = metadata.lexical_only_mode;
                let warning = metadata.warning.take();
                let context_efficiency = metadata.context_efficiency.take();
                if lexical_only_mode != Some(true)
                    && warning.is_none()
                    && context_efficiency.is_none()
                {
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
                    lexical_only_mode,
                    query_shape: None,
                    warning,
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
}
