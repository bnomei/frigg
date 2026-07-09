//! Symbol corpus construction and caching for navigation-oriented MCP tools.
//!
//! Repository corpora are expensive enough to cache across requests, but they are now bounded by
//! manifest token and global entry count so long-running servers do not retain every historical
//! snapshot forever.

use super::*;
use crate::languages::analyze_rust_indexed_source;
use rayon::prelude::*;

const SYMBOL_CORPUS_CACHE_MAX_ENTRIES: usize = 16;
const SYMBOL_CORPUS_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

impl FriggMcpServer {
    fn container_symbol_index_by_index(symbols: &[SymbolDefinition]) -> Vec<Option<usize>> {
        symbols
            .iter()
            .enumerate()
            .map(|(symbol_index, symbol)| {
                symbols
                    .iter()
                    .enumerate()
                    .filter(|(candidate_index, candidate)| {
                        *candidate_index != symbol_index
                            && candidate.path == symbol.path
                            && Self::source_span_strictly_contains(&candidate.span, &symbol.span)
                    })
                    .min_by(|(_, left), (_, right)| {
                        let left_span = left.span.end_line.saturating_sub(left.span.start_line);
                        let right_span = right.span.end_line.saturating_sub(right.span.start_line);
                        let left_column_span = if left_span == 0 {
                            left.span.end_column.saturating_sub(left.span.start_column)
                        } else {
                            usize::MAX
                        };
                        let right_column_span = if right_span == 0 {
                            right
                                .span
                                .end_column
                                .saturating_sub(right.span.start_column)
                        } else {
                            usize::MAX
                        };
                        left_span
                            .cmp(&right_span)
                            .then(left_column_span.cmp(&right_column_span))
                            .then(left.line.cmp(&right.line))
                            .then(left.stable_id.cmp(&right.stable_id))
                    })
                    .map(|(container_index, _)| container_index)
            })
            .collect()
    }

    pub(super) fn invalidate_repository_symbol_corpus_cache(&self, repository_id: &str) {
        self.cache_state
            .symbol_corpus_cache_epoch
            .fetch_add(1, Ordering::Relaxed);
        self.cache_state
            .symbol_corpus_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| key.repository_id != repository_id);
    }

    fn trim_symbol_corpus_cache(
        cache: &mut BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>,
    ) {
        Self::trim_symbol_corpus_cache_to_limits(
            cache,
            SYMBOL_CORPUS_CACHE_MAX_ENTRIES,
            SYMBOL_CORPUS_CACHE_MAX_BYTES,
        );
    }

    fn symbol_corpus_cache_entry_bytes(
        key: &SymbolCorpusCacheKey,
        corpus: &RepositorySymbolCorpus,
    ) -> usize {
        key.repository_id.len() + key.manifest_token.len() + corpus.estimated_heap_bytes()
    }

    fn symbol_corpus_cache_total_bytes(
        cache: &BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>,
    ) -> usize {
        cache
            .iter()
            .map(|(key, corpus)| Self::symbol_corpus_cache_entry_bytes(key, corpus))
            .sum()
    }

    fn trim_symbol_corpus_cache_to_limits(
        cache: &mut BTreeMap<SymbolCorpusCacheKey, Arc<RepositorySymbolCorpus>>,
        max_entries: usize,
        max_bytes: usize,
    ) {
        while cache.len() > max_entries {
            let _ = cache.pop_first();
        }
        while !cache.is_empty() && Self::symbol_corpus_cache_total_bytes(cache) > max_bytes {
            let _ = cache.pop_first();
        }
    }

    pub(super) fn collect_repository_symbol_corpus(
        &self,
        repository_id: String,
        runtime_repository_id: String,
        root: PathBuf,
    ) -> Result<Arc<RepositorySymbolCorpus>, ErrorData> {
        let cache_epoch = self
            .cache_state
            .symbol_corpus_cache_epoch
            .load(Ordering::Relaxed);
        let mut diagnostics = RepositoryDiagnosticsSummary::default();
        let mut manifest_output = None;
        let mut source_paths = None;
        let dirty_root = self
            .runtime_state
            .validated_manifest_candidate_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_dirty_root(&root);
        let can_trust_validated_manifest_cache = !dirty_root
            && self.repository_has_active_watch_lease(&repository_id)
            && !self.repository_has_active_runtime_work(&repository_id);
        let can_reuse_dirty_live_corpus =
            dirty_root && self.repository_has_active_watch_lease(&repository_id);
        let trusted_snapshot = if can_trust_validated_manifest_cache {
            Self::load_latest_cached_validated_manifest_snapshot_shared(
                &root,
                &runtime_repository_id,
                &self.runtime_state.validated_manifest_candidate_cache,
            )?
        } else {
            None
        };
        let validated_snapshot = match trusted_snapshot {
            Some(snapshot) => Some(snapshot),
            None => Self::load_latest_validated_manifest_snapshot_shared(
                &root,
                &runtime_repository_id,
                Some(&self.runtime_state.validated_manifest_candidate_cache),
            )?,
        };
        let (file_digests, manifest_token) = match validated_snapshot {
            Some(snapshot) => {
                let snapshot_source_paths =
                    Self::manifest_source_paths_for_digests(snapshot.digests.as_ref());
                source_paths = Some(snapshot_source_paths);
                (
                    snapshot.digests,
                    format!("snapshot:{}", snapshot.snapshot_id),
                )
            }
            None => {
                let live_output = ManifestBuilder::default()
                    .build_metadata_with_diagnostics(&root)
                    .map_err(Self::map_frigg_error)?;
                let live_signature = Self::root_signature(&live_output.entries);
                let live_manifest_token = format!("live:{live_signature}");
                if can_reuse_dirty_live_corpus {
                    let live_cache_key = SymbolCorpusCacheKey {
                        repository_id: repository_id.clone(),
                        manifest_token: live_manifest_token.clone(),
                    };
                    if let Some(cached) = self
                        .cache_state
                        .symbol_corpus_cache
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .get(&live_cache_key)
                        .cloned()
                    {
                        return Ok(cached);
                    }
                }
                manifest_output = Some(live_output);
                (
                    Arc::new(
                        manifest_output
                            .as_ref()
                            .expect("live manifest output just assigned")
                            .entries
                            .clone(),
                    ),
                    live_manifest_token,
                )
            }
        };
        if let Some(manifest_output) = &manifest_output {
            for manifest_diagnostic in &manifest_output.diagnostics {
                match manifest_diagnostic.kind {
                    ManifestDiagnosticKind::Walk => diagnostics.manifest_walk_count += 1,
                    ManifestDiagnosticKind::Read => diagnostics.manifest_read_count += 1,
                }
            }
        }
        let root_signature = Self::root_signature(file_digests.as_ref());
        let cache_key = SymbolCorpusCacheKey {
            repository_id: repository_id.clone(),
            manifest_token: manifest_token.clone(),
        };

        if let Some(cached) = self
            .cache_state
            .symbol_corpus_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(cached);
        }

        let mut source_paths = source_paths.unwrap_or_else(|| {
            file_digests
                .iter()
                .map(|digest| digest.path.clone())
                .filter(|path| {
                    supported_language_for_path(path, LanguageCapability::SymbolCorpus).is_some()
                })
                .collect::<Vec<_>>()
        });
        source_paths.sort();

        let SymbolExtractionOutput {
            symbols,
            diagnostics: symbol_diagnostics,
        } = extract_symbols_for_paths(&source_paths);
        diagnostics.symbol_extraction_count = symbol_diagnostics.len();
        let symbols_by_relative_path = Self::symbols_by_relative_path(&root, &symbols);
        let symbol_index_by_stable_id = Self::symbol_index_by_stable_id(&symbols);
        let symbol_indices_by_name = Self::symbol_indices_by_name(&symbols);
        let symbol_indices_by_lower_name = Self::symbol_indices_by_lower_name(&symbols);
        let mut rust_symbol_context_by_index = vec![None; symbols.len()];
        let mut rust_implementation_facts = Vec::new();
        let mut php_evidence_by_relative_path = BTreeMap::new();
        let mut blade_evidence_by_relative_path = BTreeMap::new();
        let mut canonical_symbol_name_by_stable_id = BTreeMap::new();

        for path in &source_paths {
            let relative_path = Self::relative_display_path(&root, path);
            let file_symbol_indices = symbols_by_relative_path
                .get(&relative_path)
                .cloned()
                .unwrap_or_default();
            if file_symbol_indices.is_empty() {
                continue;
            }
            if !Self::navigation_path_within_root(&root, path) {
                continue;
            }
            let Ok(source) = fs::read_to_string(path) else {
                continue;
            };
            match supported_language_for_path(path, LanguageCapability::SymbolCorpus) {
                Some(SymbolLanguage::Rust) => {
                    let analysis =
                        analyze_rust_indexed_source(&source, &symbols, &file_symbol_indices);
                    for (symbol_index, context) in analysis.symbol_contexts_by_index {
                        if let Some(slot) = rust_symbol_context_by_index.get_mut(symbol_index) {
                            *slot = Some(context);
                        }
                    }
                    rust_implementation_facts.extend(analysis.implementation_facts);
                }
                Some(SymbolLanguage::Php) => {
                    let file_symbols = file_symbol_indices
                        .iter()
                        .map(|index| symbols[*index].clone())
                        .collect::<Vec<_>>();
                    let Ok(evidence) =
                        extract_php_source_evidence_from_source(path, &source, &file_symbols)
                    else {
                        continue;
                    };
                    canonical_symbol_name_by_stable_id
                        .extend(evidence.canonical_names_by_stable_id.clone());
                    php_evidence_by_relative_path.insert(relative_path, evidence);
                }
                Some(SymbolLanguage::Blade) => {
                    let file_symbols = file_symbol_indices
                        .iter()
                        .map(|index| symbols[*index].clone())
                        .collect::<Vec<_>>();
                    let mut evidence =
                        extract_blade_source_evidence_from_source(&source, &file_symbols);
                    mark_local_flux_overlays(&mut evidence, &symbols, &symbol_indices_by_name);
                    blade_evidence_by_relative_path.insert(relative_path, evidence);
                }
                _ => {}
            }
        }
        let symbol_indices_by_canonical_name = Self::symbol_indices_by_canonical_name(
            &symbol_index_by_stable_id,
            &canonical_symbol_name_by_stable_id,
        );
        let symbol_indices_by_lower_canonical_name = Self::symbol_indices_by_lower_canonical_name(
            &symbol_index_by_stable_id,
            &canonical_symbol_name_by_stable_id,
        );
        let container_symbol_index_by_index = Self::container_symbol_index_by_index(&symbols);

        let corpus = Arc::new(RepositorySymbolCorpus {
            repository_id: repository_id.clone(),
            runtime_repository_id,
            root,
            root_signature: root_signature.clone(),
            source_paths,
            symbols,
            container_symbol_index_by_index,
            symbols_by_relative_path,
            symbol_index_by_stable_id,
            symbol_indices_by_name,
            symbol_indices_by_lower_name,
            canonical_symbol_name_by_stable_id,
            symbol_indices_by_canonical_name,
            symbol_indices_by_lower_canonical_name,
            rust_symbol_context_by_index,
            rust_implementation_facts,
            php_evidence_by_relative_path,
            blade_evidence_by_relative_path,
            diagnostics,
        });

        let mut cache = self
            .cache_state
            .symbol_corpus_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self
            .cache_state
            .symbol_corpus_cache_epoch
            .load(Ordering::Relaxed)
            == cache_epoch
        {
            cache.retain(|key, _| {
                key.repository_id != repository_id || key.manifest_token == manifest_token
            });
            cache.insert(cache_key, corpus.clone());
            Self::trim_symbol_corpus_cache(&mut cache);
        }

        Ok(corpus)
    }

    pub(super) fn load_latest_manifest_snapshot(
        root: &Path,
        repository_id: &str,
    ) -> Option<crate::storage::RepositoryManifestSnapshot> {
        let db_path = resolve_provenance_db_path(root).ok()?;
        if !db_path.exists() {
            return None;
        }
        let storage = Storage::new(db_path);
        storage
            .load_latest_manifest_for_repository(repository_id)
            .ok()?
    }

    fn load_latest_validated_manifest_snapshot_shared(
        root: &Path,
        repository_id: &str,
        cache: Option<
            &std::sync::Arc<
                std::sync::RwLock<crate::manifest_validation::ValidatedManifestCandidateCache>,
            >,
        >,
    ) -> Result<Option<crate::manifest_validation::SharedValidatedManifestSnapshot>, ErrorData>
    {
        let db_path = resolve_provenance_db_path(root).map_err(Self::map_frigg_error)?;
        if !db_path.exists() {
            return Ok(None);
        }
        let storage = Storage::new(db_path);
        crate::manifest_validation::latest_validated_manifest_snapshot_shared(
            &storage,
            repository_id,
            root,
            cache,
        )
        .map_err(Self::map_frigg_error)
    }

    fn load_latest_cached_validated_manifest_snapshot_shared(
        root: &Path,
        repository_id: &str,
        cache: &std::sync::Arc<
            std::sync::RwLock<crate::manifest_validation::ValidatedManifestCandidateCache>,
        >,
    ) -> Result<Option<crate::manifest_validation::SharedValidatedManifestSnapshot>, ErrorData>
    {
        let db_path = resolve_provenance_db_path(root).map_err(Self::map_frigg_error)?;
        if !db_path.exists() {
            return Ok(None);
        }
        let storage = Storage::new(db_path);
        crate::manifest_validation::latest_cached_validated_manifest_snapshot_shared(
            &storage,
            repository_id,
            root,
            cache,
        )
        .map_err(Self::map_frigg_error)
    }

    pub(super) fn current_root_signature_for_repository(
        root: &Path,
        repository_id: &str,
    ) -> Result<String, ErrorData> {
        match Self::load_latest_validated_manifest_snapshot_shared(root, repository_id, None) {
            Ok(Some(snapshot)) => return Ok(Self::root_signature(snapshot.digests.as_ref())),
            Ok(None) => {}
            Err(err) => return Err(err),
        }

        ManifestBuilder::default()
            .build_metadata_with_diagnostics(root)
            .map(|output| Self::root_signature(&output.entries))
            .map_err(Self::map_frigg_error)
    }

    pub(super) fn manifest_source_paths_for_digests(
        file_digests: &[FileMetadataDigest],
    ) -> Vec<PathBuf> {
        let mut source_paths = Vec::new();
        for digest in file_digests {
            if supported_language_for_path(&digest.path, LanguageCapability::SymbolCorpus).is_some()
            {
                source_paths.push(digest.path.clone());
            }
        }
        source_paths
    }

    pub(super) fn symbols_by_relative_path(
        root: &Path,
        symbols: &[SymbolDefinition],
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbols_by_relative_path = BTreeMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbols_by_relative_path
                .entry(Self::relative_display_path(root, &symbol.path))
                .or_insert_with(Vec::new)
                .push(index);
        }
        for indices in symbols_by_relative_path.values_mut() {
            indices.sort_by(|left, right| {
                symbols[*left]
                    .line
                    .cmp(&symbols[*right].line)
                    .then(
                        symbols[*left]
                            .span
                            .start_column
                            .cmp(&symbols[*right].span.start_column),
                    )
                    .then(symbols[*left].stable_id.cmp(&symbols[*right].stable_id))
            });
        }
        symbols_by_relative_path
    }

    pub(super) fn symbol_index_by_stable_id(
        symbols: &[SymbolDefinition],
    ) -> BTreeMap<String, usize> {
        symbols
            .iter()
            .enumerate()
            .map(|(index, symbol)| (symbol.stable_id.clone(), index))
            .collect()
    }

    pub(super) fn symbol_indices_by_name(
        symbols: &[SymbolDefinition],
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_name = BTreeMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbol_indices_by_name
                .entry(symbol.name.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }
        symbol_indices_by_name
    }

    pub(super) fn symbol_indices_by_lower_name(
        symbols: &[SymbolDefinition],
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_lower_name = BTreeMap::new();
        for (index, symbol) in symbols.iter().enumerate() {
            symbol_indices_by_lower_name
                .entry(symbol.name.to_ascii_lowercase())
                .or_insert_with(Vec::new)
                .push(index);
        }
        symbol_indices_by_lower_name
    }

    pub(super) fn symbol_indices_by_canonical_name(
        symbol_index_by_stable_id: &BTreeMap<String, usize>,
        canonical_symbol_name_by_stable_id: &BTreeMap<String, String>,
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_canonical_name = BTreeMap::new();
        for (stable_id, canonical_name) in canonical_symbol_name_by_stable_id {
            let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
                continue;
            };
            symbol_indices_by_canonical_name
                .entry(canonical_name.clone())
                .or_insert_with(Vec::new)
                .push(symbol_index);
        }
        symbol_indices_by_canonical_name
    }

    pub(super) fn symbol_indices_by_lower_canonical_name(
        symbol_index_by_stable_id: &BTreeMap<String, usize>,
        canonical_symbol_name_by_stable_id: &BTreeMap<String, String>,
    ) -> BTreeMap<String, Vec<usize>> {
        let mut symbol_indices_by_lower_canonical_name = BTreeMap::new();
        for (stable_id, canonical_name) in canonical_symbol_name_by_stable_id {
            let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
                continue;
            };
            symbol_indices_by_lower_canonical_name
                .entry(canonical_name.to_ascii_lowercase())
                .or_insert_with(Vec::new)
                .push(symbol_index);
        }
        symbol_indices_by_lower_canonical_name
    }

    pub(super) fn register_php_declaration_relations(
        graph: &mut SymbolGraph,
        corpus: &RepositorySymbolCorpus,
    ) {
        for path in &corpus.source_paths {
            let relative_path = Self::relative_display_path(&corpus.root, path);
            let edges = match php_declaration_relation_edges_for_file(
                &relative_path,
                path,
                &corpus.symbols,
                &corpus.symbols_by_relative_path,
                Some(&corpus.symbol_indices_by_name),
                Some(&corpus.symbol_indices_by_lower_name),
            ) {
                Ok(edges) => edges,
                Err(err) => {
                    warn!(
                        repository_id = corpus.repository_id,
                        path = %path.display(),
                        error = %err,
                        "failed to build php declaration relations while building heuristic graph"
                    );
                    continue;
                }
            };

            for (source_symbol_index, target_symbol_index, relation) in edges {
                let source_symbol = &corpus.symbols[source_symbol_index];
                let target_symbol = &corpus.symbols[target_symbol_index];
                if source_symbol.stable_id == target_symbol.stable_id {
                    continue;
                }

                let _ = graph.add_relation(
                    &source_symbol.stable_id,
                    &target_symbol.stable_id,
                    relation,
                );
            }
        }
    }

    pub(super) fn register_php_target_evidence_relations(
        graph: &mut SymbolGraph,
        corpus: &RepositorySymbolCorpus,
    ) {
        for evidence in corpus.php_evidence_by_relative_path.values() {
            for (source_symbol_index, target_symbol_index, relation) in
                resolve_php_target_evidence_edges(
                    &corpus.symbols,
                    &corpus.symbol_index_by_stable_id,
                    &corpus.symbol_indices_by_canonical_name,
                    &corpus.symbol_indices_by_lower_canonical_name,
                    evidence,
                )
            {
                let source_symbol = &corpus.symbols[source_symbol_index];
                let target_symbol = &corpus.symbols[target_symbol_index];
                if source_symbol.stable_id == target_symbol.stable_id {
                    continue;
                }
                let _ = graph.add_relation(
                    &source_symbol.stable_id,
                    &target_symbol.stable_id,
                    relation,
                );
            }
        }
    }

    pub(super) fn register_blade_relation_evidence(
        graph: &mut SymbolGraph,
        corpus: &RepositorySymbolCorpus,
    ) {
        for evidence in corpus.blade_evidence_by_relative_path.values() {
            for (source_symbol_index, target_symbol_index, relation) in
                resolve_blade_relation_evidence_edges(
                    &corpus.symbols,
                    &corpus.symbol_index_by_stable_id,
                    &corpus.symbol_indices_by_name,
                    &corpus.symbol_indices_by_lower_name,
                    evidence,
                )
            {
                let source_symbol = &corpus.symbols[source_symbol_index];
                let target_symbol = &corpus.symbols[target_symbol_index];
                if source_symbol.stable_id == target_symbol.stable_id {
                    continue;
                }
                let _ = graph.add_relation(
                    &source_symbol.stable_id,
                    &target_symbol.stable_id,
                    relation,
                );
            }
        }
    }

    pub(super) fn collect_repository_symbol_corpora(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<Arc<RepositorySymbolCorpus>>, ErrorData> {
        let mut corpora = self
            .attached_workspaces_for_repository(repository_id)?
            .into_par_iter()
            .map(|workspace| {
                self.collect_repository_symbol_corpus(
                    workspace.repository_id,
                    workspace.runtime_repository_id,
                    workspace.root,
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>, ErrorData>>()?;

        corpora.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
        Ok(corpora)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{SourceSpan, SymbolKind};
    use crate::languages::SymbolLanguage;

    fn test_symbol_corpus(
        repository_id: &str,
        manifest_token: &str,
        symbol_name_bytes: usize,
    ) -> Arc<RepositorySymbolCorpus> {
        Arc::new(RepositorySymbolCorpus {
            repository_id: repository_id.to_owned(),
            runtime_repository_id: repository_id.to_owned(),
            root: PathBuf::from(format!("/tmp/{repository_id}")),
            root_signature: manifest_token.to_owned(),
            source_paths: vec![PathBuf::from(format!("src/{repository_id}.rs"))],
            symbols: vec![SymbolDefinition {
                stable_id: format!("{repository_id}::symbol"),
                language: SymbolLanguage::Rust,
                kind: SymbolKind::Function,
                name: "x".repeat(symbol_name_bytes),
                path: PathBuf::from(format!("src/{repository_id}.rs")),
                line: 1,
                span: SourceSpan {
                    start_byte: 0,
                    end_byte: 1,
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 1,
                },
            }],
            container_symbol_index_by_index: vec![None],
            symbols_by_relative_path: BTreeMap::new(),
            symbol_index_by_stable_id: BTreeMap::new(),
            symbol_indices_by_name: BTreeMap::new(),
            symbol_indices_by_lower_name: BTreeMap::new(),
            canonical_symbol_name_by_stable_id: BTreeMap::new(),
            symbol_indices_by_canonical_name: BTreeMap::new(),
            symbol_indices_by_lower_canonical_name: BTreeMap::new(),
            rust_symbol_context_by_index: Vec::new(),
            rust_implementation_facts: Vec::new(),
            php_evidence_by_relative_path: BTreeMap::new(),
            blade_evidence_by_relative_path: BTreeMap::new(),
            diagnostics: RepositoryDiagnosticsSummary::default(),
        })
    }

    #[test]
    fn trim_symbol_corpus_cache_respects_byte_budget() {
        let mut cache = BTreeMap::new();
        for index in 0..3 {
            let repository_id = format!("repo-{index:03}");
            let manifest_token = "snapshot-001".to_owned();
            cache.insert(
                SymbolCorpusCacheKey {
                    repository_id: repository_id.clone(),
                    manifest_token,
                },
                test_symbol_corpus(&repository_id, "snapshot-001", 4096),
            );
        }

        let newest_key = SymbolCorpusCacheKey {
            repository_id: "repo-002".to_owned(),
            manifest_token: "snapshot-001".to_owned(),
        };
        let newest_bytes = FriggMcpServer::symbol_corpus_cache_entry_bytes(
            &newest_key,
            cache
                .get(&newest_key)
                .expect("newest corpus should be present before trim"),
        );

        FriggMcpServer::trim_symbol_corpus_cache_to_limits(&mut cache, 16, newest_bytes + 1);

        assert_eq!(cache.len(), 1);
        assert!(
            cache.contains_key(&newest_key),
            "byte-budget pruning should retain the newest sortable corpus key"
        );
        assert!(
            FriggMcpServer::symbol_corpus_cache_total_bytes(&cache) <= newest_bytes + 1,
            "trimmed cache should stay under the configured byte budget"
        );
    }

    #[test]
    fn trim_symbol_corpus_cache_drops_single_oversized_entry() {
        let mut cache = BTreeMap::new();
        let oversized_key = SymbolCorpusCacheKey {
            repository_id: "repo-large".to_owned(),
            manifest_token: "snapshot-001".to_owned(),
        };
        cache.insert(
            oversized_key.clone(),
            test_symbol_corpus("repo-large", "snapshot-001", 8192),
        );
        let oversized_bytes = FriggMcpServer::symbol_corpus_cache_total_bytes(&cache);

        FriggMcpServer::trim_symbol_corpus_cache_to_limits(
            &mut cache,
            16,
            oversized_bytes.saturating_sub(1),
        );

        assert!(
            cache.is_empty(),
            "a single oversized symbol corpus should not remain cached over the byte budget"
        );
    }
}
