//! `inspect_syntax_tree` and `search_structural` Tree-sitter query tools.
//!
//! Runs Tree-sitter queries against repository-scoped sources with freshness metadata; structural
//! hits are not cached as final query answers.

use super::*;
use crate::indexer::{
    search_structural_grouped_in_source, search_structural_grouped_with_follow_up_in_source,
};
use crate::mcp::server_cache::ContinuationBinding;
use crate::mcp::types::{
    ResultCompleteness, ResultIncompleteReason, ResultTruncationReason, ResultUnit,
};
use crate::mcp::types::{StructuralAnchorSelection, StructuralCaptureItem, StructuralResultMode};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

impl FriggMcpServer {
    const INSPECT_SYNTAX_TREE_DEFAULT_MAX_ANCESTORS: usize = 8;
    const INSPECT_SYNTAX_TREE_MAX_ANCESTORS: usize = 24;
    const INSPECT_SYNTAX_TREE_DEFAULT_MAX_CHILDREN: usize = 12;
    const INSPECT_SYNTAX_TREE_MAX_CHILDREN: usize = 32;

    fn syntax_tree_node_item(
        path: &str,
        node: crate::indexer::SyntaxTreeInspectionNode,
    ) -> SyntaxTreeNodeItem {
        SyntaxTreeNodeItem {
            kind: node.kind,
            named: node.named,
            path: path.to_owned(),
            line: node.span.start_line,
            column: node.span.start_column,
            end_line: node.span.end_line,
            end_column: node.span.end_column,
            excerpt: node.excerpt,
        }
    }

    fn structural_capture_item(
        capture: crate::indexer::StructuralQueryCapture,
    ) -> StructuralCaptureItem {
        StructuralCaptureItem {
            name: capture.name,
            line: capture.span.start_line,
            column: capture.span.start_column,
            end_line: capture.span.end_line,
            end_column: capture.span.end_column,
            excerpt: capture.excerpt,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn structural_noisy_result_hints(
        result_mode: StructuralResultMode,
        primary_capture: Option<&str>,
        path_regex_supplied: bool,
        files_scanned: usize,
        files_matched: usize,
        capture_rows_total: usize,
        grouped_rows_total: usize,
        grouped_rows_with_multiple_captures: usize,
    ) -> Vec<String> {
        let mut hints = Vec::new();
        if result_mode == StructuralResultMode::Matches
            && grouped_rows_with_multiple_captures > 0
            && capture_rows_total > grouped_rows_total
        {
            hints.push(
                "This query produced multiple captures per Tree-sitter match. Capture a higher-level node or set primary_capture to control the visible row anchor.".to_owned(),
            );
            hints.push(
                "Use inspect_syntax_tree on a representative file before retrying when the AST shape is unclear.".to_owned(),
            );
            hints.push(
                "Switch to result_mode=captures when you need raw capture rows for debugging."
                    .to_owned(),
            );
        }
        if primary_capture.is_none()
            && result_mode == StructuralResultMode::Matches
            && grouped_rows_with_multiple_captures > 0
        {
            hints.push(
                "Set primary_capture to the capture name you want returned when your query includes helper captures.".to_owned(),
            );
        }
        if !path_regex_supplied && files_matched > 1 && files_scanned > 1 {
            hints.push(
                "Add path_regex to keep structural scans bounded once you know the relevant file family.".to_owned(),
            );
        }
        hints.truncate(4);
        hints
    }

    pub(crate) async fn inspect_syntax_tree_impl(
        &self,
        params: InspectSyntaxTreeParams,
    ) -> Result<Json<InspectSyntaxTreeResponse>, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context("inspect_syntax_tree", params.repository_id.clone());
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut resolved_repository_id: Option<String> = None;
            let mut resolved_path: Option<String> = None;
            let mut language_name: Option<String> = None;

            let result = (|| -> Result<Json<InspectSyntaxTreeResponse>, ErrorData> {
                if params_for_blocking.path.trim().is_empty() {
                    return Err(Self::invalid_params("path must not be empty", None));
                }
                if params_for_blocking.line == Some(0) {
                    return Err(Self::invalid_params(
                        "line must be greater than zero when provided",
                        Some(json!({ "line": params_for_blocking.line })),
                    ));
                }
                if params_for_blocking.column == Some(0) {
                    return Err(Self::invalid_params(
                        "column must be greater than zero when provided",
                        Some(json!({ "column": params_for_blocking.column })),
                    ));
                }
                if params_for_blocking.line.is_none() != params_for_blocking.column.is_none() {
                    let recovery =
                        RecoveryFields::missing_line_column_pair("inspect_syntax_tree");
                    return Err(Self::invalid_params(
                        recovery
                            .message
                            .clone()
                            .unwrap_or_else(|| "line and column must be provided together".to_owned()),
                        Some(json!({
                            "error_code": recovery.error_code,
                            "line": params_for_blocking.line,
                            "column": params_for_blocking.column,
                            "correction_hint": recovery.correction_hint,
                            "related_tools": recovery.related_tools,
                            "suggested_next": recovery.suggested_next,
                        })),
                    ));
                }

                let max_ancestors = params_for_blocking
                    .max_ancestors
                    .unwrap_or(Self::INSPECT_SYNTAX_TREE_DEFAULT_MAX_ANCESTORS)
                    .clamp(1, Self::INSPECT_SYNTAX_TREE_MAX_ANCESTORS);
                let max_children = params_for_blocking
                    .max_children
                    .unwrap_or(Self::INSPECT_SYNTAX_TREE_DEFAULT_MAX_CHILDREN)
                    .clamp(1, Self::INSPECT_SYNTAX_TREE_MAX_CHILDREN);
                let read_params = ReadFileParams {
                    path: params_for_blocking.path.clone(),
                    repository_id: params_for_blocking.repository_id.clone(),
                    start_line: None,
                    end_line: None,
                    line_count: None,
                    max_bytes: None,
                    presentation_mode: None,
                    include_context_efficiency: None,
                };
                let (repository_id, absolute_path, display_path) =
                    server.resolve_file_path(&read_params)?;
                resolved_repository_id = Some(repository_id.clone());
                resolved_path = Some(display_path.clone());
                let language =
                    supported_language_for_path(&absolute_path, LanguageCapability::StructuralSearch)
                        .ok_or_else(|| {
                            Self::invalid_params(
                                LanguageCapability::StructuralSearch
                                    .unsupported_file_message("inspect_syntax_tree"),
                                Some(json!({
                                    "path": display_path.clone(),
                                    "supported_extensions": LanguageCapability::StructuralSearch.supported_extensions(),
                                })),
                            )
                        })?;
                language_name = Some(language.as_str().to_owned());
                let metadata = fs::metadata(&absolute_path).map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to stat source file {}: {err}",
                            absolute_path.display()
                        ),
                        None,
                    )
                })?;
                let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                if bytes > server.config.max_file_bytes {
                    return Err(Self::invalid_params(
                        format!("file exceeds max_bytes={}", server.config.max_file_bytes),
                        Some(json!({
                            "path": display_path.clone(),
                            "bytes": bytes,
                            "max_bytes": server.config.max_file_bytes,
                            "config_max_file_bytes": server.config.max_file_bytes,
                        })),
                    ));
                }
                let source = fs::read_to_string(&absolute_path).map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to read source file {}: {err}",
                            absolute_path.display()
                        ),
                        None,
                    )
                })?;
                let include_follow_up_structural =
                    params_for_blocking.include_follow_up_structural == Some(true);
                let (inspection, follow_up_structural) = if include_follow_up_structural {
                    inspect_syntax_tree_with_follow_up_in_source(
                        language,
                        &absolute_path,
                        &display_path,
                        &source,
                        params_for_blocking.line,
                        params_for_blocking.column,
                        max_ancestors,
                        max_children,
                        &repository_id,
                    )
                } else {
                    inspect_syntax_tree_in_source(
                        language,
                        &absolute_path,
                        &source,
                        params_for_blocking.line,
                        params_for_blocking.column,
                        max_ancestors,
                        max_children,
                    )
                    .map(|inspection| (inspection, Vec::new()))
                }
                .map_err(Self::map_frigg_error)?;

                let focus_normalized = inspection
                    .raw_focus
                    .as_ref()
                    .is_some_and(|raw_focus| raw_focus.span != inspection.focus.span);
                // Parse the same focused neighborhood without response caps so ancestor and
                // child omission remain independently visible on the wire.
                let exhaustive_inspection = inspect_syntax_tree_in_source(
                    language,
                    &absolute_path,
                    &source,
                    params_for_blocking.line,
                    params_for_blocking.column,
                    usize::MAX,
                    usize::MAX,
                )
                .map_err(Self::map_frigg_error)?;
                let ancestors_total = exhaustive_inspection.ancestors.len();
                let children_total = exhaustive_inspection.children.len();
                let ancestors_returned = inspection.ancestors.len();
                let children_returned = inspection.children.len();
                let ancestors_truncated = inspection.ancestors.len() < ancestors_total;
                let children_truncated = inspection.children.len() < children_total;
                let metadata = json!({
                    "source": "tree_sitter",
                    "language": inspection.language.as_str(),
                    "selection_mode": if params_for_blocking.line.is_some() {
                        "location"
                    } else {
                        "root"
                    },
                    "max_ancestors": max_ancestors,
                    "max_children": max_children,
                    "ancestors_total": ancestors_total,
                    "children_total": children_total,
                    "focus_normalized": focus_normalized,
                    "raw_focus_kind": inspection
                        .raw_focus
                        .as_ref()
                        .map(|raw_focus| raw_focus.kind.clone()),
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(InspectSyntaxTreeResponse {
                    repository_id,
                    path: display_path.clone(),
                    language: inspection.language.as_str().to_owned(),
                    focus: Self::syntax_tree_node_item(&display_path, inspection.focus),
                    ancestors: inspection
                        .ancestors
                        .into_iter()
                        .map(|node| Self::syntax_tree_node_item(&display_path, node))
                        .collect(),
                    children: inspection
                        .children
                        .into_iter()
                        .map(|node| Self::syntax_tree_node_item(&display_path, node))
                        .collect(),
                    ancestors_completeness: ResultCompleteness::try_new(
                        ResultUnit::SyntaxNode,
                        ancestors_returned,
                        Some(ancestors_total),
                        !ancestors_truncated,
                        ancestors_truncated,
                        ancestors_truncated.then_some(ResultTruncationReason::AncestorLimit).into_iter().collect(),
                        Vec::new(),
                        None,
                    ).map_err(|error| Self::internal(format!("invalid ancestor completeness state: {error}"), None))?,
                    children_completeness: ResultCompleteness::try_new(
                        ResultUnit::SyntaxNode,
                        children_returned,
                        Some(children_total),
                        !children_truncated,
                        children_truncated,
                        children_truncated.then_some(ResultTruncationReason::ChildLimit).into_iter().collect(),
                        Vec::new(),
                        None,
                    ).map_err(|error| Self::internal(format!("invalid child completeness state: {error}"), None))?,
                    follow_up_structural,
                    metadata,
                    note,
                }))
            })();

            (result, resolved_repository_id, resolved_path, language_name)
        })
        .await?;

        let (result, resolved_repository_id, resolved_path, language_name) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "inspect_syntax_tree",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "path": Self::bounded_text(&params.path),
                    "line": params.line,
                    "column": params.column,
                    "max_ancestors": params.max_ancestors,
                    "max_children": params.max_children,
                    "include_follow_up_structural": params.include_follow_up_structural,
                }),
                json!({
                    "resolved_repository_id": resolved_repository_id,
                    "resolved_path": resolved_path,
                    "language": language_name,
                }),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    fn structural_invalid_query_error(
        query: &str,
        language: SymbolLanguage,
        raw_message: &str,
    ) -> ErrorData {
        Self::invalid_params(
            raw_message.to_owned(),
            Some(json!({
                "query": Self::bounded_text(query),
                "language": language.as_str(),
                "error_class": "tree_sitter_query_invalid",
                "likely_cause": "tree_sitter_node_shape_mismatch",
                "fallback_tools": [
                    "inspect_syntax_tree",
                    "document_symbols",
                    "search_symbol",
                    "search_text"
                ],
                "guidance": "search_structural expects a valid tree-sitter query for the target language grammar. Use inspect_syntax_tree on a representative file to inspect real node kinds before retrying.",
            })),
        )
    }

    fn parse_symbol_language(value: Option<&str>) -> Result<Option<SymbolLanguage>, ErrorData> {
        let Some(value) = value else {
            return Ok(None);
        };
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(Self::invalid_params("language must not be empty", None));
        }

        let language = parse_supported_language(&normalized, LanguageCapability::StructuralSearch)
            .ok_or_else(|| {
                Self::invalid_params(
                    format!("unsupported language `{value}` for structural search"),
                    Some(json!({
                        "language": value,
                        "supported_languages": LanguageCapability::StructuralSearch.supported_language_names(),
                    })),
                )
            })?;
        Ok(Some(language))
    }

    pub(crate) async fn search_structural_impl(
        &self,
        params: SearchStructuralParams,
    ) -> Result<Json<SearchStructuralResponse>, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context("search_structural", params.repository_id.clone());
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut effective_limit: Option<usize> = None;
            let mut language_filter: Option<String> = None;
            let mut files_scanned = 0usize;
            let mut files_matched = 0usize;
            let mut diagnostics_count = 0usize;
            let mut capture_rows_total = 0usize;
            let mut grouped_rows_total = 0usize;
            let mut grouped_rows_with_multiple_captures = 0usize;
            let mut blade_relations_detected = 0usize;
            let mut blade_livewire_components = BTreeSet::new();
            let mut blade_wire_directives = BTreeSet::new();
            let mut blade_flux_components = BTreeSet::new();
            let mut snapshot_fingerprints = Vec::new();

            let result = (|| -> Result<Json<SearchStructuralResponse>, ErrorData> {
                let query = params_for_blocking.query.trim().to_owned();
                if params_for_blocking.limit == Some(0) {
                    return Err(Self::invalid_params("limit must be greater than zero when provided", None));
                }
                if query.is_empty() {
                    return Err(Self::invalid_params("query must not be empty", None));
                }
                if query.chars().count() > Self::SEARCH_STRUCTURAL_MAX_QUERY_CHARS {
                    return Err(Self::invalid_params(
                        "query exceeds structural search maximum length",
                        Some(json!({
                            "query_chars": query.chars().count(),
                            "max_query_chars": Self::SEARCH_STRUCTURAL_MAX_QUERY_CHARS,
                        })),
                    ));
                }

                let path_regex = match params_for_blocking.path_regex.as_ref() {
                    Some(raw) => Some(compile_safe_regex(raw).map_err(|err| {
                        Self::invalid_params(
                            format!("invalid path_regex: {err}"),
                            Some(json!({
                                "path_regex": raw,
                                "regex_error_code": err.code(),
                            })),
                        )
                    })?),
                    None => None,
                };

                let target_language =
                    Self::parse_symbol_language(params_for_blocking.language.as_deref())?;
                language_filter = target_language.map(|language| language.as_str().to_owned());
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);
                let result_mode = params_for_blocking
                    .result_mode
                    .unwrap_or(StructuralResultMode::Matches);
                let primary_capture = params_for_blocking
                    .primary_capture
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let include_follow_up_structural =
                    params_for_blocking.include_follow_up_structural == Some(true);

                let corpora = server.collect_repository_symbol_corpora(
                    params_for_blocking.repository_id.as_deref(),
                )?;
                scoped_repository_ids = corpora
                    .iter()
                    .map(|corpus| corpus.repository_id.clone())
                    .collect::<Vec<_>>();

                let mut matches = Vec::new();
                for corpus in corpora {
                    for source_path in &corpus.source_paths {
                        let Some(language) = supported_language_for_path(
                            source_path,
                            LanguageCapability::StructuralSearch,
                        ) else {
                            continue;
                        };
                        if let Some(target_language) = target_language
                            && language != target_language
                        {
                            continue;
                        }
                        let display_path = Self::relative_display_path(&corpus.root, source_path);
                        if let Some(path_regex) = &path_regex
                            && !path_regex.is_match(&display_path)
                        {
                            continue;
                        }
                        files_scanned = files_scanned.saturating_add(1);

                        if !Self::navigation_path_within_root(&corpus.root, source_path) {
                            diagnostics_count = diagnostics_count.saturating_add(1);
                            warn!(
                                repository_id = corpus.repository_id,
                                path = %source_path.display(),
                                root = %corpus.root.display(),
                                "skipping source file outside repository root for structural search"
                            );
                            continue;
                        }

                        let source = match fs::read_to_string(source_path) {
                            Ok(source) => source,
                            Err(err) => {
                                diagnostics_count = diagnostics_count.saturating_add(1);
                                warn!(
                                    repository_id = corpus.repository_id,
                                    path = %source_path.display(),
                                    error = %err,
                                    "skipping source file for structural search"
                                );
                                continue;
                            }
                        };
                        snapshot_fingerprints.push(format!(
                            "{}:{}:{}",
                            corpus.repository_id,
                            display_path,
                            blake3::hash(source.as_bytes()).to_hex()
                        ));

                        let structural_matches = match if include_follow_up_structural {
                            match result_mode {
                                StructuralResultMode::Matches =>
                                    search_structural_grouped_with_follow_up_in_source(
                                        language,
                                        source_path,
                                        &display_path,
                                        &source,
                                        &query,
                                        primary_capture,
                                        &corpus.repository_id,
                                    ),
                                StructuralResultMode::Captures =>
                                    search_structural_with_follow_up_in_source(
                                        language,
                                        source_path,
                                        &display_path,
                                        &source,
                                        &query,
                                        &corpus.repository_id,
                                    ),
                            }
                        } else {
                            match result_mode {
                                StructuralResultMode::Matches =>
                                    search_structural_grouped_in_source(
                                        language,
                                        source_path,
                                        &source,
                                        &query,
                                        primary_capture,
                                    ),
                                StructuralResultMode::Captures =>
                                    search_structural_in_source(
                                        language,
                                        source_path,
                                        &source,
                                        &query,
                                    ),
                            }
                        } {
                                Ok(matches) => matches,
                                Err(FriggError::InvalidInput(message))
                                    if message.starts_with("invalid structural query") =>
                                {
                                    return Err(Self::structural_invalid_query_error(
                                        &query,
                                        language,
                                        &message,
                                    ));
                                }
                                Err(err) => return Err(Self::map_frigg_error(err)),
                            };
                        if language == SymbolLanguage::Blade {
                            let blade_evidence =
                                extract_blade_source_evidence_from_source(&source, &[]);
                            blade_relations_detected = blade_relations_detected
                                .saturating_add(blade_evidence.relations.len());
                            blade_livewire_components
                                .extend(blade_evidence.livewire_components);
                            blade_wire_directives.extend(blade_evidence.wire_directives);
                            blade_flux_components.extend(blade_evidence.flux_components);
                        }
                        files_matched = files_matched
                            .saturating_add(usize::from(!structural_matches.is_empty()));
                        capture_rows_total = capture_rows_total.saturating_add(
                            structural_matches
                                .iter()
                                .map(|matched| matched.captures.len().max(1))
                                .sum::<usize>(),
                        );
                        if result_mode == StructuralResultMode::Matches {
                            grouped_rows_total =
                                grouped_rows_total.saturating_add(structural_matches.len());
                            grouped_rows_with_multiple_captures = grouped_rows_with_multiple_captures
                                .saturating_add(
                                    structural_matches
                                        .iter()
                                        .filter(|matched| matched.captures.len() > 1)
                                        .count(),
                                );
                        }

                        for structural_match in structural_matches {
                            matches.push(crate::mcp::types::StructuralMatch {
                                repository_id: corpus.repository_id.clone(),
                                path: display_path.clone(),
                                line: structural_match.span.start_line,
                                column: structural_match.span.start_column,
                                end_line: structural_match.span.end_line,
                                end_column: structural_match.span.end_column,
                                excerpt: structural_match.excerpt,
                                anchor_capture_name: structural_match.anchor_capture_name,
                                anchor_selection: match structural_match.anchor_selection {
                                    crate::indexer::StructuralQueryAnchorSelection::PrimaryCapture => StructuralAnchorSelection::PrimaryCapture,
                                    crate::indexer::StructuralQueryAnchorSelection::MatchCapture => StructuralAnchorSelection::MatchCapture,
                                    crate::indexer::StructuralQueryAnchorSelection::FirstUsefulNamedCapture => StructuralAnchorSelection::FirstUsefulNamedCapture,
                                    crate::indexer::StructuralQueryAnchorSelection::FirstCapture => StructuralAnchorSelection::FirstCapture,
                                    crate::indexer::StructuralQueryAnchorSelection::CaptureRow => StructuralAnchorSelection::CaptureRow,
                                },
                                captures: structural_match
                                    .captures
                                    .into_iter()
                                    .map(Self::structural_capture_item)
                                    .collect(),
                                follow_up_structural: structural_match.follow_up_structural,
                            });
                        }
                    }
                }

                matches.sort_by(|left, right| {
                    left.repository_id
                        .cmp(&right.repository_id)
                        .then(left.path.cmp(&right.path))
                        .then(left.line.cmp(&right.line))
                        .then(left.column.cmp(&right.column))
                        .then(left.end_line.cmp(&right.end_line))
                        .then(left.end_column.cmp(&right.end_column))
                        .then(left.excerpt.cmp(&right.excerpt))
                });
                snapshot_fingerprints.sort();
                let total_rows = matches.len();
                let request_digest = Self::search_structural_continuation_digest(
                    &query,
                    &params_for_blocking,
                    &scoped_repository_ids,
                    result_mode,
                );
                let resume_offset = match params_for_blocking.continuation.as_deref() {
                    Some(token) => server
                        .session_continuation_lookup(
                            token,
                            "search_structural",
                            &request_digest,
                            &scoped_repository_ids,
                            &snapshot_fingerprints,
                            ResultUnit::StructuralMatch,
                        )
                        .map(|binding| binding.next_position)
                        .map_err(|error| Self::invalid_params(error.message.clone(), Some(json!({ "continuation": error }))))?,
                    None => 0,
                };
                let matches = matches
                    .into_iter()
                    .skip(resume_offset)
                    .take(limit)
                    .collect::<Vec<_>>();
                let rows_omitted = resume_offset.saturating_add(matches.len()) < total_rows;
                let continuation = (rows_omitted && diagnostics_count == 0 && !snapshot_fingerprints.is_empty())
                    .then(|| server.store_session_continuation(ContinuationBinding {
                        tool: "search_structural",
                        request_digest,
                        repository_ids: scoped_repository_ids.clone(),
                        snapshot_fingerprints: snapshot_fingerprints.clone(),
                        unit: ResultUnit::StructuralMatch,
                        next_position: resume_offset.saturating_add(matches.len()),
                    }));
                let effective_grouped_rows_total = if result_mode == StructuralResultMode::Matches {
                    grouped_rows_total
                } else {
                    total_rows
                };
                let noisy_result_hints = Self::structural_noisy_result_hints(
                    result_mode,
                    primary_capture,
                    params_for_blocking.path_regex.is_some(),
                    files_scanned,
                    files_matched,
                    capture_rows_total,
                    effective_grouped_rows_total,
                    grouped_rows_with_multiple_captures,
                );

                let metadata = if target_language == Some(SymbolLanguage::Blade) {
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "result_mode": result_mode,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                        "capture_rows_total": capture_rows_total,
                        "grouped_rows_total": effective_grouped_rows_total,
                        "noisy_result_hints": noisy_result_hints,
                        "blade": {
                            "relations_detected": blade_relations_detected,
                            "livewire_components": blade_livewire_components.into_iter().collect::<Vec<_>>(),
                            "wire_directives": blade_wire_directives.into_iter().collect::<Vec<_>>(),
                            "flux_components": blade_flux_components.into_iter().collect::<Vec<_>>(),
                            "flux_registry_version": FLUX_REGISTRY_VERSION,
                        },
                    })
                } else {
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "result_mode": result_mode,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                        "capture_rows_total": capture_rows_total,
                        "grouped_rows_total": effective_grouped_rows_total,
                        "noisy_result_hints": noisy_result_hints,
                    })
                };
                let (metadata, note) = Self::metadata_note_pair(metadata);
                let returned = matches.len();
                Ok(Json(SearchStructuralResponse {
                    matches,
                    result_mode,
                    completeness: ResultCompleteness::try_new(
                        ResultUnit::StructuralMatch,
                        returned,
                        (diagnostics_count == 0).then_some(total_rows),
                        diagnostics_count == 0 && !rows_omitted,
                        rows_omitted,
                        rows_omitted.then_some(ResultTruncationReason::PageLimit).into_iter().collect(),
                        if diagnostics_count > 0 { vec![ResultIncompleteReason::DiagnosticCoverage] } else { Vec::new() },
                        continuation,
                    ).map_err(|error| Self::internal(format!("invalid structural completeness state: {error}"), None))?,
                    metadata,
                    note,
                }))
            })();

            (
                result,
                scoped_repository_ids,
                effective_limit,
                language_filter,
                files_scanned,
                files_matched,
                diagnostics_count,
            )
        })
        .await?;

        let (
            result,
            scoped_repository_ids,
            effective_limit,
            language_filter,
            files_scanned,
            files_matched,
            diagnostics_count,
        ) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "search_structural",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "language": params.language,
                    "path_regex": params.path_regex.map(|raw| Self::bounded_text(&raw)),
                    "limit": params.limit,
                    "effective_limit": effective_limit,
                    "include_follow_up_structural": params.include_follow_up_structural,
                }),
                json!({
                    "scoped_repository_ids": scoped_repository_ids,
                    "language_filter": language_filter,
                    "files_scanned": files_scanned,
                    "files_matched": files_matched,
                    "diagnostics_count": diagnostics_count,
                }),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    fn search_structural_continuation_digest(
        query: &str,
        params: &SearchStructuralParams,
        repository_ids: &[String],
        result_mode: StructuralResultMode,
    ) -> String {
        let normalized = json!({
            "query": query,
            "language": params.language,
            "repository_id": params.repository_id,
            "repository_ids": repository_ids,
            "path_regex": params.path_regex,
            "limit": params.limit,
            "result_mode": result_mode,
            "primary_capture": params.primary_capture,
            "include_follow_up_structural": params.include_follow_up_structural,
        });
        let mut hasher = DefaultHasher::new();
        normalized.to_string().hash(&mut hasher);
        format!("search-structural-v2:{:016x}", hasher.finish())
    }
}
