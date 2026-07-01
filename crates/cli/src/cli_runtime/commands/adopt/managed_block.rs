pub(crate) const MANAGED_BLOCK_START: &str = "<!-- frigg:managed:start -->";
pub(crate) const MANAGED_BLOCK_END: &str = "<!-- frigg:managed:end -->";

#[cfg(test)]
mod tests {
    use super::{MANAGED_BLOCK_END, MANAGED_BLOCK_START};

    #[test]
    fn adopt_managed_block_markers_are_stable() {
        assert!(MANAGED_BLOCK_START.contains("frigg:managed:start"));
        assert!(MANAGED_BLOCK_END.contains("frigg:managed:end"));
    }
}
