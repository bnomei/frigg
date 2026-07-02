//! Local embedding model artifact resolution and explicit preparation.

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use fastembed::{EmbeddingModel, ModelTrait, TextEmbedding, TextInitOptions};
use hf_hub::api::Progress;
use hf_hub::api::sync::ApiBuilder;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use thiserror::Error;

use crate::settings::SemanticRuntimeConfig;

/// Environment variable override for the local semantic model cache root.
pub const FRIGG_SEMANTIC_MODEL_CACHE_ENV: &str = "FRIGG_SEMANTIC_MODEL_CACHE";
/// Default local embedding model alias supported by the v1 local provider.
pub const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";
const HF_HOME_ENV: &str = "HF_HOME";
const HF_ENDPOINT_ENV: &str = "HF_ENDPOINT";
const DEFAULT_MODEL_REPOSITORY: &str = "Qdrant/all-MiniLM-L6-v2-onnx";
const DOWNLOAD_PROGRESS_LABEL_WIDTH: usize = 28;

/// Mapping between a semantic runtime model name and its fastembed artifact metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelAlias {
    pub semantic_model: &'static str,
    pub fastembed_model: EmbeddingModel,
    pub repository: &'static str,
    pub dimensions: usize,
}

/// Default local model alias used when semantic runtime omits an explicit model.
pub const DEFAULT_LOCAL_MODEL_ALIAS: LocalModelAlias = LocalModelAlias {
    semantic_model: DEFAULT_LOCAL_EMBEDDING_MODEL,
    fastembed_model: EmbeddingModel::AllMiniLML6V2,
    repository: DEFAULT_MODEL_REPOSITORY,
    dimensions: 384,
};

/// Host platform used when resolving the default local model cache root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePlatform {
    MacOs,
    Linux,
    Windows,
}

/// Environment snapshot used to resolve the local semantic model cache root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheResolutionEnv {
    pub frigg_semantic_model_cache: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
}

impl CacheResolutionEnv {
    pub fn from_process_env() -> Self {
        Self {
            frigg_semantic_model_cache: non_empty_env_path(FRIGG_SEMANTIC_MODEL_CACHE_ENV),
            home: non_empty_env_path("HOME"),
            xdg_cache_home: non_empty_env_path("XDG_CACHE_HOME"),
            local_app_data: non_empty_env_path("LOCALAPPDATA"),
        }
    }
}

/// Resolved on-disk artifact layout for a supported local embedding model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelArtifact {
    pub semantic_model: String,
    pub cache_root: PathBuf,
    pub cache_key: String,
    pub repository: String,
    pub repository_cache_dir: PathBuf,
    pub model_file: String,
    pub required_files: Vec<String>,
}

/// Whether a resolved local model artifact is ready for provider construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalModelArtifactStatus {
    Ready(LocalModelArtifact),
    Missing(LocalModelArtifact),
}

/// Failure modes for local model resolution, verification, and preparation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LocalModelError {
    #[error("local semantic model '{model}' is not supported; supported v1 model: {supported}")]
    Unsupported {
        model: String,
        supported: &'static str,
    },
    #[error("local semantic model cache root could not be resolved: {message}")]
    UnsupportedCacheRoot { message: String },
    #[error("local semantic model artifact is missing for model '{model}' in {cache_root}")]
    Missing { model: String, cache_root: PathBuf },
    #[error(
        "local semantic model artifact for model '{model}' requires download into {cache_root}; Frigg prepares local artifacts automatically when semantic runtime provider=local"
    )]
    DownloadRequired { model: String, cache_root: PathBuf },
    #[error(
        "local semantic model artifact for model '{model}' is corrupt in {cache_root}: {message}"
    )]
    Corrupt {
        model: String,
        cache_root: PathBuf,
        message: String,
    },
    #[error("failed to prepare local semantic model '{model}' in {cache_root}: {message}")]
    PreparationFailed {
        model: String,
        cache_root: PathBuf,
        message: String,
    },
}

/// Result alias for local model artifact resolution and preparation.
pub type LocalModelResult<T> = Result<T, LocalModelError>;

pub fn resolve_local_model_artifact(semantic_model: &str) -> LocalModelResult<LocalModelArtifact> {
    resolve_local_model_artifact_with_env(
        semantic_model,
        current_cache_platform(),
        &CacheResolutionEnv::from_process_env(),
    )
}

pub fn resolve_local_model_artifact_with_env(
    semantic_model: &str,
    platform: Option<CachePlatform>,
    env: &CacheResolutionEnv,
) -> LocalModelResult<LocalModelArtifact> {
    let alias = resolve_model_alias(semantic_model)?;
    let cache_root = resolve_cache_root_with_env(platform, env)?;
    let model_info = EmbeddingModel::get_model_info(&alias.fastembed_model).ok_or_else(|| {
        LocalModelError::Unsupported {
            model: semantic_model.trim().to_owned(),
            supported: DEFAULT_LOCAL_EMBEDDING_MODEL,
        }
    })?;
    let mut required_files = vec![
        model_info.model_file.clone(),
        "config.json".to_owned(),
        "special_tokens_map.json".to_owned(),
        "tokenizer.json".to_owned(),
        "tokenizer_config.json".to_owned(),
    ];
    required_files.extend(model_info.additional_files.iter().cloned());
    required_files.sort();
    required_files.dedup();
    let repository = model_info.model_code.clone();
    let repository_cache_dir = cache_root.join(hf_model_cache_folder(&repository));

    Ok(LocalModelArtifact {
        semantic_model: alias.semantic_model.to_owned(),
        cache_root,
        cache_key: artifact_cache_key(alias.semantic_model, &repository),
        repository,
        repository_cache_dir,
        model_file: model_info.model_file.clone(),
        required_files,
    })
}

pub fn resolve_cache_root() -> LocalModelResult<PathBuf> {
    resolve_cache_root_with_env(
        current_cache_platform(),
        &CacheResolutionEnv::from_process_env(),
    )
}

pub fn resolve_cache_root_with_env(
    platform: Option<CachePlatform>,
    env: &CacheResolutionEnv,
) -> LocalModelResult<PathBuf> {
    if let Some(path) = env.frigg_semantic_model_cache.as_ref() {
        return Ok(path.clone());
    }

    match platform {
        Some(CachePlatform::MacOs) => env
            .home
            .as_ref()
            .map(|home| {
                home.join("Library")
                    .join("Caches")
                    .join("frigg")
                    .join("models")
            })
            .ok_or_else(|| LocalModelError::UnsupportedCacheRoot {
                message: "HOME is not set for macOS cache resolution".to_owned(),
            }),
        Some(CachePlatform::Linux) => {
            if let Some(cache_home) = env.xdg_cache_home.as_ref() {
                Ok(cache_home.join("frigg").join("models"))
            } else {
                env.home
                    .as_ref()
                    .map(|home| home.join(".cache").join("frigg").join("models"))
                    .ok_or_else(|| LocalModelError::UnsupportedCacheRoot {
                        message:
                            "neither XDG_CACHE_HOME nor HOME is set for Linux cache resolution"
                                .to_owned(),
                    })
            }
        }
        Some(CachePlatform::Windows) => env
            .local_app_data
            .as_ref()
            .map(|local_app_data| local_app_data.join("frigg").join("models"))
            .ok_or_else(|| LocalModelError::UnsupportedCacheRoot {
                message: "LOCALAPPDATA is not set for Windows cache resolution".to_owned(),
            }),
        None => Err(LocalModelError::UnsupportedCacheRoot {
            message: "local semantic model cache is unsupported on this platform".to_owned(),
        }),
    }
}

pub fn check_local_model_artifact(
    semantic_model: &str,
) -> LocalModelResult<LocalModelArtifactStatus> {
    let artifact = resolve_local_model_artifact(semantic_model)?;
    check_resolved_local_model_artifact(artifact)
}

pub fn check_resolved_local_model_artifact(
    artifact: LocalModelArtifact,
) -> LocalModelResult<LocalModelArtifactStatus> {
    if !artifact.repository_cache_dir.exists() {
        return Ok(LocalModelArtifactStatus::Missing(artifact));
    }
    if !artifact.repository_cache_dir.is_dir() {
        return Err(LocalModelError::Corrupt {
            model: artifact.semantic_model,
            cache_root: artifact.cache_root,
            message: format!(
                "expected repository cache directory at {}",
                artifact.repository_cache_dir.display()
            ),
        });
    }

    let snapshots_dir = artifact.repository_cache_dir.join("snapshots");
    if !snapshots_dir.is_dir() {
        return Err(LocalModelError::Corrupt {
            model: artifact.semantic_model,
            cache_root: artifact.cache_root,
            message: format!("missing snapshots directory at {}", snapshots_dir.display()),
        });
    }

    let has_complete_snapshot = fs::read_dir(&snapshots_dir)
        .map_err(|err| LocalModelError::Corrupt {
            model: artifact.semantic_model.clone(),
            cache_root: artifact.cache_root.clone(),
            message: format!("failed to read {}: {err}", snapshots_dir.display()),
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .any(|snapshot| {
            artifact
                .required_files
                .iter()
                .all(|relative| snapshot.join(relative).is_file())
        });

    if !has_complete_snapshot {
        return Err(LocalModelError::Corrupt {
            model: artifact.semantic_model,
            cache_root: artifact.cache_root,
            message: "no complete Hugging Face snapshot contains all required model files"
                .to_owned(),
        });
    }

    Ok(LocalModelArtifactStatus::Ready(artifact))
}

pub fn require_prepared_local_model(semantic_model: &str) -> LocalModelResult<LocalModelArtifact> {
    match check_local_model_artifact(semantic_model)? {
        LocalModelArtifactStatus::Ready(artifact) => Ok(artifact),
        LocalModelArtifactStatus::Missing(artifact) => Err(LocalModelError::Missing {
            model: artifact.semantic_model,
            cache_root: artifact.cache_root,
        }),
    }
}

pub fn prepare_local_semantic_model(
    semantic_runtime: &SemanticRuntimeConfig,
) -> LocalModelResult<LocalModelArtifact> {
    let provider = semantic_runtime
        .provider
        .ok_or_else(|| LocalModelError::PreparationFailed {
            model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
            cache_root: resolve_cache_root().unwrap_or_else(|_| PathBuf::from("<unresolved>")),
            message: "semantic runtime provider must be local before preparing local artifacts"
                .to_owned(),
        })?;
    if provider != crate::settings::SemanticRuntimeProvider::Local {
        return Err(LocalModelError::PreparationFailed {
            model: semantic_runtime
                .normalized_model()
                .unwrap_or(provider.default_model())
                .to_owned(),
            cache_root: resolve_cache_root().unwrap_or_else(|_| PathBuf::from("<unresolved>")),
            message: format!(
                "semantic runtime provider '{}' uses external embeddings; set --semantic-runtime-provider local to prepare local artifacts",
                provider.as_str()
            ),
        });
    }
    if !semantic_runtime.enabled {
        return Err(LocalModelError::PreparationFailed {
            model: semantic_runtime
                .normalized_model()
                .unwrap_or(DEFAULT_LOCAL_EMBEDDING_MODEL)
                .to_owned(),
            cache_root: resolve_cache_root().unwrap_or_else(|_| PathBuf::from("<unresolved>")),
            message: "semantic runtime must be enabled before preparing local artifacts".to_owned(),
        });
    }

    let model = semantic_runtime
        .normalized_model()
        .unwrap_or(DEFAULT_LOCAL_EMBEDDING_MODEL);
    let artifact = resolve_local_model_artifact(model)?;
    if matches!(
        check_resolved_local_model_artifact(artifact.clone()),
        Ok(LocalModelArtifactStatus::Ready(_))
    ) {
        return Ok(artifact);
    }
    if std::env::var_os(HF_HOME_ENV).is_some() {
        return Err(LocalModelError::PreparationFailed {
            model: artifact.semantic_model,
            cache_root: artifact.cache_root,
            message: format!(
                "{HF_HOME_ENV} is set; unset it so {FRIGG_SEMANTIC_MODEL_CACHE_ENV} or Frigg's platform cache root controls model placement"
            ),
        });
    }

    prefetch_local_model_files(&artifact)?;

    let options = TextInitOptions::new(DEFAULT_LOCAL_MODEL_ALIAS.fastembed_model)
        .with_cache_dir(artifact.cache_root.clone())
        .with_show_download_progress(false);
    TextEmbedding::try_new(options).map_err(|err| LocalModelError::PreparationFailed {
        model: artifact.semantic_model.clone(),
        cache_root: artifact.cache_root.clone(),
        message: err.to_string(),
    })?;

    match check_resolved_local_model_artifact(artifact.clone())? {
        LocalModelArtifactStatus::Ready(prepared) => Ok(prepared),
        LocalModelArtifactStatus::Missing(missing) => Err(LocalModelError::DownloadRequired {
            model: missing.semantic_model,
            cache_root: missing.cache_root,
        }),
    }
}

fn prefetch_local_model_files(artifact: &LocalModelArtifact) -> LocalModelResult<()> {
    let mut builder = ApiBuilder::new()
        .with_cache_dir(artifact.cache_root.clone())
        .with_progress(false);
    if let Ok(endpoint) = std::env::var(HF_ENDPOINT_ENV) {
        builder = builder.with_endpoint(endpoint);
    }
    let repo = builder
        .build()
        .map_err(|err| LocalModelError::PreparationFailed {
            model: artifact.semantic_model.clone(),
            cache_root: artifact.cache_root.clone(),
            message: err.to_string(),
        })?
        .model(artifact.repository.clone());

    for file in local_model_download_files(artifact) {
        repo.download_with_progress(file, StableDownloadProgress::new())
            .map_err(|err| LocalModelError::PreparationFailed {
                model: artifact.semantic_model.clone(),
                cache_root: artifact.cache_root.clone(),
                message: format!("failed to download {file}: {err}"),
            })?;
    }

    Ok(())
}

fn local_model_download_files(artifact: &LocalModelArtifact) -> Vec<&str> {
    let mut files = Vec::with_capacity(artifact.required_files.len());
    files.push(artifact.model_file.as_str());
    files.extend(
        artifact
            .required_files
            .iter()
            .map(String::as_str)
            .filter(|file| *file != artifact.model_file),
    );
    files
}

struct StableDownloadProgress {
    progress: ProgressBar,
}

impl StableDownloadProgress {
    fn new() -> Self {
        Self {
            progress: ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr_with_hz(12)),
        }
    }
}

impl Progress for StableDownloadProgress {
    fn init(&mut self, size: usize, filename: &str) {
        self.progress.set_length(size as u64);
        self.progress.set_style(stable_download_progress_style());
        self.progress.set_message(download_progress_label(filename));
    }

    fn update(&mut self, size: usize) {
        self.progress.inc(size as u64);
    }

    fn finish(&mut self) {
        self.progress.finish();
    }
}

fn stable_download_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{msg:28} {bytes:>12}/{total_bytes:<12} {bytes_per_sec:>13} eta={eta:<8} {wide_bar:.cyan/blue}",
    )
    .expect("download progress template should be valid")
}

fn download_progress_label(filename: &str) -> String {
    const ELLIPSIS: &str = "..";
    let char_count = filename.chars().count();
    if char_count <= DOWNLOAD_PROGRESS_LABEL_WIDTH {
        return filename.to_owned();
    }

    let suffix_len = DOWNLOAD_PROGRESS_LABEL_WIDTH.saturating_sub(ELLIPSIS.len());
    let suffix = filename
        .chars()
        .rev()
        .take(suffix_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{ELLIPSIS}{suffix}")
}

pub fn resolve_model_alias(semantic_model: &str) -> LocalModelResult<&'static LocalModelAlias> {
    let normalized = semantic_model.trim();
    if normalized.eq_ignore_ascii_case(DEFAULT_LOCAL_EMBEDDING_MODEL)
        || normalized.eq_ignore_ascii_case("AllMiniLML6V2")
        || normalized.eq_ignore_ascii_case("sentence-transformers/all-MiniLM-L6-v2")
        || normalized.eq_ignore_ascii_case(DEFAULT_MODEL_REPOSITORY)
    {
        Ok(&DEFAULT_LOCAL_MODEL_ALIAS)
    } else {
        Err(LocalModelError::Unsupported {
            model: normalized.to_owned(),
            supported: DEFAULT_LOCAL_EMBEDDING_MODEL,
        })
    }
}

fn artifact_cache_key(semantic_model: &str, repository: &str) -> String {
    format!(
        "local--{}--{}",
        cache_key_component(semantic_model),
        cache_key_component(repository)
    )
}

fn cache_key_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn hf_model_cache_folder(repository: &str) -> String {
    format!("models--{}", repository.replace('/', "--"))
}

fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).and_then(non_empty_os_path)
}

fn non_empty_os_path(value: OsString) -> Option<PathBuf> {
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn current_cache_platform() -> Option<CachePlatform> {
    if cfg!(target_os = "macos") {
        Some(CachePlatform::MacOs)
    } else if cfg!(target_os = "linux") {
        Some(CachePlatform::Linux)
    } else if cfg!(target_os = "windows") {
        Some(CachePlatform::Windows)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_model_env_override_has_highest_priority() {
        let env = CacheResolutionEnv {
            frigg_semantic_model_cache: Some(PathBuf::from("/tmp/frigg-models")),
            home: Some(PathBuf::from("/home/tester")),
            xdg_cache_home: Some(PathBuf::from("/xdg/cache")),
            local_app_data: Some(PathBuf::from(r"C:\Users\tester\AppData\Local")),
        };

        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::Linux), &env).unwrap(),
            PathBuf::from("/tmp/frigg-models")
        );
        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::MacOs), &env).unwrap(),
            PathBuf::from("/tmp/frigg-models")
        );
        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::Windows), &env).unwrap(),
            PathBuf::from("/tmp/frigg-models")
        );
    }

    #[test]
    fn local_model_platform_fallback_paths_are_deterministic() {
        let env = CacheResolutionEnv {
            frigg_semantic_model_cache: None,
            home: Some(PathBuf::from("/home/tester")),
            xdg_cache_home: Some(PathBuf::from("/xdg/cache")),
            local_app_data: Some(PathBuf::from(r"C:\Users\tester\AppData\Local")),
        };

        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::MacOs), &env).unwrap(),
            PathBuf::from("/home/tester/Library/Caches/frigg/models")
        );
        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::Linux), &env).unwrap(),
            PathBuf::from("/xdg/cache/frigg/models")
        );
        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::Windows), &env).unwrap(),
            PathBuf::from(r"C:\Users\tester\AppData\Local")
                .join("frigg")
                .join("models")
        );

        let linux_home_env = CacheResolutionEnv {
            xdg_cache_home: None,
            ..env
        };
        assert_eq!(
            resolve_cache_root_with_env(Some(CachePlatform::Linux), &linux_home_env).unwrap(),
            PathBuf::from("/home/tester/.cache/frigg/models")
        );
    }

    #[test]
    fn local_model_artifact_identity_uses_stable_cache_key_and_repo_path() {
        let env = CacheResolutionEnv {
            frigg_semantic_model_cache: Some(PathBuf::from("/tmp/frigg-models")),
            ..CacheResolutionEnv::default()
        };
        let artifact = resolve_local_model_artifact_with_env(
            DEFAULT_LOCAL_EMBEDDING_MODEL,
            Some(CachePlatform::Linux),
            &env,
        )
        .unwrap();

        assert_eq!(
            artifact.cache_key,
            "local--all-minilm-l6-v2--qdrant-all-minilm-l6-v2-onnx"
        );
        assert_eq!(artifact.repository, DEFAULT_MODEL_REPOSITORY);
        assert_eq!(
            artifact.repository_cache_dir,
            PathBuf::from("/tmp/frigg-models/models--Qdrant--all-MiniLM-L6-v2-onnx")
        );
        assert!(
            artifact
                .required_files
                .iter()
                .any(|file| file == "model.onnx")
        );
        assert!(
            artifact
                .required_files
                .iter()
                .any(|file| file == "tokenizer.json")
        );
    }

    #[test]
    fn local_model_download_progress_label_keeps_fixed_width_tail() {
        let short = download_progress_label("model.onnx");
        assert_eq!(short, "model.onnx");

        let long = download_progress_label("nested/path/with/a/very-long-tokenizer-config.json");
        assert_eq!(long.chars().count(), DOWNLOAD_PROGRESS_LABEL_WIDTH);
        assert!(long.starts_with(".."));
        assert!(long.ends_with("tokenizer-config.json"));
    }

    #[test]
    fn local_model_download_progress_template_is_valid() {
        let _style = stable_download_progress_style();
    }

    #[test]
    fn local_model_download_files_start_with_model_file() {
        let artifact = LocalModelArtifact {
            semantic_model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
            cache_root: PathBuf::from("/tmp/frigg-models"),
            cache_key: "local--test".to_owned(),
            repository: DEFAULT_MODEL_REPOSITORY.to_owned(),
            repository_cache_dir: PathBuf::from("/tmp/frigg-models/repo"),
            model_file: "model.onnx".to_owned(),
            required_files: vec![
                "config.json".to_owned(),
                "model.onnx".to_owned(),
                "tokenizer.json".to_owned(),
            ],
        };

        assert_eq!(
            local_model_download_files(&artifact),
            vec!["model.onnx", "config.json", "tokenizer.json"]
        );
    }

    #[test]
    fn local_model_missing_artifact_status_is_typed() {
        let root = std::env::temp_dir().join(format!(
            "frigg-local-model-missing-{}",
            uuid::Uuid::now_v7()
        ));
        let artifact = LocalModelArtifact {
            semantic_model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
            cache_root: root.clone(),
            cache_key: "local--missing".to_owned(),
            repository: DEFAULT_MODEL_REPOSITORY.to_owned(),
            repository_cache_dir: root.join("models--Qdrant--all-MiniLM-L6-v2-onnx"),
            model_file: "model.onnx".to_owned(),
            required_files: vec!["model.onnx".to_owned()],
        };

        assert!(matches!(
            check_resolved_local_model_artifact(artifact),
            Ok(LocalModelArtifactStatus::Missing(_))
        ));
    }

    #[test]
    fn local_model_corrupt_artifact_error_is_typed() {
        let root = std::env::temp_dir().join(format!(
            "frigg-local-model-corrupt-{}",
            uuid::Uuid::now_v7()
        ));
        let repo = root.join("models--Qdrant--all-MiniLM-L6-v2-onnx");
        fs::create_dir_all(&repo).unwrap();
        let artifact = LocalModelArtifact {
            semantic_model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
            cache_root: root.clone(),
            cache_key: "local--corrupt".to_owned(),
            repository: DEFAULT_MODEL_REPOSITORY.to_owned(),
            repository_cache_dir: repo,
            model_file: "model.onnx".to_owned(),
            required_files: vec!["model.onnx".to_owned()],
        };

        let error = check_resolved_local_model_artifact(artifact).unwrap_err();
        assert!(matches!(error, LocalModelError::Corrupt { .. }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_model_download_required_error_is_typed() {
        let error = LocalModelError::DownloadRequired {
            model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
            cache_root: PathBuf::from("/tmp/frigg-models"),
        };

        assert!(
            error
                .to_string()
                .contains("prepares local artifacts automatically")
        );
    }
}
