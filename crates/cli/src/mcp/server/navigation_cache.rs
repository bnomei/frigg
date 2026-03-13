use super::*;

impl FriggMcpServer {
    pub(super) fn invalidate_repository_navigation_response_caches(&self, repository_id: &str) {
        self.go_to_definition_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| {
                !response_cache_scopes_include_repository(
                    repository_id,
                    &key.scoped_repository_ids,
                    &key.freshness_scopes,
                )
            });
        self.find_declarations_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| {
                !response_cache_scopes_include_repository(
                    repository_id,
                    &key.scoped_repository_ids,
                    &key.freshness_scopes,
                )
            });
        self.heuristic_reference_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| key.repository_id != repository_id);
    }

    pub(super) fn cached_go_to_definition_response(
        &self,
        cache_key: &GoToDefinitionResponseCacheKey,
    ) -> Option<CachedGoToDefinitionResponse> {
        self.go_to_definition_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned()
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
        self.go_to_definition_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            );
    }

    pub(super) fn cached_find_declarations_response(
        &self,
        cache_key: &FindDeclarationsResponseCacheKey,
    ) -> Option<CachedFindDeclarationsResponse> {
        self.find_declarations_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned()
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
        self.find_declarations_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
            );
    }

    pub(super) fn cached_heuristic_references(
        &self,
        cache_key: &HeuristicReferenceCacheKey,
    ) -> Option<CachedHeuristicReferences> {
        self.heuristic_reference_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned()
    }

    pub(super) fn cache_heuristic_references(
        &self,
        cache_key: HeuristicReferenceCacheKey,
        references: Vec<HeuristicReference>,
        source_read_diagnostics_count: usize,
        source_files_loaded: usize,
        source_bytes_loaded: u64,
    ) {
        self.heuristic_reference_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                cache_key,
                CachedHeuristicReferences {
                    references: Arc::new(references),
                    source_read_diagnostics_count,
                    source_files_loaded,
                    source_bytes_loaded,
                },
            );
    }
}
