use std::error::Error;
use std::io;

use frigg::indexer::{ManifestDiagnosticKind, ReindexMode, reindex_repository_with_runtime_config};
use frigg::settings::{FriggConfig, SemanticRuntimeCredentials};

use crate::cli_runtime::storage_paths::ensure_storage_db_path_for_write;

pub(crate) fn run_reindex_command(
    config: &FriggConfig,
    changed: bool,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let mode = if changed {
        ReindexMode::ChangedOnly
    } else {
        ReindexMode::Full
    };
    let mode_name = mode.as_str();
    let mut total_files_scanned = 0usize;
    let mut total_files_changed = 0usize;
    let mut total_files_deleted = 0usize;
    let mut total_diagnostics = 0usize;
    let mut total_walk_diagnostics = 0usize;
    let mut total_read_diagnostics = 0usize;
    let mut total_duration_ms = 0u128;

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "reindex summary status=failed mode={mode_name} repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = ensure_storage_db_path_for_write(root, "reindex")?;

        let summary = match reindex_repository_with_runtime_config(
            &repo.repository_id.0,
            root,
            &db_path,
            mode,
            &config.semantic_runtime,
            &SemanticRuntimeCredentials::from_process_env(),
        ) {
            Ok(summary) => summary,
            Err(err) => {
                println!(
                    "reindex summary status=failed mode={mode_name} repositories={} repository_id={} root={} db={} error={}",
                    repositories.len(),
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    err
                );
                return Err(Box::new(io::Error::other(format!(
                    "reindex failed mode={mode_name} repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                ))));
            }
        };

        total_files_scanned += summary.files_scanned;
        total_files_changed += summary.files_changed;
        total_files_deleted += summary.files_deleted;
        let diagnostics_total = summary.diagnostics.total_count();
        let diagnostics_walk = summary
            .diagnostics
            .count_by_kind(ManifestDiagnosticKind::Walk);
        let diagnostics_read = summary
            .diagnostics
            .count_by_kind(ManifestDiagnosticKind::Read);
        total_diagnostics += diagnostics_total;
        total_walk_diagnostics += diagnostics_walk;
        total_read_diagnostics += diagnostics_read;
        total_duration_ms += summary.duration_ms;

        for diagnostic in &summary.diagnostics.entries {
            println!(
                "reindex diagnostic mode={mode_name} repository_id={} kind={} path={} message={}",
                repo.repository_id.0,
                diagnostic.kind.as_str(),
                diagnostic
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                diagnostic.message
            );
        }

        println!(
            "reindex ok mode={mode_name} repository_id={} root={} db={} snapshot_id={} files_scanned={} files_changed={} files_deleted={} diagnostics_total={} diagnostics_walk={} diagnostics_read={} duration_ms={}",
            repo.repository_id.0,
            root.display(),
            db_path.display(),
            summary.snapshot_id,
            summary.files_scanned,
            summary.files_changed,
            summary.files_deleted,
            diagnostics_total,
            diagnostics_walk,
            diagnostics_read,
            summary.duration_ms
        );
    }

    println!(
        "reindex summary status=ok mode={mode_name} repositories={} files_scanned={} files_changed={} files_deleted={} diagnostics_total={} diagnostics_walk={} diagnostics_read={} duration_ms={}",
        repositories.len(),
        total_files_scanned,
        total_files_changed,
        total_files_deleted,
        total_diagnostics,
        total_walk_diagnostics,
        total_read_diagnostics,
        total_duration_ms
    );
    Ok(())
}
