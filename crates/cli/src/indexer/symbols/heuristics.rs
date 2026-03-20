use super::*;

pub struct HeuristicReferenceResolver<'a> {
    repository_id: &'a str,
    target_symbol: &'a SymbolDefinition,
    relation_hint_by_source: HashMap<String, (HeuristicReferenceConfidence, String)>,
    symbols_by_path: HashMap<PathBuf, Vec<&'a SymbolDefinition>>,
    by_location: BTreeMap<(PathBuf, usize, usize), HeuristicReference>,
}

impl<'a> HeuristicReferenceResolver<'a> {
    pub fn new(
        repository_id: &'a str,
        symbol_id: &str,
        symbols: &'a [SymbolDefinition],
        graph: &SymbolGraph,
    ) -> Option<Self> {
        let target_symbol = symbols
            .iter()
            .find(|symbol| symbol.stable_id == symbol_id)?;
        let relation_hints = graph.heuristic_relation_hints_for_target(symbol_id);
        let relation_hint_by_source = relation_hints
            .iter()
            .map(|hint| {
                (
                    hint.source_symbol.symbol_id.clone(),
                    (
                        HeuristicReferenceConfidence::from(hint.confidence),
                        hint.relation.as_str().to_owned(),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut by_location = BTreeMap::new();
        for hint in relation_hints {
            let path = PathBuf::from(&hint.source_symbol.path);
            if path == target_symbol.path && hint.source_symbol.line == target_symbol.line {
                continue;
            }
            upsert_heuristic_reference(
                &mut by_location,
                HeuristicReference {
                    repository_id: repository_id.to_owned(),
                    symbol_id: target_symbol.stable_id.clone(),
                    symbol_name: target_symbol.name.clone(),
                    path,
                    line: hint.source_symbol.line,
                    column: 1,
                    confidence: HeuristicReferenceConfidence::from(hint.confidence),
                    heuristic: true,
                    evidence: HeuristicReferenceEvidence::GraphRelation {
                        source_symbol_id: hint.source_symbol.symbol_id,
                        relation: hint.relation.as_str().to_owned(),
                    },
                },
            );
        }

        let mut symbols_by_path: HashMap<PathBuf, Vec<&'a SymbolDefinition>> = HashMap::new();
        for symbol in symbols {
            symbols_by_path
                .entry(symbol.path.clone())
                .or_default()
                .push(symbol);
        }

        Some(Self {
            repository_id,
            target_symbol,
            relation_hint_by_source,
            symbols_by_path,
            by_location,
        })
    }

    pub fn ingest_source(&mut self, path: &Path, source: &str) {
        if !is_identifier_token(&self.target_symbol.name) {
            return;
        }

        let symbols_for_path = self
            .symbols_by_path
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut containing_symbol_by_line: HashMap<usize, Option<&SymbolDefinition>> =
            HashMap::new();

        for (line_index, line) in source.lines().enumerate() {
            let line_number = line_index + 1;
            let columns = token_columns(line, &self.target_symbol.name);
            if columns.is_empty() {
                continue;
            }

            for column in columns {
                if path == self.target_symbol.path.as_path()
                    && line_number == self.target_symbol.line
                {
                    continue;
                }

                let containing_symbol = *containing_symbol_by_line
                    .entry(line_number)
                    .or_insert_with(|| {
                        find_innermost_symbol_for_line_in_file(symbols_for_path, line_number)
                    });
                let (confidence, evidence) = containing_symbol
                    .and_then(|symbol| {
                        self.relation_hint_by_source
                            .get(symbol.stable_id.as_str())
                            .map(|(confidence, relation)| {
                                (
                                    *confidence,
                                    HeuristicReferenceEvidence::GraphRelation {
                                        source_symbol_id: symbol.stable_id.clone(),
                                        relation: relation.clone(),
                                    },
                                )
                            })
                    })
                    .unwrap_or((
                        HeuristicReferenceConfidence::Low,
                        HeuristicReferenceEvidence::LexicalToken,
                    ));

                upsert_heuristic_reference(
                    &mut self.by_location,
                    HeuristicReference {
                        repository_id: self.repository_id.to_owned(),
                        symbol_id: self.target_symbol.stable_id.clone(),
                        symbol_name: self.target_symbol.name.clone(),
                        path: path.to_path_buf(),
                        line: line_number,
                        column,
                        confidence,
                        heuristic: true,
                        evidence,
                    },
                );
            }
        }
    }

    pub fn finish(self) -> Vec<HeuristicReference> {
        let mut references = self.by_location.into_values().collect::<Vec<_>>();
        references.sort_by(heuristic_reference_order);
        references
    }
}

pub fn resolve_heuristic_references(
    repository_id: &str,
    symbol_id: &str,
    symbols: &[SymbolDefinition],
    graph: &SymbolGraph,
    sources_by_path: &BTreeMap<PathBuf, String>,
) -> Vec<HeuristicReference> {
    let Some(mut resolver) =
        HeuristicReferenceResolver::new(repository_id, symbol_id, symbols, graph)
    else {
        return Vec::new();
    };

    for (path, source) in sources_by_path {
        resolver.ingest_source(path, source);
    }

    resolver.finish()
}

fn heuristic_reference_order(
    left: &HeuristicReference,
    right: &HeuristicReference,
) -> std::cmp::Ordering {
    right
        .confidence
        .cmp(&left.confidence)
        .then(left.path.cmp(&right.path))
        .then(left.line.cmp(&right.line))
        .then(left.column.cmp(&right.column))
        .then(
            heuristic_evidence_rank(&right.evidence).cmp(&heuristic_evidence_rank(&left.evidence)),
        )
}

fn heuristic_evidence_rank(evidence: &HeuristicReferenceEvidence) -> u8 {
    match evidence {
        HeuristicReferenceEvidence::GraphRelation { .. } => 2,
        HeuristicReferenceEvidence::LexicalToken => 1,
    }
}

fn upsert_heuristic_reference(
    by_location: &mut BTreeMap<(PathBuf, usize, usize), HeuristicReference>,
    candidate: HeuristicReference,
) {
    let key = (candidate.path.clone(), candidate.line, candidate.column);
    let should_replace = match by_location.get(&key) {
        None => true,
        Some(existing) => {
            candidate.confidence > existing.confidence
                || (candidate.confidence == existing.confidence
                    && heuristic_evidence_rank(&candidate.evidence)
                        > heuristic_evidence_rank(&existing.evidence))
        }
    };

    if should_replace {
        by_location.insert(key, candidate);
    }
}

fn is_identifier_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(is_identifier_byte)
}

fn token_columns(line: &str, token: &str) -> Vec<usize> {
    if token.is_empty() || token.len() > line.len() {
        return Vec::new();
    }

    let mut columns = Vec::new();
    let mut offset = 0;
    while let Some(relative) = line[offset..].find(token) {
        let start = offset + relative;
        let end = start + token.len();
        if token_has_boundaries(line.as_bytes(), start, end) {
            columns.push(start + 1);
        }
        offset = end;
        if offset >= line.len() {
            break;
        }
    }
    columns
}

fn token_has_boundaries(line: &[u8], start: usize, end: usize) -> bool {
    let left_is_boundary = if start == 0 {
        true
    } else {
        !is_identifier_byte(line[start - 1])
    };
    let right_is_boundary = if end >= line.len() {
        true
    } else {
        !is_identifier_byte(line[end])
    };

    left_is_boundary && right_is_boundary
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn find_innermost_symbol_for_line_in_file<'a>(
    symbols_for_path: &[&'a SymbolDefinition],
    line: usize,
) -> Option<&'a SymbolDefinition> {
    symbols_for_path
        .iter()
        .copied()
        .filter(|symbol| line >= symbol.span.start_line && line <= symbol.span.end_line)
        .min_by(|left, right| {
            let left_span = left.span.end_line.saturating_sub(left.span.start_line);
            let right_span = right.span.end_line.saturating_sub(right.span.start_line);
            left_span
                .cmp(&right_span)
                .then(left.span.start_line.cmp(&right.span.start_line))
                .then(left.stable_id.cmp(&right.stable_id))
        })
}
