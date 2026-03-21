//! Runtime cache helpers used by the MCP server.
//!
//! These helpers make the declared cache budgets operational by trimming process-wide response
//! caches with approximate serialized-size accounting instead of entry count alone.

use super::*;
use serde::Serialize;

impl FriggMcpServer {
    pub(super) fn runtime_text_searcher(&self, config: FriggConfig) -> TextSearcher {
        TextSearcher::with_runtime_projection_store_service(
            config,
            Arc::clone(&self.runtime_state.validated_manifest_candidate_cache),
            self.runtime_state.searcher_projection_store_service.clone(),
        )
    }

    pub(super) fn record_runtime_cache_event(
        &self,
        family: RuntimeCacheFamily,
        event: RuntimeCacheEvent,
        count: usize,
    ) {
        if count == 0 {
            return;
        }
        let mut telemetry = self
            .runtime_state
            .runtime_cache_telemetry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        telemetry.entry(family).or_default().record(event, count);
    }

    /// Trims a process-wide cache against its configured entry and byte budget.
    ///
    /// The byte estimator is intentionally approximate; the goal is bounded residency for
    /// long-lived servers rather than exact heap accounting.
    pub(super) fn trim_runtime_cache_to_budget<K, V, F>(
        &self,
        family: RuntimeCacheFamily,
        cache: &mut BTreeMap<K, V>,
        estimate_entry_bytes: F,
    ) where
        K: Ord,
        F: Fn(&K, &V) -> usize,
    {
        let budget = self.runtime_cache_budget(family);
        let mut evictions = 0usize;

        if let Some(limit) = budget.max_entries {
            while cache.len() > limit {
                let _ = cache.pop_first();
                evictions = evictions.saturating_add(1);
            }
        }

        if let Some(max_bytes) = budget.max_bytes {
            let mut total_bytes = cache
                .iter()
                .map(|(key, value)| estimate_entry_bytes(key, value))
                .sum::<usize>();
            while total_bytes > max_bytes {
                let Some((key, value)) = cache.pop_first() else {
                    break;
                };
                total_bytes = total_bytes.saturating_sub(estimate_entry_bytes(&key, &value));
                evictions = evictions.saturating_add(1);
            }
        }

        if evictions > 0 {
            self.record_runtime_cache_event(family, RuntimeCacheEvent::Eviction, evictions);
        }
    }

    pub(super) fn runtime_cache_budget(&self, family: RuntimeCacheFamily) -> RuntimeCacheBudget {
        self.runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .policy(family)
            .map(|policy| policy.budget)
            .expect("runtime cache family policy should exist")
    }

    pub(super) fn cached_file_content_window(
        &self,
        cache_key: &FileContentWindowCacheKey,
    ) -> Option<Arc<FileContentSnapshot>> {
        let cached = self
            .cache_state
            .file_content_window_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key);
        self.record_runtime_cache_event(
            RuntimeCacheFamily::FileContentWindow,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn file_content_snapshot_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        canonical_path: &Path,
    ) -> Result<Arc<FileContentSnapshot>, ErrorData> {
        let freshness = self.repository_response_cache_freshness(
            std::slice::from_ref(workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )?;
        let Some(scopes) = freshness.scopes else {
            let bytes = fs::read(canonical_path).map_err(|err| {
                Self::internal(
                    format!("failed to read file {}: {err}", canonical_path.display()),
                    None,
                )
            })?;
            return Ok(Arc::new(FileContentSnapshot::from_bytes(bytes)));
        };
        let mut scoped_repository_ids = vec![workspace.repository_id.clone()];
        scoped_repository_ids.sort();
        let cache_key = FileContentWindowCacheKey {
            scoped_repository_ids,
            freshness_scopes: scopes,
            canonical_path: canonical_path.to_path_buf(),
        };
        if let Some(cached) = self.cached_file_content_window(&cache_key) {
            return Ok(cached);
        }

        let bytes = fs::read(canonical_path).map_err(|err| {
            Self::internal(
                format!("failed to read file {}: {err}", canonical_path.display()),
                None,
            )
        })?;
        let snapshot = Arc::new(FileContentSnapshot::from_bytes(bytes));
        self.cache_file_content_window(cache_key, Arc::clone(&snapshot));
        Ok(snapshot)
    }

    pub(super) fn cache_file_content_window(
        &self,
        cache_key: FileContentWindowCacheKey,
        snapshot: Arc<FileContentSnapshot>,
    ) {
        let mut cache = self
            .cache_state
            .file_content_window_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let budget = self.runtime_cache_budget(RuntimeCacheFamily::FileContentWindow);
        let (inserted, evictions) = cache.insert(cache_key, snapshot, budget);
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FileContentWindow,
                RuntimeCacheEvent::Insert,
                1,
            );
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FileContentWindow,
                RuntimeCacheEvent::Eviction,
                evictions,
            );
        } else {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::FileContentWindow,
                RuntimeCacheEvent::Bypass,
                1,
            );
        }
    }

    pub(super) fn invalidate_repository_file_content_cache(&self, repository_id: &str) {
        let mut cache = self
            .cache_state
            .file_content_window_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = cache.retain_repository(repository_id);
        self.record_runtime_cache_event(
            RuntimeCacheFamily::FileContentWindow,
            RuntimeCacheEvent::Invalidation,
            before,
        );
    }

    pub(super) fn runtime_cache_contract_summary(&self, families: &[RuntimeCacheFamily]) -> Value {
        let registry = self
            .runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let telemetry = self
            .runtime_state
            .runtime_cache_telemetry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        Value::Array(
            families
                .iter()
                .filter_map(|family| {
                    let policy = registry.policy(*family)?;
                    let counters = telemetry.get(family).copied().unwrap_or_default();
                    Some(json!({
                        "family": family.as_str(),
                        "residency": match policy.residency {
                            crate::mcp::server_cache::RuntimeCacheResidency::ProcessWide => "process_wide",
                            crate::mcp::server_cache::RuntimeCacheResidency::RequestLocal => "request_local",
                        },
                        "reuse_class": match policy.reuse_class {
                            crate::mcp::server_cache::RuntimeCacheReuseClass::SnapshotScopedReusable => "snapshot_scoped_reusable",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::QueryResultMicroCache => "query_result_micro_cache",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::ProcessMetadata => "process_metadata",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::RequestLocalOnly => "request_local_only",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::DeferredUntilReadOnly => "deferred_until_read_only",
                        },
                        "freshness_contract": match policy.freshness_contract {
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositorySnapshot => "repository_snapshot",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositoryFreshnessScopes => "repository_freshness_scopes",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositoryId => "repository_id",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::ExactInput => "exact_input",
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RequestLocal => "request_local",
                        },
                        "budget": {
                            "max_entries": policy.budget.max_entries,
                            "max_bytes": policy.budget.max_bytes,
                        },
                        "dirty_root_bypass": policy.dirty_root_bypass,
                        "telemetry": {
                            "hits": counters.hits,
                            "misses": counters.misses,
                            "bypasses": counters.bypasses,
                            "inserts": counters.inserts,
                            "evictions": counters.evictions,
                            "invalidations": counters.invalidations,
                        },
                    }))
                })
                .collect::<Vec<_>>(),
        )
    }

    #[cfg(test)]
    pub(super) fn runtime_cache_telemetry(
        &self,
        family: RuntimeCacheFamily,
    ) -> RuntimeCacheTelemetry {
        self.runtime_state
            .runtime_cache_telemetry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&family)
            .copied()
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn runtime_cache_policy(
        &self,
        family: RuntimeCacheFamily,
    ) -> crate::mcp::server_cache::RuntimeCacheFamilyPolicy {
        *self
            .runtime_state
            .runtime_cache_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .policy(family)
            .expect("runtime cache family policy should exist")
    }

    pub(super) fn prewarm_precise_graph_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Result<(), String> {
        let discovery = Self::collect_scip_artifact_digests(&workspace.root);
        if discovery.artifact_digests.is_empty() {
            return Ok(());
        }
        if self
            .try_reuse_latest_precise_graph_for_repository(
                &workspace.repository_id,
                &workspace.root,
            )
            .is_some()
        {
            return Ok(());
        }

        self.precise_graph_for_repository_root(
            &workspace.repository_id,
            &workspace.root,
            self.find_references_resource_budgets(),
        )
        .map(|_| ())
        .map_err(|err| err.message.to_string())
    }

    pub(super) fn runtime_status_workspace(&self) -> Option<AttachedWorkspace> {
        self.current_workspace().or_else(|| {
            self.attached_workspaces()
                .into_iter()
                .min_by(|left, right| left.repository_id.cmp(&right.repository_id))
                .or_else(|| {
                    self.known_workspaces()
                        .into_iter()
                        .min_by(|left, right| left.repository_id.cmp(&right.repository_id))
                })
        })
    }

    pub(super) fn runtime_recent_provenance_repository_id(payload_json: &str) -> Option<String> {
        let payload = serde_json::from_str::<Value>(payload_json).ok()?;
        payload
            .get("target_repository_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                payload
                    .get("source_refs")
                    .and_then(|source_refs| source_refs.get("repository_id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| {
                payload
                    .get("source_refs")
                    .and_then(|source_refs| source_refs.get("repository_ids"))
                    .and_then(Value::as_array)
                    .and_then(|ids| ids.first())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
    }

    pub(super) fn runtime_recent_provenance_summaries(&self) -> Vec<RecentProvenanceSummary> {
        let Some(workspace) = self.runtime_status_workspace() else {
            return Vec::new();
        };
        let storage = Storage::new(&workspace.db_path);
        match storage.load_recent_provenance_events(Self::RUNTIME_RECENT_PROVENANCE_LIMIT) {
            Ok(rows) => rows
                .into_iter()
                .map(|row| RecentProvenanceSummary {
                    trace_id: row.trace_id,
                    tool_name: row.tool_name,
                    created_at: row.created_at,
                    repository_id: Self::runtime_recent_provenance_repository_id(&row.payload_json),
                })
                .collect(),
            Err(err) => {
                warn!(
                    repository_id = workspace.repository_id,
                    error = %err,
                    "failed to load recent runtime provenance summaries"
                );
                Vec::new()
            }
        }
    }

    pub(super) fn runtime_status_summary(&self) -> RuntimeStatusSummary {
        let (active_tasks, recent_tasks) = {
            let registry = self
                .runtime_state
                .runtime_task_registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (registry.active_tasks(), registry.recent_tasks())
        };

        RuntimeStatusSummary {
            profile: self.runtime_state.runtime_profile,
            persistent_state_available: self
                .runtime_state
                .runtime_profile
                .persistent_state_available(),
            watch_active: self.runtime_state.runtime_watch_active,
            tool_surface_profile: self.tool_surface_profile.as_str().to_owned(),
            status_tool: "workspace_current".to_owned(),
            active_tasks,
            recent_tasks,
            recent_provenance: self.runtime_recent_provenance_summaries(),
        }
    }
}

/// Best-effort serialized size estimator for cached response values.
pub(super) fn serialized_value_estimated_bytes<T>(value: &T) -> usize
where
    T: Serialize,
{
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}
