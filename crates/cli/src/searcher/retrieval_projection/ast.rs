use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::graph::RelationKind;
use crate::indexer::{
    SymbolDefinition, SymbolKind, extract_php_source_evidence_from_source,
    extract_symbols_for_paths,
};
use crate::languages::{
    SymbolLanguage, extract_blade_source_evidence_from_source, php_symbol_indices_by_lower_name,
    php_symbol_indices_by_name, resolve_blade_relation_evidence_edges,
    resolve_php_target_evidence_edges,
};
use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection,
};

use super::super::candidates::normalize_repository_relative_path;
use super::super::path_witness_projection::StoredPathWitnessProjection;
use super::super::path_witness_projection::family_bits_for_projection;
use super::RETRIEVAL_PROJECTION_INPUT_MODE_AST;

pub(crate) fn augment_path_relation_projection_records_with_ast_relation_evidence(
    workspace_root: &Path,
    absolute_manifest_paths: &[PathBuf],
    path_witness: &[StoredPathWitnessProjection],
    path_relations: &mut Vec<PathRelationProjection>,
) {
    let extracted = extract_symbols_for_paths(absolute_manifest_paths);
    if extracted.symbols.is_empty() {
        return;
    }

    let witness_by_path = path_witness
        .iter()
        .map(|projection| (projection.path.clone(), projection))
        .collect::<BTreeMap<_, _>>();
    let relative_symbols = extracted
        .symbols
        .into_iter()
        .filter_map(|symbol| {
            let relative_path = normalize_repository_relative_path(workspace_root, &symbol.path);
            witness_by_path
                .contains_key(&relative_path)
                .then_some((relative_path, symbol))
        })
        .collect::<Vec<_>>();
    if relative_symbols.is_empty() {
        return;
    }

    let mut symbols = Vec::with_capacity(relative_symbols.len());
    let mut symbol_index_by_stable_id = BTreeMap::<String, usize>::new();
    let mut relative_path_by_stable_id = BTreeMap::<String, String>::new();
    let mut file_symbols_by_path = BTreeMap::<String, Vec<SymbolDefinition>>::new();
    for (index, (relative_path, symbol)) in relative_symbols.iter().enumerate() {
        symbol_index_by_stable_id.insert(symbol.stable_id.clone(), index);
        relative_path_by_stable_id.insert(symbol.stable_id.clone(), relative_path.clone());
        file_symbols_by_path
            .entry(relative_path.clone())
            .or_default()
            .push(symbol.clone());
        symbols.push(symbol.clone());
    }

    let symbol_indices_by_name = php_symbol_indices_by_name(&symbols);
    let symbol_indices_by_lower_name = php_symbol_indices_by_lower_name(&symbols);
    let mut php_canonical_names_by_stable_id = BTreeMap::<String, String>::new();
    let mut php_evidence = Vec::new();
    let mut blade_evidence = Vec::new();

    for absolute_path in absolute_manifest_paths {
        let relative_path = normalize_repository_relative_path(workspace_root, absolute_path);
        if !witness_by_path.contains_key(&relative_path) {
            continue;
        }

        match SymbolLanguage::from_path(absolute_path) {
            Some(SymbolLanguage::Php) => {
                let Ok(source) = fs::read_to_string(absolute_path) else {
                    continue;
                };
                let file_symbols = file_symbols_by_path
                    .get(&relative_path)
                    .cloned()
                    .unwrap_or_default();
                let Ok(evidence) =
                    extract_php_source_evidence_from_source(absolute_path, &source, &file_symbols)
                else {
                    continue;
                };
                php_canonical_names_by_stable_id
                    .extend(evidence.canonical_names_by_stable_id.clone().into_iter());
                php_evidence.push(evidence);
            }
            Some(SymbolLanguage::Blade) => {
                let Ok(source) = fs::read_to_string(absolute_path) else {
                    continue;
                };
                let file_symbols = file_symbols_by_path
                    .get(&relative_path)
                    .cloned()
                    .unwrap_or_default();
                blade_evidence.push(extract_blade_source_evidence_from_source(
                    &source,
                    &file_symbols,
                ));
            }
            _ => {}
        }
    }

    let mut symbol_indices_by_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    for (stable_id, canonical_name) in &php_canonical_names_by_stable_id {
        let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
            continue;
        };
        symbol_indices_by_canonical_name
            .entry(canonical_name.clone())
            .or_default()
            .push(symbol_index);
        symbol_indices_by_lower_canonical_name
            .entry(canonical_name.to_ascii_lowercase())
            .or_default()
            .push(symbol_index);
    }

    for evidence in &php_evidence {
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_php_target_evidence_edges(
                &symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_canonical_name,
                &symbol_indices_by_lower_canonical_name,
                evidence,
            )
        {
            push_ast_relation_projection(
                path_relations,
                &symbols,
                &witness_by_path,
                &relative_path_by_stable_id,
                source_symbol_index,
                target_symbol_index,
                relation,
            );
        }
    }

    for evidence in &blade_evidence {
        for (source_symbol_index, target_symbol_index, relation) in
            resolve_blade_relation_evidence_edges(
                &symbols,
                &symbol_index_by_stable_id,
                &symbol_indices_by_name,
                &symbol_indices_by_lower_name,
                evidence,
            )
        {
            push_ast_relation_projection(
                path_relations,
                &symbols,
                &witness_by_path,
                &relative_path_by_stable_id,
                source_symbol_index,
                target_symbol_index,
                relation,
            );
        }
    }
}

pub(super) fn apply_ast_projection_contributions(
    workspace_root: &Path,
    absolute_manifest_paths: &[PathBuf],
    path_witness: &[StoredPathWitnessProjection],
    path_relations: &mut Vec<PathRelationProjection>,
    path_surface_terms: &mut Vec<PathSurfaceTermProjection>,
    path_anchor_sketches: &mut Vec<PathAnchorSketchProjection>,
    input_modes: &mut super::bundle::RetrievalProjectionInputModes,
) {
    let extracted = extract_symbols_for_paths(absolute_manifest_paths);
    if extracted.symbols.is_empty() {
        return;
    }

    let witness_by_path = path_witness
        .iter()
        .map(|projection| (projection.path.clone(), projection))
        .collect::<BTreeMap<_, _>>();
    let mut relative_symbols = extracted
        .symbols
        .into_iter()
        .filter_map(|symbol| {
            let relative_path = normalize_repository_relative_path(workspace_root, &symbol.path);
            witness_by_path
                .contains_key(&relative_path)
                .then_some((relative_path, symbol))
        })
        .collect::<Vec<_>>();
    if relative_symbols.is_empty() {
        return;
    }

    let mut anchor_candidates = path_anchor_sketches
        .drain(..)
        .map(|projection| (projection.path.clone(), projection))
        .fold(
            BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new(),
            |mut acc, (path, row)| {
                acc.entry(path).or_default().push(row);
                acc
            },
        );

    for (relative_path, symbol) in &relative_symbols {
        let Some(witness_projection) = witness_by_path.get(relative_path) else {
            continue;
        };
        let mut symbol_terms = symbol_projection_terms(&symbol.name);
        symbol_terms.extend(symbol_kind_projection_terms(symbol, witness_projection));
        symbol_terms.sort();
        symbol_terms.dedup();
        if symbol_terms.is_empty() {
            continue;
        }

        if let Some(projection) =
            find_path_surface_term_projection_mut(path_surface_terms, relative_path)
        {
            for term in &symbol_terms {
                super::builders::push_weighted_term(&mut projection.term_weights, term, 2);
            }
            projection.exact_terms.extend(symbol_terms.iter().cloned());
            if !symbol.kind.as_str().is_empty() {
                super::builders::push_weighted_term(
                    &mut projection.term_weights,
                    symbol.kind.as_str(),
                    1,
                );
                projection.exact_terms.push(symbol.kind.as_str().to_owned());
            }
            input_modes
                .path_surface_term
                .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
        }

        anchor_candidates
            .entry(relative_path.clone())
            .or_default()
            .push(PathAnchorSketchProjection {
                path: relative_path.clone(),
                anchor_rank: 0,
                line: symbol.line.max(1),
                anchor_kind: ast_anchor_kind(symbol, witness_projection).to_owned(),
                excerpt: super::builders::trim_excerpt(&format!(
                    "{} {}",
                    symbol.kind.as_str(),
                    symbol.name
                )),
                terms: symbol_terms.clone(),
                score_hint: 48,
            });
        input_modes
            .path_anchor_sketch
            .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
    }

    relative_symbols.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.line.cmp(&right.1.line))
            .then(left.1.name.cmp(&right.1.name))
            .then(left.1.stable_id.cmp(&right.1.stable_id))
    });
    let mut symbols_by_name = BTreeMap::<String, Vec<(String, SymbolDefinition)>>::new();
    for (relative_path, symbol) in relative_symbols {
        let normalized_name = symbol.name.trim().to_ascii_lowercase();
        if normalized_name.is_empty() {
            continue;
        }
        symbols_by_name
            .entry(normalized_name)
            .or_default()
            .push((relative_path, symbol));
    }

    for symbols in symbols_by_name.into_values() {
        if symbols.len() < 2 {
            continue;
        }
        let shared_terms = symbol_projection_terms(&symbols[0].1.name);
        for (relative_path, symbol) in &symbols {
            let Some(src_projection) = witness_by_path.get(relative_path) else {
                continue;
            };
            let Some(src_subtree_root) = src_projection.subtree_root.as_deref() else {
                continue;
            };
            let mut per_source = Vec::new();
            for (candidate_path, candidate_symbol) in &symbols {
                if candidate_path == relative_path {
                    continue;
                }
                let Some(dst_projection) = witness_by_path.get(candidate_path) else {
                    continue;
                };
                if dst_projection.subtree_root.as_deref() != Some(src_subtree_root) {
                    continue;
                }
                per_source.push(PathRelationProjection {
                    src_path: relative_path.clone(),
                    dst_path: candidate_path.clone(),
                    relation_kind: "symbol_overlap".to_owned(),
                    evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned(),
                    src_symbol_id: Some(symbol.stable_id.clone()),
                    dst_symbol_id: Some(candidate_symbol.stable_id.clone()),
                    src_family_bits: family_bits_for_projection(src_projection),
                    dst_family_bits: family_bits_for_projection(dst_projection),
                    shared_terms: shared_terms.clone(),
                    score_hint: 96 + usize::from(symbol.language == candidate_symbol.language) * 8,
                });
            }
            per_source.sort_by(|left, right| {
                right
                    .score_hint
                    .cmp(&left.score_hint)
                    .then(left.dst_path.cmp(&right.dst_path))
                    .then(left.dst_symbol_id.cmp(&right.dst_symbol_id))
            });
            per_source.truncate(8);
            if !per_source.is_empty() {
                path_relations.extend(per_source);
                input_modes
                    .path_relation
                    .insert(RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned());
            }
        }
    }

    *path_anchor_sketches = anchor_candidates.into_values().flatten().collect();
}

fn push_ast_relation_projection(
    path_relations: &mut Vec<PathRelationProjection>,
    symbols: &[SymbolDefinition],
    witness_by_path: &BTreeMap<String, &StoredPathWitnessProjection>,
    relative_path_by_stable_id: &BTreeMap<String, String>,
    source_symbol_index: usize,
    target_symbol_index: usize,
    relation: RelationKind,
) {
    let Some(source_symbol) = symbols.get(source_symbol_index) else {
        return;
    };
    let Some(target_symbol) = symbols.get(target_symbol_index) else {
        return;
    };
    let Some(src_path) = relative_path_by_stable_id
        .get(&source_symbol.stable_id)
        .cloned()
    else {
        return;
    };
    let Some(dst_path) = relative_path_by_stable_id
        .get(&target_symbol.stable_id)
        .cloned()
    else {
        return;
    };
    if src_path == dst_path {
        return;
    }
    let Some(src_projection) = witness_by_path.get(&src_path) else {
        return;
    };
    let Some(dst_projection) = witness_by_path.get(&dst_path) else {
        return;
    };

    path_relations.push(PathRelationProjection {
        src_path,
        dst_path,
        relation_kind: relation.as_str().to_owned(),
        evidence_source: RETRIEVAL_PROJECTION_INPUT_MODE_AST.to_owned(),
        src_symbol_id: Some(source_symbol.stable_id.clone()),
        dst_symbol_id: Some(target_symbol.stable_id.clone()),
        src_family_bits: family_bits_for_projection(src_projection),
        dst_family_bits: family_bits_for_projection(dst_projection),
        shared_terms: symbol_projection_terms(&target_symbol.name),
        score_hint: ast_relation_score_hint(relation),
    });
}

fn ast_relation_score_hint(relation: RelationKind) -> usize {
    match relation {
        RelationKind::Calls => 116,
        RelationKind::RefersTo => 112,
        RelationKind::Implements | RelationKind::Extends => 108,
        RelationKind::Contains => 104,
        RelationKind::DefinedIn => 100,
    }
}

fn symbol_projection_terms(name: &str) -> Vec<String> {
    super::super::query_terms::hybrid_query_exact_terms(name)
}

fn symbol_kind_projection_terms(
    symbol: &SymbolDefinition,
    witness_projection: &StoredPathWitnessProjection,
) -> Vec<String> {
    let mut terms = Vec::new();
    match symbol.kind {
        SymbolKind::Module => terms.push("module".to_owned()),
        SymbolKind::Component | SymbolKind::Section | SymbolKind::Slot => {
            terms.push("component".to_owned());
        }
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::EnumCase
        | SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::PhpEnum
        | SymbolKind::TypeAlias => {
            terms.push("type".to_owned());
        }
        SymbolKind::Trait | SymbolKind::PhpTrait => {
            terms.push("trait".to_owned());
            terms.push("interface".to_owned());
        }
        SymbolKind::Impl => {
            terms.push("impl".to_owned());
            terms.push("implementation".to_owned());
        }
        SymbolKind::Function => terms.push("function".to_owned()),
        SymbolKind::Method => {
            terms.push("function".to_owned());
            terms.push("method".to_owned());
        }
        SymbolKind::Property => {
            terms.push("property".to_owned());
            terms.push("field".to_owned());
        }
        SymbolKind::Const | SymbolKind::Static | SymbolKind::Constant => {
            terms.push("constant".to_owned());
        }
    }

    if symbol.name.eq_ignore_ascii_case("main") {
        terms.push("entrypoint".to_owned());
    }
    if witness_projection.flags.is_entrypoint_runtime {
        terms.push("runtime".to_owned());
    }
    if witness_projection.flags.is_test_support
        || witness_projection.flags.is_test_harness
        || symbol.name.to_ascii_lowercase().starts_with("test")
    {
        terms.push("test".to_owned());
    }

    terms.sort();
    terms.dedup();
    terms
}

fn ast_anchor_kind(
    symbol: &SymbolDefinition,
    witness_projection: &StoredPathWitnessProjection,
) -> &'static str {
    if symbol.name.eq_ignore_ascii_case("main") || witness_projection.flags.is_entrypoint_runtime {
        return "ast_entrypoint";
    }
    if witness_projection.flags.is_test_support || witness_projection.flags.is_test_harness {
        return "ast_test_symbol";
    }
    match symbol.kind {
        SymbolKind::Trait | SymbolKind::PhpTrait => "ast_trait",
        SymbolKind::Impl => "ast_impl",
        SymbolKind::Module => "ast_module",
        SymbolKind::Function => "ast_function",
        SymbolKind::Method => "ast_method",
        SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::TypeAlias
        | SymbolKind::PhpEnum => "ast_type",
        _ => "ast_symbol",
    }
}

fn find_path_surface_term_projection_mut<'a>(
    rows: &'a mut [PathSurfaceTermProjection],
    path: &str,
) -> Option<&'a mut PathSurfaceTermProjection> {
    rows.iter_mut().find(|projection| projection.path == path)
}
