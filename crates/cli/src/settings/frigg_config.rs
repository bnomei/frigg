//! Top-level Frigg configuration for workspace roots, budgets, and runtime subsystems.
//!
//! Aggregates watch, lexical, and semantic runtime sections into one serde-backed profile that CLI
//! dispatch, indexing, and MCP startup deserialize from flags, env vars, and config files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{
    FriggError, FriggResult,
    model::{RepositoryId, RepositoryRecord, stable_repository_id_for_root},
};

use super::{LexicalRuntimeConfig, SemanticRuntimeConfig, WatchConfig};

/// Default workspace root when callers omit explicit roots.
pub const DEFAULT_WORKSPACE_ROOT: &str = ".";
/// Default maximum search result count for lexical and hybrid tools.
pub const DEFAULT_MAX_SEARCH_RESULTS: usize = 200;
/// Default maximum readable file byte budget for bounded source reads.
pub const DEFAULT_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

/// Top-level configuration shared by indexing, retrieval, watch, and MCP serving.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Validates configuration for MCP serving, allowing empty workspace roots.
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

        self.watch.validate()?;

        for root in &self.workspace_roots {
            if !root.exists() {
                return Err(FriggError::InvalidInput(format!(
                    "workspace root does not exist: {}",
                    root.display()
                )));
            }
            if !Self::is_git_workspace_root(root) {
                return Err(FriggError::InvalidInput(format!(
                    "workspace root is not a Git repository: {}",
                    root.display()
                )));
            }
        }

        self.semantic_runtime
            .validate()
            .map_err(|err| FriggError::InvalidInput(err.to_string()))?;

        Ok(())
    }

    fn is_git_workspace_root(root: &Path) -> bool {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        root.ancestors()
            .any(|ancestor| ancestor.join(".git").exists())
    }

    /// Materializes repository records from configured workspace roots.
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

    /// Resolves a workspace root by stable, legacy, or display repository id.
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
