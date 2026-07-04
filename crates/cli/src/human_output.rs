//! Shared human-output block primitives used by CLI-facing renderers.

use std::fmt::Display;

const HUMAN_CARD_TITLE_PREFIX: &str = "╭─";
const HUMAN_CARD_ROW_PREFIX: &str = "│   ";
const HUMAN_CARD_FOOTER_PREFIX: &str = "╰─╮ ";
const HUMAN_CARD_MAX_LABEL_WIDTH: usize = 28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanRow {
    Kv { label: String, value: String },
    Note(String),
    Separator,
    Path(String),
}

impl HumanRow {
    pub fn kv(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Kv {
            label: label.into(),
            value: value.into(),
        }
    }

    pub fn note(value: impl Into<String>) -> Self {
        Self::Note(value.into())
    }

    pub fn path(value: impl Into<String>) -> Self {
        Self::Path(value.into())
    }

    fn into_render_row(self) -> Option<RenderRow> {
        match self {
            Self::Kv { label, value } => Some(RenderRow::Kv { label, value }),
            Self::Note(note) if !note.trim().is_empty() => Some(RenderRow::Note(note)),
            Self::Path(path) => {
                let (label, value) = human_path_row(&path);
                Some(RenderRow::Kv { label, value })
            }
            Self::Separator | Self::Note(_) => None,
        }
    }

    fn is_separator(&self) -> bool {
        matches!(self, Self::Separator)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanBlock {
    title: String,
    rows: Vec<HumanRow>,
    marker: String,
    accent: String,
    rail_accent: String,
}

impl HumanBlock {
    pub fn new(
        title: impl Into<String>,
        rows: Vec<HumanRow>,
        marker: impl Into<String>,
        accent: impl Into<String>,
        rail_accent: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            rows,
            marker: marker.into(),
            accent: accent.into(),
            rail_accent: rail_accent.into(),
        }
    }

    pub fn render(self, color: bool, width: usize) -> String {
        self.render_with_min_label_width(color, width, None)
    }

    pub fn render_with_min_label_width(
        self,
        color: bool,
        width: usize,
        min_label_width: Option<usize>,
    ) -> String {
        let mut rows = self.rows;
        trim_human_separators(&mut rows);
        let rows = rows
            .into_iter()
            .filter_map(HumanRow::into_render_row)
            .collect::<Vec<_>>();
        let title = truncate_display(&self.title, width.saturating_sub(4));
        let mut output = colorize(
            color,
            &self.accent,
            format!("{HUMAN_CARD_TITLE_PREFIX}{} {title}", self.marker),
        );
        if rows.is_empty() {
            return output;
        }

        let label_width = human_card_label_width(&rows, width, min_label_width);
        let content_count = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            let row_prefix = if index + 1 == content_count {
                HUMAN_CARD_FOOTER_PREFIX
            } else {
                HUMAN_CARD_ROW_PREFIX
            };
            output.push('\n');
            match row {
                RenderRow::Kv { label, value } => output.push_str(&format_human_kv_row(
                    &label,
                    &value,
                    label_width,
                    row_prefix,
                    &self.rail_accent,
                    color,
                    width,
                )),
                RenderRow::Note(note) => output.push_str(&format_human_kv_note(
                    &note,
                    row_prefix,
                    &self.rail_accent,
                    color,
                    width,
                )),
            }
        }
        output
    }
}

enum RenderRow {
    Kv { label: String, value: String },
    Note(String),
}

fn trim_human_separators(rows: &mut Vec<HumanRow>) {
    while rows.first().is_some_and(HumanRow::is_separator) {
        rows.remove(0);
    }
    while rows.last().is_some_and(HumanRow::is_separator) {
        rows.pop();
    }
}

fn human_path_row(value: &str) -> (String, String) {
    match value.trim() {
        "." | "./" => ("workspace".to_owned(), "current directory (.)".to_owned()),
        trimmed if trimmed.is_empty() => ("path".to_owned(), "-".to_owned()),
        _ => ("path".to_owned(), value.to_owned()),
    }
}

fn human_card_label_width(
    rows: &[RenderRow],
    width: usize,
    min_label_width: Option<usize>,
) -> usize {
    let longest = rows
        .iter()
        .filter_map(|row| match row {
            RenderRow::Kv { label, .. } => Some(display_width(label)),
            RenderRow::Note(_) => None,
        })
        .max()
        .unwrap_or(1);
    let max_for_value_column = width
        .saturating_sub(display_width(HUMAN_CARD_ROW_PREFIX) + 1 + 8)
        .max(1);
    longest
        .max(min_label_width.unwrap_or(1))
        .min(HUMAN_CARD_MAX_LABEL_WIDTH)
        .min(max_for_value_column)
        .max(1)
}

fn format_human_kv_row(
    label: &str,
    value: &str,
    label_width: usize,
    row_prefix: &str,
    accent: &str,
    color: bool,
    width: usize,
) -> String {
    let label = truncate_display(label, label_width);
    let value_prefix_width = display_width(row_prefix) + label_width + 1;
    let value = truncate_display(value, width.saturating_sub(value_prefix_width));
    format!(
        "{}{} {}",
        colorize(color, accent, row_prefix),
        colorize(color, "2", format!("{label:<label_width$}")),
        value
    )
}

fn format_human_kv_note(
    note: &str,
    row_prefix: &str,
    accent: &str,
    color: bool,
    width: usize,
) -> String {
    let budget = width.saturating_sub(display_width(row_prefix));
    format!(
        "{}{}",
        colorize(color, accent, row_prefix),
        colorize(color, "2", truncate_display(note, budget))
    )
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
