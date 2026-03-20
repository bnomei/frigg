use std::borrow::Cow;
use std::path::Path;

use crate::text_sanitization::scrub_leading_html_comment;

pub(super) fn should_scrub_leading_markdown_comment(path: &str) -> bool {
    matches!(
        Path::new(path.trim_start_matches("./"))
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown" | "mdown")
    )
}

pub(super) fn scrub_search_content<'a>(path: &str, content: &'a str) -> Cow<'a, str> {
    if should_scrub_leading_markdown_comment(path) {
        return scrub_leading_html_comment(content);
    }

    Cow::Borrowed(content)
}
