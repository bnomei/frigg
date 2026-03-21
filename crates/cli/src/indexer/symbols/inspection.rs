use super::*;
use crate::domain::model::{
    GeneratedStructuralFollowUp, GeneratedStructuralFollowUpBasis,
    GeneratedStructuralFollowUpConfidence, GeneratedStructuralFollowUpStrategy,
    GeneratedStructuralSearchParams,
};
use smallvec::SmallVec;

const GENERATED_STRUCTURAL_CAPTURE_NAME: &str = "match";
const GENERATED_STRUCTURAL_MAX_SUGGESTIONS: usize = 3;
const LOW_SIGNAL_STRUCTURAL_NODE_KINDS: &[&str] = &[
    "source_file",
    "program",
    "chunk",
    "block",
    "expression_statement",
    "statement_block",
    "declaration_list",
    "body",
    "arguments",
    "argument_list",
    "parameter_list",
    "parameters",
    "formal_parameters",
    "visibility_modifier",
    "identifier",
    "property_identifier",
    "field_identifier",
    "type_identifier",
    "name",
    "comment",
    "string",
    "string_literal",
    "integer",
    "float",
    "number",
    "ERROR",
];

struct StructuralFollowUpContext<'a> {
    display_path: &'a str,
    repository_id: &'a str,
}

struct StructuralQueryCaptureNode<'tree> {
    name: String,
    node: Node<'tree>,
    item: StructuralQueryCapture,
}

pub fn search_structural_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    query: &str,
) -> FriggResult<Vec<StructuralQueryMatch>> {
    search_structural_matches_in_source(
        language,
        path,
        source,
        query,
        StructuralQueryResultMode::Captures,
        None,
        None,
    )
}

pub fn search_structural_with_follow_up_in_source(
    language: SymbolLanguage,
    path: &Path,
    display_path: &str,
    source: &str,
    query: &str,
    repository_id: &str,
) -> FriggResult<Vec<StructuralQueryMatch>> {
    search_structural_matches_in_source(
        language,
        path,
        source,
        query,
        StructuralQueryResultMode::Captures,
        None,
        Some(StructuralFollowUpContext {
            display_path,
            repository_id,
        }),
    )
}

pub fn search_structural_grouped_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    query: &str,
    primary_capture: Option<&str>,
) -> FriggResult<Vec<StructuralQueryMatch>> {
    search_structural_matches_in_source(
        language,
        path,
        source,
        query,
        StructuralQueryResultMode::Matches,
        primary_capture,
        None,
    )
}

pub fn search_structural_grouped_with_follow_up_in_source(
    language: SymbolLanguage,
    path: &Path,
    display_path: &str,
    source: &str,
    query: &str,
    primary_capture: Option<&str>,
    repository_id: &str,
) -> FriggResult<Vec<StructuralQueryMatch>> {
    search_structural_matches_in_source(
        language,
        path,
        source,
        query,
        StructuralQueryResultMode::Matches,
        primary_capture,
        Some(StructuralFollowUpContext {
            display_path,
            repository_id,
        }),
    )
}

pub fn inspect_syntax_tree_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    line: Option<usize>,
    column: Option<usize>,
    max_ancestors: usize,
    max_children: usize,
) -> FriggResult<SyntaxTreeInspection> {
    inspect_syntax_tree_internal(
        language,
        path,
        source,
        line,
        column,
        max_ancestors,
        max_children,
        None,
    )
    .map(|(inspection, _)| inspection)
}

#[allow(clippy::too_many_arguments)]
pub fn inspect_syntax_tree_with_follow_up_in_source(
    language: SymbolLanguage,
    path: &Path,
    display_path: &str,
    source: &str,
    line: Option<usize>,
    column: Option<usize>,
    max_ancestors: usize,
    max_children: usize,
    repository_id: &str,
) -> FriggResult<(SyntaxTreeInspection, Vec<GeneratedStructuralFollowUp>)> {
    inspect_syntax_tree_internal(
        language,
        path,
        source,
        line,
        column,
        max_ancestors,
        max_children,
        Some(StructuralFollowUpContext {
            display_path,
            repository_id,
        }),
    )
}

#[cfg(test)]
pub fn generated_follow_up_structural_for_focus(
    language: SymbolLanguage,
    repository_id: &str,
    display_path: &str,
    focus: &SyntaxTreeInspectionNode,
    ancestors: &[SyntaxTreeInspectionNode],
) -> Vec<GeneratedStructuralFollowUp> {
    let raw_focus_kind = focus.kind.as_str();
    let focus_kind = if is_useful_named_kind(raw_focus_kind) {
        Some(raw_focus_kind)
    } else {
        ancestors
            .iter()
            .map(|node| node.kind.as_str())
            .find(|kind| is_useful_named_kind(kind))
    };
    let Some(focus_kind) = focus_kind else {
        return Vec::new();
    };
    let ancestor_kind = ancestors
        .iter()
        .map(|node| node.kind.as_str())
        .filter(|kind| is_useful_named_kind(kind))
        .find(|kind| *kind != focus_kind);
    structural_follow_up_queries_for_kinds(
        language,
        display_path,
        repository_id,
        raw_focus_kind,
        focus_kind,
        ancestor_kind,
    )
}

#[cfg(test)]
pub fn generated_follow_up_structural_for_location_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    line: usize,
    column: usize,
    repository_id: &str,
    display_path: &str,
) -> FriggResult<Vec<GeneratedStructuralFollowUp>> {
    generated_follow_up_structural_at_location_in_source(
        language,
        path,
        display_path,
        source,
        line,
        column,
        repository_id,
    )
}

pub fn generated_follow_up_structural_at_location_in_source(
    language: SymbolLanguage,
    path: &Path,
    display_path: &str,
    source: &str,
    line: usize,
    column: usize,
    repository_id: &str,
) -> FriggResult<Vec<GeneratedStructuralFollowUp>> {
    let tree = parse_tree_for_source(language, path, source, "structural follow-up synthesis")?;
    let root = tree.root_node();
    let offset = byte_offset_for_line_column(source, line, column).ok_or_else(|| {
        FriggError::InvalidInput(format!(
            "location {line}:{column} is outside file {}",
            path.display()
        ))
    })?;
    let focus_node = focus_node_for_offset(root, offset);
    Ok(structural_follow_up_queries_for_node(
        language,
        display_path,
        repository_id,
        focus_node,
    ))
}

fn search_structural_matches_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    query: &str,
    result_mode: StructuralQueryResultMode,
    primary_capture: Option<&str>,
    follow_up_context: Option<StructuralFollowUpContext<'_>>,
) -> FriggResult<Vec<StructuralQueryMatch>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(FriggError::InvalidInput(
            "structural query must not be empty".to_owned(),
        ));
    }

    let compiled_query = compile_structural_query(language, path, query)?;
    let tree = parse_tree_for_source(language, path, source, "structural search")?;
    let mut cursor = QueryCursor::new();
    let mut matches = Vec::new();
    match result_mode {
        StructuralQueryResultMode::Captures => {
            let capture_names = compiled_query.capture_names();
            let mut captures =
                cursor.captures(&compiled_query, tree.root_node(), source.as_bytes());
            while let Some((query_match, capture_index)) = captures.next() {
                let capture = query_match.captures[*capture_index];
                let capture_name = capture_names
                    .get(capture.index as usize)
                    .cloned()
                    .unwrap_or(GENERATED_STRUCTURAL_CAPTURE_NAME);
                let follow_up_structural = follow_up_context
                    .as_ref()
                    .map(|context| {
                        structural_follow_up_queries_for_node(
                            language,
                            context.display_path,
                            context.repository_id,
                            capture.node,
                        )
                    })
                    .unwrap_or_default();
                let Some(matched) = structural_query_capture_row(
                    source,
                    path,
                    capture_name,
                    capture.node,
                    follow_up_structural,
                ) else {
                    continue;
                };
                matches.push(matched);
            }
        }
        StructuralQueryResultMode::Matches => {
            let capture_names = compiled_query.capture_names();
            let mut query_matches =
                cursor.matches(&compiled_query, tree.root_node(), source.as_bytes());
            while let Some(query_match) = query_matches.next() {
                let mut capture_nodes = query_match
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let capture_name = capture_names
                            .get(capture.index as usize)
                            .cloned()
                            .unwrap_or(GENERATED_STRUCTURAL_CAPTURE_NAME);
                        structural_query_capture_node(source, capture_name, capture.node)
                    })
                    .collect::<SmallVec<[StructuralQueryCaptureNode<'_>; 8]>>();
                if capture_nodes.is_empty() {
                    continue;
                }
                capture_nodes.sort_by(structural_capture_node_order);
                let (anchor_index, anchor_selection) =
                    select_structural_anchor(&capture_nodes, primary_capture);
                let anchor_node = capture_nodes[anchor_index].node;
                let follow_up_structural = follow_up_context
                    .as_ref()
                    .map(|context| {
                        structural_follow_up_queries_for_node(
                            language,
                            context.display_path,
                            context.repository_id,
                            anchor_node,
                        )
                    })
                    .unwrap_or_default();
                let Some(matched) = structural_query_grouped_match(
                    path,
                    &capture_nodes,
                    anchor_index,
                    anchor_selection,
                    follow_up_structural,
                ) else {
                    continue;
                };
                matches.push(matched);
            }
        }
    }
    matches.sort_by(structural_query_match_order);
    matches.dedup();
    Ok(matches)
}

#[allow(clippy::too_many_arguments)]
fn inspect_syntax_tree_internal(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    line: Option<usize>,
    column: Option<usize>,
    max_ancestors: usize,
    max_children: usize,
    follow_up_context: Option<StructuralFollowUpContext<'_>>,
) -> FriggResult<(SyntaxTreeInspection, Vec<GeneratedStructuralFollowUp>)> {
    validate_syntax_tree_inspection_request(line, column)?;

    let tree = parse_tree_for_source(language, path, source, "syntax inspection")?;
    let root = tree.root_node();
    let (raw_focus_node, focus_node) = match (line, column) {
        (Some(line), Some(column)) => {
            let offset = byte_offset_for_line_column(source, line, column).ok_or_else(|| {
                FriggError::InvalidInput(format!(
                    "location {line}:{column} is outside file {}",
                    path.display()
                ))
            })?;
            let raw_focus = focus_node_for_offset(root, offset);
            let normalized_focus = first_useful_named_node(raw_focus).unwrap_or(raw_focus);
            (Some(raw_focus), normalized_focus)
        }
        _ => (None, root),
    };

    let inspection = build_syntax_tree_inspection(
        language,
        source,
        focus_node,
        raw_focus_node,
        max_ancestors,
        max_children,
    );
    let follow_up_structural = follow_up_context
        .map(|context| {
            structural_follow_up_queries_for_node(
                language,
                context.display_path,
                context.repository_id,
                raw_focus_node.unwrap_or(focus_node),
            )
        })
        .unwrap_or_default();
    Ok((inspection, follow_up_structural))
}

fn validate_syntax_tree_inspection_request(
    line: Option<usize>,
    column: Option<usize>,
) -> FriggResult<()> {
    if line == Some(0) {
        return Err(FriggError::InvalidInput(
            "line must be greater than zero when provided".to_owned(),
        ));
    }
    if column == Some(0) {
        return Err(FriggError::InvalidInput(
            "column must be greater than zero when provided".to_owned(),
        ));
    }
    if line.is_none() != column.is_none() {
        return Err(FriggError::InvalidInput(
            "line and column must be provided together".to_owned(),
        ));
    }
    Ok(())
}

fn compile_structural_query(
    language: SymbolLanguage,
    path: &Path,
    query: &str,
) -> FriggResult<Query> {
    let ts_language = tree_sitter_language_for_path(language, path);
    Query::new(&ts_language, query).map_err(|error| {
        FriggError::InvalidInput(format!(
            "invalid structural query for {}: {error}",
            language.as_str()
        ))
    })
}

fn parse_tree_for_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    operation: &str,
) -> FriggResult<Tree> {
    let mut parser = parser_for_path(language, path)?;
    parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for {operation}: {}",
            path.display()
        ))
    })
}

fn build_syntax_tree_inspection(
    language: SymbolLanguage,
    source: &str,
    focus_node: Node<'_>,
    raw_focus_node: Option<Node<'_>>,
    max_ancestors: usize,
    max_children: usize,
) -> SyntaxTreeInspection {
    let mut ancestors = SmallVec::<[SyntaxTreeInspectionNode; 8]>::new();
    let mut cursor = focus_node;
    while let Some(parent) = cursor.parent() {
        ancestors.push(syntax_tree_inspection_node(source, parent));
        cursor = parent;
        if ancestors.len() >= max_ancestors {
            break;
        }
    }

    let mut children = SmallVec::<[SyntaxTreeInspectionNode; 8]>::new();
    let mut child_cursor = focus_node.walk();
    for child in focus_node.children(&mut child_cursor) {
        children.push(syntax_tree_inspection_node(source, child));
        if children.len() >= max_children {
            break;
        }
    }

    SyntaxTreeInspection {
        language,
        focus: syntax_tree_inspection_node(source, focus_node),
        raw_focus: raw_focus_node.map(|node| syntax_tree_inspection_node(source, node)),
        ancestors: ancestors.into_vec(),
        children: children.into_vec(),
    }
}

fn structural_query_capture_node<'tree>(
    source: &str,
    capture_name: &str,
    node: Node<'tree>,
) -> Option<StructuralQueryCaptureNode<'tree>> {
    let span = source_span(node);
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let excerpt = if start_byte <= end_byte && end_byte <= source.len() {
        String::from_utf8_lossy(&source.as_bytes()[start_byte..end_byte])
            .trim()
            .to_owned()
    } else {
        String::new()
    };
    if excerpt.is_empty() {
        return None;
    }
    Some(StructuralQueryCaptureNode {
        name: capture_name.to_owned(),
        node,
        item: StructuralQueryCapture {
            name: capture_name.to_owned(),
            span,
            excerpt,
        },
    })
}

fn structural_query_capture_row(
    source: &str,
    path: &Path,
    capture_name: &str,
    node: Node<'_>,
    follow_up_structural: Vec<GeneratedStructuralFollowUp>,
) -> Option<StructuralQueryMatch> {
    let capture = structural_query_capture_node(source, capture_name, node)?;
    Some(StructuralQueryMatch {
        path: path.to_path_buf(),
        span: capture.item.span.clone(),
        excerpt: capture.item.excerpt.clone(),
        anchor_capture_name: Some(capture.name.clone()),
        anchor_selection: StructuralQueryAnchorSelection::CaptureRow,
        captures: vec![capture.item],
        follow_up_structural,
    })
}

fn structural_query_grouped_match(
    path: &Path,
    captures: &[StructuralQueryCaptureNode<'_>],
    anchor_index: usize,
    anchor_selection: StructuralQueryAnchorSelection,
    follow_up_structural: Vec<GeneratedStructuralFollowUp>,
) -> Option<StructuralQueryMatch> {
    let anchor = captures.get(anchor_index)?;
    Some(StructuralQueryMatch {
        path: path.to_path_buf(),
        span: anchor.item.span.clone(),
        excerpt: anchor.item.excerpt.clone(),
        anchor_capture_name: Some(anchor.name.clone()),
        anchor_selection,
        captures: captures
            .iter()
            .map(|capture| capture.item.clone())
            .collect(),
        follow_up_structural,
    })
}

fn structural_capture_node_order(
    left: &StructuralQueryCaptureNode<'_>,
    right: &StructuralQueryCaptureNode<'_>,
) -> std::cmp::Ordering {
    left.item
        .span
        .start_byte
        .cmp(&right.item.span.start_byte)
        .then(left.item.span.end_byte.cmp(&right.item.span.end_byte))
        .then(left.item.name.cmp(&right.item.name))
        .then(left.item.excerpt.cmp(&right.item.excerpt))
}

fn select_structural_anchor(
    captures: &[StructuralQueryCaptureNode<'_>],
    primary_capture: Option<&str>,
) -> (usize, StructuralQueryAnchorSelection) {
    let preferred_capture = primary_capture
        .map(str::trim)
        .filter(|capture| !capture.is_empty());
    if let Some(primary_capture) = preferred_capture
        && let Some(index) = captures
            .iter()
            .position(|capture| capture.name == primary_capture)
    {
        return (index, StructuralQueryAnchorSelection::PrimaryCapture);
    }
    if let Some(index) = captures
        .iter()
        .position(|capture| capture.name == GENERATED_STRUCTURAL_CAPTURE_NAME)
    {
        return (index, StructuralQueryAnchorSelection::MatchCapture);
    }
    if let Some(index) = captures
        .iter()
        .position(|capture| is_useful_named_node(capture.node))
    {
        return (
            index,
            StructuralQueryAnchorSelection::FirstUsefulNamedCapture,
        );
    }
    (0, StructuralQueryAnchorSelection::FirstCapture)
}

fn structural_follow_up_queries_for_node(
    language: SymbolLanguage,
    display_path: &str,
    repository_id: &str,
    raw_focus_node: Node<'_>,
) -> Vec<GeneratedStructuralFollowUp> {
    let Some(focus_node) = first_useful_named_node(raw_focus_node) else {
        return Vec::new();
    };
    let ancestor_node = next_useful_named_ancestor(focus_node);
    structural_follow_up_queries_for_kinds(
        language,
        display_path,
        repository_id,
        raw_focus_node.kind(),
        focus_node.kind(),
        ancestor_node.map(|node| node.kind()),
    )
}

fn structural_follow_up_queries_for_kinds(
    language: SymbolLanguage,
    display_path: &str,
    repository_id: &str,
    raw_focus_kind: &str,
    focus_kind: &str,
    ancestor_kind: Option<&str>,
) -> Vec<GeneratedStructuralFollowUp> {
    let basis = GeneratedStructuralFollowUpBasis {
        focus_kind: focus_kind.to_owned(),
        raw_focus_kind: (raw_focus_kind != focus_kind).then(|| raw_focus_kind.to_owned()),
        ancestor_kind: ancestor_kind.map(str::to_owned),
    };
    let mut suggestions = Vec::with_capacity(GENERATED_STRUCTURAL_MAX_SUGGESTIONS);
    push_structural_follow_up(
        &mut suggestions,
        GeneratedStructuralFollowUpStrategy::FocusNamedNodeFileScoped,
        GeneratedStructuralFollowUpConfidence::High,
        basis.clone(),
        structural_follow_up_query(focus_kind),
        Some(structural_follow_up_path_regex(display_path)),
        language,
        repository_id,
    );
    push_structural_follow_up(
        &mut suggestions,
        GeneratedStructuralFollowUpStrategy::FocusNamedNodeRepoScoped,
        GeneratedStructuralFollowUpConfidence::Medium,
        basis.clone(),
        structural_follow_up_query(focus_kind),
        None,
        language,
        repository_id,
    );
    if let Some(ancestor_kind) = ancestor_kind {
        push_structural_follow_up(
            &mut suggestions,
            GeneratedStructuralFollowUpStrategy::AncestorNamedNodeRepoScoped,
            GeneratedStructuralFollowUpConfidence::Medium,
            basis,
            structural_follow_up_query(ancestor_kind),
            None,
            language,
            repository_id,
        );
    }
    suggestions.truncate(GENERATED_STRUCTURAL_MAX_SUGGESTIONS);
    suggestions
}

#[allow(clippy::too_many_arguments)]
fn push_structural_follow_up(
    suggestions: &mut Vec<GeneratedStructuralFollowUp>,
    strategy: GeneratedStructuralFollowUpStrategy,
    confidence: GeneratedStructuralFollowUpConfidence,
    basis: GeneratedStructuralFollowUpBasis,
    query: String,
    path_regex: Option<String>,
    language: SymbolLanguage,
    repository_id: &str,
) {
    if suggestions.iter().any(|candidate| {
        candidate.params.query == query && candidate.params.path_regex == path_regex
    }) {
        return;
    }
    suggestions.push(GeneratedStructuralFollowUp {
        strategy,
        confidence,
        basis,
        params: GeneratedStructuralSearchParams {
            query,
            language: language.as_str().to_owned(),
            repository_id: repository_id.to_owned(),
            path_regex,
            limit: None,
        },
    });
}

fn first_useful_named_node(start: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = Some(start);
    while let Some(node) = cursor {
        if is_useful_named_node(node) {
            return Some(node);
        }
        cursor = node.parent();
    }
    None
}

fn next_useful_named_ancestor(start: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = start.parent();
    while let Some(node) = cursor {
        if is_useful_named_node(node) && node.kind() != start.kind() {
            return Some(node);
        }
        cursor = node.parent();
    }
    None
}

fn is_useful_named_node(node: Node<'_>) -> bool {
    node.is_named() && is_useful_named_kind(node.kind())
}

fn is_useful_named_kind(kind: &str) -> bool {
    is_query_safe_named_kind(kind) && !LOW_SIGNAL_STRUCTURAL_NODE_KINDS.contains(&kind)
}

fn is_query_safe_named_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn structural_follow_up_query(kind: &str) -> String {
    format!("({kind}) @{GENERATED_STRUCTURAL_CAPTURE_NAME}")
}

fn structural_follow_up_path_regex(display_path: &str) -> String {
    format!("^{}$", regex::escape(display_path))
}

fn focus_node_for_offset(root: Node<'_>, offset: usize) -> Node<'_> {
    let mut current = root;
    loop {
        let mut next_named = None;
        let mut next_any = None;
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.start_byte() > offset || child.end_byte() < offset {
                continue;
            }
            if next_any.is_none() {
                next_any = Some(child);
            }
            if child.is_named() {
                next_named = Some(child);
                break;
            }
        }
        let Some(next) = next_named.or(next_any) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn syntax_tree_inspection_node(source: &str, node: Node<'_>) -> SyntaxTreeInspectionNode {
    SyntaxTreeInspectionNode {
        kind: node.kind().to_owned(),
        named: node.is_named(),
        span: source_span(node),
        excerpt: trim_syntax_excerpt(source, node.start_byte(), node.end_byte()),
    }
}

fn trim_syntax_excerpt(source: &str, start_byte: usize, end_byte: usize) -> String {
    const MAX_EXCERPT_CHARS: usize = 120;
    if start_byte >= end_byte || start_byte >= source.len() {
        return String::new();
    }
    let clamped_end = end_byte.min(source.len());
    let raw = String::from_utf8_lossy(&source.as_bytes()[start_byte..clamped_end]);
    let trimmed = raw.trim();
    if trimmed.chars().count() <= MAX_EXCERPT_CHARS {
        return trimmed.to_owned();
    }
    let mut excerpt = trimmed.chars().take(MAX_EXCERPT_CHARS).collect::<String>();
    excerpt.push_str("...");
    excerpt
}

fn structural_query_match_order(
    left: &StructuralQueryMatch,
    right: &StructuralQueryMatch,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.span.start_byte.cmp(&right.span.start_byte))
        .then(left.span.end_byte.cmp(&right.span.end_byte))
        .then(left.span.start_line.cmp(&right.span.start_line))
        .then(left.span.start_column.cmp(&right.span.start_column))
        .then(left.excerpt.cmp(&right.excerpt))
}
