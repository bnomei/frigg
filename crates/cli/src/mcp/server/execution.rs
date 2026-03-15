use super::*;

use crate::domain::{NormalizedWorkloadMetadata, WorkloadPrecisionMode};

impl ReadOnlyToolExecutionContext {
    pub(super) fn normalized_workload(
        &self,
        repository_ids: &[String],
        precision_mode: WorkloadPrecisionMode,
    ) -> NormalizedWorkloadMetadata {
        NormalizedWorkloadMetadata::from_repository_ids(
            self.tool_name,
            repository_ids,
            precision_mode,
        )
    }
}

impl ScopedReadOnlyToolExecutionContext {
    #[cfg(test)]
    pub(super) fn normalized_workload(
        &self,
        precision_mode: WorkloadPrecisionMode,
    ) -> NormalizedWorkloadMetadata {
        self.base
            .normalized_workload(&self.scoped_repository_ids, precision_mode)
    }
}

#[derive(Debug, Clone)]
pub(super) struct ToolExecutionFinalization {
    pub(super) source_refs: Value,
    pub(super) normalized_workload: Option<NormalizedWorkloadMetadata>,
}

impl ToolExecutionFinalization {
    pub(super) fn new(
        source_refs: Value,
        normalized_workload: Option<NormalizedWorkloadMetadata>,
    ) -> Self {
        Self {
            source_refs,
            normalized_workload,
        }
    }
}

impl FriggMcpServer {
    pub(super) fn tool_execution_finalization(
        &self,
        source_refs: Value,
        normalized_workload: Option<NormalizedWorkloadMetadata>,
    ) -> ToolExecutionFinalization {
        ToolExecutionFinalization::new(source_refs, normalized_workload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::FriggConfig;

    fn fixture_server() -> (FriggMcpServer, AttachedWorkspace) {
        let workspace_root = std::env::current_dir()
            .expect("current working directory should exist for MCP execution tests");
        let config = FriggConfig::from_workspace_roots(vec![workspace_root])
            .expect("fixture config should build");
        let server = FriggMcpServer::new_with_runtime_options(config, false, false);
        let workspace = server
            .attached_workspaces()
            .into_iter()
            .next()
            .expect("fixture server should attach one workspace");
        (server, workspace)
    }

    #[test]
    fn tool_execution_context_scopes_to_explicit_repository() {
        let (server, workspace) = fixture_server();

        let context = server
            .scoped_read_only_tool_execution_context(
                "search_text",
                Some(workspace.repository_id.clone()),
                RepositoryResponseCacheFreshnessMode::ManifestOnly,
            )
            .expect("explicit repository scope should resolve");

        assert_eq!(context.base.tool_name, "search_text");
        assert_eq!(
            context.base.repository_hint.as_deref(),
            Some(workspace.repository_id.as_str())
        );
        assert_eq!(context.scoped_workspaces.len(), 1);
        assert_eq!(context.scoped_repository_ids, vec![workspace.repository_id]);
    }

    #[test]
    fn tool_execution_context_uses_session_default_repository() {
        let (server, workspace) = fixture_server();
        server.set_current_repository_id(Some(workspace.repository_id.clone()));

        let context = server
            .scoped_read_only_tool_execution_context(
                "workspace_current",
                None,
                RepositoryResponseCacheFreshnessMode::ManifestOnly,
            )
            .expect("session repository scope should resolve");

        assert_eq!(context.base.repository_hint, None);
        assert_eq!(context.scoped_workspaces.len(), 1);
        assert_eq!(context.scoped_repository_ids, vec![workspace.repository_id]);
    }

    #[test]
    fn tool_execution_finalization_preserves_typed_workload_metadata() {
        let (server, workspace) = fixture_server();
        let context = server
            .scoped_read_only_tool_execution_context(
                "search_text",
                Some(workspace.repository_id.clone()),
                RepositoryResponseCacheFreshnessMode::ManifestOnly,
            )
            .expect("explicit repository scope should resolve");
        let normalized_workload = context.normalized_workload(WorkloadPrecisionMode::Exact);
        let finalization = server.tool_execution_finalization(
            json!({ "scoped_repository_ids": context.scoped_repository_ids }),
            Some(normalized_workload.clone()),
        );

        assert_eq!(
            finalization.source_refs["scoped_repository_ids"],
            json!(context.scoped_repository_ids)
        );
        assert_eq!(
            finalization
                .normalized_workload
                .as_ref()
                .map(NormalizedWorkloadMetadata::repository_scope_label),
            Some("single")
        );
        assert_eq!(
            finalization
                .normalized_workload
                .as_ref()
                .map(|metadata| metadata.precision_mode),
            Some(WorkloadPrecisionMode::Exact)
        );
    }
}
