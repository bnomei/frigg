//! Shared semantic embedding provider construction for indexing and query-time recall.

use std::sync::Arc;

use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};

use super::local::local_model_error_to_embedding_error;
use super::local_model::prepare_local_semantic_model;
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

fn prepare_local_artifacts_for_policy(
    policy: LocalArtifactPolicy,
    model: &str,
) -> EmbeddingResult<()> {
    prepare_local_artifacts_for_policy_with(policy, model, |semantic_runtime| {
        prepare_local_semantic_model(semantic_runtime)
            .map(|_| ())
            .map_err(local_model_error_to_embedding_error)
    })
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

    use super::*;

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
}
