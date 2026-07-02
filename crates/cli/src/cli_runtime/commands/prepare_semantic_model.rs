//! CLI `prepare-semantic-model` command: explicit local embedding artifact preparation.

use std::error::Error;
use std::io;

use frigg::embeddings::local_model::{
    LocalModelArtifact, LocalModelArtifactStatus, LocalModelError, check_local_model_artifact,
    prepare_local_semantic_model,
};
use frigg::settings::{FriggConfig, SemanticRuntimeProvider};

use crate::cli_runtime::CliOutput;

pub(crate) fn run_prepare_semantic_model_command_with_output(
    config: &FriggConfig,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let provider = config.semantic_runtime.provider.ok_or_else(|| {
        io::Error::other(
            "prepare-semantic-model requires --semantic-runtime-enabled true --semantic-runtime-provider local",
        )
    })?;
    if provider != SemanticRuntimeProvider::Local {
        return Err(Box::new(io::Error::other(format!(
            "prepare-semantic-model only prepares local artifacts; active semantic provider '{}' uses external embeddings",
            provider.as_str()
        ))));
    }
    if !config.semantic_runtime.enabled {
        return Err(Box::new(io::Error::other(
            "prepare-semantic-model requires --semantic-runtime-enabled true",
        )));
    }
    config.semantic_runtime.validate()?;

    let model = config
        .semantic_runtime
        .normalized_model()
        .expect("semantic model exists after validation");
    match check_local_model_artifact(model) {
        Ok(LocalModelArtifactStatus::Ready(artifact)) => {
            print_prepared_summary("already_ready", &artifact, output)?;
            return Ok(());
        }
        Ok(LocalModelArtifactStatus::Missing(_)) => {}
        Err(LocalModelError::Corrupt { .. }) => {}
        Err(err) => return Err(Box::new(io::Error::other(err.to_string()))),
    }

    let artifact = prepare_local_semantic_model(&config.semantic_runtime)
        .map_err(|err| io::Error::other(err.to_string()))?;
    print_prepared_summary("prepared", &artifact, output)?;
    Ok(())
}

fn print_prepared_summary(
    status: &str,
    artifact: &LocalModelArtifact,
    output: &CliOutput,
) -> io::Result<()> {
    output.summary(format!(
        "prepare-semantic-model summary status={status} semantic_provider=local semantic_model={} cache_root={} cache_key={} repository={}",
        artifact.semantic_model,
        artifact.cache_root.display(),
        artifact.cache_key,
        artifact.repository
    ))?;
    output.advisory(format!(
        "prepare-semantic-model next_step=\"run `frigg reindex` after changing semantic provider or model so semantic rows match provider=local model={}\"",
        artifact.semantic_model
    ))
}
