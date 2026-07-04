//! Human event router that classifies structured CLI events into display components.
//!
//! This layer keeps event semantics decoupled from terminal layout choices, so new CLI events can
//! reuse the same stdout/stderr contract while gaining an appropriate TTY presentation.

use frigg::human_output::HumanRow;

use super::human_block::{
    HumanMarkerKind, compact_human_fields, field_is, field_value, format_human_activity_line,
    format_human_card, format_human_component, format_human_continuation,
    format_human_progress_component, human_event_title, human_field_value, human_file_counts,
    human_init_complete_rows, human_precise_counts, human_prune_complete_rows,
    human_repair_complete_rows, human_rows_from_fields, human_title_token, human_uses_detail_rail,
    push_field_row, push_human_row, push_human_separator,
};
use super::{OutputField, OutputLevel};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HumanEventKind {
    Error,
    Start,
    Ready,
    Complete,
    Startup,
    IndexPlan,
    IndexSemantic,
    IndexPhase,
    IndexPaths,
    Repository,
    PrecisePlan,
    PreciseRun,
    Path,
    Watch,
    PreciseGenerator,
    StorageRepair,
    ToolCall,
    Activity,
}

impl HumanEventKind {
    /// Maps stable area/event fields into one human renderer branch without changing CLI output data.
    fn classify(level: OutputLevel, area: &str, event: &str, fields: &[OutputField]) -> Self {
        if area == "tool" && event == "call" {
            return Self::ToolCall;
        }
        if level == OutputLevel::Error {
            return Self::Error;
        }
        if event == "start" {
            return Self::Start;
        }
        if area == "serve" && event == "http" && field_is(fields, "status", "listening") {
            return Self::Ready;
        }
        if event == "complete" {
            return Self::Complete;
        }
        match (area, event) {
            ("startup", "semantic" | "semantic_model" | "storage") => Self::Startup,
            ("index", "plan") => Self::IndexPlan,
            ("index", "semantic") => Self::IndexSemantic,
            ("index", "phase") => Self::IndexPhase,
            ("index", "paths" | "semantic_paths") => Self::IndexPaths,
            (_, "repo") => Self::Repository,
            ("precise", "plan") => Self::PrecisePlan,
            ("precise", "run") => Self::PreciseRun,
            ("index", "path" | "semantic_path") => Self::Path,
            ("watch", _) => Self::Watch,
            ("precise", "generator") => Self::PreciseGenerator,
            ("storage", "auto_repair") => Self::StorageRepair,
            _ => Self::Activity,
        }
    }
}

pub(crate) fn format_human_event(
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
        super::human_block::human_terminal_width(),
    )
}

pub(crate) fn format_human_event_with_width(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let width = width.max(super::human_block::HUMAN_MIN_WIDTH);
    match HumanEventKind::classify(level, area, event, fields) {
        HumanEventKind::Error => format_human_error_card(area, event, fields, path, color, width),
        HumanEventKind::Start => format_human_start_card(area, fields, path, color, width),
        HumanEventKind::Ready => format_human_ready_card(fields, color, width),
        HumanEventKind::Complete => {
            format_human_complete_card(level, area, fields, path, color, width)
        }
        HumanEventKind::Startup => {
            format_human_startup_component(level, event, fields, path, color, width)
        }
        HumanEventKind::IndexPlan => {
            super::index::format_human_index_plan_component(level, fields, path, color, width)
        }
        HumanEventKind::IndexSemantic => {
            super::index::format_human_index_semantic_component(level, fields, path, color, width)
        }
        HumanEventKind::IndexPhase => {
            super::index::format_human_index_phase_row(level, fields, path, color, width)
        }
        HumanEventKind::IndexPaths => super::index::format_human_index_paths_component(
            level, event, fields, path, color, width,
        ),
        HumanEventKind::Repository => {
            format_human_repository_component(level, area, fields, path, color, width)
        }
        HumanEventKind::PrecisePlan => {
            super::precise::format_human_precise_plan_component(level, fields, path, color, width)
        }
        HumanEventKind::PreciseRun => {
            super::precise::format_human_precise_run_component(level, fields, path, color, width)
        }
        HumanEventKind::Path => {
            super::index::format_human_path_row(level, area, event, fields, path, color, width)
        }
        HumanEventKind::Watch => {
            format_human_watch_component(level, event, fields, path, color, width)
        }
        HumanEventKind::PreciseGenerator => {
            super::precise::format_human_precise_generator_component(
                level, fields, path, color, width,
            )
        }
        HumanEventKind::StorageRepair => {
            format_human_storage_repair_card(level, fields, path, color, width)
        }
        HumanEventKind::ToolCall => format_human_tool_call_row(level, fields, color, width),
        HumanEventKind::Activity => {
            format_human_activity_row(level, area, event, fields, path, color, width)
        }
    }
}

fn format_human_tool_call_row(
    level: OutputLevel,
    fields: &[OutputField],
    color: bool,
    width: usize,
) -> String {
    let title = field_value(fields, "tool").unwrap_or("tool").to_owned();
    let mut details = Vec::new();
    if let Some(duration_ms) = field_value(fields, "duration_ms") {
        details.push(human_field_value("duration_ms", duration_ms));
    }
    if let Some(saved_percent) = field_value(fields, "context_saved_percent") {
        details.push(format!("{saved_percent} saved"));
    }
    format_human_activity_line(
        level,
        fields,
        title,
        details.join(" · "),
        HumanMarkerKind::Tool,
        None,
        color,
        width,
    )
}

fn format_human_start_card(
    area: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let rows = human_rows_from_fields(fields, &["status"]);
    let title = format!("{} starting", human_title_token(area));
    format_human_component(
        OutputLevel::Info,
        fields,
        &title,
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

fn format_human_ready_card(fields: &[OutputField], color: bool, width: usize) -> String {
    let mut rows = Vec::new();
    if let (Some(addr), Some(endpoint)) =
        (field_value(fields, "addr"), field_value(fields, "endpoint"))
    {
        rows.push(HumanRow::kv("endpoint", format!("http://{addr}{endpoint}")));
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
        fields,
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
    let mut rows = match area {
        "index" => super::index::human_index_complete_rows(fields),
        "init" => human_init_complete_rows(fields),
        "repair-storage" => human_repair_complete_rows(fields),
        "prune-storage" => human_prune_complete_rows(fields),
        _ => human_rows_from_fields(fields, &["status"]),
    };
    if let Some(path) = path {
        rows.push(HumanRow::path(path));
    }
    let title = format!("{} complete", human_title_token(area));
    format_human_card(level, &title, rows, Some(&title), fields, color, width)
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
        HumanRow::kv("area", human_title_token(area)),
        HumanRow::kv("event", human_title_token(event)),
    ];
    rows.extend(human_rows_from_fields(fields, &["status"]));
    if let Some(path) = path {
        rows.push(HumanRow::path(path));
    }
    format_human_card(
        OutputLevel::Error,
        "Frigg error",
        rows,
        Some("command stopped"),
        fields,
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
    let rows = human_rows_from_fields(fields, &["status"]);
    format_human_component(
        level,
        fields,
        "Storage auto-repair",
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
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

    format_human_progress_component(level, fields, title, rows, Vec::new(), path, color, width)
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
    push_human_separator(&mut rows);
    push_human_row(&mut rows, "files", human_file_counts(fields));
    push_human_row(
        &mut rows,
        "diagnostics",
        super::human_block::human_diagnostics(fields),
    );
    push_human_row(&mut rows, "precise", human_precise_counts(fields));
    push_human_separator(&mut rows);
    push_field_row(&mut rows, fields, "repaired", "repaired");
    push_field_row(&mut rows, fields, "manifest_snapshots_deleted", "deleted");
    push_field_row(&mut rows, fields, "keep_manifest_snapshots", "keep");
    push_human_row(
        &mut rows,
        "snapshot",
        field_value(fields, "snapshot").map(super::human_block::short_identifier),
    );
    push_field_row(&mut rows, fields, "duration_ms", "duration");
    push_field_row(&mut rows, fields, "db", "storage");

    format_human_component(level, fields, title, rows, Vec::new(), path, color, width)
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

    format_human_progress_component(
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
        HumanMarkerKind::Checkpoint,
        None,
        color,
        width,
    );
    if let Some(path) = path {
        line.push('\n');
        line.push_str(&format_human_continuation(
            path,
            human_uses_detail_rail(level, fields, HumanMarkerKind::Checkpoint),
            color,
            width,
        ));
    }
    line
}
