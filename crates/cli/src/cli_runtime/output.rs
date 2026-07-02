//! Small CLI output policy layer for stable stdout results and stderr diagnostics.

use std::error::Error;
use std::fmt::Display;
use std::io::{self, Write};

use frigg::indexer::IndexPlan;

const MAX_VERBOSE_PATH_LINES: usize = 50;

/// Quiet, normal, or verbose CLI output policy selected from global flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Quiet,
    Normal,
    Verbose,
}

impl OutputMode {
    pub(crate) fn resolve(quiet: bool, verbose: bool) -> io::Result<Self> {
        match (quiet, verbose) {
            (true, true) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--quiet and --verbose cannot be used together",
            )),
            (true, false) => Ok(Self::Quiet),
            (false, true) => Ok(Self::Verbose),
            (false, false) => Ok(Self::Normal),
        }
    }
}

/// Output sink that routes structured result lines to stdout and diagnostics to stderr by mode.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CliOutput {
    mode: OutputMode,
}

/// Severity label embedded in structured CLI event lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputLevel {
    Ok,
    Info,
    Warn,
    Error,
    Skip,
}

impl OutputLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Skip => "skip",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OutputField {
    key: &'static str,
    value: String,
}

/// Builds one `key=value` field for structured CLI event formatting.
pub(crate) fn field(key: &'static str, value: impl Display) -> OutputField {
    OutputField {
        key,
        value: value.to_string(),
    }
}

impl CliOutput {
    pub(crate) const fn new(mode: OutputMode) -> Self {
        Self { mode }
    }

    pub(crate) const fn normal() -> Self {
        Self::new(OutputMode::Normal)
    }

    pub(crate) fn from_flags(quiet: bool, verbose: bool) -> io::Result<Self> {
        Ok(Self::new(OutputMode::resolve(quiet, verbose)?))
    }

    pub(crate) const fn is_quiet(self) -> bool {
        matches!(self.mode, OutputMode::Quiet)
    }

    pub(crate) const fn is_verbose(self) -> bool {
        matches!(self.mode, OutputMode::Verbose)
    }

    pub(crate) fn result_event(
        self,
        level: OutputLevel,
        area: &str,
        event: &str,
        fields: &[OutputField],
        path: Option<&str>,
    ) -> io::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        write_stdout_line(&format_event_line(level, area, event, fields, path))
    }

    pub(crate) fn summary_event(
        self,
        level: OutputLevel,
        area: &str,
        event: &str,
        fields: &[OutputField],
        path: Option<&str>,
    ) -> io::Result<()> {
        self.result_event(level, area, event, fields, path)
    }

    pub(crate) fn warning_event(
        self,
        level: OutputLevel,
        area: &str,
        event: &str,
        fields: &[OutputField],
        path: Option<&str>,
    ) -> io::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        write_stderr_line(&format_event_line(level, area, event, fields, path))
    }

    pub(crate) fn error_event(
        self,
        area: &str,
        event: &str,
        fields: &[OutputField],
        path: Option<&str>,
    ) -> io::Result<()> {
        write_stderr_line(&format_event_line(
            OutputLevel::Error,
            area,
            event,
            fields,
            path,
        ))
    }

    pub(crate) fn progress_event(
        self,
        level: OutputLevel,
        area: &str,
        event: &str,
        fields: &[OutputField],
        path: Option<&str>,
    ) -> io::Result<()> {
        if !self.is_verbose() {
            return Ok(());
        }
        write_stderr_line(&format_event_line(level, area, event, fields, path))
    }

    pub(crate) fn diagnostic_event(
        self,
        level: OutputLevel,
        area: &str,
        event: &str,
        fields: &[OutputField],
        path: Option<&str>,
    ) -> io::Result<()> {
        self.progress_event(level, area, event, fields, path)
    }
}

pub(crate) fn format_output_event_line(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
) -> String {
    let mut line = String::new();
    line.push_str(level.as_str());
    line.push(' ');
    line.push_str(area);
    line.push_str(": ");
    line.push_str(event);
    for field in fields {
        line.push(' ');
        line.push_str(field.key);
        line.push('=');
        line.push_str(&format_field_value(&field.value));
    }
    if let Some(path) = path {
        line.push_str(" -- ");
        line.push_str(&format_field_value(path));
    }
    line
}

/// Emits verbose index planning and path-delta progress events for one repository plan.
pub(crate) fn emit_index_plan_events(
    output: CliOutput,
    repository_id: &str,
    plan: &IndexPlan,
    extra_fields: &[OutputField],
) -> io::Result<()> {
    output.progress_event(
        OutputLevel::Info,
        "index",
        "plan",
        &with_extra_fields(
            vec![
                field("status", "starting"),
                field("repo", repository_id),
                field("mode", plan.mode.as_str()),
                field("snapshot_plan", plan.snapshot_plan.as_str()),
                field(
                    "previous",
                    plan.previous_snapshot_id.as_deref().unwrap_or("-"),
                ),
                field("next", plan.snapshot_plan.snapshot_id()),
                field("semantic", plan.semantic_refresh.mode.as_str()),
                field(
                    "provider",
                    plan.semantic_refresh.provider.as_deref().unwrap_or("-"),
                ),
                field(
                    "model",
                    plan.semantic_refresh.model.as_deref().unwrap_or("-"),
                ),
                field(
                    "semantic_records",
                    plan.semantic_refresh.records_manifest.len(),
                ),
                field("changed", plan.changed_paths.len()),
                field("deleted", plan.deleted_paths.len()),
            ],
            extra_fields,
        ),
        None,
    )?;

    if plan.semantic_refresh.mode.as_str() == "full_rebuild_from_changed_only" {
        output.progress_event(
            OutputLevel::Info,
            "index",
            "fallback",
            &with_extra_fields(
                vec![
                    field("status", "ok"),
                    field("repo", repository_id),
                    field("from", "changed_only"),
                    field("semantic", "full_rebuild"),
                    field("reason", "semantic_head_stale_or_deleted_paths_unresolved"),
                ],
                extra_fields,
            ),
            None,
        )?;
    }

    emit_index_semantic_events(output, repository_id, plan, extra_fields)?;

    if plan.changed_paths.is_empty() && plan.deleted_paths.is_empty() {
        output.progress_event(
            OutputLevel::Skip,
            "index",
            "paths",
            &with_extra_fields(
                vec![
                    field("status", "empty"),
                    field("repo", repository_id),
                    field("changed", 0),
                    field("deleted", 0),
                ],
                extra_fields,
            ),
            None,
        )?;
        return Ok(());
    }

    emit_index_path_lines(
        output,
        "path",
        "paths",
        repository_id,
        "modified",
        &plan.changed_paths,
        extra_fields,
    )?;
    emit_index_path_lines(
        output,
        "path",
        "paths",
        repository_id,
        "deleted",
        &plan.deleted_paths,
        extra_fields,
    )
}

fn emit_index_semantic_events(
    output: CliOutput,
    repository_id: &str,
    plan: &IndexPlan,
    extra_fields: &[OutputField],
) -> io::Result<()> {
    if matches!(
        plan.semantic_refresh.mode.as_str(),
        "disabled" | "reuse_existing"
    ) {
        return Ok(());
    }

    output.progress_event(
        OutputLevel::Info,
        "index",
        "semantic",
        &with_extra_fields(
            vec![
                field("status", "starting"),
                field("repo", repository_id),
                field("mode", plan.semantic_refresh.mode.as_str()),
                field(
                    "provider",
                    plan.semantic_refresh.provider.as_deref().unwrap_or("-"),
                ),
                field(
                    "model",
                    plan.semantic_refresh.model.as_deref().unwrap_or("-"),
                ),
                field("records", plan.semantic_refresh.records_manifest.len()),
                field("changed", plan.semantic_refresh.changed_paths.len()),
                field("deleted", plan.semantic_refresh.deleted_paths.len()),
            ],
            extra_fields,
        ),
        None,
    )?;

    emit_index_path_lines(
        output,
        "semantic_path",
        "semantic_paths",
        repository_id,
        "modified",
        &plan.semantic_refresh.changed_paths,
        extra_fields,
    )?;
    emit_index_path_lines(
        output,
        "semantic_path",
        "semantic_paths",
        repository_id,
        "deleted",
        &plan.semantic_refresh.deleted_paths,
        extra_fields,
    )
}

fn emit_index_path_lines(
    output: CliOutput,
    event: &'static str,
    truncation_event: &'static str,
    repository_id: &str,
    action: &'static str,
    paths: &[String],
    extra_fields: &[OutputField],
) -> io::Result<()> {
    for path in paths.iter().take(MAX_VERBOSE_PATH_LINES) {
        output.progress_event(
            OutputLevel::Info,
            "index",
            event,
            &with_extra_fields(
                vec![field("action", action), field("repo", repository_id)],
                extra_fields,
            ),
            Some(path),
        )?;
    }
    if paths.len() > MAX_VERBOSE_PATH_LINES {
        output.progress_event(
            OutputLevel::Info,
            "index",
            truncation_event,
            &with_extra_fields(
                vec![
                    field("status", "truncated"),
                    field("repo", repository_id),
                    field("action", action),
                    field("shown", MAX_VERBOSE_PATH_LINES),
                    field("omitted", paths.len() - MAX_VERBOSE_PATH_LINES),
                ],
                extra_fields,
            ),
            None,
        )?;
    }
    Ok(())
}

fn with_extra_fields(
    mut fields: Vec<OutputField>,
    extra_fields: &[OutputField],
) -> Vec<OutputField> {
    fields.extend_from_slice(extra_fields);
    fields
}

fn format_field_value(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | ','))
    {
        return value.to_owned();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn write_stdout_line(message: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{message}")
}

fn write_stderr_line(message: &str) -> io::Result<()> {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    writeln!(handle, "{message}")
}

fn format_event_line(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
) -> String {
    format_output_event_line(level, area, event, fields, path)
}

#[derive(Debug)]
pub(crate) struct ReportedCliError {
    message: String,
}

impl ReportedCliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ReportedCliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ReportedCliError {}

/// Wraps an error whose message was already emitted through structured CLI output.
pub(crate) fn reported_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(ReportedCliError::new(message))
}

pub(crate) fn reported_io_error(message: impl Into<String>) -> io::Error {
    io::Error::other(ReportedCliError::new(message))
}

pub(crate) fn error_was_reported(error: &(dyn Error + 'static)) -> bool {
    if error.is::<ReportedCliError>() {
        return true;
    }
    if let Some(io_error) = error.downcast_ref::<io::Error>() {
        return io_error
            .get_ref()
            .is_some_and(|source| error_was_reported(source));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{OutputLevel, OutputMode, field, format_event_line};

    #[test]
    fn output_mode_resolves_quiet() {
        assert_eq!(OutputMode::resolve(true, false).unwrap(), OutputMode::Quiet);
    }

    #[test]
    fn output_mode_resolves_normal() {
        assert_eq!(
            OutputMode::resolve(false, false).unwrap(),
            OutputMode::Normal
        );
    }

    #[test]
    fn output_mode_resolves_verbose() {
        assert_eq!(
            OutputMode::resolve(false, true).unwrap(),
            OutputMode::Verbose
        );
    }

    #[test]
    fn output_mode_rejects_quiet_verbose_conflict() {
        let error = OutputMode::resolve(true, true)
            .expect_err("quiet and verbose must not be accepted together");
        assert!(error.to_string().contains("--quiet and --verbose"));
    }

    #[test]
    fn event_line_keeps_scan_fields_before_path_suffix() {
        let line = format_event_line(
            OutputLevel::Info,
            "index",
            "path",
            &[field("action", "modified"), field("repo", "repo-001")],
            Some("src/main.rs"),
        );

        assert_eq!(
            line,
            "info index: path action=modified repo=repo-001 -- src/main.rs"
        );
    }

    #[test]
    fn event_line_quotes_values_with_spaces() {
        let line = format_event_line(
            OutputLevel::Error,
            "startup",
            "failed",
            &[field("error", "missing API key")],
            None,
        );

        assert_eq!(line, "error startup: failed error=\"missing API key\"");
    }

    #[test]
    fn event_line_escapes_path_suffix_controls() {
        let line = format_event_line(
            OutputLevel::Info,
            "index",
            "path",
            &[field("action", "modified")],
            Some("src/path with\nnewline.rs"),
        );

        assert_eq!(
            line,
            "info index: path action=modified -- \"src/path with\\nnewline.rs\""
        );
    }
}
