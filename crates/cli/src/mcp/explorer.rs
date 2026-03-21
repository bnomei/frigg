use std::borrow::Cow;
use std::io;
use std::path::Path;

use regex::Regex;

use crate::mcp::server_cache::FileContentSnapshot;
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

#[allow(dead_code)]
pub(crate) fn read_line_slice_lossy(
    path: &Path,
    line_start: usize,
    line_end: Option<usize>,
    max_bytes: usize,
) -> Result<LossyLineSlice, LossyLineSliceError> {
    let snapshot = FileContentSnapshot::from_path(path)?;
    snapshot.read_line_slice_lossy(line_start, line_end, max_bytes)
}

#[allow(dead_code)]
pub(crate) fn scan_file_scope_lossy(
    path: &Path,
    scope: ExploreScopeRequest,
    matcher: Option<&ExploreMatcher>,
    max_matches: usize,
    resume_from: Option<&ExploreCursor>,
    include_scope_content: bool,
    max_scope_bytes: Option<usize>,
) -> Result<ExploreScanResult, io::Error> {
    let snapshot = FileContentSnapshot::from_path(path)?;
    Ok(snapshot.scan_file_scope_lossy(
        scope,
        matcher,
        max_matches,
        resume_from,
        include_scope_content,
        max_scope_bytes,
    ))
}

pub(crate) fn normalize_lossy_line_bytes(raw_line: &[u8]) -> (String, bool) {
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

pub(crate) fn position_is_before_cursor(
    line: usize,
    column: usize,
    cursor: &ExploreCursor,
) -> bool {
    line < cursor.line || (line == cursor.line && column < cursor.column)
}
