use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::graph::RelationKind;
use crate::indexer::{
    SymbolDefinition, SymbolKind, line_column_for_offset, push_symbol_definition,
    source_span_from_offsets,
};

use super::registry::SymbolLanguage;

pub(crate) fn collect_symbols_from_source(
    path: &Path,
    source: &str,
    symbols: &mut Vec<SymbolDefinition>,
) {
    let whole_file_span = source_span_from_offsets(source, 0, source.len());
    let file_anchor = usize::from(!source.is_empty());
    let module_name = blade_view_name_for_path(path).unwrap_or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(strip_blade_suffix)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "blade".to_owned())
    });
    push_symbol_definition(
        symbols,
        SymbolLanguage::Blade,
        SymbolKind::Module,
        path,
        &module_name,
        whole_file_span.clone(),
    );

    if let Some(component_name) = blade_component_name_for_path(path) {
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Component,
            path,
            &component_name,
            source_span_from_offsets(source, file_anchor, file_anchor),
        );
    }

    for capture in blade_section_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Section,
            path,
            name.as_str().trim(),
            source_span_from_offsets(source, name.start(), name.end()),
        );
    }

    for capture in blade_livewire_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        let normalized = format!("livewire:{}", name.as_str().trim());
        let Some(span) = capture.get(0) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Component,
            path,
            &normalized,
            source_span_from_offsets(source, span.start(), span.end()),
        );
    }

    for capture in blade_tag_regex().captures_iter(source) {
        let Some(tag_name) = capture.get(1) else {
            continue;
        };
        let Some((kind, normalized_name)) = classify_tag_name(tag_name.as_str()) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            kind,
            path,
            &normalized_name,
            source_span_from_offsets(source, tag_name.start(), tag_name.end()),
        );
    }

    for capture in blade_named_slot_tag_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Slot,
            path,
            name.as_str().trim(),
            source_span_from_offsets(source, name.start(), name.end()),
        );
    }

    for capture in blade_slot_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Slot,
            path,
            name.as_str().trim(),
            source_span_from_offsets(source, name.start(), name.end()),
        );
    }

    collect_property_symbols(source, path, symbols, "@props");
    collect_property_symbols(source, path, symbols, "@aware");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BladeRelationKind {
    Extends,
    Include,
    Component,
    Yield,
    DynamicComponent,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BladeRelationEvidence {
    pub(crate) owner_symbol_id: Option<String>,
    pub(crate) kind: BladeRelationKind,
    pub(crate) target_name: String,
    pub(crate) target_symbol_kind: SymbolKind,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FluxComponentHint {
    pub(crate) props: Vec<String>,
    pub(crate) slots: Vec<String>,
    pub(crate) variant_values: Vec<String>,
    pub(crate) size_values: Vec<String>,
    pub(crate) local_overlay: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BladeSourceEvidence {
    pub(crate) relations: Vec<BladeRelationEvidence>,
    pub(crate) livewire_components: Vec<String>,
    pub(crate) wire_directives: Vec<String>,
    pub(crate) flux_components: Vec<String>,
    pub(crate) flux_hints: BTreeMap<String, FluxComponentHint>,
}

pub(crate) const FLUX_REGISTRY_VERSION: &str = "2026-03-08-mvp";

pub(crate) fn extract_source_evidence_from_source(
    source: &str,
    file_symbols: &[SymbolDefinition],
) -> BladeSourceEvidence {
    let owner_symbol_id = file_symbols
        .iter()
        .find(|symbol| {
            symbol.language == SymbolLanguage::Blade && symbol.kind == SymbolKind::Module
        })
        .map(|symbol| symbol.stable_id.clone());
    let mut evidence = BladeSourceEvidence::default();

    for capture in blade_view_relation_regex().captures_iter(source) {
        let Some(directive) = capture.name("directive").map(|value| value.as_str()) else {
            continue;
        };
        let Some(target) = capture.get(2).or_else(|| capture.get(3)) else {
            continue;
        };
        let target_name = target.as_str().trim();
        if target_name.is_empty() {
            continue;
        }
        let (kind, target_symbol_kind) = match directive {
            "extends" => (BladeRelationKind::Extends, SymbolKind::Module),
            "component" => (BladeRelationKind::Component, SymbolKind::Module),
            "yield" => (BladeRelationKind::Yield, SymbolKind::Section),
            _ => (BladeRelationKind::Include, SymbolKind::Module),
        };
        evidence.relations.push(BladeRelationEvidence {
            owner_symbol_id: owner_symbol_id.clone(),
            kind,
            target_name: target_name.to_owned(),
            target_symbol_kind,
            line: line_column_for_offset(source, target.start()).0,
        });
    }

    for capture in blade_dynamic_component_regex().captures_iter(source) {
        let Some(target) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        let target_name = normalize_dynamic_component_target(target.as_str());
        if target_name.is_empty() {
            continue;
        }
        evidence.relations.push(BladeRelationEvidence {
            owner_symbol_id: owner_symbol_id.clone(),
            kind: BladeRelationKind::DynamicComponent,
            target_name,
            target_symbol_kind: SymbolKind::Component,
            line: line_column_for_offset(source, target.start()).0,
        });
    }

    for capture in blade_tag_regex().captures_iter(source) {
        let Some(tag_name) = capture.get(1) else {
            continue;
        };
        let normalized = tag_name.as_str().trim();
        if let Some(component_name) = normalized.strip_prefix("livewire:") {
            insert_sorted_unique_owned(
                &mut evidence.livewire_components,
                component_name.to_owned(),
            );
            continue;
        }
        if normalized.starts_with("flux:") {
            insert_sorted_unique_owned(&mut evidence.flux_components, normalized.to_owned());
            if let Some(hint) = flux_registry_hint(normalized) {
                evidence
                    .flux_hints
                    .entry(normalized.to_owned())
                    .or_insert(hint);
            }
            continue;
        }
        if let Some((SymbolKind::Component, component_name)) = classify_tag_name(normalized) {
            evidence.relations.push(BladeRelationEvidence {
                owner_symbol_id: owner_symbol_id.clone(),
                kind: BladeRelationKind::Component,
                target_name: component_name,
                target_symbol_kind: SymbolKind::Component,
                line: line_column_for_offset(source, tag_name.start()).0,
            });
        }
    }

    for capture in blade_livewire_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        insert_sorted_unique_owned(
            &mut evidence.livewire_components,
            name.as_str().trim().to_owned(),
        );
    }

    for capture in blade_wire_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1) else {
            continue;
        };
        insert_sorted_unique_owned(
            &mut evidence.wire_directives,
            name.as_str().trim().to_owned(),
        );
    }

    normalize_source_evidence(&mut evidence);
    evidence
}

pub(crate) fn mark_local_flux_overlays(
    evidence: &mut BladeSourceEvidence,
    symbols: &[SymbolDefinition],
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
) {
    for component_name in &evidence.flux_components {
        let local_component_name = component_name.replacen("flux:", "flux.", 1);
        let local_overlay = symbol_indices_by_name
            .get(&local_component_name)
            .into_iter()
            .flatten()
            .any(|index| {
                let symbol = &symbols[*index];
                symbol.language == SymbolLanguage::Blade && symbol.kind == SymbolKind::Component
            });
        if !local_overlay {
            continue;
        }
        evidence
            .flux_hints
            .entry(component_name.clone())
            .or_default()
            .local_overlay = true;
    }
}

pub(crate) fn resolve_relation_evidence_edges(
    symbols: &[SymbolDefinition],
    symbol_index_by_stable_id: &BTreeMap<String, usize>,
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_name: &BTreeMap<String, Vec<usize>>,
    evidence: &BladeSourceEvidence,
) -> Vec<(usize, usize, RelationKind)> {
    let mut edges = Vec::new();
    for relation in &evidence.relations {
        let Some(source_symbol_id) = relation.owner_symbol_id.as_ref() else {
            continue;
        };
        let Some(source_symbol_index) = symbol_index_by_stable_id.get(source_symbol_id).copied()
        else {
            continue;
        };
        let Some(target_symbol_index) = resolve_relation_target_symbol_index(
            symbols,
            symbol_indices_by_name,
            symbol_indices_by_lower_name,
            relation,
        ) else {
            continue;
        };
        if source_symbol_index == target_symbol_index {
            continue;
        }
        edges.push((
            source_symbol_index,
            target_symbol_index,
            RelationKind::RefersTo,
        ));
    }
    edges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    edges.dedup();
    edges
}

pub(crate) fn is_blade_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| file_name.ends_with(".blade.php"))
}

pub(crate) fn blade_view_name_for_path(path: &Path) -> Option<String> {
    let normalized = normalize_path_components(path);
    let blade_index = normalized
        .iter()
        .position(|component| component == "views")?;
    let tail = normalized.get(blade_index + 1..)?;
    let segments = tail
        .iter()
        .map(|segment| strip_blade_suffix(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("."))
}

pub(crate) fn blade_component_name_for_path(path: &Path) -> Option<String> {
    let normalized = normalize_path_components(path);
    let components_index = normalized
        .iter()
        .position(|component| component == "components")?;
    let tail = normalized.get(components_index + 1..)?;
    let segments = tail
        .iter()
        .map(|segment| strip_blade_suffix(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("."))
}

pub(crate) fn classify_tag_name(raw_tag_name: &str) -> Option<(SymbolKind, String)> {
    let normalized = raw_tag_name.trim();
    if normalized.is_empty() {
        return None;
    }
    if let Some(slot_name) = normalized
        .strip_prefix("x-slot:")
        .or_else(|| normalized.strip_prefix("x-slot."))
    {
        let slot_name = slot_name.trim();
        return (!slot_name.is_empty()).then(|| (SymbolKind::Slot, slot_name.to_owned()));
    }
    if let Some(component_name) = normalized.strip_prefix("x-") {
        if component_name == "dynamic-component" {
            return None;
        }
        let component_name = component_name.trim();
        return (!component_name.is_empty())
            .then(|| (SymbolKind::Component, component_name.to_owned()));
    }
    if normalized.starts_with("livewire:") || normalized.starts_with("flux:") {
        return Some((SymbolKind::Component, normalized.to_owned()));
    }
    None
}

fn collect_property_symbols(
    source: &str,
    path: &Path,
    symbols: &mut Vec<SymbolDefinition>,
    directive: &str,
) {
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = source[cursor..].find(directive) {
        let start = cursor + relative;
        let mut offset = start + directive.len();
        while offset < bytes.len() && bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        if offset >= bytes.len() || bytes[offset] != b'(' {
            cursor = start + directive.len();
            continue;
        }
        let Some((parameter_start, parameter_end)) = directive_parameter_bounds(source, offset)
        else {
            cursor = start + directive.len();
            continue;
        };
        let parameter_source = &source[parameter_start..parameter_end];
        for capture in blade_property_key_regex().captures_iter(parameter_source) {
            let Some(name_match) = capture.get(1).or_else(|| capture.get(2)) else {
                continue;
            };
            let normalized_name = format!("${}", name_match.as_str().trim());
            push_symbol_definition(
                symbols,
                SymbolLanguage::Blade,
                SymbolKind::Property,
                path,
                &normalized_name,
                source_span_from_offsets(
                    source,
                    parameter_start + name_match.start(),
                    parameter_start + name_match.end(),
                ),
            );
        }
        cursor = parameter_end.saturating_add(1);
    }
}

fn resolve_relation_target_symbol_index(
    symbols: &[SymbolDefinition],
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_name: &BTreeMap<String, Vec<usize>>,
    relation: &BladeRelationEvidence,
) -> Option<usize> {
    if let Some(indices) = symbol_indices_by_name.get(&relation.target_name) {
        let matches = indices
            .iter()
            .copied()
            .filter(|index| symbols[*index].kind == relation.target_symbol_kind)
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            return matches.first().copied();
        }
        if !matches.is_empty() {
            return None;
        }
    }

    let matches = symbol_indices_by_lower_name
        .get(&relation.target_name.to_ascii_lowercase())
        .into_iter()
        .flatten()
        .copied()
        .filter(|index| symbols[*index].kind == relation.target_symbol_kind)
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.first().copied()
    } else {
        None
    }
}

fn normalize_source_evidence(evidence: &mut BladeSourceEvidence) {
    evidence.relations.sort();
    evidence.relations.dedup();
    evidence.livewire_components.sort();
    evidence.livewire_components.dedup();
    evidence.wire_directives.sort();
    evidence.wire_directives.dedup();
    evidence.flux_components.sort();
    evidence.flux_components.dedup();
    for hint in evidence.flux_hints.values_mut() {
        hint.props.sort();
        hint.props.dedup();
        hint.slots.sort();
        hint.slots.dedup();
        hint.variant_values.sort();
        hint.variant_values.dedup();
        hint.size_values.sort();
        hint.size_values.dedup();
    }
}

fn insert_sorted_unique_owned(values: &mut Vec<String>, value: String) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(index) => values.insert(index, value),
    }
}

fn flux_registry_hint(component_name: &str) -> Option<FluxComponentHint> {
    match component_name {
        "flux:button" => Some(FluxComponentHint {
            props: vec!["icon".to_owned(), "size".to_owned(), "variant".to_owned()],
            slots: vec!["default".to_owned()],
            variant_values: vec![
                "danger".to_owned(),
                "ghost".to_owned(),
                "primary".to_owned(),
                "subtle".to_owned(),
            ],
            size_values: vec!["sm".to_owned(), "base".to_owned(), "lg".to_owned()],
            local_overlay: false,
        }),
        "flux:input" => Some(FluxComponentHint {
            props: vec!["size".to_owned(), "type".to_owned()],
            slots: Vec::new(),
            variant_values: Vec::new(),
            size_values: vec!["sm".to_owned(), "base".to_owned(), "lg".to_owned()],
            local_overlay: false,
        }),
        "flux:modal" => Some(FluxComponentHint {
            props: vec!["name".to_owned(), "variant".to_owned()],
            slots: vec![
                "default".to_owned(),
                "footer".to_owned(),
                "heading".to_owned(),
            ],
            variant_values: vec!["danger".to_owned(), "default".to_owned()],
            size_values: Vec::new(),
            local_overlay: false,
        }),
        "flux:dropdown" => Some(FluxComponentHint {
            props: vec!["align".to_owned(), "position".to_owned()],
            slots: vec!["default".to_owned(), "trigger".to_owned()],
            variant_values: Vec::new(),
            size_values: Vec::new(),
            local_overlay: false,
        }),
        "flux:select" => Some(FluxComponentHint {
            props: vec!["multiple".to_owned(), "size".to_owned()],
            slots: vec!["default".to_owned()],
            variant_values: Vec::new(),
            size_values: vec!["sm".to_owned(), "base".to_owned(), "lg".to_owned()],
            local_overlay: false,
        }),
        _ => None,
    }
}

fn normalize_path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
}

fn strip_blade_suffix(segment: &str) -> String {
    segment
        .strip_suffix(".blade.php")
        .or_else(|| segment.strip_suffix(".php"))
        .unwrap_or(segment)
        .to_owned()
}

fn directive_parameter_bounds(source: &str, open_paren_index: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(open_paren_index) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    for (offset, byte) in bytes[open_paren_index..].iter().copied().enumerate() {
        let index = open_paren_index + offset;
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if in_single_quote || in_double_quote => escaped = true,
            b'\'' if !in_double_quote => in_single_quote = !in_single_quote,
            b'"' if !in_single_quote => in_double_quote = !in_double_quote,
            b'(' if !in_single_quote && !in_double_quote => depth += 1,
            b')' if !in_single_quote && !in_double_quote => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((open_paren_index + 1, index));
                }
            }
            _ => {}
        }
    }
    None
}

fn blade_section_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"@(?:section|yield)\s*\(\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade section regex must compile")
    })
}

fn blade_livewire_directive_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"@livewire\s*\(\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade livewire directive regex must compile")
    })
}

fn blade_view_relation_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"@(?P<directive>extends|component|include(?:If|When|Unless|First)?|yield)\s*\(\s*(?:"([^"]+)"|'([^']+)')"#,
        )
        .expect("blade view relation regex must compile")
    })
}

fn blade_dynamic_component_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"<\s*x-dynamic-component\b[^>]*\b(?::component|component)\s*=\s*(?:"([^"]+)"|'([^']+)')"#,
        )
        .expect("blade dynamic component regex must compile")
    })
}

fn blade_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"<\s*(x-[A-Za-z0-9_.:-]+|livewire:[A-Za-z0-9_.:-]+|flux:[A-Za-z0-9_.:-]+)(?:[\s>/])"#,
        )
        .expect("blade tag regex must compile")
    })
}

fn blade_named_slot_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"<\s*x-slot\b[^>]*\bname\s*=\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade named slot regex must compile")
    })
}

fn blade_slot_directive_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"@slot\s*\(\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade slot directive regex must compile")
    })
}

fn blade_wire_directive_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"\b(wire:[A-Za-z0-9_.-]+)\s*="#)
            .expect("blade wire directive regex must compile")
    })
}

fn normalize_dynamic_component_target(raw_target: &str) -> String {
    raw_target
        .trim()
        .trim_matches(['"', '\''])
        .trim()
        .to_owned()
}

fn blade_property_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?x)
            (?:^|[\[,])\s*"([^"]+)"(?:\s*=>)?
            |
            (?:^|[\[,])\s*'([^']+)'(?:\s*=>)?
        "#,
        )
        .expect("blade property key regex must compile")
    })
}
