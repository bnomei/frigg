use super::*;
use rayon::prelude::*;

impl FriggMcpServer {
    pub(super) fn collect_repository_symbol_corpus(
        &self,
        repository_id: String,
        root: PathBuf,
    ) -> Result<Arc<RepositorySymbolCorpus>, ErrorData> {
        let mut diagnostics = RepositoryDiagnosticsSummary::default();
        let mut manifest_output = None;
        let mut source_paths = None;
        let (file_digests, manifest_token) =
            match Self::load_latest_validated_manifest_snapshot_shared(
                &root,
                &repository_id,
                Some(&self.runtime_state.validated_manifest_candidate_cache),
            ) {
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
                    manifest_output = Some(live_output);
                    (
                        Arc::new(
                            manifest_output
                                .as_ref()
                                .expect("live manifest output just assigned")
                                .entries
                                .clone(),
                        ),
                        format!("live:{live_signature}"),
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
        let mut php_evidence_by_relative_path = BTreeMap::new();
        let mut blade_evidence_by_relative_path = BTreeMap::new();
        let mut canonical_symbol_name_by_stable_id = BTreeMap::new();

        for path in &source_paths {
            let relative_path = Self::relative_display_path(&root, path);
            let file_symbols = symbols_by_relative_path
                .get(&relative_path)
                .into_iter()
                .flatten()
                .map(|index| symbols[*index].clone())
                .collect::<Vec<_>>();
            if file_symbols.is_empty() {
                continue;
            }
            let Ok(source) = fs::read_to_string(path) else {
                continue;
            };
            match supported_language_for_path(path, LanguageCapability::SymbolCorpus) {
                Some(SymbolLanguage::Php) => {
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

        let corpus = Arc::new(RepositorySymbolCorpus {
            repository_id: repository_id.clone(),
            root,
            root_signature: root_signature.clone(),
            source_paths,
            symbols,
            symbols_by_relative_path,
            symbol_index_by_stable_id,
            symbol_indices_by_name,
            symbol_indices_by_lower_name,
            canonical_symbol_name_by_stable_id,
            symbol_indices_by_canonical_name,
            symbol_indices_by_lower_canonical_name,
            php_evidence_by_relative_path,
            blade_evidence_by_relative_path,
            diagnostics,
        });

        let mut cache = self
            .cache_state
            .symbol_corpus_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != repository_id || key.manifest_token == manifest_token
        });
        cache.insert(cache_key, corpus.clone());

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
    ) -> Option<crate::manifest_validation::SharedValidatedManifestSnapshot> {
        let db_path = resolve_provenance_db_path(root).ok()?;
        if !db_path.exists() {
            return None;
        }
        let storage = Storage::new(db_path);
        crate::manifest_validation::latest_validated_manifest_snapshot_shared(
            &storage,
            repository_id,
            root,
            cache,
        )
    }

    pub(super) fn current_root_signature_for_repository(
        root: &Path,
        repository_id: &str,
    ) -> Option<String> {
        if let Some(snapshot) =
            Self::load_latest_validated_manifest_snapshot_shared(root, repository_id, None)
        {
            return Some(Self::root_signature(snapshot.digests.as_ref()));
        }

        ManifestBuilder::default()
            .build_metadata_with_diagnostics(root)
            .ok()
            .map(|output| Self::root_signature(&output.entries))
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
            .roots_for_repository(repository_id)?
            .into_par_iter()
            .map(|(repository_id, root)| self.collect_repository_symbol_corpus(repository_id, root))
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>, ErrorData>>()?;

        corpora.sort_by(|left, right| left.repository_id.cmp(&right.repository_id));
        Ok(corpora)
    }
}
