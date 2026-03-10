use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use regex::Regex;

use crate::mcp::types::{ExploreAnchor, ExploreCursor, ExploreLineWindow};

pub(crate) const DEFAULT_CONTEXT_LINES: usize = 3;
pub(crate) const MAX_CONTEXT_LINES: usize = 32;
pub(crate) const DEFAULT_MAX_MATCHES: usize = 8;

#[derive(Debug)]
pub(crate) enum LossyLineSliceError {
    Io(io::Error),
    LineStartOutside {
        line_start: usize,
        line_end: Option<usize>,
        total_lines: usize,
    },
}

impl From<io::Error> for LossyLineSliceError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LossyLineSlice {
    pub content: String,
    pub bytes: usize,
    pub total_lines: usize,
    pub lossy_utf8: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ExploreMatcher {
    Literal(String),
    Regex(Regex),
}

impl ExploreMatcher {
    pub(crate) fn find_spans(&self, line: &str) -> Vec<(usize, usize)> {
        match self {
            Self::Literal(query) => line
                .match_indices(query)
                .map(|(start, matched)| (start, start + matched.len()))
                .collect(),
            Self::Regex(regex) => regex
                .find_iter(line)
                .map(|matched| (matched.start(), matched.end()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExploreScopeRequest {
    pub start_line: usize,
    pub end_line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExploreSpanMatch {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
    pub anchor: ExploreAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExploreScanResult {
    pub total_lines: usize,
    pub effective_scope: ExploreLineWindow,
    pub scope_content: Option<String>,
    pub scope_bytes: Option<usize>,
    pub scope_within_budget: bool,
    pub total_matches: usize,
    pub matches: Vec<ExploreSpanMatch>,
    pub truncated: bool,
    pub resume_from: Option<ExploreCursor>,
    pub lossy_utf8: bool,
}

pub(crate) fn validate_anchor(anchor: &ExploreAnchor) -> Result<(), &'static str> {
    if anchor.start_line == 0 || anchor.end_line == 0 {
        return Err("anchor line positions must be greater than zero");
    }
    if anchor.start_column == 0 || anchor.end_column == 0 {
        return Err("anchor column positions must be greater than zero");
    }
    if anchor.end_line < anchor.start_line {
        return Err("anchor end_line must be greater than or equal to start_line");
    }
    if anchor.start_line == anchor.end_line && anchor.end_column < anchor.start_column {
        return Err("anchor end_column must be greater than or equal to start_column");
    }
    Ok(())
}

pub(crate) fn validate_cursor(cursor: &ExploreCursor) -> Result<(), &'static str> {
    if cursor.line == 0 {
        return Err("resume_from.line must be greater than zero");
    }
    if cursor.column == 0 {
        return Err("resume_from.column must be greater than zero");
    }
    Ok(())
}

pub(crate) fn line_window_around_anchor(
    anchor: &ExploreAnchor,
    context_lines: usize,
) -> ExploreLineWindow {
    ExploreLineWindow {
        start_line: anchor.start_line.saturating_sub(context_lines).max(1),
        end_line: anchor.end_line.saturating_add(context_lines),
    }
}

pub(crate) fn read_line_slice_lossy(
    path: &Path,
    line_start: usize,
    line_end: Option<usize>,
    max_bytes: usize,
) -> Result<LossyLineSlice, LossyLineSliceError> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut raw_line = Vec::new();
    let mut content = String::new();
    let mut total_lines = 0usize;
    let mut sliced_bytes = 0usize;
    let mut exceeded_limit = false;
    let mut lossy_utf8 = false;
    let mut first_selected_line = true;

    loop {
        raw_line.clear();
        let bytes_read = reader.read_until(b'\n', &mut raw_line)?;
        if bytes_read == 0 {
            break;
        }

        total_lines = total_lines.saturating_add(1);
        let include_line = total_lines >= line_start
            && line_end.is_none_or(|effective_end| total_lines <= effective_end);
        if !include_line {
            if line_end.is_some_and(|effective_end| total_lines >= effective_end) {
                break;
            }
            continue;
        }

        let (normalized_line, had_lossy_utf8) = normalize_lossy_line_bytes(&raw_line);
        lossy_utf8 |= had_lossy_utf8;
        if !first_selected_line {
            sliced_bytes = sliced_bytes.saturating_add(1);
            if !exceeded_limit {
                content.push('\n');
            }
        }
        sliced_bytes = sliced_bytes.saturating_add(normalized_line.len());
        if sliced_bytes > max_bytes {
            exceeded_limit = true;
        }
        if !exceeded_limit {
            content.push_str(&normalized_line);
        }
        first_selected_line = false;

        if line_end.is_some_and(|effective_end| total_lines >= effective_end) {
            break;
        }
    }

    if total_lines > 0 && line_start > total_lines {
        return Err(LossyLineSliceError::LineStartOutside {
            line_start,
            line_end,
            total_lines,
        });
    }

    Ok(LossyLineSlice {
        content,
        bytes: sliced_bytes,
        total_lines,
        lossy_utf8,
    })
}

pub(crate) fn scan_file_scope_lossy(
    path: &Path,
    scope: ExploreScopeRequest,
    matcher: Option<&ExploreMatcher>,
    max_matches: usize,
    resume_from: Option<&ExploreCursor>,
    include_scope_content: bool,
    max_scope_bytes: Option<usize>,
) -> Result<ExploreScanResult, io::Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut raw_line = Vec::new();
    let mut total_lines = 0usize;
    let mut total_matches = 0usize;
    let mut matches = Vec::new();
    let mut resume_cursor = None;
    let mut lossy_utf8 = false;
    let mut scope_content = String::new();
    let mut scope_bytes = 0usize;
    let mut scope_within_budget = true;
    let mut first_scope_line = true;

    loop {
        raw_line.clear();
        let bytes_read = reader.read_until(b'\n', &mut raw_line)?;
        if bytes_read == 0 {
            break;
        }

        total_lines = total_lines.saturating_add(1);
        let (normalized_line, had_lossy_utf8) = normalize_lossy_line_bytes(&raw_line);
        lossy_utf8 |= had_lossy_utf8;

        let in_scope = total_lines >= scope.start_line
            && scope
                .end_line
                .is_none_or(|end_line| total_lines <= end_line);
        if !in_scope {
            continue;
        }

        if include_scope_content {
            if !first_scope_line {
                scope_bytes = scope_bytes.saturating_add(1);
                if scope_within_budget {
                    scope_content.push('\n');
                }
            }
            scope_bytes = scope_bytes.saturating_add(normalized_line.len());
            if let Some(max_scope_bytes) = max_scope_bytes {
                if scope_bytes > max_scope_bytes {
                    scope_within_budget = false;
                }
            }
            if scope_within_budget {
                scope_content.push_str(&normalized_line);
            }
            first_scope_line = false;
        }

        if let Some(matcher) = matcher {
            for (start, end) in matcher.find_spans(&normalized_line) {
                let start_column = start.saturating_add(1);
                if resume_from.is_some_and(|cursor| {
                    position_is_before_cursor(total_lines, start_column, cursor)
                }) {
                    continue;
                }

                total_matches = total_matches.saturating_add(1);
                let anchor = ExploreAnchor {
                    start_line: total_lines,
                    start_column,
                    end_line: total_lines,
                    end_column: end.saturating_add(1),
                };
                if matches.len() < max_matches {
                    matches.push(ExploreSpanMatch {
                        start_line: total_lines,
                        start_column,
                        end_line: total_lines,
                        end_column: end.saturating_add(1),
                        excerpt: normalized_line.clone(),
                        anchor,
                    });
                } else if resume_cursor.is_none() {
                    resume_cursor = Some(ExploreCursor {
                        line: total_lines,
                        column: start_column,
                    });
                }
            }
        }
    }

    let effective_scope = match total_lines {
        0 => ExploreLineWindow {
            start_line: 0,
            end_line: 0,
        },
        _ => ExploreLineWindow {
            start_line: scope.start_line,
            end_line: scope.end_line.unwrap_or(total_lines).min(total_lines),
        },
    };

    Ok(ExploreScanResult {
        total_lines,
        effective_scope,
        scope_content: include_scope_content.then_some(scope_content),
        scope_bytes: include_scope_content.then_some(scope_bytes),
        scope_within_budget,
        total_matches,
        matches,
        truncated: resume_cursor.is_some(),
        resume_from: resume_cursor,
        lossy_utf8,
    })
}

fn normalize_lossy_line_bytes(raw_line: &[u8]) -> (String, bool) {
    let mut line_bytes = raw_line;
    if line_bytes.ends_with(b"\n") {
        line_bytes = &line_bytes[..line_bytes.len() - 1];
    }
    if line_bytes.ends_with(b"\r") {
        line_bytes = &line_bytes[..line_bytes.len() - 1];
    }
    let normalized = String::from_utf8_lossy(line_bytes);
    let had_lossy_utf8 = matches!(normalized, Cow::Owned(_));
    (normalized.into_owned(), had_lossy_utf8)
}

fn position_is_before_cursor(line: usize, column: usize, cursor: &ExploreCursor) -> bool {
    line < cursor.line || (line == cursor.line && column < cursor.column)
}
