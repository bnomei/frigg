//! Local embedding model artifact resolution and explicit preparation.
//!
//! Resolves Hugging Face model aliases, downloads artifacts into the Frigg cache, and surfaces
//! preparation errors before fastembed-backed providers are constructed at runtime.

#[cfg(feature = "local-embeddings")]
mod enabled {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use fastembed::{EmbeddingModel, ModelTrait, TextEmbedding, TextInitOptions};
    use hf_hub::api::Progress;
    use hf_hub::api::sync::ApiBuilder;
    use indicatif::{HumanBytes, ProgressBar, ProgressDrawTarget, ProgressStyle};
    use thiserror::Error;

    use crate::human_output::{HumanBlock, HumanRow};
    use crate::settings::SemanticRuntimeConfig;

    /// Environment variable override for the local semantic model cache root.
    pub const FRIGG_SEMANTIC_MODEL_CACHE_ENV: &str = "FRIGG_SEMANTIC_MODEL_CACHE";
    /// Default local embedding model alias supported by the v1 local provider.
    pub const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";
    const HF_HOME_ENV: &str = "HF_HOME";
    const HF_ENDPOINT_ENV: &str = "HF_ENDPOINT";
    const DEFAULT_MODEL_REPOSITORY: &str = "Qdrant/all-MiniLM-L6-v2-onnx";
    const DOWNLOAD_PROGRESS_LABEL_WIDTH: usize = 28;
    const DOWNLOAD_PROGRESS_WIDTH: usize = 80;
    const DOWNLOAD_PROGRESS_SERVER_BADGE: &str = "SRV";
    const DOWNLOAD_PROGRESS_SEMANTIC_COLOR_CODE: &str = "1;38;2;90;205;210";
    #[cfg(test)]
    const DOWNLOAD_PROGRESS_SEMANTIC_COLOR: &str = "\x1b[1;38;2;90;205;210m";
    const DOWNLOAD_PROGRESS_TEMPLATE: &str = "{msg}";
    const DOWNLOAD_PROGRESS_TITLE: &str = "Loading semantic model";

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
        /// Reads cache-related environment variables used for platform cache-root resolution.
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

    /// Resolves on-disk layout for a semantic model using the process environment and host OS.
    pub fn resolve_local_model_artifact(
        semantic_model: &str,
    ) -> LocalModelResult<LocalModelArtifact> {
        resolve_local_model_artifact_with_env(
            semantic_model,
            current_cache_platform(),
            &CacheResolutionEnv::from_process_env(),
        )
    }

    /// Resolves on-disk layout with injectable platform/env for tests and offline tooling.
    pub fn resolve_local_model_artifact_with_env(
        semantic_model: &str,
        platform: Option<CachePlatform>,
        env: &CacheResolutionEnv,
    ) -> LocalModelResult<LocalModelArtifact> {
        let alias = resolve_model_alias(semantic_model)?;
        let cache_root = resolve_cache_root_with_env(platform, env)?;
        let model_info =
            EmbeddingModel::get_model_info(&alias.fastembed_model).ok_or_else(|| {
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

    /// Resolves the local semantic model cache root from process env and host platform.
    pub fn resolve_cache_root() -> LocalModelResult<PathBuf> {
        resolve_cache_root_with_env(
            current_cache_platform(),
            &CacheResolutionEnv::from_process_env(),
        )
    }

    /// Resolves cache root with injectable platform/env; `FRIGG_SEMANTIC_MODEL_CACHE` wins if set.
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

    /// Resolves then verifies whether prepared artifacts exist for a semantic model name.
    pub fn check_local_model_artifact(
        semantic_model: &str,
    ) -> LocalModelResult<LocalModelArtifactStatus> {
        let artifact = resolve_local_model_artifact(semantic_model)?;
        check_resolved_local_model_artifact(artifact)
    }

    /// Verifies snapshot completeness for a resolved artifact; corrupt layouts become errors.
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

    /// Requires a ready local artifact; missing artifacts fail without attempting download.
    pub fn require_prepared_local_model(
        semantic_model: &str,
    ) -> LocalModelResult<LocalModelArtifact> {
        match check_local_model_artifact(semantic_model)? {
            LocalModelArtifactStatus::Ready(artifact) => Ok(artifact),
            LocalModelArtifactStatus::Missing(artifact) => Err(LocalModelError::Missing {
                model: artifact.semantic_model,
                cache_root: artifact.cache_root,
            }),
        }
    }

    /// Downloads and validates the local model when semantic runtime is local and enabled.
    ///
    /// Rejects `HF_HOME` overrides so Frigg cache roots control placement. No-ops when already ready.
    pub fn prepare_local_semantic_model(
        semantic_runtime: &SemanticRuntimeConfig,
    ) -> LocalModelResult<LocalModelArtifact> {
        let provider =
            semantic_runtime
                .provider
                .ok_or_else(|| LocalModelError::PreparationFailed {
                    model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
                    cache_root: resolve_cache_root()
                        .unwrap_or_else(|_| PathBuf::from("<unresolved>")),
                    message:
                        "semantic runtime provider must be local before preparing local artifacts"
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
                message: "semantic runtime must be enabled before preparing local artifacts"
                    .to_owned(),
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

        let files = local_model_download_files(artifact);
        let progress = StableDownloadProgressGroup::new(files.len());
        for file in files {
            repo.download_with_progress(file, progress.next_file_progress())
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

    #[derive(Clone)]
    struct StableDownloadProgressGroup {
        progress: ProgressBar,
        state: Arc<Mutex<StableDownloadProgressState>>,
        color: bool,
    }

    struct StableDownloadProgressState {
        rows: Vec<StableDownloadProgressRow>,
        expected_rows: usize,
    }

    struct StableDownloadProgressRow {
        label: String,
        loaded_bytes: usize,
        total_bytes: usize,
        finished: bool,
    }

    struct StableDownloadProgress {
        group: StableDownloadProgressGroup,
        row_index: Option<usize>,
    }

    impl StableDownloadProgressGroup {
        fn new(expected_rows: usize) -> Self {
            Self::with_color(expected_rows, std::env::var_os("NO_COLOR").is_none())
        }

        fn with_color(expected_rows: usize, color: bool) -> Self {
            let progress =
                ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr_with_hz(12));
            progress.set_style(stable_download_progress_style());
            Self {
                progress,
                state: Arc::new(Mutex::new(StableDownloadProgressState {
                    rows: Vec::with_capacity(expected_rows),
                    expected_rows,
                })),
                color,
            }
        }

        fn next_file_progress(&self) -> StableDownloadProgress {
            StableDownloadProgress {
                group: self.clone(),
                row_index: None,
            }
        }

        fn init_row(&self, row_index: Option<usize>, size: usize, filename: &str) -> usize {
            let mut state = self
                .state
                .lock()
                .expect("download progress state lock should not be poisoned");
            let row_index = match row_index {
                Some(row_index) if row_index < state.rows.len() => {
                    state.rows[row_index] = StableDownloadProgressRow {
                        label: download_progress_label(filename),
                        loaded_bytes: 0,
                        total_bytes: size,
                        finished: false,
                    };
                    row_index
                }
                _ => {
                    let row_index = state.rows.len();
                    state.rows.push(StableDownloadProgressRow {
                        label: download_progress_label(filename),
                        loaded_bytes: 0,
                        total_bytes: size,
                        finished: false,
                    });
                    row_index
                }
            };
            self.progress
                .set_message(render_download_progress_card(&state.rows, self.color));
            row_index
        }

        fn update_row(&self, row_index: usize, size: usize) {
            let mut state = self
                .state
                .lock()
                .expect("download progress state lock should not be poisoned");
            if let Some(row) = state.rows.get_mut(row_index) {
                row.loaded_bytes = row.loaded_bytes.saturating_add(size).min(row.total_bytes);
            }
            self.progress
                .set_message(render_download_progress_card(&state.rows, self.color));
        }

        fn finish_row(&self, row_index: usize) {
            let mut state = self
                .state
                .lock()
                .expect("download progress state lock should not be poisoned");
            if let Some(row) = state.rows.get_mut(row_index) {
                row.loaded_bytes = row.total_bytes;
                row.finished = true;
            }
            let card = render_download_progress_card(&state.rows, self.color);
            if state.rows.len() >= state.expected_rows && state.rows.iter().all(|row| row.finished)
            {
                self.progress.finish_with_message(card);
            } else {
                self.progress.set_message(card);
            }
        }
    }

    #[cfg(test)]
    impl StableDownloadProgressGroup {
        fn rendered_card_for_test(&self) -> String {
            let state = self
                .state
                .lock()
                .expect("download progress state lock should not be poisoned");
            render_download_progress_card(&state.rows, self.color)
        }

        fn row_count_for_test(&self) -> usize {
            self.state
                .lock()
                .expect("download progress state lock should not be poisoned")
                .rows
                .len()
        }

        fn all_rows_finished_for_test(&self) -> bool {
            self.state
                .lock()
                .expect("download progress state lock should not be poisoned")
                .rows
                .iter()
                .all(|row| row.finished)
        }
    }

    impl Progress for StableDownloadProgress {
        fn init(&mut self, size: usize, filename: &str) {
            self.row_index = Some(self.group.init_row(self.row_index, size, filename));
        }

        fn update(&mut self, size: usize) {
            if let Some(row_index) = self.row_index {
                self.group.update_row(row_index, size);
            }
        }

        fn finish(&mut self) {
            if let Some(row_index) = self.row_index {
                self.group.finish_row(row_index);
            }
        }
    }

    fn stable_download_progress_style() -> ProgressStyle {
        ProgressStyle::with_template(DOWNLOAD_PROGRESS_TEMPLATE)
            .expect("download progress template should be valid")
    }

    fn render_download_progress_card(rows: &[StableDownloadProgressRow], color: bool) -> String {
        let rows = rows
            .iter()
            .map(|row| {
                HumanRow::kv(
                    row.label.clone(),
                    download_progress_value(row.loaded_bytes, row.total_bytes),
                )
            })
            .collect::<Vec<_>>();
        HumanBlock::new(
            DOWNLOAD_PROGRESS_TITLE,
            rows,
            "○",
            DOWNLOAD_PROGRESS_SEMANTIC_COLOR_CODE,
            DOWNLOAD_PROGRESS_SEMANTIC_COLOR_CODE,
        )
        .with_badge_column(DOWNLOAD_PROGRESS_SERVER_BADGE)
        .render_with_min_label_width(
            color,
            DOWNLOAD_PROGRESS_WIDTH,
            Some(DOWNLOAD_PROGRESS_LABEL_WIDTH),
        )
    }

    fn download_progress_value(loaded_bytes: usize, total_bytes: usize) -> String {
        format!(
            "{}/{}",
            HumanBytes(loaded_bytes as u64),
            HumanBytes(total_bytes as u64)
        )
    }

    #[cfg(test)]
    fn stable_download_progress_preview(rows: &[(&str, usize, usize)], color: bool) -> String {
        let rows = rows
            .iter()
            .map(
                |(label, loaded_bytes, total_bytes)| StableDownloadProgressRow {
                    label: download_progress_label(label),
                    loaded_bytes: *loaded_bytes,
                    total_bytes: *total_bytes,
                    finished: loaded_bytes == total_bytes,
                },
            )
            .collect::<Vec<_>>();
        render_download_progress_card(&rows, color)
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

    /// Maps supported semantic/fastembed/repository aliases to the v1 local model metadata.
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
                resolve_cache_root_with_env(Some(CachePlatform::Linux), &env)
                    .expect("linux cache root should resolve"),
                PathBuf::from("/tmp/frigg-models")
            );
            assert_eq!(
                resolve_cache_root_with_env(Some(CachePlatform::MacOs), &env)
                    .expect("macOS cache root should resolve"),
                PathBuf::from("/tmp/frigg-models")
            );
            assert_eq!(
                resolve_cache_root_with_env(Some(CachePlatform::Windows), &env)
                    .expect("windows cache root should resolve"),
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
                resolve_cache_root_with_env(Some(CachePlatform::MacOs), &env)
                    .expect("macOS cache root should resolve"),
                PathBuf::from("/home/tester/Library/Caches/frigg/models")
            );
            assert_eq!(
                resolve_cache_root_with_env(Some(CachePlatform::Linux), &env)
                    .expect("linux cache root should resolve"),
                PathBuf::from("/xdg/cache/frigg/models")
            );
            assert_eq!(
                resolve_cache_root_with_env(Some(CachePlatform::Windows), &env)
                    .expect("windows cache root should resolve"),
                PathBuf::from(r"C:\Users\tester\AppData\Local")
                    .join("frigg")
                    .join("models")
            );

            let linux_home_env = CacheResolutionEnv {
                xdg_cache_home: None,
                ..env
            };
            assert_eq!(
                resolve_cache_root_with_env(Some(CachePlatform::Linux), &linux_home_env)
                    .expect("linux home fallback cache root should resolve"),
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
            .expect("local model artifact should resolve");

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

            let long =
                download_progress_label("nested/path/with/a/very-long-tokenizer-config.json");
            assert_eq!(long.chars().count(), DOWNLOAD_PROGRESS_LABEL_WIDTH);
            assert!(long.starts_with(".."));
            assert!(long.ends_with("tokenizer-config.json"));
        }

        #[test]
        fn local_model_download_progress_template_is_compact_card() {
            let _style = stable_download_progress_style();

            assert_eq!(DOWNLOAD_PROGRESS_TEMPLATE, "{msg}");
            assert!(!DOWNLOAD_PROGRESS_TEMPLATE.contains("bytes_per_sec"));
            assert!(!DOWNLOAD_PROGRESS_TEMPLATE.contains("eta"));
            assert!(!DOWNLOAD_PROGRESS_TEMPLATE.contains("wide_bar"));
        }

        #[test]
        fn local_model_download_progress_preview_groups_file_rows() {
            let preview = stable_download_progress_preview(
                &[
                    ("model.onnx", 90_387_251, 90_387_251),
                    ("config.json", 650, 650),
                    ("tokenizer.json", 711_659, 711_659),
                ],
                false,
            );
            let colored =
                stable_download_progress_preview(&[("model.onnx", 20_478_689, 90_387_251)], true);

            assert_eq!(
                preview,
                "SRV ╭─○ Loading semantic model\n    │   model.onnx                   86.20 MiB/86.20 MiB\n    │   config.json                  650 B/650 B\n    ╰─╮ tokenizer.json               694.98 KiB/694.98 KiB"
            );
            assert!(colored.starts_with("\x1b[1;97;48;5;240mSRV\x1b[0m "));
            assert!(colored.contains(DOWNLOAD_PROGRESS_SEMANTIC_COLOR));
            assert!(colored.contains("model.onnx"));
            assert!(colored.contains("19.53 MiB/86.20 MiB"));
            assert!(!preview.contains("MiB/s"));
            assert!(!preview.contains("eta"));
            assert!(!preview.contains('['));
        }

        #[test]
        fn local_model_download_progress_reuses_row_for_repeated_init() {
            let group = StableDownloadProgressGroup::with_color(2, false);
            let mut first = group.next_file_progress();
            let mut second = group.next_file_progress();

            first.init(650, "config.json");
            first.init(650, "config.json");
            first.update(325);

            assert_eq!(group.row_count_for_test(), 1);
            assert_eq!(
                group.rendered_card_for_test(),
                "SRV ╭─○ Loading semantic model\n    ╰─╮ config.json                  325 B/650 B"
            );

            first.finish();
            second.init(711_659, "tokenizer.json");
            second.init(711_659, "tokenizer.json");
            second.update(1024);
            second.update(710_635);
            second.finish();

            assert_eq!(group.row_count_for_test(), 2);
            assert!(group.all_rows_finished_for_test());
            assert_eq!(
                group.rendered_card_for_test(),
                "SRV ╭─○ Loading semantic model\n    │   config.json                  650 B/650 B\n    ╰─╮ tokenizer.json               694.98 KiB/694.98 KiB"
            );
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
            fs::create_dir_all(&repo).expect("corrupt model repository should be created");
            let artifact = LocalModelArtifact {
                semantic_model: DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned(),
                cache_root: root.clone(),
                cache_key: "local--corrupt".to_owned(),
                repository: DEFAULT_MODEL_REPOSITORY.to_owned(),
                repository_cache_dir: repo,
                model_file: "model.onnx".to_owned(),
                required_files: vec!["model.onnx".to_owned()],
            };

            let error = check_resolved_local_model_artifact(artifact)
                .expect_err("corrupt local model artifact should fail validation");
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
}

#[cfg(feature = "local-embeddings")]
pub use enabled::*;

#[cfg(not(feature = "local-embeddings"))]
mod disabled {
    use std::path::PathBuf;

    use thiserror::Error;

    use crate::settings::SemanticRuntimeConfig;

    /// Environment variable override for the local semantic model cache root.
    pub const FRIGG_SEMANTIC_MODEL_CACHE_ENV: &str = "FRIGG_SEMANTIC_MODEL_CACHE";
    /// Default local embedding model alias supported by the v1 local provider.
    pub const DEFAULT_LOCAL_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";

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
        #[error(
            "local semantic provider is not available in this build; rebuild Frigg with the `local-embeddings` feature"
        )]
        Unavailable,
    }

    /// Result alias for local model artifact resolution and preparation.
    pub type LocalModelResult<T> = Result<T, LocalModelError>;

    /// Stub when `local-embeddings` is disabled; always returns [`LocalModelError::Unavailable`].
    pub fn resolve_local_model_artifact(
        _semantic_model: &str,
    ) -> LocalModelResult<LocalModelArtifact> {
        Err(LocalModelError::Unavailable)
    }

    /// Stub when `local-embeddings` is disabled; always returns [`LocalModelError::Unavailable`].
    pub fn check_local_model_artifact(
        _semantic_model: &str,
    ) -> LocalModelResult<LocalModelArtifactStatus> {
        Err(LocalModelError::Unavailable)
    }

    /// Stub when `local-embeddings` is disabled; always returns [`LocalModelError::Unavailable`].
    pub fn prepare_local_semantic_model(
        _semantic_runtime: &SemanticRuntimeConfig,
    ) -> LocalModelResult<LocalModelArtifact> {
        Err(LocalModelError::Unavailable)
    }
}

#[cfg(not(feature = "local-embeddings"))]
pub use disabled::*;
