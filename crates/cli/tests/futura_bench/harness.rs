//! Shared helpers for `frigg-futura-bench` (FUT-019).
//!
//! Exercises shipped `FriggMcpServer` tool handlers — not a reimplementation.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use frigg::mcp::types::{PUBLIC_TOOL_NAMES, WorkspaceParams};
use frigg::mcp::FriggMcpServer;
use frigg::settings::FriggConfig;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Surface tag required by Futura Phase 7 scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Dogfood,
    Synth,
    Lang,
}

impl Surface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dogfood => "dogfood",
            Self::Synth => "synth",
            Self::Lang => "lang",
        }
    }
}

/// One scenario outcome for machine-readable summaries.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioResult {
    pub name: String,
    pub surface: Surface,
    pub passed: bool,
    pub reason: String,
    pub duration_ms: u64,
}

/// Aggregated report printed as JSON lines (and optionally written to disk).
#[derive(Debug, Default, Serialize)]
pub struct BenchReport {
    pub results: Vec<ScenarioResult>,
}

impl BenchReport {
    pub fn record(
        &mut self,
        name: impl Into<String>,
        surface: Surface,
        started: Instant,
        outcome: Result<(), String>,
    ) {
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (passed, reason) = match outcome {
            Ok(()) => (true, "ok".to_owned()),
            Err(reason) => (false, reason),
        };
        let result = ScenarioResult {
            name: name.into(),
            surface,
            passed,
            reason,
            duration_ms,
        };
        let line = serde_json::to_string(&result).expect("scenario result should serialize");
        // Human + machine: one JSON object per scenario on stdout.
        println!("FUTURA_BENCH {line}");
        self.results.push(result);
    }

    pub fn summary_value(&self) -> serde_json::Value {
        let dogfood_pass = self.count_pass(Surface::Dogfood);
        let dogfood_total = self.count_total(Surface::Dogfood);
        let synth_pass = self.count_pass(Surface::Synth);
        let synth_total = self.count_total(Surface::Synth);
        let lang_pass = self.count_pass(Surface::Lang);
        let lang_total = self.count_total(Surface::Lang);
        let failed: Vec<&ScenarioResult> = self.results.iter().filter(|r| !r.passed).collect();
        serde_json::json!({
            "harness": "frigg-futura-bench",
            "total": self.results.len(),
            "passed": self.results.iter().filter(|r| r.passed).count(),
            "failed": failed.len(),
            "surfaces": {
                "dogfood": { "passed": dogfood_pass, "total": dogfood_total },
                "synth": { "passed": synth_pass, "total": synth_total },
                "lang": { "passed": lang_pass, "total": lang_total },
            },
            "failures": failed,
            "results": self.results,
        })
    }

    fn count_pass(&self, surface: Surface) -> usize {
        self.results
            .iter()
            .filter(|r| r.surface == surface && r.passed)
            .count()
    }

    fn count_total(&self, surface: Surface) -> usize {
        self.results.iter().filter(|r| r.surface == surface).count()
    }

    pub fn emit_and_assert(&self) {
        let summary = self.summary_value();
        let pretty =
            serde_json::to_string_pretty(&summary).expect("bench summary should serialize");
        println!("FUTURA_BENCH_SUMMARY\n{pretty}");

        if let Ok(path) = std::env::var("FUTURA_BENCH_OUT") {
            if let Some(parent) = Path::new(&path).parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(&path, format!("{pretty}\n"))
                .unwrap_or_else(|err| panic!("FUTURA_BENCH_OUT write failed ({path}): {err}"));
            println!("FUTURA_BENCH wrote summary to {path}");
        }

        let failures: Vec<String> = self
            .results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| format!("[{}] {}: {}", r.surface.as_str(), r.name, r.reason))
            .collect();
        assert!(
            failures.is_empty(),
            "frigg-futura-bench failures:\n{}",
            failures.join("\n")
        );
    }
}

/// Absolute path to `crates/cli/tests/fixtures`.
pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-futura-bench-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

pub fn cleanup_workspace_root(workspace_root: &Path) {
    let _ = fs::remove_dir_all(workspace_root);
}

/// Copy a fixture tree into a temp workspace and ensure a `.git` marker exists.
pub fn materialize_fixture_workspace(fixture_dir: &Path, label: &str) -> PathBuf {
    let root = temp_workspace_root(label);
    copy_dir_recursive(fixture_dir, &root).unwrap_or_else(|err| {
        panic!(
            "copy fixture {} -> {}: {err}",
            fixture_dir.display(),
            root.display()
        )
    });
    let git_dir = root.join(".git");
    if !git_dir.exists() {
        fs::create_dir_all(&git_dir).expect("create .git marker");
    }
    root
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_file() {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Build an MCP server for a workspace root and adopt it via `workspace(path=...)`.
pub async fn server_for_root(workspace_root: &Path) -> FriggMcpServer {
    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    // Keep bench lean: no full SCIP ingest required for scripted search scenarios.
    config.full_scip_ingest = false;
    let server = FriggMcpServer::new(config);
    adopt_workspace(&server, workspace_root).await;
    server
}

/// Always adopt the target under test (no sticky wrong default).
pub async fn adopt_workspace(server: &FriggMcpServer, workspace_root: &Path) {
    let path = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf())
        .display()
        .to_string();
    let response = server
        .workspace(Parameters(WorkspaceParams {
            path: Some(path),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: None,
        }))
        .await
        .unwrap_or_else(|err| panic!("workspace(path=...) should adopt fixture: {err}"));
    assert!(
        response.0.session_default || response.0.repository.is_some(),
        "workspace adopt should expose a session repository: {:?}",
        response.0
    );
}

/// Whether a public tool name is on the shipped surface (skip gracefully if missing).
pub fn public_tool_registered(name: &str) -> bool {
    PUBLIC_TOOL_NAMES.contains(&name)
}

pub fn require(cond: bool, msg: impl Into<String>) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(msg.into())
    }
}

/// Decode structured MCP tool results (`read_match` / `read_file` presentation wrappers).
pub fn structured_tool_result<T: DeserializeOwned>(result: CallToolResult) -> Result<T, String> {
    let structured = result
        .structured_content
        .ok_or_else(|| "expected structured_content in tool result".to_owned())?;
    serde_json::from_value(structured)
        .map_err(|err| format!("structured_content should deserialize: {err}"))
}
