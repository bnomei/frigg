//! `list_files` implementation for repository-aware file discovery.
//!
//! Lists manifest paths under repository roots with ignore-aware filtering for agent file
//! discovery.

use super::*;
use crate::mcp::server_cache::ContinuationBinding;
use crate::mcp::types::{
    ContinuationValidationError, ListFilesEntry, ResultCompleteness, ResultTruncationReason,
    ResultUnit,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const DEFAULT_LIST_FILES_LIMIT: usize = 1000;
const MAX_LIST_FILES_LIMIT: usize = 5000;

impl FriggMcpServer {
    pub(crate) async fn list_files_impl(
        &self,
        params: ListFilesParams,
    ) -> Result<Json<ListFilesResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("list_files", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let (result, provenance_result) = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut scoped_repository_ids: Vec<String> = Vec::new();
                let mut effective_limit: Option<usize> = None;
                let mut diagnostics_count = 0usize;
                let result = (|| -> Result<Json<ListFilesResponse>, ErrorData> {
                    if let Err(error) = ContinuationValidationError::reject_mixed_cursor_forms(
                        params_for_blocking.resume_from.is_some(),
                        params_for_blocking.continuation.is_some(),
                    ) {
                        return Err(Self::invalid_params(
                            error.message.clone(),
                            Some(json!({ "continuation": error })),
                        ));
                    }
                    if params_for_blocking.limit == Some(0) {
                        return Err(Self::invalid_params(
                            "limit must be greater than zero when provided",
                            None,
                        ));
                    }
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(DEFAULT_LIST_FILES_LIMIT)
                        .min(MAX_LIST_FILES_LIMIT);
                    effective_limit = Some(limit);

                    let path_regex = match params_for_blocking.path_regex.clone() {
                        Some(raw) => {
                            Some(server.compile_cached_safe_regex(&raw).map_err(|err| {
                                Self::invalid_params(
                                    format!("invalid path_regex: {err}"),
                                    Some(json!({
                                        "path_regex": raw,
                                        "regex_error_code": err.code(),
                                    })),
                                )
                            })?)
                        }
                        None => None,
                    };
                    let glob_regex =
                        Self::compile_optional_path_glob(&server, "glob", &params_for_blocking.glob)?;
                    let language = match params_for_blocking.language.as_deref() {
                        Some(raw) => {
                            let normalized = raw.trim();
                            if normalized.is_empty() {
                                return Err(Self::invalid_params(
                                    "language must not be empty when provided",
                                    None,
                                ));
                            }
                            Some(parse_supported_language(
                                normalized,
                                LanguageCapability::SourceFilter,
                            )
                            .ok_or_else(|| {
                                Self::invalid_params(
                                    format!("unsupported language filter '{normalized}'"),
                                    Some(json!({
                                        "language": normalized,
                                        "supported_values": SymbolLanguage::supported_search_filter_values(),
                                    })),
                                )
                            })?)
                        }
                        None => None,
                    };
                    let path_class = params_for_blocking.path_class.map(|class| class.as_str());
                    let include_hidden = params_for_blocking.include_hidden.unwrap_or(false);

                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::ManifestOnly,
                    )?;
                    let scoped_workspaces = scoped_execution_context.scoped_workspaces.clone();
                    scoped_repository_ids = scoped_execution_context.scoped_repository_ids.clone();
                    // `list_files` builds from a live manifest, so its continuation identity
                    // must use that same live-manifest source on both issuance and replay.
                    let mut snapshot_fingerprints = Vec::with_capacity(scoped_workspaces.len());
                    let mut manifest_outputs = Vec::with_capacity(scoped_workspaces.len());
                    for workspace in &scoped_workspaces {
                        let output = ManifestBuilder::default()
                            .build_metadata_with_diagnostics(&workspace.root)
                            .map_err(Self::map_frigg_error)?;
                        snapshot_fingerprints.push(format!(
                            "{}:live:{}",
                            workspace.repository_id,
                            Self::root_signature(&output.entries)
                        ));
                        diagnostics_count =
                            diagnostics_count.saturating_add(output.diagnostics.len());
                        manifest_outputs.push((workspace, output));
                    }
                    let cursor_scope =
                        Self::list_files_cursor_scope(&params_for_blocking, &scoped_repository_ids);
                    let request_digest = format!("list-files-v2:{cursor_scope:016x}");
                    let resume_offset = match params_for_blocking.continuation.as_deref() {
                        Some(token) => server
                            .session_continuation_lookup(
                                token,
                                "list_files",
                                &request_digest,
                                &scoped_repository_ids,
                                &snapshot_fingerprints,
                                ResultUnit::File,
                            )
                            .map(|binding| binding.next_position)
                            .map_err(|error| Self::invalid_params(error.message.clone(), Some(json!({ "continuation": error }))))?,
                        None => match params_for_blocking.resume_from.as_deref() {
                            Some(raw) => Self::parse_list_files_resume_cursor(raw, cursor_scope)?,
                            None => 0,
                        },
                    };

                    let mut files = Vec::new();
                    for (workspace, output) in manifest_outputs {
                        for entry in output.entries {
                            let path = Self::relative_display_path(&workspace.root, &entry.path);
                            if !include_hidden && Self::repository_path_is_hidden(&path) {
                                continue;
                            }
                            if let Some(path_regex) = &path_regex
                                && !path_regex.is_match(&path)
                            {
                                continue;
                            }
                            if let Some(glob_regex) = &glob_regex
                                && !glob_regex.is_match(&path)
                            {
                                continue;
                            }
                            if let Some(language) = language
                                && SymbolLanguage::from_path(std::path::Path::new(&path))
                                    != Some(language)
                            {
                                continue;
                            }
                            if let Some(path_class) = path_class
                                && repository_path_class(&path) != path_class
                            {
                                continue;
                            }
                            files.push(ListFilesEntry {
                                repository_id: workspace.repository_id.clone(),
                                path,
                                size_bytes: entry.size_bytes,
                            });
                        }
                    }

                    files.sort_by(|left, right| {
                        left.repository_id
                            .cmp(&right.repository_id)
                            .then(left.path.cmp(&right.path))
                    });
                    let total_files = files.len();
                    let mut page = if resume_offset >= total_files {
                        Vec::new()
                    } else {
                        files.split_off(resume_offset)
                    };
                    let truncated = page.len() > limit;
                    page.truncate(limit);
                    let next_resume_from = truncated.then(|| {
                        Self::format_list_files_resume_cursor(
                            cursor_scope,
                            resume_offset.saturating_add(page.len()),
                        )
                    });
                    let continuation = truncated.then(|| {
                        server.store_session_continuation(ContinuationBinding {
                            tool: "list_files",
                            request_digest: request_digest.clone(),
                            repository_ids: scoped_repository_ids.clone(),
                            snapshot_fingerprints: snapshot_fingerprints.clone(),
                            unit: ResultUnit::File,
                            next_position: resume_offset.saturating_add(page.len()),
                        })
                    });
                    let completeness = ResultCompleteness::try_new(
                        ResultUnit::File,
                        page.len(),
                        Some(total_files),
                        !truncated,
                        truncated,
                        truncated.then_some(ResultTruncationReason::PageLimit).into_iter().collect(),
                        Vec::new(),
                        continuation,
                    ).map_err(|error| Self::internal(format!("invalid list_files completeness state: {error}"), None))?;

                    Ok(Json(ListFilesResponse {
                        total_files,
                        files: page,
                        truncated,
                        resume_from: next_resume_from,
                        completeness,
                    }))
                })();

                let total_files = result
                    .as_ref()
                    .map(|response| response.0.total_files)
                    .unwrap_or(0);
                let finalization =
                    server.tool_execution_finalization(
                        json!({
                            "scoped_repository_ids": scoped_repository_ids.clone(),
                            "total_files": total_files,
                            "diagnostics_count": diagnostics_count,
                        }),
                        Some(execution_context_for_blocking.normalized_workload(
                            &scoped_repository_ids,
                            WorkloadPrecisionMode::Exact,
                        )),
                    );
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    "list_files",
                    execution_context_for_blocking.repository_hint.as_deref(),
                    json!({
                        "repository_id": execution_context_for_blocking.repository_hint,
                        "path_regex": params_for_blocking
                            .path_regex
                            .as_ref()
                            .map(|raw| Self::bounded_text(raw)),
                        "limit": params_for_blocking.limit,
                        "glob": params_for_blocking
                            .glob
                            .as_ref()
                            .map(|raw| Self::bounded_text(raw)),
                        "language": params_for_blocking.language,
                        "path_class": params_for_blocking.path_class,
                        "include_hidden": params_for_blocking.include_hidden,
                        "resume_from": params_for_blocking.resume_from,
                        "effective_limit": effective_limit,
                    }),
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                    finalization.normalized_workload,
                );

                (result, provenance_result)
            })
            .await?;

        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    fn list_files_cursor_scope(params: &ListFilesParams, scoped_repository_ids: &[String]) -> u64 {
        let mut hasher = DefaultHasher::new();
        scoped_repository_ids.hash(&mut hasher);
        params.repository_id.hash(&mut hasher);
        params.path_regex.hash(&mut hasher);
        params.glob.hash(&mut hasher);
        params.language.hash(&mut hasher);
        params
            .path_class
            .map(|path_class| path_class.as_str())
            .hash(&mut hasher);
        params.include_hidden.hash(&mut hasher);
        hasher.finish()
    }

    fn format_list_files_resume_cursor(scope: u64, offset: usize) -> String {
        format!("v1:{scope:016x}:{offset}")
    }

    fn parse_list_files_resume_cursor(raw: &str, expected_scope: u64) -> Result<usize, ErrorData> {
        let Some(rest) = raw.strip_prefix("v1:") else {
            return Err(Self::invalid_params(
                "resume_from must be a cursor returned by list_files",
                Some(json!({ "resume_from": raw })),
            ));
        };
        let Some((scope_hex, offset_raw)) = rest.split_once(':') else {
            return Err(Self::invalid_params(
                "resume_from must be a cursor returned by list_files",
                Some(json!({ "resume_from": raw })),
            ));
        };
        let scope = u64::from_str_radix(scope_hex, 16).map_err(|_| {
            Self::invalid_params(
                "resume_from must be a cursor returned by list_files",
                Some(json!({ "resume_from": raw })),
            )
        })?;
        if scope != expected_scope {
            return Err(Self::invalid_params(
                "resume_from cursor does not match the current list_files scope",
                Some(json!({
                    "resume_from": raw,
                    "expected_scope": format!("{expected_scope:016x}"),
                    "cursor_scope": scope_hex,
                })),
            ));
        }
        offset_raw.parse::<usize>().map_err(|_| {
            Self::invalid_params(
                "resume_from must be a cursor returned by list_files",
                Some(json!({ "resume_from": raw })),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_files_resume_cursor_rejects_scope_mismatch() {
        let cursor = FriggMcpServer::format_list_files_resume_cursor(0xabc, 42);
        let error = FriggMcpServer::parse_list_files_resume_cursor(&cursor, 0xdef)
            .expect_err("cursor from a different scope should be rejected");

        assert!(
            error
                .message
                .contains("does not match the current list_files scope"),
            "unexpected error: {error:?}"
        );
    }
}
