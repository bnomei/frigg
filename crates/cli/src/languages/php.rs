use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::node_name_text;

#[path = "php/declarations.rs"]
mod declarations;
#[path = "php/evidence.rs"]
mod evidence;
#[path = "php/resolution.rs"]
mod resolution;

#[allow(unused_imports)]
pub(crate) use declarations::{
    PhpDeclarationRelation, PhpGraphSourceAnalysis, declaration_relation_edges_for_file,
    declaration_relation_edges_for_relations, declaration_relation_edges_for_source,
    extract_declaration_relations_from_source, extract_graph_analysis_from_source,
    heuristic_implementation_candidates_for_target, symbol_indices_by_lower_name,
    symbol_indices_by_name,
};
#[allow(unused_imports)]
pub(crate) use evidence::{
    PhpLiteralEvidence, PhpSourceEvidence, PhpTargetEvidence, PhpTargetEvidenceKind,
    PhpTypeEvidence, PhpTypeEvidenceKind, extract_source_evidence_from_source,
    resolve_target_evidence_edges,
};
#[allow(unused_imports)]
pub(crate) use resolution::{
    PhpNameResolutionContext, PhpSymbolLookup, php_class_like_name_candidates,
    php_name_resolution_context_from_root, php_relation_targets_symbol_name,
    resolve_php_declaration_relation_indices,
};

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "namespace_definition" => {
            node_name_text(node, source).map(|name| (SymbolKind::Module, name))
        }
        "function_definition" => {
            node_name_text(node, source).map(|name| (SymbolKind::Function, name))
        }
        "class_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Class, name)),
        "interface_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Interface, name))
        }
        "trait_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::PhpTrait, name))
        }
        "enum_declaration" => node_name_text(node, source).map(|name| (SymbolKind::PhpEnum, name)),
        "enum_case" => node_name_text(node, source).map(|name| (SymbolKind::EnumCase, name)),
        "method_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Method, name)),
        "property_element" => node_name_text(node, source).map(|name| (SymbolKind::Property, name)),
        "const_element" => node_name_text(node, source).map(|name| (SymbolKind::Constant, name)),
        _ => None,
    }
}
