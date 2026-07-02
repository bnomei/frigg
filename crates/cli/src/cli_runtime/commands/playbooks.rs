//! CLI `playbook-hybrid-run` command: executes markdown hybrid playbooks and prints regression summaries.

use std::error::Error;
use std::io;
use std::path::Path;

use frigg::playbooks::run_hybrid_playbook_regressions;
use frigg::searcher::TextSearcher;
use frigg::settings::FriggConfig;
use serde_json::to_string_pretty;

use crate::cli_runtime::CliOutput;

pub(crate) fn run_hybrid_playbook_command_with_output(
    config: &FriggConfig,
    playbooks_root: &Path,
    enforce_targets: bool,
    output_path: Option<&Path>,
    trace_root: Option<&Path>,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let searcher = TextSearcher::new(config.clone());
    let summary =
        run_hybrid_playbook_regressions(&searcher, playbooks_root, enforce_targets, trace_root)?;

    for outcome in &summary.outcomes {
        output.diagnostic(format!(
            "playbook result playbook_id={} file={} semantic_status={} status_allowed={} duration_ms={} execution_error={} trace_path={} required_missing={:?} target_missing={:?} hits={:?}",
            outcome.playbook_id,
            outcome.file_name,
            outcome.semantic_status,
            outcome.status_allowed,
            outcome.duration_ms,
            outcome.execution_error.as_deref().unwrap_or("-"),
            outcome.trace_path.as_deref().unwrap_or("-"),
            outcome.required_missing(),
            outcome.target_missing(),
            outcome.matched_paths
        ))?;
    }

    if let Some(output_path) = output_path {
        let parent = output_path.parent().ok_or_else(|| {
            io::Error::other(format!(
                "playbook summary output path has no parent: {}",
                output_path.display()
            ))
        })?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(output_path, to_string_pretty(&summary)?)?;
    }

    let regressions_failed =
        summary.required_failures > 0 || (summary.enforce_targets && summary.target_failures > 0);
    let status = if regressions_failed { "failed" } else { "ok" };

    let summary_line = format!(
        "playbook summary status={status} playbooks={} required_failures={} target_failures={} enforce_targets={} output={} trace_root={}",
        summary.playbook_count,
        summary.required_failures,
        summary.target_failures,
        summary.enforce_targets,
        output_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_owned()),
        trace_root
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_owned())
    );

    if regressions_failed {
        output.error(summary_line)?;
        return Err(Box::new(io::Error::other(format!(
            "hybrid playbook regressions failed: required_failures={} target_failures={} enforce_targets={}",
            summary.required_failures, summary.target_failures, summary.enforce_targets
        ))));
    }
    output.summary(summary_line)?;
    Ok(())
}
