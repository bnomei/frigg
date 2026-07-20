//! Semantic runtime configuration, credential validation, and provider defaults.
//!
//! Validates provider credentials and model aliases at startup so semantic indexing and recall
//! fail fast with MCP-friendly diagnostics instead of deferring errors to the first embed call.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default OpenAI embedding model when semantic runtime omits an explicit model.
pub const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
/// Default Google embedding model when semantic runtime omits an explicit model.
pub const DEFAULT_GOOGLE_EMBEDDING_MODEL: &str = "gemini-embedding-001";
/// Default local embedding model when semantic runtime omits an explicit model.
pub const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";
/// Protocol-default model string for OpenAI-compatible endpoints when unset.
///
/// Operators should set `--semantic-runtime-model` / `FRIGG_SEMANTIC_RUNTIME_MODEL` to the
/// backend's actual model id; this default only matches common OpenAI-protocol proxies.
pub const DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL: &str = DEFAULT_OPENAI_EMBEDDING_MODEL;
/// Environment variable holding the OpenAI API key for semantic runtime startup.
pub const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
/// Environment variable holding the Gemini API key for semantic runtime startup.
pub const GEMINI_API_KEY_ENV_VAR: &str = "GEMINI_API_KEY";
/// Environment variable holding the API key for `openai_compat` (Bearer token).
///
/// Local OpenAI-protocol servers often accept any non-empty value.
pub const OPENAI_COMPAT_API_KEY_ENV_VAR: &str = "FRIGG_OPENAI_COMPAT_API_KEY";
/// Environment variable for the full embeddings POST URL used by `openai_compat`.
pub const OPENAI_COMPAT_ENDPOINT_ENV_VAR: &str = "FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT";
/// MCP error code returned for invalid semantic runtime configuration.
pub const SEMANTIC_RUNTIME_INVALID_PARAMS_CODE: &str = "invalid_params";

/// Embedding provider selected for semantic indexing and recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRuntimeProvider {
    OpenAi,
    /// OpenAI-compatible HTTP embeddings (Azure-compatible, vLLM, LM Studio, gateways).
    ///
    /// Same wire protocol as [`Self::OpenAi`], but requires an explicit endpoint and uses a
    /// distinct provider identity for storage partitions and credentials.
    OpenAiCompat,
    Google,
    Local,
}

impl SemanticRuntimeProvider {
    /// Snake-case provider id used in config, storage partitions, and operator-facing status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::OpenAiCompat => "openai_compat",
            Self::Google => "google",
            Self::Local => "local",
        }
    }

    /// Process env var that must hold a non-empty API key when this provider is enabled.
    ///
    /// Local embeddings do not need a key. `openai_compat` prefers its dedicated var but may
    /// fall back to `OPENAI_API_KEY` via [`SemanticRuntimeCredentials::api_key_for`].
    pub fn api_key_env_var(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some(OPENAI_API_KEY_ENV_VAR),
            Self::OpenAiCompat => Some(OPENAI_COMPAT_API_KEY_ENV_VAR),
            Self::Google => Some(GEMINI_API_KEY_ENV_VAR),
            Self::Local => None,
        }
    }

    /// Built-in embedding model id when operators omit `--semantic-runtime-model`.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi | Self::OpenAiCompat => DEFAULT_OPENAI_EMBEDDING_MODEL,
            Self::Google => DEFAULT_GOOGLE_EMBEDDING_MODEL,
            Self::Local => DEFAULT_LOCAL_EMBEDDING_MODEL,
        }
    }

    /// Whether config must supply a full embeddings HTTP URL (`openai_compat` only).
    pub fn requires_endpoint(self) -> bool {
        matches!(self, Self::OpenAiCompat)
    }
}

impl std::fmt::Display for SemanticRuntimeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SemanticRuntimeProvider {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "openai" => Ok(Self::OpenAi),
            "openai_compat" | "openai-compat" | "openai_compatible" => Ok(Self::OpenAiCompat),
            "google" => Ok(Self::Google),
            "local" => Ok(Self::Local),
            _ => Err(format!(
                "semantic runtime provider must be one of: openai, openai_compat, google, local (received: {normalized})"
            )),
        }
    }
}

/// Controls whether semantic indexing and recall participate in the runtime contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SemanticRuntimeConfig {
    /// When false, semantic channels stay idle and startup skips credential checks.
    pub enabled: bool,
    /// Embedding backend; required when [`Self::enabled`] is true.
    pub provider: Option<SemanticRuntimeProvider>,
    /// Provider model id; blank strings fail validation; `None` uses the provider default.
    pub model: Option<String>,
    /// When true, serve/index fail closed if the embedding provider is not ready.
    pub strict_mode: bool,
    /// Full embeddings HTTP URL for [`SemanticRuntimeProvider::OpenAiCompat`].
    ///
    /// Example: `http://127.0.0.1:1234/v1/embeddings` or an Azure-compatible deployment URL.
    /// Required when `provider=openai_compat`. Ignored for other providers.
    pub openai_compat_endpoint: Option<String>,
}

/// Process-environment API keys consumed during semantic runtime startup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticRuntimeCredentials {
    /// Value of [`OPENAI_API_KEY_ENV_VAR`], also used as `openai_compat` fallback.
    pub openai_api_key: Option<String>,
    /// Value of [`OPENAI_COMPAT_API_KEY_ENV_VAR`] for OpenAI-protocol proxies.
    pub openai_compat_api_key: Option<String>,
    /// Value of [`GEMINI_API_KEY_ENV_VAR`] for Google embeddings.
    pub gemini_api_key: Option<String>,
}

impl SemanticRuntimeCredentials {
    /// Loads API keys from the process environment without validating presence or non-emptiness.
    pub fn from_process_env() -> Self {
        Self {
            openai_api_key: std::env::var(OPENAI_API_KEY_ENV_VAR).ok(),
            openai_compat_api_key: std::env::var(OPENAI_COMPAT_API_KEY_ENV_VAR).ok(),
            gemini_api_key: std::env::var(GEMINI_API_KEY_ENV_VAR).ok(),
        }
    }

    /// Credential for `provider`. `OpenAiCompat` uses its dedicated key, then falls back to
    /// `OPENAI_API_KEY` for shared OpenAI-compatible gateways.
    pub fn api_key_for(&self, provider: SemanticRuntimeProvider) -> Option<&str> {
        match provider {
            SemanticRuntimeProvider::OpenAi => self.openai_api_key.as_deref(),
            SemanticRuntimeProvider::OpenAiCompat => self
                .openai_compat_api_key
                .as_deref()
                .or(self.openai_api_key.as_deref()),
            SemanticRuntimeProvider::Google => self.gemini_api_key.as_deref(),
            SemanticRuntimeProvider::Local => None,
        }
    }
}

/// Configuration validation failures for enabled semantic runtime settings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticRuntimeConfigError {
    #[error("semantic_runtime.provider is required when semantic_runtime.enabled=true")]
    MissingProvider,
    #[error("semantic_runtime.model must not be blank when semantic_runtime.enabled=true")]
    BlankModel,
    #[error(
        "semantic_runtime.openai_compat_endpoint is required when provider=openai_compat (set --semantic-runtime-openai-compat-endpoint or {OPENAI_COMPAT_ENDPOINT_ENV_VAR})"
    )]
    MissingOpenAiCompatEndpoint,
    #[error(
        "semantic_runtime.openai_compat_endpoint must not be blank when provider=openai_compat"
    )]
    BlankOpenAiCompatEndpoint,
    #[error(
        "semantic_runtime.openai_compat_endpoint must be an absolute http(s) URL (received: {endpoint})"
    )]
    InvalidOpenAiCompatEndpoint { endpoint: String },
}

impl SemanticRuntimeConfigError {
    /// MCP-facing error code for invalid semantic runtime configuration.
    pub fn code(&self) -> &'static str {
        SEMANTIC_RUNTIME_INVALID_PARAMS_CODE
    }
}

/// Credential validation failures encountered during semantic runtime startup.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticRuntimeCredentialError {
    #[error("semantic runtime provider={provider} requires {env_var} to be set")]
    MissingApiKey {
        provider: SemanticRuntimeProvider,
        env_var: &'static str,
    },
    #[error("semantic runtime provider={provider} requires {env_var} to be non-empty")]
    BlankApiKey {
        provider: SemanticRuntimeProvider,
        env_var: &'static str,
    },
}

impl SemanticRuntimeCredentialError {
    /// MCP-facing error code for missing or blank provider credentials.
    pub fn code(&self) -> &'static str {
        SEMANTIC_RUNTIME_INVALID_PARAMS_CODE
    }
}

/// Combined configuration and credential failures for semantic runtime startup.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticRuntimeStartupError {
    #[error("{0}")]
    Config(#[from] SemanticRuntimeConfigError),
    #[error("{0}")]
    Credentials(#[from] SemanticRuntimeCredentialError),
}

impl SemanticRuntimeStartupError {
    /// MCP-facing error code shared by config and credential startup failures.
    pub fn code(&self) -> &'static str {
        SEMANTIC_RUNTIME_INVALID_PARAMS_CODE
    }
}

impl SemanticRuntimeConfig {
    /// Checks provider/model/endpoint shape when semantic runtime is enabled; no-op when disabled.
    pub fn validate(&self) -> Result<(), SemanticRuntimeConfigError> {
        if !self.enabled {
            return Ok(());
        }

        let provider = self
            .provider
            .ok_or(SemanticRuntimeConfigError::MissingProvider)?;

        if self
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(SemanticRuntimeConfigError::BlankModel);
        }

        if provider.requires_endpoint() {
            self.validate_openai_compat_endpoint()?;
        }

        Ok(())
    }

    /// Trimmed configured model, or the provider default when model is unset.
    pub fn normalized_model(&self) -> Option<&str> {
        match self.model.as_deref() {
            Some(model) => {
                let normalized = model.trim();
                if normalized.is_empty() {
                    None
                } else {
                    Some(normalized)
                }
            }
            None => self.provider.map(SemanticRuntimeProvider::default_model),
        }
    }

    /// Normalized full embeddings URL for `openai_compat`, when configured.
    pub fn normalized_openai_compat_endpoint(&self) -> Option<&str> {
        self.openai_compat_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn validate_openai_compat_endpoint(&self) -> Result<(), SemanticRuntimeConfigError> {
        let Some(raw) = self.openai_compat_endpoint.as_deref() else {
            return Err(SemanticRuntimeConfigError::MissingOpenAiCompatEndpoint);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(SemanticRuntimeConfigError::BlankOpenAiCompatEndpoint);
        }
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Err(SemanticRuntimeConfigError::InvalidOpenAiCompatEndpoint {
                endpoint: trimmed.to_owned(),
            });
        }
        Ok(())
    }

    /// Validates config shape plus process credentials so serve/index fail before the first embed.
    pub fn validate_startup(
        &self,
        credentials: &SemanticRuntimeCredentials,
    ) -> Result<(), SemanticRuntimeStartupError> {
        self.validate()?;
        if !self.enabled {
            return Ok(());
        }

        let provider = self
            .provider
            .ok_or(SemanticRuntimeConfigError::MissingProvider)?;
        if let Some(env_var) = provider.api_key_env_var() {
            let Some(api_key) = credentials.api_key_for(provider) else {
                return Err(
                    SemanticRuntimeCredentialError::MissingApiKey { provider, env_var }.into(),
                );
            };
            if api_key.trim().is_empty() {
                return Err(
                    SemanticRuntimeCredentialError::BlankApiKey { provider, env_var }.into(),
                );
            }
        }

        Ok(())
    }
}
