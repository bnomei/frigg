//! Tree-sitter-backed symbol extraction for single sources and parallel path batches.
//!
//! Parses supported language grammars, collects definition nodes into stable symbol records, and
//! batches path reads so indexer symbol passes stay bounded on large repositories.

use super::*;
use rayon::prelude::*;

/// Stable-symbol identity algorithm version included in public corpus snapshot tokens.
pub const STABLE_SYMBOL_ID_ALGORITHM_VERSION: u32 = 2;

/// Extracts symbol definitions from parsed source for a known language adapter.
pub fn extract_symbols_from_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
) -> FriggResult<Vec<SymbolDefinition>> {
    extract_symbols_from_source_with_identity_path(language, path, path, source)
}

/// Extracts symbols while keeping the operational source path separate from public identity.
///
/// `path` remains available for file reads and source navigation. `identity_path` is the
/// repository-relative path hashed into every stable symbol ID.
pub fn extract_symbols_from_source_with_identity_path(
    language: SymbolLanguage,
    path: &Path,
    identity_path: &Path,
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
    assign_symbol_identity_path(&mut symbols, identity_path);
    symbols.sort_by(symbol_definition_order);
    Ok(symbols)
}

/// Reassigns extracted symbols to one repository-relative identity path.
pub(crate) fn assign_symbol_identity_path(symbols: &mut [SymbolDefinition], identity_path: &Path) {
    for symbol in symbols {
        symbol.stable_id = stable_symbol_id(
            symbol.language,
            symbol.kind,
            identity_path,
            &symbol.name,
            &symbol.span,
        );
    }
}

/// Reads a file from disk and extracts symbol definitions when the extension is supported.
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

/// Extracts symbols for many paths in parallel, collecting per-file diagnostics.
pub fn extract_symbols_for_paths(paths: &[PathBuf]) -> SymbolExtractionOutput {
    let mut ordered_paths = paths.to_vec();
    ordered_paths.sort();

    extract_symbols_for_ordered_paths(ordered_paths)
}

/// Extracts symbols for repository paths while hashing only repository-relative identity paths.
pub fn extract_symbols_for_paths_with_root(
    root: &Path,
    paths: &[PathBuf],
) -> SymbolExtractionOutput {
    let mut output = extract_symbols_for_paths(paths);
    assign_repository_relative_symbol_identities(root, &mut output);
    output
}

fn extract_symbols_for_ordered_paths(ordered_paths: Vec<PathBuf>) -> SymbolExtractionOutput {
    let mut output = ordered_paths
        .into_par_iter()
        .filter_map(|path| {
            let language = SymbolLanguage::from_path(&path)?;
            let mut output = SymbolExtractionOutput::default();
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
            Some(output)
        })
        .reduce(SymbolExtractionOutput::default, |mut left, mut right| {
            left.symbols.append(&mut right.symbols);
            left.diagnostics.append(&mut right.diagnostics);
            left
        });

    output.symbols.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.span.start_byte.cmp(&right.span.start_byte))
            .then(left.span.end_byte.cmp(&right.span.end_byte))
            .then(left.kind.cmp(&right.kind))
            .then(left.name.cmp(&right.name))
            .then(left.stable_id.cmp(&right.stable_id))
    });
    output.diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.language.cmp(&right.language))
            .then(left.message.cmp(&right.message))
    });
    output
}

/// Rebinds a batch extraction to repository-relative identity while preserving operational paths.
pub(crate) fn assign_repository_relative_symbol_identities(
    root: &Path,
    output: &mut SymbolExtractionOutput,
) {
    let mut relative_symbols = Vec::with_capacity(output.symbols.len());
    for mut symbol in output.symbols.drain(..) {
        match symbol.path.strip_prefix(root) {
            Ok(relative_path) if !relative_path.as_os_str().is_empty() => {
                let relative_path = relative_path.to_path_buf();
                assign_symbol_identity_path(std::slice::from_mut(&mut symbol), &relative_path);
                relative_symbols.push(symbol);
            }
            Ok(_) | Err(_) => output.diagnostics.push(SymbolExtractionDiagnostic {
                path: symbol.path.clone(),
                language: Some(symbol.language),
                message: format!(
                    "source path {} is not a repository-relative child of {}",
                    symbol.path.display(),
                    root.display()
                ),
            }),
        }
    }
    output.symbols = relative_symbols;
    output.symbols.sort_by(symbol_definition_order);
    output.diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.language.cmp(&right.language))
            .then(left.message.cmp(&right.message))
    });
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
    hasher.update(&STABLE_SYMBOL_ID_ALGORITHM_VERSION.to_le_bytes());
    hasher.update(&[0]);
    hasher.update(language.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(kind.as_str().as_bytes());
    hasher.update(&[0]);
    let normalized_identity_path = path.to_string_lossy().replace('\\', "/");
    hasher.update(normalized_identity_path.as_bytes());
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
