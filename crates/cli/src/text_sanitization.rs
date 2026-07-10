//! Line-preserving scrubbing of leading HTML and metadata comment blocks.
//!
//! Replaces comment interiors with spaces while keeping newlines intact so lexical scans and
//! path-witness excerpts ignore generator metadata without shifting line numbers.

use std::borrow::Cow;

/// Byte range of a leading HTML comment (after optional BOM), if the buffer starts with one.
pub(crate) fn leading_html_comment_bounds(raw: &str) -> Option<(usize, usize)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let bom_len = raw.len().saturating_sub(trimmed.len());
    if !trimmed.starts_with("<!--") {
        return None;
    }

    let close_index = trimmed.find("-->")?;
    Some((bom_len, bom_len + close_index + 3))
}

/// Space-out a leading HTML comment while preserving newlines so line numbers stay stable.
pub(crate) fn scrub_leading_html_comment<'a>(raw: &'a str) -> Cow<'a, str> {
    let Some((start, end)) = leading_html_comment_bounds(raw) else {
        return Cow::Borrowed(raw);
    };

    let mut scrubbed = String::with_capacity(raw.len());
    scrubbed.push_str(&raw[..start]);
    scrubbed.extend(raw[start..end].chars().map(|ch| match ch {
        '\n' | '\r' => ch,
        _ => ' ',
    }));
    scrubbed.push_str(&raw[end..]);
    Cow::Owned(scrubbed)
}

#[cfg(test)]
fn leading_metadata_comment_bounds(raw: &str, marker: &str) -> Option<(usize, usize)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let bom_len = raw.len().saturating_sub(trimmed.len());
    let start = trimmed.find(marker)?;
    let after_marker = &trimmed[start + marker.len()..];
    let close_index = after_marker.find("-->")?;
    Some((
        bom_len + start,
        bom_len + start + marker.len() + close_index + 3,
    ))
}

#[cfg(test)]
fn scrub_leading_metadata_comment<'a>(raw: &'a str, marker: &str) -> Cow<'a, str> {
    let Some((start, end)) = leading_metadata_comment_bounds(raw, marker) else {
        return Cow::Borrowed(raw);
    };

    let mut scrubbed = String::with_capacity(raw.len());
    scrubbed.push_str(&raw[..start]);
    scrubbed.extend(raw[start..end].chars().map(|ch| match ch {
        '\n' | '\r' => ch,
        _ => ' ',
    }));
    scrubbed.push_str(&raw[end..]);
    Cow::Owned(scrubbed)
}

#[cfg(test)]
mod tests {
    use super::{scrub_leading_html_comment, scrub_leading_metadata_comment};

    #[test]
    fn scrub_leading_html_comment_preserves_line_numbers() {
        let raw = "<!-- hidden metadata -->\n# Heading\nbody\n";
        let scrubbed = scrub_leading_html_comment(raw);

        assert_eq!(scrubbed.lines().count(), raw.lines().count());
        assert!(scrubbed.contains("# Heading"));
        assert!(scrubbed.contains("body"));
        assert!(!scrubbed.contains("hidden metadata"));
    }

    #[test]
    fn scrub_leading_html_comment_handles_bom_prefixed_comments_without_leak_or_panic() {
        let raw = "\u{feff}\u{feff}<!--😀SECRET-->\n# Heading\n";
        let scrubbed = scrub_leading_html_comment(raw);

        assert_eq!(scrubbed.lines().count(), raw.lines().count());
        assert!(scrubbed.starts_with("\u{feff}\u{feff}"));
        assert!(scrubbed.contains("# Heading"));
        assert!(!scrubbed.contains("SECRET"));
        assert!(!scrubbed.contains("-->"));
    }

    #[test]
    fn scrub_leading_metadata_comment_preserves_line_numbers() {
        let raw = "<!-- marker\n{\"query\":\"secret\"}\n-->\n# Heading\n";
        let scrubbed = scrub_leading_metadata_comment(raw, "<!-- marker");

        assert_eq!(scrubbed.lines().count(), raw.lines().count());
        assert!(scrubbed.contains("# Heading"));
        assert!(!scrubbed.contains("secret"));
    }

    #[test]
    fn scrub_leading_metadata_comment_handles_bom_prefixed_comments() {
        let raw = "\u{feff}<!-- marker\n{\"query\":\"secret\"}\n-->\n# Heading\n";
        let scrubbed = scrub_leading_metadata_comment(raw, "<!-- marker");

        assert_eq!(scrubbed.lines().count(), raw.lines().count());
        assert!(scrubbed.starts_with('\u{feff}'));
        assert!(scrubbed.contains("# Heading"));
        assert!(!scrubbed.contains("secret"));
        assert!(!scrubbed.contains("-->"));
    }
}
