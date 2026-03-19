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
        "bench",
        "benches",
        "benchmark",
        "benchmarks",
        "build",
        "cli",
        "command",
        "commands",
        "component",
        "components",
        "create",
        "creates",
        "entry",
        "entrypoint",
        "example",
        "examples",
        "flow",
        "fixture",
        "fixtures",
        "form",
        "forms",
        "integration",
        "integrations",
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

    let mut seen = BTreeSet::new();
    let mut terms = Vec::new();
    let exact_terms = hybrid_query_exact_terms(query_text);
    let compound_overlap_terms = hybrid_query_overlap_terms(query_text);

    for term in exact_terms.into_iter().chain(
        compound_overlap_terms
            .into_iter()
            .filter(|term| term.contains('-')),
    ) {
        if GENERIC_WITNESS_TERMS
            .iter()
            .any(|generic| generic == &term.as_str())
        {
            continue;
        }
        if seen.insert(term.clone()) {
            terms.push(term);
        }
    }

    terms
}

pub(super) fn hybrid_query_has_kotlin_android_ui_terms(query_text: &str) -> bool {
    let exact_terms = hybrid_query_exact_terms(query_text);
    if exact_terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "ui" | "compose" | "screen" | "fragment" | "layout" | "viewmodel"
        )
    }) {
        return true;
    }

    let has_view = exact_terms.iter().any(|term| term == "view");
    let has_model = exact_terms.iter().any(|term| term == "model");
    has_view && has_model
}

pub(super) fn hybrid_query_mentions_cli_command(query_text: &str) -> bool {
    let mut current = String::new();

    for ch in query_text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
            continue;
        }

        if matches!(current.as_str(), "cli" | "cmd" | "command" | "commands") {
            return true;
        }
        current.clear();
    }

    matches!(current.as_str(), "cli" | "cmd" | "command" | "commands")
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

pub(super) fn hybrid_canonical_match_multiplier(path: &str, exact_terms: &[String]) -> f32 {
    const CANONICAL_SUFFIXES: &[&str] = &[
        "reference",
        "request",
        "response",
        "result",
        "results",
        "handler",
        "formatter",
    ];

    let Some(stem) = Path::new(path).file_stem().and_then(|stem| stem.to_str()) else {
        return 1.0;
    };
    let normalized_stem = stem.trim().to_ascii_lowercase();
    if normalized_stem.is_empty() || exact_terms.is_empty() {
        return 1.0;
    }
    if exact_terms
        .iter()
        .any(|term| term.eq_ignore_ascii_case(&normalized_stem))
    {
        return 1.65;
    }

    for term in exact_terms {
        if !normalized_stem.starts_with(term.as_str()) || normalized_stem == *term {
            continue;
        }
        let suffix = &normalized_stem[term.len()..];
        if CANONICAL_SUFFIXES
            .iter()
            .any(|candidate| candidate == &suffix)
        {
            return 0.78;
        }
    }

    1.0
}

pub(super) fn hybrid_path_has_exact_stem_match(path: &str, exact_terms: &[String]) -> bool {
    let Some(stem) = Path::new(path).file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let normalized_stem = stem.trim().to_ascii_lowercase();
    if normalized_stem.is_empty() {
        return false;
    }

    exact_terms
        .iter()
        .any(|term| term.eq_ignore_ascii_case(&normalized_stem))
}

pub(super) fn hybrid_query_overlap_terms(query_text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let normalized_query = query_text.trim().to_ascii_lowercase();

    for ch in query_text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            current.push(ch);
            continue;
        }
        push_hybrid_query_overlap_terms(&mut current, &mut seen, &mut tokens);
    }
    push_hybrid_query_overlap_terms(&mut current, &mut seen, &mut tokens);
    push_known_compound_query_terms(&normalized_query, &mut seen, &mut tokens);

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
    if let Some(token) = normalize_hybrid_overlap_token(current) {
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
        if let Some(token) = normalize_hybrid_overlap_token(normalized_component) {
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
    if let Some(token) = normalize_hybrid_overlap_token(current) {
        tokens.push(token);
    }
    current.clear();
}

fn normalize_hybrid_overlap_token(token: &str) -> Option<String> {
    let normalized = token.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "cmd" | "pkg") {
        return Some(normalized);
    }

    normalize_hybrid_recall_token(&normalized)
}

fn push_known_compound_query_terms(
    query_text: &str,
    seen: &mut BTreeSet<String>,
    tokens: &mut Vec<String>,
) {
    const COMPOUND_TERMS: &[(&str, &str)] = &[
        ("edge functions", "edge-functions"),
        ("js sdk", "js-sdk"),
        ("node cli", "node-cli"),
        ("python sdk", "python-sdk"),
        ("self hosted", "self-hosted"),
        ("task runner", "task-runner"),
        ("task runners", "task-runners"),
    ];

    for (phrase, token) in COMPOUND_TERMS {
        if query_text.contains(phrase) && seen.insert((*token).to_owned()) {
            tokens.push((*token).to_owned());
        }
    }
}

fn hybrid_terms_overlap(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }

    const TERM_ALIASES: &[(&str, &[&str])] = &[
        ("cmd", &["command", "commands"]),
        ("command", &["cmd"]),
        ("commands", &["cmd"]),
        ("pkg", &["package", "packages"]),
        ("package", &["pkg"]),
        ("packages", &["pkg"]),
    ];

    let aliases_overlap = |candidate: &str, query: &str| {
        TERM_ALIASES
            .iter()
            .find(|(term, _)| *term == candidate)
            .is_some_and(|(_, aliases)| aliases.iter().any(|alias| *alias == query))
    };
    if aliases_overlap(left, right) || aliases_overlap(right, left) {
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

#[cfg(test)]
mod tests {
    use super::{
        hybrid_path_overlap_count, hybrid_query_mentions_cli_command, hybrid_query_overlap_terms,
        hybrid_specific_witness_query_terms,
    };

    #[test]
    fn hybrid_path_overlap_counts_cmd_as_command_abbreviation() {
        assert_eq!(
            hybrid_path_overlap_count(
                "cmd/frpc/main.go",
                "entry point bootstrap server api main cli command",
            ),
            2
        );
    }

    #[test]
    fn hybrid_path_overlap_counts_pkg_as_package_abbreviation() {
        assert_eq!(
            hybrid_path_overlap_count(
                "pkg/config/source/aggregator.go",
                "tests packages internal library integration",
            ),
            1
        );
    }

    #[test]
    fn hybrid_query_mentions_cli_command_uses_token_matches_not_substrings() {
        assert!(hybrid_query_mentions_cli_command(
            "ruff analyze cli entrypoint"
        ));
        assert!(hybrid_query_mentions_cli_command(
            "entry point bootstrap cli command"
        ));
        assert!(hybrid_query_mentions_cli_command(
            "entry point bootstrap commands runner"
        ));
        assert!(!hybrid_query_mentions_cli_command(
            "entry point bootstrap client runtime"
        ));
    }

    #[test]
    fn hybrid_specific_witness_terms_treat_cli_context_as_generic() {
        assert_eq!(
            hybrid_specific_witness_query_terms("ruff analyze cli command entrypoint"),
            vec!["ruff".to_owned(), "analyze".to_owned()]
        );
    }

    #[test]
    fn hybrid_specific_witness_terms_preserve_known_compound_path_anchors() {
        let terms = hybrid_specific_witness_query_terms(
            "firecrawl js sdk client task runner self hosted edge functions",
        );

        assert!(terms.contains(&"js-sdk".to_owned()));
        assert!(terms.contains(&"task-runner".to_owned()));
        assert!(terms.contains(&"self-hosted".to_owned()));
        assert!(terms.contains(&"edge-functions".to_owned()));
    }

    #[test]
    fn hybrid_query_overlap_terms_preserve_known_compound_language_and_runner_terms() {
        let terms = hybrid_query_overlap_terms(
            "firecrawl js sdk client task runner self hosted edge functions",
        );

        assert!(terms.contains(&"js-sdk".to_owned()));
        assert!(terms.contains(&"task-runner".to_owned()));
        assert!(terms.contains(&"self-hosted".to_owned()));
        assert!(terms.contains(&"edge-functions".to_owned()));
    }
}
