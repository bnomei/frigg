use std::path::Path;

use crate::settings::FriggConfig;

pub fn config_for(root: &Path) -> FriggConfig {
    FriggConfig::from_workspace_roots(vec![root.to_path_buf()]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::config_for;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("frigg-test-support-{suffix}"));
        fs::create_dir_all(&root).expect("temp workspace root should be creatable");
        root
    }

    #[test]
    fn config_for_preserves_workspace_root_and_repository_mapping() {
        let root = temp_workspace_root();

        let config = config_for(&root);

        let repositories = config.repositories();
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].root_path, root.display().to_string());
        assert_eq!(
            config.root_by_repository_id("repo-001"),
            Some(root.as_path())
        );

        let _ = fs::remove_dir_all(root);
    }
}
