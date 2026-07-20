//! In-file exploration helpers backing the `explore` tool: bounded line windows, literal/regex
//! matching, and lossy UTF-8 normalization over shared runtime file-content snapshots.

use std::borrow::Cow;
use std::io;
use std::path::Path;

use regex::Regex;

use crate::mcp::server_cache::FileContentSnapshot;
use crate::mcp::types::{ExploreAnchor, ExploreCursor, ExploreLineWindow};

/// Default context lines above/below an explore zoom anchor.
pub(crate) const DEFAULT_CONTEXT_LINES: usize = 3;
/// Hard cap on explore context lines accepted from tool params.
pub(crate) const MAX_CONTEXT_LINES: usize = 32;
/// Default page size for explore probe/refine match rows.
pub(crate) const DEFAULT_MAX_MATCHES: usize = 8;

/// Explore window failure: I/O error or a line_start outside the file's total line count.
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

/// Bounded lossy UTF-8 line slice returned by shared file-content snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LossyLineSlice {
    pub content: String,
    pub bytes: usize,
    pub total_lines: usize,
    pub lossy_utf8: bool,
}

/// Literal or safe-regex matcher used by explore probe and refine scans.
#[derive(Debug, Clone)]
pub(crate) enum ExploreMatcher {
    Literal(String),
    Regex(Regex),
}

impl ExploreMatcher {
    /// Byte spans `(start, end)` of all non-overlapping matches on one line.
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

/// Inclusive 1-based line scope for one explore scan.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExploreScopeRequest {
    pub start_line: usize,
    pub end_line: Option<usize>,
}

/// One in-scope explore match with excerpt and anchor metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExploreSpanMatch {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
    pub anchor: ExploreAnchor,
}

/// Aggregate result from scanning one file scope through the explorer helpers.
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

/// Validates 1-based inclusive explore anchors (lines/columns must be non-zero and ordered).
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

/// Validates a 1-based explore resume cursor (`line` and `column` must be non-zero).
pub(crate) fn validate_cursor(cursor: &ExploreCursor) -> Result<(), &'static str> {
    if cursor.line == 0 {
        return Err("resume_from.line must be greater than zero");
    }
    if cursor.column == 0 {
        return Err("resume_from.column must be greater than zero");
    }
    Ok(())
}

/// Expands an anchor by `context_lines` into an inclusive 1-based line window (clamped at line 1).
pub(crate) fn line_window_around_anchor(
    anchor: &ExploreAnchor,
    context_lines: usize,
) -> ExploreLineWindow {
    ExploreLineWindow {
        start_line: anchor.start_line.saturating_sub(context_lines).max(1),
        end_line: anchor.end_line.saturating_add(context_lines),
    }
}

/// Path convenience wrapper: load a file snapshot then slice a lossy line window.
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

/// Path convenience wrapper: load a file snapshot then run an explore scope scan.
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

/// Strips trailing CR/LF and decodes a line lossily; second value is true when replacement occurred.
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

/// True when `(line, column)` is strictly before a resume cursor (same line: column-before).
pub(crate) fn position_is_before_cursor(
    line: usize,
    column: usize,
    cursor: &ExploreCursor,
) -> bool {
    line < cursor.line || (line == cursor.line && column < cursor.column)
}
