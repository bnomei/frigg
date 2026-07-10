//! Read-only tool execution scoping, blocking offload, and normalized workload metadata assembly.
//!
//! Scopes read-only tool work to attached repositories and offloads blocking IO; assembles
//! normalized workload metadata for provenance on MCP responses.

use super::*;
use rmcp::model::ProgressNotificationParam;

use crate::domain::{NormalizedWorkloadMetadata, WorkloadPrecisionMode};

impl ReadOnlyToolExecutionContext {
    pub(super) fn set_display_context_saved_percent(&self, percent: Option<f64>) {
        if let Some(percent) = percent {
            *self
                .display_context_saved_percent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(percent);
        }
    }

    fn display_context_saved_percent(&self) -> Option<f64> {
        *self
            .display_context_saved_percent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
    pub(super) async fn notify_progress(
        meta: &Meta,
        client: &Peer<RoleServer>,
        progress: f64,
        total: f64,
        message: impl Into<String>,
    ) {
        let Some(progress_token) = meta.get_progress_token() else {
            return;
        };
        let _ = client
            .notify_progress(
                ProgressNotificationParam::new(progress_token, progress)
                    .with_total(total)
                    .with_message(message.into()),
            )
            .await;
    }

    pub(super) fn read_only_tool_execution_context(
        &self,
        tool_name: &'static str,
        repository_hint: Option<String>,
    ) -> ReadOnlyToolExecutionContext {
        ReadOnlyToolExecutionContext {
            tool_name,
            repository_hint,
            started_at: Instant::now(),
            display_context_saved_percent: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn scoped_read_only_tool_execution_context(
        &self,
        tool_name: &'static str,
        repository_hint: Option<String>,
        freshness_mode: RepositoryResponseCacheFreshnessMode,
    ) -> Result<ScopedReadOnlyToolExecutionContext, ErrorData> {
        let base = self.read_only_tool_execution_context(tool_name, repository_hint);
        let scoped_workspaces =
            self.attached_workspaces_for_repository(base.repository_hint.as_deref())?;
        let scoped_repository_ids = scoped_workspaces
            .iter()
            .map(|workspace| workspace.repository_id.clone())
            .collect::<Vec<_>>();
        let cache_freshness =
            self.repository_response_cache_freshness(&scoped_workspaces, freshness_mode)?;

        Ok(ScopedReadOnlyToolExecutionContext {
            #[cfg(test)]
            base,
            scoped_workspaces,
            scoped_repository_ids,
            cache_freshness,
        })
    }

    pub(super) async fn run_read_only_tool_blocking<T, F>(
        &self,
        context: &ReadOnlyToolExecutionContext,
        task_fn: F,
    ) -> Result<T, ErrorData>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        Self::run_blocking_task(context.tool_name, task_fn).await
    }

    pub(super) fn finalize_read_only_tool<T>(
        &self,
        context: &ReadOnlyToolExecutionContext,
        result: Result<T, ErrorData>,
        provenance_result: Result<(), ErrorData>,
    ) -> Result<T, ErrorData> {
        self.finalize_with_provenance_timed(
            context.tool_name,
            context.started_at,
            result,
            provenance_result,
            context.display_context_saved_percent(),
        )
    }

    /// Run CPU/IO work off the async path. Multi-thread runtimes use `block_in_place` so short
    /// exact-search tools avoid spawn_blocking pool hops that dominate small-repo latency.
    pub(super) async fn run_blocking_task<T, F>(
        operation: &'static str,
        task_fn: F,
    ) -> Result<T, ErrorData>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        match tokio::runtime::Handle::current().runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => Ok(tokio::task::block_in_place(task_fn)),
            _ => task::spawn_blocking(task_fn).await.map_err(|err| {
                Self::internal(
                    format!("blocking task join failure in {operation}: {err}"),
                    Some(json!({
                        "operation": operation,
                        "join_error": Self::bounded_text(&err.to_string()),
                    })),
                )
            }),
        }
    }

    pub(super) fn tool_execution_finalization(
        &self,
        source_refs: Value,
        normalized_workload: Option<NormalizedWorkloadMetadata>,
    ) -> ToolExecutionFinalization {
        ToolExecutionFinalization::new(source_refs, normalized_workload)
    }

    pub fn set_tool_call_display_sink(&self, sink: Option<ToolCallDisplaySink>) {
        *self
            .runtime_state
            .tool_call_display_sink
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = sink;
    }

    pub(super) fn tool_call_display_enabled(&self) -> bool {
        self.runtime_state
            .tool_call_display_sink
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    pub(super) fn finalize_with_provenance_timed<T>(
        &self,
        tool_name: &str,
        started_at: Instant,
        result: Result<T, ErrorData>,
        provenance_result: Result<(), ErrorData>,
        context_saved_percent: Option<f64>,
    ) -> Result<T, ErrorData> {
        let duration_ms = Self::context_efficiency_elapsed_ms(started_at);
        crate::mcp::routing_stats::record_tool_call(tool_name);
        if tool_name == "workspace" || tool_name == "workspace_current" {
            crate::mcp::routing_stats::record_workspace_gate_use();
        }
        self.emit_tool_call_display_event(tool_name, duration_ms, &result, context_saved_percent);
        self.finalize_with_provenance(tool_name, result, provenance_result)
    }

    fn emit_tool_call_display_event<T>(
        &self,
        tool_name: &str,
        duration_ms: u64,
        result: &Result<T, ErrorData>,
        context_saved_percent: Option<f64>,
    ) {
        let sink = self
            .runtime_state
            .tool_call_display_sink
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(sink) = sink else {
            return;
        };
        sink(ToolCallDisplayEvent {
            tool_name: tool_name.to_owned(),
            duration_ms,
            status: if result.is_ok() {
                ToolCallDisplayStatus::Ok
            } else {
                ToolCallDisplayStatus::Failed
            },
            context_saved_percent,
            session_id: self.session_state.display_session_id(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::settings::FriggConfig;
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TempWorkspace {
        root: PathBuf,
    }

    impl TempWorkspace {
        fn new(name: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let root = std::env::current_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join(".codex-cache")
                .join("mcp-execution-tests")
                .join(format!(
                    "frigg-mcp-execution-{name}-{}-{stamp}",
                    std::process::id()
                ));
            fs::create_dir_all(&root).expect("temporary workspace should be creatable");
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture_server() -> (FriggMcpServer, AttachedWorkspace, TempWorkspace) {
        let workspace_root = TempWorkspace::new("fixture");
        fs::create_dir_all(workspace_root.path().join(".git"))
            .expect("temporary git root marker should be creatable");
        fs::create_dir_all(workspace_root.path().join("src"))
            .expect("temporary source directory should be creatable");
        fs::write(
            workspace_root.path().join("src/lib.rs"),
            "pub fn fixture() -> &'static str { \"fixture\" }\n",
        )
        .expect("temporary source file should be writable");
        let config = FriggConfig::from_optional_workspace_roots(Vec::<PathBuf>::new())
            .expect("fixture config should build");
        let server = FriggMcpServer::new_with_runtime_options(config, false);
        let _ = server
            .attach_workspace_internal(workspace_root.path(), true, WorkspaceResolveMode::GitRoot)
            .expect("fixture workspace should attach");
        let workspace = server
            .attached_workspaces()
            .into_iter()
            .next()
            .expect("fixture server should attach one workspace");
        (server, workspace, workspace_root)
    }

    #[test]
    fn tool_execution_context_scopes_to_explicit_repository() {
        let (server, workspace, _workspace_root) = fixture_server();

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
        let (server, workspace, _workspace_root) = fixture_server();
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
        let (server, workspace, _workspace_root) = fixture_server();
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
