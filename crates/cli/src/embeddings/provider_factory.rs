//! Shared semantic embedding provider construction for indexing and query-time recall.
//!
//! Centralizes provider selection, credential loading, and process-wide local-model caching so
//! indexer and searcher paths share one embedding backend per semantic runtime configuration.

use std::collections::BTreeMap;
#[cfg(feature = "local-embeddings")]
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(feature = "local-embeddings")]
use crate::settings::SemanticRuntimeConfig;
use crate::settings::{SemanticRuntimeCredentials, SemanticRuntimeProvider};

#[cfg(feature = "local-embeddings")]
use super::LocalEmbeddingProvider;
#[cfg(feature = "local-embeddings")]
use super::local::{local_model_error_to_embedding_error, reject_hf_home_override_for_provider};
#[cfg(feature = "local-embeddings")]
use super::local_model::{
    LocalModelArtifact, prepare_local_semantic_model, require_prepared_local_model,
    resolve_local_model_artifact, resolve_model_alias,
};
use super::{
    EmbeddingError, EmbeddingProvider, EmbeddingProviderKind, EmbeddingResult,
    GoogleEmbeddingProvider, OpenAiEmbeddingProvider, OpenAiEmbeddingProviderConfig,
    ProviderFailure,
};

/// Policy controlling whether local model artifacts may be prepared during provider construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalArtifactPolicy {
    /// Fail construction unless artifacts are already prepared on disk.
    RequirePrepared,
    /// Download/prepare missing local model artifacts before constructing the provider.
    AllowPreparation,
}

/// Inputs required to construct a semantic embedding provider for indexing or recall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEmbeddingProviderFactoryConfig<'a> {
    pub provider: SemanticRuntimeProvider,
    pub model: &'a str,
    pub credentials: &'a SemanticRuntimeCredentials,
    pub local_artifact_policy: LocalArtifactPolicy,
    /// Full embeddings POST URL required for [`SemanticRuntimeProvider::OpenAiCompat`].
    pub endpoint: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SemanticEmbeddingProviderCacheKey {
    provider: &'static str,
    model: String,
    credential_fingerprint: Option<String>,
    local_artifact_identity: Option<String>,
    endpoint_fingerprint: Option<String>,
}

static SEMANTIC_EMBEDDING_PROVIDER_CACHE: OnceLock<
    Mutex<BTreeMap<SemanticEmbeddingProviderCacheKey, Arc<dyn EmbeddingProvider>>>,
> = OnceLock::new();
static SEMANTIC_EMBEDDING_PROVIDER_BUILD_LOCKS: OnceLock<
    Mutex<BTreeMap<SemanticEmbeddingProviderCacheKey, Arc<Mutex<()>>>>,
> = OnceLock::new();

/// Builds the active semantic embedding provider from runtime configuration and credentials.
fn build_semantic_embedding_provider(
    config: SemanticEmbeddingProviderFactoryConfig<'_>,
) -> EmbeddingResult<Arc<dyn EmbeddingProvider>> {
    let model = config.model.trim();
    match config.provider {
        SemanticRuntimeProvider::OpenAi => {
            let api_key = require_api_key(config.provider, config.credentials)?;
            Ok(Arc::new(OpenAiEmbeddingProvider::new(api_key.to_owned())))
        }
        SemanticRuntimeProvider::OpenAiCompat => {
            let api_key = require_api_key(config.provider, config.credentials)?;
            let endpoint = require_openai_compat_endpoint(config.endpoint)?;
            let provider_config = OpenAiEmbeddingProviderConfig {
                endpoint,
                ..Default::default()
            };
            Ok(Arc::new(OpenAiEmbeddingProvider::with_config_kind(
                api_key.to_owned(),
                provider_config,
                EmbeddingProviderKind::OpenAiCompat,
            )))
        }
        SemanticRuntimeProvider::Google => {
            let api_key = require_api_key(config.provider, config.credentials)?;
            Ok(Arc::new(GoogleEmbeddingProvider::new(api_key.to_owned())))
        }
        SemanticRuntimeProvider::Local => build_local_embedding_provider(config, model),
    }
}

#[cfg(feature = "local-embeddings")]
fn build_local_embedding_provider(
    config: SemanticEmbeddingProviderFactoryConfig<'_>,
    model: &str,
) -> EmbeddingResult<Arc<dyn EmbeddingProvider>> {
    prepare_local_artifacts_for_policy(config.local_artifact_policy, model)?;
    Ok(Arc::new(LocalEmbeddingProvider::new(model)?))
}

#[cfg(not(feature = "local-embeddings"))]
fn build_local_embedding_provider(
    _config: SemanticEmbeddingProviderFactoryConfig<'_>,
    _model: &str,
) -> EmbeddingResult<Arc<dyn EmbeddingProvider>> {
    Err(local_embeddings_unavailable_error())
}

/// Builds or reuses a process-resident semantic embedding provider for the selected target.
pub fn cached_semantic_embedding_provider(
    config: SemanticEmbeddingProviderFactoryConfig<'_>,
) -> EmbeddingResult<Arc<dyn EmbeddingProvider>> {
    let runtime_provider = config.provider;
    let key = provider_cache_key(&config)?;
    let cache = SEMANTIC_EMBEDDING_PROVIDER_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let cached_provider = {
        let guard = lock_provider_cache(cache, runtime_provider)?;
        guard.get(&key).cloned()
    };
    if let Some(provider) = cached_provider {
        return Ok(provider);
    }

    let build_lock = provider_build_lock(&key, runtime_provider)?;
    let _build_guard = build_lock.lock().map_err(|_| {
        EmbeddingError::Provider(ProviderFailure::non_retryable(
            provider_kind(runtime_provider),
            "semantic embedding provider build lock was poisoned",
            Some("provider_build_lock_unavailable".to_owned()),
            None,
            None,
        ))
    })?;
    let cached_provider = {
        let guard = lock_provider_cache(cache, runtime_provider)?;
        guard.get(&key).cloned()
    };
    if let Some(provider) = cached_provider {
        return Ok(provider);
    }

    let provider = build_semantic_embedding_provider(config)?;
    let mut guard = lock_provider_cache(cache, runtime_provider)?;
    if let Some(provider) = guard.get(&key) {
        return Ok(Arc::clone(provider));
    }
    guard.insert(key, Arc::clone(&provider));
    Ok(provider)
}

/// Returns the canonical model identifier used for provider-cache and semantic storage keys.
pub(crate) fn canonical_provider_model(
    provider: SemanticRuntimeProvider,
    model: &str,
) -> EmbeddingResult<String> {
    match provider {
        #[cfg(feature = "local-embeddings")]
        SemanticRuntimeProvider::Local => resolve_model_alias(model)
            .map(|alias| alias.semantic_model.to_owned())
            .map_err(local_model_error_to_embedding_error),
        #[cfg(not(feature = "local-embeddings"))]
        SemanticRuntimeProvider::Local => Err(local_embeddings_unavailable_error()),
        SemanticRuntimeProvider::OpenAi
        | SemanticRuntimeProvider::OpenAiCompat
        | SemanticRuntimeProvider::Google => Ok(model.trim().to_owned()),
    }
}

fn provider_build_lock(
    key: &SemanticEmbeddingProviderCacheKey,
    provider: SemanticRuntimeProvider,
) -> EmbeddingResult<Arc<Mutex<()>>> {
    let locks = SEMANTIC_EMBEDDING_PROVIDER_BUILD_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut guard = locks.lock().map_err(|_| {
        EmbeddingError::Provider(ProviderFailure::non_retryable(
            provider_kind(provider),
            "semantic embedding provider build-lock registry was poisoned",
            Some("provider_build_lock_registry_unavailable".to_owned()),
            None,
            None,
        ))
    })?;
    Ok(Arc::clone(
        guard
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(()))),
    ))
}

fn lock_provider_cache(
    cache: &Mutex<BTreeMap<SemanticEmbeddingProviderCacheKey, Arc<dyn EmbeddingProvider>>>,
    provider: SemanticRuntimeProvider,
) -> EmbeddingResult<
    std::sync::MutexGuard<
        '_,
        BTreeMap<SemanticEmbeddingProviderCacheKey, Arc<dyn EmbeddingProvider>>,
    >,
> {
    cache.lock().map_err(|_| {
        EmbeddingError::Provider(ProviderFailure::non_retryable(
            provider_kind(provider),
            "semantic embedding provider cache lock was poisoned",
            Some("provider_cache_unavailable".to_owned()),
            None,
            None,
        ))
    })
}

fn provider_cache_key(
    config: &SemanticEmbeddingProviderFactoryConfig<'_>,
) -> EmbeddingResult<SemanticEmbeddingProviderCacheKey> {
    let (model, local_artifact_identity) = match config.provider {
        #[cfg(feature = "local-embeddings")]
        SemanticRuntimeProvider::Local => {
            let artifact = resolve_local_model_artifact(config.model)
                .map_err(local_model_error_to_embedding_error)?;
            reject_hf_home_override_for_provider(&artifact, hf_home_from_process_env())?;
            let model = canonical_provider_model(config.provider, config.model)?;
            (model, Some(local_artifact_cache_identity(&artifact)))
        }
        #[cfg(not(feature = "local-embeddings"))]
        SemanticRuntimeProvider::Local => return Err(local_embeddings_unavailable_error()),
        SemanticRuntimeProvider::OpenAi
        | SemanticRuntimeProvider::OpenAiCompat
        | SemanticRuntimeProvider::Google => (config.model.trim().to_owned(), None),
    };

    let endpoint_fingerprint = match config.provider {
        SemanticRuntimeProvider::OpenAiCompat => {
            let endpoint = require_openai_compat_endpoint(config.endpoint)?;
            Some(blake3::hash(endpoint.as_bytes()).to_hex().to_string())
        }
        _ => None,
    };

    Ok(SemanticEmbeddingProviderCacheKey {
        provider: config.provider.as_str(),
        model,
        credential_fingerprint: config
            .credentials
            .api_key_for(config.provider)
            .map(|api_key| blake3::hash(api_key.as_bytes()).to_hex().to_string()),
        local_artifact_identity,
        endpoint_fingerprint,
    })
}

#[cfg(feature = "local-embeddings")]
fn prepare_local_artifacts_for_policy(
    policy: LocalArtifactPolicy,
    model: &str,
) -> EmbeddingResult<()> {
    validate_local_artifacts_for_policy(policy, model).map(|_| ())
}

#[cfg(feature = "local-embeddings")]
fn validate_local_artifacts_for_policy(
    policy: LocalArtifactPolicy,
    model: &str,
) -> EmbeddingResult<LocalModelArtifact> {
    prepare_local_artifacts_for_policy_with(policy, model, |semantic_runtime| {
        prepare_local_semantic_model(semantic_runtime)
            .map(|_| ())
            .map_err(local_model_error_to_embedding_error)
    })?;
    let artifact =
        require_prepared_local_model(model).map_err(local_model_error_to_embedding_error)?;
    reject_hf_home_override_for_provider(&artifact, hf_home_from_process_env())?;
    Ok(artifact)
}

#[cfg(feature = "local-embeddings")]
fn local_artifact_cache_identity(artifact: &LocalModelArtifact) -> String {
    format!(
        "semantic_model={};cache_root={};cache_key={};repository={};model_file={};required_files={}",
        artifact.semantic_model,
        artifact.cache_root.display(),
        artifact.cache_key,
        artifact.repository,
        artifact.model_file,
        artifact.required_files.join(",")
    )
}

#[cfg(feature = "local-embeddings")]
fn prepare_local_artifacts_for_policy_with(
    policy: LocalArtifactPolicy,
    model: &str,
    prepare: impl FnOnce(&SemanticRuntimeConfig) -> EmbeddingResult<()>,
) -> EmbeddingResult<()> {
    match policy {
        LocalArtifactPolicy::RequirePrepared => Ok(()),
        LocalArtifactPolicy::AllowPreparation => {
            let semantic_runtime = SemanticRuntimeConfig {
                enabled: true,
                provider: Some(SemanticRuntimeProvider::Local),
                model: Some(model.to_owned()),
                strict_mode: false,
                openai_compat_endpoint: None,
            };
            prepare(&semantic_runtime)
        }
    }
}

#[cfg(feature = "local-embeddings")]
fn hf_home_from_process_env() -> Option<PathBuf> {
    std::env::var_os("HF_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(feature = "local-embeddings"))]
fn local_embeddings_unavailable_error() -> EmbeddingError {
    EmbeddingError::Provider(ProviderFailure::non_retryable(
        EmbeddingProviderKind::Local,
        "local semantic provider is not available in this build; rebuild Frigg with the `local-embeddings` feature",
        Some("local_provider_unavailable".to_owned()),
        None,
        None,
    ))
}

fn require_api_key(
    provider: SemanticRuntimeProvider,
    credentials: &SemanticRuntimeCredentials,
) -> EmbeddingResult<&str> {
    credentials.api_key_for(provider).ok_or_else(|| {
        EmbeddingError::Provider(ProviderFailure::non_retryable(
            provider_kind(provider),
            format!(
                "semantic runtime provider '{}' requires credentials before embedding provider construction",
                provider.as_str()
            ),
            Some("missing_api_key".to_owned()),
            None,
            None,
        ))
    })
}

fn provider_kind(provider: SemanticRuntimeProvider) -> EmbeddingProviderKind {
    match provider {
        SemanticRuntimeProvider::OpenAi => EmbeddingProviderKind::OpenAi,
        SemanticRuntimeProvider::OpenAiCompat => EmbeddingProviderKind::OpenAiCompat,
        SemanticRuntimeProvider::Google => EmbeddingProviderKind::Google,
        SemanticRuntimeProvider::Local => EmbeddingProviderKind::Local,
    }
}

fn require_openai_compat_endpoint(endpoint: Option<&str>) -> EmbeddingResult<String> {
    let Some(raw) = endpoint else {
        return Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
            EmbeddingProviderKind::OpenAiCompat,
            "semantic runtime provider 'openai_compat' requires an embeddings endpoint before provider construction",
            Some("missing_openai_compat_endpoint".to_owned()),
            None,
            None,
        )));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
            EmbeddingProviderKind::OpenAiCompat,
            "semantic runtime provider 'openai_compat' requires a non-empty embeddings endpoint",
            Some("blank_openai_compat_endpoint".to_owned()),
            None,
            None,
        )));
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
            EmbeddingProviderKind::OpenAiCompat,
            format!(
                "semantic runtime provider 'openai_compat' endpoint must be an absolute http(s) URL (received: {trimmed})"
            ),
            Some("invalid_openai_compat_endpoint".to_owned()),
            None,
            None,
        )));
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "local-embeddings")]
    use std::cell::Cell;
    #[cfg(feature = "local-embeddings")]
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::*;

    static PROVIDER_CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn clear_provider_cache_for_test() {
        if let Some(cache) = SEMANTIC_EMBEDDING_PROVIDER_CACHE.get() {
            cache
                .lock()
                .expect("provider cache lock should be available")
                .clear();
        }
    }

    #[cfg(feature = "local-embeddings")]
    fn local_artifact_for_test(cache_root: &str) -> LocalModelArtifact {
        LocalModelArtifact {
            semantic_model: "all-MiniLM-L6-v2".to_owned(),
            cache_root: PathBuf::from(cache_root),
            cache_key: "all-minilm-l6-v2--sentence-transformers-all-MiniLM-L6-v2".to_owned(),
            repository: "sentence-transformers/all-MiniLM-L6-v2".to_owned(),
            repository_cache_dir: PathBuf::from(cache_root)
                .join("models--sentence-transformers--all-MiniLM-L6-v2"),
            model_file: "model.onnx".to_owned(),
            required_files: vec![
                "config.json".to_owned(),
                "model.onnx".to_owned(),
                "tokenizer.json".to_owned(),
            ],
        }
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn require_prepared_policy_does_not_invoke_local_preparer() {
        let called = Cell::new(false);

        prepare_local_artifacts_for_policy_with(
            LocalArtifactPolicy::RequirePrepared,
            "all-MiniLM-L6-v2",
            |_| {
                called.set(true);
                Ok(())
            },
        )
        .expect(
            "RequirePrepared should only require later provider construction to load artifacts",
        );
        assert!(!called.get());
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn allow_preparation_policy_invokes_local_preparer_with_selected_model() {
        let called = Cell::new(false);

        prepare_local_artifacts_for_policy_with(
            LocalArtifactPolicy::AllowPreparation,
            "all-MiniLM-L6-v2",
            |semantic_runtime| {
                called.set(true);
                assert!(semantic_runtime.enabled);
                assert_eq!(
                    semantic_runtime.provider,
                    Some(SemanticRuntimeProvider::Local)
                );
                assert_eq!(
                    semantic_runtime.normalized_model(),
                    Some("all-MiniLM-L6-v2")
                );
                Ok(())
            },
        )
        .expect("AllowPreparation should delegate to explicit local artifact preparation");

        assert!(called.get());
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn local_artifact_cache_identity_includes_cache_root() {
        let first = local_artifact_cache_identity(&local_artifact_for_test("/tmp/frigg-cache-a"));
        let second = local_artifact_cache_identity(&local_artifact_for_test("/tmp/frigg-cache-b"));

        assert_ne!(first, second);
        assert!(first.contains("cache_root=/tmp/frigg-cache-a"));
        assert!(second.contains("cache_root=/tmp/frigg-cache-b"));
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn local_provider_cache_key_canonicalizes_supported_aliases() {
        let credentials = SemanticRuntimeCredentials::default();
        let canonical = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::Local,
            model: "all-MiniLM-L6-v2",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("canonical local model key should build");
        let fastembed_alias = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::Local,
            model: "AllMiniLML6V2",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("fastembed local model alias key should build");
        let repository_alias = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::Local,
            model: "Qdrant/all-MiniLM-L6-v2-onnx",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("repository local model alias key should build");

        assert_eq!(canonical, fastembed_alias);
        assert_eq!(canonical, repository_alias);
        assert_eq!(canonical.model, "all-MiniLM-L6-v2");
    }

    #[test]
    #[cfg(feature = "local-embeddings")]
    fn canonical_provider_model_only_canonicalizes_local_aliases() {
        assert_eq!(
            canonical_provider_model(SemanticRuntimeProvider::Local, "AllMiniLML6V2")
                .expect("local alias should canonicalize"),
            "all-MiniLM-L6-v2"
        );
        assert_eq!(
            canonical_provider_model(SemanticRuntimeProvider::OpenAi, " text-embedding-3-small ")
                .expect("remote model should trim only"),
            "text-embedding-3-small"
        );
    }

    #[test]
    fn provider_build_lock_is_scoped_by_cache_key() {
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-key".to_owned()),
            gemini_api_key: None,
            openai_compat_api_key: None,
        };
        let first_key = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-small",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("remote provider key should build");
        let matching_key = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: " text-embedding-3-small ",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("normalized remote provider key should build");
        let different_key = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-large",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("different remote provider key should build");

        let first_lock = provider_build_lock(&first_key, SemanticRuntimeProvider::OpenAi)
            .expect("first build lock should be available");
        let matching_lock = provider_build_lock(&matching_key, SemanticRuntimeProvider::OpenAi)
            .expect("matching build lock should be available");
        let different_lock = provider_build_lock(&different_key, SemanticRuntimeProvider::OpenAi)
            .expect("different build lock should be available");

        assert!(Arc::ptr_eq(&first_lock, &matching_lock));
        assert!(!Arc::ptr_eq(&first_lock, &different_lock));
    }

    #[test]
    fn cached_provider_reuses_remote_provider_for_same_target() {
        let _guard = PROVIDER_CACHE_TEST_LOCK
            .lock()
            .expect("provider cache test lock should be available");
        clear_provider_cache_for_test();
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-key".to_owned()),
            gemini_api_key: None,
            openai_compat_api_key: None,
        };

        let first = cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-small",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("first provider should build");
        let second = cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: " text-embedding-3-small ",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("second provider should reuse cache");

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn cached_provider_separates_remote_credentials_and_models() {
        let _guard = PROVIDER_CACHE_TEST_LOCK
            .lock()
            .expect("provider cache test lock should be available");
        clear_provider_cache_for_test();
        let first_credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("first-key".to_owned()),
            gemini_api_key: None,
            openai_compat_api_key: None,
        };
        let second_credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("second-key".to_owned()),
            gemini_api_key: None,
            openai_compat_api_key: None,
        };

        let first = cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-small",
            credentials: &first_credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            endpoint: None,
        })
        .expect("first provider should build");
        let different_credentials =
            cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
                provider: SemanticRuntimeProvider::OpenAi,
                model: "text-embedding-3-small",
                credentials: &second_credentials,
                local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
                endpoint: None,
            })
            .expect("different credentials should build a separate provider");
        let different_model =
            cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
                provider: SemanticRuntimeProvider::OpenAi,
                model: "text-embedding-3-large",
                credentials: &first_credentials,
                local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
                endpoint: None,
            })
            .expect("different model should build a separate provider");

        assert!(!Arc::ptr_eq(&first, &different_credentials));
        assert!(!Arc::ptr_eq(&first, &different_model));
    }
}
