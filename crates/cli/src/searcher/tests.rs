use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::domain::{FriggError, FriggResult, model::TextMatch};
use crate::settings::{
    FriggConfig, LexicalBackendMode, SemanticRuntimeConfig, SemanticRuntimeCredentials,
    SemanticRuntimeProvider,
};
use crate::storage::{
    ManifestEntry, SemanticChunkEmbeddingRecord, Storage, ensure_provenance_db_parent_dir,
    reset_semantic_read_trace, resolve_provenance_db_path, snapshot_semantic_read_trace,
};
use regex::Regex;

use super::build_hybrid_path_witness_hits_with_intent;
use super::lexical_channel::{
    HybridPathWitnessQueryContext, hybrid_path_witness_recall_score,
    hybrid_path_witness_recall_score_for_projection,
};
use crate::searcher::{
    HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridRankingIntent,
    HybridSemanticStatus, HybridSourceClass, MAX_REGEX_ALTERNATIONS, MAX_REGEX_GROUPS,
    MAX_REGEX_PATTERN_BYTES, MAX_REGEX_QUANTIFIERS, RegexSearchError, SearchDiagnosticKind,
    SearchFilters, SearchHybridQuery, SearchLexicalBackend, SearchTextQuery,
    SemanticRuntimeQueryEmbeddingExecutor, StoredPathWitnessProjection, TextSearcher,
    ValidatedManifestCandidateCache, build_hybrid_lexical_hits,
    build_hybrid_lexical_hits_for_query, build_hybrid_lexical_recall_regex,
    build_regex_prefilter_plan, clear_ripgrep_availability_cache, compile_safe_regex,
    hybrid_lexical_recall_tokens, normalize_search_filters, rank_hybrid_evidence,
    rank_hybrid_evidence_for_query,
};

use super::graph_channel;

mod ecosystem_ranking;
mod entrypoint_ranking;
mod hard_pack_trace;
mod laravel_path_witness;
mod overlay_projection;
mod python_ranking;
mod rust_ranking;
mod semantic;
mod text_search;

#[derive(Debug, Clone)]
struct MockSemanticQueryEmbeddingExecutor {
    result: Result<Vec<f32>, String>,
}

impl MockSemanticQueryEmbeddingExecutor {
    fn success(vector: Vec<f32>) -> Self {
        Self {
            result: Ok(pad_semantic_test_vector(vector)),
        }
    }

    fn failure(message: &str) -> Self {
        Self {
            result: Err(message.to_owned()),
        }
    }
}

impl SemanticRuntimeQueryEmbeddingExecutor for MockSemanticQueryEmbeddingExecutor {
    fn embed_query<'a>(
        &'a self,
        _provider: SemanticRuntimeProvider,
        _model: &'a str,
        _query: String,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>> {
        let result = self.result.clone();
        Box::pin(async move {
            match result {
                Ok(vector) => Ok(vector),
                Err(message) => Err(FriggError::Internal(message)),
            }
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct PanicSemanticQueryEmbeddingExecutor;

impl SemanticRuntimeQueryEmbeddingExecutor for PanicSemanticQueryEmbeddingExecutor {
    fn embed_query<'a>(
        &'a self,
        _provider: SemanticRuntimeProvider,
        _model: &'a str,
        _query: String,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>> {
        Box::pin(async move {
            Err(FriggError::Internal(
                "semantic executor should not be called when semantic toggle is disabled"
                    .to_owned(),
            ))
        })
    }
}

fn semantic_hybrid_fixture(
    test_name: &str,
    semantic_runtime: SemanticRuntimeConfig,
) -> FriggResult<(TextSearcher, PathBuf)> {
    let root = temp_workspace_root(test_name);
    let semantic_b = semantic_record("repo-001", "snapshot-001", "src/b.rs", 0, vec![1.0, 0.0]);
    let mut semantic_z = semantic_record("repo-001", "snapshot-001", "src/z.rs", 0, vec![0.0, 1.0]);
    semantic_z.start_line = 2;
    semantic_z.end_line = 3;
    prepare_workspace(
        &root,
        &[
            ("src/b.rs", "pub fn b() { let _ = \"needle\"; }\n"),
            (
                "src/z.rs",
                "pub fn z() {\n    let _ = \"needle\";\n    let _ = \"semantic\";\n}\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(&root, "repo-001", "snapshot-001", &[semantic_b, semantic_z])?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime;
    Ok((TextSearcher::new(config), root))
}

fn semantic_runtime_enabled(strict_mode: bool) -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode,
    }
}

fn write_fake_ripgrep_script(root: &Path, body: &str) -> FriggResult<PathBuf> {
    let path = root.join("fake-rg.sh");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo 'ripgrep 15.1.0'\n  exit 0\nfi\ncat <<'EOF'\n{body}\nEOF\n"
    );
    fs::write(&path, script).map_err(FriggError::Io)?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&path).map_err(FriggError::Io)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).map_err(FriggError::Io)?;
    }
    Ok(path)
}

fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
    system_time
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
}

fn seed_semantic_embeddings(
    workspace_root: &Path,
    repository_id: &str,
    snapshot_id: &str,
    records: &[SemanticChunkEmbeddingRecord],
) -> FriggResult<()> {
    let db_path = ensure_provenance_db_parent_dir(workspace_root)?;
    let resolved_db_path = resolve_provenance_db_path(workspace_root)?;
    assert_eq!(db_path, resolved_db_path);

    let storage = Storage::new(db_path);
    storage.initialize()?;

    let mut manifest_entries = records
        .iter()
        .map(|record| {
            let metadata = fs::metadata(workspace_root.join(&record.path))
                .expect("semantic embedding manifest path should exist");
            ManifestEntry {
                path: record.path.clone(),
                sha256: format!("hash-{}", record.path),
                size_bytes: metadata.len(),
                mtime_ns: metadata.modified().ok().and_then(system_time_to_unix_nanos),
            }
        })
        .collect::<Vec<_>>();
    manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
    manifest_entries.dedup_by(|left, right| left.path == right.path);

    storage.upsert_manifest(repository_id, snapshot_id, &manifest_entries)?;
    let mut grouped =
        std::collections::BTreeMap::<(String, String), Vec<SemanticChunkEmbeddingRecord>>::new();
    for record in records {
        grouped
            .entry((record.provider.clone(), record.model.clone()))
            .or_default()
            .push(record.clone());
    }
    for ((provider, model), group) in grouped {
        storage.replace_semantic_embeddings_for_repository(
            repository_id,
            snapshot_id,
            &provider,
            &model,
            &group,
        )?;
    }

    Ok(())
}

fn seed_manifest_snapshot(
    workspace_root: &Path,
    repository_id: &str,
    snapshot_id: &str,
    paths: &[&str],
) -> FriggResult<()> {
    let db_path = ensure_provenance_db_parent_dir(workspace_root)?;
    let resolved_db_path = resolve_provenance_db_path(workspace_root)?;
    assert_eq!(db_path, resolved_db_path);

    let storage = Storage::new(db_path);
    storage.initialize()?;

    let mut manifest_entries = paths
        .iter()
        .map(|path| {
            let metadata = fs::metadata(workspace_root.join(path)).map_err(FriggError::Io)?;
            Ok(ManifestEntry {
                path: (*path).to_owned(),
                sha256: format!("hash-{path}"),
                size_bytes: metadata.len(),
                mtime_ns: metadata.modified().ok().and_then(system_time_to_unix_nanos),
            })
        })
        .collect::<FriggResult<Vec<_>>>()?;
    manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
    manifest_entries.dedup_by(|left, right| left.path == right.path);

    storage.upsert_manifest(repository_id, snapshot_id, &manifest_entries)?;
    Ok(())
}

fn semantic_record(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    chunk_index: usize,
    embedding: Vec<f32>,
) -> SemanticChunkEmbeddingRecord {
    let language = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| match extension {
            "php" => "php",
            "md" | "markdown" => "markdown",
            "json" => "json",
            _ => "rust",
        })
        .unwrap_or("rust")
        .to_owned();
    let path_slug = path.replace('/', "_");

    SemanticChunkEmbeddingRecord {
        chunk_id: format!("chunk-{path_slug}-{chunk_index}"),
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        language,
        chunk_index,
        start_line: 1,
        end_line: 1,
        provider: "openai".to_owned(),
        model: "text-embedding-3-small".to_owned(),
        trace_id: Some("trace-semantic-test".to_owned()),
        content_hash_blake3: format!("hash-content-{path_slug}-{chunk_index}"),
        content_text: format!("semantic excerpt for {path}"),
        embedding: pad_semantic_test_vector(embedding),
    }
}

fn pad_semantic_test_vector(mut embedding: Vec<f32>) -> Vec<f32> {
    embedding.resize(crate::storage::DEFAULT_VECTOR_DIMENSIONS, 0.0);
    embedding
}

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!(
        "frigg-search-{test_name}-{nonce}-{}",
        std::process::id()
    ))
}

fn prepare_workspace(root: &Path, files: &[(&str, &str)]) -> FriggResult<()> {
    fs::create_dir_all(root).map_err(FriggError::Io)?;
    for (relative_path, contents) in files {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(FriggError::Io)?;
        }
        fs::write(path, contents).map_err(FriggError::Io)?;
    }

    Ok(())
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn rewrite_file_with_new_mtime(path: &Path, contents: &str) -> FriggResult<()> {
    let before = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_to_unix_nanos);

    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        fs::write(path, contents).map_err(FriggError::Io)?;
        let after = fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_to_unix_nanos);
        if after != before {
            return Ok(());
        }
    }

    Err(FriggError::Internal(
        "fixture file mtime did not advance after rewrite".to_owned(),
    ))
}

fn text_match(
    repository_id: &str,
    path: &str,
    line: usize,
    column: usize,
    excerpt: &str,
) -> TextMatch {
    TextMatch {
        match_id: None,
        repository_id: repository_id.to_owned(),
        path: path.to_owned(),
        line,
        column,
        excerpt: excerpt.to_owned(),
        witness_score_hint_millis: None,
        witness_provenance_ids: None,
    }
}

fn hybrid_hit(
    repository_id: &str,
    path: &str,
    raw_score: f32,
    provenance_id: &str,
) -> HybridChannelHit {
    hybrid_hit_with_channel(
        crate::domain::EvidenceChannel::LexicalManifest,
        repository_id,
        path,
        raw_score,
        provenance_id,
    )
}

fn hybrid_hit_with_channel(
    channel: crate::domain::EvidenceChannel,
    repository_id: &str,
    path: &str,
    raw_score: f32,
    provenance_id: &str,
) -> HybridChannelHit {
    HybridChannelHit {
        channel,
        document: HybridDocumentRef {
            repository_id: repository_id.to_owned(),
            path: path.to_owned(),
            line: 1,
            column: 1,
        },
        anchor: crate::domain::EvidenceAnchor::new(
            crate::domain::EvidenceAnchorKind::TextSpan,
            1,
            1,
            1,
            1,
        ),
        raw_score,
        excerpt: format!("excerpt for {path}"),
        provenance_ids: vec![provenance_id.to_owned()],
    }
}
