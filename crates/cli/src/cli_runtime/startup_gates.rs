//! Startup gates that block serve and semantic reindex until vector storage and embedding
//! credentials satisfy the resolved runtime contract.

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

use crate::cli_runtime::CliOutput;
use crate::cli_runtime::storage_paths::resolve_storage_db_path;

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

pub(crate) fn run_strict_startup_vector_readiness_gate_with_output(
    config: &FriggConfig,
    output: &CliOutput,
) -> io::Result<()> {
    let repositories = config.repositories();

    for repo in &repositories {
        let root = match config.root_by_repository_id(&repo.repository_id.0) {
            Some(root) => root,
            None => {
                let message = format!(
                    "startup summary status=failed repository_id={} error=workspace root lookup failed",
                    repo.repository_id.0
                );
                output.error(&message)?;
                return Err(io::Error::other(message));
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
            output.error(format!(
                "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err_message
            ))?;
            return Err(io::Error::other(err_message));
        }
        let storage = Storage::new(&db_path);
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
                output.error(format!(
                    "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                    repositories.len(),
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    err
                ))?;
                return Err(err);
            }
        };

        if status.backend != VectorStoreBackend::SqliteVec {
            let err_message = format!(
                "vector subsystem not ready: sqlite-vec backend unavailable (active backend: {})",
                status.backend.as_str()
            );
            output.error(format!(
                "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err_message
            ))?;
            return Err(io::Error::other(format!(
                "startup strict vector readiness failed repository_id={} root={} db={}: {err_message}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            )));
        }

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
        output.error(format!(
            "startup summary status=failed semantic_enabled=true semantic_provider={} semantic_model={} semantic_code={} error={}",
            provider,
            model,
            startup_error.code(),
            startup_error
        ))?;
        return Err(io::Error::other(format!(
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
        format!(
            "startup semantic_model_prepare status=started semantic_provider=local semantic_model={} cache_root={} cache_key={} repository={} reason={}",
            artifact.semantic_model,
            artifact.cache_root.display(),
            artifact.cache_key,
            artifact.repository,
            reason
        ),
    )?;

    match prepare_local_semantic_model(&config.semantic_runtime) {
        Ok(prepared) => {
            emit_model_prepare_progress(
                output,
                prepare_output,
                format!(
                    "startup semantic_model_prepare status=finished semantic_provider=local semantic_model={} cache_root={} cache_key={} repository={}",
                    prepared.semantic_model,
                    prepared.cache_root.display(),
                    prepared.cache_key,
                    prepared.repository
                ),
            )?;
            Ok(())
        }
        Err(err) => fail_local_model_prepare(output, &artifact.semantic_model, err.to_string()),
    }
}

fn fail_local_model_prepare(output: &CliOutput, model: &str, message: String) -> io::Result<()> {
    let startup_error = SemanticStartupGateError::LocalModelPrepare(message);
    output.error(format!(
        "startup summary status=failed semantic_enabled=true semantic_provider=local semantic_model={} semantic_code={} error={}",
        model,
        startup_error.code(),
        startup_error
    ))?;
    Err(io::Error::other(format!(
        "startup semantic runtime readiness failed code={}: {}",
        startup_error.code(),
        startup_error
    )))
}

fn emit_model_prepare_progress(
    output: &CliOutput,
    target: SemanticModelPrepareOutput,
    message: String,
) -> io::Result<()> {
    match target {
        SemanticModelPrepareOutput::Stdout => output.summary(message),
        SemanticModelPrepareOutput::Stderr => output.warning(message),
    }
}
