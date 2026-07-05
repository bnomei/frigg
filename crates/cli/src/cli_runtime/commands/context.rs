//! CLI `context` command: compact summaries for local context-efficiency JSONL logs.
//!
//! Summarizes local context-efficiency JSONL logs for operator review without mutating workspace
//! index state.

use std::error::Error;

use chrono::{DateTime, Utc};
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
    let day_range = format_context_day_range(summary);
    format!(
        "{saved_percent} saved, {} {}, {day_range}",
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

fn format_context_day_range(summary: &ContextLogSummary) -> String {
    let Some(days) = context_day_range(summary) else {
        return "0 days".to_owned();
    };
    let day_label = if days == "1" { "day" } else { "days" };
    format!("{days} {day_label}")
}

fn context_day_range(summary: &ContextLogSummary) -> Option<String> {
    const MILLIS_PER_DAY: i64 = 86_400_000;

    let since = DateTime::parse_from_rfc3339(&summary.date_since).ok()?;
    let until = DateTime::parse_from_rfc3339(&summary.date_until).ok()?;
    let millis = until.signed_duration_since(since).num_milliseconds();
    if millis < 0 {
        return None;
    }
    if millis % MILLIS_PER_DAY == 0 {
        return Some((millis / MILLIS_PER_DAY).to_string());
    }

    Some(format_trimmed_decimal(
        millis as f64 / MILLIS_PER_DAY as f64,
    ))
}

fn format_trimmed_decimal(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let mut value = format!("{rounded:.2}");
    while value.contains('.') && value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    value
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
        std::fs::create_dir_all(root.join(".git")).expect("git marker should be created");
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
            "88.57% saved, 2 tool calls, 30 days"
        );
    }

    #[test]
    fn cli_context_command_formats_fractional_day_range() {
        let summary = ContextLogSummary {
            date_since: "2026-06-01T00:00:00+00:00".to_owned(),
            date_until: "2026-06-05T16:33:36+00:00".to_owned(),
            roots: Vec::new(),
            totals: ContextLogAggregate {
                events: 390,
                returned_unique_file_bytes: 400_000,
                returned_source_bytes_estimate: 9_800,
                matched_file_context_saved_bytes_estimate: 390_200,
                ..ContextLogAggregate::default()
            },
        };

        assert_eq!(
            format_context_summary_line(&summary),
            "97.55% saved, 390 tool calls, 4.69 days"
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
            "0% saved, 0 tool calls, 30 days"
        );
    }
}
