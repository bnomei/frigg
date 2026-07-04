//! CLI `context` command: compact summaries for local context-efficiency JSONL logs.

use std::error::Error;

use chrono::Utc;
use frigg::context_efficiency::{
    ContextLogAggregate, ContextLogSummary, ContextSummaryWindow, summarize_context_logs_for_roots,
};
use frigg::settings::FriggConfig;
use serde_json::to_string_pretty;

use crate::cli_runtime::output::format_context_saved_percent;

/// Loads context-efficiency JSONL logs for configured roots and prints a compact summary window.
pub(crate) fn run_context_summary_command(
    config: &FriggConfig,
    since: Option<&str>,
    until: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn Error>> {
    let window = ContextSummaryWindow::resolve(since, until, Utc::now())?;
    let summary = summarize_context_logs_for_roots(&config.workspace_roots, &window)?;
    if json {
        println!("{}", to_string_pretty(&summary)?);
    } else {
        println!("{}", format_context_summary_line(&summary));
    }
    Ok(())
}

fn format_context_summary_line(summary: &ContextLogSummary) -> String {
    let saved_percent = estimated_saved_output_percent(&summary.totals).unwrap_or(0.0);
    let saved_percent =
        format_context_saved_percent(Some(saved_percent)).unwrap_or_else(|| "0%".to_owned());
    format!(
        "{saved_percent} saved, {} {}",
        summary.totals.events,
        tool_call_label(summary.totals.events)
    )
}

fn estimated_saved_output_percent(totals: &ContextLogAggregate) -> Option<f64> {
    let saved_bytes = totals.matched_file_context_saved_bytes_estimate as f64;
    let denominator = if totals.returned_unique_file_bytes > 0 {
        totals.returned_unique_file_bytes as f64
    } else {
        saved_bytes + totals.returned_source_bytes_estimate as f64
    };
    (denominator > 0.0).then_some(saved_bytes / denominator * 100.0)
}

fn tool_call_label(count: usize) -> &'static str {
    if count == 1 {
        "tool call"
    } else {
        "tool calls"
    }
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
            r#"{"timestamp":"2026-06-10T00:00:00Z","tool":"read_file","repository_id":"repo-1","snapshot_id":"snapshot-1","indexed_readable_files":2,"indexed_readable_bytes":200,"returned_unique_paths":1,"returned_unique_file_bytes":100,"returned_source_bytes_estimate":12,"matched_file_context_saved_bytes_estimate":88,"matched_file_context_saved_percent_estimate":88.0,"narrowing_ratio_estimate":8}"#,
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

    #[test]
    fn cli_context_command_formats_compact_saved_percent_and_tool_calls() {
        let summary = ContextLogSummary {
            date_since: "2026-06-01T00:00:00+00:00".to_owned(),
            date_until: "2026-07-01T00:00:00+00:00".to_owned(),
            roots: Vec::new(),
            totals: ContextLogAggregate {
                events: 2,
                returned_unique_file_bytes: 280,
                returned_source_bytes_estimate: 32,
                matched_file_context_saved_bytes_estimate: 248,
                ..ContextLogAggregate::default()
            },
        };

        assert_eq!(
            format_context_summary_line(&summary),
            "88.57% saved, 2 tool calls"
        );
    }

    #[test]
    fn cli_context_command_formats_zero_when_no_saved_byte_denominator_exists() {
        let summary = ContextLogSummary {
            date_since: "2026-06-01T00:00:00+00:00".to_owned(),
            date_until: "2026-07-01T00:00:00+00:00".to_owned(),
            roots: Vec::new(),
            totals: ContextLogAggregate::default(),
        };

        assert_eq!(
            format_context_summary_line(&summary),
            "0% saved, 0 tool calls"
        );
    }
}
