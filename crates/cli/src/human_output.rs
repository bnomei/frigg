//! Shared human-output block primitives used by CLI-facing renderers.

use std::fmt::Display;

const HUMAN_CARD_TITLE_PREFIX: &str = "╭─";
const HUMAN_CARD_ROW_PREFIX: &str = "│   ";
const HUMAN_CARD_FOOTER_PREFIX: &str = "╰─╮ ";
const HUMAN_CARD_MAX_LABEL_WIDTH: usize = 28;
const HUMAN_BADGE_STYLE: &str = "1;97;48;5;240";
const HUMAN_BADGE_WIDTH: usize = 3;
const HUMAN_BADGE_SEPARATOR_WIDTH: usize = 1;

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
    badge: Option<String>,
    badge_mode: HumanBadgeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HumanBadgeMode {
    None,
    FirstLine,
    TextThenEmpty,
    EmptyColumn,
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
            badge: None,
            badge_mode: HumanBadgeMode::None,
        }
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        let badge = badge.into();
        if !badge.trim().is_empty() {
            self.badge = Some(badge);
            self.badge_mode = HumanBadgeMode::FirstLine;
        }
        self
    }

    pub fn with_badge_column(mut self, badge: impl Into<String>) -> Self {
        let badge = badge.into();
        if !badge.trim().is_empty() {
            self.badge = Some(badge);
            self.badge_mode = HumanBadgeMode::TextThenEmpty;
        }
        self
    }

    pub fn with_empty_badge_column(mut self) -> Self {
        self.badge = None;
        self.badge_mode = HumanBadgeMode::EmptyColumn;
        self
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
        let title_badge = match self.badge_mode {
            HumanBadgeMode::None => None,
            HumanBadgeMode::FirstLine | HumanBadgeMode::TextThenEmpty => self.badge.as_deref(),
            HumanBadgeMode::EmptyColumn => Some(""),
        };
        let badge_prefix_width = human_badge_prefix_width(title_badge);
        let title = truncate_display(&self.title, width.saturating_sub(4 + badge_prefix_width));
        let title_line = colorize(
            color,
            &self.accent,
            format!("{HUMAN_CARD_TITLE_PREFIX}{} {title}", self.marker),
        );
        let mut output = format_human_badged_line(title_badge, title_line, color);
        if rows.is_empty() {
            return output;
        }

        let row_badge = match self.badge_mode {
            HumanBadgeMode::TextThenEmpty | HumanBadgeMode::EmptyColumn => Some(""),
            HumanBadgeMode::None | HumanBadgeMode::FirstLine => None,
        };
        let row_width = width.saturating_sub(human_badge_prefix_width(row_badge));
        let label_width = human_card_label_width(&rows, row_width, min_label_width);
        let content_count = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            let row_prefix = if index + 1 == content_count {
                HUMAN_CARD_FOOTER_PREFIX
            } else {
                HUMAN_CARD_ROW_PREFIX
            };
            output.push('\n');
            let row_line = match row {
                RenderRow::Kv { label, value } => format_human_kv_row(
                    &label,
                    &value,
                    label_width,
                    row_prefix,
                    &self.rail_accent,
                    color,
                    row_width,
                ),
                RenderRow::Note(note) => {
                    format_human_kv_note(&note, row_prefix, &self.rail_accent, color, row_width)
                }
            };
            output.push_str(&format_human_badged_line(row_badge, row_line, color));
        }
        output
    }
}

pub fn human_badge_prefix_width(badge: Option<&str>) -> usize {
    if badge.is_some() {
        HUMAN_BADGE_WIDTH + HUMAN_BADGE_SEPARATOR_WIDTH
    } else {
        0
    }
}

pub fn format_human_badged_line(
    badge: Option<&str>,
    line: impl Into<String>,
    color: bool,
) -> String {
    let line = line.into();
    let Some(badge) = badge else {
        return line;
    };
    let badge = truncate_display(badge.trim(), 3);
    let badge = format!("{badge:<HUMAN_BADGE_WIDTH$}");
    let badge = if color {
        colorize(color, HUMAN_BADGE_STYLE, badge)
    } else {
        badge
    };
    format!("{badge} {line}")
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
