//! Semantic runtime configuration, credential validation, and provider defaults.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default OpenAI embedding model when semantic runtime omits an explicit model.
pub const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
/// Default Google embedding model when semantic runtime omits an explicit model.
pub const DEFAULT_GOOGLE_EMBEDDING_MODEL: &str = "gemini-embedding-001";
/// Default local embedding model when semantic runtime omits an explicit model.
pub const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";
/// Environment variable holding the OpenAI API key for semantic runtime startup.
pub const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
/// Environment variable holding the Gemini API key for semantic runtime startup.
pub const GEMINI_API_KEY_ENV_VAR: &str = "GEMINI_API_KEY";
/// MCP error code returned for invalid semantic runtime configuration.
pub const SEMANTIC_RUNTIME_INVALID_PARAMS_CODE: &str = "invalid_params";

/// Embedding provider selected for semantic indexing and recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRuntimeProvider {
    OpenAi,
    Google,
    Local,
}

impl SemanticRuntimeProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Google => "google",
            Self::Local => "local",
        }
    }

    pub fn api_key_env_var(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some(OPENAI_API_KEY_ENV_VAR),
            Self::Google => Some(GEMINI_API_KEY_ENV_VAR),
            Self::Local => None,
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => DEFAULT_OPENAI_EMBEDDING_MODEL,
            Self::Google => DEFAULT_GOOGLE_EMBEDDING_MODEL,
            Self::Local => DEFAULT_LOCAL_EMBEDDING_MODEL,
        }
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
            "google" => Ok(Self::Google),
            "local" => Ok(Self::Local),
            _ => Err(format!(
                "semantic runtime provider must be one of: openai, google, local (received: {normalized})"
            )),
        }
    }
}

/// Controls whether semantic indexing and recall participate in the runtime contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SemanticRuntimeConfig {
    pub enabled: bool,
    pub provider: Option<SemanticRuntimeProvider>,
    pub model: Option<String>,
    pub strict_mode: bool,
}

/// Process-environment API keys consumed during semantic runtime startup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SemanticRuntimeCredentials {
    pub openai_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
}

impl SemanticRuntimeCredentials {
    pub fn from_process_env() -> Self {
        Self {
            openai_api_key: std::env::var(OPENAI_API_KEY_ENV_VAR).ok(),
            gemini_api_key: std::env::var(GEMINI_API_KEY_ENV_VAR).ok(),
        }
    }

    pub fn api_key_for(&self, provider: SemanticRuntimeProvider) -> Option<&str> {
        match provider {
            SemanticRuntimeProvider::OpenAi => self.openai_api_key.as_deref(),
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
}

impl SemanticRuntimeConfigError {
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
    pub fn code(&self) -> &'static str {
        SEMANTIC_RUNTIME_INVALID_PARAMS_CODE
    }
}

impl SemanticRuntimeConfig {
    pub fn validate(&self) -> Result<(), SemanticRuntimeConfigError> {
        if !self.enabled {
            return Ok(());
        }

        self.provider
            .ok_or(SemanticRuntimeConfigError::MissingProvider)?;

        if self
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(SemanticRuntimeConfigError::BlankModel);
        }

        Ok(())
    }

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
