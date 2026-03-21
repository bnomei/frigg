//! Runtime configuration types that decide what Frigg can serve and which background services
//! should be active. Centralizing these switches keeps CLI, indexing, watch, and MCP startup on
//! the same operating profile.

mod frigg_config;
mod lexical_runtime;
mod runtime_profile;
mod semantic_runtime;
mod watch;

#[cfg(test)]
use std::path::PathBuf;

pub use frigg_config::{
    DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_SEARCH_RESULTS, DEFAULT_WORKSPACE_ROOT, FriggConfig,
};
pub use lexical_runtime::{LexicalBackendMode, LexicalRuntimeConfig};
pub use runtime_profile::{RuntimeProfile, RuntimeTransportKind, runtime_profile_for_transport};
pub use semantic_runtime::{
    DEFAULT_GOOGLE_EMBEDDING_MODEL, DEFAULT_OPENAI_EMBEDDING_MODEL, GEMINI_API_KEY_ENV_VAR,
    OPENAI_API_KEY_ENV_VAR, SEMANTIC_RUNTIME_INVALID_PARAMS_CODE, SemanticRuntimeConfig,
    SemanticRuntimeConfigError, SemanticRuntimeCredentialError, SemanticRuntimeCredentials,
    SemanticRuntimeProvider, SemanticRuntimeStartupError,
};
pub use watch::{DEFAULT_WATCH_DEBOUNCE_MS, DEFAULT_WATCH_RETRY_MS, WatchConfig, WatchMode};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FriggError;

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
    fn lexical_runtime_defaults_and_parsing_are_stable() {
        let config = LexicalRuntimeConfig::default();
        assert_eq!(config.backend, LexicalBackendMode::Auto);
        assert_eq!(
            "auto"
                .parse::<LexicalBackendMode>()
                .unwrap_or(LexicalBackendMode::Native),
            LexicalBackendMode::Auto
        );
        assert_eq!(
            "native"
                .parse::<LexicalBackendMode>()
                .unwrap_or(LexicalBackendMode::Auto),
            LexicalBackendMode::Native
        );
        assert_eq!(
            "rg".parse::<LexicalBackendMode>()
                .unwrap_or(LexicalBackendMode::Auto),
            LexicalBackendMode::Ripgrep
        );
        assert!("wat".parse::<LexicalBackendMode>().is_err());
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
    fn runtime_profile_resolution_distinguishes_ephemeral_and_persistent_modes() {
        assert_eq!(
            runtime_profile_for_transport(RuntimeTransportKind::Stdio, false),
            RuntimeProfile::StdioEphemeral
        );
        assert_eq!(
            runtime_profile_for_transport(RuntimeTransportKind::Stdio, true),
            RuntimeProfile::StdioAttached
        );
        assert_eq!(
            runtime_profile_for_transport(RuntimeTransportKind::LoopbackHttp, true),
            RuntimeProfile::HttpLoopbackService
        );
        assert_eq!(
            runtime_profile_for_transport(RuntimeTransportKind::RemoteHttp, true),
            RuntimeProfile::HttpRemoteService
        );
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
            lexical_runtime: LexicalRuntimeConfig::default(),
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
        let mut config = FriggConfig {
            workspace_roots: vec![existing_workspace_root()],
            ..FriggConfig::default()
        };
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
