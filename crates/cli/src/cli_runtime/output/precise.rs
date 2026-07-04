use frigg::human_output::HumanRow;

use super::human_block::{
    HumanMarkerKind, field_is, field_value, format_human_activity_line, format_human_component,
    format_human_component_with_marker, human_artifact_counts, human_field_label, human_generators,
    push_field_row, push_human_row, push_human_separator,
};
use super::{OutputField, OutputLevel};

pub(crate) fn format_human_precise_plan_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "repo", "repo");
    push_field_row(&mut rows, fields, "command", "command");
    push_human_row(&mut rows, "delta", super::human_block::human_delta(fields));
    push_human_row(&mut rows, "generators", human_generators(fields));

    let mut notes = Vec::new();
    if field_is(fields, "status", "empty") {
        notes.push("no generators need refresh".to_owned());
    }
    if field_is(fields, "status", "failed") {
        if let Some(error) = super::human_block::field_display(fields, "error") {
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

pub(crate) fn format_human_precise_run_component(
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

pub(crate) fn format_human_precise_generator_component(
    level: OutputLevel,
    fields: &[OutputField],
    path: Option<&str>,
    color: bool,
    width: usize,
) -> String {
    if field_is(fields, "status", "starting") {
        return format_human_precise_generator_start_line(level, fields, color, width);
    }
    let generator = field_value(fields, "generator")
        .or_else(|| field_value(fields, "tool"))
        .unwrap_or("generator");
    let mut rows = Vec::new();
    push_field_row(&mut rows, fields, "language", "language");
    push_field_row(&mut rows, fields, "generator", "generator");
    push_field_row(&mut rows, fields, "tool", "tool");
    push_precise_detail_rows(&mut rows, fields);
    push_human_separator(&mut rows);
    push_human_row(&mut rows, "output", human_artifact_counts(fields));
    push_field_row(&mut rows, fields, "duration_ms", "duration");
    push_field_row(&mut rows, fields, "next", "next");

    format_human_component_with_marker(
        level,
        fields,
        &format!("Precise {generator}"),
        rows,
        Vec::new(),
        path,
        HumanMarkerKind::Checkpoint,
        Some(super::human_topic::HumanTopic::Precise),
        color,
        width,
    )
}

fn format_human_precise_generator_start_line(
    level: OutputLevel,
    fields: &[OutputField],
    color: bool,
    width: usize,
) -> String {
    let tool = field_value(fields, "tool")
        .or_else(|| field_value(fields, "generator"))
        .unwrap_or("generator");
    let details = field_value(fields, "language")
        .filter(|language| *language != tool)
        .map(|language| format!("{language} generator"))
        .unwrap_or_default();
    format_human_activity_line(
        level,
        fields,
        format!("Running {tool}..."),
        details,
        HumanMarkerKind::Progress,
        Some(super::human_topic::HumanTopic::Precise),
        color,
        width,
    )
}

fn push_precise_detail_rows(rows: &mut Vec<HumanRow>, fields: &[OutputField]) {
    let structured_version =
        field_value(fields, "version").filter(|value| !value.is_empty() && *value != "-");
    if let Some(version) = structured_version {
        rows.push(HumanRow::kv("version", version));
    }

    let Some(detail) = field_value(fields, "detail") else {
        return;
    };
    let mut parsed = false;
    let mut pushed = false;
    for (key, value) in parse_detail_key_values(detail) {
        parsed = true;
        if matches!(key.as_str(), "generator" | "tool")
            || (key == "version" && structured_version.is_some())
        {
            continue;
        }
        push_human_row(rows, &human_field_label(&key), Some(value));
        pushed = true;
    }
    if !parsed {
        push_field_row(rows, fields, "detail", "detail");
    } else if !pushed && structured_version.is_none() {
        push_field_row(rows, fields, "detail", "detail");
    }
}

fn parse_detail_key_values(detail: &str) -> Vec<(String, String)> {
    let spans = detail_key_spans(detail);
    if spans.is_empty() {
        return Vec::new();
    }
    let first_non_whitespace = detail
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(0);
    if spans
        .first()
        .is_some_and(|(key_start, _, _)| *key_start != first_non_whitespace)
    {
        return Vec::new();
    }

    spans
        .iter()
        .enumerate()
        .filter_map(|(index, (_key_start, value_start, key))| {
            let next_key_start = spans
                .get(index + 1)
                .map(|(next_key_start, _, _)| *next_key_start)
                .unwrap_or(detail.len());
            let value_end = detail[..next_key_start].trim_end().len();
            let value = detail[*value_start..value_end].trim();
            (!value.is_empty()).then(|| (key.clone(), value.to_owned()))
        })
        .collect()
}

fn detail_key_spans(detail: &str) -> Vec<(usize, usize, String)> {
    let bytes = detail.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len() && is_detail_key_byte(bytes[index]) {
            index += 1;
        }
        if index > key_start && index < bytes.len() && bytes[index] == b'=' {
            let key = detail[key_start..index].to_owned();
            spans.push((key_start, index + 1, key));
            index += 1;
        } else {
            index = key_start.saturating_add(1);
        }
    }
    spans
}

fn is_detail_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}
