use super::*;

pub fn extract_symbols_from_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
) -> FriggResult<Vec<SymbolDefinition>> {
    let mut parser = parser_for_path(language, path)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for symbol extraction: {}",
            path.display()
        ))
    })?;
    let mut symbols = Vec::new();
    collect_symbols_from_tree(language, path, source, &tree, &mut symbols);
    symbols.sort_by(symbol_definition_order);
    Ok(symbols)
}

pub fn extract_symbols_from_file(path: &Path) -> FriggResult<Vec<SymbolDefinition>> {
    let language = SymbolLanguage::from_path(path).ok_or_else(|| {
        FriggError::InvalidInput(format!(
            "unsupported source file extension for symbol extraction: {}",
            path.display()
        ))
    })?;
    let source = fs::read_to_string(path).map_err(FriggError::Io)?;
    extract_symbols_from_source(language, path, &source)
}

pub fn extract_symbols_for_paths(paths: &[PathBuf]) -> SymbolExtractionOutput {
    let mut ordered_paths = paths.to_vec();
    ordered_paths.sort();

    let mut output = SymbolExtractionOutput::default();
    for path in ordered_paths {
        let Some(language) = SymbolLanguage::from_path(&path) else {
            continue;
        };

        match fs::read_to_string(&path) {
            Ok(source) => match extract_symbols_from_source(language, &path, &source) {
                Ok(mut symbols) => output.symbols.append(&mut symbols),
                Err(err) => output.diagnostics.push(SymbolExtractionDiagnostic {
                    path: path.clone(),
                    language: Some(language),
                    message: err.to_string(),
                }),
            },
            Err(err) => output.diagnostics.push(SymbolExtractionDiagnostic {
                path: path.clone(),
                language: Some(language),
                message: err.to_string(),
            }),
        }
    }

    output.symbols.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.span.start_byte.cmp(&right.span.start_byte))
            .then(left.span.end_byte.cmp(&right.span.end_byte))
            .then(left.kind.cmp(&right.kind))
            .then(left.name.cmp(&right.name))
            .then(left.stable_id.cmp(&right.stable_id))
    });
    output
}

fn collect_symbols_from_tree(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    tree: &Tree,
    symbols: &mut Vec<SymbolDefinition>,
) {
    if language == SymbolLanguage::Blade {
        collect_blade_symbols_from_source(path, source, symbols);
        return;
    }
    collect_symbols_from_node(language, path, source, tree.root_node(), symbols);
}

fn collect_symbols_from_node(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    node: Node<'_>,
    symbols: &mut Vec<SymbolDefinition>,
) {
    if let Some((kind, name)) = symbol_from_node(language, source, node) {
        let span = source_span(node);
        symbols.push(SymbolDefinition {
            stable_id: stable_symbol_id(language, kind, path, &name, &span),
            language,
            kind,
            name,
            path: path.to_path_buf(),
            line: span.start_line,
            span,
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_symbols_from_node(language, path, source, child, symbols);
    }
}

pub(crate) fn push_symbol_definition(
    symbols: &mut Vec<SymbolDefinition>,
    language: SymbolLanguage,
    kind: SymbolKind,
    path: &Path,
    name: &str,
    span: SourceSpan,
) {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return;
    }
    let stable_id = stable_symbol_id(language, kind, path, trimmed_name, &span);
    if symbols.iter().any(|symbol| symbol.stable_id == stable_id) {
        return;
    }
    symbols.push(SymbolDefinition {
        stable_id,
        language,
        kind,
        name: trimmed_name.to_owned(),
        path: path.to_path_buf(),
        line: span.start_line,
        span,
    });
}

fn stable_symbol_id(
    language: SymbolLanguage,
    kind: SymbolKind,
    path: &Path,
    name: &str,
    span: &SourceSpan,
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(language.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(kind.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(&[0]);
    hasher.update(name.as_bytes());
    hasher.update(&[0]);
    hasher.update(span.start_byte.to_string().as_bytes());
    hasher.update(&[0]);
    hasher.update(span.end_byte.to_string().as_bytes());
    format!("sym-{}", hasher.finalize().to_hex())
}

fn symbol_definition_order(
    left: &SymbolDefinition,
    right: &SymbolDefinition,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.span.start_byte.cmp(&right.span.start_byte))
        .then(left.span.end_byte.cmp(&right.span.end_byte))
        .then(left.kind.cmp(&right.kind))
        .then(left.name.cmp(&right.name))
        .then(left.stable_id.cmp(&right.stable_id))
}
