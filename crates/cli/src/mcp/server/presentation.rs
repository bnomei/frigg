//! Compact versus full response shaping, text-first read surfaces, and session `result_handle` allocation.
//!
//! Shapes compact versus full tool payloads and allocates session-local `result_handle` ids for
//! deferred `read_match` follow-ups.

use super::*;
use crate::domain::model::TextMatch;
use crate::mcp::server_cache::{ContinuationBinding, SessionContinuationCache};
use crate::mcp::types::{
    ContinuationValidationError, DocumentSymbolItem, FindReferencesParams, GoToDefinitionParams,
    HybridPivotMatchSource, ImpactBundleParams, NextActionDependency, NextActionDependencyMode,
    NextActionId, NextActionOrigin, NextActionRole, NextActionTarget, ReadMatchParams,
    ReplayOriginTarget, ResultCompleteness, ResultIncompleteReason, ResultTruncationReason,
    ResultUnit, SearchHybridDiagnosticsSummary, SearchHybridMatch, SearchHybridMetadata,
    SearchHybridParams, SearchHybridRankReason, canonical_next_action,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Outcome of resolving a session `result_handle` + `match_id` pair for `read_match`.
#[derive(Debug, Clone)]
pub(in crate::mcp::server) enum SessionResultHandleLookup {
    Found(crate::mcp::server_cache::ResultHandleMatchAnchor),
    /// Target scope belongs to a different MCP session; this is checked before cache lookup.
    #[allow(dead_code)]
    TargetScopeMismatch,
    /// `result_handle` is missing (expired, never issued, or invalidated).
    StaleHandle,
    /// Handle exists but `match_id` does not belong to it (often mixed across calls).
    MixedHandle {
        foreign_handle_has_match: bool,
        foreign_handle: Option<String>,
    },
}

impl FriggMcpServer {
    /// Follow-up navigation actions must preserve the opaque target issued with the result row.
    /// They intentionally carry no reconstructed symbol or coordinate fields: replaying these
    /// params routes through the shared target resolver without falling back to name ranking.
    pub(super) fn target_follow_up_actions(
        target: Option<&crate::mcp::types::TargetRef>,
    ) -> Vec<crate::mcp::types::NextAction> {
        let Some(target) = target.cloned() else {
            return Vec::new();
        };
        vec![
            canonical_next_action(
                "target-definition",
                NextActionRole::ResolveTarget,
                0,
                NextActionTarget::GoToDefinition(GoToDefinitionParams {
                    target: Some(target.clone()),
                    ..GoToDefinitionParams::default()
                }),
                "resolve this issued result target to its definition",
            ),
            canonical_next_action(
                "target-references",
                NextActionRole::ResolveTarget,
                1,
                NextActionTarget::FindReferences(FindReferencesParams {
                    target: Some(target.clone()),
                    ..FindReferencesParams::default()
                }),
                "find references for this issued result target",
            ),
            canonical_next_action(
                "target-impact",
                NextActionRole::ResolveTarget,
                2,
                NextActionTarget::ImpactBundle(ImpactBundleParams {
                    target: Some(target),
                    ..ImpactBundleParams::default()
                }),
                "inspect the impact bundle for this issued result target",
            ),
        ]
    }

    pub(super) fn target_is_semantically_resolvable(
        &self,
        target: &crate::mcp::types::TargetRef,
    ) -> bool {
        let Ok(repository_id) = self.navigation_target_repository_hint(Some(target), None) else {
            return false;
        };
        let Ok(corpora) = self.collect_repository_symbol_corpora(repository_id.as_deref()) else {
            return false;
        };
        self.resolve_target_ref(&corpora, target).is_ok()
    }

    fn append_target_follow_up_actions<'a>(
        &self,
        recovery: &mut RecoveryFields,
        targets: impl IntoIterator<Item = &'a crate::mcp::types::TargetRef>,
    ) {
        let target = targets
            .into_iter()
            .find(|target| self.target_is_semantically_resolvable(target));
        let mut actions = std::mem::take(&mut recovery.next_actions);
        let next_order = actions
            .iter()
            .map(|action| action.order)
            .max()
            .map_or(0, |order| order.saturating_add(1));
        let mut target_actions = Self::target_follow_up_actions(target);
        for (offset, action) in target_actions.iter_mut().enumerate() {
            action.order = next_order.saturating_add(offset as u16);
        }
        actions.extend(target_actions);
        recovery.set_next_actions(actions);
    }

    /// Navigation continuations always need a positive page width. Returning a token from an
    /// empty explicit page would retain the same offset and create an infinite replay loop.
    pub(super) fn navigation_page_limit(
        &self,
        requested: Option<usize>,
    ) -> Result<usize, ErrorData> {
        if requested == Some(0) {
            return Err(Self::invalid_params(
                "limit must be at least 1 for pageable navigation results",
                Some(serde_json::json!({ "limit": 0 })),
            ));
        }
        Ok(requested
            .unwrap_or(self.config.max_search_results)
            .min(self.config.max_search_results.max(1)))
    }

    /// Stable v2 binding identity for navigation requests. The opaque continuation itself is
    /// deliberately excluded: replay validates the repeated, behavior-affecting request.
    pub(super) fn navigation_continuation_digest<T: serde::Serialize>(
        tool: &str,
        params: &T,
        repository_ids: &[String],
    ) -> String {
        let mut request = serde_json::to_value(params)
            .expect("navigation parameter DTOs must serialize for continuation binding");
        if let Some(object) = request.as_object_mut() {
            object.remove("continuation");
        }
        let normalized = serde_json::json!({
            "tool": tool,
            "request": request,
            "repository_ids": repository_ids,
        });
        let mut hasher = DefaultHasher::new();
        normalized.to_string().hash(&mut hasher);
        format!("navigation-v2:{:016x}", hasher.finish())
    }

    /// Resolve the page offset and replay binding for navigation surfaces. Navigation recomputes
    /// rows rather than retaining them, so the binding deliberately carries only identity and
    /// offset. This is also used for branches whose target resolution discovers the final scope
    /// late (route helpers, direct precise fallback, and disambiguation).
    pub(super) fn navigation_continuation_context<T: serde::Serialize>(
        &self,
        tool: &'static str,
        params: &T,
        repository_hint: Option<String>,
        unit: ResultUnit,
    ) -> Result<(usize, Option<ContinuationBinding>), ErrorData> {
        let freshness = self
            .scoped_read_only_tool_execution_context(
                tool,
                repository_hint.clone(),
                RepositoryResponseCacheFreshnessMode::ManifestOnly,
            )?
            .cache_freshness;
        let repository_ids = self
            .collect_repository_symbol_corpora(repository_hint.as_deref())?
            .iter()
            .map(|corpus| corpus.repository_id.clone())
            .collect::<Vec<_>>();
        let snapshot_fingerprints = freshness
            .scopes
            .as_ref()
            .map(|scopes| {
                scopes
                    .iter()
                    .map(|scope| format!("{}:{}", scope.repository_id, scope.snapshot_id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let request_digest = Self::navigation_continuation_digest(tool, params, &repository_ids);
        let request = serde_json::to_value(params)
            .expect("navigation parameter DTOs must serialize for continuation lookup");
        let continuation = request
            .get("continuation")
            .and_then(serde_json::Value::as_str);
        let offset = match continuation {
            Some(token) => self
                .session_continuation_lookup(
                    token,
                    tool,
                    &request_digest,
                    &repository_ids,
                    &snapshot_fingerprints,
                    unit,
                )
                .map(|binding| binding.next_position)
                .map_err(|error| {
                    Self::invalid_params(
                        error.message.clone(),
                        Some(serde_json::json!({ "continuation": error })),
                    )
                })?,
            None => 0,
        };
        let binding = (!snapshot_fingerprints.is_empty()).then_some(ContinuationBinding {
            tool,
            request_digest,
            repository_ids,
            snapshot_fingerprints,
            unit,
            next_position: 0,
        });
        Ok((offset, binding))
    }

    /// Turn a recomputed, over-fetched active-mode result into one page. The cache binding owns
    /// only the next offset; rows are always recomputed from the current snapshot.
    pub(super) fn paginate_navigation_rows<T>(
        &self,
        rows: &mut Vec<T>,
        completeness: &mut ResultCompleteness,
        mode: NavigationMode,
        page_limit: usize,
        resume_offset: usize,
        binding: Option<ContinuationBinding>,
    ) {
        let available = rows.len();
        if resume_offset >= available {
            rows.clear();
        } else if resume_offset > 0 {
            rows.drain(..resume_offset);
        }
        if rows.len() > page_limit {
            rows.truncate(page_limit);
        }
        let total = completeness.total;
        let omitted = total.is_some_and(|total| resume_offset.saturating_add(rows.len()) < total);
        let continuation = omitted
            .then(|| {
                binding.map(|mut binding| {
                    binding.next_position = resume_offset.saturating_add(rows.len());
                    self.store_session_continuation(binding)
                })
            })
            .flatten();
        *completeness = Self::navigation_completeness(
            completeness.unit,
            rows.len(),
            total,
            mode,
            omitted,
            continuation,
        );
    }

    /// Build the shared navigation envelope without allowing an exhausted row page to imply
    /// semantic precision. `total` is the active-mode pre-page cardinality when known.
    pub(super) fn navigation_completeness(
        unit: ResultUnit,
        returned: usize,
        total: Option<usize>,
        mode: NavigationMode,
        truncated: bool,
        continuation: Option<String>,
    ) -> ResultCompleteness {
        let incomplete_reason = match mode {
            NavigationMode::Precise => None,
            NavigationMode::PrecisePartial => {
                Some(ResultIncompleteReason::NavigationPartialCoverage)
            }
            NavigationMode::HeuristicNoPrecise => {
                Some(ResultIncompleteReason::NavigationHeuristicCoverage)
            }
            NavigationMode::UnavailableNoPrecise => {
                Some(ResultIncompleteReason::NavigationUnavailable)
            }
        };
        ResultCompleteness::try_new(
            unit,
            returned,
            total,
            mode == NavigationMode::Precise && !truncated,
            truncated,
            truncated
                .then_some(ResultTruncationReason::PageLimit)
                .into_iter()
                .collect(),
            incomplete_reason.into_iter().collect(),
            continuation,
        )
        .expect("navigation completeness arguments must be internally consistent")
    }

    pub(super) fn response_mode(mode: Option<ResponseMode>) -> ResponseMode {
        mode.unwrap_or(ResponseMode::Compact)
    }

    fn should_return_full_response(mode: Option<ResponseMode>) -> bool {
        matches!(Self::response_mode(mode), ResponseMode::Full)
    }

    /// Hybrid pivot rows for recovery. Exact preference uses post-guardrail `rank_reasons` when
    /// present, else strong lexical sources (production probe runs before rank_reasons are filled).
    pub(super) fn hybrid_pivot_match_sources(
        matches: &[SearchHybridMatch],
    ) -> Vec<HybridPivotMatchSource<'_>> {
        matches
            .iter()
            .map(|matched| HybridPivotMatchSource {
                path: matched.path.as_str(),
                excerpt: matched.excerpt.as_str(),
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
                origin_tool: _tool_name,
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

    /// Allocate an opaque, metadata-only v2 continuation in this MCP session.
    #[allow(dead_code)] // public-to-crate lifecycle hook consumed by subsequent surface tasks.
    pub(crate) fn store_session_continuation(&self, binding: ContinuationBinding) -> String {
        let now = Instant::now();
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_session_continuations(&mut cache.continuations, now);
        cache.continuations.next_id = cache.continuations.next_id.saturating_add(1);
        let session_prefix = self.session_state.display_session_id();
        let token = format!(
            "continuation-{}-{:06}",
            session_prefix, cache.continuations.next_id
        );
        cache.continuations.insertion_order.push_back(token.clone());
        cache.continuations.entries.insert(
            token.clone(),
            crate::mcp::server_cache::SessionContinuationEntry {
                generated_at: now,
                binding,
            },
        );
        while cache.continuations.entries.len() > Self::SESSION_CONTINUATION_MAX_ENTRIES {
            if let Some(oldest) = cache.continuations.insertion_order.pop_front() {
                cache.continuations.entries.remove(&oldest);
            } else {
                break;
            }
        }
        token
    }

    /// Resolve a continuation only when all behavior-affecting request and snapshot bindings
    /// match. The returned entry carries just a next position, never retained result rows.
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // public-to-crate lifecycle hook consumed by subsequent surface tasks.
    pub(crate) fn session_continuation_lookup(
        &self,
        token: &str,
        tool: &str,
        request_digest: &str,
        repository_ids: &[String],
        snapshot_fingerprints: &[String],
        unit: ResultUnit,
    ) -> Result<ContinuationBinding, ContinuationValidationError> {
        let now = Instant::now();
        let session_prefix = self.session_state.display_session_id();
        let expected_prefix = format!("continuation-{session_prefix}-");
        if !token.starts_with(&expected_prefix) {
            return Err(ContinuationValidationError::scope_mismatch());
        }
        let mut cache = self
            .session_state
            .inner
            .result_handles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_session_continuations(&mut cache.continuations, now);
        let Some(entry) = cache.continuations.entries.get(token) else {
            return Err(ContinuationValidationError::stale());
        };
        let binding = &entry.binding;
        if binding.tool != tool
            || binding.request_digest != request_digest
            || binding.repository_ids != repository_ids
            || binding.unit != unit
        {
            return Err(ContinuationValidationError::scope_mismatch());
        }
        if binding.snapshot_fingerprints != snapshot_fingerprints {
            return Err(ContinuationValidationError::stale());
        }
        Ok(binding.clone())
    }

    #[allow(dead_code)] // reached through the continuation hooks above.
    fn prune_session_continuations(cache: &mut SessionContinuationCache, now: Instant) {
        while let Some(oldest) = cache.insertion_order.front().cloned() {
            let Some(entry) = cache.entries.get(&oldest) else {
                cache.insertion_order.pop_front();
                continue;
            };
            if now.duration_since(entry.generated_at) < Self::SESSION_CONTINUATION_TTL {
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
            | SessionResultHandleLookup::TargetScopeMismatch
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

    /// Resolve a public result target, rejecting foreign session scopes before consulting the
    /// handle cache. The legacy lookup remains available for `read_match` compatibility.
    #[allow(dead_code)]
    pub(super) fn session_result_target_lookup(
        &self,
        target: &crate::mcp::types::TargetRef,
    ) -> SessionResultHandleLookup {
        let crate::mcp::types::TargetRef::ResultMatch {
            result_handle,
            match_id,
            target_scope,
        } = target
        else {
            return SessionResultHandleLookup::TargetScopeMismatch;
        };
        if target_scope != &self.session_state.display_session_id() {
            return SessionResultHandleLookup::TargetScopeMismatch;
        }
        self.session_result_handle_lookup(result_handle, match_id)
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
        self.invalidate_session_continuations_for_repository_ids(repository_ids);
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
        // Continuation entries intentionally do not retain source paths. A dirty path can alter
        // cardinality/order anywhere in a deterministic result set, so invalidate the affected
        // repository conservatively instead of risking a cross-snapshot page.
        self.invalidate_session_continuations_for_repository_ids(repository_ids.iter().copied());
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
    /// Match dirty paths against anchors: exact, directory prefix, or absolute suffix.
    /// Bare basenames never match nested `*/basename` paths.
    fn handle_path_is_dirty(anchor_path: &str, dirty: &BTreeSet<String>) -> bool {
        if dirty.contains(anchor_path) {
            return true;
        }
        dirty.iter().any(|dirty_path| {
            if dirty_path == anchor_path {
                return true;
            }
            if !dirty_path.is_empty() && anchor_path.starts_with(&format!("{dirty_path}/")) {
                return true;
            }
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

    fn invalidate_session_continuations_for_repository_ids<'a>(
        &self,
        repository_ids: impl IntoIterator<Item = &'a str>,
    ) {
        let repository_ids = repository_ids.into_iter().collect::<Vec<_>>();
        if repository_ids.is_empty() {
            return;
        }
        let mut registry = self
            .runtime_state
            .session_result_handle_caches
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.retain(|weak| {
            if let Some(cache) = weak.upgrade() {
                let mut cache = cache
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                cache.continuations.entries.retain(|_, entry| {
                    !entry.binding.repository_ids.iter().any(|repository_id| {
                        repository_ids
                            .iter()
                            .any(|invalidated| repository_id == invalidated)
                    })
                });
                let retained = cache
                    .continuations
                    .entries
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                cache
                    .continuations
                    .insertion_order
                    .retain(|token| retained.contains(token));
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

    /// Opaque per-session correlation scope used by additive result targets.
    pub(super) fn result_target_scope(&self) -> String {
        self.session_state.display_session_id()
    }

    pub(super) fn result_target(
        &self,
        result_handle: String,
        match_id: String,
    ) -> Option<crate::mcp::types::TargetRef> {
        crate::mcp::types::TargetRef::result_match(
            result_handle,
            match_id,
            self.result_target_scope(),
        )
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

    /// Resolve and snapshot each distinct producer source once before assigning any match ids.
    /// Failed containment, authorization, bounded-read, or digest work leaves that discovery row
    /// visible but deliberately unhandleable.
    fn bind_result_handle_source_snapshots(
        &self,
        pending: impl IntoIterator<Item = (String, String)>,
    ) -> BTreeMap<(String, String), Arc<crate::mcp::server_cache::ResultHandleSourceSnapshot>> {
        let mut snapshots = BTreeMap::new();
        for (repository_id, path) in pending.into_iter().collect::<BTreeSet<_>>() {
            let read_params = ReadFileParams {
                path: path.clone(),
                repository_id: Some(repository_id.clone()),
                max_bytes: None,
                start_line: None,
                end_line: None,
                line_count: None,
                presentation_mode: None,
                include_context_efficiency: None,
            };
            let Ok((resolved_repository_id, canonical_path, _)) =
                self.resolve_file_path(&read_params)
            else {
                continue;
            };
            if resolved_repository_id != repository_id {
                continue;
            }
            let Ok(workspaces) = self.attached_workspaces_for_repository(Some(&repository_id))
            else {
                continue;
            };
            let Some(workspace) = workspaces
                .into_iter()
                .find(|workspace| workspace.repository_id == repository_id)
            else {
                continue;
            };
            let Ok(snapshot) =
                self.file_content_snapshot_for_workspace(&workspace, &canonical_path)
            else {
                continue;
            };
            snapshots.insert(
                (repository_id.clone(), path.clone()),
                Arc::new(crate::mcp::server_cache::ResultHandleSourceSnapshot {
                    repository_id,
                    path,
                    revision: snapshot.source_revision(),
                }),
            );
        }
        snapshots
    }

    /// The only result-handle assignment path. It binds sources before exposing match ids so
    /// every tool family shares identical containment and bounded-snapshot behavior.
    fn assign_bound_result_handle<T>(
        &self,
        tool_name: &'static str,
        matches: &mut [T],
        source: impl Fn(&T) -> (String, String, usize, Option<usize>, Option<String>),
        assign_match_id: impl Fn(&mut T, Option<String>, Option<crate::mcp::types::TargetRef>),
    ) -> Option<String> {
        let snapshots = self.bind_result_handle_source_snapshots(matches.iter().map(|found| {
            let (repository_id, path, _, _, _) = source(found);
            (repository_id, path)
        }));
        let scope = Self::result_handle_scope_for_tool(tool_name);
        let mut stored = BTreeMap::new();
        let mut assigned = Vec::with_capacity(matches.len());
        for (index, found) in matches.iter_mut().enumerate() {
            let (repository_id, path, line, column, stable_symbol_id) = source(found);
            let Some(snapshot) = snapshots.get(&(repository_id, path)).cloned() else {
                assign_match_id(found, None, None);
                assigned.push(None);
                continue;
            };
            let match_id = Self::scoped_match_id(scope, index);
            stored.insert(
                match_id.clone(),
                crate::mcp::server_cache::ResultHandleMatchAnchor {
                    source: snapshot,
                    line,
                    column,
                    stable_symbol_id,
                },
            );
            assigned.push(Some(match_id.clone()));
            assign_match_id(found, Some(match_id), None);
        }
        let handle = self.store_session_result_handle(tool_name, stored);
        for (found, match_id) in matches.iter_mut().zip(assigned) {
            let target = handle
                .clone()
                .and_then(|handle| self.result_target(handle, match_id.clone()?));
            if target.is_some() {
                assign_match_id(found, match_id, target);
            }
        }
        handle
    }

    fn assign_result_handle_for_text_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [TextMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    Some(found.column),
                    None,
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_symbol_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [SymbolMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    found.column,
                    found.stable_symbol_id.clone(),
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    pub(super) fn assign_result_handle_for_batch_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [crate::mcp::types::SearchBatchMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    found.column,
                    found.stable_symbol_id.clone(),
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_hybrid_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [SearchHybridMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    Some(found.column),
                    None,
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_reference_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [ReferenceMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    Some(found.column),
                    found.stable_symbol_id.clone(),
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_navigation_locations(
        &self,
        tool_name: &'static str,
        matches: &mut [NavigationLocation],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    Some(found.column),
                    found.stable_symbol_id.clone(),
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_implementation_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [ImplementationMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    Some(found.column),
                    found.stable_symbol_id.clone(),
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_call_hierarchy_matches(
        &self,
        tool_name: &'static str,
        matches: &mut [CallHierarchyMatch],
    ) -> Option<String> {
        self.assign_bound_result_handle(
            tool_name,
            matches,
            |found| {
                let stable_symbol_id = match tool_name {
                    "incoming_calls" => found.source_stable_symbol_id.clone(),
                    "outgoing_calls" => found.target_stable_symbol_id.clone(),
                    _ => None,
                };
                (
                    found.repository_id.clone(),
                    found.path.clone(),
                    found.line,
                    Some(found.column),
                    stable_symbol_id,
                )
            },
            |found, match_id, target| {
                found.match_id = match_id;
                found.target_ref = target;
            },
        )
    }

    fn assign_result_handle_for_document_symbols(
        &self,
        tool_name: &'static str,
        symbols: &mut [DocumentSymbolItem],
    ) -> Option<String> {
        let scope = Self::result_handle_scope_for_tool(tool_name);
        fn pending_sources(symbols: &[DocumentSymbolItem], pending: &mut Vec<(String, String)>) {
            for symbol in symbols {
                pending.push((symbol.repository_id.clone(), symbol.path.clone()));
                pending_sources(&symbol.children, pending);
            }
        }
        fn visit(
            scope: &str,
            symbols: &mut [DocumentSymbolItem],
            next_id: &mut usize,
            stored: &mut BTreeMap<String, crate::mcp::server_cache::ResultHandleMatchAnchor>,
            snapshots: &BTreeMap<
                (String, String),
                Arc<crate::mcp::server_cache::ResultHandleSourceSnapshot>,
            >,
        ) {
            for symbol in symbols {
                let match_id = format!("{scope}:m{}", *next_id);
                *next_id = next_id.saturating_add(1);
                if let Some(snapshot) = snapshots
                    .get(&(symbol.repository_id.clone(), symbol.path.clone()))
                    .cloned()
                {
                    stored.insert(
                        match_id.clone(),
                        crate::mcp::server_cache::ResultHandleMatchAnchor {
                            source: snapshot,
                            line: symbol.line,
                            column: Some(symbol.column),
                            stable_symbol_id: symbol.stable_symbol_id.clone(),
                        },
                    );
                    symbol.match_id = Some(match_id);
                } else {
                    symbol.match_id = None;
                }
                visit(scope, &mut symbol.children, next_id, stored, snapshots);
            }
        }

        fn assign_targets(
            server: &FriggMcpServer,
            result_handle: &Option<String>,
            symbols: &mut [DocumentSymbolItem],
        ) {
            for symbol in symbols {
                symbol.target_ref = result_handle
                    .as_ref()
                    .zip(symbol.match_id.as_ref())
                    .and_then(|(handle, match_id)| {
                        server.result_target(handle.clone(), match_id.clone())
                    });
                assign_targets(server, result_handle, &mut symbol.children);
            }
        }

        let mut pending = Vec::new();
        pending_sources(symbols, &mut pending);
        let snapshots = self.bind_result_handle_source_snapshots(pending);
        let mut stored = BTreeMap::new();
        let mut next_id = 1usize;
        visit(scope, symbols, &mut next_id, &mut stored, &snapshots);
        let result_handle = self.store_session_result_handle(tool_name, stored);
        assign_targets(self, &result_handle, symbols);
        result_handle
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
            self.validate_recovery_actions(&mut response.recovery);
            return Ok(response);
        }

        response.result_handle =
            self.assign_result_handle_for_text_matches("search_text", &mut response.matches);
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
        );
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
                Self::attach_recovery_index_if_missing(&mut response.recovery, index);
            }
        } else if response.recovery.scope.is_none() {
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
        self.validate_recovery_actions(&mut response.recovery);
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
        params: Option<&SearchHybridParams>,
    ) -> SearchHybridResponse {
        response.result_handle =
            self.assign_result_handle_for_hybrid_matches("search_hybrid", &mut response.matches);
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "search_hybrid");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.latency_class.is_none() {
            response.latency_class = Some(if response.matches.is_empty() {
                crate::mcp::types::LatencyClass::Warm
            } else {
                crate::mcp::types::LatencyClass::Cold
            });
        }
        let lexical_only = response
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lexical_only_mode)
            .unwrap_or(false);
        let graph_channel_contributed = response.matches.iter().any(|matched| {
            matched.graph_score > 0.0
                || !matched.graph_sources.is_empty()
                || matched.graph_mode.is_some()
        });
        response.ranking_note = Some(match (lexical_only, graph_channel_contributed) {
            (true, true) => {
                "discovery_only; lexical_only (semantic not contributing); hybrid graph is ranking signal (not nav call edges); confirm with exact search"
                    .to_owned()
            }
            (true, false) => {
                "discovery_only; lexical_only (semantic not contributing); confirm with exact search"
                    .to_owned()
            }
            (false, true) => {
                "discovery_only; hybrid graph is ranking signal (not nav call edges); confirm with exact search"
                    .to_owned()
            }
            (false, false) => "discovery_only; confirm with exact search".to_owned(),
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
        } else if !response.matches.is_empty() && response.recovery.next_actions.is_empty() {
            let pivot_sources = Self::hybrid_pivot_match_sources(&response.matches);
            response.recovery = RecoveryFields::hybrid_discovery_exact_pivot(
                query.unwrap_or(""),
                response.best_pivot_path.as_deref(),
                &pivot_sources,
            );
            if let (Some(params), Some(result_handle), Some(match_id)) = (
                params,
                response.result_handle.as_ref(),
                response
                    .matches
                    .first()
                    .and_then(|matched| matched.match_id.as_ref()),
            ) {
                let mut actions = std::mem::take(&mut response.recovery.next_actions);
                for (order, action) in actions.iter_mut().enumerate() {
                    action.id = NextActionId(format!("hybrid-verify:{order}"));
                    action.order = order as u16;
                    action.dependencies.clear();
                }
                let dependencies = actions
                    .iter()
                    .map(|action| action.id.clone())
                    .collect::<Vec<_>>();
                let proof_order = actions.len() as u16;
                let mut proof = canonical_next_action(
                    "hybrid-proof",
                    NextActionRole::ProofRead,
                    proof_order,
                    NextActionTarget::ReadMatch(ReadMatchParams {
                        result_handle: result_handle.clone(),
                        match_id: match_id.clone(),
                        before: None,
                        after: None,
                        presentation_mode: None,
                        include_context_efficiency: None,
                        origin: Some(NextActionOrigin(ReplayOriginTarget::SearchHybrid(
                            params.clone(),
                        ))),
                    }),
                    "proof-read the ranked hybrid pivot after an exact verification action",
                );
                if !dependencies.is_empty() {
                    proof.dependencies = vec![NextActionDependency {
                        mode: NextActionDependencyMode::Any,
                        action_ids: dependencies,
                    }];
                }
                actions.push(proof);
                response.recovery.set_next_actions(actions);
            }
            crate::mcp::routing_stats::record_recovery_issued();
        }
        if !Self::should_return_full_response(response_mode) {
            response.metadata = response.metadata.and_then(|mut metadata| {
                let context_efficiency = metadata.context_efficiency.take();
                context_efficiency.as_ref()?;
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
        );
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
        );
        let (handle_scope, handle_expires) =
            Self::attach_handle_metadata(&response.result_handle, "search_symbol");
        response.handle_scope = handle_scope;
        response.handle_expires = handle_expires;
        if response.latency_class.is_none() {
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
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
        );
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
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
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
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
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
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
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
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
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
        self.validate_recovery_actions(&mut response.recovery);
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
        self.append_target_follow_up_actions(
            &mut response.recovery,
            response
                .matches
                .iter()
                .filter_map(|matched| matched.target_ref.as_ref()),
        );
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
            response.note = None;
        }
        self.validate_recovery_actions(&mut response.recovery);
        response
    }

    pub(super) fn present_document_symbols_response(
        &self,
        mut response: DocumentSymbolsResponse,
        params: &DocumentSymbolsParams,
        resume_offset: usize,
        continuation_binding: Option<ContinuationBinding>,
    ) -> DocumentSymbolsResponse {
        const DEFAULT_DOCUMENT_SYMBOLS_LIMIT: usize = 200;
        const MAX_DOCUMENT_SYMBOLS_LIMIT: usize = 1000;

        let top_level_only = params.top_level_only.unwrap_or(true);
        response.top_level_only = top_level_only;
        if top_level_only {
            for symbol in &mut response.symbols {
                symbol.children.clear();
            }
        }

        let total_symbols = response.symbols.len();
        response.total_symbols = total_symbols;
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
        let continuation = truncated
            .then(|| {
                continuation_binding.map(|mut binding| {
                    binding.next_position = resume_offset.saturating_add(response.returned);
                    self.store_session_continuation(binding)
                })
            })
            .flatten();
        response.completeness = ResultCompleteness::try_new(
            ResultUnit::DocumentSymbol,
            response.returned,
            Some(total_symbols),
            !truncated,
            truncated,
            truncated
                .then_some(ResultTruncationReason::PageLimit)
                .into_iter()
                .collect(),
            Vec::new(),
            continuation,
        )
        .expect("document symbol pagination has internally consistent completeness");

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
    use crate::mcp::types::{NavigationResolutionSource, ResultCompleteness, SearchPatternType};
    use crate::settings::FriggConfig;
    use rmcp::model::ErrorCode;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn continuation_binding(repository_id: &str) -> ContinuationBinding {
        ContinuationBinding {
            tool: "search_text",
            request_digest: "normalized-request-v1".to_owned(),
            repository_ids: vec![repository_id.to_owned()],
            snapshot_fingerprints: vec!["snapshot-v1".to_owned()],
            unit: ResultUnit::Occurrence,
            next_position: 10,
        }
    }

    #[test]
    fn continuation_is_bound_to_session_request_scope_and_repository_snapshot() {
        let server = presentation_test_server();
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let token = server.store_session_continuation(continuation_binding(&repository_id));

        let resolved = server
            .session_continuation_lookup(
                &token,
                "search_text",
                "normalized-request-v1",
                std::slice::from_ref(&repository_id),
                &["snapshot-v1".to_owned()],
                ResultUnit::Occurrence,
            )
            .expect("matching continuation should resolve");
        assert_eq!(resolved.next_position, 10);

        let error = server
            .session_continuation_lookup(
                &token,
                "search_text",
                "different-normalized-request",
                std::slice::from_ref(&repository_id),
                &["snapshot-v1".to_owned()],
                ResultUnit::Occurrence,
            )
            .expect_err("changed request must reject the token");
        assert_eq!(
            error.kind,
            crate::mcp::types::ContinuationErrorKind::ScopeMismatch
        );

        let other_session = server.clone_for_new_session();
        let error = other_session
            .session_continuation_lookup(
                &token,
                "search_text",
                "normalized-request-v1",
                std::slice::from_ref(&repository_id),
                &["snapshot-v1".to_owned()],
                ResultUnit::Occurrence,
            )
            .expect_err("another session must reject the token");
        assert_eq!(
            error.kind,
            crate::mcp::types::ContinuationErrorKind::ScopeMismatch
        );
    }

    #[test]
    fn continuation_expires_and_repository_lifecycle_invalidation_makes_it_stale() {
        let server = presentation_test_server();
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let token = server.store_session_continuation(continuation_binding(&repository_id));
        {
            let mut cache = server
                .session_state
                .inner
                .result_handles
                .write()
                .expect("continuation cache lock");
            cache
                .continuations
                .entries
                .get_mut(&token)
                .expect("stored continuation")
                .generated_at =
                Instant::now() - FriggMcpServer::SESSION_CONTINUATION_TTL - Duration::from_secs(1);
        }
        let error = server
            .session_continuation_lookup(
                &token,
                "search_text",
                "normalized-request-v1",
                std::slice::from_ref(&repository_id),
                &["snapshot-v1".to_owned()],
                ResultUnit::Occurrence,
            )
            .expect_err("expired continuation must be stale");
        assert_eq!(error.kind, crate::mcp::types::ContinuationErrorKind::Stale);

        let token = server.store_session_continuation(continuation_binding(&repository_id));
        server.invalidate_session_result_handles_for_repository_ids([repository_id.as_str()]);
        let error = server
            .session_continuation_lookup(
                &token,
                "search_text",
                "normalized-request-v1",
                std::slice::from_ref(&repository_id),
                &["snapshot-v1".to_owned()],
                ResultUnit::Occurrence,
            )
            .expect_err("repository invalidation must make continuation stale");
        assert_eq!(error.kind, crate::mcp::types::ContinuationErrorKind::Stale);
    }

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
                        target_ref: None,
                        repository_id: workspace.repository_id.clone(),
                        path: "src/leak.rs".to_owned(),
                        line: 1,
                        column: 8,
                        excerpt: "secret_token".to_owned(),
                        witness_score_hint_millis: None,
                        witness_provenance_ids: None,
                    }],
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 1, 1)
                        .expect("complete fixture"),
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
        let server = FriggMcpServer::new(
            FriggConfig::from_workspace_roots(vec![workspace_root])
                .expect("workspace root must produce valid config"),
        );
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("presentation fixture workspace");
        server
            .adopt_workspace(&workspace, true)
            .expect("presentation fixture workspace should be adopted");
        server
    }

    fn sample_text_match(repository_id: &str, path: &str) -> TextMatch {
        TextMatch {
            match_id: None,
            target_ref: None,
            repository_id: repository_id.to_owned(),
            path: path.to_owned(),
            line: 1,
            column: 1,
            excerpt: "needle".to_owned(),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        }
    }

    fn bound_text_match(server: &FriggMcpServer, path: &str) -> TextMatch {
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("presentation fixture workspace");
        let source = workspace.root.join(path);
        fs::create_dir_all(source.parent().expect("source parent"))
            .expect("source parent should be creatable");
        fs::write(&source, "fn proof_fixture() {}\n").expect("source should be writable");
        sample_text_match(&workspace.repository_id, path)
    }

    fn call_hierarchy_match(
        server: &FriggMcpServer,
        source_stable_symbol_id: Option<&str>,
        target_stable_symbol_id: Option<&str>,
        line: usize,
        column: usize,
    ) -> CallHierarchyMatch {
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("presentation fixture workspace");
        let path = "src/calls.rs";
        let source = workspace.root.join(path);
        fs::create_dir_all(source.parent().expect("source parent"))
            .expect("source parent should be creatable");
        fs::write(&source, "    fn caller() {}\n    fn callee() {}\n")
            .expect("source should be writable");
        CallHierarchyMatch {
            match_id: None,
            target_ref: None,
            source_stable_symbol_id: source_stable_symbol_id.map(str::to_owned),
            target_stable_symbol_id: target_stable_symbol_id.map(str::to_owned),
            source_symbol: "caller".to_owned(),
            target_symbol: "callee".to_owned(),
            repository_id: workspace.repository_id,
            path: path.to_owned(),
            line,
            column,
            relation: "calls".to_owned(),
            source_container: None,
            target_container: None,
            source_signature: None,
            target_signature: None,
            precision: Some("heuristic".to_owned()),
            call_path: None,
            call_line: None,
            call_column: None,
            call_end_line: None,
            call_end_column: None,
            follow_up_structural: Vec::new(),
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
                        bound_text_match(&server, "src/a.rs"),
                        bound_text_match(&server, "src/b.rs"),
                    ],
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 2, 2)
                        .expect("complete fixture"),
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
            .expect("presentation should preserve handler-selected rows");

        assert_eq!(response.matches.len(), 2);
        assert_eq!(response.total_matches, 2);
        assert!(response.result_handle.is_some());
    }

    #[test]
    fn search_text_limit_zero_with_files_with_matches_returns_no_matches() {
        let server = presentation_test_server();
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 1,
                    matches: vec![bound_text_match(&server, "src/a.rs")],
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 1, 1)
                        .expect("complete fixture"),
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
            .expect("presentation should preserve handler-selected rows");

        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.total_matches, 1);
        assert!(response.result_handle.is_some());
    }

    #[test]
    fn search_text_zero_hit_serializes_recovery_at_top_level() {
        let server = presentation_test_server();
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 0,
                    matches: Vec::new(),
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
                        .expect("complete fixture"),
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
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
                        .expect("complete fixture"),
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
        let matched = bound_text_match(&server, "src/a.rs");
        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 1,
                    matches: vec![matched],
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 1, 1)
                        .expect("complete fixture"),
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
    fn symbol_binder_preserves_stable_identity_in_the_verified_anchor() {
        let server = presentation_test_server();
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("presentation fixture workspace");
        let source = workspace.root.join("src/symbol.rs");
        fs::create_dir_all(source.parent().expect("source parent"))
            .expect("source parent should be creatable");
        fs::write(&source, "fn stable_target() {}\n").expect("source should be writable");
        let mut matches = vec![SymbolMatch {
            match_id: None,
            target_ref: None,
            stable_symbol_id: Some("scip-rust pkg repo#stable_target".to_owned()),
            repository_id: workspace.repository_id,
            symbol: "stable_target".to_owned(),
            kind: "function".to_owned(),
            path: "src/symbol.rs".to_owned(),
            line: 1,
            column: Some(4),
            excerpt: Some("fn stable_target()".to_owned()),
            path_class: Some("runtime".to_owned()),
            container: None,
            signature: None,
        }];

        let handle = server
            .assign_result_handle_for_symbol_matches("search_symbol", &mut matches)
            .expect("bound symbol should create a handle");
        let match_id = matches[0]
            .match_id
            .as_deref()
            .expect("bound symbol should create a match id");
        assert!(matches[0].target_ref.is_some());
        let anchor = server
            .session_result_handle_match(&handle, match_id)
            .expect("bound symbol anchor should be stored");
        assert_eq!(
            anchor.stable_symbol_id.as_deref(),
            Some("scip-rust pkg repo#stable_target")
        );
    }

    #[test]
    fn call_hierarchy_binder_uses_the_row_side_identity_and_coordinate() {
        let server = presentation_test_server();
        let mut incoming = vec![call_hierarchy_match(
            &server,
            Some("caller-stable-id"),
            Some("selected-target-id"),
            1,
            5,
        )];
        let incoming_handle = server
            .assign_result_handle_for_call_hierarchy_matches("incoming_calls", &mut incoming)
            .expect("incoming row should bind");
        let incoming_anchor = server
            .session_result_handle_match(
                &incoming_handle,
                incoming[0].match_id.as_deref().expect("incoming match id"),
            )
            .expect("incoming anchor");
        assert_eq!(
            incoming_anchor.stable_symbol_id.as_deref(),
            Some("caller-stable-id")
        );
        assert_eq!(incoming_anchor.column, Some(5));

        let mut outgoing = vec![call_hierarchy_match(
            &server,
            Some("selected-source-id"),
            Some("callee-stable-id"),
            2,
            5,
        )];
        let outgoing_handle = server
            .assign_result_handle_for_call_hierarchy_matches("outgoing_calls", &mut outgoing)
            .expect("outgoing row should bind");
        let outgoing_anchor = server
            .session_result_handle_match(
                &outgoing_handle,
                outgoing[0].match_id.as_deref().expect("outgoing match id"),
            )
            .expect("outgoing anchor");
        assert_eq!(
            outgoing_anchor.stable_symbol_id.as_deref(),
            Some("callee-stable-id")
        );
        assert_eq!(outgoing_anchor.column, Some(5));
    }

    #[test]
    fn orphan_text_target_is_retained_without_navigation_actions() {
        let server = presentation_test_server();
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("presentation fixture workspace");
        let path = "src/orphan.rs";
        let source = workspace.root.join(path);
        fs::create_dir_all(source.parent().expect("source parent"))
            .expect("source parent should be creatable");
        fs::write(&source, "// orphan_marker\n").expect("source should be writable");

        let response = server
            .present_search_text_response(
                SearchTextResponse {
                    total_matches: 1,
                    matches: vec![sample_text_match(&workspace.repository_id, path)],
                    completeness: ResultCompleteness::complete(ResultUnit::Occurrence, 1, 1)
                        .expect("complete fixture"),
                    result_handle: None,
                    handle_scope: None,
                    handle_expires: None,
                    count_only: None,
                    latency_class: None,
                    metadata: None,
                    recovery: RecoveryFields::default(),
                },
                &SearchTextParams {
                    query: "orphan_marker".to_owned(),
                    repository_id: Some(workspace.repository_id),
                    ..Default::default()
                },
            )
            .expect("orphan text result should still be presented");

        assert!(response.matches[0].target_ref.is_some());
        assert!(
            response
                .recovery
                .next_actions
                .iter()
                .all(|action| !matches!(
                    &action.target,
                    NextActionTarget::GoToDefinition(_)
                        | NextActionTarget::FindReferences(_)
                        | NextActionTarget::ImpactBundle(_)
                )),
            "unresolvable anchors must not advertise target navigation"
        );
    }

    #[test]
    fn bound_anchors_share_one_source_snapshot_and_skip_unreadable_rows() {
        let server = presentation_test_server();
        let mut matches = vec![
            bound_text_match(&server, "src/shared.rs"),
            bound_text_match(&server, "src/shared.rs"),
            sample_text_match("missing-repository", "src/missing.rs"),
        ];

        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("readable rows should create a handle");
        assert_eq!(matches[0].match_id.as_deref(), Some("search:m1"));
        assert_eq!(matches[1].match_id.as_deref(), Some("search:m2"));
        assert!(matches[2].match_id.is_none());

        let cache = server
            .session_state
            .inner
            .result_handles
            .read()
            .expect("handle cache lock");
        let entry = cache.entries.get(&handle).expect("stored handle");
        assert_eq!(entry.origin_tool, "search_text");
        let first = entry.matches.get("search:m1").expect("first anchor");
        let second = entry.matches.get("search:m2").expect("second anchor");
        assert!(Arc::ptr_eq(&first.source, &second.source));
        assert_eq!(entry.matches.len(), 2);
    }

    #[test]
    fn hybrid_compact_ranking_note_signals_lexical_only_without_metadata_dump() {
        use crate::mcp::types::{ResponseMode, SearchHybridMetadata};

        let server = presentation_test_server();
        let with_lexical_only = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: Vec::new(),
                completeness: ResultCompleteness::try_new(
                    ResultUnit::Occurrence,
                    0,
                    None,
                    false,
                    false,
                    vec![],
                    vec![crate::mcp::types::ResultIncompleteReason::RankedDiscovery],
                    None,
                )
                .expect("ranked discovery completeness"),
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
            None,
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
                completeness: ResultCompleteness::try_new(
                    ResultUnit::Occurrence,
                    0,
                    None,
                    false,
                    false,
                    vec![],
                    vec![crate::mcp::types::ResultIncompleteReason::RankedDiscovery],
                    None,
                )
                .expect("ranked discovery completeness"),
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
            None,
        );
        assert_eq!(
            multi_channel.ranking_note.as_deref(),
            Some("discovery_only; confirm with exact search"),
            "when semantic contributes, ranking_note stays the short discovery form"
        );

        let with_graph = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: vec![SearchHybridMatch {
                    match_id: None,
                    target_ref: None,
                    repository_id: "repo".to_owned(),
                    path: "src/a.rs".to_owned(),
                    line: 1,
                    column: 1,
                    excerpt: "fn a".to_owned(),
                    anchor: None,
                    blended_score: 1.0,
                    lexical_score: 0.5,
                    graph_score: 0.4,
                    semantic_score: 0.0,
                    lexical_sources: vec![],
                    graph_sources: vec!["graph:foo:calls:src/a.rs:1".to_owned()],
                    semantic_sources: vec![],
                    graph_mode: Some("heuristic_symbol_graph".to_owned()),
                    path_class: None,
                    source_class: None,
                    surface_families: vec![],
                    navigation_hint: None,
                    rank_reasons: vec![],
                }],
                completeness: ResultCompleteness::try_new(
                    ResultUnit::Occurrence,
                    1,
                    None,
                    false,
                    false,
                    vec![],
                    vec![crate::mcp::types::ResultIncompleteReason::RankedDiscovery],
                    None,
                )
                .expect("ranked discovery completeness"),
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
                    semantic_hit_count: Some(0),
                    semantic_match_count: Some(0),
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
            None,
        );
        assert_eq!(
            with_graph.ranking_note.as_deref(),
            Some(
                "discovery_only; hybrid graph is ranking signal (not nav call edges); confirm with exact search"
            ),
            "compact must surface hybrid-graph≠nav honesty when graph contributes"
        );
        assert_eq!(
            with_graph.matches[0].graph_mode.as_deref(),
            Some("heuristic_symbol_graph"),
            "graph_mode must survive compact presentation"
        );

        let lexical_only_and_graph = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: vec![SearchHybridMatch {
                    match_id: None,
                    target_ref: None,
                    repository_id: "repo".to_owned(),
                    path: "src/a.rs".to_owned(),
                    line: 1,
                    column: 1,
                    excerpt: "fn a".to_owned(),
                    anchor: None,
                    blended_score: 1.0,
                    lexical_score: 0.5,
                    graph_score: 0.4,
                    semantic_score: 0.0,
                    lexical_sources: vec![],
                    graph_sources: vec!["graph_projection:t:src/a.rs:1".to_owned()],
                    semantic_sources: vec![],
                    graph_mode: Some("projection".to_owned()),
                    path_class: None,
                    source_class: None,
                    surface_families: vec![],
                    navigation_hint: None,
                    rank_reasons: vec![],
                }],
                completeness: ResultCompleteness::try_new(
                    ResultUnit::Occurrence,
                    1,
                    None,
                    false,
                    false,
                    vec![],
                    vec![crate::mcp::types::ResultIncompleteReason::RankedDiscovery],
                    None,
                )
                .expect("ranked discovery completeness"),
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
                    semantic_requested: Some(false),
                    semantic_enabled: Some(false),
                    semantic_status: None,
                    semantic_reason: None,
                    semantic_candidate_count: None,
                    semantic_hit_count: None,
                    semantic_match_count: None,
                    lexical_only_mode: Some(true),
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
            None,
        );
        assert_eq!(
            lexical_only_and_graph.ranking_note.as_deref(),
            Some(
                "discovery_only; lexical_only (semantic not contributing); hybrid graph is ranking signal (not nav call edges); confirm with exact search"
            ),
            "lexical_only + graph must combine both honesty tokens"
        );
    }

    #[test]
    fn hybrid_and_symbol_zero_hit_include_recovery() {
        let server = presentation_test_server();
        let hybrid = server.present_search_hybrid_response(
            SearchHybridResponse {
                matches: Vec::new(),
                completeness: ResultCompleteness::try_new(
                    ResultUnit::Occurrence,
                    0,
                    None,
                    false,
                    false,
                    vec![],
                    vec![crate::mcp::types::ResultIncompleteReason::RankedDiscovery],
                    None,
                )
                .expect("ranked discovery completeness"),
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
            None,
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
                completeness: ResultCompleteness::complete(ResultUnit::Symbol, 0, 0)
                    .expect("empty symbol completeness"),
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
                completeness: FriggMcpServer::navigation_completeness(
                    ResultUnit::Reference,
                    0,
                    Some(0),
                    NavigationMode::UnavailableNoPrecise,
                    false,
                    None,
                ),
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
                completeness: FriggMcpServer::navigation_completeness(
                    ResultUnit::Definition,
                    0,
                    Some(0),
                    NavigationMode::HeuristicNoPrecise,
                    false,
                    None,
                ),
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
        let mut matches = vec![bound_text_match(&server, "src/lib.rs")];
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
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let mut dirty_matches = vec![bound_text_match(&server, "src/dirty.rs")];
        let mut clean_matches = vec![bound_text_match(&server, "src/clean.rs")];
        let mut mixed_matches = vec![
            bound_text_match(&server, "src/dirty.rs"),
            bound_text_match(&server, "src/other.rs"),
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
        let mixed_dirty_mid = mixed_matches[0]
            .match_id
            .clone()
            .expect("mixed dirty match_id");
        let mixed_clean_mid = mixed_matches[1]
            .match_id
            .clone()
            .expect("mixed clean match_id");

        server.invalidate_session_result_handles_for_paths(
            &[&repository_id],
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
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let mut matches = vec![bound_text_match(&server, "src/a.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        server.invalidate_session_result_handles_for_paths(&[&repository_id], &[]);

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::Found(_)
        ));
    }

    #[test]
    fn whole_repo_invalidation_drops_all_repo_handles() {
        let server = presentation_test_server();
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let mut matches = vec![bound_text_match(&server, "src/a.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        server.invalidate_session_result_handles_for_repository_ids([repository_id.as_str()]);

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::StaleHandle
        ));
    }

    #[test]
    fn directory_dirty_path_invalidates_nested_anchors() {
        let server = presentation_test_server();
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let mut matches = vec![bound_text_match(&server, "src/nested/lib.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        server.invalidate_session_result_handles_for_paths(&[&repository_id], &["src".to_owned()]);

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::StaleHandle
        ));
    }

    #[test]
    fn basename_dirty_does_not_overmatch_nested_paths() {
        let server = presentation_test_server();
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let mut matches = vec![bound_text_match(&server, "src/nested/foo.rs")];
        let handle = server
            .assign_result_handle_for_text_matches("search_text", &mut matches)
            .expect("handle");
        let mid = matches[0].match_id.clone().expect("match_id");

        server
            .invalidate_session_result_handles_for_paths(&[&repository_id], &["foo.rs".to_owned()]);

        assert!(matches!(
            server.session_result_handle_lookup(&handle, &mid),
            SessionResultHandleLookup::Found(_)
        ));
    }

    #[test]
    fn path_scoped_invalidation_does_not_touch_other_paths() {
        let server = presentation_test_server();
        let repository_id = server.known_workspaces()[0].repository_id.clone();
        let mut repo_a = vec![bound_text_match(&server, "src/lib.rs")];
        let mut repo_b = vec![bound_text_match(&server, "src/other.rs")];
        let handle_a = server
            .assign_result_handle_for_text_matches("search_text", &mut repo_a)
            .expect("handle a");
        let handle_b = server
            .assign_result_handle_for_text_matches("search_text", &mut repo_b)
            .expect("handle b");
        let mid_a = repo_a[0].match_id.clone().expect("mid a");
        let mid_b = repo_b[0].match_id.clone().expect("mid b");

        server.invalidate_session_result_handles_for_paths(
            &[&repository_id],
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
                completeness: FriggMcpServer::navigation_completeness(
                    ResultUnit::Reference,
                    0,
                    Some(0),
                    NavigationMode::UnavailableNoPrecise,
                    false,
                    None,
                ),
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                mode: NavigationMode::UnavailableNoPrecise,
                target_selection: Some(NavigationTargetSelectionSummary {
                    status: NavigationTargetSelectionStatus::DisambiguationRequired,
                    resolution_source: NavigationResolutionSource::DirectSymbol,
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
                completeness: FriggMcpServer::navigation_completeness(
                    ResultUnit::Definition,
                    0,
                    Some(0),
                    NavigationMode::UnavailableNoPrecise,
                    false,
                    None,
                ),
                matches: Vec::new(),
                result_handle: None,
                handle_scope: None,
                handle_expires: None,
                mode: NavigationMode::UnavailableNoPrecise,
                target_selection: Some(NavigationTargetSelectionSummary {
                    status: NavigationTargetSelectionStatus::DisambiguationRequired,
                    resolution_source: NavigationResolutionSource::DirectSymbol,
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

    #[test]
    fn target_follow_up_actions_replay_the_issued_target_unchanged() {
        let target = crate::mcp::types::TargetRef::result_match(
            "result-000001".to_owned(),
            "search:m1".to_owned(),
            "session-scope".to_owned(),
        )
        .expect("non-empty target");

        let actions = FriggMcpServer::target_follow_up_actions(Some(&target));
        assert_eq!(actions.len(), 3);
        for action in actions {
            let copied = match action.target {
                NextActionTarget::GoToDefinition(params) => {
                    assert!(params.symbol.is_none());
                    assert!(params.path.is_none());
                    params.target
                }
                NextActionTarget::FindReferences(params) => {
                    assert!(params.symbol.is_none());
                    assert!(params.path.is_none());
                    params.target
                }
                NextActionTarget::ImpactBundle(params) => {
                    assert!(params.symbol.is_empty());
                    params.target
                }
                _ => None,
            };
            assert!(copied.is_some(), "target action must use a supported tool");
            assert_eq!(copied, Some(target.clone()));
        }
    }
}
