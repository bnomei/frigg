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

pub(super) fn normalize_runtime_anchor_test_stem(stem: &str) -> String {
    let normalized = stem.trim().to_ascii_lowercase();
    normalized
        .strip_prefix("test_")
        .or_else(|| normalized.strip_prefix("tests_"))
        .or_else(|| normalized.strip_suffix("_test"))
        .or_else(|| normalized.strip_suffix("_tests"))
        .unwrap_or(normalized.as_str())
        .to_owned()
}

fn push_hybrid_identifier_token(tokens: &mut Vec<String>, current: &mut String) {
    let normalized = current
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    if normalized.len() >= 2 {
        tokens.push(normalized);
    }
    current.clear();
}
