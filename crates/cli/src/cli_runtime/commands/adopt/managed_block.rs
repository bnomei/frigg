//! Managed markdown block insertion and removal for agent directive adoption targets.

use frigg::agent_directive;

pub(crate) const MANAGED_BLOCK_START: &str = agent_directive::MANAGED_BLOCK_START;
pub(crate) const MANAGED_BLOCK_END: &str = agent_directive::MANAGED_BLOCK_END;
const LEGACY_MANAGED_BLOCK_START_PREFIX: &str = "FRIGG:BEGIN v";
const LEGACY_MANAGED_BLOCK_END: &str = "FRIGG:END";

/// Outcome of inserting, updating, or removing a managed markdown adoption block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedBlockEdit {
    Changed(String),
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedBlockError {
    InvalidMarkers(String),
}

impl std::fmt::Display for ManagedBlockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMarkers(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ManagedBlockError {}

/// Returns the canonical managed markdown block rendered for agent directive targets.
pub(crate) fn desired_markdown() -> String {
    agent_directive::render_managed_block()
}

pub(crate) fn has_managed_block(contents: &str) -> bool {
    locate_managed_block(contents)
        .map(|span| span.is_some())
        .unwrap_or(false)
}

/// Inserts or replaces the managed block in markdown contents without touching surrounding text.
pub(crate) fn upsert_managed_block(
    contents: &str,
    desired_block: &str,
) -> Result<ManagedBlockEdit, ManagedBlockError> {
    let Some(span) = locate_managed_block(contents)? else {
        let updated = insert_managed_block(contents, desired_block);
        return Ok(if updated == contents {
            ManagedBlockEdit::Unchanged
        } else {
            ManagedBlockEdit::Changed(updated)
        });
    };

    if &contents[span.start..span.end] == desired_block {
        return Ok(ManagedBlockEdit::Unchanged);
    }

    let mut updated = String::with_capacity(contents.len() - span.len() + desired_block.len());
    updated.push_str(&contents[..span.start]);
    updated.push_str(desired_block);
    updated.push_str(&contents[span.end..]);
    Ok(ManagedBlockEdit::Changed(updated))
}

/// Removes the managed block and any adjacent adoption whitespace when uninstalling markdown targets.
pub(crate) fn remove_managed_block(contents: &str) -> Result<ManagedBlockEdit, ManagedBlockError> {
    let Some(span) = locate_managed_block(contents)? else {
        return Ok(ManagedBlockEdit::Unchanged);
    };

    let remove = removal_span(contents, span);
    let mut updated = String::with_capacity(contents.len() - remove.len());
    updated.push_str(&contents[..remove.start]);
    updated.push_str(&contents[remove.end..]);

    if updated == contents {
        Ok(ManagedBlockEdit::Unchanged)
    } else {
        Ok(ManagedBlockEdit::Changed(updated))
    }
}

fn insert_managed_block(contents: &str, desired_block: &str) -> String {
    if contents.is_empty() {
        return desired_block.to_owned();
    }

    let mut updated = String::with_capacity(contents.len() + desired_block.len() + 3);
    updated.push_str(contents);
    if !contents.ends_with('\n') {
        updated.push('\n');
    }
    if !contents.ends_with("\n\n") {
        updated.push('\n');
    }
    updated.push_str(desired_block);
    updated.push('\n');
    updated
}

fn locate_managed_block(contents: &str) -> Result<Option<Span>, ManagedBlockError> {
    let starts = marker_positions(contents, MarkerKind::Start);
    let ends = marker_positions(contents, MarkerKind::End);

    match (starts.len(), ends.len()) {
        (0, 0) => Ok(None),
        (1, 1) if starts[0].start < ends[0].start => Ok(Some(Span {
            start: starts[0].start,
            end: ends[0].end,
        })),
        _ => Err(ManagedBlockError::InvalidMarkers(
            "invalid nested or unmatched Frigg managed block markers".to_owned(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    Start,
    End,
}

fn marker_positions(contents: &str, kind: MarkerKind) -> Vec<Span> {
    let mut spans = Vec::new();
    for (line_start, line) in lines_with_offsets(contents) {
        let trimmed = line.trim();
        let matched = match kind {
            MarkerKind::Start => {
                trimmed == MANAGED_BLOCK_START
                    || trimmed.starts_with(LEGACY_MANAGED_BLOCK_START_PREFIX)
            }
            MarkerKind::End => trimmed == MANAGED_BLOCK_END || trimmed == LEGACY_MANAGED_BLOCK_END,
        };

        if matched {
            let marker_start = line_start + line.find(trimmed).unwrap_or(0);
            spans.push(Span {
                start: marker_start,
                end: marker_start + trimmed.len(),
            });
        }
    }
    spans
}

fn lines_with_offsets(contents: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    contents.split_inclusive('\n').map(move |line| {
        let line_start = offset;
        offset += line.len();
        (
            line_start,
            line.trim_end_matches('\n').trim_end_matches('\r'),
        )
    })
}

fn removal_span(contents: &str, block: Span) -> Span {
    let mut start = block.start;
    let mut end = block.end;

    if end < contents.len() && contents[end..].starts_with("\r\n") {
        end += 2;
    } else if end < contents.len() && contents[end..].starts_with('\n') {
        end += 1;
    }

    let before = &contents[..start];
    let after = &contents[end..];
    if before.ends_with("\r\n\r\n") && after.starts_with("\r\n") {
        start -= 2;
    } else if before.ends_with("\n\n") && after.starts_with('\n') {
        start -= 1;
    } else if before.ends_with("\r\n\r\n") && after.is_empty() {
        start -= 2;
    } else if before.ends_with("\n\n") && after.is_empty() {
        start -= 1;
    }

    Span { start, end }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::{
        MANAGED_BLOCK_END, MANAGED_BLOCK_START, ManagedBlockEdit, desired_markdown,
        has_managed_block, remove_managed_block, upsert_managed_block,
    };

    #[test]
    fn adopt_managed_block_markers_are_stable() {
        assert!(MANAGED_BLOCK_START.contains("frigg-directive:start"));
        assert!(MANAGED_BLOCK_END.contains("frigg-directive:end"));
    }

    #[test]
    fn adopt_managed_block_inserts_into_empty_file() {
        let desired = desired_markdown();

        assert_eq!(
            upsert_managed_block("", &desired).expect("upsert block"),
            ManagedBlockEdit::Changed(desired)
        );
    }

    #[test]
    fn adopt_managed_block_inserts_into_non_empty_file() {
        let desired = desired_markdown();

        assert_eq!(
            upsert_managed_block("# Existing\nTail", &desired).expect("upsert block"),
            ManagedBlockEdit::Changed(format!("# Existing\nTail\n\n{desired}\n"))
        );
    }

    #[test]
    fn adopt_managed_block_replaces_existing_current_marker_block() {
        let desired = desired_markdown();
        let contents =
            format!("# Existing\n\n{MANAGED_BLOCK_START}\nold\n{MANAGED_BLOCK_END}\nTail");

        assert_eq!(
            upsert_managed_block(&contents, &desired).expect("upsert block"),
            ManagedBlockEdit::Changed(format!("# Existing\n\n{desired}\nTail"))
        );
    }

    #[test]
    fn adopt_managed_block_replaces_legacy_versioned_block() {
        let desired = desired_markdown();
        let contents = "# Existing\nFRIGG:BEGIN v1\nold\nFRIGG:END\nTail";

        assert_eq!(
            upsert_managed_block(contents, &desired).expect("upsert block"),
            ManagedBlockEdit::Changed(format!("# Existing\n{desired}\nTail"))
        );
    }

    #[test]
    fn adopt_managed_block_upsert_detects_unchanged_content() {
        let desired = desired_markdown();
        let contents = format!("# Existing\n\n{desired}\n\nTail\n");

        assert!(has_managed_block(&contents));
        assert_eq!(
            upsert_managed_block(&contents, &desired).expect("upsert block"),
            ManagedBlockEdit::Unchanged
        );
    }

    #[test]
    fn adopt_managed_block_upsert_detects_drifted_content() {
        let desired = desired_markdown();
        let contents = contents_with_drifted_block();

        assert!(has_managed_block(&contents));
        assert!(matches!(
            upsert_managed_block(&contents, &desired).expect("upsert block"),
            ManagedBlockEdit::Changed(_)
        ));
    }

    #[test]
    fn adopt_managed_block_removes_only_owned_block() {
        let desired = desired_markdown();
        let contents = format!("# Existing\n\n{desired}\n\nTail\n");

        assert_eq!(
            remove_managed_block(&contents).expect("remove block"),
            ManagedBlockEdit::Changed("# Existing\n\nTail\n".to_owned())
        );
    }

    #[test]
    fn adopt_managed_block_removes_separator_from_appended_block() {
        let desired = desired_markdown();
        let contents = format!("# Existing\nTail\n\n{desired}\n");

        assert_eq!(
            remove_managed_block(&contents).expect("remove block"),
            ManagedBlockEdit::Changed("# Existing\nTail\n".to_owned())
        );
    }

    #[test]
    fn adopt_managed_block_insert_then_remove_round_trips_non_empty_file() {
        let desired = desired_markdown();
        let original = "# Existing\nTail";
        let ManagedBlockEdit::Changed(inserted) =
            upsert_managed_block(original, &desired).expect("upsert block")
        else {
            panic!("insert should change content");
        };

        assert_eq!(
            remove_managed_block(&inserted).expect("remove block"),
            ManagedBlockEdit::Changed("# Existing\nTail\n".to_owned())
        );
    }

    #[test]
    fn adopt_managed_block_remove_detects_unchanged_without_block() {
        assert_eq!(
            remove_managed_block("# Existing\n").expect("remove block"),
            ManagedBlockEdit::Unchanged
        );
    }

    #[test]
    fn adopt_managed_block_rejects_nested_or_multiple_markers() {
        let contents = format!(
            "{MANAGED_BLOCK_START}\nouter\n{MANAGED_BLOCK_START}\ninner\n{MANAGED_BLOCK_END}\n{MANAGED_BLOCK_END}\n"
        );

        assert!(upsert_managed_block(&contents, &desired_markdown()).is_err());
        assert!(remove_managed_block(&contents).is_err());
    }

    fn contents_with_drifted_block() -> String {
        format!("{MANAGED_BLOCK_START}\nold\n{MANAGED_BLOCK_END}\n")
    }
}
