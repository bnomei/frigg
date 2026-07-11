//! `document_symbols` outline extraction for a single supported source file.
//!
//! Builds a single-file outline from cached symbol corpora for supported extensions; rejects
//! unsupported paths before parsing.

use super::*;
use crate::mcp::server_cache::ContinuationBinding;
use crate::mcp::types::{ResultCompleteness, ResultUnit};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

impl FriggMcpServer {
    pub(crate) async fn document_symbols_impl(
        &self,
        params: DocumentSymbolsParams,
    ) -> Result<Json<DocumentSymbolsResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("document_symbols", params.repository_id.clone());
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut resolved_repository_id: Option<String> = None;
            let mut resolved_path: Option<String> = None;
            let mut symbol_count = 0usize;

            let result = (|| -> Result<Json<DocumentSymbolsResponse>, ErrorData> {
                if let Err(error) = crate::mcp::types::ContinuationValidationError::reject_mixed_cursor_forms(
                    params_for_blocking.resume_from.is_some(),
                    params_for_blocking.continuation.is_some(),
                ) {
                    return Err(Self::invalid_params(error.message.clone(), Some(json!({ "continuation": error }))));
                }
                let read_params = ReadFileParams {
                    path: params_for_blocking.path.clone(),
                    repository_id: params_for_blocking.repository_id.clone(),
                    max_bytes: None,
                    start_line: None,
                    end_line: None,
                    line_count: None,
                    presentation_mode: None,
                    include_context_efficiency: None,
                };
                let (repository_id, absolute_path, display_path) =
                    server.resolve_file_path(&read_params)?;
                resolved_repository_id = Some(repository_id.clone());
                resolved_path = Some(display_path.clone());

                let language =
                    supported_language_for_path(&absolute_path, LanguageCapability::DocumentSymbols)
                        .ok_or_else(|| {
                            Self::invalid_params(
                                LanguageCapability::DocumentSymbols
                                    .unsupported_file_message("document_symbols"),
                                Some(json!({
                                    "path": display_path.clone(),
                                    "supported_extensions": LanguageCapability::DocumentSymbols.supported_extensions(),
                                })),
                            )
                        })?;
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
                            "suggested_max_bytes": bytes.min(server.config.max_file_bytes),
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
                let top_level_only = params_for_blocking.top_level_only.unwrap_or(true);
                let request_digest = Self::document_symbols_continuation_digest(
                    &params_for_blocking,
                    &repository_id,
                    top_level_only,
                );
                let snapshot_fingerprints = vec![format!(
                    "{}:{}:{}",
                    repository_id,
                    display_path,
                    blake3::hash(source.as_bytes()).to_hex(),
                )];
                let resume_offset = match params_for_blocking.continuation.as_deref() {
                    Some(token) => server
                        .session_continuation_lookup(
                            token,
                            "document_symbols",
                            &request_digest,
                            std::slice::from_ref(&repository_id),
                            &snapshot_fingerprints,
                            ResultUnit::DocumentSymbol,
                        )
                        .map(|binding| binding.next_position)
                        .map_err(|error| Self::invalid_params(error.message.clone(), Some(json!({ "continuation": error }))))?,
                    None => params_for_blocking.resume_from.unwrap_or(0),
                };
                let symbols = extract_symbols_from_source(language, &absolute_path, &source)
                    .map_err(Self::map_frigg_error)?;

                let outline = Self::build_document_symbol_tree(
                    &symbols,
                    &repository_id,
                    &display_path,
                    if params_for_blocking.include_follow_up_structural == Some(true) {
                        Some((language, &absolute_path, source.as_str()))
                    } else {
                        None
                    },
                );
                symbol_count = outline.len();

                let metadata = if language == SymbolLanguage::Blade {
                    let blade_evidence = extract_blade_source_evidence_from_source(&source, &symbols);
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                        "blade": {
                            "relations_detected": blade_evidence.relations.len(),
                            "livewire_components": blade_evidence.livewire_components,
                            "wire_directives": blade_evidence.wire_directives,
                            "flux_components": blade_evidence.flux_components,
                            "flux_registry_version": FLUX_REGISTRY_VERSION,
                            "flux_hints": blade_evidence.flux_hints,
                        },
                    })
                } else if language == SymbolLanguage::Php {
                    let php_metadata = extract_php_source_evidence_from_source(
                        &absolute_path,
                        &source,
                        &symbols,
                    )
                    .ok()
                    .map(|evidence| {
                        json!({
                            "canonical_name_count": evidence.canonical_names_by_stable_id.len(),
                            "type_evidence_count": evidence.type_evidence.len(),
                            "target_evidence_count": evidence.target_evidence.len(),
                            "literal_evidence_count": evidence.literal_evidence.len(),
                        })
                    });
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                        "php": php_metadata,
                    })
                } else {
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                    })
                };
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(server.present_document_symbols_response(
                    DocumentSymbolsResponse {
                        symbols: outline,
                        total_symbols: 0,
                        returned: 0,
                        truncated: false,
                        resume_from: None,
                        completeness: ResultCompleteness::complete(ResultUnit::DocumentSymbol, 0, 0)
                            .expect("empty document symbol response is complete"),
                        top_level_only: params_for_blocking.top_level_only.unwrap_or(true),
                        result_handle: None,
                        metadata,
                        note,
                    },
                    &params_for_blocking,
                    resume_offset,
                    Some(ContinuationBinding {
                        tool: "document_symbols",
                        request_digest,
                        repository_ids: vec![repository_id],
                        snapshot_fingerprints,
                        unit: ResultUnit::DocumentSymbol,
                        next_position: 0,
                    }),
                )))
            })();

            (result, resolved_repository_id, resolved_path, symbol_count)
        })
        .await?;

        let (result, resolved_repository_id, resolved_path, symbol_count) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "document_symbols",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "path": Self::bounded_text(&params.path),
                }),
                json!({
                    "resolved_repository_id": resolved_repository_id,
                    "resolved_path": resolved_path,
                    "symbol_count": symbol_count,
                }),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    fn document_symbols_continuation_digest(
        params: &DocumentSymbolsParams,
        repository_id: &str,
        top_level_only: bool,
    ) -> String {
        let normalized = json!({
            "path": params.path,
            "repository_id": repository_id,
            "include_follow_up_structural": params.include_follow_up_structural,
            "top_level_only": top_level_only,
            "limit": params.limit,
        });
        let mut hasher = DefaultHasher::new();
        normalized.to_string().hash(&mut hasher);
        format!("document-symbols-v2:{:016x}", hasher.finish())
    }

    fn source_span_contains_symbol(parent: &SourceSpan, child: &SourceSpan) -> bool {
        parent.start_byte <= child.start_byte
            && child.end_byte <= parent.end_byte
            && (parent.start_byte < child.start_byte || child.end_byte < parent.end_byte)
    }

    fn build_document_symbol_tree(
        symbols: &[SymbolDefinition],
        repository_id: &str,
        display_path: &str,
        follow_up_source: Option<(SymbolLanguage, &Path, &str)>,
    ) -> Vec<crate::mcp::types::DocumentSymbolItem> {
        #[derive(Clone)]
        struct PendingDocumentSymbolNode {
            item: crate::mcp::types::DocumentSymbolItem,
            span: SourceSpan,
            children: Vec<usize>,
        }

        fn materialize(
            nodes: &[PendingDocumentSymbolNode],
            index: usize,
        ) -> crate::mcp::types::DocumentSymbolItem {
            let mut item = nodes[index].item.clone();
            item.children = nodes[index]
                .children
                .iter()
                .map(|child_index| materialize(nodes, *child_index))
                .collect();
            item
        }

        let mut nodes: Vec<PendingDocumentSymbolNode> = Vec::with_capacity(symbols.len());
        let mut root_indices = Vec::new();
        let mut stack: Vec<usize> = Vec::new();

        for symbol in symbols {
            while let Some(parent_index) = stack.last().copied() {
                if Self::source_span_contains_symbol(&nodes[parent_index].span, &symbol.span) {
                    break;
                }
                stack.pop();
            }

            let container = stack
                .last()
                .map(|parent_index| nodes[*parent_index].item.symbol.clone());
            let node_index = nodes.len();
            nodes.push(PendingDocumentSymbolNode {
                item: crate::mcp::types::DocumentSymbolItem {
                    match_id: None,
                    stable_symbol_id: Some(symbol.stable_id.clone()),
                    symbol: symbol.name.clone(),
                    kind: symbol.kind.as_str().to_owned(),
                    repository_id: repository_id.to_owned(),
                    path: display_path.to_owned(),
                    line: symbol.span.start_line,
                    column: symbol.span.start_column,
                    end_line: Some(symbol.span.end_line),
                    end_column: Some(symbol.span.end_column),
                    container,
                    signature: None,
                    follow_up_structural: follow_up_source
                        .map(|(language, absolute_path, source)| {
                            generated_follow_up_structural_at_location_in_source(
                                language,
                                absolute_path,
                                display_path,
                                source,
                                symbol.span.start_line,
                                symbol.span.start_column,
                                repository_id,
                            )
                            .unwrap_or_default()
                        })
                        .unwrap_or_default(),
                    children: Vec::new(),
                },
                span: symbol.span.clone(),
                children: Vec::new(),
            });

            if let Some(parent_index) = stack.last().copied() {
                nodes[parent_index].children.push(node_index);
            } else {
                root_indices.push(node_index);
            }
            stack.push(node_index);
        }

        root_indices
            .into_iter()
            .map(|index| materialize(&nodes, index))
            .collect()
    }
}
