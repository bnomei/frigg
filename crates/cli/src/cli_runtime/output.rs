//! Small CLI output policy layer for stable stdout results and stderr diagnostics.

use std::error::Error;
use std::fmt::Display;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use frigg::indexer::{IndexPlan, IndexProgressEvent, IndexProgressStatus};

const MAX_VERBOSE_PATH_LINES: usize = 50;
const HUMAN_DEFAULT_WIDTH: usize = 100;
const HUMAN_MIN_WIDTH: usize = 48;
const HUMAN_TEXT_COLUMN: usize = 4;
const HUMAN_CARD_LABEL_WIDTH: usize = 20;
const HUMAN_CARD_MAX_LABEL_WIDTH: usize = 28;
const HUMAN_CARD_TITLE_PREFIX: &str = "╭─  ";
const HUMAN_CARD_ROW_PREFIX: &str = "│   ";
const HUMAN_CARD_FOOTER_PREFIX: &str = "╰─  ";
const HUMAN_ACTIVITY_PREFIX: &str = "  ";
const HUMAN_DETAIL_PREFIX: &str = "    ";
const HUMAN_CONTINUATION_PREFIX: &str = "    └─ ";
const HUMAN_INTRO_LOGO_LINES: &[&str] = &[
    "█████ ████  ███  ███   ███",
    "█     █   █  █  █     █",
    "████  ████   █  █  ██ █  ██",
    "█     █  █   █  █   █ █   █",
    "█     █   █ ███  ███   ███",
];
const HUMAN_INTRO_COLOR_CODES: &[&str] = &[
    "1;38;2;238;240;242",
    "1;38;2;238;240;242",
    "1;38;2;214;238;234",
    "1;38;2;125;199;190",
    "1;38;2;125;199;190",
];
const HUMAN_INTRO_ENABLED: bool = false;
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

fn format_human_event(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
) -> String {
    format_human_event_with_width(
        level,
        area,
        event,
        fields,
        path,
        color,
        human_terminal_width(),
    )
}

fn format_human_event_with_width(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let width = width.max(HUMAN_MIN_WIDTH);
    if level == OutputLevel::Error {
        return format_human_error_card(area, event, fields, path, color, width);
    }
    if event == "start" {
        return format_human_start_card(area, fields, path, color, width);
    }
    if area == "serve" && event == "http" && field_is(fields, "status", "listening") {
        return format_human_ready_card(fields, color, width);
    }
    if event == "complete" {
        return format_human_complete_card(level, area, fields, path, color, width);
    }
    if area == "startup" && matches!(event, "semantic" | "semantic_model" | "storage") {
        return format_human_startup_component(level, event, fields, path, color, width);
    }
    if area == "index" && event == "plan" {
        return format_human_index_plan_component(level, fields, path, color, width);
    }
    if area == "index" && event == "semantic" {
        return format_human_index_semantic_component(level, fields, path, color, width);
    }
    if area == "index" && event == "phase" {
        return format_human_index_phase_row(level, fields, path, color, width);
    }
    if area == "index" && matches!(event, "paths" | "semantic_paths") {
        return format_human_index_paths_component(level, event, fields, path, color, width);
    }
    if event == "repo" {
        return format_human_repository_component(level, area, fields, path, color, width);
    }
    if area == "precise" && event == "plan" {
        return format_human_precise_plan_component(level, fields, path, color, width);
    }
    if area == "precise" && event == "run" {
        return format_human_precise_run_component(level, fields, path, color, width);
    }
    if area == "index" && matches!(event, "path" | "semantic_path") {
        return format_human_path_row(level, area, event, fields, path, color, width);
    }
    if area == "watch" {
        return format_human_watch_component(level, event, fields, path, color, width);
    }
    if area == "precise" && event == "generator" {
        return format_human_precise_generator_component(level, fields, path, color, width);
    }
    if area == "storage" && event == "auto_repair" {
        return format_human_storage_repair_card(level, fields, path, color, width);
    }
    format_human_activity_row(level, area, event, fields, path, color, width)
}

fn format_human_start_card(
    area: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = human_rows_from_fields(fields, &["status"]);
    if let Some(path) = path {
        rows.push(("path".to_owned(), path.to_owned()));
    }
    format_human_card(
        OutputLevel::Info,
        &format!("Frigg {}", human_title_token(area)),
        rows,
        Some("starting"),
        color,
        width,
    )
}

fn format_human_ready_card(fields: &[OutputField], color: bool, width: usize) -> String {
    let mut rows = Vec::new();
    if let (Some(addr), Some(endpoint)) =
        (field_value(fields, "addr"), field_value(fields, "endpoint"))
    {
        rows.push(("endpoint".to_owned(), format!("http://{addr}{endpoint}")));
    }
    rows.extend(human_rows_from_fields(
        fields,
        &["status", "addr", "endpoint"],
    ));
    format_human_card(
        OutputLevel::Ok,
        "Frigg serve",
        rows,
        Some("ready for MCP connections"),
        color,
        width,
    )
}

fn format_human_complete_card(
    level: OutputLevel,
    area: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let rows = match area {
        "index" => human_index_complete_rows(fields),
        "init" => human_init_complete_rows(fields),
        "repair-storage" => human_repair_complete_rows(fields),
        "prune-storage" => human_prune_complete_rows(fields),
        _ => human_rows_from_fields(fields, &["status"]),
    };
    let mut rows = rows;
    if let Some(path) = path {
        rows.push(("path".to_owned(), path.to_owned()));
    }
    format_human_card(
        level,
        &format!("{} complete", human_title_token(area)),
        rows,
        Some("done"),
        color,
        width,
    )
}

fn format_human_error_card(
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = vec![
        ("area".to_owned(), human_title_token(area)),
        ("event".to_owned(), human_title_token(event)),
    ];
    rows.extend(human_rows_from_fields(fields, &["status"]));
    if let Some(path) = path {
        rows.push(("path".to_owned(), path.to_owned()));
    }
    format_human_card(
        OutputLevel::Error,
        "Frigg error",
        rows,
        Some("command stopped"),
        color,
        width,
    )
}

fn format_human_storage_repair_card(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = human_rows_from_fields(fields, &["status"]);
    if let Some(path) = path {
        rows.push(("path".to_owned(), path.to_owned()));
    }
    format_human_card(level, "Storage auto-repair", rows, None, color, width)
}

fn format_human_startup_component(
    level: OutputLevel,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let title = match event {
        "semantic_model" => "Semantic model",
        "semantic" => "Semantic runtime",
        "storage" => "Storage ready",
        _ => "Startup",
    };
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_field_row(&mut rows, fields, "provider", "provider");
    push_field_row(&mut rows, fields, "model", "model");
    push_field_row(&mut rows, fields, "strict", "strict");
    push_field_row(&mut rows, fields, "backend", "backend");
    push_field_row(&mut rows, fields, "extension_version", "extension");
    push_field_row(&mut rows, fields, "cache_key", "cache");
    push_field_row(&mut rows, fields, "repository", "source");
    push_field_row(&mut rows, fields, "reason", "reason");
    push_field_row(&mut rows, fields, "db", "storage");

    format_human_component(level, fields, title, rows, Vec::new(), path, color, width)
}

fn format_human_index_plan_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_human_row(&mut rows, "mode", field_display(fields, "mode"));

    let snapshot = match (
        field_value(fields, "snapshot_plan"),
        field_value(fields, "previous"),
        field_value(fields, "next"),
    ) {
        (Some(plan), Some(previous), Some(next)) if previous != "-" => Some(format!(
            "{} · {} → {}",
            human_mode_label(plan),
            short_identifier(previous),
            short_identifier(next)
        )),
        (Some(plan), _, Some(next)) => Some(format!(
            "{} · {}",
            human_mode_label(plan),
            short_identifier(next)
        )),
        _ => None,
    };
    push_human_row(&mut rows, "snapshot", snapshot);

    let semantic = field_value(fields, "semantic").map(|semantic| {
        let mut parts = vec![human_mode_label(semantic)];
        if let Some(provider) = field_value(fields, "provider").filter(|value| *value != "-") {
            parts.push(provider.to_owned());
        }
        if let Some(records) = field_value(fields, "semantic_records") {
            parts.push(format!("{records} records"));
        }
        parts.join(" · ")
    });
    push_human_row(&mut rows, "semantic", semantic);
    push_human_row(&mut rows, "delta", human_delta(fields));
    push_field_row(&mut rows, fields, "source", "source");
    push_field_row(&mut rows, fields, "class", "class");

    format_human_component(
        level,
        fields,
        "Index plan",
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

fn format_human_index_semantic_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_human_row(
        &mut rows,
        "mode",
        field_value(fields, "mode").map(human_mode_label),
    );
    push_human_row(&mut rows, "model", human_provider_model(fields));
    push_field_row(&mut rows, fields, "records", "records");
    push_human_row(&mut rows, "delta", human_delta(fields));
    push_field_row(&mut rows, fields, "source", "source");
    push_field_row(&mut rows, fields, "class", "class");

    format_human_component(
        level,
        fields,
        "Semantic index",
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

fn format_human_index_phase_row(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let symbol = human_symbol(level, fields, color);
    let title = human_index_phase_title(fields);
    let detail = human_index_phase_detail(fields);
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN);
    let title_budget = content_width.saturating_sub(2);
    let detail_budget = detail
        .as_ref()
        .map(|detail| display_width(detail).min(content_width / 2))
        .unwrap_or(0);
    let title_budget = title_budget.saturating_sub(detail_budget);
    let title = truncate_display(&title, title_budget);
    let mut line = format!("{HUMAN_ACTIVITY_PREFIX}{symbol} {title}");
    if let Some(detail) = detail {
        let used_width = HUMAN_TEXT_COLUMN + display_width(&title);
        let remaining = width.saturating_sub(used_width + 2);
        if remaining >= 8 {
            line.push_str("  ");
            line.push_str(&colorize(color, "2", truncate_display(&detail, remaining)));
        }
    }
    if let Some(path) = path {
        let detail = format_human_continuation(path, color, width);
        line.push('\n');
        line.push_str(&detail);
    }
    line
}

fn human_index_phase_title(fields: &[OutputField]) -> String {
    let phase = field_value(fields, "phase").unwrap_or("work");
    let status = field_value(fields, "status").unwrap_or("starting");
    let title = match (phase, status) {
        ("initialize_storage", "starting") => "Opening storage",
        ("initialize_storage", "ok") => "Storage open",
        ("initialize_storage", "skipped") => "Storage already open",
        ("load_manifest", "starting") => "Loading manifest",
        ("load_manifest", "ok") => "Manifest loaded",
        ("load_manifest", "skipped") => "Manifest load skipped",
        ("build_manifest", "starting") => "Walking files",
        ("build_manifest", "ok") => "Files scanned",
        ("build_manifest", "skipped") => "File scan skipped",
        ("build_plan", "starting") => "Planning index",
        ("build_plan", "ok") => "Index planned",
        ("build_plan", "skipped") => "Index plan skipped",
        ("persist_manifest_snapshot", "starting") => "Writing manifest",
        ("persist_manifest_snapshot", "ok") => "Manifest written",
        ("persist_manifest_snapshot", "skipped") => "Manifest unchanged",
        ("refresh_retrieval_projections", "starting") => "Refreshing search projections",
        ("refresh_retrieval_projections", "ok") => "Search projections updated",
        ("refresh_retrieval_projections", "skipped") => "Search projections current",
        ("semantic_refresh", "starting") => "Embedding semantic chunks",
        ("semantic_refresh", "ok") => "Semantic chunks stored",
        ("semantic_refresh", "skipped") => "Semantic refresh skipped",
        ("prune_manifest_snapshots", "starting") => "Pruning snapshots",
        ("prune_manifest_snapshots", "ok") => "Snapshots pruned",
        ("prune_manifest_snapshots", "skipped") => "Snapshots retained",
        ("checkpoint_wal", "starting") => "Flushing storage",
        ("checkpoint_wal", "ok") => "Storage flushed",
        ("checkpoint_wal", "skipped") => "Storage flush skipped",
        ("checkpoint_wal", "warning") => "Storage flush warning",
        _ => return human_title_token(phase),
    };
    title.to_owned()
}

fn human_index_phase_detail(fields: &[OutputField]) -> Option<String> {
    let phase = field_value(fields, "phase").unwrap_or("work");
    let parts = match phase {
        "load_manifest" => vec![
            field_value(fields, "previous")
                .filter(|value| *value != "-")
                .map(|snapshot| format!("previous {}", short_identifier(snapshot))),
            field_value(fields, "scanned")
                .filter(|value| *value != "0")
                .map(|files| format!("{files} previous files")),
        ],
        "build_manifest" => vec![
            field_value(fields, "scanned").map(|files| format!("{files} scanned")),
            field_value(fields, "diagnostics")
                .filter(|value| *value != "0")
                .map(|diagnostics| format!("{diagnostics} diagnostics")),
        ],
        "build_plan" => vec![
            human_delta(fields),
            semantic_records_detail(fields),
            snapshot_detail(fields),
        ],
        "persist_manifest_snapshot" | "refresh_retrieval_projections" | "checkpoint_wal" => {
            vec![snapshot_detail(fields)]
        }
        "semantic_refresh" => vec![semantic_records_detail(fields), human_delta(fields)],
        "prune_manifest_snapshots" => vec![
            field_value(fields, "pruned_snapshots")
                .filter(|value| *value != "0")
                .map(|pruned| format!("{pruned} removed")),
        ],
        _ => vec![human_file_counts(fields), snapshot_detail(fields)],
    };
    let parts = parts.into_iter().flatten().collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn semantic_records_detail(fields: &[OutputField]) -> Option<String> {
    field_value(fields, "records")
        .filter(|value| *value != "0")
        .map(|records| format!("{records} records"))
}

fn snapshot_detail(fields: &[OutputField]) -> Option<String> {
    field_value(fields, "snapshot")
        .filter(|value| *value != "-")
        .map(|snapshot| format!("snapshot {}", short_identifier(snapshot)))
}

fn format_human_index_paths_component(
    level: OutputLevel,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let title = if event == "semantic_paths" {
        "Semantic paths"
    } else {
        "Index paths"
    };
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_human_row(&mut rows, "delta", human_delta(fields));
    push_field_row(&mut rows, fields, "action", "action");
    push_field_row(&mut rows, fields, "shown", "shown");
    push_field_row(&mut rows, fields, "omitted", "omitted");

    let mut notes = Vec::new();
    if field_is(fields, "status", "empty") {
        notes.push("no path changes".to_owned());
    }
    if field_is(fields, "status", "truncated") {
        notes.push("path list truncated".to_owned());
    }

    format_human_component(level, fields, title, rows, notes, path, color, width)
}

fn format_human_repository_component(
    level: OutputLevel,
    area: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let title = match area {
        "init" => "Init repository",
        "index" => "Index repository",
        "repair-storage" => "Repair repository",
        "prune-storage" => "Prune repository",
        _ => "Repository",
    };
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_field_row(&mut rows, fields, "mode", "mode");
    push_human_row(&mut rows, "files", human_file_counts(fields));
    push_human_row(&mut rows, "diagnostics", human_diagnostics(fields));
    push_human_row(&mut rows, "precise", human_precise_counts(fields));
    push_field_row(&mut rows, fields, "repaired", "repaired");
    push_field_row(&mut rows, fields, "manifest_snapshots_deleted", "deleted");
    push_field_row(&mut rows, fields, "keep_manifest_snapshots", "keep");
    push_human_row(
        &mut rows,
        "snapshot",
        field_value(fields, "snapshot").map(short_identifier),
    );
    push_field_row(&mut rows, fields, "duration_ms", "duration");
    push_field_row(&mut rows, fields, "db", "storage");

    format_human_component(level, fields, title, rows, Vec::new(), path, color, width)
}

fn format_human_precise_plan_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_field_row(&mut rows, fields, "command", "command");
    push_human_row(&mut rows, "delta", human_delta(fields));
    push_human_row(&mut rows, "generators", human_generators(fields));

    let mut notes = Vec::new();
    if field_is(fields, "status", "empty") {
        notes.push("no generators need refresh".to_owned());
    }
    if field_is(fields, "status", "failed") {
        if let Some(error) = field_display(fields, "error") {
            notes.push(error);
        }
    }

    format_human_component(
        level,
        fields,
        "Precise plan",
        rows,
        notes,
        path,
        color,
        width,
    )
}

fn format_human_precise_run_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_field_row(&mut rows, fields, "command", "command");
    push_field_row(&mut rows, fields, "error", "error");

    format_human_component(
        level,
        fields,
        "Precise run",
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

fn format_human_precise_generator_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let generator = field_value(fields, "generator")
        .or_else(|| field_value(fields, "tool"))
        .unwrap_or("generator");
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "language", "language");
    push_field_row(&mut rows, fields, "tool", "tool");
    push_human_row(&mut rows, "output", human_artifact_counts(fields));
    push_field_row(&mut rows, fields, "duration_ms", "duration");
    push_field_row(&mut rows, fields, "next", "next");
    push_field_row(&mut rows, fields, "detail", "detail");

    format_human_component(
        level,
        fields,
        &format!("Precise {generator}"),
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

fn format_human_watch_component(
    level: OutputLevel,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let status = field_value(fields, "status")
        .or_else(|| field_value(fields, "action"))
        .unwrap_or(event);
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_field_row(&mut rows, fields, "mode", "mode");
    push_field_row(&mut rows, fields, "transport", "transport");
    push_field_row(&mut rows, fields, "class", "class");
    push_field_row(&mut rows, fields, "reason", "reason");
    push_field_row(&mut rows, fields, "snapshot", "snapshot");
    push_human_row(&mut rows, "delta", human_file_counts(fields));
    push_field_row(&mut rows, fields, "debounce_ms", "debounce");
    push_field_row(&mut rows, fields, "retry_ms", "retry");
    push_field_row(&mut rows, fields, "error", "error");

    format_human_component(
        level,
        fields,
        &format!("Watch {}", human_title_token(status)),
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

fn format_human_path_row(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let action = field_value(fields, "action").unwrap_or("changed");
    let action_title = human_title_token(action);
    let title_prefix = if event == "semantic_path" {
        format!("Semantic {action_title}")
    } else {
        action_title
    };
    let title_suffix = path.unwrap_or_else(|| human_event_title_static(area, event));
    let detail_skip = &["status", "action", "repo"];
    let details = compact_human_fields(fields, detail_skip, 3);
    format_human_action_line(
        level,
        fields,
        &title_prefix,
        action,
        title_suffix,
        details,
        color,
        width,
    )
}

fn format_human_action_line(
    level: OutputLevel,
    fields: &[OutputField],
    title_prefix: &str,
    action: &str,
    title_suffix: &str,
    details: String,
    color: bool,
    width: usize,
) -> String {
    let symbol = human_symbol(level, fields, color);
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN);
    let title_budget = content_width.saturating_sub(2);
    let prefix_width = display_width(title_prefix);
    let suffix_budget = title_budget.saturating_sub(prefix_width + 1);
    let title_suffix = truncate_display(title_suffix, suffix_budget);
    let title_width = prefix_width + 1 + display_width(&title_suffix);
    let mut line = format!(
        "{HUMAN_ACTIVITY_PREFIX}{symbol} {} {}",
        colorize_action_title(color, action, title_prefix),
        title_suffix
    );
    let used_width = HUMAN_TEXT_COLUMN + title_width;
    let detail_budget = width.saturating_sub(used_width + 2);
    if !details.is_empty() && detail_budget >= 8 {
        line.push_str("  ");
        line.push_str(&colorize(
            color,
            "2",
            truncate_display(&details, detail_budget),
        ));
    }
    line
}

fn colorize_action_title(color: bool, action: &str, title_prefix: &str) -> String {
    let action_title = human_title_token(action);
    match title_prefix.strip_suffix(&action_title) {
        Some(label_prefix) => format!(
            "{label_prefix}{}",
            colorize(color, action_color(action), &action_title)
        ),
        None => colorize(color, action_color(action), title_prefix),
    }
}

fn action_color(action: &str) -> &'static str {
    match action {
        "created" | "create" | "added" | "add" | "new" => "1;32",
        "modified" | "changed" | "updated" | "update" => "1;33",
        "deleted" | "delete" | "removed" | "remove" => "1;31",
        "renamed" | "moved" => "1;35",
        "skipped" | "unchanged" => "2",
        _ => "1;36",
    }
}

fn human_event_title_static(area: &str, event: &str) -> &'static str {
    match (area, event) {
        ("index", "path") => "Index path",
        ("index", "semantic_path") => "Semantic path",
        _ => "Path",
    }
}

fn format_human_activity_row(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut line = format_human_activity_line(
        level,
        fields,
        human_event_title(area, event),
        compact_human_fields(fields, &["status"], 7),
        color,
        width,
    );
    if let Some(path) = path {
        line.push('\n');
        line.push_str(&format_human_continuation(path, color, width));
    }
    line
}

fn format_human_activity_line(
    level: OutputLevel,
    fields: &[OutputField],
    title: String,
    details: String,
    color: bool,
    width: usize,
) -> String {
    let symbol = human_symbol(level, fields, color);
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN);
    let title = truncate_display(&title, content_width);
    let mut line = format!(
        "{HUMAN_ACTIVITY_PREFIX}{symbol} {}",
        colorize(color, title_color(level), &title)
    );
    let used_width = HUMAN_TEXT_COLUMN + display_width(&title);
    let detail_budget = width.saturating_sub(used_width + 2);
    if !details.is_empty() && detail_budget >= 8 {
        line.push_str("  ");
        line.push_str(&colorize(
            color,
            "2",
            truncate_display(&details, detail_budget),
        ));
    }
    line
}

fn format_human_continuation(value: &str, color: bool, width: usize) -> String {
    let value = human_continuation_value(value);
    let budget = width.saturating_sub(display_width(HUMAN_CONTINUATION_PREFIX));
    let value = truncate_display(&value, budget);
    format!("{HUMAN_CONTINUATION_PREFIX}{}", colorize(color, "2", value))
}

fn format_human_component(
    level: OutputLevel,
    fields: &[OutputField],
    title: &str,
    rows: Vec<(String, String)>,
    notes: Vec<String>,
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut output =
        format_human_activity_line(level, fields, title.to_owned(), String::new(), color, width);
    let label_width = human_component_label_width(&rows, width);
    for (label, value) in rows {
        output.push('\n');
        output.push_str(&format_human_detail_row(
            &label,
            &value,
            label_width,
            color,
            width,
        ));
    }
    for note in notes {
        output.push('\n');
        output.push_str(&format_human_note_row(&note, color, width));
    }
    if let Some(path) = path {
        output.push('\n');
        output.push_str(&format_human_continuation(path, color, width));
    }
    output
}

fn format_human_detail_row(
    label: &str,
    value: &str,
    label_width: usize,
    color: bool,
    width: usize,
) -> String {
    let label = truncate_display(label, label_width);
    let value_prefix_width = display_width(HUMAN_DETAIL_PREFIX) + label_width + 1;
    let value = truncate_display(value, width.saturating_sub(value_prefix_width));
    format!(
        "{HUMAN_DETAIL_PREFIX}{} {}",
        colorize(color, "2", format!("{label:<label_width$}")),
        value
    )
}

fn format_human_note_row(note: &str, color: bool, width: usize) -> String {
    let budget = width.saturating_sub(display_width(HUMAN_DETAIL_PREFIX));
    format!(
        "{HUMAN_DETAIL_PREFIX}{}",
        colorize(color, "2", truncate_display(note, budget))
    )
}

fn format_human_card(
    level: OutputLevel,
    title: &str,
    rows: Vec<(String, String)>,
    footer: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut output = String::new();
    let title = truncate_display(title, width.saturating_sub(HUMAN_TEXT_COLUMN));
    output.push_str(&colorize(
        color,
        title_color(level),
        format!("{HUMAN_CARD_TITLE_PREFIX}{title}"),
    ));
    output.push('\n');
    let label_width = human_card_label_width(&rows, width);
    for (label, value) in rows {
        let label = truncate_display(&label, label_width);
        let value_prefix_width = display_width(HUMAN_CARD_ROW_PREFIX) + label_width + 1;
        let value = truncate_display(&value, width.saturating_sub(value_prefix_width));
        output.push_str(&colorize(color, "2", HUMAN_CARD_ROW_PREFIX));
        output.push_str(&colorize(color, "2", format!("{label:<label_width$}")));
        output.push(' ');
        output.push_str(&value);
        output.push('\n');
    }
    let footer = footer
        .map(|footer| {
            format!(
                "{HUMAN_CARD_FOOTER_PREFIX}{}",
                truncate_display(footer, width.saturating_sub(HUMAN_TEXT_COLUMN))
            )
        })
        .unwrap_or_else(|| "╰─".to_owned());
    output.push_str(&colorize(color, title_color(level), footer));
    output
}

fn push_field_row(
    rows: &mut Vec<(String, String)>,
    fields: &[OutputField],
    key: &str,
    label: &str,
) {
    push_human_row(rows, label, field_display(fields, key));
}

fn push_human_row(rows: &mut Vec<(String, String)>, label: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty() && value != "-") {
        rows.push((label.to_owned(), value));
    }
}

fn human_continuation_value(value: &str) -> String {
    match value.trim() {
        "." | "./" => "workspace: current directory (.)".to_owned(),
        trimmed if trimmed.is_empty() => "path: -".to_owned(),
        _ => format!("path: {value}"),
    }
}

fn human_component_label_width(rows: &[(String, String)], width: usize) -> usize {
    let longest = rows
        .iter()
        .map(|(label, _)| display_width(label))
        .max()
        .unwrap_or(10);
    let max_for_value_column = width
        .saturating_sub(display_width(HUMAN_DETAIL_PREFIX) + 1 + 8)
        .max(1);
    longest.min(16).min(max_for_value_column).max(1)
}

fn human_card_label_width(rows: &[(String, String)], width: usize) -> usize {
    let longest = rows
        .iter()
        .map(|(label, _)| display_width(label))
        .max()
        .unwrap_or(HUMAN_CARD_LABEL_WIDTH);
    let max_for_value_column = width
        .saturating_sub(display_width(HUMAN_CARD_ROW_PREFIX) + 1 + 8)
        .max(1);
    longest
        .max(HUMAN_CARD_LABEL_WIDTH)
        .min(HUMAN_CARD_MAX_LABEL_WIDTH)
        .min(max_for_value_column)
        .max(1)
}

fn human_rows_from_fields(fields: &[OutputField], skip_keys: &[&str]) -> Vec<(String, String)> {
    fields
        .iter()
        .filter(|field| !skip_keys.contains(&field.key))
        .map(|field| {
            (
                human_field_label(field.key),
                human_field_value(field.key, &field.value),
            )
        })
        .collect()
}

fn human_index_complete_rows(fields: &[OutputField]) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "mode", "mode");
    push_field_row(&mut rows, fields, "repos", "repos");
    push_human_row(&mut rows, "files", human_file_counts(fields));
    push_human_row(&mut rows, "diagnostics", human_diagnostics(fields));
    push_human_row(&mut rows, "precise", human_precise_counts(fields));
    push_field_row(&mut rows, fields, "duration_ms", "duration");
    rows
}

fn human_init_complete_rows(fields: &[OutputField]) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repos", "repos");
    push_human_row(&mut rows, "precise", human_precise_counts(fields));
    rows
}

fn human_repair_complete_rows(fields: &[OutputField]) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repos", "repos");
    push_field_row(&mut rows, fields, "repaired", "repaired");
    rows
}

fn human_prune_complete_rows(fields: &[OutputField]) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repos", "repos");
    push_field_row(&mut rows, fields, "keep_manifest_snapshots", "keep");
    push_field_row(&mut rows, fields, "manifest_snapshots_deleted", "deleted");
    rows
}

fn field_display(fields: &[OutputField], key: &str) -> Option<String> {
    field_value(fields, key).map(|value| human_field_value(key, value))
}

fn human_delta(fields: &[OutputField]) -> Option<String> {
    let changed = field_value(fields, "changed")?;
    let deleted = field_value(fields, "deleted")?;
    Some(format!("{changed} changed · {deleted} deleted"))
}

fn human_provider_model(fields: &[OutputField]) -> Option<String> {
    let provider = field_value(fields, "provider").filter(|value| *value != "-");
    let model = field_value(fields, "model").filter(|value| *value != "-");
    match (provider, model) {
        (Some(provider), Some(model)) => Some(format!("{provider} · {model}")),
        (Some(provider), None) => Some(provider.to_owned()),
        (None, Some(model)) => Some(model.to_owned()),
        (None, None) => None,
    }
}

fn human_file_counts(fields: &[OutputField]) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(scanned) = field_value(fields, "scanned") {
        parts.push(format!("{scanned} scanned"));
    }
    if let Some(changed) = field_value(fields, "changed") {
        parts.push(format!("{changed} changed"));
    }
    if let Some(deleted) = field_value(fields, "deleted") {
        parts.push(format!("{deleted} deleted"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn human_diagnostics(fields: &[OutputField]) -> Option<String> {
    let diagnostics = field_value(fields, "diagnostics")?;
    let walk = field_value(fields, "diagnostics_walk").unwrap_or("0");
    let read = field_value(fields, "diagnostics_read").unwrap_or("0");
    Some(format!("{diagnostics} total · {walk} walk · {read} read"))
}

fn human_precise_counts(fields: &[OutputField]) -> Option<String> {
    let generators = field_value(fields, "precise_generators")?;
    let succeeded = field_value(fields, "precise_succeeded").unwrap_or("0");
    let failed = field_value(fields, "precise_failed").unwrap_or("0");
    let missing = field_value(fields, "precise_missing_tool").unwrap_or("0");
    let skipped = field_value(fields, "precise_skipped").unwrap_or("0");
    Some(format!(
        "{generators} generators · {succeeded} ok · {failed} failed · {missing} missing · {skipped} skipped"
    ))
}

fn human_generators(fields: &[OutputField]) -> Option<String> {
    let count = field_value(fields, "generators")?;
    let ids = field_value(fields, "generator_ids")
        .filter(|ids| !ids.is_empty() && *ids != "-")
        .map(|ids| format!(" · {ids}"))
        .unwrap_or_default();
    Some(format!("{count}{ids}"))
}

fn human_artifact_counts(fields: &[OutputField]) -> Option<String> {
    let artifacts = field_value(fields, "artifacts")?;
    let bytes = field_value(fields, "bytes").unwrap_or("0");
    Some(format!("{artifacts} artifacts · {bytes} bytes"))
}

fn human_mode_label(value: &str) -> String {
    value.replace('_', " ")
}

fn short_identifier(value: &str) -> String {
    if value == "-" || value.chars().count() <= 16 {
        return value.to_owned();
    }
    let prefix = value.chars().take(12).collect::<String>();
    format!("{prefix}…")
}

fn compact_human_fields(fields: &[OutputField], skip_keys: &[&str], limit: usize) -> String {
    fields
        .iter()
        .filter(|field| !skip_keys.contains(&field.key))
        .take(limit)
        .map(|field| {
            format!(
                "{}={}",
                compact_human_field_key(field.key),
                truncate_display(&human_field_value(field.key, &field.value), 80)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn human_field_label(key: &str) -> String {
    let key = key.strip_suffix("_ms").unwrap_or(key);
    key.replace('_', " ")
}

fn compact_human_field_key(key: &str) -> &str {
    match key {
        "duration_ms" => "duration",
        "debounce_ms" => "debounce",
        "retry_ms" => "retry",
        "repository_id" => "repo",
        "snapshot_plan" => "snapshot",
        "origin_allowlist" => "origins",
        "precise_generators" => "precise",
        "precise_succeeded" => "precise_ok",
        "precise_missing_tool" => "precise_missing",
        _ => key,
    }
}

fn human_field_value(key: &str, value: &str) -> String {
    if key.ends_with("_ms") && value.chars().all(|ch| ch.is_ascii_digit()) {
        return format!("{value}ms");
    }
    value.to_owned()
}

fn field_value<'a>(fields: &'a [OutputField], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|field| field.key == key)
        .map(|field| field.value.as_str())
}

fn field_is(fields: &[OutputField], key: &str, value: &str) -> bool {
    field_value(fields, key).is_some_and(|field_value| field_value == value)
}

fn human_event_title(area: &str, event: &str) -> String {
    match (area, event) {
        ("index", "plan") => "Index plan".to_owned(),
        ("index", "semantic") => "Semantic index".to_owned(),
        ("index", "fallback") => "Index fallback".to_owned(),
        ("index", "repo") => "Index repository".to_owned(),
        ("index", "diagnostic") => "Index diagnostic".to_owned(),
        ("precise", "plan") => "Precise plan".to_owned(),
        ("precise", "run") => "Precise run".to_owned(),
        ("startup", "semantic_model") => "Semantic model".to_owned(),
        ("startup", "semantic") => "Semantic runtime".to_owned(),
        ("startup", "storage") => "Storage ready".to_owned(),
        ("watch", event) => format!("Watch {}", human_title_token(event)),
        _ => format!("{} {}", human_title_token(area), human_title_token(event)),
    }
}

fn human_title_token(token: &str) -> String {
    if token.eq_ignore_ascii_case("http") {
        return "HTTP".to_owned();
    }
    if token.eq_ignore_ascii_case("mcp") {
        return "MCP".to_owned();
    }
    if token.eq_ignore_ascii_case("db") {
        return "DB".to_owned();
    }
    token
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_uppercase().collect::<String>();
                    word.push_str(chars.as_str());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn human_symbol(level: OutputLevel, fields: &[OutputField], color: bool) -> String {
    let status = field_value(fields, "status").unwrap_or_default();
    let symbol = match (level, status) {
        (OutputLevel::Error, _) | (_, "failed") | (_, "blocked") => "✕",
        (OutputLevel::Warn, _) | (_, "retry") | (_, "stale") => "⚠",
        (OutputLevel::Skip, _) | (_, "empty") | (_, "skipped") | (_, "fresh") => "−",
        (_, "ok") | (_, "finished") | (_, "listening") => "✓",
        (_, "starting") | (_, "started") | (_, "queued") | (_, "enabled") => "◇",
        _ => match level {
            OutputLevel::Ok => "✓",
            OutputLevel::Info => "◇",
            OutputLevel::Warn => "⚠",
            OutputLevel::Error => "✕",
            OutputLevel::Skip => "−",
        },
    };
    colorize(color, title_color(level), symbol)
}

fn title_color(level: OutputLevel) -> &'static str {
    match level {
        OutputLevel::Ok => "1;32",
        OutputLevel::Info => "1;36",
        OutputLevel::Warn => "1;33",
        OutputLevel::Error => "1;31",
        OutputLevel::Skip => "2",
    }
}

fn colorize(color: bool, code: &str, value: impl Display) -> String {
    if color {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.to_string()
    }
}

fn truncate_display(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let prefix = value.chars().take(keep).collect::<String>();
    format!("{prefix}…")
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

fn human_terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= HUMAN_MIN_WIDTH)
        .unwrap_or(HUMAN_DEFAULT_WIDTH)
}

fn human_color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn format_human_intro(color: bool) -> String {
    let mut output = String::new();
    output.push('\n');
    for (line, color_code) in HUMAN_INTRO_LOGO_LINES
        .iter()
        .zip(HUMAN_INTRO_COLOR_CODES.iter().copied())
    {
        output.push_str(&colorize(color, color_code, line));
        output.push('\n');
    }
    output.push('\n');
    output
}

/// Emits one UI-agnostic index progress snapshot through the CLI output policy.
pub(crate) fn emit_index_progress_event(
    output: CliOutput,
    event: IndexProgressEvent,
    extra_fields: &[OutputField],
) -> io::Result<()> {
    let status = event.status;
    let mut fields = vec![
        field("status", status.as_str()),
        field("repo", event.repository_id),
        field("phase", event.phase.as_str()),
        field("mode", event.mode.as_str()),
    ];
    if let Some(snapshot_id) = event.snapshot_id {
        fields.push(field("snapshot", snapshot_id));
    }
    if let Some(previous_snapshot_id) = event.previous_snapshot_id {
        fields.push(field("previous", previous_snapshot_id));
    }
    if let Some(scanned) = event.files_scanned {
        fields.push(field("scanned", scanned));
    }
    if let Some(changed) = event.files_changed {
        fields.push(field("changed", changed));
    }
    if let Some(deleted) = event.files_deleted {
        fields.push(field("deleted", deleted));
    }
    if let Some(diagnostics) = event.diagnostics {
        fields.push(field("diagnostics", diagnostics));
    }
    if let Some(records) = event.records {
        fields.push(field("records", records));
    }
    if let Some(changed_paths) = event.changed_paths {
        fields.push(field("changed_paths", changed_paths));
    }
    if let Some(deleted_paths) = event.deleted_paths {
        fields.push(field("deleted_paths", deleted_paths));
    }
    if let Some(pruned_snapshots) = event.pruned_snapshots {
        fields.push(field("pruned_snapshots", pruned_snapshots));
    }

    let level = match status {
        IndexProgressStatus::Starting => OutputLevel::Info,
        IndexProgressStatus::Ok => OutputLevel::Ok,
        IndexProgressStatus::Skipped => OutputLevel::Skip,
        IndexProgressStatus::Warning => OutputLevel::Warn,
    };
    output.progress_event(
        level,
        "index",
        "phase",
        &with_extra_fields(fields, extra_fields),
        None,
    )
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
    use super::{
        OutputLevel, OutputMode, field, format_event_line, format_human_event,
        format_human_event_with_width, format_human_intro,
    };

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

        assert!(output.contains("╭─  Index complete"));
        assert!(output.contains("repos"));
        assert!(output.contains("42ms"));
        assert!(output.contains("╰─"));
    }

    #[test]
    fn human_intro_renders_unicode_logo_with_blank_padding() {
        let output = format_human_intro(false);

        assert!(output.starts_with('\n'));
        assert!(output.ends_with("\n\n"));
        assert_eq!(output.lines().filter(|line| !line.is_empty()).count(), 5);
        assert!(output.contains("█████ ████  ███  ███   ███"));
        assert!(output.contains("█     █   █ ███  ███   ███"));
    }

    #[test]
    fn human_intro_uses_theme_color_when_enabled() {
        let output = format_human_intro(true);

        assert!(output.contains("\u{1b}[1;38;2;238;240;242m"));
        assert!(output.contains("\u{1b}[1;38;2;125;199;190m"));
        assert!(output.contains("\u{1b}[0m"));
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

        assert!(output.contains("╭─  Frigg error"));
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

        assert!(output.contains("◇ Watch Queued"));
        assert!(output.contains("debounce"));
        assert!(output.contains("250ms"));
        assert!(!output.contains("debounce="));
        assert!(output.contains("└─ path: /repo/src/main.rs"));
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

        assert!(output.contains("◇ Index plan"));
        assert!(output.contains("repo"));
        assert!(output.contains("snapshot"));
        assert!(output.contains("full rebuild · local · 4 records"));
        assert!(!output.contains("repo="));
        assert!(!output.contains("mode="));
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

        assert!(output.contains("− Precise plan"));
        assert!(output.contains("no generators need refresh"));
        assert!(!output.contains("reason="));
        assert!(output.contains("└─ workspace: current directory (.)"));
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

        assert!(output.contains("− Index paths"));
        assert!(output.contains("no path changes"));
        assert!(!output.contains("repo="));
        assert!(!output.contains("changed="));
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

        assert!(output.contains("◇ Walking files"));
        assert!(output.contains("477 scanned"));
        assert!(!output.contains("477 changed · 2 deleted"));
        assert!(!output.contains("phase="));
        assert!(!output.contains("status="));
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

        assert!(modified.contains("\u{1b}[1;33mModified\u{1b}[0m"));
        assert!(deleted.contains("\u{1b}[1;31mDeleted\u{1b}[0m"));
        assert!(created.contains("\u{1b}[1;32mCreated\u{1b}[0m"));
    }

    #[test]
    fn human_semantic_path_rows_color_only_action_keyword() {
        let output = format_human_event_with_width(
            OutputLevel::Info,
            "index",
            "semantic_path",
            &[field("action", "modified"), field("repo", "repo-001")],
            Some("src/lib.rs"),
            true,
            80,
        );

        assert!(output.contains("Semantic \u{1b}[1;33mModified\u{1b}[0m src/lib.rs"));
        assert!(!output.contains("\u{1b}[1;33mSemantic Modified\u{1b}[0m"));
    }

    #[test]
    fn human_card_text_starts_at_shared_column() {
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

        assert_eq!(char_column(lines[0], "Frigg Serve"), Some(4));
        assert_eq!(char_column(lines[1], "transport"), Some(4));
        assert_eq!(char_column(lines[2], "starting"), Some(4));
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
            .find(|line| line.contains("path: /Users/bnomei/Library/Caches/frigg/models"))
            .expect("semantic model path line should be rendered");

        assert_eq!(char_column(lines[0], "Semantic model"), Some(4));
        assert_eq!(char_column(path_line, "└─"), Some(4));
        assert!(lines.iter().all(|line| line.chars().count() <= 64));
    }

    #[test]
    fn human_continuation_expands_current_directory_dot() {
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

        assert!(output.contains("└─ workspace: current directory (.)"));
    }

    fn char_column(line: &str, needle: &str) -> Option<usize> {
        line.find(needle)
            .map(|byte_index| line[..byte_index].chars().count())
    }
}
