#![allow(clippy::panic)]

use std::env;
use std::path::PathBuf;

use frigg::playbooks::{
    HybridWitnessRequirement, LoadedHybridPlaybookRegression, load_hybrid_playbook_regressions,
};
use frigg::searcher::{SearchFilters, SearchHybridQuery, TextSearcher};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};

const FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_ENABLED";
const FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_PROVIDER";
const FRIGG_SEMANTIC_RUNTIME_MODEL_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_MODEL";
const FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE";
const FRIGG_PLAYBOOK_ENFORCE_TARGETS_ENV: &str = "FRIGG_PLAYBOOK_ENFORCE_TARGETS";

#[derive(Debug)]
struct PlaybookProbeOutcome {
    file_name: String,
    playbook_id: String,
    semantic_status: String,
    matched_paths: Vec<String>,
    required_missing: Vec<String>,
    target_missing: Vec<String>,
}

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

fn missing_required_witness_groups(
    regression: &LoadedHybridPlaybookRegression,
    matched_paths: &[String],
    semantic_status_ok: bool,
) -> Vec<String> {
    regression
        .spec
        .witness_groups
        .iter()
        .filter(|group| {
            let required = match group.required_when {
                HybridWitnessRequirement::Always => true,
                HybridWitnessRequirement::SemanticOk => semantic_status_ok,
            };
            required
                && !group
                    .match_any
                    .iter()
                    .any(|path| matched_paths.iter().any(|candidate| candidate == path))
        })
        .map(|group| format!("{} -> {:?}", group.group_id, group.match_any))
        .collect::<Vec<_>>()
}

fn missing_target_witness_groups(
    regression: &LoadedHybridPlaybookRegression,
    matched_paths: &[String],
) -> Vec<String> {
    regression
        .spec
        .target_witness_groups
        .iter()
        .filter(|group| {
            !group
                .match_any
                .iter()
                .any(|path| matched_paths.iter().any(|candidate| candidate == path))
        })
        .map(|group| format!("{} -> {:?}", group.group_id, group.match_any))
        .collect::<Vec<_>>()
}

fn run_playbook_probe(
    searcher: &TextSearcher,
    regression: &LoadedHybridPlaybookRegression,
    enforce_targets: bool,
) -> PlaybookProbeOutcome {
    let output = searcher
        .search_hybrid_with_filters(
            SearchHybridQuery {
                query: regression.spec.query.clone(),
                limit: regression.spec.top_k,
                weights: Default::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
        )
        .unwrap_or_else(|err| {
            panic!(
                "hybrid playbook probe failed for {} ({}): {err}",
                regression.metadata.playbook_id,
                regression.path.display()
            )
        });
    let semantic_status = output.note.semantic_status.as_str().to_owned();
    let allowed_statuses = regression
        .spec
        .allowed_semantic_statuses
        .iter()
        .map(|status| status.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    assert!(
        allowed_statuses
            .iter()
            .any(|status| status == &semantic_status),
        "playbook {} returned unsupported semantic status '{}'; allowed={allowed_statuses:?}",
        regression.metadata.playbook_id,
        semantic_status
    );

    let matched_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.clone())
        .collect::<Vec<_>>();
    let semantic_status_ok = output.note.semantic_status.as_str() == "ok";

    PlaybookProbeOutcome {
        file_name: regression
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        playbook_id: regression.metadata.playbook_id.clone(),
        semantic_status,
        required_missing: missing_required_witness_groups(
            regression,
            &matched_paths,
            semantic_status_ok,
        ),
        target_missing: if enforce_targets && semantic_status_ok {
            missing_target_witness_groups(regression, &matched_paths)
        } else {
            Vec::new()
        },
        matched_paths,
    }
}

fn run_all_playbook_probes(enforce_targets: bool) -> Vec<PlaybookProbeOutcome> {
    let searcher = build_searcher();
    load_hybrid_playbook_regressions(&playbooks_root())
        .expect("playbook metadata should load")
        .into_iter()
        .map(|regression| run_playbook_probe(&searcher, &regression, enforce_targets))
        .collect::<Vec<_>>()
}

fn format_outcome(outcome: &PlaybookProbeOutcome) -> String {
    format!(
        "{} [{}] semantic_status={} hits={:?} required_missing={:?} target_missing={:?}",
        outcome.playbook_id,
        outcome.file_name,
        outcome.semantic_status,
        outcome.matched_paths,
        outcome.required_missing,
        outcome.target_missing
    )
}

#[test]
fn playbook_markdown_required_witnesses_hold() {
    let outcomes = run_all_playbook_probes(false);
    let required_failures = outcomes
        .iter()
        .filter(|outcome| !outcome.required_missing.is_empty())
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
        .filter(|outcome| !outcome.target_missing.is_empty())
        .map(format_outcome)
        .collect::<Vec<_>>();

    assert!(
        target_failures.is_empty(),
        "playbook hybrid target witness failures:\n{}",
        target_failures.join("\n")
    );
}
