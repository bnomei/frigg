use std::borrow::Cow;

pub(crate) fn leading_html_comment_bounds(raw: &str) -> Option<(usize, usize)> {
    let raw = raw.trim_start_matches('\u{feff}');
    if !raw.starts_with("<!--") {
        return None;
    }

    let close_index = raw.find("-->")?;
    Some((0, close_index + 3))
}

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

pub(crate) fn leading_metadata_comment_bounds(raw: &str, marker: &str) -> Option<(usize, usize)> {
    let raw = raw.trim_start_matches('\u{feff}');
    let start = raw.find(marker)?;
    let after_marker = &raw[start + marker.len()..];
    let close_index = after_marker.find("-->")?;
    Some((start, start + marker.len() + close_index + 3))
}

pub(crate) fn scrub_leading_metadata_comment<'a>(raw: &'a str, marker: &str) -> Cow<'a, str> {
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
    fn scrub_leading_metadata_comment_preserves_line_numbers() {
        let raw = "<!-- marker\n{\"query\":\"secret\"}\n-->\n# Heading\n";
        let scrubbed = scrub_leading_metadata_comment(raw, "<!-- marker");

        assert_eq!(scrubbed.lines().count(), raw.lines().count());
        assert!(scrubbed.contains("# Heading"));
        assert!(!scrubbed.contains("secret"));
    }
}
