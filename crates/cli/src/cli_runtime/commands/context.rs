//! CLI `context` command: compact summaries for local context-efficiency JSONL logs.

use std::error::Error;

use chrono::Utc;
use frigg::context_efficiency::{ContextSummaryWindow, summarize_context_logs_for_roots};
use frigg::settings::FriggConfig;
use serde_json::to_string_pretty;

pub(crate) fn run_context_summary_command(
    config: &FriggConfig,
    since: Option<&str>,
    until: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let window = ContextSummaryWindow::resolve(since, until, Utc::now())?;
    let summary = summarize_context_logs_for_roots(&config.workspace_roots, &window)?;
    println!("{}", to_string_pretty(&summary)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_context_command_serializes_root_date_fields_without_window_object() {
        let root = std::env::temp_dir().join(format!(
            "frigg-cli-context-summary-shape-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".frigg")).expect("frigg dir should be created");
        std::fs::write(
            root.join(".frigg/context.jsonl"),
            r#"{"timestamp":"2026-06-10T00:00:00Z","tool":"read_file","repository_id":"repo-1","snapshot_id":"snapshot-1","indexed_readable_files":2,"indexed_readable_bytes":200,"returned_unique_paths":1,"returned_unique_file_bytes":100,"returned_source_bytes_estimate":12,"narrowing_ratio_estimate":16.666666666666668}"#,
        )
        .expect("context log should be written");
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])
            .expect("config should accept temp root");
        let window = ContextSummaryWindow::resolve(
            Some("2026-06-01"),
            Some("2026-07-01T00:00:00Z"),
            Utc::now(),
        )
        .expect("window should resolve");
        let summary = summarize_context_logs_for_roots(&config.workspace_roots, &window)
            .expect("summary should load");
        let json = to_string_pretty(&summary).expect("summary should serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");

        assert_eq!(value["date_since"], "2026-06-01T00:00:00+00:00");
        assert_eq!(value["date_until"], "2026-07-01T00:00:00+00:00");
        assert!(value.get("window").is_none());
        assert_eq!(value["totals"]["events"], 1);
        assert!(!json.contains("snapshot-1"));
        assert!(!json.contains("timestamp"));

        let _ = std::fs::remove_dir_all(root);
    }
}
