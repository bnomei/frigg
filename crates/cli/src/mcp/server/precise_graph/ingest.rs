use super::*;

impl FriggMcpServer {
    pub(in crate::mcp::server) fn scip_candidate_directories(root: &Path) -> [PathBuf; 1] {
        [root.join(".frigg/scip")]
    }

    pub(in crate::mcp::server) fn system_time_to_unix_nanos(
        system_time: SystemTime,
    ) -> Option<u64> {
        system_time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
    }

    pub(in crate::mcp::server) fn root_signature(file_digests: &[FileMetadataDigest]) -> String {
        let mut hasher = DeterministicSignatureHasher::new();
        for digest in file_digests {
            hasher.write_str(&digest.path.to_string_lossy());
            hasher.write_u64(digest.size_bytes);
            hasher.write_optional_u64(digest.mtime_ns);
        }
        hasher.finish_hex()
    }

    pub(in crate::mcp::server) fn scip_signature(
        artifact_digests: &[ScipArtifactDigest],
    ) -> String {
        let mut hasher = DeterministicSignatureHasher::new();
        for artifact in artifact_digests {
            hasher.write_str(&artifact.path.to_string_lossy());
            hasher.write_str(artifact.format.as_str());
            hasher.write_u64(artifact.size_bytes);
            hasher.write_optional_u64(artifact.mtime_ns);
        }
        hasher.finish_hex()
    }

    pub(in crate::mcp::server) fn collect_scip_artifact_digests(
        root: &Path,
    ) -> ScipArtifactDiscovery {
        let mut artifacts = Vec::new();
        let mut candidate_directories = Vec::new();
        let mut candidate_directory_digests = Vec::new();
        for directory in Self::scip_candidate_directories(root) {
            candidate_directories.push(directory.display().to_string());
            let directory_metadata = fs::metadata(&directory).ok();
            let directory_mtime_ns = directory_metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(Self::system_time_to_unix_nanos);
            candidate_directory_digests.push(ScipCandidateDirectoryDigest {
                path: directory.clone(),
                exists: directory_metadata.is_some(),
                mtime_ns: directory_mtime_ns,
            });
            let read_dir = match fs::read_dir(&directory) {
                Ok(read_dir) => read_dir,
                Err(_) => continue,
            };

            for entry in read_dir {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                let path = entry.path();
                let Some(format) = ScipArtifactFormat::from_path(&path) else {
                    continue;
                };
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if !metadata.is_file() {
                    continue;
                }
                let mtime_ns = metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_unix_nanos);
                artifacts.push(ScipArtifactDigest {
                    path,
                    format,
                    size_bytes: metadata.len(),
                    mtime_ns,
                });
            }
        }

        artifacts.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.size_bytes.cmp(&right.size_bytes))
                .then(left.mtime_ns.cmp(&right.mtime_ns))
        });
        artifacts.dedup_by(|left, right| left.path == right.path);
        ScipArtifactDiscovery {
            candidate_directories,
            candidate_directory_digests,
            artifact_digests: artifacts,
        }
    }

    fn ingest_precise_artifacts_for_repository(
        graph: &mut SymbolGraph,
        workspace_root: &Path,
        repository_id: &str,
        discovery: &ScipArtifactDiscovery,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<PreciseIngestStats, ErrorData> {
        let precise_config = Self::load_workspace_precise_config(workspace_root);
        let ingest_matcher = Self::compile_workspace_precise_exclude_matcher(
            workspace_root,
            &precise_config.ingest_excludes,
        );
        let artifact_digests = discovery
            .artifact_digests
            .iter()
            .filter(|digest| {
                !Self::workspace_precise_excludes_path(
                    workspace_root,
                    &digest.path,
                    ingest_matcher.as_ref(),
                    false,
                )
            })
            .collect::<Vec<_>>();
        let discovered_bytes = artifact_digests
            .iter()
            .fold(0u64, |acc, digest| acc.saturating_add(digest.size_bytes));
        let mut stats = PreciseIngestStats {
            candidate_directories: discovery.candidate_directories.clone(),
            discovered_artifacts: artifact_digests
                .iter()
                .take(Self::PRECISE_DISCOVERY_SAMPLE_LIMIT)
                .map(|digest| digest.path.display().to_string())
                .collect(),
            artifacts_discovered: artifact_digests.len(),
            artifacts_discovered_bytes: discovered_bytes,
            ..PreciseIngestStats::default()
        };
        let max_artifacts = Self::usize_to_u64(budgets.scip_max_artifacts);
        if stats.artifacts_discovered > budgets.scip_max_artifacts {
            return Err(Self::find_references_resource_budget_error(
                "scip",
                "scip_artifact_count",
                "find_references SCIP artifact count exceeds configured budget",
                json!({
                    "repository_id": repository_id,
                    "actual": Self::usize_to_u64(stats.artifacts_discovered),
                    "limit": max_artifacts,
                }),
            ));
        }

        let max_artifact_bytes = Self::usize_to_u64(budgets.scip_max_artifact_bytes);
        let max_total_bytes = Self::usize_to_u64(budgets.scip_max_total_bytes);
        if discovered_bytes > max_total_bytes {
            warn!(
                repository_id,
                discovered_bytes,
                max_total_bytes,
                "scip discovery bytes exceed configured budget; precise ingest may degrade to heuristic fallback"
            );
        }

        let started_at = Instant::now();
        let max_elapsed = Duration::from_millis(budgets.scip_max_elapsed_ms);
        let mut processed_artifacts = 0usize;
        let mut processed_bytes = 0u64;

        for artifact_digest in artifact_digests {
            if started_at.elapsed() > max_elapsed {
                let elapsed_ms =
                    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                warn!(
                    repository_id,
                    actual_elapsed_ms = elapsed_ms,
                    limit_elapsed_ms = budgets.scip_max_elapsed_ms,
                    processed_artifacts,
                    bytes_processed = processed_bytes,
                    "scip processing exceeded time budget; degrading precise path to heuristic fallback"
                );
                Self::push_precise_failure_sample(
                    &mut stats,
                    "<scip-processing-budget>".to_owned(),
                    "ingest_budget_elapsed_ms",
                    format!(
                        "scip processing elapsed_ms={} exceeded limit={} after processing {} artifacts and {} bytes",
                        elapsed_ms,
                        budgets.scip_max_elapsed_ms,
                        processed_artifacts,
                        processed_bytes
                    ),
                );
                break;
            }

            if artifact_digest.size_bytes > max_artifact_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    actual_bytes = artifact_digest.size_bytes,
                    limit_bytes = max_artifact_bytes,
                    "skipping scip artifact that exceeds per-file byte budget"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes = stats
                    .artifacts_failed_bytes
                    .saturating_add(artifact_digest.size_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "artifact_budget_bytes",
                    format!(
                        "artifact bytes {} exceed configured per-file limit {}",
                        artifact_digest.size_bytes, max_artifact_bytes
                    ),
                );
                continue;
            }
            let projected_processed_bytes =
                processed_bytes.saturating_add(artifact_digest.size_bytes);
            if projected_processed_bytes > max_total_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    projected_processed_bytes,
                    limit_bytes = max_total_bytes,
                    "skipping scip artifact because cumulative byte budget would be exceeded"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes = stats
                    .artifacts_failed_bytes
                    .saturating_add(artifact_digest.size_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "artifact_budget_total_bytes",
                    format!(
                        "projected cumulative bytes {} exceed configured total limit {}",
                        projected_processed_bytes, max_total_bytes
                    ),
                );
                continue;
            }
            processed_bytes = projected_processed_bytes;

            let payload = match fs::read(&artifact_digest.path) {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(
                        repository_id,
                        path = %artifact_digest.path.display(),
                        error = %err,
                        "failed to read scip artifact payload while resolving references"
                    );
                    stats.artifacts_failed += 1;
                    stats.artifacts_failed_bytes = stats
                        .artifacts_failed_bytes
                        .saturating_add(artifact_digest.size_bytes);
                    Self::push_precise_failure_sample(
                        &mut stats,
                        artifact_digest.path.display().to_string(),
                        "read_payload",
                        err.to_string(),
                    );
                    continue;
                }
            };
            let payload_bytes = Self::usize_to_u64(payload.len());
            if payload_bytes > max_artifact_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    actual_bytes = payload_bytes,
                    limit_bytes = max_artifact_bytes,
                    "skipping scip artifact payload that exceeds per-file byte budget after read"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes =
                    stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "payload_budget_bytes",
                    format!(
                        "payload bytes {} exceed configured per-file limit {}",
                        payload_bytes, max_artifact_bytes
                    ),
                );
                continue;
            }

            let artifact_label = artifact_digest.path.to_string_lossy().into_owned();
            let ingest_result = match artifact_digest.format {
                ScipArtifactFormat::Json => graph.overlay_scip_json_with_budgets(
                    repository_id,
                    &artifact_label,
                    &payload,
                    ScipResourceBudgets {
                        max_payload_bytes: budgets.scip_max_artifact_bytes,
                        max_documents: budgets.scip_max_documents_per_artifact,
                        max_elapsed_ms: budgets.scip_max_elapsed_ms,
                    },
                ),
                ScipArtifactFormat::Protobuf => graph.overlay_scip_protobuf_with_budgets(
                    repository_id,
                    &artifact_label,
                    &payload,
                    ScipResourceBudgets {
                        max_payload_bytes: budgets.scip_max_artifact_bytes,
                        max_documents: budgets.scip_max_documents_per_artifact,
                        max_elapsed_ms: budgets.scip_max_elapsed_ms,
                    },
                ),
            };
            match ingest_result {
                Ok(_) => {
                    stats.artifacts_ingested += 1;
                    stats.artifacts_ingested_bytes =
                        stats.artifacts_ingested_bytes.saturating_add(payload_bytes);
                }
                Err(err) => {
                    if let ScipIngestError::ResourceBudgetExceeded { diagnostic } = &err {
                        warn!(
                            repository_id,
                            path = %artifact_digest.path.display(),
                            budget_code = diagnostic.code.as_str(),
                            actual = diagnostic.actual,
                            limit = diagnostic.limit,
                            detail = %diagnostic.message,
                            "scip ingest exceeded resource budget; degrading precise path to heuristic fallback"
                        );
                        stats.artifacts_failed += 1;
                        stats.artifacts_failed_bytes =
                            stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                        Self::push_precise_failure_sample(
                            &mut stats,
                            artifact_digest.path.display().to_string(),
                            &format!("ingest_budget_{}", diagnostic.code.as_str()),
                            format!(
                                "ingest budget {} exceeded (actual={}, limit={}): {}",
                                diagnostic.code.as_str(),
                                diagnostic.actual,
                                diagnostic.limit,
                                diagnostic.message
                            ),
                        );
                        continue;
                    }
                    warn!(
                        repository_id,
                        path = %artifact_digest.path.display(),
                        error = %err,
                        "failed to ingest scip artifact while resolving references"
                    );
                    stats.artifacts_failed += 1;
                    stats.artifacts_failed_bytes =
                        stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                    Self::push_precise_failure_sample(
                        &mut stats,
                        artifact_digest.path.display().to_string(),
                        "ingest_payload",
                        err.to_string(),
                    );
                }
            }
            processed_artifacts = processed_artifacts.saturating_add(1);
        }

        Ok(stats)
    }

    fn try_reuse_cached_precise_graph(
        &self,
        corpus: &RepositorySymbolCorpus,
    ) -> Option<CachedPreciseGraph> {
        let cached = self
            .cache_state
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&corpus.repository_id)
            .cloned()?;
        if cached.corpus_signature != corpus.root_signature {
            return None;
        }
        if !Self::cached_scip_discovery_is_current(&corpus.root, &cached.discovery) {
            return None;
        }
        Some((*cached).clone())
    }

    pub(in crate::mcp::server) fn try_reuse_latest_precise_graph_for_repository(
        &self,
        repository_id: &str,
        root: &Path,
    ) -> Option<CachedPreciseGraph> {
        let current_root_signature =
            Self::current_root_signature_for_repository(root, repository_id)?;
        let cached = self
            .cache_state
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(repository_id)
            .cloned()?;
        if cached.corpus_signature != current_root_signature {
            return None;
        }
        if !Self::cached_scip_discovery_is_current(root, &cached.discovery) {
            return None;
        }
        Some((*cached).clone())
    }

    fn cached_scip_discovery_is_current(root: &Path, discovery: &ScipArtifactDiscovery) -> bool {
        let expected_directories = Self::scip_candidate_directories(root);
        if discovery.candidate_directory_digests.len() != expected_directories.len() {
            return false;
        }

        for (expected_path, cached_digest) in expected_directories
            .iter()
            .zip(discovery.candidate_directory_digests.iter())
        {
            if cached_digest.path != *expected_path {
                return false;
            }
            let metadata = fs::metadata(expected_path).ok();
            let exists = metadata.is_some();
            let mtime_ns = metadata
                .as_ref()
                .and_then(|value| value.modified().ok())
                .and_then(Self::system_time_to_unix_nanos);
            if cached_digest.exists != exists || cached_digest.mtime_ns != mtime_ns {
                return false;
            }
        }

        discovery.artifact_digests.iter().all(|artifact| {
            let metadata = match fs::metadata(&artifact.path) {
                Ok(metadata) => metadata,
                Err(_) => return false,
            };
            metadata.is_file()
                && metadata.len() == artifact.size_bytes
                && metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_unix_nanos)
                    == artifact.mtime_ns
        })
    }

    pub(in crate::mcp::server) fn precise_graph_for_corpus(
        &self,
        corpus: &RepositorySymbolCorpus,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<CachedPreciseGraph, ErrorData> {
        if let Some(cached) = self.try_reuse_cached_precise_graph(corpus) {
            return Ok(cached);
        }

        let discovery = Self::collect_scip_artifact_digests(&corpus.root);
        let scip_signature = Self::scip_signature(&discovery.artifact_digests);
        let cache_key = PreciseGraphCacheKey {
            repository_id: corpus.repository_id.clone(),
            scip_signature: scip_signature.clone(),
            corpus_signature: corpus.root_signature.clone(),
        };

        if let Some(cached) = self
            .cache_state
            .precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            self.cache_state
                .latest_precise_graph_cache
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(corpus.repository_id.clone(), cached.clone());
            return Ok((*cached).clone());
        }

        let mut graph = SymbolGraph::default();
        register_symbol_definitions(&mut graph, &corpus.repository_id, &corpus.symbols);
        Self::register_php_declaration_relations(&mut graph, corpus);
        Self::register_php_target_evidence_relations(&mut graph, corpus);
        Self::register_blade_relation_evidence(&mut graph, corpus);
        let ingest_stats = Self::ingest_precise_artifacts_for_repository(
            &mut graph,
            &corpus.root,
            &corpus.repository_id,
            &discovery,
            budgets,
        )?;
        let coverage_mode = Self::precise_coverage_mode(&ingest_stats);
        if coverage_mode == PreciseCoverageMode::Partial {
            warn!(
                repository_id = corpus.repository_id,
                artifacts_ingested = ingest_stats.artifacts_ingested,
                artifacts_failed = ingest_stats.artifacts_failed,
                "retaining partial precise graph because some SCIP artifacts ingested successfully"
            );
        }
        if coverage_mode == PreciseCoverageMode::None && ingest_stats.artifacts_failed > 0 {
            warn!(
                repository_id = corpus.repository_id,
                artifacts_ingested = ingest_stats.artifacts_ingested,
                artifacts_failed = ingest_stats.artifacts_failed,
                "precise graph has no usable artifact data after SCIP ingest failures"
            );
        }
        let cached_graph = CachedPreciseGraph {
            graph: Arc::new(graph),
            ingest_stats,
            corpus_signature: corpus.root_signature.clone(),
            discovery: discovery.clone(),
            coverage_mode,
        };

        let mut cache = self
            .cache_state
            .precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != corpus.repository_id
                || (key.scip_signature == scip_signature
                    && key.corpus_signature == corpus.root_signature)
        });
        let cached_graph = Arc::new(cached_graph);
        cache.insert(cache_key, cached_graph.clone());
        self.cache_state
            .latest_precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(corpus.repository_id.clone(), cached_graph.clone());

        Ok((*cached_graph).clone())
    }

    pub(in crate::mcp::server) fn precise_graph_for_repository_root(
        &self,
        repository_id: &str,
        root: &Path,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<CachedPreciseGraph, ErrorData> {
        if let Some(cached) =
            self.try_reuse_latest_precise_graph_for_repository(repository_id, root)
        {
            return Ok(cached);
        }

        let discovery = Self::collect_scip_artifact_digests(root);
        let current_root_signature =
            Self::current_root_signature_for_repository(root, repository_id).ok_or_else(|| {
                Self::internal(
                    "failed to compute current root signature for precise graph",
                    Some(json!({
                        "repository_id": repository_id,
                        "root": root.display().to_string(),
                    })),
                )
            })?;
        let scip_signature = Self::scip_signature(&discovery.artifact_digests);
        let cache_key = PreciseGraphCacheKey {
            repository_id: repository_id.to_owned(),
            scip_signature: scip_signature.clone(),
            corpus_signature: current_root_signature.clone(),
        };

        if let Some(cached) = self
            .cache_state
            .precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            self.cache_state
                .latest_precise_graph_cache
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(repository_id.to_owned(), cached.clone());
            return Ok((*cached).clone());
        }

        let mut graph = SymbolGraph::default();
        let ingest_stats = Self::ingest_precise_artifacts_for_repository(
            &mut graph,
            root,
            repository_id,
            &discovery,
            budgets,
        )?;
        let coverage_mode = Self::precise_coverage_mode(&ingest_stats);
        let cached_graph = CachedPreciseGraph {
            graph: Arc::new(graph),
            ingest_stats,
            corpus_signature: current_root_signature,
            discovery,
            coverage_mode,
        };

        let mut cache = self
            .cache_state
            .precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != repository_id
                || (key.scip_signature == scip_signature
                    && key.corpus_signature == cached_graph.corpus_signature)
        });
        let cached_graph = Arc::new(cached_graph);
        cache.insert(cache_key, cached_graph.clone());
        self.cache_state
            .latest_precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(repository_id.to_owned(), cached_graph.clone());

        Ok((*cached_graph).clone())
    }
}
