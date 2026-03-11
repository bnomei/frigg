use std::collections::BTreeSet;
use std::path::Path;

use super::lexical_recall::normalize_hybrid_recall_token;

pub(super) fn hybrid_query_exact_terms(query_text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query_text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
            continue;
        }

        if let Some(token) = normalize_hybrid_recall_token(&current) {
            if seen.insert(token.clone()) {
                tokens.push(token);
            }
        }
        current.clear();
    }

    if let Some(token) = normalize_hybrid_recall_token(&current) {
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }

    tokens
}

pub(super) fn hybrid_specific_witness_query_terms(query_text: &str) -> Vec<String> {
    const GENERIC_WITNESS_TERMS: &[&str] = &[
        "action",
        "actions",
        "app",
        "application",
        "applications",
        "blade",
        "build",
        "component",
        "components",
        "create",
        "creates",
        "entry",
        "entrypoint",
        "flow",
        "form",
        "forms",
        "layout",
        "layouts",
        "middleware",
        "modal",
        "modals",
        "page",
        "pages",
        "part",
        "parts",
        "provider",
        "providers",
        "render",
        "resource",
        "resources",
        "route",
        "routes",
        "script",
        "scripts",
        "section",
        "sections",
        "slot",
        "slots",
        "test",
        "tests",
        "ui",
        "view",
        "views",
        "wire",
        "wiring",
    ];

    hybrid_query_exact_terms(query_text)
        .into_iter()
        .filter(|term| {
            !GENERIC_WITNESS_TERMS
                .iter()
                .any(|generic| generic == &term.as_str())
        })
        .collect()
}

pub(super) fn path_has_exact_query_term_match(path: &str, exact_terms: &[String]) -> bool {
    let Some(stem) = Path::new(path).file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let normalized_stem = stem.trim().to_ascii_lowercase();
    if normalized_stem.is_empty() {
        return false;
    }

    exact_terms.iter().any(|term| term == &normalized_stem)
}

pub(super) fn hybrid_query_overlap_terms(query_text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query_text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            current.push(ch);
            continue;
        }
        push_hybrid_query_overlap_terms(&mut current, &mut seen, &mut tokens);
    }
    push_hybrid_query_overlap_terms(&mut current, &mut seen, &mut tokens);

    tokens
}

pub(super) fn hybrid_overlap_count(candidate_terms: &[String], query_terms: &[String]) -> usize {
    candidate_terms
        .iter()
        .filter(|candidate_term| {
            query_terms
                .iter()
                .any(|query_term| hybrid_terms_overlap(candidate_term, query_term))
        })
        .count()
}

pub(super) fn hybrid_path_overlap_count(path: &str, query_text: &str) -> usize {
    let query_terms = hybrid_query_overlap_terms(query_text);
    hybrid_path_overlap_count_with_terms(path, &query_terms)
}

pub(super) fn hybrid_path_overlap_count_with_terms(path: &str, query_terms: &[String]) -> usize {
    let path_tokens = hybrid_path_overlap_tokens(path);
    if path_tokens.is_empty() {
        return 0;
    }

    if query_terms.is_empty() {
        return 0;
    }

    hybrid_overlap_count(&path_tokens, &query_terms)
}

pub(super) fn hybrid_identifier_tokens(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous_was_lowercase = false;

    for ch in raw.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            push_hybrid_identifier_token(&mut tokens, &mut current);
            previous_was_lowercase = false;
            continue;
        }
        if !ch.is_ascii_alphanumeric() {
            push_hybrid_identifier_token(&mut tokens, &mut current);
            previous_was_lowercase = false;
            continue;
        }
        if ch.is_ascii_uppercase() && previous_was_lowercase {
            push_hybrid_identifier_token(&mut tokens, &mut current);
        }
        current.push(ch.to_ascii_lowercase());
        previous_was_lowercase = ch.is_ascii_lowercase();
    }

    push_hybrid_identifier_token(&mut tokens, &mut current);
    tokens
}

pub(super) fn hybrid_excerpt_has_build_flow_anchor(excerpt: &str, query_terms: &[String]) -> bool {
    let normalized = excerpt.trim().to_ascii_lowercase();
    let overlap = hybrid_overlap_count(&hybrid_identifier_tokens(&normalized), query_terms);
    overlap >= 2
        && [
            "build_",
            "build(",
            "build ",
            "builder",
            "construct",
            "wire",
            "runner = build_",
            "fn build_",
            "let mut runner = build_",
        ]
        .iter()
        .any(|needle| normalized.contains(needle))
}

pub(super) fn hybrid_excerpt_has_exact_identifier_anchor(excerpt: &str, query_text: &str) -> bool {
    let normalized = excerpt.trim().to_ascii_lowercase();
    hybrid_query_exact_terms(query_text)
        .into_iter()
        .filter(|term| term.contains('_') || term.len() >= 16)
        .any(|term| normalized.contains(&term))
}

pub(super) fn hybrid_excerpt_has_test_double_anchor(excerpt: &str) -> bool {
    let normalized = excerpt.trim().to_ascii_lowercase();
    ["fake", "mock", "fixture", "#[test]", "contract"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn push_hybrid_query_overlap_terms(
    current: &mut String,
    seen: &mut BTreeSet<String>,
    tokens: &mut Vec<String>,
) {
    if current.is_empty() {
        return;
    }
    if let Some(token) = normalize_hybrid_recall_token(current) {
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }
    for token in hybrid_identifier_tokens(current) {
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }
    current.clear();
}

pub(super) fn hybrid_path_overlap_tokens(path: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut tokens = Vec::new();

    for raw_component in path.split('/') {
        let normalized_component = Path::new(raw_component)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(raw_component);
        if let Some(token) = normalize_hybrid_recall_token(normalized_component) {
            if !is_low_signal_path_overlap_token(&token) && seen.insert(token.clone()) {
                tokens.push(token);
            }
        }
        for token in hybrid_identifier_tokens(normalized_component) {
            if is_low_signal_path_overlap_token(&token) {
                continue;
            }
            if seen.insert(token.clone()) {
                tokens.push(token);
            }
        }
    }

    tokens
}

fn is_low_signal_path_overlap_token(token: &str) -> bool {
    matches!(
        token,
        "src"
            | "docs"
            | "doc"
            | "tests"
            | "test"
            | "crates"
            | "contracts"
            | "playbooks"
            | "specs"
            | "target"
            | "debug"
            | "vendor"
    )
}

fn push_hybrid_identifier_token(tokens: &mut Vec<String>, current: &mut String) {
    if let Some(token) = normalize_hybrid_recall_token(current) {
        tokens.push(token);
    }
    current.clear();
}

fn hybrid_terms_overlap(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }

    let normalize_plural = |term: &str| -> Option<String> {
        let normalized = term.trim().to_ascii_lowercase();
        if normalized.len() > 4 && normalized.ends_with("ies") {
            return Some(format!("{}y", &normalized[..normalized.len() - 3]));
        }
        if normalized.len() > 3 && normalized.ends_with('s') && !normalized.ends_with("ss") {
            return Some(normalized[..normalized.len() - 1].to_owned());
        }
        None
    };

    normalize_plural(left).as_deref() == Some(right)
        || normalize_plural(right).as_deref() == Some(left)
}
