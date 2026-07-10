//! Low-level terminal block rendering for Frigg's human CLI output.
//!
//! Width budgeting, color accents, badges, continuation lines, and row cleanup live here so
//! command-specific formatters can pass semantic rows instead of terminal layout details.

use std::fmt::Display;

use frigg::human_output::{
    HumanBlock, HumanRow, format_human_badged_line, human_badge_prefix_width,
};

use super::fields::FieldBag;
use super::human_topic::{
    HUMAN_COLOR_NEUTRAL, HumanTopic, action_color, human_severity_accent_color,
    human_sidecar_line_color, human_title_accent_color,
};
use super::{OutputField, OutputLevel};

pub(crate) const HUMAN_DEFAULT_WIDTH: usize = 100;
pub(crate) const HUMAN_MIN_WIDTH: usize = 48;
pub(crate) const HUMAN_TEXT_COLUMN: usize = 4;
pub(crate) const HUMAN_ACTIVITY_PREFIX: &str = "  ";
const HUMAN_DETAIL_PREFIX: &str = "    ";
const HUMAN_DETAIL_RAIL_PREFIX: &str = "  │ ";
const HUMAN_CONTINUATION_MARKER: &str = "└─ ";
const HUMAN_SESSION_FIELD: &str = "session";
const HUMAN_SERVER_BADGE: &str = "SRV";
const HUMAN_INTRO_MARKER: &str = "◆";
const HUMAN_INTRO_TAGLINE: &str = "Local, source-backed code search and navigation for AI agents.";

/// Glyph/style family for human-mode component markers (metadata, progress, checkpoint, tool).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HumanMarkerKind {
    Metadata,
    Progress,
    Checkpoint,
    Tool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn format_human_component(
    level: OutputLevel,
    fields: &[OutputField],
    title: &str,
    rows: Vec<HumanRow>,
    notes: Vec<String>,
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    format_human_component_with_marker(
        level,
        fields,
        title,
        rows,
        notes,
        path,
        HumanMarkerKind::Metadata,
        None,
        color,
        width,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn format_human_progress_component(
    level: OutputLevel,
    fields: &[OutputField],
    title: &str,
    rows: Vec<HumanRow>,
    notes: Vec<String>,
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    format_human_component_with_marker(
        level,
        fields,
        title,
        rows,
        notes,
        path,
        HumanMarkerKind::Progress,
        None,
        color,
        width,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn format_human_component_with_marker(
    level: OutputLevel,
    fields: &[OutputField],
    title: &str,
    mut rows: Vec<HumanRow>,
    notes: Vec<String>,
    path: Option<&str>,
    marker_kind: HumanMarkerKind,
    topic: Option<HumanTopic>,
    color: bool,
    width: usize,
) -> String {
    trim_human_separators(&mut rows);
    if let Some(path) = path {
        rows.push(HumanRow::path(path));
    }
    rows.extend(notes.into_iter().map(HumanRow::note));

    let has_rows = rows.iter().any(|row| match row {
        HumanRow::Separator => false,
        HumanRow::Note(note) => !note.trim().is_empty(),
        HumanRow::Kv { value, .. } => !value.is_empty() && value != "-",
        HumanRow::Path(path) => !path.trim().is_empty(),
    });
    if !has_rows {
        return format_human_activity_line(
            level,
            fields,
            title.to_owned(),
            String::new(),
            marker_kind,
            topic,
            color,
            width,
        );
    }

    let block_marker = human_block_marker_kind(marker_kind);
    format_human_kv_block(
        level,
        fields,
        title,
        rows,
        block_marker,
        topic,
        color,
        width,
    )
}

fn human_block_marker_kind(marker_kind: HumanMarkerKind) -> HumanMarkerKind {
    match marker_kind {
        HumanMarkerKind::Progress => HumanMarkerKind::Metadata,
        marker_kind => marker_kind,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn format_human_kv_block(
    level: OutputLevel,
    fields: &[OutputField],
    title: &str,
    rows: Vec<HumanRow>,
    marker_kind: HumanMarkerKind,
    topic: Option<HumanTopic>,
    color: bool,
    width: usize,
) -> String {
    let fields = FieldBag::new(fields);
    let accent = human_title_accent_color(level, fields, topic, title);
    let rail_accent = human_sidecar_line_color(fields, topic, title, accent);
    let marker = human_symbol(level, fields, marker_kind);
    format_human_kv_block_with_marker_and_badge(
        title,
        rows,
        marker,
        accent,
        rail_accent,
        human_session_badge(fields),
        color,
        width,
    )
}

#[cfg(test)]
pub(crate) fn format_human_kv_block_with_marker(
    title: &str,
    rows: Vec<HumanRow>,
    marker: &str,
    accent: &str,
    rail_accent: &str,
    color: bool,
    width: usize,
) -> String {
    format_human_kv_block_with_marker_and_badge(
        title,
        rows,
        marker,
        accent,
        rail_accent,
        None,
        color,
        width,
    )
}

#[allow(clippy::too_many_arguments)]
fn format_human_kv_block_with_marker_and_badge(
    title: &str,
    rows: Vec<HumanRow>,
    marker: &str,
    accent: &str,
    rail_accent: &str,
    badge: Option<String>,
    color: bool,
    width: usize,
) -> String {
    let mut block = HumanBlock::new(title, rows, marker, accent, rail_accent);
    if let Some(badge) = badge {
        block = if badge == HUMAN_SERVER_BADGE {
            block.with_badge_column(badge)
        } else {
            block.with_badge(badge)
        };
    }
    block.render(color, width)
}

pub(crate) fn format_human_card(
    level: OutputLevel,
    title: &str,
    mut rows: Vec<HumanRow>,
    _footer: Option<&str>,
    fields: &[OutputField],
    color: bool,
    width: usize,
) -> String {
    trim_human_separators(&mut rows);
    let fields = FieldBag::new(fields);
    let accent = human_title_accent_color(level, fields, None, title);
    let rail_accent = human_sidecar_line_color(fields, None, title, accent);
    format_human_kv_block_with_marker_and_badge(
        title,
        rows,
        human_card_marker(level),
        accent,
        rail_accent,
        human_session_badge(fields),
        color,
        width,
    )
}

fn human_card_marker(level: OutputLevel) -> &'static str {
    match level {
        OutputLevel::Ok => "●",
        OutputLevel::Info | OutputLevel::Skip => "○",
        OutputLevel::Warn => "▲",
        OutputLevel::Error => "×",
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn format_human_activity_line(
    level: OutputLevel,
    fields: &[OutputField],
    title: String,
    details: String,
    marker_kind: HumanMarkerKind,
    topic: Option<HumanTopic>,
    color: bool,
    width: usize,
) -> String {
    let fields = FieldBag::new(fields);
    let accent = if marker_kind == HumanMarkerKind::Tool {
        human_severity_accent_color(level, fields.value("status").unwrap_or_default())
            .unwrap_or(HUMAN_COLOR_NEUTRAL)
    } else {
        human_title_accent_color(level, fields, topic, &title)
    };
    let badge = human_session_badge(fields);
    let badge_prefix_width = human_badge_prefix_width(badge.as_deref());
    let symbol = human_symbol_with_color(level, fields, marker_kind, color, accent);
    let content_width = width.saturating_sub(HUMAN_TEXT_COLUMN + badge_prefix_width);
    let title = truncate_display(&title, content_width);
    let mut line = format!(
        "{HUMAN_ACTIVITY_PREFIX}{symbol} {}",
        colorize(color, accent, &title)
    );
    let used_width = badge_prefix_width + HUMAN_TEXT_COLUMN + display_width(&title);
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

pub(crate) fn format_human_continuation(
    value: &str,
    detail_rail: bool,
    color: bool,
    width: usize,
) -> String {
    let value = human_continuation_value(value);
    let budget = width.saturating_sub(human_continuation_prefix_width(detail_rail));
    let value = truncate_display(&value, budget);
    format!(
        "{}{}{}",
        human_detail_prefix(detail_rail, color),
        HUMAN_CONTINUATION_MARKER,
        colorize(color, "2", value)
    )
}

fn human_detail_prefix(detail_rail: bool, color: bool) -> String {
    if detail_rail {
        return format!("  {} ", colorize(color, "2", "│"));
    }
    HUMAN_DETAIL_PREFIX.to_owned()
}

fn human_detail_prefix_width(detail_rail: bool) -> usize {
    display_width(if detail_rail {
        HUMAN_DETAIL_RAIL_PREFIX
    } else {
        HUMAN_DETAIL_PREFIX
    })
}

fn human_continuation_prefix_width(detail_rail: bool) -> usize {
    human_detail_prefix_width(detail_rail) + display_width(HUMAN_CONTINUATION_MARKER)
}

pub(crate) fn push_field_row(
    rows: &mut Vec<HumanRow>,
    fields: &[OutputField],
    key: &str,
    label: &str,
) {
    push_human_row(rows, label, field_display(fields, key));
}

pub(crate) fn push_human_row(rows: &mut Vec<HumanRow>, label: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.is_empty() && value != "-") {
        rows.push(HumanRow::kv(label, value));
    }
}

pub(crate) fn push_human_separator(rows: &mut Vec<HumanRow>) {
    if rows
        .last()
        .is_some_and(|row| matches!(row, HumanRow::Separator))
    {
        return;
    }
    rows.push(HumanRow::Separator);
}

pub(crate) fn trim_human_separators(rows: &mut Vec<HumanRow>) {
    while rows
        .first()
        .is_some_and(|row| matches!(row, HumanRow::Separator))
    {
        rows.remove(0);
    }
    while rows
        .last()
        .is_some_and(|row| matches!(row, HumanRow::Separator))
    {
        rows.pop();
    }
}

pub(crate) fn human_rows_from_fields(fields: &[OutputField], skip_keys: &[&str]) -> Vec<HumanRow> {
    FieldBag::new(fields)
        .iter()
        .filter(|field| !human_metadata_field_skipped(field.key, skip_keys))
        .map(|field| {
            HumanRow::kv(
                human_field_label(field.key),
                human_field_value(field.key, &field.value),
            )
        })
        .collect()
}

pub(crate) fn human_init_complete_rows(fields: &[OutputField]) -> Vec<HumanRow> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repos", "repos");
    push_human_row(&mut rows, "precise", human_precise_counts(fields));
    rows
}

pub(crate) fn human_repair_complete_rows(fields: &[OutputField]) -> Vec<HumanRow> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repos", "repos");
    push_field_row(&mut rows, fields, "repaired", "repaired");
    rows
}

pub(crate) fn human_prune_complete_rows(fields: &[OutputField]) -> Vec<HumanRow> {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repos", "repos");
    push_field_row(&mut rows, fields, "keep_manifest_snapshots", "keep");
    push_field_row(&mut rows, fields, "manifest_snapshots_deleted", "deleted");
    rows
}

pub(crate) fn field_display(fields: &[OutputField], key: &str) -> Option<String> {
    FieldBag::new(fields)
        .value(key)
        .map(|value| human_field_value(key, value))
}

pub(crate) fn field_value<'a>(fields: &'a [OutputField], key: &str) -> Option<&'a str> {
    FieldBag::new(fields).value(key)
}

pub(crate) fn field_is(fields: &[OutputField], key: &str, value: &str) -> bool {
    FieldBag::new(fields).is(key, value)
}

pub(crate) fn human_delta(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let changed = fields.value("changed")?;
    let deleted = fields.value("deleted")?;
    Some(format!("{changed} changed · {deleted} deleted"))
}

pub(crate) fn human_provider_model(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let provider = fields.value("provider").filter(|value| *value != "-");
    let model = fields.value("model").filter(|value| *value != "-");
    match (provider, model) {
        (Some(provider), Some(model)) => Some(format!("{provider} · {model}")),
        (Some(provider), None) => Some(provider.to_owned()),
        (None, Some(model)) => Some(model.to_owned()),
        (None, None) => None,
    }
}

pub(crate) fn human_file_counts(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let mut parts = Vec::new();
    if let Some(scanned) = fields.value("scanned") {
        parts.push(format!("{scanned} scanned"));
    }
    if let Some(changed) = fields.value("changed") {
        parts.push(format!("{changed} changed"));
    }
    if let Some(deleted) = fields.value("deleted") {
        parts.push(format!("{deleted} deleted"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub(crate) fn human_diagnostics(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let diagnostics = fields.value("diagnostics")?;
    let walk = fields.value("diagnostics_walk").unwrap_or("0");
    let read = fields.value("diagnostics_read").unwrap_or("0");
    Some(format!("{diagnostics} total · {walk} walk · {read} read"))
}

pub(crate) fn human_precise_counts(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let generators = fields.value("precise_generators")?;
    let succeeded = fields.value("precise_succeeded").unwrap_or("0");
    let failed = fields.value("precise_failed").unwrap_or("0");
    let missing = fields.value("precise_missing_tool").unwrap_or("0");
    let skipped = fields.value("precise_skipped").unwrap_or("0");
    Some(format!(
        "{generators} generators · {succeeded} ok · {failed} failed · {missing} missing · {skipped} skipped"
    ))
}

pub(crate) fn human_generators(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let count = fields.value("generators")?;
    let ids = fields
        .value("generator_ids")
        .filter(|ids| !ids.is_empty() && *ids != "-")
        .map(|ids| format!(" · {ids}"))
        .unwrap_or_default();
    Some(format!("{count}{ids}"))
}

pub(crate) fn human_artifact_counts(fields: &[OutputField]) -> Option<String> {
    let fields = FieldBag::new(fields);
    let artifacts = fields.value("artifacts")?;
    let bytes = fields.value("bytes").unwrap_or("0");
    Some(format!("{artifacts} artifacts · {bytes} bytes"))
}

pub(crate) fn duration_detail(fields: &[OutputField]) -> Option<String> {
    field_display(fields, "duration_ms")
}

pub(crate) fn format_per_doc_duration(duration_ms: u128, records: u128) -> String {
    if duration_ms == 0 {
        return "<1".to_owned();
    }
    let tenths = ((duration_ms * 10) + (records / 2)) / records;
    if tenths == 0 {
        return "<0.1".to_owned();
    }
    if tenths >= 1000 || tenths.is_multiple_of(10) {
        (tenths / 10).to_string()
    } else {
        format!("{}.{:01}", tenths / 10, tenths % 10)
    }
}

pub(crate) fn human_mode_label(value: &str) -> String {
    value.replace('_', " ")
}

pub(crate) fn short_identifier(value: &str) -> String {
    if value == "-" || value.chars().count() <= 16 {
        return value.to_owned();
    }
    let prefix = value.chars().take(12).collect::<String>();
    format!("{prefix}…")
}

pub(crate) fn compact_human_fields(
    fields: &[OutputField],
    skip_keys: &[&str],
    limit: usize,
) -> String {
    let mut fields = FieldBag::new(fields)
        .iter()
        .enumerate()
        .filter(|(_, field)| !human_metadata_field_skipped(field.key, skip_keys))
        .collect::<Vec<_>>();
    fields.sort_by_key(|(index, field)| (compact_human_field_order(field.key), *index));

    fields
        .into_iter()
        .take(limit)
        .map(|(_, field)| {
            format!(
                "{}={}",
                compact_human_field_key(field.key),
                truncate_display(&human_field_value(field.key, &field.value), 80)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn human_metadata_field_skipped(key: &str, skip_keys: &[&str]) -> bool {
    key == HUMAN_SESSION_FIELD || skip_keys.contains(&key)
}

fn compact_human_field_order(key: &str) -> u8 {
    if key.ends_with("_ms") { 0 } else { 1 }
}

pub(crate) fn human_session_badge(fields: FieldBag<'_>) -> Option<String> {
    fields
        .value(HUMAN_SESSION_FIELD)
        .and_then(human_session_badge_label)
        .or_else(|| Some(HUMAN_SERVER_BADGE.to_owned()))
}

pub(crate) fn human_session_badge_label(session_id: &str) -> Option<String> {
    let session_id = session_id.trim();
    if session_id.is_empty() || session_id == "-" {
        return None;
    }
    let mut chars = session_id.chars().rev().take(3).collect::<Vec<_>>();
    chars.reverse();
    Some(chars.into_iter().collect::<String>())
}

pub(crate) fn human_field_label(key: &str) -> String {
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

pub(crate) fn human_field_value(key: &str, value: &str) -> String {
    if key.ends_with("_ms") && value.chars().all(|ch| ch.is_ascii_digit()) {
        return format!("{value}ms");
    }
    value.to_owned()
}

pub(crate) fn human_event_title(area: &str, event: &str) -> String {
    match (area, event) {
        ("index", "plan") => "Index plan".to_owned(),
        ("index", "semantic") => "Semantic refresh".to_owned(),
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

pub(crate) fn human_title_token(token: &str) -> String {
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

pub(crate) fn human_symbol_with_color(
    level: OutputLevel,
    fields: FieldBag<'_>,
    marker_kind: HumanMarkerKind,
    color: bool,
    color_code: &str,
) -> String {
    colorize(color, color_code, human_symbol(level, fields, marker_kind))
}

pub(crate) fn human_uses_detail_rail(
    level: OutputLevel,
    fields: &[OutputField],
    marker_kind: HumanMarkerKind,
) -> bool {
    human_symbol(level, FieldBag::new(fields), marker_kind) == "│"
}

pub(crate) fn human_symbol(
    level: OutputLevel,
    fields: FieldBag<'_>,
    marker_kind: HumanMarkerKind,
) -> &'static str {
    if marker_kind == HumanMarkerKind::Tool {
        return "◇";
    }
    let status = fields.value("status").unwrap_or_default();
    match (level, status) {
        (OutputLevel::Error, _) | (_, "failed") | (_, "blocked") => "×",
        (OutputLevel::Warn, _) | (_, "retry") | (_, "stale") => "▲",
        (OutputLevel::Skip, _) | (_, "empty") | (_, "skipped") | (_, "fresh") => "○",
        _ if marker_kind == HumanMarkerKind::Metadata => "○",
        (_, "ok") | (_, "finished") | (_, "listening") => "●",
        (_, "starting" | "started" | "queued" | "enabled")
            if marker_kind == HumanMarkerKind::Progress =>
        {
            "│"
        }
        (_, "starting" | "started" | "queued" | "enabled") => "○",
        _ => match level {
            OutputLevel::Ok => "●",
            OutputLevel::Info => "●",
            OutputLevel::Warn => "▲",
            OutputLevel::Error => "×",
            OutputLevel::Skip => "○",
        },
    }
}

pub(crate) fn colorize_action_title(
    color: bool,
    action: &str,
    title_prefix: &str,
    accent: &str,
) -> String {
    let action_title = human_title_token(action);
    match title_prefix.strip_suffix(&action_title) {
        Some(label_prefix) => format!(
            "{}{}",
            colorize(color, accent, label_prefix),
            colorize(color, action_color(action), &action_title)
        ),
        None => colorize(color, action_color(action), title_prefix),
    }
}

pub(crate) fn colorize(color: bool, code: &str, value: impl Display) -> String {
    if color {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.to_string()
    }
}

pub(crate) fn truncate_display(value: &str, max_chars: usize) -> String {
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

pub(crate) fn display_width(value: &str) -> usize {
    value.chars().count()
}

pub(crate) fn human_terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= HUMAN_MIN_WIDTH)
        .unwrap_or(HUMAN_DEFAULT_WIDTH)
}

pub(crate) fn human_color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

pub(crate) fn format_human_intro(color: bool) -> String {
    let mut output = HumanBlock::new(
        "Frigg",
        vec![HumanRow::note(HUMAN_INTRO_TAGLINE)],
        HUMAN_INTRO_MARKER,
        HUMAN_COLOR_NEUTRAL,
        HUMAN_COLOR_NEUTRAL,
    )
    .with_empty_badge_column()
    .render(color, HUMAN_DEFAULT_WIDTH);
    output.push('\n');
    output
}

fn human_continuation_value(value: &str) -> String {
    match value.trim() {
        "." | "./" => "workspace: current directory (.)".to_owned(),
        "" => "path: -".to_owned(),
        _ => format!("path: {value}"),
    }
}
