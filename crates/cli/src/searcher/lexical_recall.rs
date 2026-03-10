use std::collections::BTreeSet;

use regex::escape;

use super::{HYBRID_LEXICAL_RECALL_MAX_TOKENS, HYBRID_LEXICAL_RECALL_MIN_TOKEN_LEN};

pub(super) fn build_hybrid_lexical_recall_regex(query_text: &str) -> Option<String> {
    let tokens = hybrid_lexical_recall_tokens(query_text);
    if tokens.len() < 2 {
        return None;
    }

    let token_pattern = tokens
        .into_iter()
        .take(HYBRID_LEXICAL_RECALL_MAX_TOKENS)
        .map(|token| escape(&token))
        .collect::<Vec<_>>()
        .join("|");
    if token_pattern.is_empty() {
        return None;
    }

    Some(format!(r"(?i)\b(?:{token_pattern})\b"))
}

pub(super) fn hybrid_lexical_recall_tokens(query_text: &str) -> Vec<String> {
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
            current.clear();
        } else {
            current.clear();
        }
    }
    if let Some(token) = normalize_hybrid_recall_token(&current) {
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }

    tokens
}

pub(super) fn normalize_hybrid_recall_token(token: &str) -> Option<String> {
    if token.len() < HYBRID_LEXICAL_RECALL_MIN_TOKEN_LEN {
        return None;
    }

    let token = token.trim().to_ascii_lowercase();
    if token.is_empty() || is_low_signal_hybrid_recall_token(&token) {
        return None;
    }

    Some(token)
}

fn is_low_signal_hybrid_recall_token(token: &str) -> bool {
    matches!(
        token,
        "about"
            | "does"
            | "from"
            | "frigg"
            | "into"
            | "that"
            | "these"
            | "this"
            | "those"
            | "turn"
            | "what"
            | "when"
            | "where"
            | "which"
    )
}
