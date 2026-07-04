//! Human formatting for index, semantic refresh, and path-level progress events.
//!
//! This module preserves the structured event contract while grouping repository, snapshot,
//! semantic, and file-delta fields into compact terminal rows.

use std::collections::HashSet;
use std::io;

use frigg::human_output::{HumanRow, format_human_badged_line, human_badge_prefix_width};
use frigg::indexer::{
    IndexPlan, IndexProgressEvent, IndexProgressPhase, IndexProgressStatus, SemanticRefreshMode,
};

use super::fields::FieldBag;
use super::human_block::{
    HUMAN_ACTIVITY_PREFIX, HUMAN_TEXT_COLUMN, HumanMarkerKind, colorize, colorize_action_title,
    compact_human_fields, display_width, duration_detail, field_display, field_is, field_value,
    format_human_component, format_human_continuation, human_delta, human_diagnostics,
    human_file_counts, human_mode_label, human_precise_counts, human_provider_model,
    human_session_badge, human_symbol_with_color, human_terminal_width, human_title_token,
    human_uses_detail_rail, push_field_row, push_human_row, push_human_separator, short_identifier,
    truncate_display,
};
use super::human_topic::{HumanTopic, human_severity_accent_color, human_title_accent_color};
use super::{CliOutput, OutputField, OutputLevel, field};

const MAX_VERBOSE_PATH_LINES: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PathEventKind {
    Index,
    Semantic,
}

impl PathEventKind {
    fn event(self) -> &'static str {
        match self {
            Self::Index => "path",
            Self::Semantic => "semantic_path",
        }
    }

    fn truncation_event(self) -> &'static str {
        match self {
            Self::Index => "paths",
            Self::Semantic => "semantic_paths",
        }
    }

    fn from_event(event: &str) -> Self {
        match event {
            "semantic_path" | "semantic_paths" => Self::Semantic,
            _ => Self::Index,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PathAction {
    Modified,
    Deleted,
    Created,
    Renamed,
    Skipped,
}

impl PathAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Created => "created",
            Self::Renamed => "renamed",
            Self::Skipped => "skipped",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "deleted" | "delete" | "removed" | "remove" => Self::Deleted,
            "created" | "create" | "added" | "add" | "new" => Self::Created,
            "renamed" | "moved" => Self::Renamed,
            "skipped" | "unchanged" => Self::Skipped,
            _ => Self::Modified,
        }
    }
}

pub(crate) fn format_human_index_plan_component(
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
    push_human_separator(&mut rows);
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

pub(crate) fn format_human_index_semantic_component(
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
    push_human_separator(&mut rows);
    push_human_row(&mut rows, "model", human_provider_model(fields));
    push_field_row(&mut rows, fields, "records", "records");
    push_human_row(&mut rows, "work", semantic_scope_detail(fields));
    push_human_separator(&mut rows);
    push_field_row(&mut rows, fields, "source", "source");
    push_field_row(&mut rows, fields, "class", "class");

    format_human_component(
        level,
        fields,
        "Semantic refresh",
        rows,
        Vec::new(),
        path,
        color,
        width,
    )
}

pub(crate) fn format_human_index_phase_row(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    format_human_index_phase_row_with_semantic(level, fields, None, path, color, width)
}

fn format_human_index_phase_row_with_semantic(
    level: OutputLevel,
    fields: &[OutputField],
    semantic_mode: Option<SemanticRefreshMode>,
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let title = human_index_phase_title(fields, semantic_mode);
    let fields_bag = FieldBag::new(fields);
    let badge = human_session_badge(fields_bag);
    let badge_prefix_width = human_badge_prefix_width(badge.as_deref());
    let accent = human_title_accent_color(level, fields_bag, None, &title);
    let symbol =
        human_symbol_with_color(level, fields_bag, HumanMarkerKind::Progress, color, accent);
    let detail = human_index_phase_detail(fields, semantic_mode);
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN + badge_prefix_width);
    let title_budget = content_width.saturating_sub(2);
    let detail_budget = detail
        .as_ref()
        .map(|detail| display_width(detail).min(content_width / 2))
        .unwrap_or(0);
    let title_budget = title_budget.saturating_sub(detail_budget);
    let title = truncate_display(&title, title_budget);
    let mut line = format!(
        "{HUMAN_ACTIVITY_PREFIX}{symbol} {}",
        colorize(color, accent, &title)
    );
    if let Some(detail) = detail {
        let used_width = badge_prefix_width + HUMAN_TEXT_COLUMN + display_width(&title);
        let remaining = width.saturating_sub(used_width + 2);
        if remaining >= 8 {
            line.push_str("  ");
            line.push_str(&colorize(color, "2", truncate_display(&detail, remaining)));
        }
    }
    if let Some(path) = path {
        let detail = format_human_continuation(
            path,
            human_uses_detail_rail(level, fields, HumanMarkerKind::Progress),
            color,
            width,
        );
        line.push('\n');
        line.push_str(&detail);
    }
    format_human_badged_line(badge.as_deref(), line, color)
}

pub(crate) fn format_human_index_paths_component(
    level: OutputLevel,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let title = if PathEventKind::from_event(event) == PathEventKind::Semantic {
        "Semantic paths"
    } else {
        "Index paths"
    };
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_human_row(&mut rows, "delta", human_delta(fields));
    push_human_separator(&mut rows);
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

pub(crate) fn format_human_path_row(
    level: OutputLevel,
    area: &str,
    event: &str,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let kind = PathEventKind::from_event(event);
    let action = PathAction::from_str(field_value(fields, "action").unwrap_or("changed"));
    let action_title = human_title_token(action.as_str());
    let title_prefix = match kind {
        PathEventKind::Semantic => format!("Semantic embedding {action_title}"),
        PathEventKind::Index => action_title,
    };
    let title_suffix = path.unwrap_or_else(|| human_event_title_static(area, kind));
    let detail_skip = &["status", "action", "repo"];
    let details = match kind {
        PathEventKind::Semantic => "semantic embedding input".to_owned(),
        PathEventKind::Index => compact_human_fields(fields, detail_skip, 3),
    };
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

#[allow(clippy::too_many_arguments)]
fn format_human_action_line(
    level: OutputLevel,
    fields: &[OutputField],
    title_prefix: &str,
    action: PathAction,
    title_suffix: &str,
    details: String,
    color: bool,
    width: usize,
) -> String {
    let fields_bag = FieldBag::new(fields);
    let badge = human_session_badge(fields_bag);
    let badge_prefix_width = human_badge_prefix_width(badge.as_deref());
    let accent = human_title_accent_color(level, fields_bag, None, title_prefix);
    let symbol = human_symbol_with_color(
        level,
        fields_bag,
        HumanMarkerKind::Checkpoint,
        color,
        accent,
    );
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN + badge_prefix_width);
    let title_budget = content_width.saturating_sub(2);
    let prefix_width = display_width(title_prefix);
    let suffix_budget = title_budget.saturating_sub(prefix_width + 1);
    let title_suffix = truncate_display(title_suffix, suffix_budget);
    let title_width = prefix_width + 1 + display_width(&title_suffix);
    let mut line = format!(
        "{HUMAN_ACTIVITY_PREFIX}{symbol} {} {}",
        colorize_action_title(color, action.as_str(), title_prefix, accent),
        title_suffix
    );
    let used_width = badge_prefix_width + HUMAN_TEXT_COLUMN + title_width;
    let detail_budget = width.saturating_sub(used_width + 2);
    if !details.is_empty() && detail_budget >= 8 {
        line.push_str("  ");
        line.push_str(&colorize(
            color,
            "2",
            truncate_display(&details, detail_budget),
        ));
    }
    format_human_badged_line(badge.as_deref(), line, color)
}

fn human_event_title_static(area: &str, kind: PathEventKind) -> &'static str {
    match (area, kind) {
        ("index", PathEventKind::Index) => "Index path",
        ("index", PathEventKind::Semantic) => "Semantic path",
        _ => "Path",
    }
}

pub(crate) fn human_index_complete_rows(fields: &[OutputField]) -> Vec<HumanRow> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "mode", "mode");
    push_field_row(&mut rows, fields, "repos", "repos");
    push_human_separator(&mut rows);
    push_human_row(&mut rows, "files", human_file_counts(fields));
    push_human_row(&mut rows, "diagnostics", human_diagnostics(fields));
    push_human_row(&mut rows, "precise", human_precise_counts(fields));
    push_human_separator(&mut rows);
    push_field_row(&mut rows, fields, "duration_ms", "duration");
    rows
}

pub(crate) fn format_human_index_progress_event(
    event: &IndexProgressEvent,
    extra_fields: &[OutputField],
    color: bool,
    width: usize,
) -> String {
    let level = index_progress_level(event.status);
    let title = human_index_progress_title(event);
    let badge = human_session_badge(FieldBag::new(extra_fields));
    let badge_prefix_width = human_badge_prefix_width(badge.as_deref());
    let accent = index_progress_accent(level, event);
    let symbol = colorize(color, accent, index_progress_symbol(event.status));
    let detail = human_index_progress_detail(event);
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN + badge_prefix_width);
    let title_budget = content_width.saturating_sub(2);
    let detail_budget = detail
        .as_ref()
        .map(|detail| display_width(detail).min(content_width / 2))
        .unwrap_or(0);
    let title_budget = title_budget.saturating_sub(detail_budget);
    let title = truncate_display(&title, title_budget);
    let mut line = format!(
        "{HUMAN_ACTIVITY_PREFIX}{symbol} {}",
        colorize(color, accent, &title)
    );
    if let Some(detail) = detail {
        let used_width = badge_prefix_width + HUMAN_TEXT_COLUMN + display_width(&title);
        let remaining = width.saturating_sub(used_width + 2);
        if remaining >= 8 {
            line.push_str("  ");
            line.push_str(&colorize(color, "2", truncate_display(&detail, remaining)));
        }
    }
    format_human_badged_line(badge.as_deref(), line, color)
}

fn index_progress_symbol(status: IndexProgressStatus) -> &'static str {
    match status {
        IndexProgressStatus::Starting => "│",
        IndexProgressStatus::Ok => "●",
        IndexProgressStatus::Skipped => "○",
        IndexProgressStatus::Warning => "▲",
    }
}

fn index_progress_accent(level: OutputLevel, event: &IndexProgressEvent) -> &'static str {
    human_severity_accent_color(level, event.status.as_str())
        .unwrap_or_else(|| index_progress_topic(event.phase).color())
}

fn index_progress_topic(phase: IndexProgressPhase) -> HumanTopic {
    match phase {
        IndexProgressPhase::InitializeStorage
        | IndexProgressPhase::CheckpointWal
        | IndexProgressPhase::PruneManifestSnapshots => HumanTopic::Storage,
        IndexProgressPhase::SemanticRefresh => HumanTopic::Semantic,
        IndexProgressPhase::LoadManifest
        | IndexProgressPhase::BuildManifest
        | IndexProgressPhase::BuildPlan
        | IndexProgressPhase::PersistManifestSnapshot
        | IndexProgressPhase::RefreshRetrievalProjections => HumanTopic::Index,
    }
}

fn human_index_progress_title(event: &IndexProgressEvent) -> String {
    if event.phase == IndexProgressPhase::SemanticRefresh {
        return human_semantic_progress_title(event);
    }
    let title = match (event.phase, event.status) {
        (IndexProgressPhase::InitializeStorage, IndexProgressStatus::Starting) => {
            "Opening storage..."
        }
        (IndexProgressPhase::InitializeStorage, IndexProgressStatus::Ok) => "Storage open",
        (IndexProgressPhase::InitializeStorage, IndexProgressStatus::Skipped) => {
            "Storage already open"
        }
        (IndexProgressPhase::LoadManifest, IndexProgressStatus::Starting) => "Loading manifest...",
        (IndexProgressPhase::LoadManifest, IndexProgressStatus::Ok) => "Manifest loaded",
        (IndexProgressPhase::LoadManifest, IndexProgressStatus::Skipped) => "Manifest load skipped",
        (IndexProgressPhase::BuildManifest, IndexProgressStatus::Starting) => "Walking files...",
        (IndexProgressPhase::BuildManifest, IndexProgressStatus::Ok) => "Files scanned",
        (IndexProgressPhase::BuildManifest, IndexProgressStatus::Skipped) => "File scan skipped",
        (IndexProgressPhase::BuildPlan, IndexProgressStatus::Starting) => "Planning index...",
        (IndexProgressPhase::BuildPlan, IndexProgressStatus::Ok) => "Index planned",
        (IndexProgressPhase::BuildPlan, IndexProgressStatus::Skipped) => "Index plan skipped",
        (IndexProgressPhase::PersistManifestSnapshot, IndexProgressStatus::Starting) => {
            "Writing manifest..."
        }
        (IndexProgressPhase::PersistManifestSnapshot, IndexProgressStatus::Ok) => {
            "Manifest written"
        }
        (IndexProgressPhase::PersistManifestSnapshot, IndexProgressStatus::Skipped) => {
            "Manifest unchanged"
        }
        (IndexProgressPhase::RefreshRetrievalProjections, IndexProgressStatus::Starting) => {
            "Refreshing search projections..."
        }
        (IndexProgressPhase::RefreshRetrievalProjections, IndexProgressStatus::Ok) => {
            "Search projections updated"
        }
        (IndexProgressPhase::RefreshRetrievalProjections, IndexProgressStatus::Skipped) => {
            "Search projections current"
        }
        (IndexProgressPhase::PruneManifestSnapshots, IndexProgressStatus::Starting) => {
            "Pruning old snapshots..."
        }
        (IndexProgressPhase::PruneManifestSnapshots, IndexProgressStatus::Ok) => {
            "Old snapshots pruned"
        }
        (IndexProgressPhase::PruneManifestSnapshots, IndexProgressStatus::Skipped) => {
            "Snapshot retention unchanged"
        }
        (IndexProgressPhase::CheckpointWal, IndexProgressStatus::Starting) => {
            "Saving storage checkpoint..."
        }
        (IndexProgressPhase::CheckpointWal, IndexProgressStatus::Ok) => "Storage checkpoint saved",
        (IndexProgressPhase::CheckpointWal, IndexProgressStatus::Skipped) => {
            "Storage checkpoint skipped"
        }
        (IndexProgressPhase::CheckpointWal, IndexProgressStatus::Warning) => {
            "Storage checkpoint warning"
        }
        _ => return human_title_token(event.phase.as_str()),
    };
    title.to_owned()
}

fn human_index_progress_detail(event: &IndexProgressEvent) -> Option<String> {
    let parts = match event.phase {
        IndexProgressPhase::InitializeStorage => vec![progress_duration_detail(event)],
        IndexProgressPhase::LoadManifest => vec![
            progress_duration_detail(event),
            event
                .previous_snapshot_id
                .as_deref()
                .filter(|value| *value != "-")
                .map(|snapshot| format!("previous {}", short_identifier(snapshot))),
            event
                .files_scanned
                .filter(|files| *files != 0)
                .map(|files| format!("{files} previous files")),
        ],
        IndexProgressPhase::BuildManifest => vec![
            progress_duration_detail(event),
            event.files_scanned.map(|files| format!("{files} scanned")),
            event
                .diagnostics
                .filter(|diagnostics| *diagnostics != 0)
                .map(|diagnostics| format!("{diagnostics} diagnostics")),
        ],
        IndexProgressPhase::BuildPlan => vec![
            progress_duration_detail(event),
            progress_delta_detail(event),
            progress_semantic_records_detail(event),
            progress_snapshot_detail(event),
        ],
        IndexProgressPhase::PersistManifestSnapshot
        | IndexProgressPhase::RefreshRetrievalProjections => {
            vec![
                progress_duration_detail(event),
                progress_snapshot_detail(event),
            ]
        }
        IndexProgressPhase::CheckpointWal => {
            vec![
                progress_duration_detail(event),
                progress_storage_checkpoint_detail(event),
            ]
        }
        IndexProgressPhase::SemanticRefresh => vec![
            progress_duration_detail(event),
            progress_semantic_records_detail(event),
            progress_semantic_mode_detail(event),
            progress_semantic_path_delta(event),
            progress_semantic_duration_per_doc_detail(event),
        ],
        IndexProgressPhase::PruneManifestSnapshots => vec![
            progress_duration_detail(event),
            event
                .pruned_snapshots
                .filter(|pruned| *pruned != 0)
                .map(|pruned| format!("{pruned} removed")),
        ],
    };
    let parts = parts.into_iter().flatten().collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn human_semantic_progress_title(event: &IndexProgressEvent) -> String {
    let title = match (event.status, event.semantic_refresh_mode) {
        (IndexProgressStatus::Starting, Some(mode)) if mode.is_rebuild_like() => {
            "Rebuilding semantic chunks..."
        }
        (IndexProgressStatus::Starting, Some(SemanticRefreshMode::IncrementalAdvance))
            if progress_semantic_zero_delta(event) && progress_semantic_zero_records(event) =>
        {
            "Advancing semantic snapshot..."
        }
        (IndexProgressStatus::Starting, _) => "Embedding semantic chunks...",
        (IndexProgressStatus::Ok, Some(mode)) if mode.is_rebuild_like() => {
            "Semantic chunks rebuilt"
        }
        (IndexProgressStatus::Ok, Some(SemanticRefreshMode::IncrementalAdvance))
            if progress_semantic_zero_delta(event) && progress_semantic_zero_records(event) =>
        {
            "Semantic snapshot advanced"
        }
        (IndexProgressStatus::Ok, _) => "Semantic chunks stored",
        (IndexProgressStatus::Skipped, Some(SemanticRefreshMode::ReuseExisting)) => {
            "Semantic snapshot current"
        }
        (IndexProgressStatus::Skipped, Some(SemanticRefreshMode::Disabled)) => {
            "Semantic refresh disabled"
        }
        (IndexProgressStatus::Skipped, _) => "Semantic refresh skipped",
        _ => "Semantic refresh",
    };
    title.to_owned()
}

fn progress_duration_detail(event: &IndexProgressEvent) -> Option<String> {
    event.duration_ms.map(|duration| format!("{duration}ms"))
}

fn progress_delta_detail(event: &IndexProgressEvent) -> Option<String> {
    Some(format!(
        "{} changed · {} deleted",
        event.files_changed?, event.files_deleted?
    ))
}

fn progress_semantic_records_detail(event: &IndexProgressEvent) -> Option<String> {
    event
        .records
        .filter(|records| *records != 0)
        .map(|records| format!("{records} records"))
}

fn progress_semantic_mode_detail(event: &IndexProgressEvent) -> Option<String> {
    match event.semantic_refresh_mode {
        Some(SemanticRefreshMode::FullRebuild) => Some("full rebuild".to_owned()),
        Some(SemanticRefreshMode::FullRebuildFromChangedOnly) => {
            Some("changed-only rebuild".to_owned())
        }
        Some(SemanticRefreshMode::ReuseExisting)
            if event.status != IndexProgressStatus::Skipped =>
        {
            Some("snapshot current".to_owned())
        }
        Some(SemanticRefreshMode::Disabled) if event.status != IndexProgressStatus::Skipped => {
            Some("disabled".to_owned())
        }
        Some(SemanticRefreshMode::IncrementalAdvance)
            if progress_semantic_zero_delta(event) && progress_semantic_zero_records(event) =>
        {
            Some("metadata advance".to_owned())
        }
        _ => None,
    }
}

fn progress_semantic_path_delta(event: &IndexProgressEvent) -> Option<String> {
    if event
        .semantic_refresh_mode
        .is_some_and(|mode| !mode.shows_path_delta())
    {
        return None;
    }
    match (event.changed_paths, event.deleted_paths) {
        (Some(changed), Some(deleted)) => Some(format!("{changed} changed · {deleted} deleted")),
        _ => progress_delta_detail(event),
    }
}

fn progress_semantic_zero_delta(event: &IndexProgressEvent) -> bool {
    let changed = event.changed_paths.or(event.files_changed);
    let deleted = event.deleted_paths.or(event.files_deleted);
    matches!((changed, deleted), (Some(0), Some(0)))
}

fn progress_semantic_zero_records(event: &IndexProgressEvent) -> bool {
    matches!(event.records, None | Some(0))
}

fn progress_semantic_duration_per_doc_detail(event: &IndexProgressEvent) -> Option<String> {
    let duration_ms = event.duration_ms?;
    let records = event.records? as u128;
    if records == 0 {
        return None;
    }
    Some(format!(
        "{}ms/doc",
        super::human_block::format_per_doc_duration(duration_ms, records)
    ))
}

fn progress_snapshot_detail(event: &IndexProgressEvent) -> Option<String> {
    event
        .snapshot_id
        .as_deref()
        .filter(|value| *value != "-")
        .map(|snapshot| format!("manifest {}", short_identifier(snapshot)))
}

fn progress_storage_checkpoint_detail(event: &IndexProgressEvent) -> Option<String> {
    event
        .snapshot_id
        .as_deref()
        .filter(|value| *value != "-")
        .map(|snapshot| format!("manifest checkpoint {}", short_identifier(snapshot)))
}

/// Emits one UI-agnostic index progress snapshot through the CLI output policy.
pub(crate) fn emit_index_progress_event(
    output: CliOutput,
    event: IndexProgressEvent,
    extra_fields: &[OutputField],
) -> io::Result<()> {
    if !output.wants_progress_events() {
        return Ok(());
    }

    let level = index_progress_level(event.status);
    if output.tui_enabled() {
        let color = super::human_block::human_color_enabled();
        return super::write_human_stderr_block(
            &format_human_index_progress_event(&event, extra_fields, color, human_terminal_width()),
            color,
        );
    }

    let fields = index_progress_fields(&event, extra_fields);
    output.progress_event(level, "index", "phase", &fields, None)
}

fn index_progress_level(status: IndexProgressStatus) -> OutputLevel {
    match status {
        IndexProgressStatus::Starting => OutputLevel::Info,
        IndexProgressStatus::Ok => OutputLevel::Ok,
        IndexProgressStatus::Skipped => OutputLevel::Skip,
        IndexProgressStatus::Warning => OutputLevel::Warn,
    }
}

fn index_progress_fields(
    event: &IndexProgressEvent,
    extra_fields: &[OutputField],
) -> Vec<OutputField> {
    let mut fields = vec![
        field("status", event.status.as_str()),
        field("repo", &event.repository_id),
        field("phase", event.phase.as_str()),
        field("mode", event.mode.as_str()),
    ];
    if let Some(snapshot_id) = event.snapshot_id.as_deref() {
        fields.push(field("snapshot", snapshot_id));
    }
    if let Some(previous_snapshot_id) = event.previous_snapshot_id.as_deref() {
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
    if let Some(semantic_refresh_mode) = event.semantic_refresh_mode {
        fields.push(field("semantic", semantic_refresh_mode.as_str()));
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
    if let Some(duration_ms) = event.duration_ms {
        fields.push(field("duration_ms", duration_ms));
    }
    fields.extend_from_slice(extra_fields);
    fields
}

/// Emits verbose index planning and path-delta progress events for one repository plan.
pub(crate) fn emit_index_plan_events(
    output: CliOutput,
    repository_id: &str,
    plan: &IndexPlan,
    extra_fields: &[OutputField],
) -> io::Result<()> {
    if !output.wants_progress_events() {
        return Ok(());
    }

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

    if plan.semantic_refresh.mode == SemanticRefreshMode::FullRebuildFromChangedOnly {
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

    let generic_changed_paths = visible_index_paths_without_semantic(
        &plan.changed_paths,
        &plan.semantic_refresh.changed_paths,
    );
    let generic_deleted_paths = visible_index_paths_without_semantic(
        &plan.deleted_paths,
        &plan.semantic_refresh.deleted_paths,
    );

    emit_index_path_lines(
        output,
        PathEventKind::Index,
        repository_id,
        PathAction::Modified,
        &generic_changed_paths,
        extra_fields,
    )?;
    emit_index_path_lines(
        output,
        PathEventKind::Index,
        repository_id,
        PathAction::Deleted,
        &generic_deleted_paths,
        extra_fields,
    )
}

fn visible_index_paths_without_semantic<'a>(
    paths: &'a [String],
    semantic_paths: &'a [String],
) -> Vec<&'a str> {
    if semantic_paths.is_empty() {
        return paths.iter().map(String::as_str).collect();
    }
    let semantic_paths = semantic_paths
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    paths
        .iter()
        .filter(|path| !semantic_paths.contains(path.as_str()))
        .map(String::as_str)
        .collect()
}

fn emit_index_semantic_events(
    output: CliOutput,
    repository_id: &str,
    plan: &IndexPlan,
    extra_fields: &[OutputField],
) -> io::Result<()> {
    if plan.semantic_refresh.mode.is_noop() {
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
        PathEventKind::Semantic,
        repository_id,
        PathAction::Modified,
        &plan.semantic_refresh.changed_paths,
        extra_fields,
    )?;
    emit_index_path_lines(
        output,
        PathEventKind::Semantic,
        repository_id,
        PathAction::Deleted,
        &plan.semantic_refresh.deleted_paths,
        extra_fields,
    )
}

fn emit_index_path_lines(
    output: CliOutput,
    kind: PathEventKind,
    repository_id: &str,
    action: PathAction,
    paths: &[impl AsRef<str>],
    extra_fields: &[OutputField],
) -> io::Result<()> {
    for path in paths.iter().take(MAX_VERBOSE_PATH_LINES) {
        output.progress_event(
            OutputLevel::Info,
            "index",
            kind.event(),
            &with_extra_fields(
                vec![
                    field("action", action.as_str()),
                    field("repo", repository_id),
                ],
                extra_fields,
            ),
            Some(path.as_ref()),
        )?;
    }
    if paths.len() > MAX_VERBOSE_PATH_LINES {
        output.progress_event(
            OutputLevel::Info,
            "index",
            kind.truncation_event(),
            &with_extra_fields(
                vec![
                    field("status", "truncated"),
                    field("repo", repository_id),
                    field("action", action.as_str()),
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

fn human_index_phase_title(
    fields: &[OutputField],
    semantic_mode: Option<SemanticRefreshMode>,
) -> String {
    let phase = field_value(fields, "phase").unwrap_or("work");
    if phase == "semantic_refresh" {
        return human_semantic_phase_title(fields, semantic_mode);
    }
    let status = field_value(fields, "status").unwrap_or("starting");
    let title = match (phase, status) {
        ("initialize_storage", "starting") => "Opening storage...",
        ("initialize_storage", "ok") => "Storage open",
        ("initialize_storage", "skipped") => "Storage already open",
        ("load_manifest", "starting") => "Loading manifest...",
        ("load_manifest", "ok") => "Manifest loaded",
        ("load_manifest", "skipped") => "Manifest load skipped",
        ("build_manifest", "starting") => "Walking files...",
        ("build_manifest", "ok") => "Files scanned",
        ("build_manifest", "skipped") => "File scan skipped",
        ("build_plan", "starting") => "Planning index...",
        ("build_plan", "ok") => "Index planned",
        ("build_plan", "skipped") => "Index plan skipped",
        ("persist_manifest_snapshot", "starting") => "Writing manifest...",
        ("persist_manifest_snapshot", "ok") => "Manifest written",
        ("persist_manifest_snapshot", "skipped") => "Manifest unchanged",
        ("refresh_retrieval_projections", "starting") => "Refreshing search projections...",
        ("refresh_retrieval_projections", "ok") => "Search projections updated",
        ("refresh_retrieval_projections", "skipped") => "Search projections current",
        ("prune_manifest_snapshots", "starting") => "Pruning old snapshots...",
        ("prune_manifest_snapshots", "ok") => "Old snapshots pruned",
        ("prune_manifest_snapshots", "skipped") => "Snapshot retention unchanged",
        ("checkpoint_wal", "starting") => "Saving storage checkpoint...",
        ("checkpoint_wal", "ok") => "Storage checkpoint saved",
        ("checkpoint_wal", "skipped") => "Storage checkpoint skipped",
        ("checkpoint_wal", "warning") => "Storage checkpoint warning",
        _ => return human_title_token(phase),
    };
    title.to_owned()
}

fn human_index_phase_detail(
    fields: &[OutputField],
    semantic_mode: Option<SemanticRefreshMode>,
) -> Option<String> {
    let phase = field_value(fields, "phase").unwrap_or("work");
    let parts = match phase {
        "initialize_storage" => vec![duration_detail(fields)],
        "load_manifest" => vec![
            duration_detail(fields),
            field_value(fields, "previous")
                .filter(|value| *value != "-")
                .map(|snapshot| format!("previous {}", short_identifier(snapshot))),
            field_value(fields, "scanned")
                .filter(|value| *value != "0")
                .map(|files| format!("{files} previous files")),
        ],
        "build_manifest" => vec![
            duration_detail(fields),
            field_value(fields, "scanned").map(|files| format!("{files} scanned")),
            field_value(fields, "diagnostics")
                .filter(|value| *value != "0")
                .map(|diagnostics| format!("{diagnostics} diagnostics")),
        ],
        "build_plan" => vec![
            duration_detail(fields),
            human_delta(fields),
            semantic_records_detail(fields),
            snapshot_detail(fields),
        ],
        "persist_manifest_snapshot" | "refresh_retrieval_projections" => {
            vec![duration_detail(fields), snapshot_detail(fields)]
        }
        "checkpoint_wal" => vec![duration_detail(fields), storage_checkpoint_detail(fields)],
        "semantic_refresh" => vec![
            duration_detail(fields),
            semantic_records_detail(fields),
            semantic_mode_detail(fields, semantic_mode),
            semantic_path_delta(fields, semantic_mode),
            semantic_duration_per_doc_detail(fields),
        ],
        "prune_manifest_snapshots" => vec![
            duration_detail(fields),
            field_value(fields, "pruned_snapshots")
                .filter(|value| *value != "0")
                .map(|pruned| format!("{pruned} removed")),
        ],
        _ => vec![
            duration_detail(fields),
            human_file_counts(fields),
            snapshot_detail(fields),
        ],
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

fn semantic_scope_detail(fields: &[OutputField]) -> Option<String> {
    match field_value(fields, "mode").and_then(parse_semantic_refresh_mode) {
        Some(SemanticRefreshMode::FullRebuild) => Some("full rebuild".to_owned()),
        Some(SemanticRefreshMode::FullRebuildFromChangedOnly) => {
            Some("changed-only rebuild".to_owned())
        }
        Some(SemanticRefreshMode::ReuseExisting) => Some("snapshot current".to_owned()),
        Some(SemanticRefreshMode::Disabled) => Some("disabled".to_owned()),
        Some(SemanticRefreshMode::IncrementalAdvance) => human_delta(fields),
        None => human_delta(fields),
    }
}

fn semantic_mode_detail(
    fields: &[OutputField],
    semantic_mode: Option<SemanticRefreshMode>,
) -> Option<String> {
    let status = field_value(fields, "status");
    let mode = semantic_mode
        .or_else(|| field_value(fields, "semantic").and_then(parse_semantic_refresh_mode));
    match mode {
        Some(SemanticRefreshMode::FullRebuild) => Some("full rebuild".to_owned()),
        Some(SemanticRefreshMode::FullRebuildFromChangedOnly) => {
            Some("changed-only rebuild".to_owned())
        }
        Some(SemanticRefreshMode::ReuseExisting) if status != Some("skipped") => {
            Some("snapshot current".to_owned())
        }
        Some(SemanticRefreshMode::Disabled) if status != Some("skipped") => {
            Some("disabled".to_owned())
        }
        Some(SemanticRefreshMode::IncrementalAdvance)
            if semantic_zero_delta(fields) && semantic_zero_records(fields) =>
        {
            Some("metadata advance".to_owned())
        }
        _ => None,
    }
}

fn human_semantic_phase_title(
    fields: &[OutputField],
    semantic_mode: Option<SemanticRefreshMode>,
) -> String {
    let status = field_value(fields, "status").unwrap_or("starting");
    let mode = semantic_mode
        .or_else(|| field_value(fields, "semantic").and_then(parse_semantic_refresh_mode));
    let title = match (status, mode) {
        ("starting", Some(mode)) if mode.is_rebuild_like() => "Rebuilding semantic chunks...",
        ("starting", Some(SemanticRefreshMode::IncrementalAdvance))
            if semantic_zero_delta(fields) && semantic_zero_records(fields) =>
        {
            "Advancing semantic snapshot..."
        }
        ("starting", _) => "Embedding semantic chunks...",
        ("ok", Some(mode)) if mode.is_rebuild_like() => "Semantic chunks rebuilt",
        ("ok", Some(SemanticRefreshMode::IncrementalAdvance))
            if semantic_zero_delta(fields) && semantic_zero_records(fields) =>
        {
            "Semantic snapshot advanced"
        }
        ("ok", _) => "Semantic chunks stored",
        ("skipped", Some(SemanticRefreshMode::ReuseExisting)) => "Semantic snapshot current",
        ("skipped", Some(SemanticRefreshMode::Disabled)) => "Semantic refresh disabled",
        ("skipped", _) => "Semantic refresh skipped",
        _ => "Semantic refresh",
    };
    title.to_owned()
}

fn semantic_path_delta(
    fields: &[OutputField],
    semantic_mode: Option<SemanticRefreshMode>,
) -> Option<String> {
    let mode = semantic_mode
        .or_else(|| field_value(fields, "semantic").and_then(parse_semantic_refresh_mode));
    if mode.is_some_and(|mode| !mode.shows_path_delta()) {
        return None;
    }
    match (
        field_value(fields, "changed_paths"),
        field_value(fields, "deleted_paths"),
    ) {
        (Some(changed), Some(deleted)) => Some(format!("{changed} changed · {deleted} deleted")),
        _ => human_delta(fields),
    }
}

fn semantic_zero_delta(fields: &[OutputField]) -> bool {
    let fields = FieldBag::new(fields);
    let changed = fields
        .value("changed_paths")
        .or_else(|| fields.value("changed"));
    let deleted = fields
        .value("deleted_paths")
        .or_else(|| fields.value("deleted"));
    matches!((changed, deleted), (Some("0"), Some("0")))
}

fn semantic_zero_records(fields: &[OutputField]) -> bool {
    matches!(field_value(fields, "records"), None | Some("0"))
}

fn semantic_duration_per_doc_detail(fields: &[OutputField]) -> Option<String> {
    let duration_ms = field_value(fields, "duration_ms")?.parse::<u128>().ok()?;
    let records = field_value(fields, "records")?.parse::<u128>().ok()?;
    if records == 0 {
        return None;
    }
    Some(format!(
        "{}ms/doc",
        super::human_block::format_per_doc_duration(duration_ms, records)
    ))
}

fn snapshot_detail(fields: &[OutputField]) -> Option<String> {
    field_value(fields, "snapshot")
        .filter(|value| *value != "-")
        .map(|snapshot| format!("manifest {}", short_identifier(snapshot)))
}

fn storage_checkpoint_detail(fields: &[OutputField]) -> Option<String> {
    field_value(fields, "snapshot")
        .filter(|value| *value != "-")
        .map(|snapshot| format!("manifest checkpoint {}", short_identifier(snapshot)))
}

fn parse_semantic_refresh_mode(value: &str) -> Option<SemanticRefreshMode> {
    match value {
        "disabled" => Some(SemanticRefreshMode::Disabled),
        "full_rebuild" => Some(SemanticRefreshMode::FullRebuild),
        "full_rebuild_from_changed_only" => Some(SemanticRefreshMode::FullRebuildFromChangedOnly),
        "incremental_advance" => Some(SemanticRefreshMode::IncrementalAdvance),
        "reuse_existing" => Some(SemanticRefreshMode::ReuseExisting),
        _ => None,
    }
}
