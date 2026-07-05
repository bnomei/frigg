//! CLI output policy for stable machine lines and human terminal diagnostics.
//!
//! Non-interactive callers receive one-line structured events on stdout or stderr, while TTY
//! sessions receive richer human blocks on stderr without changing the underlying event fields.

use std::error::Error;
use std::fmt::Display;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use frigg::mcp::{ToolCallDisplayEvent, ToolCallDisplayStatus};

mod fields;
mod human_block;
mod human_event;
mod human_topic;
mod index;
mod precise;

use fields::format_field_value;
use human_block::{format_human_intro, human_color_enabled};
use human_event::format_human_event;
#[cfg(test)]
use human_event::format_human_event_with_width;
#[cfg(test)]
use human_topic::{
    HUMAN_COLOR_ACTION_CREATED, HUMAN_COLOR_ACTION_DELETED, HUMAN_COLOR_ACTION_MODIFIED,
    HUMAN_COLOR_NEUTRAL, HUMAN_COLOR_TOPIC_INDEX, HUMAN_COLOR_TOPIC_PRECISE,
    HUMAN_COLOR_TOPIC_SEMANTIC, HUMAN_COLOR_TOPIC_STORAGE, HUMAN_COLOR_TOPIC_WATCH,
    HUMAN_COLOR_WARN,
};
pub(crate) use index::{emit_index_plan_events, emit_index_progress_event};

const HUMAN_INTRO_ENABLED: bool = true;
static HUMAN_INTRO_EMITTED: AtomicBool = AtomicBool::new(false);

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

    pub(crate) fn tui_enabled(self) -> bool {
        let term_allows_tui = match std::env::var_os("TERM") {
            Some(term) => term.to_string_lossy().as_ref() != "dumb",
            None => true,
        };
        io::stdout().is_terminal() && io::stderr().is_terminal() && term_allows_tui
    }

    pub(crate) fn wants_progress_events(self) -> bool {
        self.is_verbose() || (!self.is_quiet() && self.tui_enabled())
    }

    fn wants_tool_call_events(self) -> bool {
        self.wants_progress_events() && self.tui_enabled()
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
        if self.tui_enabled() {
            let color = human_color_enabled();
            return write_human_stderr_block(
                &format_human_event(level, area, event, fields, path, color),
                color,
            );
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
        if self.tui_enabled() {
            let color = human_color_enabled();
            return write_human_stderr_block(
                &format_human_event(level, area, event, fields, path, color),
                color,
            );
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
        if self.tui_enabled() {
            let color = human_color_enabled();
            return write_human_stderr_block(
                &format_human_event(OutputLevel::Error, area, event, fields, path, color),
                color,
            );
        }
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
        if !self.wants_progress_events() {
            return Ok(());
        }
        if self.tui_enabled() {
            let color = human_color_enabled();
            return write_human_stderr_block(
                &format_human_event(level, area, event, fields, path, color),
                color,
            );
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

    pub(crate) fn tool_call_event(self, event: ToolCallDisplayEvent) -> io::Result<()> {
        if !self.wants_tool_call_events() {
            return Ok(());
        }
        let level = match event.status {
            ToolCallDisplayStatus::Ok => OutputLevel::Ok,
            ToolCallDisplayStatus::Failed => OutputLevel::Error,
        };
        let mut fields = vec![
            field("status", event.status.as_str()),
            field("session", event.session_id),
            field("tool", event.tool_name),
            field("duration_ms", event.duration_ms),
        ];
        if let Some(percent) = format_context_saved_percent(event.context_saved_percent) {
            fields.push(field("context_saved_percent", percent));
        }
        let color = human_color_enabled();
        write_human_stderr_block(
            &format_human_event(level, "tool", "call", &fields, None, color),
            color,
        )
    }
}

pub(crate) fn format_context_saved_percent(percent: Option<f64>) -> Option<String> {
    let percent = percent?;
    if !percent.is_finite() {
        return None;
    }
    let rounded = (percent * 100.0).round() / 100.0;
    let mut value = format!("{rounded:.2}");
    while value.contains('.') && value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    value.push('%');
    Some(value)
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

fn write_human_stderr_block(message: &str, color: bool) -> io::Result<()> {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    if HUMAN_INTRO_ENABLED && !HUMAN_INTRO_EMITTED.swap(true, Ordering::Relaxed) {
        handle.write_all(format_human_intro(color).as_bytes())?;
    }
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
    use frigg::human_output::HumanRow;
    use frigg::indexer::{
        IndexMode, IndexProgressEvent, IndexProgressPhase, IndexProgressStatus, SemanticRefreshMode,
    };

    use super::format_context_saved_percent;
    use super::{
        CliOutput, HUMAN_COLOR_ACTION_CREATED, HUMAN_COLOR_ACTION_DELETED,
        HUMAN_COLOR_ACTION_MODIFIED, HUMAN_COLOR_NEUTRAL, HUMAN_COLOR_TOPIC_INDEX,
        HUMAN_COLOR_TOPIC_PRECISE, HUMAN_COLOR_TOPIC_SEMANTIC, HUMAN_COLOR_TOPIC_STORAGE,
        HUMAN_COLOR_TOPIC_WATCH, HUMAN_COLOR_WARN, OutputLevel, OutputMode, field,
        format_event_line, format_human_event, format_human_event_with_width, format_human_intro,
    };
    use super::{human_block, human_topic};

    #[test]
    fn output_mode_resolves_quiet() {
        assert_eq!(
            OutputMode::resolve(true, false).expect("quiet mode should resolve"),
            OutputMode::Quiet
        );
    }

    #[test]
    fn output_mode_resolves_normal() {
        assert_eq!(
            OutputMode::resolve(false, false).expect("normal mode should resolve"),
            OutputMode::Normal
        );
    }

    #[test]
    fn output_mode_resolves_verbose() {
        assert_eq!(
            OutputMode::resolve(false, true).expect("verbose mode should resolve"),
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

    #[test]
    fn human_summary_card_renders_component_shape() {
        let output = format_human_event(
            OutputLevel::Ok,
            "index",
            "complete",
            &[
                field("status", "ok"),
                field("repos", 1),
                field("duration_ms", 42),
            ],
            None,
            false,
        );

        assert!(output.contains("╭─● Index complete"));
        assert!(output.contains("repos"));
        assert!(output.contains("42ms"));
        assert!(output.contains("╰─╮ duration"));
        assert!(!output.contains("╰ Index complete"));
    }

    #[test]
    fn human_intro_renders_line_ui_card_without_blank_tail() {
        let output = format_human_intro(false);
        let lines = output.lines().collect::<Vec<_>>();

        assert!(!output.starts_with('\n'));
        assert!(output.ends_with('\n'));
        assert!(!output.ends_with("\n\n"));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "    ╭─◆ Frigg");
        assert_eq!(
            lines[1],
            "    ╰─╮ Local, source-backed code search and navigation for AI agents."
        );
    }

    #[test]
    fn human_intro_uses_theme_color_when_enabled() {
        let output = format_human_intro(true);

        assert!(output.contains(&format!("\u{1b}[{HUMAN_COLOR_NEUTRAL}m╭─◆ Frigg\u{1b}[0m")));
        assert!(output.contains(&format!("\u{1b}[{HUMAN_COLOR_NEUTRAL}m╰─╮ \u{1b}[0m")));
        assert!(!output.contains("\u{1b}[2m╰─╮ \u{1b}[0m"));
        assert!(output.contains(
            "\u{1b}[2mLocal, source-backed code search and navigation for AI agents.\u{1b}[0m"
        ));
        assert!(output.contains("\u{1b}[0m"));
    }

    #[test]
    fn human_row_block_trims_empty_edges_and_drops_separators() {
        let output = human_block::format_human_kv_block_with_marker(
            "Test block",
            vec![
                HumanRow::Separator,
                HumanRow::Separator,
                HumanRow::kv("alpha", "one"),
                HumanRow::Separator,
                HumanRow::kv("beta", "two"),
                HumanRow::Separator,
            ],
            "○",
            "1;37",
            "1;37",
            false,
            80,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("╭─○ Test block"));
        assert!(lines[1].starts_with("│   alpha"));
        assert!(lines[2].starts_with("╰─╮ beta"));
        assert!(!output.contains("\n\n"));
    }

    #[test]
    fn human_topic_colors_are_enum_backed_with_title_fallback() {
        assert_eq!(
            human_topic::HumanTopic::Semantic.color(),
            HUMAN_COLOR_TOPIC_SEMANTIC
        );
        assert_eq!(
            human_topic::HumanTopic::Precise.color(),
            HUMAN_COLOR_TOPIC_PRECISE
        );
        assert_eq!(
            human_topic::human_title_topic("Semantic refresh"),
            Some(human_topic::HumanTopic::Semantic)
        );
        assert_eq!(
            human_topic::human_title_topic("Watch Refresh"),
            Some(human_topic::HumanTopic::Watch)
        );
    }

    #[test]
    fn human_error_card_renders_next_step() {
        let output = format_human_event(
            OutputLevel::Error,
            "startup",
            "failed",
            &[
                field("status", "failed"),
                field("next", "run `frigg init`"),
                field("error", "storage db file is missing"),
            ],
            Some("/repo"),
            false,
        );

        assert!(output.contains("╭─× Frigg error"));
        assert!(output.contains("run `frigg init`"));
        assert!(output.contains("storage db file is missing"));
        assert!(output.contains("/repo"));
    }

    #[test]
    fn human_watch_row_renders_path_tail() {
        let output = format_human_event(
            OutputLevel::Info,
            "watch",
            "refresh",
            &[
                field("status", "queued"),
                field("repo", "repo-001"),
                field("debounce_ms", 250),
            ],
            Some("/repo/src/main.rs"),
            false,
        );

        assert!(output.contains("╭─○ Watch Queued"));
        assert!(output.contains("debounce"));
        assert!(output.contains("250ms"));
        assert!(!output.contains("debounce="));
        assert!(!output.starts_with('\n'));
        assert!(!output.ends_with('\n'));
        let path_line = output
            .lines()
            .find(|line| line.contains("/repo/src/main.rs"))
            .expect("watch path should render as a kv row");
        assert!(path_line.starts_with("    ╰─╮ "));
        assert_eq!(char_column(path_line, "path"), Some(8));
        assert!(!output.contains("└─ path: /repo/src/main.rs"));
    }

    #[test]
    fn human_index_plan_uses_vertical_component_rows() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "plan",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("mode", "full"),
                field("snapshot_plan", "persist_new"),
                field("previous", "-"),
                field(
                    "next",
                    "snapshot-32bae36a07d84d9807e8a2486814c9561eed4abc55",
                ),
                field("semantic", "full_rebuild"),
                field("provider", "local"),
                field("semantic_records", 4),
                field("changed", 3),
                field("deleted", 0),
            ],
            None,
            false,
            80,
        );

        assert!(output.contains("○ Index plan"));
        assert!(output.contains("repo"));
        assert!(output.contains("snapshot"));
        assert!(output.contains("full rebuild · local · 4 records"));
        assert!(!output.contains("\n\n"));
        assert!(!output.contains("repo="));
        assert!(!output.contains("mode="));
    }

    #[test]
    fn human_path_list_components_use_dot_without_sideline() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "semantic_paths",
            &[
                field("status", "truncated"),
                field("repo", "repo-001"),
                field("changed", 50),
                field("deleted", 0),
                field("action", "modified"),
                field("shown", 50),
                field("omitted", 139),
            ],
            None,
            false,
            80,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert!(lines[0].starts_with("SRV ╭─○ Semantic paths"));
        assert_eq!(char_column(lines[0], "○"), Some(6));
        assert_eq!(char_column(lines[0], "Semantic paths"), Some(8));
        assert!(lines[1].starts_with("    │   repo"));
        assert!(
            lines
                .iter()
                .enumerate()
                .filter(|(_, line)| !line.is_empty())
                .all(|(index, line)| if index == 0 {
                    line.starts_with("SRV ")
                } else {
                    line.starts_with("    ")
                })
        );
        assert!(
            lines
                .last()
                .is_some_and(|line| line.starts_with("    ╰─╮ "))
        );
        assert!(!output.contains("\n\n"));
        assert!(!output.starts_with('\n'));
        assert!(!output.ends_with('\n'));
        assert!(output.contains("path list truncated"));
    }

    #[test]
    fn human_metadata_components_use_side_rail_block() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "startup",
            "semantic_model",
            &[
                field("status", "ok"),
                field("provider", "local"),
                field("model", "all-MiniLM-L6-v2"),
            ],
            None,
            false,
            80,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert!(lines[0].starts_with("SRV ╭─○ Semantic model"));
        assert!(lines[1].starts_with("    │   provider"));
        assert!(lines[2].starts_with("    ╰─╮ model"));
        assert_eq!(char_column(lines[0], "○"), Some(6));
        assert_eq!(char_column(lines[2], "╮"), Some(6));
        assert_eq!(char_column(lines[0], "Semantic model"), Some(8));
        assert_eq!(char_column(lines[1], "provider"), Some(8));
    }

    #[test]
    fn human_card_corner_keeps_topic_color_under_warning_status() {
        let output = format_human_event_with_width(
            OutputLevel::Warn,
            "startup",
            "semantic_model",
            &[
                field("status", "retry"),
                field("provider", "local"),
                field("model", "all-MiniLM-L6-v2"),
            ],
            None,
            true,
            80,
        );

        assert!(output.starts_with("\u{1b}[1;97;48;5;240mSRV\u{1b}[0m "));
        assert!(output.contains(&format!(
            "\u{1b}[{HUMAN_COLOR_WARN}m╭─▲ Semantic model\u{1b}[0m"
        )));
        assert!(output.contains(&format!(
            "\u{1b}[{HUMAN_COLOR_TOPIC_SEMANTIC}m╰─╮ \u{1b}[0m"
        )));
        assert!(!output.contains(&format!("\u{1b}[{HUMAN_COLOR_WARN}m╰─╮ \u{1b}[0m")));
    }

    #[test]
    fn human_precise_plan_uses_reason_note_not_key_value_tail() {
        let output = format_human_event_with_width(
            OutputLevel::Skip,
            "precise",
            "plan",
            &[
                field("status", "empty"),
                field("repo", "repo-001"),
                field("command", "index"),
                field("reason", "no_generators_need_refresh"),
                field("changed", 0),
                field("deleted", 0),
            ],
            Some("."),
            false,
            80,
        );

        assert!(output.contains("○ Precise plan"));
        assert!(output.contains("no generators need refresh"));
        assert!(!output.contains("reason="));
        assert!(output.contains("workspace current directory (.)"));
        assert!(!output.contains("└─ workspace: current directory (.)"));
    }

    #[test]
    fn human_index_paths_empty_uses_component_rows() {
        let output = format_human_event_with_width(
            OutputLevel::Skip,
            "index",
            "paths",
            &[
                field("status", "empty"),
                field("repo", "repo-001"),
                field("changed", 0),
                field("deleted", 0),
            ],
            None,
            false,
            80,
        );

        assert!(output.contains("○ Index paths"));
        assert!(output.contains("no path changes"));
        assert!(!output.contains("repo="));
        assert!(!output.contains("changed="));
    }

    #[test]
    fn human_semantic_component_renders_refresh_story() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "semantic",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("mode", "incremental_advance"),
                field("provider", "local"),
                field("model", "all-MiniLM-L6-v2"),
                field("records", 2),
                field("changed", 2),
                field("deleted", 0),
            ],
            None,
            false,
            100,
        );

        assert!(output.starts_with("SRV ╭─○ Semantic refresh"));
        assert!(output.contains("incremental advance"));
        assert!(output.contains("local · all-MiniLM-L6-v2"));
        assert!(output.contains("2 changed · 0 deleted"));
        assert!(!output.contains("Semantic index"));
    }

    #[test]
    fn human_index_plan_preserves_semantic_mode_field_wording() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "plan",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("mode", "changed"),
                field("snapshot_plan", "persist_new"),
                field("previous", "snapshot-old"),
                field("next", "snapshot-new"),
                field("semantic", "full_rebuild_from_changed_only"),
                field("provider", "local"),
                field("semantic_records", 4),
                field("changed", 3),
                field("deleted", 0),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("full rebuild from changed only · local · 4 records"));
        assert!(!output.contains("changed-only rebuild · local"));
    }

    #[test]
    fn human_index_phase_rows_describe_current_work_without_key_value_noise() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "phase",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("phase", "build_manifest"),
                field("mode", "full"),
                field("scanned", 477),
                field("changed", 477),
                field("deleted", 2),
            ],
            None,
            false,
            80,
        );

        assert!(output.starts_with("SRV   │ Walking files..."));
        assert!(output.contains("477 scanned"));
        assert!(!output.contains("477 changed · 2 deleted"));
        assert!(!output.contains("phase="));
        assert!(!output.contains("status="));
    }

    #[test]
    fn human_storage_checkpoint_phase_avoids_duplicate_snapshot_wording() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "phase",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("phase", "checkpoint_wal"),
                field("mode", "changed"),
                field("snapshot", "snapshot-218c693c9fbc6c2d2de5"),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("│ Saving storage checkpoint..."));
        assert!(output.contains("manifest checkpoint snapshot-218"));
        assert!(!output.contains("snapshot snapshot"));
    }

    #[test]
    fn human_semantic_phase_completion_shows_duration_per_doc() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "index",
            "phase",
            &[
                field("status", "ok"),
                field("repo", "repo-001"),
                field("phase", "semantic_refresh"),
                field("mode", "changed"),
                field("records", 4),
                field("changed", 4),
                field("deleted", 0),
                field("duration_ms", 42),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("● Semantic chunks stored"));
        assert!(output.contains("Semantic chunks stored  42ms · 4 records"));
        assert!(output.contains("4 records"));
        assert!(output.contains("42ms"));
        assert!(output.contains("10.5ms/doc"));
        assert!(!output.contains("phase="));
    }

    #[test]
    fn human_index_progress_event_starts_gray_detail_with_duration() {
        let event = IndexProgressEvent::new(
            "repo-001",
            IndexMode::ChangedOnly,
            IndexProgressPhase::SemanticRefresh,
            IndexProgressStatus::Ok,
        )
        .with_semantic_refresh_mode(SemanticRefreshMode::IncrementalAdvance)
        .with_records(4)
        .with_path_counts(4, 0)
        .with_duration_ms(42);
        let output = super::index::format_human_index_progress_event(&event, &[], false, 100);

        assert!(output.starts_with("SRV   ● Semantic chunks stored"));
        assert!(output.contains("Semantic chunks stored  42ms · 4 records"));
        assert!(output.contains("10.5ms/doc"));
    }

    #[test]
    fn human_semantic_phase_completion_uses_semantic_delta() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "index",
            "phase",
            &[
                field("status", "ok"),
                field("repo", "repo-001"),
                field("phase", "semantic_refresh"),
                field("mode", "changed"),
                field("records", 1),
                field("changed", 0),
                field("deleted", 0),
                field("changed_paths", 1),
                field("deleted_paths", 0),
                field("duration_ms", 8),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("● Semantic chunks stored"));
        assert!(output.contains("1 records"));
        assert!(output.contains("1 changed · 0 deleted"));
        assert!(output.contains("8ms/doc"));
    }

    #[test]
    fn human_semantic_phase_full_rebuild_omits_zero_path_delta() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "phase",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("phase", "semantic_refresh"),
                field("mode", "changed"),
                field("semantic", "full_rebuild_from_changed_only"),
                field("records", 477),
                field("changed", 477),
                field("deleted", 0),
                field("changed_paths", 0),
                field("deleted_paths", 0),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("│ Rebuilding semantic chunks"));
        assert!(output.contains("477 records"));
        assert!(output.contains("changed-only rebuild"));
        assert!(!output.contains("0 changed · 0 deleted"));
    }

    #[test]
    fn human_semantic_phase_reuse_existing_describes_current_snapshot() {
        let output = format_human_event_with_width(
            OutputLevel::Skip,
            "index",
            "phase",
            &[
                field("status", "skipped"),
                field("repo", "repo-001"),
                field("phase", "semantic_refresh"),
                field("mode", "changed"),
                field("semantic", "reuse_existing"),
                field("records", 0),
                field("changed", 0),
                field("deleted", 0),
                field("changed_paths", 0),
                field("deleted_paths", 0),
                field("duration_ms", 0),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("○ Semantic snapshot current"));
        assert!(!output.contains("Semantic refresh skipped"));
        assert!(!output.contains("0 changed · 0 deleted"));
    }

    #[test]
    fn human_path_rows_color_action_keywords() {
        let modified = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "path",
            &[field("action", "modified"), field("repo", "repo-001")],
            Some("src/lib.rs"),
            true,
            80,
        );
        let deleted = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "path",
            &[field("action", "deleted"), field("repo", "repo-001")],
            Some("src/old.rs"),
            true,
            80,
        );
        let created = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "path",
            &[field("action", "created"), field("repo", "repo-001")],
            Some("src/new.rs"),
            true,
            80,
        );

        assert!(modified.contains(&format!(
            "\u{1b}[{HUMAN_COLOR_ACTION_MODIFIED}mModified\u{1b}[0m"
        )));
        assert!(deleted.contains(&format!(
            "\u{1b}[{HUMAN_COLOR_ACTION_DELETED}mDeleted\u{1b}[0m"
        )));
        assert!(created.contains(&format!(
            "\u{1b}[{HUMAN_COLOR_ACTION_CREATED}mCreated\u{1b}[0m"
        )));
    }

    #[test]
    fn human_topic_colors_are_distinct_from_action_green() {
        let semantic = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "semantic",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("mode", "incremental_advance"),
                field("provider", "local"),
                field("model", "all-MiniLM-L6-v2"),
                field("records", 2),
                field("changed", 2),
                field("deleted", 0),
            ],
            None,
            true,
            100,
        );
        let precise = format_human_event_with_width(
            OutputLevel::Info,
            "precise",
            "generator",
            &[
                field("status", "starting"),
                field("generator", "rust"),
                field("language", "rust"),
                field("tool", "rust-analyzer"),
            ],
            None,
            true,
            100,
        );
        let storage = format_human_event_with_width(
            OutputLevel::Ok,
            "index",
            "phase",
            &[
                field("status", "ok"),
                field("phase", "prune_manifest_snapshots"),
                field("pruned_snapshots", 1),
            ],
            None,
            true,
            100,
        );
        let index = format_human_event_with_width(
            OutputLevel::Skip,
            "index",
            "phase",
            &[
                field("status", "skipped"),
                field("phase", "load_manifest"),
                field("previous", "snapshot-218c693c9fbc6c2d2de5"),
            ],
            None,
            true,
            100,
        );
        let precise_skip = format_human_event_with_width(
            OutputLevel::Skip,
            "precise",
            "plan",
            &[
                field("status", "empty"),
                field("reason", "no_generators_need_refresh"),
            ],
            None,
            true,
            100,
        );
        let watch_skip = format_human_event_with_width(
            OutputLevel::Skip,
            "watch",
            "refresh",
            &[field("status", "skipped"), field("repo", "repo-001")],
            None,
            true,
            100,
        );

        assert!(semantic.contains(&format!("\u{1b}[{HUMAN_COLOR_TOPIC_SEMANTIC}m")));
        assert!(precise.contains(&format!("\u{1b}[{HUMAN_COLOR_TOPIC_PRECISE}m")));
        assert!(storage.contains(&format!("\u{1b}[{HUMAN_COLOR_TOPIC_STORAGE}m")));
        assert!(index.contains(&format!("\u{1b}[{HUMAN_COLOR_TOPIC_INDEX}m○")));
        assert!(precise_skip.contains(&format!("\u{1b}[{HUMAN_COLOR_TOPIC_PRECISE}m╭─○")));
        assert!(watch_skip.contains(&format!("\u{1b}[{HUMAN_COLOR_TOPIC_WATCH}m╭─○")));
        assert!(!semantic.contains(&format!("\u{1b}[{HUMAN_COLOR_ACTION_CREATED}m")));
        assert!(!precise.contains(&format!("\u{1b}[{HUMAN_COLOR_ACTION_CREATED}m")));
        assert!(!storage.contains(&format!("\u{1b}[{HUMAN_COLOR_ACTION_CREATED}m")));
    }

    #[test]
    fn human_semantic_path_rows_color_only_action_keyword() {
        let plain = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "semantic_path",
            &[field("action", "modified"), field("repo", "repo-001")],
            Some("src/lib.rs"),
            false,
            80,
        );
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "semantic_path",
            &[field("action", "modified"), field("repo", "repo-001")],
            Some("src/lib.rs"),
            true,
            80,
        );

        assert!(plain.starts_with("SRV   ● Semantic embedding Modified src/lib.rs"));
        assert!(output.contains("Semantic embedding"));
        assert!(output.contains("\u{1b}[1;33mModified\u{1b}[0m src/lib.rs"));
        assert!(output.contains("semantic embedding input"));
        assert!(!output.contains("\u{1b}[1;33mSemantic embedding Modified\u{1b}[0m"));
    }

    #[test]
    fn human_precise_generator_expands_compact_detail_rows() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "precise",
            "generator",
            &[
                field("status", "ok"),
                field("repo", "repo-001"),
                field("generator", "rust"),
                field("language", "rust"),
                field("tool", "rust-analyzer"),
                field("artifacts", 1),
                field("bytes", 26067716),
                field("duration_ms", 18820),
                field(
                    "detail",
                    "generator=rust tool=rust-analyzer version=rust-analyzer 1.96.0 (ac68faa2 2026-05-25)",
                ),
            ],
            Some("/repo/.frigg/scip/rust.scip"),
            false,
            120,
        );

        assert!(output.contains("● Precise rust"));
        assert!(output.contains("generator rust"));
        assert!(output.contains("tool      rust-analyzer"));
        assert!(output.contains("version   rust-analyzer 1.96.0"));
        assert!(!output.contains("detail    generator=rust"));
        assert!(output.contains("path      /repo/.frigg/scip/rust.scip"));
        assert!(!output.contains("└─ path: /repo/.frigg/scip/rust.scip"));
    }

    #[test]
    fn human_precise_generator_prefers_structured_detail_fields() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "precise",
            "generator",
            &[
                field("status", "ok"),
                field("repo", "repo-001"),
                field("generator", "rust"),
                field("language", "rust"),
                field("tool", "rust-analyzer"),
                field("artifacts", 1),
                field("bytes", 26067716),
                field("duration_ms", 18820),
                field("version", "rust-analyzer 1.96.0 (ac68faa2 2026-05-25)"),
                field(
                    "detail",
                    "generator=rust tool=rust-analyzer version=legacy string",
                ),
            ],
            Some("/repo/.frigg/scip/rust.scip"),
            false,
            120,
        );

        assert!(output.contains("version   rust-analyzer 1.96.0"));
        assert!(!output.contains("legacy string"));
        assert!(!output.contains("detail    generator=rust"));
    }

    #[test]
    fn human_precise_generator_failure_keeps_detail_with_failure_class() {
        let output = format_human_event_with_width(
            OutputLevel::Warn,
            "precise",
            "generator",
            &[
                field("status", "failed"),
                field("repo", "repo-001"),
                field("generator", "rust"),
                field("language", "rust"),
                field("tool", "rust-analyzer"),
                field("artifacts", 0),
                field("bytes", 0),
                field("duration_ms", 0),
                field("failure_class", "missing_tool"),
                field("next", "install_tool"),
                field(
                    "detail",
                    "precise generator tool 'rust-analyzer' is not installed",
                ),
            ],
            Some("/repo/.frigg/scip/rust.scip"),
            false,
            120,
        );

        assert!(output.contains("precise generator tool 'rust-analyzer' is not installed"));
        assert!(output.contains("next      install_tool"));
    }

    #[test]
    fn human_precise_generator_start_renders_progress_line_only() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "precise",
            "generator",
            &[
                field("status", "starting"),
                field("repo", "repo-001"),
                field("command", "index"),
                field("generator", "rust"),
                field("language", "rust"),
                field("tool", "rust-analyzer"),
            ],
            None,
            false,
            100,
        );

        assert!(output.starts_with("SRV   │ Running rust-analyzer..."));
        assert!(output.contains("rust generator"));
        assert_eq!(output.lines().count(), 1);
        assert!(!output.contains("language"));
        assert!(!output.contains("tool"));
    }

    #[test]
    fn human_start_events_use_circle_message_with_context_rows() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "serve",
            "start",
            &[field("status", "starting"), field("transport", "stdio")],
            None,
            false,
            72,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert!(lines[0].starts_with("SRV ╭─○ Serve starting"));
        assert_eq!(char_column(lines[0], "○"), Some(6));
        assert_eq!(char_column(lines[0], "Serve starting"), Some(8));
        assert_eq!(char_column(lines[1], "transport"), Some(8));
        assert!(lines[1].starts_with("    ╰─╮ "));
        assert!(!output.starts_with('\n'));
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn human_card_short_rows_use_content_sized_label_column() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "index",
            "complete",
            &[
                field("status", "ok"),
                field("mode", "changed"),
                field("repos", 1),
            ],
            None,
            false,
            100,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert_eq!(char_column(lines[1], "mode"), Some(8));
        assert_eq!(char_column(lines[1], "changed"), Some(14));
        assert_eq!(char_column(lines[2], "repos"), Some(8));
        assert_eq!(char_column(lines[2], "1"), Some(14));
        assert!(lines[2].starts_with("    ╰─╮ "));
        assert!(!output.starts_with('\n'));
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn human_card_values_share_column_for_long_labels() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "custom",
            "complete",
            &[
                field("status", "ok"),
                field("repos", 1),
                field("precise_generators", 0),
                field("precise_missing_tool", 0),
                field("precise_skipped", 0),
            ],
            None,
            false,
            80,
        );
        let lines = output.lines().collect::<Vec<_>>();

        let repos_value_column = char_column(lines[1], "1");
        let generators_value_column = char_column(lines[2], "0");
        let missing_tool_value_column = char_column(lines[3], "0");
        let skipped_value_column = char_column(lines[4], "0");

        assert_eq!(repos_value_column, generators_value_column);
        assert_eq!(repos_value_column, missing_tool_value_column);
        assert_eq!(repos_value_column, skipped_value_column);
        assert!(lines[4].starts_with("    ╰─╮ "));
        assert!(!output.starts_with('\n'));
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn human_index_complete_groups_related_metrics() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "index",
            "complete",
            &[
                field("status", "ok"),
                field("mode", "changed"),
                field("repos", 1),
                field("scanned", 10),
                field("changed", 2),
                field("deleted", 1),
                field("diagnostics", 0),
                field("diagnostics_walk", 0),
                field("diagnostics_read", 0),
                field("precise_generators", 1),
                field("precise_succeeded", 1),
                field("precise_failed", 0),
                field("precise_missing_tool", 0),
                field("precise_skipped", 0),
                field("duration_ms", 25),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("files"));
        assert!(output.contains("10 scanned · 2 changed · 1 deleted"));
        assert!(output.contains("diagnostics"));
        assert!(output.contains("0 total · 0 walk · 0 read"));
        assert!(output.contains("precise"));
        assert!(output.contains("1 generators · 1 ok"));
        assert!(!output.contains("diagnostics walk"));
        assert!(!output.contains("precise generators"));
    }

    #[test]
    fn human_tool_call_uses_diamond_and_metadata_without_equals() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "tool",
            "call",
            &[
                field("status", "ok"),
                field("tool", "search_hybrid"),
                field("duration_ms", 1008),
                field("context_saved_percent", "100%"),
            ],
            None,
            false,
            72,
        );

        assert_eq!(output, "SRV   ◇ search_hybrid  1008ms · 100% saved");
        assert!(!output.contains('='));

        let colored = format_human_event_with_width(
            OutputLevel::Ok,
            "tool",
            "call",
            &[
                field("status", "ok"),
                field("tool", "search_hybrid"),
                field("duration_ms", 1008),
            ],
            None,
            true,
            72,
        );
        assert!(colored.contains(&format!("\x1b[{HUMAN_COLOR_NEUTRAL}msearch_hybrid\x1b[0m")));
        assert!(!colored.contains(HUMAN_COLOR_TOPIC_INDEX));
    }

    #[test]
    fn human_compact_activity_details_promote_ms_fields() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "custom",
            "checkpoint",
            &[
                field("status", "ok"),
                field("repo", "repo-001"),
                field("records", 2),
                field("duration_ms", 7),
                field("class", "fast"),
            ],
            None,
            false,
            100,
        );

        assert!(output.contains("Custom Checkpoint  duration=7ms repo=repo-001 records=2"));
    }

    #[test]
    fn human_tool_call_uses_session_badge_when_present() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "tool",
            "call",
            &[
                field("status", "ok"),
                field("session", "connected-session-abcdef"),
                field("tool", "search_hybrid"),
                field("duration_ms", 1008),
            ],
            None,
            false,
            72,
        );

        assert_eq!(output, "def   ◇ search_hybrid  1008ms");
        assert!(!output.contains("session="));

        let colored = format_human_event_with_width(
            OutputLevel::Ok,
            "tool",
            "call",
            &[
                field("status", "ok"),
                field("session", "connected-session-abcdef"),
                field("tool", "search_hybrid"),
                field("duration_ms", 1008),
            ],
            None,
            true,
            72,
        );

        assert!(colored.starts_with("\x1b[1;97;48;5;240mdef\x1b[0m "));
    }

    #[test]
    fn human_component_session_badge_only_decorates_first_line() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "startup",
            "semantic_model",
            &[
                field("status", "ok"),
                field("session", "connected-session-abcde"),
                field("provider", "local"),
                field("model", "all-MiniLM-L6-v2"),
            ],
            None,
            false,
            80,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert!(lines[0].starts_with("cde ╭─○ Semantic model"));
        assert!(lines[1].starts_with("│   provider"));
        assert!(lines[2].starts_with("╰─╮ model"));
        assert!(lines[1..].iter().all(|line| !line.contains("cde")));
        assert!(!output.contains("session="));
    }

    #[test]
    fn human_card_session_badge_only_decorates_first_line() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "init",
            "complete",
            &[
                field("status", "ok"),
                field("session", "connected-session-xyz123"),
                field("repos", 1),
            ],
            None,
            false,
            80,
        );
        let lines = output.lines().collect::<Vec<_>>();

        assert!(lines[0].starts_with("123 ╭─● Init complete"));
        assert!(lines[1].starts_with("╰─╮ repos"));
        assert!(lines[1..].iter().all(|line| !line.contains("123")));
        assert!(!output.contains("session="));
    }

    #[test]
    fn tool_call_events_follow_quiet_progress_gate() {
        assert!(!CliOutput::new(OutputMode::Quiet).wants_tool_call_events());
    }

    #[test]
    fn tool_call_context_saved_percent_trims_trailing_zeroes() {
        assert_eq!(
            format_context_saved_percent(Some(96.4)).as_deref(),
            Some("96.4%")
        );
        assert_eq!(
            format_context_saved_percent(Some(100.0)).as_deref(),
            Some("100%")
        );
        assert_eq!(format_context_saved_percent(Some(f64::NAN)), None);
    }

    #[test]
    fn human_activity_text_starts_at_shared_column_and_does_not_overflow() {
        let output = format_human_event_with_width(
            OutputLevel::Ok,
            "startup",
            "semantic_model",
            &[
                field("status", "ready"),
                field("provider", "local"),
                field("model", "all-MiniLM-L6-v2"),
                field(
                    "cache_key",
                    "local--all-minilm-l6-v2--qdrant-all-minilm-l6-v2-onnx",
                ),
            ],
            Some("/Users/bnomei/Library/Caches/frigg/models"),
            false,
            64,
        );
        let lines = output.lines().collect::<Vec<_>>();
        let path_line = lines
            .iter()
            .find(|line| line.contains("/Users/bnomei/Library/Caches/frigg/models"))
            .expect("semantic model path line should be rendered");

        assert!(lines[0].starts_with("SRV "));
        assert_eq!(char_column(lines[0], "Semantic model"), Some(8));
        assert_eq!(char_column(path_line, "path"), Some(8));
        assert!(!path_line.contains("└─"));
        assert!(lines.iter().all(|line| line.chars().count() <= 64));
    }

    #[test]
    fn human_component_path_expands_current_directory_dot() {
        let output = format_human_event_with_width(
            OutputLevel::Skip,
            "precise",
            "plan",
            &[
                field("status", "empty"),
                field("repo", "repo-001"),
                field("reason", "no_generators_need_refresh"),
            ],
            Some("."),
            false,
            80,
        );

        assert!(output.contains("workspace current directory (.)"));
        assert!(!output.contains("└─ workspace: current directory (.)"));
    }

    fn char_column(line: &str, needle: &str) -> Option<usize> {
        line.find(needle)
            .map(|byte_index| line[..byte_index].chars().count())
    }
}
