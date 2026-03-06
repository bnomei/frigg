use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::domain::{
    FriggError, FriggResult,
    model::{RepositoryId, RepositoryRecord},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_WORKSPACE_ROOT: &str = ".";
pub const DEFAULT_MAX_SEARCH_RESULTS: usize = 200;
pub const DEFAULT_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_WATCH_DEBOUNCE_MS: u64 = 750;
pub const DEFAULT_WATCH_RETRY_MS: u64 = 5_000;
pub const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "text-embedding-3-small";
pub const DEFAULT_GOOGLE_EMBEDDING_MODEL: &str = "gemini-embedding-001";
pub const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
pub const GEMINI_API_KEY_ENV_VAR: &str = "GEMINI_API_KEY";
pub const SEMANTIC_RUNTIME_INVALID_PARAMS_CODE: &str = "invalid_params";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchMode {
    Auto,
    On,
    Off,
}

impl WatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

impl Default for WatchMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl std::fmt::Display for WatchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for WatchMode {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "auto" => Ok(Self::Auto),
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            _ => Err(format!(
                "watch mode must be one of: auto, on, off (received: {normalized})"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTransportKind {
    Stdio,
    LoopbackHttp,
    RemoteHttp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchConfig {
    pub mode: WatchMode,
    pub debounce_ms: u64,
    pub retry_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            mode: WatchMode::Auto,
            debounce_ms: DEFAULT_WATCH_DEBOUNCE_MS,
            retry_ms: DEFAULT_WATCH_RETRY_MS,
        }
    }
}

impl WatchConfig {
    pub fn default_for_transport(transport: RuntimeTransportKind) -> Self {
        let mut watch = Self::default();
        if transport == RuntimeTransportKind::Stdio {
            watch.mode = WatchMode::Off;
        }
        watch
    }

    pub fn enabled_for_transport(&self, transport: RuntimeTransportKind) -> bool {
        match self.mode {
            WatchMode::On => true,
            WatchMode::Off => false,
            WatchMode::Auto => matches!(
                transport,
                RuntimeTransportKind::Stdio | RuntimeTransportKind::LoopbackHttp
            ),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticRuntimeConfig {
    pub enabled: bool,
    pub provider: Option<SemanticRuntimeProvider>,
    pub model: Option<String>,
    pub strict_mode: bool,
}

impl Default for SemanticRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            model: None,
            strict_mode: false,
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriggConfig {
    pub workspace_roots: Vec<PathBuf>,
    pub max_search_results: usize,
    pub max_file_bytes: usize,
    pub watch: WatchConfig,
    pub semantic_runtime: SemanticRuntimeConfig,
}

impl Default for FriggConfig {
    fn default() -> Self {
        Self {
            workspace_roots: vec![PathBuf::from(DEFAULT_WORKSPACE_ROOT)],
            max_search_results: DEFAULT_MAX_SEARCH_RESULTS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            watch: WatchConfig::default(),
            semantic_runtime: SemanticRuntimeConfig::default(),
        }
    }
}

impl FriggConfig {
    pub fn from_workspace_roots(workspace_roots: Vec<PathBuf>) -> FriggResult<Self> {
        Self::from_workspace_roots_with_mode(workspace_roots, true)
    }

    pub fn from_optional_workspace_roots(workspace_roots: Vec<PathBuf>) -> FriggResult<Self> {
        Self::from_workspace_roots_with_mode(workspace_roots, false)
    }

    fn from_workspace_roots_with_mode(
        workspace_roots: Vec<PathBuf>,
        default_when_empty: bool,
    ) -> FriggResult<Self> {
        let roots = if workspace_roots.is_empty() {
            if default_when_empty {
                vec![PathBuf::from(DEFAULT_WORKSPACE_ROOT)]
            } else {
                Vec::new()
            }
        } else {
            workspace_roots
        };

        let cfg = Self {
            workspace_roots: roots,
            ..Self::default()
        };
        if default_when_empty {
            cfg.validate()?;
        } else {
            cfg.validate_for_serving()?;
        }
        Ok(cfg)
    }

    pub fn validate(&self) -> FriggResult<()> {
        self.validate_with_root_requirement(true)
    }

    pub fn validate_for_serving(&self) -> FriggResult<()> {
        self.validate_with_root_requirement(false)
    }

    pub fn ensure_workspace_roots_configured(&self) -> FriggResult<()> {
        if self.workspace_roots.is_empty() {
            return Err(FriggError::InvalidInput(
                "at least one workspace root is required".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_with_root_requirement(&self, require_workspace_roots: bool) -> FriggResult<()> {
        if require_workspace_roots {
            self.ensure_workspace_roots_configured()?;
        }

        if self.max_search_results == 0 {
            return Err(FriggError::InvalidInput(
                "max_search_results must be greater than zero".to_owned(),
            ));
        }

        if self.max_file_bytes == 0 {
            return Err(FriggError::InvalidInput(
                "max_file_bytes must be greater than zero".to_owned(),
            ));
        }

        if self.watch.debounce_ms == 0 {
            return Err(FriggError::InvalidInput(
                "watch.debounce_ms must be greater than zero".to_owned(),
            ));
        }

        if self.watch.retry_ms == 0 {
            return Err(FriggError::InvalidInput(
                "watch.retry_ms must be greater than zero".to_owned(),
            ));
        }

        for root in &self.workspace_roots {
            if !root.exists() {
                return Err(FriggError::InvalidInput(format!(
                    "workspace root does not exist: {}",
                    root.display()
                )));
            }
        }

        self.semantic_runtime
            .validate()
            .map_err(|err| FriggError::InvalidInput(err.to_string()))?;

        Ok(())
    }

    pub fn repositories(&self) -> Vec<RepositoryRecord> {
        self.workspace_roots
            .iter()
            .enumerate()
            .map(|(idx, root)| RepositoryRecord {
                repository_id: RepositoryId(format!("repo-{:03}", idx + 1)),
                display_name: root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| root.display().to_string()),
                root_path: root.display().to_string(),
            })
            .collect()
    }

    pub fn root_by_repository_id(&self, repository_id: &str) -> Option<&Path> {
        self.repositories()
            .into_iter()
            .zip(self.workspace_roots.iter())
            .find_map(|(repo, root)| {
                (repo.repository_id.0 == repository_id).then_some(root.as_path())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing_workspace_root() -> PathBuf {
        std::env::current_dir().expect("current working directory should exist for tests")
    }

    #[test]
    fn semantic_runtime_disabled_defaults_validate() {
        let config = SemanticRuntimeConfig::default();
        assert!(!config.enabled);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn watch_config_defaults_enable_local_transports_only() {
        let watch = WatchConfig::default();
        assert_eq!(watch.mode, WatchMode::Auto);
        assert_eq!(watch.debounce_ms, DEFAULT_WATCH_DEBOUNCE_MS);
        assert_eq!(watch.retry_ms, DEFAULT_WATCH_RETRY_MS);
        assert!(watch.enabled_for_transport(RuntimeTransportKind::Stdio));
        assert!(watch.enabled_for_transport(RuntimeTransportKind::LoopbackHttp));
        assert!(!watch.enabled_for_transport(RuntimeTransportKind::RemoteHttp));
    }

    #[test]
    fn watch_mode_parsing_and_transport_override_behave_as_expected() {
        assert_eq!(
            "auto".parse::<WatchMode>().unwrap_or(WatchMode::Off),
            WatchMode::Auto
        );
        assert_eq!(
            "on".parse::<WatchMode>().unwrap_or(WatchMode::Off),
            WatchMode::On
        );
        assert_eq!(
            "off".parse::<WatchMode>().unwrap_or(WatchMode::On),
            WatchMode::Off
        );
        assert!("wat".parse::<WatchMode>().is_err());

        let on = WatchConfig {
            mode: WatchMode::On,
            debounce_ms: DEFAULT_WATCH_DEBOUNCE_MS,
            retry_ms: DEFAULT_WATCH_RETRY_MS,
        };
        assert!(on.enabled_for_transport(RuntimeTransportKind::RemoteHttp));

        let off = WatchConfig {
            mode: WatchMode::Off,
            debounce_ms: DEFAULT_WATCH_DEBOUNCE_MS,
            retry_ms: DEFAULT_WATCH_RETRY_MS,
        };
        assert!(!off.enabled_for_transport(RuntimeTransportKind::Stdio));
    }

    #[test]
    fn watch_config_transport_defaults_disable_stdio_by_default() {
        let stdio = WatchConfig::default_for_transport(RuntimeTransportKind::Stdio);
        assert_eq!(stdio.mode, WatchMode::Off);
        assert_eq!(stdio.debounce_ms, DEFAULT_WATCH_DEBOUNCE_MS);
        assert_eq!(stdio.retry_ms, DEFAULT_WATCH_RETRY_MS);

        let http = WatchConfig::default_for_transport(RuntimeTransportKind::LoopbackHttp);
        assert_eq!(http.mode, WatchMode::Auto);
    }

    #[test]
    fn semantic_runtime_enabled_requires_provider_and_rejects_blank_model() {
        let missing_provider = SemanticRuntimeConfig {
            enabled: true,
            provider: None,
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        };
        let err = missing_provider
            .validate()
            .expect_err("enabled semantic runtime must require provider");
        assert_eq!(err.code(), SEMANTIC_RUNTIME_INVALID_PARAMS_CODE);
        assert_eq!(
            err.to_string(),
            "semantic_runtime.provider is required when semantic_runtime.enabled=true"
        );

        let blank_model = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("   ".to_owned()),
            strict_mode: false,
        };
        let err = blank_model
            .validate()
            .expect_err("enabled semantic runtime must reject blank model");
        assert_eq!(err.code(), SEMANTIC_RUNTIME_INVALID_PARAMS_CODE);
        assert_eq!(
            err.to_string(),
            "semantic_runtime.model must not be blank when semantic_runtime.enabled=true"
        );
    }

    #[test]
    fn semantic_runtime_enabled_defaults_model_from_provider() {
        let openai = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: None,
            strict_mode: false,
        };
        openai
            .validate()
            .expect("enabled semantic runtime should allow provider default model");
        assert_eq!(
            openai.normalized_model(),
            Some(DEFAULT_OPENAI_EMBEDDING_MODEL)
        );

        let google = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::Google),
            model: None,
            strict_mode: false,
        };
        google
            .validate()
            .expect("enabled semantic runtime should allow provider default model");
        assert_eq!(
            google.normalized_model(),
            Some(DEFAULT_GOOGLE_EMBEDDING_MODEL)
        );
    }

    #[test]
    fn semantic_runtime_startup_requires_provider_credentials() {
        let runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: None,
            strict_mode: false,
        };

        let missing_key = runtime
            .validate_startup(&SemanticRuntimeCredentials::default())
            .expect_err("startup validation should require provider api key");
        assert_eq!(missing_key.code(), SEMANTIC_RUNTIME_INVALID_PARAMS_CODE);
        assert_eq!(
            missing_key.to_string(),
            "semantic runtime provider=openai requires OPENAI_API_KEY to be set"
        );

        let blank_key = runtime
            .validate_startup(&SemanticRuntimeCredentials {
                openai_api_key: Some("   ".to_owned()),
                gemini_api_key: None,
            })
            .expect_err("startup validation should reject blank provider api key");
        assert_eq!(blank_key.code(), SEMANTIC_RUNTIME_INVALID_PARAMS_CODE);
        assert_eq!(
            blank_key.to_string(),
            "semantic runtime provider=openai requires OPENAI_API_KEY to be non-empty"
        );

        runtime
            .validate_startup(&SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            })
            .expect("startup validation should pass with non-empty openai key");
    }

    #[test]
    fn frigg_config_validate_surfaces_semantic_runtime_failures() {
        let config = FriggConfig {
            workspace_roots: vec![existing_workspace_root()],
            max_search_results: DEFAULT_MAX_SEARCH_RESULTS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            watch: WatchConfig::default(),
            semantic_runtime: SemanticRuntimeConfig {
                enabled: true,
                provider: None,
                model: Some("text-embedding-3-small".to_owned()),
                strict_mode: false,
            },
        };

        let err = config
            .validate()
            .expect_err("semantic runtime config errors should fail FriggConfig validation");
        assert!(
            matches!(err, FriggError::InvalidInput(_)),
            "expected invalid input error, got: {err:?}"
        );
        assert_eq!(
            err.to_string(),
            "invalid input: semantic_runtime.provider is required when semantic_runtime.enabled=true"
        );
    }

    #[test]
    fn frigg_config_default_uses_aggressive_max_file_bytes_budget() {
        let config = FriggConfig::default();
        assert_eq!(config.max_file_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn frigg_config_serving_mode_allows_empty_workspace_roots() {
        let config = FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("serving config should allow empty workspace roots");
        assert!(config.workspace_roots.is_empty());
        config
            .validate_for_serving()
            .expect("serving validation should allow empty workspace roots");
        let err = config
            .validate()
            .expect_err("command validation should still require workspace roots");
        assert_eq!(
            err.to_string(),
            "invalid input: at least one workspace root is required"
        );
    }

    #[test]
    fn frigg_config_rejects_zero_watch_timers() {
        let mut config = FriggConfig::default();
        config.workspace_roots = vec![existing_workspace_root()];
        config.watch.debounce_ms = 0;
        let debounce_err = config
            .validate()
            .expect_err("watch debounce must reject zero");
        assert_eq!(
            debounce_err.to_string(),
            "invalid input: watch.debounce_ms must be greater than zero"
        );

        config.watch.debounce_ms = DEFAULT_WATCH_DEBOUNCE_MS;
        config.watch.retry_ms = 0;
        let retry_err = config.validate().expect_err("watch retry must reject zero");
        assert_eq!(
            retry_err.to_string(),
            "invalid input: watch.retry_ms must be greater than zero"
        );
    }
}
