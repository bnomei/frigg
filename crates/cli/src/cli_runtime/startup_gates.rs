use std::io;

use frigg::settings::{
    FriggConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider, SemanticRuntimeStartupError,
};
use frigg::storage::{DEFAULT_VECTOR_DIMENSIONS, Storage, VectorStoreBackend};
use tracing::info;

use crate::cli_runtime::storage_paths::resolve_storage_db_path;

#[derive(Debug)]
pub(super) enum SemanticStartupGateError {
    InvalidConfig(SemanticRuntimeStartupError),
}

impl SemanticStartupGateError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig(err) => err.code(),
        }
    }
}

impl std::fmt::Display for SemanticStartupGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(err) => write!(f, "{err}"),
        }
    }
}

pub(crate) fn run_strict_startup_vector_readiness_gate(config: &FriggConfig) -> io::Result<()> {
    let repositories = config.repositories();

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "startup summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
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
            println!(
                "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err_message
            );
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
                println!(
                    "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                    repositories.len(),
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    err
                );
                return Err(err);
            }
        };

        if status.backend != VectorStoreBackend::SqliteVec {
            let err_message = format!(
                "vector subsystem not ready: sqlite-vec backend unavailable (active backend: {})",
                status.backend.as_str()
            );
            println!(
                "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err_message
            );
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

pub(crate) fn run_semantic_runtime_startup_gate(config: &FriggConfig) -> io::Result<()> {
    let credentials = SemanticRuntimeCredentials::from_process_env();
    run_semantic_runtime_startup_gate_with_credentials(config, &credentials)
}

pub(crate) fn run_semantic_runtime_startup_gate_with_credentials(
    config: &FriggConfig,
    credentials: &SemanticRuntimeCredentials,
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
        println!(
            "startup summary status=failed semantic_enabled=true semantic_provider={} semantic_model={} semantic_code={} error={}",
            provider,
            model,
            startup_error.code(),
            startup_error
        );
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
    info!(
        semantic_provider = %provider.as_str(),
        semantic_model = %model,
        semantic_strict_mode = config.semantic_runtime.strict_mode,
        "startup semantic runtime readiness passed"
    );
    Ok(())
}
