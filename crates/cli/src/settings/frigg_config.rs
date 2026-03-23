use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{
    FriggError, FriggResult,
    model::{RepositoryId, RepositoryRecord, stable_repository_id_for_root},
};

use super::{LexicalRuntimeConfig, SemanticRuntimeConfig, WatchConfig};

pub const DEFAULT_WORKSPACE_ROOT: &str = ".";
pub const DEFAULT_MAX_SEARCH_RESULTS: usize = 200;
pub const DEFAULT_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Top-level configuration shared by indexing, retrieval, watch, and MCP serving.
pub struct FriggConfig {
    pub workspace_roots: Vec<PathBuf>,
    pub max_search_results: usize,
    pub max_file_bytes: usize,
    pub full_scip_ingest: bool,
    pub watch: WatchConfig,
    pub lexical_runtime: LexicalRuntimeConfig,
    pub semantic_runtime: SemanticRuntimeConfig,
}

impl Default for FriggConfig {
    fn default() -> Self {
        Self {
            workspace_roots: vec![PathBuf::from(DEFAULT_WORKSPACE_ROOT)],
            max_search_results: DEFAULT_MAX_SEARCH_RESULTS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            full_scip_ingest: true,
            watch: WatchConfig::default(),
            lexical_runtime: LexicalRuntimeConfig::default(),
            semantic_runtime: SemanticRuntimeConfig::default(),
        }
    }
}

impl FriggConfig {
    pub(crate) fn legacy_repository_id_for_workspace_index(index: usize) -> RepositoryId {
        RepositoryId(format!("repo-{:03}", index + 1))
    }

    pub fn from_workspace_roots(workspace_roots: Vec<PathBuf>) -> FriggResult<Self> {
        Self::from_workspace_roots_with_mode(workspace_roots, true)
    }

    pub fn from_optional_workspace_roots(workspace_roots: Vec<PathBuf>) -> FriggResult<Self> {
        Self::from_workspace_roots_with_mode(workspace_roots, false)
    }

    fn from_workspace_roots_with_mode(
        workspace_roots: Vec<PathBuf>,
        default_when_empty: bool,
    ) -> FriggResult<Self> {
        let roots = if workspace_roots.is_empty() {
            if default_when_empty {
                vec![PathBuf::from(DEFAULT_WORKSPACE_ROOT)]
            } else {
                Vec::new()
            }
        } else {
            workspace_roots
        };

        let cfg = Self {
            workspace_roots: roots,
            ..Self::default()
        };
        if default_when_empty {
            cfg.validate()?;
        } else {
            cfg.validate_for_serving()?;
        }
        Ok(cfg)
    }

    pub fn validate(&self) -> FriggResult<()> {
        self.validate_with_root_requirement(true)
    }

    pub fn validate_for_serving(&self) -> FriggResult<()> {
        self.validate_with_root_requirement(false)
    }

    pub fn ensure_workspace_roots_configured(&self) -> FriggResult<()> {
        if self.workspace_roots.is_empty() {
            return Err(FriggError::InvalidInput(
                "at least one workspace root is required".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_with_root_requirement(&self, require_workspace_roots: bool) -> FriggResult<()> {
        if require_workspace_roots {
            self.ensure_workspace_roots_configured()?;
        }

        if self.max_search_results == 0 {
            return Err(FriggError::InvalidInput(
                "max_search_results must be greater than zero".to_owned(),
            ));
        }

        if self.max_file_bytes == 0 {
            return Err(FriggError::InvalidInput(
                "max_file_bytes must be greater than zero".to_owned(),
            ));
        }

        if self.watch.debounce_ms == 0 {
            return Err(FriggError::InvalidInput(
                "watch.debounce_ms must be greater than zero".to_owned(),
            ));
        }

        if self.watch.retry_ms == 0 {
            return Err(FriggError::InvalidInput(
                "watch.retry_ms must be greater than zero".to_owned(),
            ));
        }

        for root in &self.workspace_roots {
            if !root.exists() {
                return Err(FriggError::InvalidInput(format!(
                    "workspace root does not exist: {}",
                    root.display()
                )));
            }
        }

        self.semantic_runtime
            .validate()
            .map_err(|err| FriggError::InvalidInput(err.to_string()))?;

        Ok(())
    }

    pub fn repositories(&self) -> Vec<RepositoryRecord> {
        self.workspace_roots
            .iter()
            .enumerate()
            .map(|(idx, root)| RepositoryRecord {
                repository_id: Self::legacy_repository_id_for_workspace_index(idx),
                display_name: root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| root.display().to_string()),
                root_path: root.display().to_string(),
            })
            .collect()
    }

    pub fn root_by_repository_id(&self, repository_id: &str) -> Option<&Path> {
        self.repositories()
            .into_iter()
            .zip(self.workspace_roots.iter().enumerate())
            .find_map(|(repo, (index, root))| {
                let stable_repository_id = stable_repository_id_for_root(root);
                let legacy_repository_id = Self::legacy_repository_id_for_workspace_index(index);
                (repo.repository_id.0 == repository_id
                    || stable_repository_id.0 == repository_id
                    || legacy_repository_id.0 == repository_id)
                    .then_some(root.as_path())
            })
    }
}
