use super::*;
use protobuf::Message;

pub(super) fn parse_scip_json(
    artifact_label: &str,
    payload: &[u8],
) -> ScipIngestResult<ScipIndexJson> {
    serde_json::from_slice::<ScipIndexJson>(payload).map_err(|error| {
        ScipIngestError::InvalidInput {
            diagnostic: ScipInvalidInputDiagnostic {
                artifact_label: artifact_label.to_owned(),
                code: ScipInvalidInputCode::JsonDecode,
                message: error.to_string(),
                line: Some(error.line()),
                column: Some(error.column()),
            },
        }
    })
}

pub(super) fn parse_scip_protobuf(
    artifact_label: &str,
    payload: &[u8],
) -> ScipIngestResult<ScipIndexJson> {
    let index = ScipIndexProto::parse_from_bytes(payload).map_err(|error: protobuf::Error| {
        invalid_input(
            artifact_label,
            ScipInvalidInputCode::ProtobufDecode,
            error.to_string(),
        )
    })?;
    scip_index_from_protobuf(artifact_label, index)
}

fn scip_index_from_protobuf(
    artifact_label: &str,
    index: ScipIndexProto,
) -> ScipIngestResult<ScipIndexJson> {
    let mut documents = Vec::with_capacity(index.documents.len());
    for document in index.documents {
        documents.push(scip_document_from_protobuf(artifact_label, document)?);
    }
    Ok(ScipIndexJson { documents })
}

fn scip_document_from_protobuf(
    artifact_label: &str,
    document: ScipDocumentProto,
) -> ScipIngestResult<ScipDocumentJson> {
    let relative_path = document.relative_path;
    let mut occurrences = Vec::with_capacity(document.occurrences.len());
    for occurrence in document.occurrences {
        occurrences.push(scip_occurrence_from_protobuf(
            artifact_label,
            &relative_path,
            occurrence,
        )?);
    }

    let symbols = document
        .symbols
        .into_iter()
        .map(scip_symbol_information_from_protobuf)
        .collect();

    Ok(ScipDocumentJson {
        relative_path,
        occurrences,
        symbols,
    })
}

fn scip_occurrence_from_protobuf(
    artifact_label: &str,
    path: &str,
    occurrence: ScipOccurrenceProto,
) -> ScipIngestResult<ScipOccurrenceJson> {
    let symbol = occurrence.symbol.trim().to_owned();
    let range = occurrence
        .range
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            u32::try_from(value).map_err(|_| {
                invalid_input(
                    artifact_label,
                    ScipInvalidInputCode::InvalidRange,
                    format!(
                        "occurrence range component {index} for symbol '{}' in '{}' must be non-negative",
                        symbol, path
                    ),
                )
            })
        })
        .collect::<ScipIngestResult<Vec<_>>>()?;

    let symbol_roles = u32::try_from(occurrence.symbol_roles).map_err(|_| {
        invalid_input(
            artifact_label,
            ScipInvalidInputCode::InvalidRange,
            format!(
                "occurrence symbol_roles for symbol '{}' in '{}' must be non-negative",
                symbol, path
            ),
        )
    })?;

    Ok(ScipOccurrenceJson {
        symbol,
        range,
        symbol_roles,
    })
}

fn scip_symbol_information_from_protobuf(
    symbol: ScipSymbolInformationProto,
) -> ScipSymbolInformationJson {
    ScipSymbolInformationJson {
        symbol: symbol.symbol,
        display_name: symbol.display_name,
        kind: Some(ScipSymbolKindJson::Numeric(i64::from(symbol.kind.value()))),
        relationships: symbol
            .relationships
            .into_iter()
            .map(scip_relationship_from_protobuf)
            .collect(),
    }
}

fn scip_relationship_from_protobuf(relationship: ScipRelationshipProto) -> ScipRelationshipJson {
    ScipRelationshipJson {
        symbol: relationship.symbol,
        is_reference: relationship.is_reference,
        is_implementation: relationship.is_implementation,
        is_type_definition: relationship.is_type_definition,
        is_definition: relationship.is_definition,
    }
}

pub(super) fn map_scip_documents(
    repository_id: &str,
    artifact_label: &str,
    index_json: ScipIndexJson,
) -> ScipIngestResult<Vec<ParsedScipDocument>> {
    let mut documents = index_json
        .documents
        .into_iter()
        .map(|document| map_scip_document(repository_id, artifact_label, document))
        .collect::<ScipIngestResult<Vec<_>>>()?;
    documents.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(documents)
}

fn map_scip_document(
    repository_id: &str,
    artifact_label: &str,
    document: ScipDocumentJson,
) -> ScipIngestResult<ParsedScipDocument> {
    let path = document.relative_path.trim().to_owned();
    if path.is_empty() {
        return Err(invalid_input(
            artifact_label,
            ScipInvalidInputCode::MissingDocumentPath,
            "document.relative_path must not be empty",
        ));
    }

    let mut occurrences = document
        .occurrences
        .into_iter()
        .map(|occurrence| map_scip_occurrence(repository_id, artifact_label, &path, occurrence))
        .collect::<ScipIngestResult<Vec<_>>>()?;
    occurrences.sort_by(precise_occurrence_order);
    occurrences.dedup();

    let mut symbols = Vec::new();
    let mut relationships = Vec::new();
    for symbol_info in document.symbols {
        let symbol = symbol_info.symbol.trim().to_owned();
        if symbol.is_empty() {
            return Err(invalid_input(
                artifact_label,
                ScipInvalidInputCode::MissingSymbol,
                format!("document '{}' contains symbol info with empty symbol", path),
            ));
        }
        let display_name = symbol_info.display_name.trim().to_owned();
        let kind = normalize_scip_symbol_kind(symbol_info.kind);
        symbols.push(PreciseSymbolRecord {
            repository_id: repository_id.to_owned(),
            symbol: symbol.clone(),
            display_name,
            kind,
        });

        let mapped_relationships = map_scip_relationships(
            repository_id,
            artifact_label,
            &path,
            &symbol,
            symbol_info.relationships,
        )?;
        relationships.extend(mapped_relationships);
    }

    symbols.sort_by(precise_symbol_order);
    symbols.dedup();
    relationships.sort_by(precise_relationship_order);
    relationships.dedup();

    Ok(ParsedScipDocument {
        repository_id: repository_id.to_owned(),
        path,
        symbols,
        occurrences,
        relationships,
    })
}

fn normalize_scip_symbol_kind(kind: Option<ScipSymbolKindJson>) -> String {
    match kind {
        Some(ScipSymbolKindJson::Numeric(value)) => i32::try_from(value)
            .ok()
            .and_then(ScipSymbolKindProto::from_i32)
            .map(|kind| normalize_scip_kind_name(&format!("{kind:?}")))
            .unwrap_or_else(|| format!("kind_{value}")),
        Some(ScipSymbolKindJson::Text(value)) => {
            let normalized = value.trim();
            if normalized.is_empty() {
                "unknown".to_owned()
            } else {
                normalize_scip_kind_name(normalized)
            }
        }
        None => "unknown".to_owned(),
    }
}

fn normalize_scip_kind_name(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut previous_was_separator = false;
    let mut previous_was_lower_or_digit = false;

    for character in raw.chars() {
        if matches!(character, '_' | '-' | ' ' | '\t') {
            if !output.ends_with('_') && !output.is_empty() {
                output.push('_');
            }
            previous_was_separator = true;
            previous_was_lower_or_digit = false;
            continue;
        }

        if character.is_ascii_uppercase()
            && !output.is_empty()
            && !previous_was_separator
            && previous_was_lower_or_digit
        {
            output.push('_');
        }

        output.push(character.to_ascii_lowercase());
        previous_was_separator = false;
        previous_was_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
    }

    output
}

fn map_scip_occurrence(
    repository_id: &str,
    artifact_label: &str,
    path: &str,
    occurrence: ScipOccurrenceJson,
) -> ScipIngestResult<PreciseOccurrenceRecord> {
    let symbol = occurrence.symbol.trim().to_owned();
    if symbol.is_empty() {
        return Err(invalid_input(
            artifact_label,
            ScipInvalidInputCode::MissingSymbol,
            format!("occurrence in '{}' has empty symbol", path),
        ));
    }
    let range = map_scip_range(artifact_label, path, &symbol, &occurrence.range)?;
    Ok(PreciseOccurrenceRecord {
        repository_id: repository_id.to_owned(),
        path: path.to_owned(),
        symbol,
        range,
        symbol_roles: occurrence.symbol_roles,
    })
}

fn map_scip_range(
    artifact_label: &str,
    path: &str,
    symbol: &str,
    range: &[u32],
) -> ScipIngestResult<PreciseRange> {
    let mapped = match range {
        [start_line, start_column, end_column] => PreciseRange {
            start_line: (*start_line as usize).saturating_add(1),
            start_column: (*start_column as usize).saturating_add(1),
            end_line: (*start_line as usize).saturating_add(1),
            end_column: (*end_column as usize).saturating_add(1),
        },
        [start_line, start_column, end_line, end_column] => PreciseRange {
            start_line: (*start_line as usize).saturating_add(1),
            start_column: (*start_column as usize).saturating_add(1),
            end_line: (*end_line as usize).saturating_add(1),
            end_column: (*end_column as usize).saturating_add(1),
        },
        _ => {
            return Err(invalid_input(
                artifact_label,
                ScipInvalidInputCode::InvalidRange,
                format!(
                    "occurrence range for symbol '{}' in '{}' must have 3 or 4 numbers",
                    symbol, path
                ),
            ));
        }
    };

    let valid_order = mapped.end_line > mapped.start_line
        || (mapped.end_line == mapped.start_line && mapped.end_column >= mapped.start_column);
    if !valid_order {
        return Err(invalid_input(
            artifact_label,
            ScipInvalidInputCode::InvalidRange,
            format!(
                "occurrence range for symbol '{}' in '{}' has end before start",
                symbol, path
            ),
        ));
    }

    Ok(mapped)
}

fn map_scip_relationships(
    repository_id: &str,
    artifact_label: &str,
    path: &str,
    source_symbol: &str,
    relationships: Vec<ScipRelationshipJson>,
) -> ScipIngestResult<Vec<PreciseRelationshipRecord>> {
    let mut mapped = Vec::new();
    for relationship in relationships {
        let target_symbol = relationship.symbol.trim().to_owned();
        if target_symbol.is_empty() {
            return Err(invalid_input(
                artifact_label,
                ScipInvalidInputCode::MissingSymbol,
                format!(
                    "relationship for symbol '{}' in '{}' has empty target symbol",
                    source_symbol, path
                ),
            ));
        }

        let relationship_kinds = relationship_kinds(&relationship);
        if relationship_kinds.is_empty() {
            return Err(invalid_input(
                artifact_label,
                ScipInvalidInputCode::InvalidRelationship,
                format!(
                    "relationship for symbol '{}' in '{}' must set at least one relationship flag",
                    source_symbol, path
                ),
            ));
        }

        for kind in relationship_kinds {
            mapped.push(PreciseRelationshipRecord {
                repository_id: repository_id.to_owned(),
                from_symbol: source_symbol.to_owned(),
                to_symbol: target_symbol.clone(),
                kind,
            });
        }
    }

    mapped.sort_by(precise_relationship_order);
    mapped.dedup();
    Ok(mapped)
}

fn relationship_kinds(relationship: &ScipRelationshipJson) -> Vec<PreciseRelationshipKind> {
    let mut kinds = Vec::new();
    if relationship.is_definition {
        kinds.push(PreciseRelationshipKind::Definition);
    }
    if relationship.is_reference {
        kinds.push(PreciseRelationshipKind::Reference);
    }
    if relationship.is_implementation {
        kinds.push(PreciseRelationshipKind::Implementation);
    }
    if relationship.is_type_definition {
        kinds.push(PreciseRelationshipKind::TypeDefinition);
    }
    kinds
}

pub(super) fn apply_scip_documents(
    graph: &mut SymbolGraph,
    artifact_label: &str,
    documents: &[ParsedScipDocument],
    mode: ScipFileIngestMode,
) -> ScipIngestSummary {
    let mut symbols_upserted = 0usize;
    let mut occurrences_upserted = 0usize;
    let mut relationships_upserted = 0usize;

    for document in documents {
        match mode {
            ScipFileIngestMode::Replace => {
                replace_precise_occurrences_for_file(
                    graph,
                    &document.repository_id,
                    &document.path,
                    &document.occurrences,
                );
                replace_precise_symbols_for_file(
                    graph,
                    &document.repository_id,
                    &document.path,
                    &document.symbols,
                );
                replace_precise_relationships_for_file(
                    graph,
                    &document.repository_id,
                    &document.path,
                    &document.relationships,
                );
            }
            ScipFileIngestMode::Overlay => {
                overlay_precise_occurrences_for_file(
                    graph,
                    &document.repository_id,
                    &document.path,
                    &document.occurrences,
                );
                overlay_precise_symbols_for_file(
                    graph,
                    &document.repository_id,
                    &document.path,
                    &document.symbols,
                );
                overlay_precise_relationships_for_file(
                    graph,
                    &document.repository_id,
                    &document.path,
                    &document.relationships,
                );
            }
        }

        occurrences_upserted += document.occurrences.len();
        symbols_upserted += document.symbols.len();
        relationships_upserted += document.relationships.len();
    }

    ScipIngestSummary {
        artifact_label: artifact_label.to_owned(),
        documents_ingested: documents.len(),
        symbols_upserted,
        occurrences_upserted,
        relationships_upserted,
    }
}
