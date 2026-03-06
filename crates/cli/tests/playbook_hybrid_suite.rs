#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use frigg::searcher::{SearchFilters, SearchHybridQuery, TextSearcher};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};
use serde::Deserialize;

const FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_ENABLED";
const FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_PROVIDER";
const FRIGG_SEMANTIC_RUNTIME_MODEL_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_MODEL";
const FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE";
const FRIGG_PLAYBOOK_ENFORCE_TARGETS_ENV: &str = "FRIGG_PLAYBOOK_ENFORCE_TARGETS";
const PLAYBOOK_CONTRACT_MARKER: &str = "<!-- frigg-playbook";
const PLAYBOOK_CONTRACT_END: &str = "-->";

#[derive(Debug, Deserialize)]
struct HybridMarkdownPlaybookContract {
    schema: String,
    playbook_id: String,
    query: String,
    top_k: usize,
    allowed_semantic_statuses: Vec<String>,
    required_witness_groups: Vec<PlaybookWitnessGroup>,
    #[serde(default)]
    target_witness_groups: Vec<PlaybookWitnessGroup>,
}

#[derive(Debug, Deserialize)]
struct PlaybookWitnessGroup {
    name: String,
    paths: Vec<String>,
}

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

fn markdown_playbook_paths() -> Vec<PathBuf> {
    let mut paths = [
        "http-auth-entrypoint-trace.md",
        "tool-surface-gating.md",
        "hybrid-search-context-retrieval.md",
        "implementation-fallback-navigation.md",
        "error-contract-alignment.md",
        "deep-search-replay-and-citations.md",
    ]
    .into_iter()
    .map(|name| playbooks_root().join(name))
    .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn load_contract(path: &Path) -> HybridMarkdownPlaybookContract {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read playbook markdown {}: {err}", path.display()));
    let mut scan_offset = 0usize;
    while let Some(start) = raw[scan_offset..].find(PLAYBOOK_CONTRACT_MARKER) {
        let marker_start = scan_offset + start;
        let after_marker = &raw[marker_start + PLAYBOOK_CONTRACT_MARKER.len()..];
        let end = after_marker.find(PLAYBOOK_CONTRACT_END).unwrap_or_else(|| {
            panic!(
                "playbook markdown {} is missing the contract terminator",
                path.display()
            )
        });
        let contract_json = after_marker[..end].trim();
        if let Ok(contract) = serde_json::from_str::<HybridMarkdownPlaybookContract>(contract_json)
        {
            assert_eq!(
                contract.schema,
                "frigg.playbook.hybrid.v1",
                "unexpected playbook contract schema in {}",
                path.display()
            );
            return contract;
        }
        scan_offset =
            marker_start + PLAYBOOK_CONTRACT_MARKER.len() + end + PLAYBOOK_CONTRACT_END.len();
    }

    panic!(
        "failed to find a compatible hybrid playbook contract in {}",
        path.display()
    );
}

fn parse_bool_env(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn semantic_runtime_from_env() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: parse_bool_env(FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV),
        provider: std::env::var(FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV)
            .ok()
            .and_then(|value| value.parse::<SemanticRuntimeProvider>().ok()),
        model: std::env::var(FRIGG_SEMANTIC_RUNTIME_MODEL_ENV).ok(),
        strict_mode: parse_bool_env(FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV),
    }
}

fn build_searcher() -> TextSearcher {
    let mut config = FriggConfig::from_workspace_roots(vec![repo_root()])
        .expect("repo root should produce a valid FriggConfig");
    config.semantic_runtime = semantic_runtime_from_env();
    TextSearcher::new(config)
}

fn missing_witness_groups(
    groups: &[PlaybookWitnessGroup],
    matched_paths: &[String],
) -> Vec<String> {
    groups
        .iter()
        .filter(|group| {
            !group
                .paths
                .iter()
                .any(|path| matched_paths.iter().any(|candidate| candidate == path))
        })
        .map(|group| format!("{} -> {:?}", group.name, group.paths))
        .collect::<Vec<_>>()
}

fn run_playbook_probe(
    searcher: &TextSearcher,
    path: &Path,
    contract: &HybridMarkdownPlaybookContract,
) -> PlaybookProbeOutcome {
    let output = searcher
        .search_hybrid_with_filters(
            SearchHybridQuery {
                query: contract.query.clone(),
                limit: contract.top_k,
                weights: Default::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
        )
        .unwrap_or_else(|err| {
            panic!(
                "hybrid playbook probe failed for {} ({}): {err}",
                contract.playbook_id,
                path.display()
            )
        });
    let semantic_status = output.note.semantic_status.as_str().to_owned();
    let allowed_statuses = contract
        .allowed_semantic_statuses
        .iter()
        .map(|status| status.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    assert!(
        allowed_statuses
            .iter()
            .any(|status| status == &semantic_status),
        "playbook {} returned unsupported semantic status '{}'; allowed={allowed_statuses:?}",
        contract.playbook_id,
        semantic_status
    );

    let matched_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.clone())
        .collect::<Vec<_>>();

    PlaybookProbeOutcome {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        playbook_id: contract.playbook_id.clone(),
        semantic_status,
        required_missing: missing_witness_groups(&contract.required_witness_groups, &matched_paths),
        target_missing: missing_witness_groups(&contract.target_witness_groups, &matched_paths),
        matched_paths,
    }
}

fn run_all_playbook_probes() -> Vec<PlaybookProbeOutcome> {
    let searcher = build_searcher();
    markdown_playbook_paths()
        .into_iter()
        .map(|path| {
            let contract = load_contract(&path);
            run_playbook_probe(&searcher, &path, &contract)
        })
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
    let outcomes = run_all_playbook_probes();
    let required_failures = outcomes
        .iter()
        .filter(|outcome| !outcome.required_missing.is_empty())
        .map(format_outcome)
        .collect::<Vec<_>>();
    let target_gaps = outcomes
        .iter()
        .filter(|outcome| !outcome.target_missing.is_empty())
        .map(format_outcome)
        .collect::<Vec<_>>();

    if !target_gaps.is_empty() {
        println!(
            "playbook hybrid target gaps (non-failing unless {FRIGG_PLAYBOOK_ENFORCE_TARGETS_ENV}=1):\n{}",
            target_gaps.join("\n")
        );
    }

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

    let outcomes = run_all_playbook_probes();
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
