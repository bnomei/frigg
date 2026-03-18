use super::*;

impl FriggMcpServer {
    pub(super) fn invalidate_repository_navigation_response_caches(&self, repository_id: &str) {
        let mut go_to_definition_cache = self
            .cache_state
            .go_to_definition_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = go_to_definition_cache.len();
        go_to_definition_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::GoToDefinitionResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(go_to_definition_cache.len()),
        );

        let mut find_declarations_cache = self
            .cache_state
            .find_declarations_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = find_declarations_cache.len();
        find_declarations_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::FindDeclarationsResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(find_declarations_cache.len()),
        );

        let mut heuristic_reference_cache = self
            .cache_state
            .heuristic_reference_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = heuristic_reference_cache.len();
        heuristic_reference_cache.retain(|key, _| key.repository_id != repository_id);
        self.record_runtime_cache_event(
            RuntimeCacheFamily::HeuristicReference,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(heuristic_reference_cache.len()),
        );
    }

    pub(super) fn cached_go_to_definition_response(
        &self,
        cache_key: &GoToDefinitionResponseCacheKey,
    ) -> Option<CachedGoToDefinitionResponse> {
        let cached = self
            .cache_state
            .go_to_definition_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::GoToDefinitionResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_go_to_definition_response(
        &self,
        cache_key: GoToDefinitionResponseCacheKey,
        response: &GoToDefinitionResponse,
        scoped_repository_ids: &[String],
        selected_symbol_id: Option<&str>,
        selected_precise_symbol: Option<&str>,
        resolution_precision: Option<&str>,
        resolution_source: Option<&str>,
        effective_limit: usize,
        precise_artifacts_ingested: usize,
        precise_artifacts_failed: usize,
        match_count: usize,
    ) {
        let mut cache = self
            .cache_state
            .go_to_definition_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedGoToDefinitionResponse {
                    response: response.clone(),
                    scoped_repository_ids: scoped_repository_ids.to_owned(),
                    selected_symbol_id: selected_symbol_id.map(str::to_owned),
                    selected_precise_symbol: selected_precise_symbol.map(str::to_owned),
                    resolution_precision: resolution_precision.map(str::to_owned),
                    resolution_source: resolution_source.map(str::to_owned),
                    effective_limit,
                    precise_artifacts_ingested,
                    precise_artifacts_failed,
                    match_count,
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::GoToDefinitionResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(
            RuntimeCacheFamily::GoToDefinitionResponse,
            &mut cache,
        );
    }

    pub(super) fn cached_find_declarations_response(
        &self,
        cache_key: &FindDeclarationsResponseCacheKey,
    ) -> Option<CachedFindDeclarationsResponse> {
        let cached = self
            .cache_state
            .find_declarations_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::FindDeclarationsResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_find_declarations_response(
        &self,
        cache_key: FindDeclarationsResponseCacheKey,
        response: &FindDeclarationsResponse,
        scoped_repository_ids: &[String],
        selected_symbol_id: Option<&str>,
        selected_precise_symbol: Option<&str>,
        resolution_precision: Option<&str>,
        resolution_source: Option<&str>,
        effective_limit: usize,
        precise_artifacts_ingested: usize,
        precise_artifacts_failed: usize,
        match_count: usize,
    ) {
        let mut cache = self
            .cache_state
            .find_declarations_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedFindDeclarationsResponse {
                    response: response.clone(),
                    scoped_repository_ids: scoped_repository_ids.to_owned(),
                    selected_symbol_id: selected_symbol_id.map(str::to_owned),
                    selected_precise_symbol: selected_precise_symbol.map(str::to_owned),
                    resolution_precision: resolution_precision.map(str::to_owned),
                    resolution_source: resolution_source.map(str::to_owned),
                    effective_limit,
                    precise_artifacts_ingested,
                    precise_artifacts_failed,
                    match_count,
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FindDeclarationsResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(
            RuntimeCacheFamily::FindDeclarationsResponse,
            &mut cache,
        );
    }

    pub(super) fn cached_heuristic_references(
        &self,
        cache_key: &HeuristicReferenceCacheKey,
    ) -> Option<CachedHeuristicReferences> {
        let cached = self
            .cache_state
            .heuristic_reference_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::HeuristicReference,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_heuristic_references(
        &self,
        cache_key: HeuristicReferenceCacheKey,
        references: Vec<HeuristicReference>,
        source_read_diagnostics_count: usize,
        source_files_loaded: usize,
        source_bytes_loaded: u64,
    ) {
        let mut cache = self
            .cache_state
            .heuristic_reference_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedHeuristicReferences {
                    references: Arc::new(references),
                    source_read_diagnostics_count,
                    source_files_loaded,
                    source_bytes_loaded,
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::HeuristicReference,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(RuntimeCacheFamily::HeuristicReference, &mut cache);
    }
}
