//! Shared semantic embedding provider construction for indexing and query-time recall.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};

use super::local::{local_model_error_to_embedding_error, reject_hf_home_override_for_provider};
use super::local_model::{
    LocalModelArtifact, prepare_local_semantic_model, require_prepared_local_model,
    resolve_local_model_artifact,
};
use super::{
    EmbeddingError, EmbeddingProvider, EmbeddingProviderKind, EmbeddingResult,
    GoogleEmbeddingProvider, LocalEmbeddingProvider, OpenAiEmbeddingProvider, ProviderFailure,
};

/// Policy controlling whether local model artifacts may be prepared during provider construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalArtifactPolicy {
    RequirePrepared,
    AllowPreparation,
}

/// Inputs required to construct a semantic embedding provider for indexing or recall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEmbeddingProviderFactoryConfig<'a> {
    pub provider: SemanticRuntimeProvider,
    pub model: &'a str,
    pub credentials: &'a SemanticRuntimeCredentials,
    pub local_artifact_policy: LocalArtifactPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SemanticEmbeddingProviderCacheKey {
    provider: &'static str,
    model: String,
    credential_fingerprint: Option<String>,
    local_artifact_identity: Option<String>,
}

static SEMANTIC_EMBEDDING_PROVIDER_CACHE: OnceLock<
    Mutex<BTreeMap<SemanticEmbeddingProviderCacheKey, Arc<dyn EmbeddingProvider>>>,
> = OnceLock::new();
static SEMANTIC_EMBEDDING_PROVIDER_BUILD_LOCKS: OnceLock<
    Mutex<BTreeMap<SemanticEmbeddingProviderCacheKey, Arc<Mutex<()>>>>,
> = OnceLock::new();

/// Builds the active semantic embedding provider from runtime configuration and credentials.
pub fn build_semantic_embedding_provider(
    config: SemanticEmbeddingProviderFactoryConfig<'_>,
) -> EmbeddingResult<Arc<dyn EmbeddingProvider>> {
    let model = config.model.trim();
    match config.provider {
        SemanticRuntimeProvider::OpenAi => {
            let api_key = require_api_key(config.provider, config.credentials)?;
            Ok(Arc::new(OpenAiEmbeddingProvider::new(api_key.to_owned())))
        }
        SemanticRuntimeProvider::Google => {
            let api_key = require_api_key(config.provider, config.credentials)?;
            Ok(Arc::new(GoogleEmbeddingProvider::new(api_key.to_owned())))
        }
        SemanticRuntimeProvider::Local => {
            prepare_local_artifacts_for_policy(config.local_artifact_policy, model)?;
            Ok(Arc::new(LocalEmbeddingProvider::new(model)?))
        }
    }
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
    let local_artifact_identity = match config.provider {
        SemanticRuntimeProvider::Local => Some(resolve_local_artifact_identity(config.model)?),
        SemanticRuntimeProvider::OpenAi | SemanticRuntimeProvider::Google => None,
    };

    Ok(SemanticEmbeddingProviderCacheKey {
        provider: config.provider.as_str(),
        model: config.model.trim().to_owned(),
        credential_fingerprint: config
            .credentials
            .api_key_for(config.provider)
            .map(|api_key| blake3::hash(api_key.as_bytes()).to_hex().to_string()),
        local_artifact_identity,
    })
}

fn prepare_local_artifacts_for_policy(
    policy: LocalArtifactPolicy,
    model: &str,
) -> EmbeddingResult<()> {
    validate_local_artifacts_for_policy(policy, model).map(|_| ())
}

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

fn resolve_local_artifact_identity(model: &str) -> EmbeddingResult<String> {
    let artifact =
        resolve_local_model_artifact(model).map_err(local_model_error_to_embedding_error)?;
    reject_hf_home_override_for_provider(&artifact, hf_home_from_process_env())?;
    Ok(local_artifact_cache_identity(&artifact))
}

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
            };
            prepare(&semantic_runtime)
        }
    }
}

fn hf_home_from_process_env() -> Option<PathBuf> {
    std::env::var_os("HF_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn require_api_key<'a>(
    provider: SemanticRuntimeProvider,
    credentials: &'a SemanticRuntimeCredentials,
) -> EmbeddingResult<&'a str> {
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
        SemanticRuntimeProvider::Google => EmbeddingProviderKind::Google,
        SemanticRuntimeProvider::Local => EmbeddingProviderKind::Local,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
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
    fn require_prepared_policy_does_not_invoke_local_preparer() {
        prepare_local_artifacts_for_policy_with(
            LocalArtifactPolicy::RequirePrepared,
            "all-MiniLM-L6-v2",
            |_| panic!("RequirePrepared must not prepare or download local artifacts"),
        )
        .expect(
            "RequirePrepared should only require later provider construction to load artifacts",
        );
    }

    #[test]
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
    fn local_artifact_cache_identity_includes_cache_root() {
        let first = local_artifact_cache_identity(&local_artifact_for_test("/tmp/frigg-cache-a"));
        let second = local_artifact_cache_identity(&local_artifact_for_test("/tmp/frigg-cache-b"));

        assert_ne!(first, second);
        assert!(first.contains("cache_root=/tmp/frigg-cache-a"));
        assert!(second.contains("cache_root=/tmp/frigg-cache-b"));
    }

    #[test]
    fn provider_build_lock_is_scoped_by_cache_key() {
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-key".to_owned()),
            gemini_api_key: None,
        };
        let first_key = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-small",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
        })
        .expect("remote provider key should build");
        let matching_key = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: " text-embedding-3-small ",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
        })
        .expect("normalized remote provider key should build");
        let different_key = provider_cache_key(&SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-large",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
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
        };

        let first = cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-small",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
        })
        .expect("first provider should build");
        let second = cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: " text-embedding-3-small ",
            credentials: &credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
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
        };
        let second_credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("second-key".to_owned()),
            gemini_api_key: None,
        };

        let first = cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
            provider: SemanticRuntimeProvider::OpenAi,
            model: "text-embedding-3-small",
            credentials: &first_credentials,
            local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
        })
        .expect("first provider should build");
        let different_credentials =
            cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
                provider: SemanticRuntimeProvider::OpenAi,
                model: "text-embedding-3-small",
                credentials: &second_credentials,
                local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            })
            .expect("different credentials should build a separate provider");
        let different_model =
            cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
                provider: SemanticRuntimeProvider::OpenAi,
                model: "text-embedding-3-large",
                credentials: &first_credentials,
                local_artifact_policy: LocalArtifactPolicy::RequirePrepared,
            })
            .expect("different model should build a separate provider");

        assert!(!Arc::ptr_eq(&first, &different_credentials));
        assert!(!Arc::ptr_eq(&first, &different_model));
    }
}
