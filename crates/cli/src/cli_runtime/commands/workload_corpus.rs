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

fn sanitize_workload_corpus_value(value: &Value, remaining_depth: usize) -> Value {
    if remaining_depth == 0 {
        return Value::String("[truncated-depth]".to_owned());
    }

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(bounded_workload_corpus_text(text)),
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
                    Value::String(bounded_workload_corpus_text(&row.payload_json)),
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
