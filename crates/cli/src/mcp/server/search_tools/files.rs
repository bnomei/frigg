//! `list_files` implementation for repository-aware file discovery.

use super::*;
use crate::mcp::types::ListFilesEntry;

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
                    if params_for_blocking.limit == Some(0) {
                        return Err(Self::invalid_params(
                            "limit must be greater than zero when provided",
                            None,
                        ));
                    }
                    let resume_offset = match params_for_blocking.resume_from.as_deref() {
                        Some(raw) => raw.parse::<usize>().map_err(|_| {
                            Self::invalid_params(
                                "resume_from must be a cursor returned by list_files",
                                Some(json!({
                                    "resume_from": raw,
                                })),
                            )
                        })?,
                        None => 0,
                    };
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

                    let mut files = Vec::new();
                    for workspace in &scoped_workspaces {
                        let output = ManifestBuilder::default()
                            .build_metadata_with_diagnostics(&workspace.root)
                            .map_err(Self::map_frigg_error)?;
                        diagnostics_count =
                            diagnostics_count.saturating_add(output.diagnostics.len());

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
                    let next_resume_from = truncated
                        .then(|| resume_offset.saturating_add(page.len()).to_string());

                    Ok(Json(ListFilesResponse {
                        total_files,
                        files: page,
                        truncated,
                        resume_from: next_resume_from,
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
}
