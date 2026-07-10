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

    pub(super) fn runtime_text_searcher_with_repository_ids(
        &self,
        config: FriggConfig,
        repository_ids: Vec<String>,
    ) -> TextSearcher {
        self.runtime_text_searcher(config)
            .with_runtime_repository_ids(repository_ids)
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

    fn read_file_content_bytes_bounded(&self, canonical_path: &Path) -> Result<Vec<u8>, ErrorData> {
        let max_file_bytes = self.config.max_file_bytes;
        let metadata = fs::metadata(canonical_path).map_err(|err| {
            Self::internal(
                format!("failed to stat file {}: {err}", canonical_path.display()),
                None,
            )
        })?;
        let file_bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if file_bytes > max_file_bytes {
            return Err(Self::invalid_params(
                format!("file exceeds max_file_bytes={max_file_bytes}"),
                Some(json!({
                    "path": canonical_path.display().to_string(),
                    "bytes": file_bytes,
                    "max_file_bytes": max_file_bytes,
                })),
            ));
        }
        fs::read(canonical_path).map_err(|err| {
            Self::internal(
                format!("failed to read file {}: {err}", canonical_path.display()),
                None,
            )
        })
    }

    pub(super) fn file_content_snapshot_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        canonical_path: &Path,
    ) -> Result<Arc<FileContentSnapshot>, ErrorData> {
        let _freshness = self.repository_response_cache_freshness(
            std::slice::from_ref(workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )?;
        let bytes = self.read_file_content_bytes_bounded(canonical_path)?;
        Ok(Arc::new(FileContentSnapshot::from_bytes(bytes)))
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
                            crate::mcp::server_cache::RuntimeCacheReuseClass::ProcessMetadata => "process_metadata",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::RequestLocalOnly => "request_local_only",
                            crate::mcp::server_cache::RuntimeCacheReuseClass::DeferredUntilReadOnly => "deferred_until_read_only",
                        },
                        "freshness_contract": match policy.freshness_contract {
                            crate::mcp::server_cache::RuntimeCacheFreshnessContract::RepositorySnapshot => "repository_snapshot",
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
        let corpus = self
            .collect_repository_symbol_corpus(
                workspace.repository_id.clone(),
                workspace.runtime_repository_id.clone(),
                workspace.root.clone(),
            )
            .map_err(|err| err.message.to_string())?;

        self.precise_graph_for_corpus(corpus.as_ref(), self.find_references_resource_budgets())
            .map(|_| ())
            .map_err(|err| err.message.to_string())
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

        let mut tools_exposed = self.runtime_registered_tool_names();
        tools_exposed.sort();
        tools_exposed.dedup();

        let session_workspace = self.current_workspace();
        let watch_status = Some(self.watch_status_summary(session_workspace.as_ref(), &active_tasks));

        RuntimeStatusSummary {
            profile: self.runtime_state.runtime_profile,
            persistent_state_available: self
                .runtime_state
                .runtime_profile
                .persistent_state_available(),
            watch_active: self.runtime_state.runtime_watch_active,
            watch_status,
            tool_surface_profile: self.tool_surface_profile.as_str().to_owned(),
            tools_exposed,
            status_tool: "workspace".to_owned(),
            active_tasks,
            recent_tasks,
        }
    }

    /// Compact agent-facing watch projection from mode, leases, dual-class queue, and tasks.
    ///
    /// Does not dump raw `WatchEvent` history. Queue depth / dirty counts (EXP-hotpath-queue D)
    /// help choose `wait_watch` vs path-scoped live-disk; dual-class only (no third queue).
    ///
    /// Lease lookups use `runtime_repository_id` (watch supervisor key). The public
    /// `repository_id` field on the summary remains the stable agent-facing id.
    pub(super) fn watch_status_summary(
        &self,
        workspace: Option<&crate::mcp::workspace_registry::AttachedWorkspace>,
        active_tasks: &[crate::mcp::types::RuntimeTaskSummary],
    ) -> crate::mcp::types::WatchStatusSummary {
        use crate::mcp::types::{RuntimeTaskKind, RuntimeTaskStatus, WatchStatusReason, WatchStatusSummary};

        let public_repo_id = workspace.map(|ws| ws.repository_id.as_str());
        let runtime_repo_id = workspace.map(|ws| ws.runtime_repository_id.as_str());

        // Single lock: lease + dual-class queue snapshot (EXP-hotpath-queue D).
        let (
            lease_count,
            has_runtime,
            queue_depth,
            dirty_from_scheduler,
            oldest_age_ms,
            queue_pending,
            queue_in_flight,
        ) = {
            let guard = self
                .runtime_state
                .watch_runtime
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match (guard.as_ref(), runtime_repo_id) {
                (Some(runtime), Some(repo_id)) => {
                    let lease = runtime.lease_status(repo_id);
                    if let Some(snap) = runtime.queue_status(repo_id) {
                        let now = tokio::time::Instant::now();
                        let in_flight =
                            snap.manifest_fast_in_flight || snap.semantic_followup_in_flight;
                        let pending =
                            snap.manifest_fast_pending || snap.semantic_followup_pending;
                        (
                            lease.lease_count,
                            true,
                            Some(snap.refresh_queue_depth()),
                            Some(snap.dirty_path_hint_count),
                            snap.oldest_pending_age_ms(now),
                            pending && !in_flight,
                            in_flight,
                        )
                    } else {
                        (lease.lease_count, true, None, None, None, false, false)
                    }
                }
                (Some(_), None) => (0, true, None, None, None, false, false),
                (None, _) => (0, false, None, None, None, false, false),
            }
        };

        // Gate dirty oracle (precise pending) supplements scheduler dirty hints.
        let dirty_from_gate = public_repo_id.map(|id| {
            self.changed_paths_since_snapshot_for_gate(id).len()
        });
        let pending_dirty_path_count = match (dirty_from_scheduler, dirty_from_gate) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) if b > 0 => Some(b),
            (None, _) => None,
        };

        let queue_fields = |reason: WatchStatusReason,
                            lease_count: usize,
                            detail: Option<String>|
         -> WatchStatusSummary {
            WatchStatusSummary {
                reason,
                lease_count,
                repository_id: public_repo_id.map(ToOwned::to_owned),
                detail,
                refresh_queue_depth: queue_depth,
                pending_dirty_path_count,
                oldest_pending_age_ms: oldest_age_ms,
            }
        };

        if !self.runtime_state.runtime_watch_active {
            return queue_fields(
                WatchStatusReason::ModeOff,
                0,
                Some("watch mode disabled for this transport/profile".to_owned()),
            );
        }

        let task_matches_session = |task_repo: &str| -> bool {
            public_repo_id.is_some_and(|id| id == task_repo)
                || runtime_repo_id.is_some_and(|id| id == task_repo)
        };

        // Prefer in-flight refresh signal over lease absence (tasks may use runtime ids).
        let refresh_running = active_tasks.iter().any(|task| {
            task.status == RuntimeTaskStatus::Running
                && matches!(
                    task.kind,
                    RuntimeTaskKind::ChangedIndex | RuntimeTaskKind::SemanticRefresh
                )
                && (public_repo_id.is_none() && runtime_repo_id.is_none()
                    || task_matches_session(&task.repository_id))
        });

        if refresh_running || queue_in_flight {
            let detail = if refresh_running {
                "incremental refresh task running"
            } else {
                "dual-class refresh in flight"
            };
            return queue_fields(
                WatchStatusReason::Refreshing,
                lease_count,
                Some(detail.to_owned()),
            );
        }

        if !has_runtime {
            // Watch enabled for profile but supervisor handle not attached yet.
            return queue_fields(
                WatchStatusReason::NoLease,
                0,
                Some("watch runtime not started".to_owned()),
            );
        }

        if lease_count == 0 {
            return queue_fields(
                WatchStatusReason::NoLease,
                0,
                Some("no active watch lease for session repository".to_owned()),
            );
        }

        // Dual-class pending work, no in-flight task → debouncing/queued (not a third class).
        if queue_pending {
            return queue_fields(
                WatchStatusReason::Debouncing,
                lease_count,
                Some("dual-class watch queue has pending work".to_owned()),
            );
        }

        queue_fields(WatchStatusReason::Active, lease_count, None)
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
