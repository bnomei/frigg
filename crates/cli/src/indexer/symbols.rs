use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{FriggError, FriggResult};
use crate::graph::{HeuristicConfidence, SymbolGraph, SymbolNode};
use crate::languages::{
    SymbolLanguage, collect_blade_symbols_from_source, parser_for_path, symbol_from_node,
    tree_sitter_language_for_path,
};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

mod extraction;
mod heuristics;
mod inspection;
mod model;
mod spans;

pub(crate) use extraction::push_symbol_definition;
pub use extraction::{
    extract_symbols_for_paths, extract_symbols_from_file, extract_symbols_from_source,
};
pub use heuristics::{HeuristicReferenceResolver, resolve_heuristic_references};
pub use inspection::{
    generated_follow_up_structural_at_location_in_source, inspect_syntax_tree_in_source,
    inspect_syntax_tree_with_follow_up_in_source, search_structural_grouped_in_source,
    search_structural_grouped_with_follow_up_in_source, search_structural_in_source,
    search_structural_with_follow_up_in_source,
};
#[cfg(test)]
pub use inspection::{
    generated_follow_up_structural_for_focus, generated_follow_up_structural_for_location_in_source,
};
pub use model::*;
pub(crate) use spans::{
    byte_offset_for_line_column, line_column_for_offset, source_span, source_span_from_offsets,
};

pub fn register_symbol_definitions(
    graph: &mut SymbolGraph,
    repository_id: &str,
    symbols: &[SymbolDefinition],
) {
    graph.register_symbols(symbols.iter().map(|symbol| {
        SymbolNode::new(
            symbol.stable_id.clone(),
            repository_id.to_owned(),
            symbol.name.clone(),
            symbol.kind.as_str().to_owned(),
            symbol.path.to_string_lossy().into_owned(),
            symbol.line,
        )
    }));
}

pub fn navigation_symbol_target_rank(symbol: &SymbolDefinition, symbol_query: &str) -> Option<u8> {
    if symbol.stable_id == symbol_query {
        return Some(0);
    }
    if symbol.name == symbol_query {
        return Some(1);
    }
    if symbol.name.eq_ignore_ascii_case(symbol_query) {
        return Some(2);
    }

    None
}
