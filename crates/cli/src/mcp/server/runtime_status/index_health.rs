use super::*;

impl FriggMcpServer {
    fn response_freshness_cache_eligible(&self, workspaces: &[AttachedWorkspace]) -> bool {
        !workspaces.is_empty()
            && workspaces
                .iter()
                .all(|workspace| self.repository_has_active_watch_lease(&workspace.repository_id))
    }

    pub(in crate::mcp::server) fn invalidate_repository_response_freshness_cache(
        &self,
        repository_id: &str,
    ) {
        self.cache_state
            .repository_response_freshness_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| {
                !key.scoped_repository_ids
                    .iter()
                    .any(|candidate| candidate == repository_id)
            });
    }

    fn cached_repository_response_freshness(
        &self,
        scoped_repository_ids: &[String],
        mode: RepositoryResponseCacheFreshnessMode,
    ) -> Option<RepositoryResponseCacheFreshness> {
        let cache_key = RepositoryResponseFreshnessCacheKey {
            scoped_repository_ids: scoped_repository_ids.to_vec(),
            mode: mode.as_str(),
        };
        let cache = self
            .cache_state
            .repository_response_freshness_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.get(&cache_key)?;
        (entry.generated_at.elapsed() <= Self::REPOSITORY_RESPONSE_FRESHNESS_CACHE_TTL)
            .then(|| entry.freshness.clone())
    }

    fn cache_repository_response_freshness(
        &self,
        scoped_repository_ids: &[String],
        mode: RepositoryResponseCacheFreshnessMode,
        freshness: &RepositoryResponseCacheFreshness,
    ) {
        let cache_key = RepositoryResponseFreshnessCacheKey {
            scoped_repository_ids: scoped_repository_ids.to_vec(),
            mode: mode.as_str(),
        };
        let mut cache = self
            .cache_state
            .repository_response_freshness_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(
            cache_key,
            CachedRepositoryResponseFreshness {
                freshness: freshness.clone(),
                generated_at: Instant::now(),
            },
        );
        while cache.len() > Self::REPOSITORY_RESPONSE_FRESHNESS_CACHE_MAX_ENTRIES {
            let _ = cache.pop_first();
        }
    }

    pub(in crate::mcp::server) fn workspace_index_health_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexHealthSummary {
        let scip_discovery = Self::collect_scip_artifact_digests(&workspace.root);
        WorkspaceIndexHealthSummary {
            lexical: self.workspace_lexical_index_summary(workspace, storage),
            semantic: self.workspace_semantic_index_summary(workspace, storage),
            scip: self.workspace_scip_index_summary_from_discovery(&scip_discovery),
            precise_ingest: Some(
                self.workspace_precise_ingest_summary_from_discovery(workspace, &scip_discovery),
            ),
            precise_generators: self.workspace_precise_generator_summaries(workspace),
        }
    }

    fn public_precise_coverage_mode(
        coverage_mode: PreciseCoverageMode,
    ) -> WorkspacePreciseCoverageMode {
        match coverage_mode {
            PreciseCoverageMode::Full => WorkspacePreciseCoverageMode::Full,
            PreciseCoverageMode::Partial => WorkspacePreciseCoverageMode::Partial,
            PreciseCoverageMode::None => WorkspacePreciseCoverageMode::None,
        }
    }

    fn precise_ingest_failure_summaries(
        stats: &PreciseIngestStats,
    ) -> Vec<WorkspacePreciseArtifactFailureSummary> {
        stats
            .failed_artifacts
            .iter()
            .map(|failure| WorkspacePreciseArtifactFailureSummary {
                artifact_label: failure.artifact_label.clone(),
                stage: failure.stage.clone(),
                detail: failure.detail.clone(),
            })
            .collect()
    }

    fn precise_ingest_reason(
        state: WorkspacePreciseIngestState,
        stats: &PreciseIngestStats,
    ) -> Option<String> {
        match state {
            WorkspacePreciseIngestState::Missing => Some("no_scip_artifacts_discovered".to_owned()),
            WorkspacePreciseIngestState::Ready => None,
            WorkspacePreciseIngestState::Partial => Some(format!(
                "scip ingest partial: {} of {} artifacts failed",
                stats.artifacts_failed, stats.artifacts_discovered
            )),
            WorkspacePreciseIngestState::Failed => Some(format!(
                "scip ingest failed: {} of {} artifacts failed",
                stats.artifacts_failed, stats.artifacts_discovered
            )),
            WorkspacePreciseIngestState::Error => None,
        }
    }

    fn precise_ingest_summary_from_stats(
        stats: &PreciseIngestStats,
        coverage_mode: PreciseCoverageMode,
    ) -> WorkspacePreciseIngestSummary {
        let state = if stats.artifacts_discovered == 0 {
            WorkspacePreciseIngestState::Missing
        } else {
            match coverage_mode {
                PreciseCoverageMode::Full => WorkspacePreciseIngestState::Ready,
                PreciseCoverageMode::Partial => WorkspacePreciseIngestState::Partial,
                PreciseCoverageMode::None => {
                    if stats.artifacts_failed > 0 {
                        WorkspacePreciseIngestState::Failed
                    } else {
                        WorkspacePreciseIngestState::Missing
                    }
                }
            }
        };
        WorkspacePreciseIngestSummary {
            state,
            coverage_mode: Some(Self::public_precise_coverage_mode(coverage_mode)),
            reason: Self::precise_ingest_reason(state, stats),
            artifacts_discovered: stats.artifacts_discovered,
            artifacts_discovered_bytes: stats.artifacts_discovered_bytes,
            artifacts_ingested: stats.artifacts_ingested,
            artifacts_ingested_bytes: stats.artifacts_ingested_bytes,
            artifacts_failed: stats.artifacts_failed,
            artifacts_failed_bytes: stats.artifacts_failed_bytes,
            failed_artifacts: Self::precise_ingest_failure_summaries(stats),
        }
    }

    fn workspace_precise_ingest_summary_from_discovery(
        &self,
        workspace: &AttachedWorkspace,
        discovery: &ScipArtifactDiscovery,
    ) -> WorkspacePreciseIngestSummary {
        if discovery.artifact_digests.is_empty() {
            return WorkspacePreciseIngestSummary {
                state: WorkspacePreciseIngestState::Missing,
                coverage_mode: Some(WorkspacePreciseCoverageMode::None),
                reason: Some("no_scip_artifacts_discovered".to_owned()),
                artifacts_discovered: 0,
                artifacts_discovered_bytes: 0,
                artifacts_ingested: 0,
                artifacts_ingested_bytes: 0,
                artifacts_failed: 0,
                artifacts_failed_bytes: 0,
                failed_artifacts: Vec::new(),
            };
        }

        if let Some(cached) = self.try_reuse_latest_precise_graph_for_repository(
            &workspace.repository_id,
            &workspace.root,
        ) {
            return Self::precise_ingest_summary_from_stats(
                &cached.ingest_stats,
                cached.coverage_mode,
            );
        }

        match self.precise_graph_for_repository_root_with_discovery(
            &workspace.repository_id,
            &workspace.root,
            discovery,
            self.find_references_resource_budgets(),
        ) {
            Ok(cached) => {
                Self::precise_ingest_summary_from_stats(&cached.ingest_stats, cached.coverage_mode)
            }
            Err(err) => WorkspacePreciseIngestSummary {
                state: WorkspacePreciseIngestState::Error,
                coverage_mode: None,
                reason: Some(err.message.to_string()),
                artifacts_discovered: discovery.artifact_digests.len(),
                artifacts_discovered_bytes: discovery
                    .artifact_digests
                    .iter()
                    .map(|artifact| artifact.size_bytes)
                    .sum(),
                artifacts_ingested: 0,
                artifacts_ingested_bytes: 0,
                artifacts_failed: 0,
                artifacts_failed_bytes: 0,
                failed_artifacts: Vec::new(),
            },
        }
    }

    pub(in crate::mcp::server) fn workspace_repository_freshness_status(
        &self,
        workspace: &AttachedWorkspace,
        semantic_runtime: &SemanticRuntimeConfig,
    ) -> Result<crate::manifest_validation::RepositoryFreshnessStatus, String> {
        if !workspace.db_path.is_file() {
            return Ok(crate::manifest_validation::RepositoryFreshnessStatus {
                snapshot_id: None,
                manifest_entry_count: None,
                manifest: RepositoryManifestFreshness::MissingSnapshot,
                semantic: if semantic_runtime.enabled {
                    RepositorySemanticFreshness::MissingManifestSnapshot
                } else {
                    RepositorySemanticFreshness::Disabled
                },
                validated_manifest_digests: None,
                semantic_target: None,
            });
        }

        let storage = Storage::new(&workspace.db_path);
        if matches!(storage.schema_version(), Ok(0)) {
            return Ok(crate::manifest_validation::RepositoryFreshnessStatus {
                snapshot_id: None,
                manifest_entry_count: None,
                manifest: RepositoryManifestFreshness::MissingSnapshot,
                semantic: if semantic_runtime.enabled {
                    RepositorySemanticFreshness::MissingManifestSnapshot
                } else {
                    RepositorySemanticFreshness::Disabled
                },
                validated_manifest_digests: None,
                semantic_target: None,
            });
        }

        repository_freshness_status(
            &storage,
            &workspace.runtime_repository_id,
            &workspace.root,
            semantic_runtime,
            |_| false,
        )
        .map_err(|err| err.to_string())
    }

    pub(in crate::mcp::server) fn repository_response_cache_freshness(
        &self,
        workspaces: &[AttachedWorkspace],
        mode: RepositoryResponseCacheFreshnessMode,
    ) -> Result<RepositoryResponseCacheFreshness, ErrorData> {
        let mut scoped_repository_ids = workspaces
            .iter()
            .map(|workspace| workspace.repository_id.clone())
            .collect::<Vec<_>>();
        scoped_repository_ids.sort();
        let cache_eligible = self.response_freshness_cache_eligible(workspaces);
        if cache_eligible
            && let Some(cached) =
                self.cached_repository_response_freshness(&scoped_repository_ids, mode)
        {
            return Ok(cached);
        }

        let semantic_runtime = self.cache_freshness_runtime(mode);
        let mut cacheable = true;
        let mut scopes = Vec::with_capacity(workspaces.len());
        let mut repositories = Vec::with_capacity(workspaces.len());

        for workspace in workspaces {
            let status = self
                .workspace_repository_freshness_status(workspace, &semantic_runtime)
                .map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to compute response cache freshness for repository '{}': {err}",
                            workspace.repository_id
                        ),
                        None,
                    )
                })?;
            let dirty_root = self
                .runtime_state
                .validated_manifest_candidate_cache
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_dirty_root(&workspace.root);

            let manifest = Self::repository_manifest_freshness_label(&status.manifest);
            let semantic = Self::repository_semantic_freshness_label(&status.semantic);
            let snapshot_id = status.snapshot_id.clone();
            let semantic_target = status.semantic_target.clone();

            repositories.push(json!({
                "repository_id": workspace.repository_id,
                "snapshot_id": snapshot_id,
                "manifest": manifest,
                "semantic": semantic,
                "dirty_root": dirty_root,
                "provider": semantic_target.as_ref().map(|target| target.provider.clone()),
                "model": semantic_target.as_ref().map(|target| target.model.clone()),
            }));

            if dirty_root || !matches!(status.manifest, RepositoryManifestFreshness::Ready) {
                cacheable = false;
                continue;
            }
            let Some(snapshot_id) = status.snapshot_id else {
                cacheable = false;
                continue;
            };

            scopes.push(RepositoryFreshnessCacheScope {
                repository_id: workspace.repository_id.clone(),
                snapshot_id,
                semantic_state: matches!(mode, RepositoryResponseCacheFreshnessMode::SemanticAware)
                    .then(|| semantic.to_owned()),
                semantic_provider: matches!(
                    mode,
                    RepositoryResponseCacheFreshnessMode::SemanticAware
                )
                .then(|| {
                    semantic_target
                        .as_ref()
                        .map(|target| target.provider.clone())
                })
                .flatten(),
                semantic_model: matches!(mode, RepositoryResponseCacheFreshnessMode::SemanticAware)
                    .then(|| semantic_target.as_ref().map(|target| target.model.clone()))
                    .flatten(),
            });
        }

        scopes.sort();

        let freshness = RepositoryResponseCacheFreshness {
            scopes: cacheable.then_some(scopes),
            basis: json!({
                "mode": mode.as_str(),
                "cacheable": cacheable,
                "repositories": repositories,
                "runtime_cache_contract": self.runtime_cache_contract_summary(&[
                    crate::mcp::server_cache::RuntimeCacheFamily::ValidatedManifestCandidate,
                    crate::mcp::server_cache::RuntimeCacheFamily::SearchTextResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::SearchHybridResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::SearchSymbolResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::GoToDefinitionResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::FindDeclarationsResponse,
                    ]),
            }),
        };
        if cache_eligible && freshness.scopes.is_some() {
            self.cache_repository_response_freshness(&scoped_repository_ids, mode, &freshness);
        }
        Ok(freshness)
    }

    fn cache_freshness_runtime(
        &self,
        mode: RepositoryResponseCacheFreshnessMode,
    ) -> SemanticRuntimeConfig {
        let mut runtime = self.config.semantic_runtime.clone();
        if matches!(mode, RepositoryResponseCacheFreshnessMode::ManifestOnly) {
            runtime.enabled = false;
        }
        runtime
    }

    fn repository_manifest_freshness_label(
        freshness: &RepositoryManifestFreshness,
    ) -> &'static str {
        match freshness {
            RepositoryManifestFreshness::MissingSnapshot => "missing_snapshot",
            RepositoryManifestFreshness::StaleSnapshot => "stale_snapshot",
            RepositoryManifestFreshness::Ready => "ready",
        }
    }

    fn repository_semantic_freshness_label(
        freshness: &RepositorySemanticFreshness,
    ) -> &'static str {
        match freshness {
            RepositorySemanticFreshness::Disabled => "disabled",
            RepositorySemanticFreshness::MissingManifestSnapshot => "missing_manifest_snapshot",
            RepositorySemanticFreshness::StaleManifestSnapshot => "stale_manifest_snapshot",
            RepositorySemanticFreshness::NoEligibleEntries => "no_eligible_entries",
            RepositorySemanticFreshness::MissingForActiveModel => "missing_for_active_model",
            RepositorySemanticFreshness::Ready => "ready",
        }
    }

    pub(in crate::mcp::server) fn workspace_manifest_entry_count(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Option<usize> {
        let db_path = resolve_provenance_db_path(&workspace.root).ok()?;
        if db_path.exists() {
            let storage = Storage::new(db_path.clone());
            if let Some(snapshot) =
                crate::manifest_validation::latest_validated_manifest_snapshot_shared(
                    &storage,
                    &workspace.runtime_repository_id,
                    &workspace.root,
                    Some(&self.runtime_state.validated_manifest_candidate_cache),
                )
            {
                return Some(snapshot.digests.len());
            }
        }

        Self::load_latest_manifest_snapshot(&workspace.root, &workspace.runtime_repository_id)
            .map(|snapshot| snapshot.entries.len())
    }

    pub(in crate::mcp::server) fn workspace_lexical_index_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexComponentSummary {
        if let Some(summary) = Self::storage_error_health_summary(storage) {
            return summary;
        }

        let mut manifest_only_runtime = self.config.semantic_runtime.clone();
        manifest_only_runtime.enabled = false;
        let freshness =
            match self.workspace_repository_freshness_status(workspace, &manifest_only_runtime) {
                Ok(freshness) => freshness,
                Err(err) => {
                    return WorkspaceIndexComponentSummary {
                        state: WorkspaceIndexComponentState::Error,
                        reason: Some(err),
                        snapshot_id: None,
                        compatible_snapshot_id: None,
                        provider: None,
                        model: None,
                        artifact_count: None,
                    };
                }
            };
        let manifest_entry_count = self.workspace_manifest_entry_count(workspace);
        let dirty_root = self.workspace_has_dirty_root(workspace);
        if dirty_root && freshness.snapshot_id.is_some() {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Stale,
                reason: Some("dirty_root".to_owned()),
                snapshot_id: freshness.snapshot_id,
                compatible_snapshot_id: None,
                provider: None,
                model: None,
                artifact_count: manifest_entry_count
                    .or_else(|| freshness.validated_manifest_digests.as_ref().map(Vec::len)),
            };
        }
        let manifest_state = freshness.manifest.clone();
        let (state, reason) = match manifest_state {
            RepositoryManifestFreshness::MissingSnapshot => (
                WorkspaceIndexComponentState::Missing,
                Some("missing_manifest_snapshot".to_owned()),
            ),
            RepositoryManifestFreshness::StaleSnapshot => (
                WorkspaceIndexComponentState::Stale,
                Some("stale_manifest_snapshot".to_owned()),
            ),
            RepositoryManifestFreshness::Ready => (WorkspaceIndexComponentState::Ready, None),
        };
        WorkspaceIndexComponentSummary {
            state,
            reason,
            snapshot_id: freshness.snapshot_id,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: match freshness.manifest {
                RepositoryManifestFreshness::MissingSnapshot => None,
                RepositoryManifestFreshness::StaleSnapshot => manifest_entry_count,
                RepositoryManifestFreshness::Ready => manifest_entry_count
                    .or_else(|| freshness.validated_manifest_digests.as_ref().map(Vec::len)),
            },
        }
    }

    pub(in crate::mcp::server) fn workspace_semantic_index_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexComponentSummary {
        if !self.config.semantic_runtime.enabled {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Disabled,
                reason: Some("semantic_runtime_disabled".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider: None,
                model: None,
                artifact_count: None,
            };
        }

        let provider = self
            .config
            .semantic_runtime
            .provider
            .map(|value| value.as_str().to_owned());
        let model = self
            .config
            .semantic_runtime
            .normalized_model()
            .map(ToOwned::to_owned);
        if self.config.semantic_runtime.validate().is_err() || provider.is_none() || model.is_none()
        {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Error,
                reason: Some("semantic_runtime_invalid_config".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            };
        }
        if let Some(summary) = Self::storage_error_health_summary(storage) {
            return WorkspaceIndexComponentSummary {
                provider,
                model,
                ..summary
            };
        }

        let freshness = match self
            .workspace_repository_freshness_status(workspace, &self.config.semantic_runtime)
        {
            Ok(freshness) => freshness,
            Err(err) => {
                return WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Error,
                    reason: Some(err),
                    snapshot_id: None,
                    compatible_snapshot_id: None,
                    provider,
                    model,
                    artifact_count: None,
                };
            }
        };
        let storage_reader = Storage::new(&workspace.db_path);
        let provider_ref = provider
            .as_deref()
            .expect("semantic provider should exist after config validation");
        let model_ref = model
            .as_deref()
            .expect("semantic model should exist after config validation");
        let semantic_health = storage_reader
            .collect_semantic_storage_health_for_repository_model(
                &workspace.runtime_repository_id,
                provider_ref,
                model_ref,
            )
            .ok();
        let semantic_state = freshness.semantic.clone();
        match semantic_state {
            RepositorySemanticFreshness::MissingManifestSnapshot => {
                WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Missing,
                    reason: Some("missing_manifest_snapshot".to_owned()),
                    snapshot_id: None,
                    compatible_snapshot_id: None,
                    provider,
                    model,
                    artifact_count: None,
                }
            }
            RepositorySemanticFreshness::StaleManifestSnapshot => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Stale,
                reason: Some("stale_manifest_snapshot".to_owned()),
                snapshot_id: freshness.snapshot_id,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            },
            RepositorySemanticFreshness::NoEligibleEntries => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Ready,
                reason: Some("manifest_valid_no_semantic_eligible_entries".to_owned()),
                snapshot_id: freshness.snapshot_id,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: Some(0),
            },
            RepositorySemanticFreshness::Ready => {
                let snapshot_id = freshness
                    .snapshot_id
                    .expect("ready semantic freshness should carry a snapshot id");
                if semantic_health
                    .as_ref()
                    .is_some_and(|health| !health.vector_consistent)
                {
                    return WorkspaceIndexComponentSummary {
                        state: WorkspaceIndexComponentState::Error,
                        reason: Some("semantic_vector_partition_out_of_sync".to_owned()),
                        snapshot_id: Some(snapshot_id),
                        compatible_snapshot_id: None,
                        provider: provider.clone(),
                        model: model.clone(),
                        artifact_count: semantic_health
                            .as_ref()
                            .map(|health| health.live_embedding_rows),
                    };
                }
                WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Ready,
                    reason: None,
                    snapshot_id: Some(snapshot_id.clone()),
                    compatible_snapshot_id: None,
                    provider: provider.clone(),
                    model: model.clone(),
                    artifact_count: semantic_health
                        .as_ref()
                        .map(|health| health.live_embedding_rows)
                        .or_else(|| {
                            storage_reader
                                .count_semantic_embeddings_for_repository_snapshot_model(
                                    &workspace.runtime_repository_id,
                                    &snapshot_id,
                                    provider_ref,
                                    model_ref,
                                )
                                .ok()
                        }),
                }
            }
            RepositorySemanticFreshness::MissingForActiveModel => {
                let snapshot_id = freshness.snapshot_id.clone();
                WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Missing,
                    reason: Some("semantic_snapshot_missing_for_active_model".to_owned()),
                    snapshot_id,
                    compatible_snapshot_id: None,
                    provider: provider.clone(),
                    model: model.clone(),
                    artifact_count: None,
                }
            }
            RepositorySemanticFreshness::Disabled => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Disabled,
                reason: Some("semantic_runtime_disabled".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            },
        }
    }

    fn workspace_scip_index_summary_from_discovery(
        &self,
        discovery: &ScipArtifactDiscovery,
    ) -> WorkspaceIndexComponentSummary {
        let artifact_count = discovery.artifact_digests.len();
        WorkspaceIndexComponentSummary {
            state: if artifact_count == 0 {
                WorkspaceIndexComponentState::Missing
            } else {
                WorkspaceIndexComponentState::Ready
            },
            reason: if artifact_count == 0 {
                Some("no_scip_artifacts_discovered".to_owned())
            } else {
                None
            },
            snapshot_id: None,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: Some(artifact_count),
        }
    }

    pub(in crate::mcp::server) fn workspace_semantic_refresh_plan(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Option<WorkspaceSemanticRefreshPlan> {
        if !self.config.semantic_runtime.enabled {
            return None;
        }

        self.config.semantic_runtime.validate().ok()?;
        let freshness = self
            .workspace_repository_freshness_status(workspace, &self.config.semantic_runtime)
            .ok()?;
        let latest_snapshot_id = freshness.snapshot_id?;
        match (freshness.manifest.clone(), freshness.semantic.clone()) {
            (RepositoryManifestFreshness::StaleSnapshot, _) => Some(WorkspaceSemanticRefreshPlan {
                latest_snapshot_id,
                reason: "stale_manifest_snapshot",
            }),
            (
                RepositoryManifestFreshness::Ready,
                RepositorySemanticFreshness::MissingForActiveModel,
            ) => Some(WorkspaceSemanticRefreshPlan {
                latest_snapshot_id,
                reason: "semantic_snapshot_missing_for_active_model",
            }),
            _ => None,
        }
    }

    pub(in crate::mcp::server) fn refresh_workspace_semantic_snapshot_with_plan(
        &self,
        workspace: &AttachedWorkspace,
        plan: &WorkspaceSemanticRefreshPlan,
    ) -> Result<(), String> {
        let started_at = Instant::now();
        tracing::info!(
            repository_id = %workspace.repository_id,
            root = %workspace.root.display(),
            snapshot_id = %plan.latest_snapshot_id,
            reason = plan.reason,
            provider = self
                .config
                .semantic_runtime
                .provider
                .map(|value| value.as_str())
                .unwrap_or(""),
            model = self
                .config
                .semantic_runtime
                .normalized_model()
                .unwrap_or(""),
            "workspace semantic refresh started"
        );
        let credentials = SemanticRuntimeCredentials::from_process_env();
        self.config
            .semantic_runtime
            .validate_startup(&credentials)
            .map_err(|err| err.to_string())?;

        reindex_repository_with_runtime_config(
            &workspace.runtime_repository_id,
            &workspace.root,
            &workspace.db_path,
            ReindexMode::Full,
            &self.config.semantic_runtime,
            &credentials,
        )
        .map(|_| {
            tracing::info!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                snapshot_id = %plan.latest_snapshot_id,
                reason = plan.reason,
                duration_ms = started_at.elapsed().as_millis() as u64,
                "workspace semantic refresh completed"
            );
        })
        .map_err(|err| {
            let error = err.to_string();
            warn!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                snapshot_id = %plan.latest_snapshot_id,
                reason = plan.reason,
                duration_ms = started_at.elapsed().as_millis() as u64,
                error = %error,
                "workspace semantic refresh failed"
            );
            error
        })
    }

    pub(in crate::mcp::server) fn maybe_refresh_workspace_semantic_snapshot(
        &self,
        workspace: &AttachedWorkspace,
    ) {
        let Some(plan) = self.workspace_semantic_refresh_plan(workspace) else {
            return;
        };
        if plan.reason != "semantic_snapshot_missing_for_active_model" {
            return;
        }
        if self
            .runtime_state
            .runtime_task_registry
            .read()
            .expect("runtime task registry poisoned")
            .has_active_task_for_repository(
                crate::mcp::types::RuntimeTaskKind::SemanticRefresh,
                &workspace.repository_id,
            )
        {
            return;
        }
        if let Err(err) = self.refresh_workspace_semantic_snapshot_with_plan(workspace, &plan) {
            warn!(
                repository_id = workspace.repository_id,
                snapshot_id = %plan.latest_snapshot_id,
                reason = plan.reason,
                error = %err,
                "workspace semantic refresh failed during attach"
            );
        }
    }

    pub(in crate::mcp::server) fn maybe_spawn_workspace_runtime_prewarm(
        &self,
        workspace: &AttachedWorkspace,
    ) {
        let semantic_plan = self.workspace_semantic_refresh_plan(workspace);
        let should_refresh_semantic = semantic_plan
            .as_ref()
            .is_some_and(|plan| plan.reason == "stale_manifest_snapshot");
        let should_prewarm_precise = !Self::collect_scip_artifact_digests(&workspace.root)
            .artifact_digests
            .is_empty();
        if !should_refresh_semantic && !should_prewarm_precise {
            return;
        }

        let semantic_refresh_already_running = should_refresh_semantic
            && self
                .runtime_state
                .runtime_task_registry
                .read()
                .expect("runtime task registry poisoned")
                .has_active_task_for_repository(
                    crate::mcp::types::RuntimeTaskKind::SemanticRefresh,
                    &workspace.repository_id,
                );

        if should_refresh_semantic && !semantic_refresh_already_running {
            let server = self.clone();
            let workspace = workspace.clone();
            let semantic_plan = semantic_plan.clone();
            let task_id = self
                .runtime_state
                .runtime_task_registry
                .write()
                .expect("runtime task registry poisoned")
                .start_task(
                    crate::mcp::types::RuntimeTaskKind::SemanticRefresh,
                    workspace.repository_id.clone(),
                    "semantic_attach_refresh",
                    semantic_plan.as_ref().map(|plan| {
                        format!(
                            "attach root {} snapshot {} reason {}",
                            workspace.root.display(),
                            plan.latest_snapshot_id,
                            plan.reason
                        )
                    }),
                );
            let task_registry = Arc::clone(&self.runtime_state.runtime_task_registry);
            let task_id_for_thread = task_id.clone();
            let spawn_result = std::thread::Builder::new()
                .name(format!(
                    "frigg-semantic-refresh-{}",
                    workspace.repository_id
                ))
                .spawn(move || {
                    let result = semantic_plan
                        .as_ref()
                        .ok_or_else(|| "missing semantic refresh plan".to_owned())
                        .and_then(|plan| {
                            server.refresh_workspace_semantic_snapshot_with_plan(&workspace, plan)
                        });
                    let (status, detail) = match result {
                        Ok(()) => (crate::mcp::types::RuntimeTaskStatus::Succeeded, None),
                        Err(err) => {
                            warn!(
                                repository_id = workspace.repository_id,
                                error = %err,
                                "workspace semantic refresh failed during runtime prewarm"
                            );
                            (crate::mcp::types::RuntimeTaskStatus::Failed, Some(err))
                        }
                    };
                    task_registry
                        .write()
                        .expect("runtime task registry poisoned")
                        .finish_task(&task_id_for_thread, status, detail);
                });
            if let Err(err) = spawn_result {
                self.runtime_state
                    .runtime_task_registry
                    .write()
                    .expect("runtime task registry poisoned")
                    .finish_task(
                        &task_id,
                        crate::mcp::types::RuntimeTaskStatus::Failed,
                        Some(format!("failed to spawn semantic prewarm thread: {err}")),
                    );
            }
        }

        if should_prewarm_precise {
            let server = self.clone();
            let workspace = workspace.clone();
            let task_id = self
                .runtime_state
                .runtime_task_registry
                .write()
                .expect("runtime task registry poisoned")
                .start_task(
                    crate::mcp::types::RuntimeTaskKind::PrecisePrewarm,
                    workspace.repository_id.clone(),
                    "precise_attach_prewarm",
                    Some(format!("attach root {}", workspace.root.display())),
                );
            let task_registry = Arc::clone(&self.runtime_state.runtime_task_registry);
            let task_id_for_thread = task_id.clone();
            let spawn_result = std::thread::Builder::new()
                .name(format!("frigg-precise-prewarm-{}", workspace.repository_id))
                .spawn(move || {
                    let result = server.prewarm_precise_graph_for_workspace(&workspace);
                    let (status, detail) = match result {
                        Ok(()) => (crate::mcp::types::RuntimeTaskStatus::Succeeded, None),
                        Err(err) => {
                            warn!(
                                repository_id = workspace.repository_id,
                                error = %err,
                                "failed to prewarm precise graph during workspace attach"
                            );
                            (crate::mcp::types::RuntimeTaskStatus::Failed, Some(err))
                        }
                    };
                    task_registry
                        .write()
                        .expect("runtime task registry poisoned")
                        .finish_task(&task_id_for_thread, status, detail);
                });
            if let Err(err) = spawn_result {
                self.runtime_state
                    .runtime_task_registry
                    .write()
                    .expect("runtime task registry poisoned")
                    .finish_task(
                        &task_id,
                        crate::mcp::types::RuntimeTaskStatus::Failed,
                        Some(format!("failed to spawn precise prewarm thread: {err}")),
                    );
            }
        }
    }

    pub(in crate::mcp::server) fn maybe_spawn_workspace_precise_generation_for_paths(
        &self,
        workspace: &AttachedWorkspace,
        changed_paths: &[String],
        deleted_paths: &[String],
    ) -> WorkspacePreciseGenerationAction {
        self.maybe_spawn_workspace_precise_generation(workspace, changed_paths, deleted_paths)
    }

    pub(in crate::mcp::server) fn workspace_precise_summary_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        generation_action: Option<WorkspacePreciseGenerationAction>,
    ) -> WorkspacePreciseSummary {
        let storage = Self::workspace_storage_summary(workspace);
        let health = self.workspace_index_health_summary(workspace, &storage);
        let default_action =
            generation_action.or(Some(WorkspacePreciseGenerationAction::NotApplicable));

        if let Some(precise_ingest) = health.precise_ingest.as_ref() {
            let failure_summary = precise_ingest.reason.clone().or_else(|| {
                precise_ingest.failed_artifacts.first().map(|failure| {
                    format!(
                        "{} [{}]: {}",
                        failure.artifact_label, failure.stage, failure.detail
                    )
                })
            });
            match precise_ingest.state {
                WorkspacePreciseIngestState::Ready => {
                    return WorkspacePreciseSummary {
                        state: WorkspacePreciseState::Ok,
                        failure_tool: None,
                        failure_class: None,
                        failure_summary: None,
                        recommended_action: None,
                        generation_action: default_action,
                    };
                }
                WorkspacePreciseIngestState::Partial => {
                    return WorkspacePreciseSummary {
                        state: WorkspacePreciseState::Partial,
                        failure_tool: None,
                        failure_class: None,
                        failure_summary,
                        recommended_action: Some(WorkspaceRecommendedAction::UseHeuristicMode),
                        generation_action: default_action,
                    };
                }
                WorkspacePreciseIngestState::Failed | WorkspacePreciseIngestState::Error => {
                    return WorkspacePreciseSummary {
                        state: WorkspacePreciseState::Failed,
                        failure_tool: None,
                        failure_class: None,
                        failure_summary,
                        recommended_action: Some(WorkspaceRecommendedAction::UseHeuristicMode),
                        generation_action: default_action,
                    };
                }
                WorkspacePreciseIngestState::Missing => {}
            }
        }

        let failed_generator =
            health.precise_generators.iter().find(|generator| {
                generator.last_generation.as_ref().is_some_and(|summary| {
                    summary.status == WorkspacePreciseGenerationStatus::Failed
                }) || generator.state == WorkspacePreciseGeneratorState::Error
            });
        if let Some(generator) = failed_generator {
            let last_generation = generator.last_generation.as_ref();
            let failure_class = last_generation.and_then(|summary| summary.failure_class);
            let failure_detail = last_generation
                .and_then(|summary| summary.detail.as_deref())
                .or(generator.reason.as_deref());
            return WorkspacePreciseSummary {
                state: if health.scip.state == WorkspaceIndexComponentState::Stale {
                    WorkspacePreciseState::Partial
                } else {
                    WorkspacePreciseState::Failed
                },
                failure_tool: generator.tool.clone(),
                failure_class,
                failure_summary: Self::concise_precise_failure_summary(
                    generator.tool.as_deref(),
                    failure_class,
                    failure_detail,
                ),
                recommended_action: last_generation
                    .and_then(|summary| summary.recommended_action)
                    .or(Some(WorkspaceRecommendedAction::UseHeuristicMode)),
                generation_action: default_action,
            };
        }

        let missing_tool_generator = health.precise_generators.iter().find(|generator| {
            generator.state == WorkspacePreciseGeneratorState::MissingTool
                || generator.last_generation.as_ref().is_some_and(|summary| {
                    summary.status == WorkspacePreciseGenerationStatus::MissingTool
                })
        });
        if let Some(generator) = missing_tool_generator {
            let failure_detail = generator.reason.as_deref().or_else(|| {
                generator
                    .last_generation
                    .as_ref()
                    .and_then(|summary| summary.detail.as_deref())
            });
            return WorkspacePreciseSummary {
                state: WorkspacePreciseState::Unavailable,
                failure_tool: generator.tool.clone(),
                failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                failure_summary: Self::concise_precise_failure_summary(
                    generator.tool.as_deref(),
                    Some(WorkspacePreciseFailureClass::MissingTool),
                    failure_detail,
                ),
                recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                generation_action: default_action,
            };
        }

        WorkspacePreciseSummary {
            state: WorkspacePreciseState::Unavailable,
            failure_tool: None,
            failure_class: None,
            failure_summary: health
                .precise_ingest
                .as_ref()
                .and_then(|summary| summary.reason.clone())
                .or_else(|| health.scip.reason.clone()),
            recommended_action: Some(WorkspaceRecommendedAction::UseHeuristicMode),
            generation_action: default_action,
        }
    }

    pub(in crate::mcp::server) fn storage_error_health_summary(
        storage: &WorkspaceStorageSummary,
    ) -> Option<WorkspaceIndexComponentSummary> {
        let (state, reason) = match storage.index_state {
            WorkspaceStorageIndexState::MissingDb => (
                WorkspaceIndexComponentState::Missing,
                Some("missing_db".to_owned()),
            ),
            WorkspaceStorageIndexState::Uninitialized => (
                WorkspaceIndexComponentState::Missing,
                Some(if storage.initialized {
                    "missing_manifest_snapshot".to_owned()
                } else {
                    "uninitialized_db".to_owned()
                }),
            ),
            WorkspaceStorageIndexState::Ready => return None,
            WorkspaceStorageIndexState::Error => (
                WorkspaceIndexComponentState::Error,
                storage
                    .error
                    .clone()
                    .or_else(|| Some("storage_error".to_owned())),
            ),
        };
        Some(WorkspaceIndexComponentSummary {
            state,
            reason,
            snapshot_id: None,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: None,
        })
    }
}
