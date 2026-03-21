use super::*;
use crate::mcp::server::runtime_cache::serialized_value_estimated_bytes;

impl FriggMcpServer {
    pub(crate) fn invalidate_repository_search_response_caches(&self, repository_id: &str) {
        let mut search_text_cache = self
            .cache_state
            .search_text_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = search_text_cache.len();
        search_text_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchTextResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(search_text_cache.len()),
        );

        let mut search_hybrid_cache = self
            .cache_state
            .search_hybrid_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = search_hybrid_cache.len();
        search_hybrid_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchHybridResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(search_hybrid_cache.len()),
        );

        let mut search_symbol_cache = self
            .cache_state
            .search_symbol_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = search_symbol_cache.len();
        search_symbol_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchSymbolResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(search_symbol_cache.len()),
        );
    }

    pub(crate) fn search_pattern_type_cache_key(pattern_type: &SearchPatternType) -> &'static str {
        match pattern_type {
            SearchPatternType::Literal => "literal",
            SearchPatternType::Regex => "regex",
        }
    }

    pub(crate) fn compile_cached_safe_regex(
        &self,
        raw: &str,
    ) -> Result<regex::Regex, crate::searcher::RegexSearchError> {
        if let Some(cached) = self
            .cache_state
            .compiled_safe_regex_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(raw)
            .cloned()
        {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Hit,
                1,
            );
            return Ok(cached);
        }
        self.record_runtime_cache_event(
            RuntimeCacheFamily::CompiledSafeRegex,
            RuntimeCacheEvent::Miss,
            1,
        );

        let compiled = compile_safe_regex(raw)?;
        let mut cache = self
            .cache_state
            .compiled_safe_regex_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.get(raw).cloned() {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Hit,
                1,
            );
            return Ok(cached);
        }
        let inserted = cache.insert(raw.to_owned(), compiled.clone()).is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::CompiledSafeRegex,
            &mut cache,
            |pattern, _| pattern.len().saturating_add(256),
        );
        Ok(compiled)
    }

    pub(crate) fn cached_search_text_response(
        &self,
        cache_key: &SearchTextResponseCacheKey,
    ) -> Option<CachedSearchTextResponse> {
        let cached = self
            .cache_state
            .search_text_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchTextResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(crate) fn cache_search_text_response(
        &self,
        cache_key: SearchTextResponseCacheKey,
        response: &SearchTextResponse,
        source_refs: &Value,
    ) {
        let mut cache = self
            .cache_state
            .search_text_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedSearchTextResponse {
                    response: response.clone(),
                    source_refs: source_refs.clone(),
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::SearchTextResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::SearchTextResponse,
            &mut cache,
            |_, entry| {
                serialized_value_estimated_bytes(&entry.response)
                    .saturating_add(serialized_value_estimated_bytes(&entry.source_refs))
            },
        );
    }

    pub(crate) fn cached_search_hybrid_response(
        &self,
        cache_key: &SearchHybridResponseCacheKey,
    ) -> Option<CachedSearchHybridResponse> {
        let cached = self
            .cache_state
            .search_hybrid_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchHybridResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(crate) fn cache_search_hybrid_response(
        &self,
        cache_key: SearchHybridResponseCacheKey,
        response: &SearchHybridResponse,
        source_refs: &Value,
    ) {
        let mut cache = self
            .cache_state
            .search_hybrid_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedSearchHybridResponse {
                    response: response.clone(),
                    source_refs: source_refs.clone(),
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::SearchHybridResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::SearchHybridResponse,
            &mut cache,
            |_, entry| {
                serialized_value_estimated_bytes(&entry.response)
                    .saturating_add(serialized_value_estimated_bytes(&entry.source_refs))
            },
        );
    }

    pub(crate) fn cached_search_symbol_response(
        &self,
        cache_key: &SearchSymbolResponseCacheKey,
    ) -> Option<CachedSearchSymbolResponse> {
        let cached = self
            .cache_state
            .search_symbol_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchSymbolResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cache_search_symbol_response(
        &self,
        cache_key: SearchSymbolResponseCacheKey,
        response: &SearchSymbolResponse,
        scoped_repository_ids: &[String],
        diagnostics_count: usize,
        manifest_walk_diagnostics_count: usize,
        manifest_read_diagnostics_count: usize,
        symbol_extraction_diagnostics_count: usize,
        effective_limit: usize,
    ) {
        let mut cache = self
            .cache_state
            .search_symbol_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedSearchSymbolResponse {
                    response: response.clone(),
                    scoped_repository_ids: scoped_repository_ids.to_owned(),
                    diagnostics_count,
                    manifest_walk_diagnostics_count,
                    manifest_read_diagnostics_count,
                    symbol_extraction_diagnostics_count,
                    effective_limit,
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::SearchSymbolResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::SearchSymbolResponse,
            &mut cache,
            |_, entry| {
                serialized_value_estimated_bytes(&entry.response).saturating_add(
                    entry
                        .scoped_repository_ids
                        .iter()
                        .map(String::len)
                        .sum::<usize>(),
                )
            },
        );
    }
}
