use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::domain::{FriggError, FriggResult};
use crate::embeddings::{
    EmbeddingProvider, EmbeddingPurpose, EmbeddingRequest, GoogleEmbeddingProvider,
    OpenAiEmbeddingProvider,
};
use crate::settings::{SemanticRuntimeCredentials, SemanticRuntimeProvider};
use crate::storage::{SemanticChunkEmbeddingProjection, Storage, resolve_provenance_db_path};

use super::{
    HYBRID_SEMANTIC_CANDIDATE_POOL_MIN, HYBRID_SEMANTIC_CANDIDATE_POOL_MULTIPLIER,
    HYBRID_SEMANTIC_RETAIN_RELATIVE_FLOOR, HYBRID_SEMANTIC_RETAINED_DOCUMENT_MIN,
    HYBRID_SEMANTIC_RETAINED_DOCUMENT_MULTIPLIER, HybridChannelHit, HybridDocumentRef,
    HybridRankingIntent, HybridSemanticStatus, SearchFilters, TextSearcher,
    hard_excluded_runtime_path, hybrid_identifier_tokens, hybrid_overlap_count,
    hybrid_path_overlap_count, hybrid_path_quality_multiplier_with_intent,
    hybrid_query_exact_terms, hybrid_query_overlap_terms, normalize_search_filters,
    semantic_excerpt,
};

pub(super) trait SemanticRuntimeQueryEmbeddingExecutor: Sync {
    fn embed_query<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        query: String,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>>;
}

#[derive(Debug, Default)]
pub(super) struct RuntimeSemanticQueryEmbeddingExecutor {
    credentials: SemanticRuntimeCredentials,
}

impl RuntimeSemanticQueryEmbeddingExecutor {
    pub(super) fn new(credentials: SemanticRuntimeCredentials) -> Self {
        Self { credentials }
    }
}

impl SemanticRuntimeQueryEmbeddingExecutor for RuntimeSemanticQueryEmbeddingExecutor {
    fn embed_query<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        query: String,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>> {
        let model = model.trim().to_owned();
        let api_key = self
            .credentials
            .api_key_for(provider)
            .map(str::to_owned)
            .unwrap_or_default();
        Box::pin(async move {
            let request = EmbeddingRequest {
                model,
                input: vec![query],
                purpose: EmbeddingPurpose::Query,
                dimensions: None,
                trace_id: None,
            };
            let response = match provider {
                SemanticRuntimeProvider::OpenAi => {
                    let client = OpenAiEmbeddingProvider::new(api_key);
                    client.embed(request).await
                }
                SemanticRuntimeProvider::Google => {
                    let client = GoogleEmbeddingProvider::new(api_key);
                    client.embed(request).await
                }
            }
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic query embedding provider call failed: {err}"
                ))
            })?;

            if response.vectors.len() != 1 {
                return Err(FriggError::Internal(format!(
                    "semantic query embedding response length mismatch: expected 1 vector, received {}",
                    response.vectors.len()
                )));
            }
            let vector = response
                .vectors
                .into_iter()
                .next()
                .map(|entry| entry.values);
            let Some(vector) = vector else {
                return Err(FriggError::Internal(
                    "semantic query embedding response did not include vector payload".to_owned(),
                ));
            };
            if vector.is_empty() {
                return Err(FriggError::Internal(
                    "semantic query embedding provider returned an empty vector".to_owned(),
                ));
            }
            if vector.iter().any(|value| !value.is_finite()) {
                return Err(FriggError::Internal(
                    "semantic query embedding provider returned non-finite vector values"
                        .to_owned(),
                ));
            }

            Ok(vector)
        })
    }
}

pub(super) fn block_on_semantic_query_embedding(
    semantic_executor: &dyn SemanticRuntimeQueryEmbeddingExecutor,
    provider: SemanticRuntimeProvider,
    model: &str,
    query: String,
) -> FriggResult<Vec<f32>> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let model_owned = model.to_owned();
        return std::thread::scope(|scope| {
            let handle = scope.spawn(move || {
                let runtime = build_semantic_query_runtime()?;
                runtime.block_on(semantic_executor.embed_query(provider, &model_owned, query))
            });
            handle.join().map_err(|_| {
                FriggError::Internal("semantic query embedding worker thread panicked".to_owned())
            })?
        });
    }

    let runtime = build_semantic_query_runtime()?;
    runtime.block_on(semantic_executor.embed_query(provider, model, query))
}

pub(super) fn semantic_projection_score(
    query_embedding: &[f32],
    projection: &SemanticChunkEmbeddingProjection,
    repository_id: &str,
) -> FriggResult<f32> {
    cosine_similarity(query_embedding, &projection.embedding).ok_or_else(|| {
        FriggError::Internal(format!(
            "semantic similarity dimension mismatch for repository '{repository_id}' path '{}' chunk_id='{}' (query={}, chunk={})",
            projection.path,
            projection.chunk_id,
            query_embedding.len(),
            projection.embedding.len()
        ))
    })
}

fn build_semantic_query_runtime() -> FriggResult<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to build tokio runtime for semantic query embedding request: {err}"
            ))
        })
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }

    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (&left_value, &right_value) in left.iter().zip(right.iter()) {
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }

    if left_norm <= 0.0 || right_norm <= 0.0 {
        return Some(0.0);
    }

    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

#[derive(Debug, Clone)]
pub(super) struct SemanticChannelSearchOutput {
    pub(super) hits: Vec<HybridChannelHit>,
    pub(super) candidate_count: usize,
    pub(super) status: HybridSemanticStatus,
    pub(super) reason: Option<String>,
}

pub(super) fn search_semantic_channel_hits(
    searcher: &TextSearcher,
    query_text: &str,
    filters: &SearchFilters,
    limit: usize,
    credentials: &SemanticRuntimeCredentials,
    semantic_executor: &dyn SemanticRuntimeQueryEmbeddingExecutor,
) -> FriggResult<SemanticChannelSearchOutput> {
    #[derive(Debug)]
    struct PendingSemanticHit {
        repository_id: String,
        snapshot_id: String,
        path: String,
        chunk_id: String,
        raw_score: f32,
    }

    searcher
        .config
        .semantic_runtime
        .validate_startup(credentials)
        .map_err(|err| {
            FriggError::InvalidInput(format!(
                "semantic runtime validation failed code={}: {err}",
                err.code()
            ))
        })?;

    let provider = searcher.config.semantic_runtime.provider.ok_or_else(|| {
        FriggError::Internal(
            "semantic runtime provider missing after successful startup validation".to_owned(),
        )
    })?;
    let model = searcher
        .config
        .semantic_runtime
        .normalized_model()
        .ok_or_else(|| {
            FriggError::Internal(
                "semantic runtime model missing after successful startup validation".to_owned(),
            )
        })?;
    let query_embedding = block_on_semantic_query_embedding(
        semantic_executor,
        provider,
        model,
        query_text.to_owned(),
    )?;
    if query_embedding.is_empty() {
        return Err(FriggError::Internal(
            "semantic query embedding provider returned an empty vector".to_owned(),
        ));
    }
    if query_embedding.iter().any(|value| !value.is_finite()) {
        return Err(FriggError::Internal(
            "semantic query embedding provider returned non-finite vector values".to_owned(),
        ));
    }

    let normalized_filters = normalize_search_filters(filters.clone())?;
    let ranking_intent = HybridRankingIntent::from_query(query_text);
    let mut repositories = searcher.config.repositories();
    repositories.sort_by(|left, right| {
        left.repository_id
            .cmp(&right.repository_id)
            .then(left.root_path.cmp(&right.root_path))
    });

    let mut pending_hits = Vec::new();
    let mut db_paths_by_repository = BTreeMap::new();
    let mut degraded_reasons = Vec::new();
    let mut unavailable_reasons = Vec::new();
    for repo in repositories {
        if normalized_filters
            .repository_id
            .as_ref()
            .is_some_and(|repository_id| repository_id != &repo.repository_id.0)
        {
            continue;
        }
        let repository_id = repo.repository_id.0;
        let root = Path::new(&repo.root_path);
        let db_path = resolve_provenance_db_path(root).map_err(|err| {
            FriggError::Internal(format!(
                "semantic storage path resolution failed for repository '{repository_id}': {err}"
            ))
        })?;
        if !db_path.exists() {
            unavailable_reasons.push(format!(
                "repository '{repository_id}' has no semantic storage database at '{}'",
                db_path.display()
            ));
            continue;
        }
        db_paths_by_repository.insert(repository_id.clone(), db_path.clone());

        let storage = Storage::new(db_path);
        let latest = storage
            .load_latest_manifest_for_repository(&repository_id)
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic storage snapshot lookup failed for repository '{repository_id}': {err}"
                ))
            })?;
        let Some(latest_snapshot) = latest else {
            unavailable_reasons.push(format!(
                "repository '{repository_id}' has no manifest snapshot"
            ));
            continue;
        };
        let latest_manifest_paths = latest_snapshot
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<BTreeSet<_>>();
        let latest_snapshot_has_embeddings = storage
            .has_semantic_embeddings_for_repository_snapshot_model(
                &repository_id,
                &latest_snapshot.snapshot_id,
                provider.as_str(),
                model,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic storage embedding presence lookup failed for repository '{repository_id}' snapshot '{}': {err}",
                    latest_snapshot.snapshot_id
                ))
            })?;
        let selected_snapshot_id = if latest_snapshot_has_embeddings {
            latest_snapshot.snapshot_id.clone()
        } else if let Some(fallback_snapshot_id) = storage
            .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                &repository_id,
                provider.as_str(),
                model,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic storage fallback snapshot lookup failed for repository '{repository_id}': {err}"
                ))
            })?
        {
            if fallback_snapshot_id != latest_snapshot.snapshot_id {
                degraded_reasons.push(format!(
                    "repository '{repository_id}' latest manifest snapshot '{}' has no semantic embeddings for provider '{}' model '{}'; using older semantic snapshot '{}'",
                    latest_snapshot.snapshot_id,
                    provider.as_str(),
                    model,
                    fallback_snapshot_id
                ));
            }
            fallback_snapshot_id
        } else {
            unavailable_reasons.push(format!(
                "repository '{repository_id}' latest manifest snapshot '{}' has no semantic embeddings for provider '{}' model '{}' on any snapshot",
                latest_snapshot.snapshot_id,
                provider.as_str(),
                model
            ));
            continue;
        };
        let using_fallback_snapshot = selected_snapshot_id != latest_snapshot.snapshot_id;
        let projections = storage
            .load_semantic_embedding_projections_for_repository_snapshot_model(
                &repository_id,
                &selected_snapshot_id,
                Some(provider.as_str()),
                Some(model),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic storage embedding projection load failed for repository '{repository_id}' snapshot '{}': {err}",
                    selected_snapshot_id
                ))
            })?;

        for projection in projections {
            if using_fallback_snapshot && !latest_manifest_paths.contains(&projection.path) {
                continue;
            }
            if hard_excluded_runtime_path(root, Path::new(&projection.path)) {
                continue;
            }
            if let Some(language) = normalized_filters.language {
                if !language.matches_path(Path::new(&projection.path)) {
                    continue;
                }
            }
            let score = semantic_projection_score(&query_embedding, &projection, &repository_id)?
                * hybrid_path_quality_multiplier_with_intent(&projection.path, &ranking_intent);
            if !score.is_finite() {
                return Err(FriggError::Internal(format!(
                    "semantic similarity produced non-finite score for repository '{repository_id}' path '{}' chunk_id='{}'",
                    projection.path, projection.chunk_id
                )));
            }

            pending_hits.push(PendingSemanticHit {
                repository_id: repository_id.clone(),
                snapshot_id: selected_snapshot_id.clone(),
                path: projection.path,
                chunk_id: projection.chunk_id,
                raw_score: score,
            });
        }
    }

    pending_hits.sort_by(|left, right| {
        right
            .raw_score
            .total_cmp(&left.raw_score)
            .then(left.repository_id.cmp(&right.repository_id))
            .then(left.path.cmp(&right.path))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });
    let semantic_candidate_limit = limit
        .saturating_mul(HYBRID_SEMANTIC_CANDIDATE_POOL_MULTIPLIER)
        .max(HYBRID_SEMANTIC_CANDIDATE_POOL_MIN);
    pending_hits.truncate(semantic_candidate_limit);

    let mut chunk_texts_by_group = BTreeMap::new();
    for ((repository_id, snapshot_id), chunk_ids) in pending_hits.iter().fold(
        BTreeMap::<(String, String), Vec<String>>::new(),
        |mut grouped, hit| {
            grouped
                .entry((hit.repository_id.clone(), hit.snapshot_id.clone()))
                .or_default()
                .push(hit.chunk_id.clone());
            grouped
        },
    ) {
        let Some(db_path) = db_paths_by_repository.get(&repository_id) else {
            continue;
        };
        let storage = Storage::new(db_path.clone());
        let texts = storage
            .load_semantic_chunk_texts_for_repository_snapshot(
                &repository_id,
                &snapshot_id,
                &chunk_ids,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic storage chunk text load failed for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
        chunk_texts_by_group.insert((repository_id, snapshot_id), texts);
    }

    let semantic_hits = pending_hits
        .into_iter()
        .map(|hit| {
            let excerpt_source = chunk_texts_by_group
                .get(&(hit.repository_id.clone(), hit.snapshot_id.clone()))
                .and_then(|texts| texts.get(&hit.chunk_id))
                .map(|text| semantic_excerpt(text, &hit.path))
                .unwrap_or_else(|| semantic_excerpt("", &hit.path));
            HybridChannelHit {
                document: HybridDocumentRef {
                    repository_id: hit.repository_id,
                    path: hit.path.clone(),
                    line: 1,
                    column: 1,
                },
                raw_score: hit.raw_score,
                excerpt: excerpt_source,
                provenance_id: hit.chunk_id,
            }
        })
        .collect::<Vec<_>>();
    let semantic_candidate_count = semantic_hits.len();

    let status = if semantic_hits.is_empty() {
        if !degraded_reasons.is_empty() {
            HybridSemanticStatus::Degraded
        } else if !unavailable_reasons.is_empty() {
            HybridSemanticStatus::Unavailable
        } else {
            HybridSemanticStatus::Ok
        }
    } else if degraded_reasons.is_empty() && unavailable_reasons.is_empty() {
        HybridSemanticStatus::Ok
    } else {
        HybridSemanticStatus::Degraded
    };
    let mut non_ok_reasons = degraded_reasons;
    non_ok_reasons.extend(unavailable_reasons);
    let reason = (!non_ok_reasons.is_empty()).then(|| non_ok_reasons.join("; "));

    Ok(SemanticChannelSearchOutput {
        hits: semantic_hits,
        candidate_count: semantic_candidate_count,
        status,
        reason,
    })
}

pub(super) fn retain_semantic_hits_for_query(
    hits: Vec<HybridChannelHit>,
    query_text: &str,
    limit: usize,
) -> (Vec<HybridChannelHit>, usize) {
    if hits.is_empty() || limit == 0 {
        return (Vec::new(), 0);
    }

    let best_raw_score = hits
        .iter()
        .map(|hit| hit.raw_score.max(0.0))
        .fold(0.0_f32, f32::max);
    if best_raw_score <= 0.0 {
        return (Vec::new(), 0);
    }

    let retain_floor = best_raw_score * HYBRID_SEMANTIC_RETAIN_RELATIVE_FLOOR;
    let query_exact_terms = hybrid_query_exact_terms(query_text);
    let retained_document_limit = limit
        .saturating_mul(HYBRID_SEMANTIC_RETAINED_DOCUMENT_MULTIPLIER)
        .max(HYBRID_SEMANTIC_RETAINED_DOCUMENT_MIN);
    let query_overlap_terms = hybrid_query_overlap_terms(query_text);
    let preserve_overlap_hits = query_overlap_terms.len() > query_exact_terms.len();
    let mut retained_hits = Vec::new();
    let mut retained_documents = BTreeSet::new();
    let mut chunks_per_document = BTreeMap::<(String, String), usize>::new();

    for hit in hits {
        let document_key = (
            hit.document.repository_id.clone(),
            hit.document.path.clone(),
        );
        let path_overlap = hybrid_path_overlap_count(&hit.document.path, query_text);
        let excerpt_overlap = hybrid_overlap_count(
            &hybrid_identifier_tokens(&hit.excerpt),
            &query_overlap_terms,
        );
        if hit.raw_score < retain_floor
            && (!preserve_overlap_hits || (path_overlap == 0 && excerpt_overlap == 0))
        {
            continue;
        }

        if !retained_documents.contains(&document_key)
            && retained_documents.len() >= retained_document_limit
        {
            continue;
        }
        let chunk_count = chunks_per_document.entry(document_key.clone()).or_insert(0);
        if *chunk_count >= 2 {
            continue;
        }
        *chunk_count += 1;

        retained_documents.insert(document_key);
        retained_hits.push(hit);
    }

    (retained_hits, retained_documents.len())
}
