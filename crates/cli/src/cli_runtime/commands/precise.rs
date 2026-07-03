//! CLI hooks that plan and run synchronous SCIP precise-artifact generation after index or init.
//!
//! Reuses the MCP server's generator selection and external-tool execution paths, then emits
//! structured progress events and aggregate counters for operator-facing command summaries.

use std::path::Path;

use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    WorkspacePreciseGenerationPlanItem, WorkspacePreciseGenerationPlanSummary,
    WorkspacePreciseGenerationRunItem, WorkspacePreciseGenerationRunSummary,
    WorkspacePreciseGenerationStatus, WorkspaceRecommendedAction,
};

use crate::cli_runtime::{CliOutput, OutputField, OutputLevel, field};

/// Roll-up counters for precise-generator progress emitted during one CLI command run.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CliPreciseGenerationCounters {
    pub(crate) generators: usize,
    pub(crate) succeeded: usize,
    pub(crate) failed: usize,
    pub(crate) missing_tool: usize,
    pub(crate) skipped: usize,
}

impl CliPreciseGenerationCounters {
    /// Merges counters from one repository run into a command-level roll-up.
    pub(crate) fn add(&mut self, other: Self) {
        self.generators += other.generators;
        self.succeeded += other.succeeded;
        self.failed += other.failed;
        self.missing_tool += other.missing_tool;
        self.skipped += other.skipped;
    }

    fn record_status(&mut self, status: WorkspacePreciseGenerationStatus) {
        self.generators += 1;
        match status {
            WorkspacePreciseGenerationStatus::Succeeded => self.succeeded += 1,
            WorkspacePreciseGenerationStatus::Failed
            | WorkspacePreciseGenerationStatus::Timeout => {
                self.failed += 1;
            }
            WorkspacePreciseGenerationStatus::MissingTool => self.missing_tool += 1,
            WorkspacePreciseGenerationStatus::Skipped
            | WorkspacePreciseGenerationStatus::Unsupported
            | WorkspacePreciseGenerationStatus::NotConfigured => self.skipped += 1,
        }
    }
}

/// Maps precise-generation counters into CLI output fields for per-repo and command summaries.
pub(crate) fn precise_counter_fields(counters: CliPreciseGenerationCounters) -> Vec<OutputField> {
    vec![
        field("precise_generators", counters.generators),
        field("precise_succeeded", counters.succeeded),
        field("precise_failed", counters.failed),
        field("precise_missing_tool", counters.missing_tool),
        field("precise_skipped", counters.skipped),
    ]
}

/// Plans and runs synchronous precise-artifact generation for one repository.
///
/// Plan or run failures are reported as progress warnings and return zeroed counters so
/// `index` and `init` can finish without treating generator errors as command failures.
pub(crate) fn run_cli_precise_generation(
    server: &FriggMcpServer,
    output: CliOutput,
    command_name: &'static str,
    repository_id: &str,
    root: &Path,
    changed_paths: &[String],
    deleted_paths: &[String],
) -> CliPreciseGenerationCounters {
    let plan = match server.precise_generation_plan_for_repository(
        repository_id,
        changed_paths,
        deleted_paths,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            let _ = output.progress_event(
                OutputLevel::Warn,
                "precise",
                "plan",
                &[
                    field("status", "failed"),
                    field("repo", repository_id),
                    field("command", command_name),
                    field("error", error),
                ],
                Some(&root.display().to_string()),
            );
            return CliPreciseGenerationCounters::default();
        }
    };

    emit_precise_plan(
        output,
        command_name,
        repository_id,
        root,
        changed_paths,
        deleted_paths,
        &plan,
    );
    if plan.generators.is_empty() {
        return CliPreciseGenerationCounters::default();
    }
    emit_precise_generator_start_events(output, command_name, repository_id, &plan);

    let run = match server.run_precise_generation_for_repository(
        repository_id,
        changed_paths,
        deleted_paths,
    ) {
        Ok(run) => run,
        Err(error) => {
            let _ = output.progress_event(
                OutputLevel::Warn,
                "precise",
                "run",
                &[
                    field("status", "failed"),
                    field("repo", repository_id),
                    field("command", command_name),
                    field("error", error),
                ],
                Some(&root.display().to_string()),
            );
            return CliPreciseGenerationCounters::default();
        }
    };

    emit_precise_run(output, command_name, repository_id, run)
}

fn emit_precise_plan(
    output: CliOutput,
    command_name: &'static str,
    repository_id: &str,
    root: &Path,
    changed_paths: &[String],
    deleted_paths: &[String],
    plan: &WorkspacePreciseGenerationPlanSummary,
) {
    if plan.generators.is_empty() {
        let _ = output.progress_event(
            OutputLevel::Skip,
            "precise",
            "plan",
            &[
                field("status", "empty"),
                field("repo", repository_id),
                field("command", command_name),
                field("reason", "no_generators_need_refresh"),
                field("changed", changed_paths.len()),
                field("deleted", deleted_paths.len()),
            ],
            Some(&root.display().to_string()),
        );
        return;
    }

    let generator_ids = plan
        .generators
        .iter()
        .map(|generator| generator.generator_id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let _ = output.progress_event(
        OutputLevel::Info,
        "precise",
        "plan",
        &[
            field("status", "starting"),
            field("repo", repository_id),
            field("command", command_name),
            field("generators", plan.generators.len()),
            field("generator_ids", generator_ids),
            field("changed", changed_paths.len()),
            field("deleted", deleted_paths.len()),
        ],
        Some(&root.display().to_string()),
    );
}

fn emit_precise_generator_start_events(
    output: CliOutput,
    command_name: &'static str,
    repository_id: &str,
    plan: &WorkspacePreciseGenerationPlanSummary,
) {
    for generator in &plan.generators {
        emit_precise_generator_start(output, command_name, repository_id, generator);
    }
}

fn emit_precise_generator_start(
    output: CliOutput,
    command_name: &'static str,
    repository_id: &str,
    generator: &WorkspacePreciseGenerationPlanItem,
) {
    let _ = output.progress_event(
        OutputLevel::Info,
        "precise",
        "generator",
        &[
            field("status", "starting"),
            field("repo", repository_id),
            field("command", command_name),
            field("generator", &generator.generator_id),
            field("language", &generator.language),
            field("tool", &generator.tool),
        ],
        None,
    );
}

fn emit_precise_run(
    output: CliOutput,
    command_name: &'static str,
    repository_id: &str,
    run: WorkspacePreciseGenerationRunSummary,
) -> CliPreciseGenerationCounters {
    let mut counters = CliPreciseGenerationCounters::default();
    for item in &run.generators {
        counters.record_status(item.summary.status);
        emit_precise_generator(output, command_name, repository_id, item);
    }
    counters
}

fn emit_precise_generator(
    output: CliOutput,
    command_name: &'static str,
    repository_id: &str,
    item: &WorkspacePreciseGenerationRunItem,
) {
    let summary = &item.summary;
    let mut fields = vec![
        field("status", generation_status_label(summary.status)),
        field("repo", repository_id),
        field("command", command_name),
        field("generator", &item.generator_id),
        field("language", &item.language),
        field("tool", &item.tool),
        field("artifacts", summary.artifact_count.unwrap_or(0)),
        field("bytes", summary.artifact_bytes.unwrap_or(0)),
        field("duration_ms", summary.duration_ms.unwrap_or(0)),
    ];
    if let Some(failure_class) = summary.failure_class {
        fields.push(field(
            "failure_class",
            format!("{failure_class:?}").to_ascii_snake_case(),
        ));
    }
    if let Some(recommended_action) = summary.recommended_action {
        fields.push(field("next", recommended_action_label(recommended_action)));
    }
    if let Some(detail) = summary.detail.as_deref() {
        fields.push(field("detail", detail));
    }

    let path = summary
        .artifact_path
        .as_deref()
        .unwrap_or(item.expected_output_path.as_str());
    let _ = output.progress_event(
        generation_output_level(summary.status),
        "precise",
        "generator",
        &fields,
        Some(path),
    );
}

fn generation_output_level(status: WorkspacePreciseGenerationStatus) -> OutputLevel {
    match status {
        WorkspacePreciseGenerationStatus::Succeeded => OutputLevel::Ok,
        WorkspacePreciseGenerationStatus::Failed | WorkspacePreciseGenerationStatus::Timeout => {
            OutputLevel::Warn
        }
        WorkspacePreciseGenerationStatus::Skipped
        | WorkspacePreciseGenerationStatus::MissingTool
        | WorkspacePreciseGenerationStatus::Unsupported
        | WorkspacePreciseGenerationStatus::NotConfigured => OutputLevel::Skip,
    }
}

fn generation_status_label(status: WorkspacePreciseGenerationStatus) -> &'static str {
    match status {
        WorkspacePreciseGenerationStatus::Succeeded => "ok",
        WorkspacePreciseGenerationStatus::Failed => "failed",
        WorkspacePreciseGenerationStatus::Skipped => "skipped",
        WorkspacePreciseGenerationStatus::MissingTool => "missing_tool",
        WorkspacePreciseGenerationStatus::Unsupported => "unsupported",
        WorkspacePreciseGenerationStatus::NotConfigured => "not_configured",
        WorkspacePreciseGenerationStatus::Timeout => "timeout",
    }
}

fn recommended_action_label(action: WorkspaceRecommendedAction) -> &'static str {
    match action {
        WorkspaceRecommendedAction::InstallTool => "install_tool",
        WorkspaceRecommendedAction::RerunIndex => "rerun_index",
        WorkspaceRecommendedAction::CheckEnvironment => "check_environment",
        WorkspaceRecommendedAction::UpstreamToolFailure => "upstream_tool_failure",
        WorkspaceRecommendedAction::UseHeuristicMode => "use_heuristic_mode",
    }
}

trait ToAsciiSnakeCase {
    fn to_ascii_snake_case(self) -> String;
}

impl ToAsciiSnakeCase for String {
    fn to_ascii_snake_case(self) -> String {
        let mut output = String::new();
        for (index, ch) in self.chars().enumerate() {
            if ch.is_ascii_uppercase() {
                if index > 0 {
                    output.push('_');
                }
                output.push(ch.to_ascii_lowercase());
            } else {
                output.push(ch);
            }
        }
        output
    }
}
