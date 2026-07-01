use frigg::agent_directive;

pub(crate) const MANAGED_BLOCK_START: &str = agent_directive::MANAGED_BLOCK_START;
pub(crate) const MANAGED_BLOCK_END: &str = agent_directive::MANAGED_BLOCK_END;

pub(crate) fn desired_markdown() -> String {
    agent_directive::render_managed_block()
}

pub(crate) fn has_managed_block(contents: &str) -> bool {
    contents.contains(MANAGED_BLOCK_START) && contents.contains(MANAGED_BLOCK_END)
}

pub(crate) fn managed_block_matches(contents: &str, desired_block: &str) -> bool {
    extract_managed_block(contents).is_some_and(|existing| existing == desired_block)
}

fn extract_managed_block(contents: &str) -> Option<&str> {
    let start = contents.find(MANAGED_BLOCK_START)?;
    let after_start = start + MANAGED_BLOCK_START.len();
    let end_offset = contents[after_start..].find(MANAGED_BLOCK_END)?;
    let end = after_start + end_offset + MANAGED_BLOCK_END.len();
    Some(&contents[start..end])
}

#[cfg(test)]
mod tests {
    use super::{
        MANAGED_BLOCK_END, MANAGED_BLOCK_START, desired_markdown, has_managed_block,
        managed_block_matches,
    };

    #[test]
    fn adopt_managed_block_markers_are_stable() {
        assert!(MANAGED_BLOCK_START.contains("frigg-directive:start"));
        assert!(MANAGED_BLOCK_END.contains("frigg-directive:end"));
    }

    #[test]
    fn adopt_managed_block_match_detects_exact_rendered_block() {
        let desired = desired_markdown();
        let contents = format!("# Existing\n\n{desired}\n\nTail\n");

        assert!(has_managed_block(&contents));
        assert!(managed_block_matches(&contents, &desired));
    }

    #[test]
    fn adopt_managed_block_match_rejects_drifted_block() {
        let desired = desired_markdown();
        let contents = contents_with_drifted_block();

        assert!(has_managed_block(&contents));
        assert!(!managed_block_matches(&contents, &desired));
    }

    fn contents_with_drifted_block() -> String {
        format!("{MANAGED_BLOCK_START}\nold\n{MANAGED_BLOCK_END}\n")
    }
}
