//! `read_file`, `read_match`, and `explore` implementations with path containment and runtime
//! file-content window reuse.
//!
//! Enforces workspace path containment on reads and reuses file-content windows within a tool
//! execution scope to limit duplicate IO.

use super::presentation::SessionResultHandleLookup;
use super::*;
use crate::mcp::server_cache::{ContinuationBinding, ResultHandleSourceRevision};
use crate::mcp::types::{
    ContextEfficiencyMetadata, ExploreAnchor, ExploreCursor, NextActionOrigin, ResultCompleteness,
    ResultTruncationReason, ResultUnit,
};
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Read;

#[derive(Clone)]
pub(super) struct ReadFileProvenanceContext {
    tool_name: &'static str,
    extra_params: Value,
}

/// Private proof expectation used only by `read_match`.
///
/// Keeping this separate from `ReadFileParams` preserves the public `read_file` contract while
/// ensuring a proof-bound response is built only after the captured raw bytes are verified.
#[derive(Clone)]
pub(super) struct ReadMatchProofExpectation {
    revision: ResultHandleSourceRevision,
    origin_tool: &'static str,
    origin: Option<NextActionOrigin>,
    result_handle: String,
    match_id: String,
    repository_id: String,
    path: String,
}

#[derive(Debug)]
enum BoundedProofReadError {
    Io(std::io::Error),
    TooLarge,
}

/// Read no more than `max_bytes + 1` bytes, so a size race cannot allocate an unbounded source.
fn read_proof_bytes_bounded(
    path: &Path,
    max_bytes: usize,
) -> Result<Vec<u8>, BoundedProofReadError> {
    let mut file = fs::File::open(path).map_err(BoundedProofReadError::Io)?;
    let read_limit = u64::try_from(max_bytes.saturating_add(1)).unwrap_or(u64::MAX);
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(BoundedProofReadError::Io)?;
    if bytes.len() > max_bytes {
        return Err(BoundedProofReadError::TooLarge);
    }
    Ok(bytes)
}

impl ReadFileProvenanceContext {
    pub(super) fn read_file() -> Self {
        Self {
            tool_name: "read_file",
            extra_params: Value::Null,
        }
    }

    pub(super) fn read_match(result_handle: &str, match_id: &str) -> Self {
        Self {
            tool_name: "read_match",
            extra_params: json!({
                "result_handle": result_handle,
                "match_id": match_id,
            }),
        }
    }

    fn merge_extra_params(&self, params: &mut Value) {
        let (Some(target), Some(extra)) = (params.as_object_mut(), self.extra_params.as_object())
        else {
            return;
        };
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
}

impl FriggMcpServer {
    pub(super) async fn read_file_impl(
        &self,
        params: ReadFileParams,
    ) -> Result<ReadFileResponse, ErrorData> {
        self.read_file_impl_with_provenance(params, ReadFileProvenanceContext::read_file(), None)
            .await
    }

    pub(super) async fn read_file_impl_with_provenance(
        &self,
        params: ReadFileParams,
        provenance: ReadFileProvenanceContext,
        proof_expectation: Option<ReadMatchProofExpectation>,
    ) -> Result<ReadFileResponse, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context(provenance.tool_name, params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let provenance_for_blocking = provenance.clone();
        let proof_expectation_for_blocking = proof_expectation.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut resolved_repository_id: Option<String> = None;
                let mut resolved_path: Option<String> = None;
                let mut resolved_absolute_path: Option<String> = None;
                let mut effective_max_bytes: Option<usize> = None;
                let mut effective_start_line: Option<usize> = None;
                let mut effective_end_line: Option<usize> = None;
                let result = (|| -> Result<ReadFileResponse, ErrorData> {
                    let query_started_at = std::time::Instant::now();
                    let context_efficiency_log_enabled =
                        crate::context_efficiency::context_efficiency_log_enabled();
                    let need_context_efficiency =
                        crate::context_efficiency::need_context_efficiency_with_log_state(
                            params_for_blocking.include_context_efficiency,
                            context_efficiency_log_enabled,
                        ) || server.tool_call_display_enabled();
                    let requested_max_bytes = params_for_blocking
                        .max_bytes
                        .unwrap_or(server.config.max_file_bytes);
                    if requested_max_bytes == 0 {
                        return Err(Self::invalid_params(
                            "max_bytes must be greater than zero",
                            None,
                        ));
                    }

                    let max_bytes = requested_max_bytes.min(server.config.max_file_bytes);
                    effective_max_bytes = Some(max_bytes);
                    let has_line_range = params_for_blocking.start_line.is_some()
                        || params_for_blocking.end_line.is_some()
                        || params_for_blocking.line_count.is_some();
                    if params_for_blocking.start_line == Some(0) {
                        return Err(Self::invalid_params(
                            "start_line must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.end_line == Some(0) {
                        return Err(Self::invalid_params(
                            "end_line must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.line_count == Some(0) {
                        return Err(Self::invalid_params(
                            "line_count must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.end_line.is_some()
                        && params_for_blocking.line_count.is_some()
                    {
                        return Err(Self::invalid_params(
                            "end_line and line_count are mutually exclusive",
                            None,
                        ));
                    }
                    if let (Some(start_line), Some(end_line)) =
                        (params_for_blocking.start_line, params_for_blocking.end_line)
                        && end_line < start_line
                    {
                        return Err(Self::invalid_params(
                            "end_line must be greater than or equal to start_line",
                            Some(json!({
                                "start_line": start_line,
                                "end_line": end_line,
                            })),
                        ));
                    }

                    let (repository_id, path, display_path) =
                        server.resolve_file_path(&params_for_blocking)?;
                    resolved_repository_id = Some(repository_id.clone());
                    resolved_path = Some(display_path.clone());
                    resolved_absolute_path = Some(path.display().to_string());

                    let workspace = server
                        .attached_workspaces_for_repository(Some(repository_id.as_str()))?
                        .into_iter()
                        .find(|workspace| workspace.repository_id == repository_id)
                        .ok_or_else(|| {
                            Self::resource_not_found(
                                "repository_id not found",
                                Some(json!({ "repository_id": repository_id })),
                            )
                        })?;
                    let pre_read_bytes = if !has_line_range {
                        let metadata = fs::metadata(&path).map_err(|err| {
                            Self::internal(
                                format!("failed to stat file {}: {err}", path.display()),
                                None,
                            )
                        })?;
                        Some(usize::try_from(metadata.len()).unwrap_or(usize::MAX))
                    } else {
                        None
                    };
                    if let Some(pre_read_bytes) = pre_read_bytes
                        && pre_read_bytes > max_bytes
                    {
                        let suggested_max_bytes = pre_read_bytes.min(server.config.max_file_bytes);
                        return Err(Self::invalid_params(
                            format!("file exceeds max_bytes={max_bytes}"),
                            Some(json!({
                                "path": display_path.clone(),
                                "bytes": pre_read_bytes,
                                "max_bytes": max_bytes,
                                "requested_max_bytes": requested_max_bytes,
                                "config_max_file_bytes": server.config.max_file_bytes,
                                "suggested_max_bytes": suggested_max_bytes,
                            })),
                        ));
                    }
                    let snapshot = if proof_expectation_for_blocking.is_some() {
                        server.file_content_snapshot_for_bound_proof(&workspace, &path)?
                    } else {
                        server.file_content_snapshot_for_workspace(&workspace, &path)?
                    };
                    if proof_expectation_for_blocking
                        .as_ref()
                        .is_some_and(|expectation| {
                            snapshot.source_revision() != expectation.revision
                        })
                    {
                        // Do not extract content or metadata from an unverified proof snapshot.
                        return Err(Self::resource_not_found(
                            "source revision no longer matches the proof anchor",
                            None,
                        ));
                    }
                    if proof_expectation_for_blocking.is_some()
                        && snapshot.raw_bytes_len() > server.config.max_file_bytes
                    {
                        // A file that grew after the bounded-read metadata check is not a
                        // verifiable proof source, even when the requested line window is small.
                        return Err(Self::resource_not_found(
                            "source exceeds the proof read bound",
                            None,
                        ));
                    }
                    let _pre_read_bytes =
                        pre_read_bytes.unwrap_or_else(|| snapshot.raw_bytes_len());
                    if !has_line_range {
                        let post_read_bytes = snapshot.raw_bytes_len();
                        if post_read_bytes > max_bytes {
                            let suggested_max_bytes =
                                post_read_bytes.min(server.config.max_file_bytes);
                            return Err(Self::invalid_params(
                                format!("file exceeds max_bytes={max_bytes}"),
                                Some(json!({
                                    "path": display_path.clone(),
                                    "bytes": post_read_bytes,
                                    "max_bytes": max_bytes,
                                    "requested_max_bytes": requested_max_bytes,
                                    "config_max_file_bytes": server.config.max_file_bytes,
                                    "suggested_max_bytes": suggested_max_bytes,
                                })),
                            ));
                        }
                        let content = snapshot.read_file_content();
                        let total_lines = if content.is_empty() {
                            0
                        } else {
                            content.lines().count().max(1)
                        };
                        let mut response = ReadFileResponse {
                            repository_id,
                            path: display_path,
                            start_line: Some(1),
                            end_line: Some(total_lines.max(1)),
                            bytes: post_read_bytes,
                            content,
                            context_efficiency: None,
                        };
                        if need_context_efficiency {
                            let context_efficiency = server
                                .context_efficiency_metadata_for_tool_observers(
                                    &execution_context_for_blocking,
                                    params_for_blocking.include_context_efficiency,
                                    context_efficiency_log_enabled,
                                    || {
                                        server.read_surface_context_efficiency_metadata(
                                            &response.repository_id,
                                            &response.path,
                                            response.bytes,
                                            None,
                                            Some(Self::context_efficiency_elapsed_ms(
                                                query_started_at,
                                            )),
                                        )
                                    },
                                )?;
                            if let Some(context_efficiency) = context_efficiency.as_ref() {
                                server.append_context_efficiency_log_for_workspaces(
                                    provenance_for_blocking.tool_name,
                                    std::slice::from_ref(&workspace),
                                    context_efficiency,
                                );
                            }
                            if params_for_blocking.include_context_efficiency == Some(true) {
                                response.context_efficiency = context_efficiency;
                            }
                        }
                        return Ok(response);
                    }

                    let start_line = params_for_blocking.start_line.unwrap_or(1);
                    let requested_line_end = match params_for_blocking.end_line {
                        Some(end_line) => Some(end_line),
                        None => params_for_blocking.line_count.map(|line_count| {
                            start_line.saturating_add(line_count.saturating_sub(1))
                        }),
                    };
                    let effective_end_hint = requested_line_end;
                    effective_start_line = Some(start_line);
                    effective_end_line = Some(effective_end_hint.unwrap_or(1));

                    let line_slice = snapshot
                        .read_line_slice_lossy(start_line, requested_line_end, max_bytes)
                        .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                    let sliced_content = line_slice.content;
                    let sliced_bytes = line_slice.bytes;
                    let total_lines = line_slice.total_lines;
                    let effective_end = requested_line_end
                        .unwrap_or(total_lines.max(1))
                        .min(total_lines.max(1));
                    effective_end_line = Some(effective_end);

                    if sliced_bytes > max_bytes {
                        let suggested_max_bytes = sliced_bytes.min(server.config.max_file_bytes);
                        return Err(Self::invalid_params(
                            format!("selected line range exceeds max_bytes={max_bytes}"),
                            Some(json!({
                                "path": display_path.clone(),
                                "bytes": sliced_bytes,
                                "max_bytes": max_bytes,
                                "requested_max_bytes": requested_max_bytes,
                                "config_max_file_bytes": server.config.max_file_bytes,
                                "suggested_max_bytes": suggested_max_bytes,
                                "start_line": start_line,
                                "end_line": effective_end,
                                "total_lines": total_lines,
                            })),
                        ));
                    }

                    let mut response = ReadFileResponse {
                        repository_id,
                        path: display_path,
                        start_line: Some(start_line),
                        end_line: Some(effective_end),
                        bytes: sliced_bytes,
                        content: sliced_content,
                        context_efficiency: None,
                    };
                    if need_context_efficiency {
                        let context_efficiency = server
                            .context_efficiency_metadata_for_tool_observers(
                                &execution_context_for_blocking,
                                params_for_blocking.include_context_efficiency,
                                context_efficiency_log_enabled,
                                || {
                                    server.read_surface_context_efficiency_metadata(
                                        &response.repository_id,
                                        &response.path,
                                        response.bytes,
                                        None,
                                        Some(Self::context_efficiency_elapsed_ms(query_started_at)),
                                    )
                                },
                            )?;
                        if let Some(context_efficiency) = context_efficiency.as_ref() {
                            server.append_context_efficiency_log_for_workspaces(
                                provenance_for_blocking.tool_name,
                                std::slice::from_ref(&workspace),
                                context_efficiency,
                            );
                        }
                        if params_for_blocking.include_context_efficiency == Some(true) {
                            response.context_efficiency = context_efficiency;
                        }
                    }
                    Ok(response)
                })();
                let repository_ids = resolved_repository_id
                    .clone()
                    .or_else(|| execution_context_for_blocking.repository_hint.clone())
                    .into_iter()
                    .collect::<Vec<_>>();
                let normalized_workload = (!repository_ids.is_empty()).then(|| {
                    execution_context_for_blocking
                        .normalized_workload(&repository_ids, WorkloadPrecisionMode::Exact)
                });
                let result = match (proof_expectation_for_blocking.as_ref(), result) {
                    (Some(expectation), Err(_)) => {
                        crate::mcp::routing_stats::record_handle_failure();
                        Err(server.stale_proof_anchor_error(expectation))
                    }
                    (_, result) => result,
                };
                let source_refs = if let Some(expectation) = proof_expectation_for_blocking.as_ref()
                {
                    json!({
                        "repository_id": expectation.repository_id,
                        "path": expectation.path,
                        "origin_tool": expectation.origin_tool,
                        "result_handle": expectation.result_handle,
                        "match_id": expectation.match_id,
                    })
                } else {
                    json!({
                        "resolved_repository_id": resolved_repository_id.clone(),
                        "resolved_path": resolved_path
                            .clone()
                            .map(|path| Self::bounded_text(&path)),
                        "resolved_absolute_path": resolved_absolute_path
                            .clone()
                            .map(|path| Self::bounded_text(&path)),
                    })
                };
                let finalization =
                    server.tool_execution_finalization(source_refs, normalized_workload);
                let mut provenance_params = json!({
                    "repository_id": execution_context_for_blocking.repository_hint,
                    "path": Self::bounded_text(&params_for_blocking.path),
                    "max_bytes": params_for_blocking.max_bytes,
                    "start_line": params_for_blocking.start_line,
                    "end_line": params_for_blocking.end_line,
                    "line_count": params_for_blocking.line_count,
                    "effective_max_bytes": effective_max_bytes,
                    "effective_start_line": effective_start_line,
                    "effective_end_line": effective_end_line,
                });
                provenance_for_blocking.merge_extra_params(&mut provenance_params);
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    provenance_for_blocking.tool_name,
                    execution_context_for_blocking.repository_hint.as_deref(),
                    provenance_params,
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                    finalization.normalized_workload,
                );

                ReadFileExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    fn stale_proof_anchor_error(&self, expectation: &ReadMatchProofExpectation) -> ErrorData {
        let mut recovery = RecoveryFields::stale_proof_anchor_with_origin(
            expectation.origin_tool,
            &expectation.result_handle,
            &expectation.match_id,
            &expectation.repository_id,
            &expectation.path,
            expectation.origin.as_ref(),
        );
        self.validate_recovery_actions(&mut recovery);
        let message = recovery
            .message
            .clone()
            .unwrap_or_else(|| "source proof anchor is stale".to_owned());
        let RecoveryFields {
            error_code,
            correction_hint,
            related_tools,
            next_actions,
            suggested_next,
            ..
        } = recovery;
        Self::resource_not_found(
            message,
            Some(json!({
                "error_code": error_code,
                "repository_id": expectation.repository_id,
                "path": expectation.path,
                "origin_tool": expectation.origin_tool,
                "result_handle": expectation.result_handle,
                "match_id": expectation.match_id,
                "correction_hint": correction_hint,
                "related_tools": related_tools,
                "next_actions": next_actions,
                "suggested_next": suggested_next,
            })),
        )
    }

    fn session_result_handle_origin_tool(&self, result_handle: &str) -> Option<&'static str> {
        self.session_state
            .inner
            .result_handles
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .get(result_handle)
            .map(|entry| entry.origin_tool)
    }

    /// Builds a bounded snapshot for proof validation without changing `read_file` behavior.
    fn file_content_snapshot_for_bound_proof(
        &self,
        workspace: &AttachedWorkspace,
        canonical_path: &Path,
    ) -> Result<Arc<FileContentSnapshot>, ErrorData> {
        let _freshness = self.repository_response_cache_freshness(
            std::slice::from_ref(workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )?;
        let max_file_bytes = self.config.max_file_bytes;
        let proof_path = self.bound_proof_path_within_workspace(workspace, canonical_path)?;
        let bytes =
            read_proof_bytes_bounded(&proof_path, max_file_bytes).map_err(|error| match error {
                BoundedProofReadError::TooLarge => Self::invalid_params(
                    format!("file exceeds max_file_bytes={max_file_bytes}"),
                    Some(json!({
                        "path": canonical_path.display().to_string(),
                        "max_file_bytes": max_file_bytes,
                    })),
                ),
                BoundedProofReadError::Io(error) => Self::internal(
                    format!("failed to read file {}: {error}", canonical_path.display()),
                    None,
                ),
            })?;
        // The pathname may have been swapped while it was open. Re-check the live object after
        // the bounded read so an external symlink swap cannot supply a same-byte proof source.
        if self.bound_proof_path_within_workspace(workspace, canonical_path)? != proof_path {
            return Err(Self::access_denied(
                "proof source changed while its workspace containment was being verified",
                None,
            ));
        }
        Ok(Arc::new(FileContentSnapshot::from_bytes(bytes)))
    }

    /// Re-authorizes the live proof source after `read_match`'s earlier path resolution.
    ///
    /// The regular resolver is intentionally shared with `read_file`, but a revision-bound proof
    /// must fail closed if the resolved path is replaced by a symlink before or during the read.
    fn bound_proof_path_within_workspace(
        &self,
        workspace: &AttachedWorkspace,
        resolved_path: &Path,
    ) -> Result<PathBuf, ErrorData> {
        let root = workspace.root.canonicalize().map_err(|err| {
            Self::internal(
                "failed to canonicalize proof workspace root",
                Some(json!({
                    "reason": Self::bounded_text(&err.to_string()),
                })),
            )
        })?;
        let live_path = resolved_path.canonicalize().map_err(|err| {
            Self::resource_not_found(
                "proof source can no longer be resolved",
                Some(json!({ "reason": Self::bounded_text(&err.to_string()) })),
            )
        })?;
        if !live_path.starts_with(&root) {
            return Err(Self::access_denied(
                "proof source is outside workspace roots",
                None,
            ));
        }
        let metadata = fs::metadata(&live_path).map_err(|err| {
            Self::resource_not_found(
                "proof source can no longer be read",
                Some(json!({ "reason": Self::bounded_text(&err.to_string()) })),
            )
        })?;
        if !metadata.is_file() {
            return Err(Self::resource_not_found(
                "proof source is no longer a file",
                None,
            ));
        }
        Ok(live_path)
    }

    pub(super) async fn read_match_impl(
        &self,
        params: ReadMatchParams,
    ) -> Result<ReadMatchResponse, ErrorData> {
        let started_at = Instant::now();
        let origin_tool = self.session_result_handle_origin_tool(&params.result_handle);
        let anchor = match self
            .session_result_handle_lookup(&params.result_handle, &params.match_id)
        {
            SessionResultHandleLookup::Found(anchor) => anchor,
            SessionResultHandleLookup::StaleHandle => {
                let mut recovery = RecoveryFields::stale_read_match(
                    Some(params.result_handle.as_str()),
                    Some(params.match_id.as_str()),
                    params.origin.as_ref(),
                );
                self.validate_recovery_actions(&mut recovery);
                crate::mcp::routing_stats::record_handle_failure();
                let message = recovery
                    .message
                    .clone()
                    .unwrap_or_else(|| "result_handle not found".to_owned());
                let RecoveryFields {
                    error_code,
                    correction_hint,
                    related_tools,
                    next_actions,
                    suggested_next,
                    ..
                } = recovery;
                let result: Result<ReadMatchResponse, ErrorData> = Err(Self::resource_not_found(
                    message,
                    Some(json!({
                        "error_code": error_code.unwrap_or_else(|| "STALE_HANDLE".to_owned()),
                        "result_handle": params.result_handle,
                        "match_id": params.match_id,
                        "correction_hint": correction_hint,
                        "related_tools": related_tools,
                        "next_actions": next_actions,
                        "suggested_next": suggested_next,
                    })),
                ));
                return self.finalize_with_provenance_timed(
                    "read_match",
                    started_at,
                    result,
                    Ok(()),
                    None,
                );
            }
            SessionResultHandleLookup::TargetScopeMismatch => {
                return Err(Self::invalid_params(
                    "result target belongs to a different session",
                    Some(serde_json::json!({"error_code": "TARGET_SCOPE_MISMATCH"})),
                ));
            }
            SessionResultHandleLookup::MixedHandle {
                foreign_handle_has_match,
                foreign_handle,
            } => {
                let mut recovery = RecoveryFields::mixed_read_match(
                    Some(params.result_handle.as_str()),
                    Some(params.match_id.as_str()),
                    params.origin.as_ref(),
                );
                self.validate_recovery_actions(&mut recovery);
                crate::mcp::routing_stats::record_handle_failure();
                let message = if foreign_handle_has_match {
                    format!(
                        "match_id {:?} does not belong to result_handle {:?} (belongs to another handle{}).",
                        params.match_id,
                        params.result_handle,
                        foreign_handle
                            .as_ref()
                            .map(|handle| format!(" {handle:?}"))
                            .unwrap_or_default()
                    )
                } else {
                    recovery
                        .message
                        .clone()
                        .unwrap_or_else(|| "match_id does not belong to result_handle".to_owned())
                };
                let RecoveryFields {
                    error_code,
                    correction_hint,
                    related_tools,
                    next_actions,
                    suggested_next,
                    ..
                } = recovery;
                let result: Result<ReadMatchResponse, ErrorData> = Err(Self::resource_not_found(
                    message,
                    Some(json!({
                        "error_code": error_code.unwrap_or_else(|| "MIXED_HANDLE".to_owned()),
                        "result_handle": params.result_handle,
                        "match_id": params.match_id,
                        "foreign_handle": foreign_handle,
                        "correction_hint": correction_hint,
                        "related_tools": related_tools,
                        "next_actions": next_actions,
                        "suggested_next": suggested_next,
                    })),
                ));
                return self.finalize_with_provenance_timed(
                    "read_match",
                    started_at,
                    result,
                    Ok(()),
                    None,
                );
            }
        };
        let before = params.before.unwrap_or(10).min(MAX_CONTEXT_LINES);
        let after = params.after.unwrap_or(10).min(MAX_CONTEXT_LINES);
        let line_start = anchor.line.saturating_sub(before).max(1);
        let line_end = anchor.line.saturating_add(after);
        let read_params = ReadFileParams {
            path: anchor.path.clone(),
            repository_id: Some(anchor.repository_id.clone()),
            max_bytes: None,
            start_line: Some(line_start),
            end_line: Some(line_end),
            line_count: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: params.include_context_efficiency,
        };
        let read = self
            .read_file_impl_with_provenance(
                read_params,
                ReadFileProvenanceContext::read_match(&params.result_handle, &params.match_id),
                Some(ReadMatchProofExpectation {
                    revision: anchor.revision.clone(),
                    origin_tool: origin_tool.unwrap_or("read_match"),
                    origin: params.origin.clone(),
                    result_handle: params.result_handle.clone(),
                    match_id: params.match_id.clone(),
                    repository_id: anchor.repository_id.clone(),
                    path: anchor.path.clone(),
                }),
            )
            .await?;
        Ok(ReadMatchResponse {
            repository_id: read.repository_id,
            path: read.path,
            line: anchor.line,
            column: anchor.column,
            start_line: line_start,
            end_line: read.end_line.unwrap_or(line_start),
            bytes: read.bytes,
            content: read.content,
            context_efficiency: read.context_efficiency,
        })
    }

    pub(super) async fn explore_impl(
        &self,
        params: ExploreParams,
    ) -> Result<ExploreResponse, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("explore", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut resolved_repository_id: Option<String> = None;
                let mut resolved_path: Option<String> = None;
                let mut resolved_absolute_path: Option<String> = None;
                let mut effective_context_lines: Option<usize> = None;
                let mut effective_max_matches: Option<usize> = None;
                let mut scan_scope = None;
                let mut total_matches = 0usize;
                let mut truncated = false;

                let result = (|| -> Result<ExploreResponse, ErrorData> {
                    let query_started_at = std::time::Instant::now();
                    let context_efficiency_log_enabled =
                        crate::context_efficiency::context_efficiency_log_enabled();
                    let need_context_efficiency =
                        crate::context_efficiency::need_context_efficiency_with_log_state(
                            params_for_blocking.include_context_efficiency,
                            context_efficiency_log_enabled,
                        ) || server.tool_call_display_enabled();
                    let requested_context_lines = params_for_blocking
                        .context_lines
                        .unwrap_or(DEFAULT_CONTEXT_LINES);
                    let context_lines = requested_context_lines.min(MAX_CONTEXT_LINES);
                    effective_context_lines = Some(context_lines);

                    let requested_max_matches = params_for_blocking
                        .max_matches
                        .unwrap_or(DEFAULT_MAX_MATCHES);
                    if requested_max_matches == 0 {
                        return Err(Self::invalid_params(
                            "max_matches must be greater than zero",
                            None,
                        ));
                    }
                    let max_matches =
                        requested_max_matches.min(server.config.max_search_results.max(1));
                    effective_max_matches = Some(max_matches);

                    let operation = params_for_blocking.operation;
                    let query = params_for_blocking
                        .query
                        .as_ref()
                        .map(|value| value.trim().to_owned());
                    let anchor = params_for_blocking.anchor.clone();
                    let resume_from = params_for_blocking.resume_from.clone();
                    if let Err(error) =
                        crate::mcp::types::ContinuationValidationError::reject_mixed_cursor_forms(
                            resume_from.is_some(),
                            params_for_blocking.continuation.is_some(),
                        )
                    {
                        return Err(Self::invalid_params(
                            error.message.clone(),
                            Some(json!({ "continuation": error })),
                        ));
                    }

                    let (
                        matcher,
                        response_query,
                        response_pattern_type,
                        scope,
                        include_scope_content,
                    ) = match operation {
                        ExploreOperation::Probe => {
                            if anchor.is_some() {
                                return Err(Self::invalid_params(
                                    "anchor is not allowed for probe",
                                    None,
                                ));
                            }
                            let Some(query) = query.clone().filter(|value| !value.is_empty())
                            else {
                                return Err(Self::invalid_params("query must not be empty", None));
                            };
                            if let Some(cursor) = resume_from.as_ref() {
                                validate_cursor(cursor).map_err(|message| {
                                    Self::invalid_params(
                                        message,
                                        Some(json!({ "resume_from": cursor })),
                                    )
                                })?;
                            }

                            let pattern_type = params_for_blocking
                                .pattern_type
                                .clone()
                                .unwrap_or(SearchPatternType::Literal);
                            let matcher = match pattern_type.clone() {
                                SearchPatternType::Literal => {
                                    ExploreMatcher::Literal(query.clone())
                                }
                                SearchPatternType::Regex => {
                                    let regex = compile_safe_regex(&query).map_err(|err| {
                                        Self::invalid_params(
                                            format!("invalid query regex: {err}"),
                                            Some(json!({
                                                "query": query,
                                                "regex_error_code": err.code(),
                                            })),
                                        )
                                    })?;
                                    if regex.is_match("") {
                                        return Err(Self::invalid_params(
                                            "query regex must not match empty strings",
                                            Some(json!({ "query": query })),
                                        ));
                                    }
                                    ExploreMatcher::Regex(regex)
                                }
                            };

                            (
                                Some(matcher),
                                Some(query),
                                Some(pattern_type),
                                ExploreScopeRequest {
                                    start_line: resume_from
                                        .as_ref()
                                        .map(|cursor| cursor.line)
                                        .unwrap_or(1),
                                    end_line: None,
                                },
                                false,
                            )
                        }
                        ExploreOperation::Zoom => {
                            if params_for_blocking.query.is_some() {
                                return Err(Self::invalid_params(
                                    "query is not allowed for zoom",
                                    None,
                                ));
                            }
                            if params_for_blocking.pattern_type.is_some() {
                                return Err(Self::invalid_params(
                                    "pattern_type is not allowed for zoom",
                                    None,
                                ));
                            }
                            if resume_from.is_some() {
                                return Err(Self::invalid_params(
                                    "resume_from is not allowed for zoom",
                                    None,
                                ));
                            }
                            if params_for_blocking.continuation.is_some() {
                                return Err(Self::invalid_params(
                                    "continuation is not allowed for zoom",
                                    None,
                                ));
                            }
                            let Some(anchor) = anchor.as_ref() else {
                                return Err(Self::invalid_params(
                                    "anchor is required for zoom",
                                    None,
                                ));
                            };
                            validate_anchor(anchor).map_err(|message| {
                                Self::invalid_params(message, Some(json!({ "anchor": anchor })))
                            })?;
                            let scope_window = line_window_around_anchor(anchor, context_lines);
                            (
                                None,
                                None,
                                None,
                                ExploreScopeRequest {
                                    start_line: scope_window.start_line,
                                    end_line: Some(scope_window.end_line),
                                },
                                true,
                            )
                        }
                        ExploreOperation::Refine => {
                            let Some(anchor) = anchor.as_ref() else {
                                return Err(Self::invalid_params(
                                    "anchor is required for refine",
                                    None,
                                ));
                            };
                            validate_anchor(anchor).map_err(|message| {
                                Self::invalid_params(message, Some(json!({ "anchor": anchor })))
                            })?;
                            let Some(query) = query.clone().filter(|value| !value.is_empty())
                            else {
                                return Err(Self::invalid_params("query must not be empty", None));
                            };
                            let scope_window = line_window_around_anchor(anchor, context_lines);
                            if let Some(cursor) = resume_from.as_ref() {
                                validate_cursor(cursor).map_err(|message| {
                                    Self::invalid_params(
                                        message,
                                        Some(json!({ "resume_from": cursor })),
                                    )
                                })?;
                                if cursor.line < scope_window.start_line
                                    || cursor.line > scope_window.end_line
                                {
                                    return Err(Self::invalid_params(
                                        "resume_from must stay within the refine scan scope",
                                        Some(json!({
                                            "resume_from": cursor,
                                            "scan_scope": scope_window.clone(),
                                        })),
                                    ));
                                }
                            }

                            let pattern_type = params_for_blocking
                                .pattern_type
                                .clone()
                                .unwrap_or(SearchPatternType::Literal);
                            let matcher = match pattern_type.clone() {
                                SearchPatternType::Literal => {
                                    ExploreMatcher::Literal(query.clone())
                                }
                                SearchPatternType::Regex => {
                                    let regex = compile_safe_regex(&query).map_err(|err| {
                                        Self::invalid_params(
                                            format!("invalid query regex: {err}"),
                                            Some(json!({
                                                "query": query,
                                                "regex_error_code": err.code(),
                                            })),
                                        )
                                    })?;
                                    if regex.is_match("") {
                                        return Err(Self::invalid_params(
                                            "query regex must not match empty strings",
                                            Some(json!({ "query": query })),
                                        ));
                                    }
                                    ExploreMatcher::Regex(regex)
                                }
                            };

                            (
                                Some(matcher),
                                Some(query),
                                Some(pattern_type),
                                ExploreScopeRequest {
                                    start_line: scope_window.start_line,
                                    end_line: Some(scope_window.end_line),
                                },
                                true,
                            )
                        }
                    };

                    let read_params = ReadFileParams {
                        path: params_for_blocking.path.clone(),
                        repository_id: params_for_blocking.repository_id.clone(),
                        max_bytes: None,
                        start_line: None,
                        end_line: None,
                        line_count: None,
                        presentation_mode: Some(ReadPresentationMode::Json),
                        include_context_efficiency: None,
                    };
                    let (repository_id, path, display_path) =
                        server.resolve_file_path(&read_params)?;
                    resolved_repository_id = Some(repository_id.clone());
                    resolved_path = Some(display_path.clone());
                    resolved_absolute_path = Some(path.display().to_string());

                    let workspace = server
                        .attached_workspaces_for_repository(Some(repository_id.as_str()))?
                        .into_iter()
                        .find(|workspace| workspace.repository_id == repository_id)
                        .ok_or_else(|| {
                            Self::resource_not_found(
                                "repository_id not found",
                                Some(json!({ "repository_id": repository_id })),
                            )
                        })?;
                    let snapshot = server.file_content_snapshot_for_workspace(&workspace, &path)?;
                    let snapshot_revision = snapshot.source_revision();
                    let snapshot_fingerprints = vec![format!(
                        "{}:{}:{}:{}",
                        repository_id,
                        display_path,
                        snapshot_revision.blake3.to_hex(),
                        snapshot_revision.byte_len,
                    )];
                    let request_digest = Self::explore_continuation_digest(
                        &params_for_blocking,
                        &repository_id,
                        &display_path,
                        operation,
                        response_query.as_deref(),
                        response_pattern_type.as_ref(),
                        anchor.as_ref(),
                    );
                    let continuation_offset = match params_for_blocking.continuation.as_deref() {
                        Some(token) => server
                            .session_continuation_lookup(
                                token,
                                "explore",
                                &request_digest,
                                std::slice::from_ref(&repository_id),
                                &snapshot_fingerprints,
                                ResultUnit::Occurrence,
                            )
                            .map(|binding| binding.next_position)
                            .map_err(|error| {
                                Self::invalid_params(
                                    error.message.clone(),
                                    Some(json!({ "continuation": error })),
                                )
                            })?,
                        None => 0,
                    };
                    // Count the original request before applying a legacy cursor. The page scan
                    // deliberately starts at the cursor, but `total_matches` is request-global.
                    let original_scope = match operation {
                        ExploreOperation::Probe => ExploreScopeRequest {
                            start_line: 1,
                            end_line: None,
                        },
                        ExploreOperation::Zoom | ExploreOperation::Refine => scope.clone(),
                    };
                    let original_scan = snapshot.scan_file_scope_lossy(
                        original_scope,
                        matcher.as_ref(),
                        usize::MAX,
                        None,
                        false,
                        None,
                    );
                    let mut scan = snapshot.scan_file_scope_lossy(
                        scope,
                        matcher.as_ref(),
                        if params_for_blocking.continuation.is_some() {
                            usize::MAX
                        } else {
                            max_matches
                        },
                        if params_for_blocking.continuation.is_some() {
                            None
                        } else {
                            resume_from.as_ref()
                        },
                        include_scope_content,
                        include_scope_content.then_some(server.config.max_file_bytes),
                    );
                    let page_start_position = if params_for_blocking.continuation.is_some() {
                        let all_matches = std::mem::take(&mut scan.matches);
                        let start = continuation_offset.min(all_matches.len());
                        let end = start.saturating_add(max_matches).min(all_matches.len());
                        scan.matches = all_matches[start..end].to_vec();
                        scan.truncated = end < all_matches.len();
                        scan.resume_from = all_matches.get(end).map(|matched| ExploreCursor {
                            line: matched.start_line,
                            column: matched.start_column,
                        });
                        start
                    } else {
                        scan.matches
                            .first()
                            .and_then(|first| {
                                original_scan.matches.iter().position(|candidate| {
                                    candidate.start_line == first.start_line
                                        && candidate.start_column == first.start_column
                                })
                            })
                            .unwrap_or(0)
                    };

                    if let Some(anchor) = anchor.as_ref()
                        && (scan.total_lines == 0 || anchor.end_line > scan.total_lines)
                    {
                        return Err(Self::invalid_params(
                            "anchor is outside file bounds",
                            Some(json!({
                                "anchor": anchor,
                                "total_lines": scan.total_lines,
                            })),
                        ));
                    }
                    if let Some(cursor) = resume_from.as_ref()
                        && cursor.line > scan.total_lines
                        && !(scan.total_lines == 0 && cursor.line == 1)
                    {
                        return Err(Self::invalid_params(
                            "resume_from is outside file bounds",
                            Some(json!({
                                "resume_from": cursor,
                                "total_lines": scan.total_lines,
                            })),
                        ));
                    }

                    let window = if include_scope_content {
                        if !scan.scope_within_budget {
                            return Err(Self::line_slice_budget_error(
                                &display_path,
                                scan.scope_bytes.unwrap_or(0),
                                server.config.max_file_bytes,
                                scope.start_line,
                                scan.effective_scope.end_line,
                                scan.total_lines,
                            ));
                        }

                        Some(ExploreWindow {
                            start_line: scan.effective_scope.start_line,
                            end_line: scan.effective_scope.end_line,
                            bytes: scan.scope_bytes.unwrap_or(0),
                            content: scan.scope_content.clone().unwrap_or_default(),
                        })
                    } else {
                        None
                    };

                    let mut matches = Vec::with_capacity(scan.matches.len());
                    for (index, matched) in scan.matches.iter().enumerate() {
                        let match_window =
                            line_window_around_anchor(&matched.anchor, context_lines);
                        let match_window_slice = snapshot
                            .read_line_slice_lossy(
                                match_window.start_line,
                                Some(match_window.end_line),
                                server.config.max_file_bytes,
                            )
                            .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                        if match_window_slice.bytes > server.config.max_file_bytes {
                            return Err(Self::line_slice_budget_error(
                                &display_path,
                                match_window_slice.bytes,
                                server.config.max_file_bytes,
                                match_window.start_line,
                                match_window.end_line.min(
                                    match_window_slice.total_lines.max(match_window.start_line),
                                ),
                                match_window_slice.total_lines,
                            ));
                        }
                        let match_window_end = if match_window_slice.total_lines == 0 {
                            0
                        } else {
                            match_window.end_line.min(match_window_slice.total_lines)
                        };

                        matches.push(ExploreMatch {
                            match_id: format!("match-{index:04}"),
                            start_line: matched.start_line,
                            start_column: matched.start_column,
                            end_line: matched.end_line,
                            end_column: matched.end_column,
                            excerpt: matched.excerpt.clone(),
                            window: ExploreWindow {
                                start_line: match_window.start_line,
                                end_line: match_window_end,
                                bytes: match_window_slice.bytes,
                                content: match_window_slice.content,
                            },
                            anchor: matched.anchor.clone(),
                        });
                    }

                    scan_scope = Some(scan.effective_scope.clone());
                    total_matches = original_scan.total_matches;
                    truncated = scan.truncated;
                    let continuation = scan.truncated.then(|| {
                        server.store_session_continuation(ContinuationBinding {
                            tool: "explore",
                            request_digest,
                            repository_ids: vec![repository_id.clone()],
                            snapshot_fingerprints,
                            unit: ResultUnit::Occurrence,
                            next_position: page_start_position.saturating_add(scan.matches.len()),
                        })
                    });

                    let context_efficiency = if need_context_efficiency {
                        let returned_source_bytes_estimate = if let Some(window) = window.as_ref() {
                            window.bytes
                        } else {
                            matches
                                .iter()
                                .map(|matched| matched.window.bytes)
                                .fold(0usize, usize::saturating_add)
                        };
                        let context_efficiency = server
                            .context_efficiency_metadata_for_tool_observers(
                                &execution_context_for_blocking,
                                params_for_blocking.include_context_efficiency,
                                context_efficiency_log_enabled,
                                || {
                                    server.read_surface_context_efficiency_metadata(
                                        &repository_id,
                                        &display_path,
                                        returned_source_bytes_estimate,
                                        Some(scan.total_matches),
                                        Some(Self::context_efficiency_elapsed_ms(query_started_at)),
                                    )
                                },
                            )?;
                        if let Some(context_efficiency) = context_efficiency.as_ref() {
                            server.append_context_efficiency_log_for_workspaces(
                                "explore",
                                std::slice::from_ref(&workspace),
                                context_efficiency,
                            );
                        }
                        if params_for_blocking.include_context_efficiency == Some(true) {
                            context_efficiency
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    Ok(ExploreResponse {
                        repository_id,
                        path: display_path,
                        operation,
                        query: response_query,
                        pattern_type: response_pattern_type,
                        total_lines: scan.total_lines,
                        scan_scope: scan.effective_scope,
                        window,
                        total_matches: original_scan.total_matches,
                        matches,
                        truncated: scan.truncated,
                        resume_from: scan.resume_from,
                        completeness: ResultCompleteness::try_new(
                            ResultUnit::Occurrence,
                            scan.matches.len(),
                            Some(original_scan.total_matches),
                            !scan.truncated,
                            scan.truncated,
                            scan.truncated
                                .then_some(ResultTruncationReason::PageLimit)
                                .into_iter()
                                .collect(),
                            Vec::new(),
                            continuation,
                        )
                        .map_err(|error| {
                            Self::internal(
                                format!("invalid explore completeness state: {error}"),
                                None,
                            )
                        })?,
                        metadata: ExploreMetadata {
                            lossy_utf8: scan.lossy_utf8,
                            effective_context_lines: context_lines,
                            effective_max_matches: max_matches,
                            context_efficiency,
                        },
                    })
                })();

                ExploreExecution {
                    result,
                    resolved_repository_id,
                    resolved_path,
                    resolved_absolute_path,
                    effective_context_lines,
                    effective_max_matches,
                    scan_scope,
                    total_matches,
                    truncated,
                }
            })
            .await?;

        let result = execution.result;
        let repository_ids = execution
            .resolved_repository_id
            .clone()
            .or_else(|| execution_context.repository_hint.clone())
            .into_iter()
            .collect::<Vec<_>>();
        let metadata =
            execution_context.normalized_workload(&repository_ids, WorkloadPrecisionMode::Exact);
        let provenance_result = self
            .record_provenance_blocking_with_metadata(
                "explore",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "path": Self::bounded_text(&params.path),
                    "operation": params.operation,
                    "query": params.query.as_ref().map(|value| Self::bounded_text(value)),
                    "pattern_type": params.pattern_type,
                    "context_lines": params.context_lines,
                    "max_matches": params.max_matches,
                    "resume_from": params.resume_from,
                    "effective_context_lines": execution.effective_context_lines,
                    "effective_max_matches": execution.effective_max_matches,
                }),
                json!({
                    "resolved_repository_id": execution.resolved_repository_id,
                    "resolved_path": execution
                        .resolved_path
                        .map(|path| Self::bounded_text(&path)),
                    "resolved_absolute_path": execution
                        .resolved_absolute_path
                        .map(|path| Self::bounded_text(&path)),
                    "scan_scope": execution.scan_scope,
                    "total_matches": execution.total_matches,
                    "truncated": execution.truncated,
                }),
                Some(metadata),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    fn explore_continuation_digest(
        params: &ExploreParams,
        repository_id: &str,
        path: &str,
        operation: ExploreOperation,
        query: Option<&str>,
        pattern_type: Option<&SearchPatternType>,
        anchor: Option<&ExploreAnchor>,
    ) -> String {
        let normalized = json!({
            "repository_id": repository_id,
            "path": path,
            "operation": operation,
            "query": query,
            "pattern_type": pattern_type,
            "anchor": anchor,
            "context_lines": params.context_lines,
            "max_matches": params.max_matches,
        });
        let mut hasher = DefaultHasher::new();
        normalized.to_string().hash(&mut hasher);
        format!("explore-v2:{:016x}", hasher.finish())
    }

    pub(super) fn read_presentation_mode(
        mode: Option<ReadPresentationMode>,
    ) -> ReadPresentationMode {
        mode.unwrap_or(ReadPresentationMode::Text)
    }

    pub(super) fn explore_presentation_mode(
        params: &ExploreParams,
    ) -> Result<ReadPresentationMode, ErrorData> {
        match params.presentation_mode {
            Some(ReadPresentationMode::Json) => Ok(ReadPresentationMode::Json),
            Some(mode @ (ReadPresentationMode::Text | ReadPresentationMode::Citation))
                if matches!(
                    params.operation,
                    ExploreOperation::Probe | ExploreOperation::Refine
                ) =>
            {
                Err(Self::invalid_params(
                    "presentation_mode=text and presentation_mode=citation are only supported for zoom",
                    Some(json!({
                        "operation": params.operation,
                        "presentation_mode": mode,
                    })),
                ))
            }
            Some(ReadPresentationMode::Text) => Ok(ReadPresentationMode::Text),
            Some(ReadPresentationMode::Citation) => Ok(ReadPresentationMode::Citation),
            None if params.operation == ExploreOperation::Zoom => Ok(ReadPresentationMode::Text),
            None => Ok(ReadPresentationMode::Json),
        }
    }

    pub(super) fn present_read_file_result(
        &self,
        params: &ReadFileParams,
        response: ReadFileResponse,
    ) -> Result<CallToolResult, ErrorData> {
        match Self::read_presentation_mode(params.presentation_mode) {
            ReadPresentationMode::Json => Self::structured_tool_result(&response),
            ReadPresentationMode::Text => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                Ok(Self::text_read_surface_result(response.content))
            }
            ReadPresentationMode::Citation => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                let start_line = response.start_line.unwrap_or(1);
                Ok(Self::text_read_surface_result(Self::format_citation_text(
                    start_line,
                    &response.content,
                )))
            }
        }
    }

    pub(super) fn present_read_match_result(
        &self,
        params: &ReadMatchParams,
        response: ReadMatchResponse,
    ) -> Result<CallToolResult, ErrorData> {
        match Self::read_presentation_mode(params.presentation_mode) {
            ReadPresentationMode::Json => Self::structured_tool_result(&response),
            ReadPresentationMode::Text => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                Ok(Self::text_read_surface_result(response.content))
            }
            ReadPresentationMode::Citation => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                Ok(Self::text_read_surface_result(Self::format_citation_text(
                    response.start_line,
                    &response.content,
                )))
            }
        }
    }

    pub(super) fn present_explore_result(
        &self,
        params: &ExploreParams,
        response: ExploreResponse,
    ) -> Result<CallToolResult, ErrorData> {
        let mode = Self::explore_presentation_mode(params)?;
        match mode {
            ReadPresentationMode::Json => Self::structured_tool_result(&response),
            ReadPresentationMode::Text | ReadPresentationMode::Citation => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                let Some(window) = response.window else {
                    return Err(Self::internal(
                        "explore zoom response missing window",
                        Some(json!({
                            "operation": response.operation,
                            "path": response.path,
                        })),
                    ));
                };
                let content = if mode == ReadPresentationMode::Citation {
                    Self::format_citation_text(window.start_line, &window.content)
                } else {
                    window.content
                };
                Ok(Self::text_read_surface_result(content))
            }
        }
    }

    /// Format source as `LINE|content` lines for citation-trained agents.
    pub(super) fn format_citation_text(start_line: usize, content: &str) -> String {
        if content.is_empty() {
            return String::new();
        }
        let ends_with_newline = content.ends_with('\n');
        let mut out = String::with_capacity(content.len().saturating_add(content.len() / 8 + 8));
        for (offset, line) in content.lines().enumerate() {
            let line_no = start_line.saturating_add(offset);
            out.push_str(&line_no.to_string());
            out.push('|');
            out.push_str(line);
            out.push('\n');
        }
        if !ends_with_newline && out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn read_surface_context_efficiency_metadata(
        &self,
        repository_id: &str,
        path: &str,
        returned_source_bytes_estimate: usize,
        returned_match_count: Option<usize>,
        query_duration_ms: Option<u64>,
    ) -> Result<ContextEfficiencyMetadata, ErrorData> {
        let workspace = self
            .attached_workspaces_for_repository(Some(repository_id))?
            .into_iter()
            .find(|workspace| workspace.repository_id == repository_id);
        let summary = workspace
            .as_ref()
            .map(|workspace| {
                if !workspace.db_path.exists() {
                    return Ok(None);
                }
                Storage::new(&workspace.db_path)
                    .load_latest_context_efficiency_manifest_summary_for_repository(
                        &workspace.runtime_repository_id,
                    )
                    .map_err(Self::map_frigg_error)
            })
            .transpose()?
            .flatten();

        let indexed_readable_files = summary
            .as_ref()
            .map(|summary| summary.indexed_readable_files)
            .unwrap_or(0);
        let indexed_readable_bytes = summary
            .as_ref()
            .map(|summary| summary.indexed_readable_bytes)
            .unwrap_or(0);
        let indexed_min_mtime_ns = summary.as_ref().and_then(|summary| summary.min_mtime_ns);
        let indexed_max_mtime_ns = summary.as_ref().and_then(|summary| summary.max_mtime_ns);
        let returned_source_bytes_estimate =
            u64::try_from(returned_source_bytes_estimate).unwrap_or(u64::MAX);
        let returned_unique_file_bytes = summary
            .as_ref()
            .and_then(|summary| {
                summary.file_size_bytes_for_path(
                    workspace.as_ref().map(|workspace| workspace.root.as_path()),
                    path,
                )
            })
            .or(Some(returned_source_bytes_estimate));

        Ok(Self::finalize_context_efficiency_metadata(
            ContextEfficiencyMetadata {
                indexed_readable_files,
                indexed_readable_bytes,
                indexed_min_mtime_ns,
                indexed_max_mtime_ns,
                candidate_input_count: None,
                candidate_output_count: None,
                returned_match_count,
                returned_unique_paths: Some(1),
                returned_unique_file_bytes,
                returned_source_bytes_estimate: Some(returned_source_bytes_estimate),
                matched_file_context_saved_bytes_estimate: None,
                matched_file_context_saved_percent_estimate: None,
                corpus_context_saved_bytes_estimate: None,
                corpus_context_saved_percent_estimate: None,
                corpus_narrowing_ratio_estimate: None,
                query_duration_ms,
                narrowing_ratio_estimate: None,
                stage_attribution: None,
            },
        ))
    }

    fn structured_tool_result<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
        serde_json::to_value(value)
            .map(CallToolResult::structured)
            .map_err(|err| {
                Self::internal(
                    format!("failed to serialize structured tool result: {err}"),
                    None,
                )
            })
    }

    fn reject_text_context_efficiency(
        include_context_efficiency: Option<bool>,
    ) -> Result<(), ErrorData> {
        if include_context_efficiency == Some(true) {
            return Err(Self::invalid_params(
                "include_context_efficiency requires presentation_mode=json",
                Some(json!({
                    "include_context_efficiency": true,
                    "presentation_mode": "text_or_citation",
                    "supported_presentation_mode": ReadPresentationMode::Json,
                })),
            ));
        }
        Ok(())
    }

    /// Text/citation modes return raw selected bytes only — no path headers or structuredContent.
    fn text_read_surface_result(content: String) -> CallToolResult {
        CallToolResult::success(vec![ContentBlock::text(content)])
    }

    pub(super) fn map_lossy_line_slice_error(path: &Path, error: LossyLineSliceError) -> ErrorData {
        match error {
            LossyLineSliceError::Io(err) => Self::internal(
                format!("failed to read file {}: {err}", path.display()),
                None,
            ),
            LossyLineSliceError::LineStartOutside {
                line_start,
                line_end,
                total_lines,
            } => Self::invalid_params(
                "start_line is outside file bounds",
                Some(json!({
                    "start_line": line_start,
                    "end_line": line_end,
                    "total_lines": total_lines,
                })),
            ),
        }
    }

    fn line_slice_budget_error(
        path: &str,
        bytes: usize,
        max_bytes: usize,
        line_start: usize,
        line_end: usize,
        total_lines: usize,
    ) -> ErrorData {
        Self::invalid_params(
            format!("selected line range exceeds max_bytes={max_bytes}"),
            Some(json!({
                "path": path,
                "bytes": bytes,
                "max_bytes": max_bytes,
                "config_max_file_bytes": max_bytes,
                "start_line": line_start,
                "end_line": line_end,
                "total_lines": total_lines,
            })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ErrorCode;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_workspace_root(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "frigg-content-{test_name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".git")).expect("fixture git marker should be creatable");
        root
    }

    struct ReadMatchFixture {
        server: FriggMcpServer,
        workspace_root: PathBuf,
        source_path: PathBuf,
        result_handle: String,
        match_id: String,
        original_content: &'static str,
    }

    async fn read_match_fixture(test_name: &str) -> ReadMatchFixture {
        let workspace_root = temporary_workspace_root(test_name);
        fs::create_dir_all(workspace_root.join("src")).expect("create workspace source directory");
        let source_path = workspace_root.join("src/lib.rs");
        let original_content = "pub fn proof_marker() {}\n";
        fs::write(&source_path, original_content).expect("write source fixture");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("fixture config should be valid");
        let repository = config
            .repositories()
            .into_iter()
            .next()
            .expect("fixture config should declare a repository");
        let repository_root = PathBuf::from(&repository.root_path);
        let db_path = crate::storage::ensure_provenance_db_parent_dir(&repository_root)
            .expect("storage path should resolve");
        Storage::new(&db_path)
            .initialize()
            .expect("storage should initialize");
        crate::indexer::index_repository_with_runtime_config(
            &repository.repository_id.0,
            &repository_root,
            &db_path,
            IndexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
        )
        .expect("fixture index should succeed");

        let server = FriggMcpServer::new(config);
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        server
            .adopt_workspace(&workspace, true)
            .expect("server should adopt fixture workspace");
        let search = server
            .search_text_impl(crate::mcp::types::SearchTextParams {
                query: "proof_marker".to_owned(),
                pattern_type: None,
                repository_id: Some(workspace.repository_id),
                path_regex: None,
                limit: None,
                context_lines: None,
                case_sensitive: None,
                ignore_case: None,
                word: None,
                files_with_matches: None,
                count_only: None,
                glob: None,
                exclude_glob: None,
                include_hidden: None,
                max_count_per_file: None,
                collapse_by_file: None,
                continuation: None,
                response_mode: None,
                include_context_efficiency: None,
            })
            .await
            .expect("search should succeed")
            .0;

        ReadMatchFixture {
            server,
            workspace_root,
            source_path,
            result_handle: search.result_handle.expect("search should issue a handle"),
            match_id: search
                .matches
                .first()
                .and_then(|matched| matched.match_id.clone())
                .expect("search match should carry an id"),
            original_content,
        }
    }

    async fn read_fixture_match(
        fixture: &ReadMatchFixture,
    ) -> Result<ReadMatchResponse, ErrorData> {
        fixture
            .server
            .read_match_impl(ReadMatchParams {
                result_handle: fixture.result_handle.clone(),
                match_id: fixture.match_id.clone(),
                before: Some(0),
                after: Some(0),
                presentation_mode: None,
                include_context_efficiency: None,
                origin: None,
            })
            .await
    }

    fn assert_stale_proof_anchor(error: ErrorData, expected_path: &str) {
        assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
        let data = error.data.expect("stale proof error data");
        assert_eq!(
            data.get("error_code").and_then(Value::as_str),
            Some("STALE_PROOF_ANCHOR")
        );
        assert_eq!(
            data.get("path").and_then(Value::as_str),
            Some(expected_path)
        );
        assert!(data.get("content").is_none());
        assert!(data.get("suggested_next").is_none());
        assert!(data.get("next_actions").is_none());
    }

    #[test]
    fn bounded_proof_read_refuses_growth_without_unbounded_allocation() {
        let directory = temporary_workspace_root("bounded-proof-read");
        fs::create_dir_all(&directory).expect("create fixture directory");
        let path = directory.join("source.rs");
        fs::write(&path, b"0123456789").expect("write oversized fixture");

        assert!(matches!(
            read_proof_bytes_bounded(&path, 4),
            Err(BoundedProofReadError::TooLarge)
        ));
        assert_eq!(
            read_proof_bytes_bounded(&path, 10).expect("exact-bound source succeeds"),
            b"0123456789"
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[tokio::test]
    async fn read_match_rejects_changed_proof_anchor_without_source_content() {
        let workspace_root = temporary_workspace_root("stale-proof-anchor");
        fs::create_dir_all(workspace_root.join("src")).expect("create workspace source directory");
        let source_path = workspace_root.join("src/lib.rs");
        fs::write(&source_path, "pub fn proof_marker() {}\n").expect("write source fixture");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("fixture config should be valid");
        let repository = config
            .repositories()
            .into_iter()
            .next()
            .expect("fixture config should declare a repository");
        let repository_root = PathBuf::from(&repository.root_path);
        let db_path = crate::storage::ensure_provenance_db_parent_dir(&repository_root)
            .expect("storage path should resolve");
        Storage::new(&db_path)
            .initialize()
            .expect("storage should initialize");
        crate::indexer::index_repository_with_runtime_config(
            &repository.repository_id.0,
            &repository_root,
            &db_path,
            IndexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
        )
        .expect("fixture index should succeed");

        let server = FriggMcpServer::new(config);
        let workspace = server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("server should register workspace");
        server
            .adopt_workspace(&workspace, true)
            .expect("server should adopt fixture workspace");
        let search = server
            .search_text_impl(crate::mcp::types::SearchTextParams {
                query: "proof_marker".to_owned(),
                pattern_type: None,
                repository_id: Some(workspace.repository_id.clone()),
                path_regex: None,
                limit: None,
                context_lines: None,
                case_sensitive: None,
                ignore_case: None,
                word: None,
                files_with_matches: None,
                count_only: None,
                glob: None,
                exclude_glob: None,
                include_hidden: None,
                max_count_per_file: None,
                collapse_by_file: None,
                continuation: None,
                response_mode: None,
                include_context_efficiency: None,
            })
            .await
            .expect("search should succeed")
            .0;
        let result_handle = search.result_handle.expect("search should issue a handle");
        let match_id = search
            .matches
            .first()
            .and_then(|matched| matched.match_id.clone())
            .expect("search match should carry an id");

        fs::write(
            &source_path,
            "// inserted before proof\npub fn proof_marker() {}\n",
        )
        .expect("rewrite source after handle issuance");
        let error = server
            .read_match_impl(ReadMatchParams {
                result_handle,
                match_id,
                before: Some(0),
                after: Some(0),
                presentation_mode: None,
                include_context_efficiency: None,
                origin: None,
            })
            .await
            .expect_err("changed source must fail closed");

        assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
        let data = error.data.expect("stale proof error data");
        assert_eq!(
            data.get("error_code").and_then(Value::as_str),
            Some("STALE_PROOF_ANCHOR")
        );
        assert_eq!(data.get("path").and_then(Value::as_str), Some("src/lib.rs"));
        assert!(data.get("content").is_none());
        assert!(data.get("suggested_next").is_none());
        assert!(data.get("next_actions").is_none());
        assert!(
            !data
                .to_string()
                .contains(&source_path.display().to_string()),
            "error data must not expose the absolute source path"
        );

        let _ = fs::remove_dir_all(workspace_root);
    }

    #[tokio::test]
    async fn read_match_accepts_an_identical_byte_rewrite() {
        let fixture = read_match_fixture("identical-proof-rewrite").await;
        fs::write(&fixture.source_path, fixture.original_content)
            .expect("identical rewrite should be writable");

        let response = read_fixture_match(&fixture)
            .await
            .expect("identical raw bytes must preserve the proof anchor");

        assert_eq!(response.content, fixture.original_content.trim_end());
        assert_eq!(response.bytes, fixture.original_content.trim_end().len());
        let _ = fs::remove_dir_all(fixture.workspace_root);
    }

    #[tokio::test]
    async fn read_match_rejects_deleted_replaced_and_newly_oversized_proof_sources() {
        let deleted = read_match_fixture("deleted-proof-source").await;
        fs::remove_file(&deleted.source_path).expect("fixture source should be removable");
        assert_stale_proof_anchor(
            read_fixture_match(&deleted)
                .await
                .expect_err("deleted proof source must fail closed"),
            "src/lib.rs",
        );
        let _ = fs::remove_dir_all(deleted.workspace_root);

        let replaced = read_match_fixture("replaced-proof-source").await;
        fs::write(&replaced.source_path, "pub fn replacement_marker() {}\n")
            .expect("replacement fixture should be writable");
        assert_stale_proof_anchor(
            read_fixture_match(&replaced)
                .await
                .expect_err("replacement proof source must fail closed"),
            "src/lib.rs",
        );
        let _ = fs::remove_dir_all(replaced.workspace_root);

        let oversized = read_match_fixture("oversized-proof-source").await;
        fs::write(
            &oversized.source_path,
            vec![b'x'; oversized.server.config.max_file_bytes.saturating_add(1)],
        )
        .expect("oversized fixture should be writable");
        assert_stale_proof_anchor(
            read_fixture_match(&oversized)
                .await
                .expect_err("newly oversized proof source must fail closed"),
            "src/lib.rs",
        );
        let _ = fs::remove_dir_all(oversized.workspace_root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_match_rejects_identical_byte_external_symlink_replacement() {
        let fixture = read_match_fixture("external-symlink-proof-source").await;
        let resolved_before_swap = fixture
            .source_path
            .canonicalize()
            .expect("fixture source should resolve before the swap");
        let outside_path = fixture.workspace_root.with_extension("outside.rs");
        fs::write(&outside_path, fixture.original_content)
            .expect("outside fixture should be writable");
        fs::remove_file(&fixture.source_path).expect("fixture source should be removable");
        std::os::unix::fs::symlink(&outside_path, &fixture.source_path)
            .expect("external symlink replacement should be creatable");

        let workspace = fixture
            .server
            .known_workspaces()
            .into_iter()
            .next()
            .expect("fixture workspace should remain known");
        assert!(
            fixture
                .server
                .file_content_snapshot_for_bound_proof(&workspace, &resolved_before_swap)
                .is_err(),
            "the post-resolution proof read must reject an external symlink even with identical bytes"
        );
        assert_stale_proof_anchor(
            read_fixture_match(&fixture)
                .await
                .expect_err("external identical-byte replacement must fail closed"),
            "src/lib.rs",
        );

        let _ = fs::remove_file(&outside_path);
        let _ = fs::remove_dir_all(fixture.workspace_root);
    }
}
