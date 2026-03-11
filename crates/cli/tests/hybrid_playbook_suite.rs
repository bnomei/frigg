#![allow(clippy::panic)]

use std::env;
use std::path::PathBuf;

use frigg::domain::{FriggError, FriggResult};
use frigg::playbooks::{
    HybridWitnessRequirement, LoadedHybridPlaybookRegression, load_hybrid_playbook_regressions,
};
use frigg::searcher::{
    HybridSemanticStatus, SearchHybridExecutionOutput, SearchHybridQuery, TextSearcher,
};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn playbooks_root() -> PathBuf {
    repo_root().join("playbooks")
}

fn parse_env_bool(name: &str) -> Option<bool> {
    env::var(name)
        .ok()
        .and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn semantic_runtime_from_env() -> FriggResult<SemanticRuntimeConfig> {
    let enabled = parse_env_bool("FRIGG_SEMANTIC_RUNTIME_ENABLED").unwrap_or(false);
    let provider = env::var("FRIGG_SEMANTIC_RUNTIME_PROVIDER")
        .ok()
        .map(|raw| {
            raw.parse::<SemanticRuntimeProvider>().map_err(|err| {
                FriggError::InvalidInput(format!(
                    "invalid FRIGG_SEMANTIC_RUNTIME_PROVIDER for hybrid playbook suite: {err}"
                ))
            })
        })
        .transpose()?;
    let model = env::var("FRIGG_SEMANTIC_RUNTIME_MODEL")
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty());
    let strict_mode = parse_env_bool("FRIGG_SEMANTIC_RUNTIME_STRICT_MODE").unwrap_or(false);

    Ok(SemanticRuntimeConfig {
        enabled,
        provider,
        model,
        strict_mode,
    })
}

fn build_searcher() -> FriggResult<TextSearcher> {
    let root = repo_root();
    let mut config = FriggConfig::from_workspace_roots(vec![root])?;
    config.semantic_runtime = semantic_runtime_from_env()?;
    config.validate()?;
    Ok(TextSearcher::new(config))
}

fn top_paths(output: &SearchHybridExecutionOutput) -> Vec<&str> {
    output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect()
}

fn semantic_status_allowed(allowed_statuses: &[String], semantic_status: &str) -> bool {
    let semantic_status = semantic_status.trim().to_ascii_lowercase();
    allowed_statuses
        .iter()
        .any(|status| status.trim().eq_ignore_ascii_case(&semantic_status))
        || (semantic_status == "unavailable"
            && allowed_statuses
                .iter()
                .any(|status| status.trim().eq_ignore_ascii_case("disabled")))
}

fn assert_playbook_regression(
    regression: &LoadedHybridPlaybookRegression,
    output: &SearchHybridExecutionOutput,
    enforce_targets: bool,
) {
    let semantic_status = output.note.semantic_status.as_str();
    assert!(
        semantic_status_allowed(&regression.spec.allowed_semantic_statuses, semantic_status),
        "playbook {} returned disallowed semantic status '{}' with reason {:?}",
        regression.metadata.playbook_id,
        semantic_status,
        output.note.semantic_reason
    );

    let matched_paths = top_paths(output);
    for group in &regression.spec.witness_groups {
        let required = match group.required_when {
            HybridWitnessRequirement::Always => true,
            HybridWitnessRequirement::SemanticOk => {
                output.note.semantic_status == HybridSemanticStatus::Ok
            }
        };
        if !required {
            continue;
        }

        assert!(
            group
                .match_any
                .iter()
                .any(|path| matched_paths.contains(&path.as_str())),
            "playbook {} missing witness group '{}' in top {} results; expected one of {:?}, got {:?}",
            regression.metadata.playbook_id,
            group.group_id,
            regression.spec.top_k,
            group.match_any,
            matched_paths
        );
    }

    if enforce_targets && output.note.semantic_status == HybridSemanticStatus::Ok {
        for group in &regression.spec.target_witness_groups {
            assert!(
                group
                    .match_any
                    .iter()
                    .any(|path| matched_paths.contains(&path.as_str())),
                "playbook {} missing target witness group '{}' in top {} results; expected one of {:?}, got {:?}",
                regression.metadata.playbook_id,
                group.group_id,
                regression.spec.top_k,
                group.match_any,
                matched_paths
            );
        }
    }
}

#[test]
fn hybrid_playbook_suite_executes_against_current_workspace() -> FriggResult<()> {
    let regressions = load_hybrid_playbook_regressions(&playbooks_root())?;
    let searcher = build_searcher()?;
    let enforce_targets = parse_env_bool("FRIGG_PLAYBOOK_ENFORCE_TARGETS").unwrap_or(false);

    for regression in regressions {
        let output = searcher.search_hybrid(SearchHybridQuery {
            query: regression.spec.query.clone(),
            limit: regression.spec.top_k,
            weights: Default::default(),
            semantic: Some(true),
        })?;
        assert_playbook_regression(&regression, &output, enforce_targets);
    }

    Ok(())
}
