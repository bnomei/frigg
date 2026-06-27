//! CLI `export-workload-corpus` command: deterministic sanitized provenance export for evaluation.

use std::error::Error;
use std::io;
use std::path::Path;

use frigg::settings::FriggConfig;
use frigg::storage::Storage;
use serde::Serialize;
use serde_json::{Map, Value, to_string_pretty};

use crate::WorkloadCorpusExportFormat;
use crate::cli_runtime::storage_paths::resolve_storage_db_path;

const WORKLOAD_CORPUS_MAX_STRING_CHARS: usize = 256;
const WORKLOAD_CORPUS_MAX_ARRAY_ITEMS: usize = 8;
const WORKLOAD_CORPUS_MAX_OBJECT_ENTRIES: usize = 16;
const WORKLOAD_CORPUS_MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, Serialize)]
struct WorkloadCorpusExportRow {
    trace_id: String,
    created_at: String,
    repository_id: String,
    tool_name: String,
    parameter_summary: Value,
    outcome_summary: Value,
    source_refs_summary: Value,
    source_ref_count: usize,
    normalized_workload: Option<Value>,
}

const WORKLOAD_CORPUS_REDACTED: &str = "[REDACTED]";

const WORKLOAD_CORPUS_SECRET_KEY_FRAGMENTS: &[&str] = &[
    "api_key",
    "apikey",
    "access_token",
    "auth_token",
    "authorization",
    "client_secret",
    "credential",
    "password",
    "passwd",
    "private_key",
    "secret",
    "session_key",
    "token",
];

const WORKLOAD_CORPUS_SECRET_TOKEN_PREFIXES: &[&str] = &[
    "sk-",
    "sk_",
    "rk-",
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxr-",
    "akia",
    "asia",
    "aiza",
    "ya29.",
    "eyj",
    "npm_",
    "shpat_",
    "shpss_",
];

const WORKLOAD_CORPUS_SECRET_INTRODUCERS: &[&str] = &[
    "bearer",
    "token",
    "authorization",
    "apikey",
    "password",
    "secret",
];

const WORKLOAD_CORPUS_STRUCTURAL_PUNCT: &[char] =
    &['"', '\'', ',', ';', '(', ')', '[', ']', '{', '}', '<', '>'];

fn bounded_workload_corpus_text(value: &str) -> String {
    if value.chars().count() <= WORKLOAD_CORPUS_MAX_STRING_CHARS {
        return value.to_owned();
    }

    let mut bounded = value
        .chars()
        .take(WORKLOAD_CORPUS_MAX_STRING_CHARS)
        .collect::<String>();
    bounded.push_str("...");
    bounded
}

fn sanitized_workload_corpus_text(value: &str) -> String {
    bounded_workload_corpus_text(&redact_workload_corpus_text(value))
}

fn workload_corpus_key_is_secret(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    WORKLOAD_CORPUS_SECRET_KEY_FRAGMENTS
        .iter()
        .any(|fragment| lowered.contains(fragment))
}

fn workload_corpus_token_is_secret_like(token: &str) -> bool {
    let lowered = token.to_ascii_lowercase();
    if WORKLOAD_CORPUS_SECRET_TOKEN_PREFIXES
        .iter()
        .any(|prefix| lowered.starts_with(prefix))
    {
        return true;
    }

    let len = token.chars().count();
    let is_token_charset = token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '-' | '_' | '.'));
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let has_alpha = token.chars().any(|ch| ch.is_ascii_alphabetic());
    len >= 32 && is_token_charset && has_digit && has_alpha
}

fn redact_workload_corpus_token_core(word: &str) -> String {
    let after_leading =
        word.trim_start_matches(|ch: char| WORKLOAD_CORPUS_STRUCTURAL_PUNCT.contains(&ch));
    let leading = &word[..word.len() - after_leading.len()];
    let core =
        after_leading.trim_end_matches(|ch: char| WORKLOAD_CORPUS_STRUCTURAL_PUNCT.contains(&ch));
    let trailing = &after_leading[core.len()..];
    if core.is_empty() {
        return word.to_owned();
    }
    format!("{leading}{WORKLOAD_CORPUS_REDACTED}{trailing}")
}

struct WorkloadCorpusWordRedaction {
    text: String,
    next_word_is_secret: bool,
}

fn redact_workload_corpus_word(
    word: &str,
    previous_word_introduces_secret: bool,
) -> WorkloadCorpusWordRedaction {
    let normalized = word
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    let is_introducer = WORKLOAD_CORPUS_SECRET_INTRODUCERS
        .iter()
        .any(|introducer| normalized == *introducer);

    if previous_word_introduces_secret {
        if is_introducer {
            return WorkloadCorpusWordRedaction {
                text: word.to_owned(),
                next_word_is_secret: true,
            };
        }
        return WorkloadCorpusWordRedaction {
            text: redact_workload_corpus_token_core(word),
            next_word_is_secret: false,
        };
    }

    if let Some(separator_index) = word.find(['=', ':']) {
        let key = &word[..separator_index];
        if workload_corpus_key_is_secret(key) {
            let prefix = &word[..=separator_index];
            let value = &word[separator_index + 1..];
            if value.is_empty() {
                return WorkloadCorpusWordRedaction {
                    text: word.to_owned(),
                    next_word_is_secret: true,
                };
            }
            return WorkloadCorpusWordRedaction {
                text: format!("{prefix}{}", redact_workload_corpus_token_core(value)),
                next_word_is_secret: false,
            };
        }
    }

    if workload_corpus_token_is_secret_like(word) {
        return WorkloadCorpusWordRedaction {
            text: redact_workload_corpus_token_core(word),
            next_word_is_secret: false,
        };
    }

    WorkloadCorpusWordRedaction {
        text: word.to_owned(),
        next_word_is_secret: is_introducer,
    }
}

fn redact_workload_corpus_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_word_introduces_secret = false;

    for chunk in value.split_inclusive(char::is_whitespace) {
        let (word, trailing) = match chunk.char_indices().next_back() {
            Some((index, ch)) if ch.is_whitespace() => (&chunk[..index], &chunk[index..]),
            _ => (chunk, ""),
        };

        if word.is_empty() {
            output.push_str(chunk);
            continue;
        }

        let redaction = redact_workload_corpus_word(word, previous_word_introduces_secret);
        output.push_str(&redaction.text);
        output.push_str(trailing);
        previous_word_introduces_secret = redaction.next_word_is_secret;
    }

    output
}

fn sanitize_workload_corpus_value(value: &Value, remaining_depth: usize) -> Value {
    if remaining_depth == 0 {
        return Value::String("[truncated-depth]".to_owned());
    }

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(sanitized_workload_corpus_text(text)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(WORKLOAD_CORPUS_MAX_ARRAY_ITEMS)
                .map(|item| sanitize_workload_corpus_value(item, remaining_depth - 1))
                .collect(),
        ),
        Value::Object(entries) => {
            let mut ordered_keys = entries.keys().cloned().collect::<Vec<_>>();
            ordered_keys.sort();

            let mut sanitized = Map::new();
            for key in ordered_keys
                .into_iter()
                .take(WORKLOAD_CORPUS_MAX_OBJECT_ENTRIES)
            {
                if let Some(entry_value) = entries.get(&key) {
                    if workload_corpus_key_is_secret(&key) {
                        sanitized.insert(key, Value::String(WORKLOAD_CORPUS_REDACTED.to_owned()));
                        continue;
                    }
                    sanitized.insert(
                        key,
                        sanitize_workload_corpus_value(entry_value, remaining_depth - 1),
                    );
                }
            }

            Value::Object(sanitized)
        }
    }
}

fn workload_corpus_summary_field(payload: &Value, key: &str) -> Value {
    payload
        .get(key)
        .map(|value| sanitize_workload_corpus_value(value, WORKLOAD_CORPUS_MAX_DEPTH))
        .unwrap_or(Value::Null)
}

pub(crate) fn run_workload_corpus_export_command(
    config: &FriggConfig,
    output_path: &Path,
    format: WorkloadCorpusExportFormat,
    limit: usize,
) -> Result<(), Box<dyn Error>> {
    if limit == 0 {
        return Err(Box::new(io::Error::other(
            "export-workload-corpus limit must be greater than zero",
        )));
    }

    let repositories = config.repositories();
    let mut rows = Vec::new();

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "export-workload-corpus summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = resolve_storage_db_path(root, "export-workload-corpus")?;
        let storage = Storage::new(&db_path);
        let repo_rows = storage
            .load_recent_provenance_events(limit)
            .map_err(|err| {
                io::Error::other(format!(
                    "export-workload-corpus failed for repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                ))
            })?;

        let exported_count = repo_rows.len();
        for row in repo_rows {
            let payload = serde_json::from_str::<Value>(&row.payload_json).unwrap_or_else(|_| {
                Value::Object(Map::from_iter([(
                    "payload_decode_error".to_owned(),
                    Value::String(sanitized_workload_corpus_text(&row.payload_json)),
                )]))
            });
            let repository_id = payload
                .get("target_repository_id")
                .and_then(|value| value.as_str())
                .unwrap_or(&repo.repository_id.0)
                .to_owned();
            let source_refs_summary = workload_corpus_summary_field(&payload, "source_refs");
            let source_ref_count = payload
                .get("source_refs")
                .and_then(|value| value.as_array())
                .map_or(0, Vec::len);

            rows.push(WorkloadCorpusExportRow {
                trace_id: row.trace_id,
                created_at: row.created_at,
                repository_id,
                tool_name: row.tool_name,
                parameter_summary: workload_corpus_summary_field(&payload, "params"),
                outcome_summary: workload_corpus_summary_field(&payload, "outcome"),
                source_refs_summary,
                source_ref_count,
                normalized_workload: payload
                    .get("normalized_workload")
                    .map(|value| sanitize_workload_corpus_value(value, WORKLOAD_CORPUS_MAX_DEPTH)),
            });
        }

        println!(
            "export-workload-corpus ok repository_id={} root={} db={} rows={}",
            repo.repository_id.0,
            root.display(),
            db_path.display(),
            exported_count
        );
    }

    rows.sort_by(|left, right| {
        left.repository_id
            .cmp(&right.repository_id)
            .then(left.created_at.cmp(&right.created_at))
            .then(left.trace_id.cmp(&right.trace_id))
            .then(left.tool_name.cmp(&right.tool_name))
    });

    let parent = output_path.parent().ok_or_else(|| {
        io::Error::other(format!(
            "export-workload-corpus output path has no parent: {}",
            output_path.display()
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    match format {
        WorkloadCorpusExportFormat::Json => {
            std::fs::write(output_path, to_string_pretty(&rows)?)?;
        }
        WorkloadCorpusExportFormat::Jsonl => {
            let mut encoded = String::new();
            for row in &rows {
                encoded.push_str(&serde_json::to_string(row)?);
                encoded.push('\n');
            }
            std::fs::write(output_path, encoded)?;
        }
    }

    println!(
        "export-workload-corpus summary status=ok repositories={} rows={} format={} output={} limit={}",
        repositories.len(),
        rows.len(),
        format.as_str(),
        output_path.display(),
        limit
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_secret_bearing_object_keys() {
        let value = json!({
            "api_key": "value-should-not-survive",
            "query": "find handler",
            "nested": { "Authorization": "Bearer abc" }
        });
        let sanitized = sanitize_workload_corpus_value(&value, WORKLOAD_CORPUS_MAX_DEPTH);
        assert_eq!(sanitized["api_key"], json!("[REDACTED]"));
        assert_eq!(sanitized["query"], json!("find handler"));
        assert_eq!(sanitized["nested"]["Authorization"], json!("[REDACTED]"));
    }

    #[test]
    fn redacts_secret_like_tokens_in_free_text_values() {
        let value = json!({ "query": "look for sk-ant-api03-abcDEF123456ghIJKL7890mnopQRSTuv" });
        let sanitized = sanitize_workload_corpus_value(&value, WORKLOAD_CORPUS_MAX_DEPTH);
        let query = sanitized["query"].as_str().expect("query stays a string");
        assert!(
            !query.contains("sk-ant-api03"),
            "secret-like token must be redacted: {query}"
        );
        assert!(
            query.contains("[REDACTED]"),
            "expected redaction marker: {query}"
        );
        assert!(
            query.contains("look for"),
            "non-secret words must survive: {query}"
        );
    }

    #[test]
    fn redacts_high_entropy_tokens_and_inline_assignments() {
        assert!(
            redact_workload_corpus_text("token=abc123DEF456ghi789JKL012mno345PQR")
                .contains("[REDACTED]")
        );
        assert!(
            !redact_workload_corpus_text("Authorization: Bearer abcDEF123456ghIJKL7890mnopq")
                .contains("abcDEF123456"),
            "bearer token must be redacted"
        );
    }

    #[test]
    fn preserves_ordinary_text_without_secrets() {
        let text = "search for the parseToken function in module";
        assert_eq!(redact_workload_corpus_text(text), text);
    }
}
