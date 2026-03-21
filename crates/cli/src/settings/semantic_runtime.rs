use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
pub const DEFAULT_GOOGLE_EMBEDDING_MODEL: &str = "gemini-embedding-001";
pub const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
pub const GEMINI_API_KEY_ENV_VAR: &str = "GEMINI_API_KEY";
pub const SEMANTIC_RUNTIME_INVALID_PARAMS_CODE: &str = "invalid_params";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRuntimeProvider {
    OpenAi,
    Google,
}

impl SemanticRuntimeProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Google => "google",
        }
    }

    pub fn required_api_key_env_var(self) -> &'static str {
        match self {
            Self::OpenAi => OPENAI_API_KEY_ENV_VAR,
            Self::Google => GEMINI_API_KEY_ENV_VAR,
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => DEFAULT_OPENAI_EMBEDDING_MODEL,
            Self::Google => DEFAULT_GOOGLE_EMBEDDING_MODEL,
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
            _ => Err(format!(
                "semantic runtime provider must be one of: openai, google (received: {normalized})"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
/// Controls whether semantic indexing and semantic recall are part of the runtime contract.
pub struct SemanticRuntimeConfig {
    pub enabled: bool,
    pub provider: Option<SemanticRuntimeProvider>,
    pub model: Option<String>,
    pub strict_mode: bool,
}

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
        }
    }
}

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
        let env_var = provider.required_api_key_env_var();
        let Some(api_key) = credentials.api_key_for(provider) else {
            return Err(SemanticRuntimeCredentialError::MissingApiKey { provider, env_var }.into());
        };
        if api_key.trim().is_empty() {
            return Err(SemanticRuntimeCredentialError::BlankApiKey { provider, env_var }.into());
        }

        Ok(())
    }
}
