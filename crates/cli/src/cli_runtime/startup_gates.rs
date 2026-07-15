//! Startup gates that block serve and semantic index until vector storage and embedding
//! credentials satisfy the resolved runtime contract.
//!
//! Fails fast before serve or semantic index when vector storage, sqlite-vec, or embedding
//! credentials violate the resolved contract.

use std::io;

use frigg::embeddings::local_model::{
    FRIGG_SEMANTIC_MODEL_CACHE_ENV, LocalModelArtifact, LocalModelArtifactStatus,
    check_local_model_artifact, prepare_local_semantic_model,
};
use frigg::settings::{
    FriggConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider, SemanticRuntimeStartupError,
};
use frigg::storage::{DEFAULT_VECTOR_DIMENSIONS, Storage, VectorStoreBackend};
use tracing::info;

use crate::cli_runtime::storage_paths::resolve_storage_db_path;
use crate::cli_runtime::{
    CliOutput, OutputField, OutputLevel, field, format_output_event_line, reported_io_error,
};

const HF_HOME_ENV: &str = "HF_HOME";

#[derive(Debug)]
pub(super) enum SemanticStartupGateError {
    InvalidConfig(SemanticRuntimeStartupError),
    LocalModelPrepare(String),
}

impl SemanticStartupGateError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig(err) => err.code(),
            Self::LocalModelPrepare(_) => "local_model_prepare_failed",
        }
    }
}

impl std::fmt::Display for SemanticStartupGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(err) => write!(f, "{err}"),
            Self::LocalModelPrepare(message) => write!(f, "{message}"),
        }
    }
}

#[cfg(test)]
pub(crate) fn run_strict_startup_vector_readiness_gate(config: &FriggConfig) -> io::Result<()> {
    run_strict_startup_vector_readiness_gate_with_output(config, &CliOutput::normal())
}

/// Blocks serve and strict index paths until sqlite-vec storage is initialized and schema-compatible.
pub(crate) fn run_strict_startup_vector_readiness_gate_with_output(
    config: &FriggConfig,
    output: &CliOutput,
) -> io::Result<()> {
    let repositories = config.repositories();

    for repo in &repositories {
        let root = match config.root_by_repository_id(&repo.repository_id.0) {
            Some(root) => root,
            None => {
                let message = format_output_event_line(
                    OutputLevel::Error,
                    "startup",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                );
                output.error_event(
                    "startup",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                )?;
                return Err(reported_io_error(message));
            }
        };
        let db_path = resolve_storage_db_path(root, "startup")?;
        if !db_path.is_file() {
            let err_message = format!(
                "startup strict vector readiness failed repository_id={} root={} db={}: storage db file is missing; run `frigg init` from {} or `frigg init --workspace-root {}` first",
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                root.display(),
                root.display()
            );
            output.error_event(
                "startup",
                "failed",
                &[
                    field("status", "failed"),
                    field("repos", repositories.len()),
                    field("repo", &repo.repository_id.0),
                    field("db", db_path.display()),
                    field("next", "run `frigg init` or `frigg index`"),
                    field("error", &err_message),
                ],
                Some(&root.display().to_string()),
            )?;
            return Err(reported_io_error(err_message));
        }
        let storage = Storage::new(&db_path);
        if let Err(err) = storage.verify_runtime_readiness() {
            let err_message = format!(
                "startup strict vector readiness failed repository_id={} root={} db={}: {err}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            );
            output.error_event(
                "startup",
                "failed",
                &[
                    field("status", "failed"),
                    field("repos", repositories.len()),
                    field("repo", &repo.repository_id.0),
                    field("db", db_path.display()),
                    field("error", &err_message),
                ],
                Some(&root.display().to_string()),
            )?;
            return Err(reported_io_error(err_message));
        }
        let status = storage
            .verify_vector_store(DEFAULT_VECTOR_DIMENSIONS)
            .map_err(|err| {
                io::Error::other(format!(
                    "startup strict vector readiness failed repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                ))
            });

        let status = match status {
            Ok(status) => status,
            Err(err) => {
                output.error_event(
                    "startup",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repos", repositories.len()),
                        field("repo", &repo.repository_id.0),
                        field("db", db_path.display()),
                        field("error", &err),
                    ],
                    Some(&root.display().to_string()),
                )?;
                return Err(reported_io_error(err.to_string()));
            }
        };

        if status.backend != VectorStoreBackend::SqliteVec {
            let err_message = format!(
                "vector subsystem not ready: sqlite-vec backend unavailable (active backend: {})",
                status.backend.as_str()
            );
            output.error_event(
                "startup",
                "failed",
                &[
                    field("status", "failed"),
                    field("repos", repositories.len()),
                    field("repo", &repo.repository_id.0),
                    field("db", db_path.display()),
                    field("error", &err_message),
                ],
                Some(&root.display().to_string()),
            )?;
            return Err(reported_io_error(format!(
                "startup strict vector readiness failed repository_id={} root={} db={}: {err_message}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            )));
        }

        output.progress_event(
            OutputLevel::Ok,
            "startup",
            "storage",
            &[
                field("status", "ok"),
                field("repo", &repo.repository_id.0),
                field("db", db_path.display()),
                field("backend", status.backend.as_str()),
                field("extension_version", &status.extension_version),
            ],
            Some(&root.display().to_string()),
        )?;
        info!(
            repository_id = %repo.repository_id.0,
            root = %root.display(),
            db = %db_path.display(),
            extension_version = %status.extension_version,
            "startup strict vector readiness passed"
        );
    }

    Ok(())
}

/// Validates semantic-runtime credentials and prepares local model artifacts before serve or index.
pub(crate) fn run_semantic_runtime_startup_gate_with_output(
    config: &FriggConfig,
    output: &CliOutput,
) -> io::Result<()> {
    let credentials = SemanticRuntimeCredentials::from_process_env();
    run_semantic_runtime_startup_gate_with_credentials_and_output(
        config,
        &credentials,
        output,
        SemanticModelPrepareOutput::Stdout,
        ensure_local_semantic_model_prepared,
    )
}

/// Like [`run_semantic_runtime_startup_gate_with_output`], but routes local-model prep messages to stderr.
pub(crate) fn run_semantic_runtime_startup_gate_with_stderr_prepare_output(
    config: &FriggConfig,
    output: &CliOutput,
) -> io::Result<()> {
    let credentials = SemanticRuntimeCredentials::from_process_env();
    run_semantic_runtime_startup_gate_with_credentials_and_output(
        config,
        &credentials,
        output,
        SemanticModelPrepareOutput::Stderr,
        ensure_local_semantic_model_prepared,
    )
}

#[cfg(test)]
pub(crate) fn run_semantic_runtime_startup_gate_with_credentials(
    config: &FriggConfig,
    credentials: &SemanticRuntimeCredentials,
) -> io::Result<()> {
    run_semantic_runtime_startup_gate_with_credentials_and_output(
        config,
        credentials,
        &CliOutput::normal(),
        SemanticModelPrepareOutput::Stdout,
        skip_local_semantic_model_preparation,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticModelPrepareOutput {
    Stdout,
    Stderr,
}

type LocalModelStartupPreparer =
    fn(&FriggConfig, &str, &CliOutput, SemanticModelPrepareOutput) -> io::Result<()>;

fn run_semantic_runtime_startup_gate_with_credentials_and_output(
    config: &FriggConfig,
    credentials: &SemanticRuntimeCredentials,
    output: &CliOutput,
    prepare_output: SemanticModelPrepareOutput,
    local_model_preparer: LocalModelStartupPreparer,
) -> io::Result<()> {
    if !config.semantic_runtime.enabled {
        output.progress_event(
            OutputLevel::Skip,
            "startup",
            "semantic",
            &[field("status", "disabled")],
            None,
        )?;
        return Ok(());
    }

    if let Err(err) = config.semantic_runtime.validate_startup(credentials) {
        let startup_error = SemanticStartupGateError::InvalidConfig(err);
        let provider = config
            .semantic_runtime
            .provider
            .map(SemanticRuntimeProvider::as_str)
            .unwrap_or("-");
        let model = config.semantic_runtime.normalized_model().unwrap_or("-");
        output.error_event(
            "startup",
            "failed",
            &[
                field("status", "failed"),
                field("semantic_enabled", true),
                field("provider", provider),
                field("model", model),
                field("code", startup_error.code()),
                field("next", "check semantic runtime configuration"),
                field("error", &startup_error),
            ],
            None,
        )?;
        return Err(reported_io_error(format!(
            "startup semantic runtime readiness failed code={}: {}",
            startup_error.code(),
            startup_error
        )));
    }

    let provider = config
        .semantic_runtime
        .provider
        .expect("semantic runtime provider must exist after successful validation");
    let model = config
        .semantic_runtime
        .normalized_model()
        .expect("semantic runtime model must exist after successful validation");
    if provider == SemanticRuntimeProvider::Local {
        local_model_preparer(config, model, output, prepare_output)?;
    }
    output.progress_event(
        OutputLevel::Ok,
        "startup",
        "semantic",
        &[
            field("status", "ok"),
            field("provider", provider.as_str()),
            field("model", model),
            field("strict", config.semantic_runtime.strict_mode),
        ],
        None,
    )?;
    info!(
        semantic_provider = %provider.as_str(),
        semantic_model = %model,
        semantic_strict_mode = config.semantic_runtime.strict_mode,
        "startup semantic runtime readiness passed"
    );
    Ok(())
}

#[cfg(test)]
fn skip_local_semantic_model_preparation(
    _config: &FriggConfig,
    _model: &str,
    _output: &CliOutput,
    _prepare_output: SemanticModelPrepareOutput,
) -> io::Result<()> {
    Ok(())
}

fn ensure_local_semantic_model_prepared(
    config: &FriggConfig,
    model: &str,
    output: &CliOutput,
    prepare_output: SemanticModelPrepareOutput,
) -> io::Result<()> {
    match check_local_model_artifact(model) {
        Ok(LocalModelArtifactStatus::Ready(artifact)) => {
            if let Some(hf_home) = std::env::var_os(HF_HOME_ENV) {
                return fail_local_model_prepare(
                    output,
                    &artifact.semantic_model,
                    format!(
                        "{HF_HOME_ENV} is set to {}; unset it so {FRIGG_SEMANTIC_MODEL_CACHE_ENV} or Frigg's platform cache root controls prepared local model loading from {}",
                        std::path::PathBuf::from(hf_home).display(),
                        artifact.cache_root.display()
                    ),
                );
            }
            info!(
                semantic_provider = "local",
                semantic_model = %artifact.semantic_model,
                cache_root = %artifact.cache_root.display(),
                cache_key = %artifact.cache_key,
                "startup local semantic model already prepared"
            );
            output.progress_event(
                OutputLevel::Ok,
                "startup",
                "semantic_model",
                &[
                    field("status", "ready"),
                    field("provider", "local"),
                    field("model", &artifact.semantic_model),
                    field("cache_key", &artifact.cache_key),
                ],
                Some(&artifact.cache_root.display().to_string()),
            )?;
            Ok(())
        }
        Ok(LocalModelArtifactStatus::Missing(artifact)) => {
            prepare_missing_or_corrupt_local_semantic_model(
                config,
                artifact,
                "missing",
                output,
                prepare_output,
            )
        }
        Err(err) => {
            let model = config.semantic_runtime.normalized_model().unwrap_or(model);
            let artifact = frigg::embeddings::local_model::resolve_local_model_artifact(model);
            match artifact {
                Ok(artifact) => prepare_missing_or_corrupt_local_semantic_model(
                    config,
                    artifact,
                    "corrupt",
                    output,
                    prepare_output,
                ),
                Err(_) => fail_local_model_prepare(output, model, err.to_string()),
            }
        }
    }
}

fn prepare_missing_or_corrupt_local_semantic_model(
    config: &FriggConfig,
    artifact: LocalModelArtifact,
    reason: &str,
    output: &CliOutput,
    prepare_output: SemanticModelPrepareOutput,
) -> io::Result<()> {
    emit_model_prepare_progress(
        output,
        prepare_output,
        OutputLevel::Info,
        &[
            field("status", "started"),
            field("provider", "local"),
            field("model", &artifact.semantic_model),
            field("cache_key", &artifact.cache_key),
            field("repository", &artifact.repository),
            field("reason", reason),
        ],
        Some(&artifact.cache_root.display().to_string()),
    )?;

    match prepare_local_semantic_model(&config.semantic_runtime) {
        Ok(prepared) => {
            emit_model_prepare_progress(
                output,
                prepare_output,
                OutputLevel::Ok,
                &[
                    field("status", "finished"),
                    field("provider", "local"),
                    field("model", &prepared.semantic_model),
                    field("cache_key", &prepared.cache_key),
                    field("repository", &prepared.repository),
                ],
                Some(&prepared.cache_root.display().to_string()),
            )?;
            Ok(())
        }
        Err(err) => fail_local_model_prepare(output, &artifact.semantic_model, err.to_string()),
    }
}

fn fail_local_model_prepare(output: &CliOutput, model: &str, message: String) -> io::Result<()> {
    let startup_error = SemanticStartupGateError::LocalModelPrepare(message);
    output.error_event(
        "startup",
        "failed",
        &[
            field("status", "failed"),
            field("semantic_enabled", true),
            field("provider", "local"),
            field("model", model),
            field("code", startup_error.code()),
            field("next", "check local semantic model cache"),
            field("error", &startup_error),
        ],
        None,
    )?;
    Err(reported_io_error(format!(
        "startup semantic runtime readiness failed code={}: {}",
        startup_error.code(),
        startup_error
    )))
}

fn emit_model_prepare_progress(
    output: &CliOutput,
    target: SemanticModelPrepareOutput,
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
) -> io::Result<()> {
    match target {
        SemanticModelPrepareOutput::Stdout => {
            output.summary_event(level, "startup", "semantic_model", fields, path)
        }
        SemanticModelPrepareOutput::Stderr => {
            output.warning_event(level, "startup", "semantic_model", fields, path)
        }
    }
}
