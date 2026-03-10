#![allow(clippy::panic)]

use std::env;
use std::path::PathBuf;

use frigg::playbooks::{HybridPlaybookProbeOutcome, run_hybrid_playbook_regressions};
use frigg::searcher::TextSearcher;
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};

const FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_ENABLED";
const FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_PROVIDER";
const FRIGG_SEMANTIC_RUNTIME_MODEL_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_MODEL";
const FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE";
const FRIGG_PLAYBOOK_ENFORCE_TARGETS_ENV: &str = "FRIGG_PLAYBOOK_ENFORCE_TARGETS";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize")
}

fn playbooks_root() -> PathBuf {
    repo_root().join("playbooks")
}

fn parse_bool_env(name: &str) -> bool {
    matches!(
        env::var(name)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn semantic_runtime_from_env() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: parse_bool_env(FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV),
        provider: env::var(FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV)
            .ok()
            .and_then(|value| value.parse::<SemanticRuntimeProvider>().ok()),
        model: env::var(FRIGG_SEMANTIC_RUNTIME_MODEL_ENV).ok(),
        strict_mode: parse_bool_env(FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV),
    }
}

fn build_searcher() -> TextSearcher {
    let mut config = FriggConfig::from_workspace_roots(vec![repo_root()])
        .expect("repo root should produce a valid FriggConfig");
    config.semantic_runtime = semantic_runtime_from_env();
    TextSearcher::new(config)
}

fn run_all_playbook_probes(enforce_targets: bool) -> Vec<HybridPlaybookProbeOutcome> {
    let searcher = build_searcher();
    run_hybrid_playbook_regressions(&searcher, &playbooks_root(), enforce_targets)
        .expect("playbook metadata should load and execute")
        .outcomes
}

fn format_outcome(outcome: &HybridPlaybookProbeOutcome) -> String {
    format!(
        "{} [{}] semantic_status={} hits={:?} required_missing={:?} target_missing={:?}",
        outcome.playbook_id,
        outcome.file_name,
        outcome.semantic_status,
        outcome.matched_paths,
        outcome.required_missing(),
        outcome.target_missing()
    )
}

#[test]
fn playbook_markdown_required_witnesses_hold() {
    let outcomes = run_all_playbook_probes(false);
    let required_failures = outcomes
        .iter()
        .filter(|outcome| !outcome.required_missing().is_empty())
        .map(format_outcome)
        .collect::<Vec<_>>();

    assert!(
        required_failures.is_empty(),
        "playbook hybrid required witness failures:\n{}",
        required_failures.join("\n")
    );
}

#[test]
fn playbook_markdown_target_witnesses_hold_when_requested() {
    if !parse_bool_env(FRIGG_PLAYBOOK_ENFORCE_TARGETS_ENV) {
        return;
    }

    let outcomes = run_all_playbook_probes(true);
    let target_failures = outcomes
        .iter()
        .filter(|outcome| !outcome.target_missing().is_empty())
        .map(format_outcome)
        .collect::<Vec<_>>();

    assert!(
        target_failures.is_empty(),
        "playbook hybrid target witness failures:\n{}",
        target_failures.join("\n")
    );
}
