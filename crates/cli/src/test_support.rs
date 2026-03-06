use std::path::Path;

use crate::settings::FriggConfig;

pub fn config_for(root: &Path) -> FriggConfig {
    FriggConfig::from_workspace_roots(vec![root.to_path_buf()]).unwrap_or_default()
}
