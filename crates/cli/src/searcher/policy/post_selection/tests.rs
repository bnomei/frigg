use super::*;
use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, EvidenceDocumentRef};

fn make_ranked(path: &str, score: f32) -> HybridRankedEvidence {
    HybridRankedEvidence {
        document: EvidenceDocumentRef {
            repository_id: "repo".to_owned(),
            path: path.to_owned(),
            line: 1,
            column: 1,
        },
        anchor: EvidenceAnchor::new(EvidenceAnchorKind::PathWitness, 1, 1, 1, 1),
        excerpt: path.to_owned(),
        blended_score: score,
        lexical_score: score,
        graph_score: 0.0,
        semantic_score: 0.0,
        lexical_sources: Vec::new(),
        graph_sources: Vec::new(),
        semantic_sources: Vec::new(),
    }
}

fn make_witness(path: &str, score: f32) -> HybridChannelHit {
    HybridChannelHit {
        channel: EvidenceChannel::PathSurfaceWitness,
        document: EvidenceDocumentRef {
            repository_id: "repo".to_owned(),
            path: path.to_owned(),
            line: 1,
            column: 1,
        },
        anchor: EvidenceAnchor::new(EvidenceAnchorKind::PathWitness, 1, 1, 1, 1),
        raw_score: score,
        excerpt: path.to_owned(),
        provenance_ids: vec!["path_witness:test".to_owned()],
    }
}

fn apply_context(
    matches: Vec<HybridRankedEvidence>,
    candidate_pool: &[HybridRankedEvidence],
    witness_hits: &[HybridChannelHit],
    intent: &HybridRankingIntent,
    query_text: &str,
    limit: usize,
) -> Vec<HybridRankedEvidence> {
    let ctx = PostSelectionContext::new(intent, query_text, limit, candidate_pool, witness_hits);
    apply(matches, &ctx)
}

fn apply_context_with_trace(
    matches: Vec<HybridRankedEvidence>,
    candidate_pool: &[HybridRankedEvidence],
    witness_hits: &[HybridChannelHit],
    intent: &HybridRankingIntent,
    query_text: &str,
    limit: usize,
) -> (Vec<HybridRankedEvidence>, PostSelectionTrace) {
    let ctx = PostSelectionContext::new_with_trace(
        intent,
        query_text,
        limit,
        candidate_pool,
        witness_hits,
    );
    let final_matches = apply(matches, &ctx);
    let trace = ctx
        .trace_snapshot()
        .expect("trace capture should be enabled");

    (final_matches, trace)
}

fn test_rule_meta(rule_id: &'static str) -> PostSelectionRuleMeta {
    RULES
        .iter()
        .copied()
        .find(|rule| rule.id == rule_id)
        .map(PostSelectionRule::meta)
        .expect("post-selection rule should exist")
}

#[test]
fn post_selection_policy_runtime_config_recovers_specific_surface_and_root_manifest_without_exceeding_limit()
 {
    let matches = vec![
        make_ranked(".github/workflows/ci.yml", 0.90),
        make_ranked("tests/runtime_config_test.rs", 0.84),
    ];
    let witness_hits = vec![
        make_witness("src/server.ts", 0.88),
        make_witness("src/index.ts", 0.87),
        make_witness("package.json", 0.86),
    ];
    let intent = HybridRankingIntent::from_query("runtime config package.json server build");
    assert!(intent.wants_runtime_config_artifacts);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "package json server tsconfig",
        2,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert_eq!(final_matches.len(), 2);
    assert!(paths.contains(&"package.json"));
    assert!(paths.contains(&"src/server.ts"));
    assert!(!paths.contains(&"src/index.ts"));
}

#[test]
fn post_selection_policy_runtime_config_uses_candidate_pool_when_witness_hits_are_missing() {
    let matches = vec![
        make_ranked("src/main.rs", 0.96),
        make_ranked("tests/runtime_config_test.rs", 0.84),
    ];
    let candidate_pool = vec![
        make_ranked("src/lib.rs", 0.95),
        make_ranked("Cargo.toml", 0.94),
    ];
    let intent = HybridRankingIntent::from_query("entry point build flow config cargo");
    assert!(intent.wants_runtime_config_artifacts);

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "config cargo server",
        2,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.contains(&"src/lib.rs"));
    assert!(paths.contains(&"Cargo.toml"));
    assert!(!paths.contains(&"tests/runtime_config_test.rs"));
}

#[test]
fn post_selection_policy_entrypoint_build_flow_inserts_workflow_without_replacing_canonical_main_or_lib()
 {
    let matches = vec![
        make_ranked("src/main.rs", 0.96),
        make_ranked("src/runner.rs", 0.90),
        make_ranked("README.md", 0.70),
    ];
    let witness_hits = vec![make_witness(".github/workflows/release.yml", 0.92)];
    let intent = HybridRankingIntent::from_query("entrypoint build workflow release runner");
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "build workflow release main",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert_eq!(final_matches.len(), 3);
    assert!(paths.contains(&"src/main.rs"));
    assert!(paths.contains(&".github/workflows/release.yml"));
    assert!(!paths.contains(&"README.md"));
}

#[test]
fn post_selection_policy_entrypoint_build_flow_uses_candidate_pool_when_witness_hits_are_missing() {
    let matches = vec![
        make_ranked("src/main.rs", 0.96),
        make_ranked("src/lib.rs", 0.92),
        make_ranked("README.md", 0.70),
    ];
    let candidate_pool = vec![make_ranked(".github/workflows/build-docker.yml", 0.91)];
    let intent = HybridRankingIntent::from_query("entrypoint build workflow release runner");

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "build workflow release main",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.contains(&"src/main.rs"));
    assert!(paths.contains(&"src/lib.rs"));
    assert!(paths.contains(&".github/workflows/build-docker.yml"));
    assert!(!paths.contains(&"README.md"));
}

#[test]
fn post_selection_policy_entrypoint_build_flow_promotes_specific_build_workflow_over_generic_selected_workflow()
 {
    let matches = vec![
        make_ranked("tools/cargo-run/src/main.rs", 0.99),
        make_ranked("desktop/platform/linux/src/main.rs", 0.98),
        make_ranked(".github/workflows/comment-!build-commands.yml", 0.97),
        make_ranked("frontend/src/main.ts", 0.96),
    ];
    let candidate_pool = vec![
        make_ranked(".github/workflows/build-linux-bundle.yml", 0.91),
        make_ranked(".github/workflows/build-mac-bundle.yml", 0.90),
    ];
    let intent = HybridRankingIntent::from_query(
        "entry point bootstrap build flow command runner main config cargo github workflow build linux build mac",
    );
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "entry point bootstrap build flow command runner main config cargo github workflow build linux build mac",
        4,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&".github/workflows/build-linux-bundle.yml"),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&".github/workflows/comment-!build-commands.yml"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_entrypoint_queries_recover_root_runtime_manifest() {
    let matches = vec![
        make_ranked("backend/app.py", 0.96),
        make_ranked("backend/cli.py", 0.92),
        make_ranked("README.md", 0.78),
    ];
    let witness_hits = vec![make_witness("backend/pyproject.toml", 0.86)];
    let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
    assert!(intent.wants_entrypoint_build_flow);
    assert!(surfaces::is_runtime_config_artifact_path(
        "backend/pyproject.toml"
    ));
    assert!(is_runtime_config_guardrail_replacement(&make_ranked(
        "README.md",
        0.78,
    )));

    let ctx = PostSelectionContext::new(
        &intent,
        "entry point bootstrap app startup cli main",
        3,
        &[],
        &witness_hits,
    );
    let final_matches = apply_runtime_config_surface_selection(
        matches,
        &ctx,
        test_rule_meta("post_selection.runtime_config"),
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"backend/pyproject.toml"),
        "final paths: {paths:?}"
    );
    assert!(!paths.contains(&"README.md"), "final paths: {paths:?}");
}

#[test]
fn post_selection_policy_entrypoint_queries_recover_android_runtime_entrypoint_under_resource_crowding()
 {
    let matches = vec![
        make_ranked(".github/workflows/build_test.yaml", 0.99),
        make_ranked("app/src/main/AndroidManifest.xml", 0.98),
        make_ranked("app/src/main/res/drawable/logo_no_fill.png", 0.97),
    ];
    let witness_hits = vec![
        make_witness(
            "app/src/main/java/com/example/android/todoapp/TodoActivity.kt",
            0.90,
        ),
        make_witness(
            "app/src/main/java/com/example/android/todoapp/TodoApplication.kt",
            0.88,
        ),
    ];
    let intent =
        HybridRankingIntent::from_query("entry point bootstrap app activity navigation main cli");
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point bootstrap app activity navigation main cli",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.iter().any(|path| {
            matches!(
                *path,
                "app/src/main/java/com/example/android/todoapp/TodoActivity.kt"
                    | "app/src/main/java/com/example/android/todoapp/TodoApplication.kt"
            )
        }),
        "final paths: {paths:?}"
    );
    assert!(paths.contains(&".github/workflows/build_test.yaml"));
    assert!(paths.contains(&"app/src/main/AndroidManifest.xml"));
    assert!(!paths.contains(&"app/src/main/res/drawable/logo_no_fill.png"));
}

#[test]
fn post_selection_policy_generic_android_test_queries_do_not_inject_unmatched_ui_companions() {
    let matches = vec![
        make_ranked(
            "app/src/test/java/com/example/android/todoapp/data/source/local/TaskDaoTest.kt",
            0.99,
        ),
        make_ranked(
            "app/src/androidTest/java/com/example/android/todoapp/tasks/TasksTest.kt",
            0.97,
        ),
        make_ranked("README.md", 0.60),
    ];
    let candidate_pool = vec![
        make_ranked(
            "app/src/main/java/com/example/android/todoapp/addedittask/AddEditTaskScreen.kt",
            0.96,
        ),
        make_ranked(
            "app/src/main/java/com/example/android/todoapp/statistics/StatisticsScreen.kt",
            0.95,
        ),
    ];
    let intent = HybridRankingIntent::from_query("tests fixtures integration dao");
    assert!(intent.wants_test_witness_recall);

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "tests fixtures integration dao",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        !paths.iter().any(|path| {
            matches!(
                *path,
                "app/src/main/java/com/example/android/todoapp/addedittask/AddEditTaskScreen.kt"
                    | "app/src/main/java/com/example/android/todoapp/statistics/StatisticsScreen.kt"
            )
        }),
        "generic Android test queries should not inject unrelated UI companions: {paths:?}"
    );
}

#[test]
fn post_selection_policy_entrypoint_queries_prefer_repo_root_config_over_nested_template_config() {
    let matches = vec![
        make_ranked("packages/@n8n/node-cli/src/index.ts", 0.98),
        make_ranked(".github/workflows/build-windows.yml", 0.97),
        make_ranked("packages/cli/src/server.ts", 0.96),
        make_ranked("packages/@n8n/task-runner-python/src/main.py", 0.95),
    ];
    let witness_hits = vec![
        make_witness(
            "packages/@n8n/node-cli/src/template/templates/declarative/custom/template/tsconfig.json",
            0.99,
        ),
        make_witness("tsconfig.json", 0.74),
    ];
    let intent =
        HybridRankingIntent::from_query("entry point bootstrap server app cli router main");
    assert!(intent.wants_entrypoint_build_flow);

    let ctx = PostSelectionContext::new(
        &intent,
        "entry point bootstrap server app cli router main",
        4,
        &[],
        &witness_hits,
    );
    let final_matches = apply_runtime_config_surface_selection(
        matches,
        &ctx,
        test_rule_meta("post_selection.runtime_config"),
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.contains(&"tsconfig.json"), "final paths: {paths:?}");
    assert!(
            !paths.contains(
                &"packages/@n8n/node-cli/src/template/templates/declarative/custom/template/tsconfig.json"
            ),
            "final paths: {paths:?}"
        );
}

#[test]
fn post_selection_policy_runtime_config_trace_records_root_manifest_replacement() {
    let matches = vec![
        make_ranked("backend/app.py", 0.96),
        make_ranked("backend/cli.py", 0.92),
        make_ranked("README.md", 0.78),
    ];
    let witness_hits = vec![make_witness("backend/pyproject.toml", 0.86)];
    let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");

    let (final_matches, trace) = apply_context_with_trace(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point bootstrap app startup cli main",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"backend/pyproject.toml"),
        "final paths: {paths:?}"
    );
    assert!(!paths.contains(&"README.md"), "final paths: {paths:?}");
    assert_eq!(
        trace.events,
        vec![PostSelectionTraceEvent {
            rule_id: "post_selection.runtime_config",
            rule_stage: PolicyStage::PostSelectionRuntime,
            action: PostSelectionRepairAction::Replaced,
            candidate_path: "backend/pyproject.toml".to_owned(),
            replaced_path: Some("README.md".to_owned()),
        }]
    );
}

#[test]
fn post_selection_policy_ci_scripts_prefers_top_level_ops_and_ci_surfaces() {
    let matches = vec![
        make_ranked("scripts/ty_benchmark/src/benchmark/run.py", 0.96),
        make_ranked("scripts/ty_benchmark/pyproject.toml", 0.94),
        make_ranked("crates/ruff/src/lib.rs", 0.90),
    ];
    let candidate_pool = vec![
        make_ranked("scripts/Dockerfile.ecosystem", 0.89),
        make_ranked(".github/workflows/build-docker.yml", 0.88),
    ];
    let intent = HybridRankingIntent::from_query(
        "ci release workflow github action publish package deploy cross compile script scripts dockerfile utils build binaries build docker",
    );
    assert!(intent.wants_ci_workflow_witnesses);
    assert!(intent.wants_scripts_ops_witnesses);

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "scripts dockerfile build workflow",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.contains(&"scripts/Dockerfile.ecosystem"));
    assert!(paths.contains(&".github/workflows/build-docker.yml"));
    assert!(!paths.contains(&"scripts/ty_benchmark/pyproject.toml"));
}

#[test]
fn post_selection_policy_mixed_support_recovers_missing_bench_and_plain_test_at_limit() {
    let matches = vec![
        make_ranked("tests/support/render_helpers.rs", 0.93),
        make_ranked("benchmarks/rendering.md", 0.78),
    ];
    let witness_hits = vec![
        make_witness("tests/support/bench_assertions.rs", 0.88),
        make_witness("benches/support/render_harness.rs", 0.87),
    ];
    let intent = HybridRankingIntent::from_query("tests benchmark render harness");
    assert!(intent.wants_test_witness_recall);
    assert!(intent.wants_benchmarks);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "test bench support render",
        2,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert_eq!(final_matches.len(), 2);
    assert!(paths.iter().any(|path| is_plain_test_support_path(path)));
    assert!(
        paths
            .iter()
            .any(|path| surfaces::is_bench_support_path(path))
    );
}

#[test]
fn post_selection_policy_mixed_support_recovers_missing_example_support_at_limit() {
    let matches = vec![
        make_ranked("platform/main.roc", 0.97),
        make_ranked("tests/cmd-test.roc", 0.92),
        make_ranked("crates/roc_host/src/lib.rs", 0.88),
    ];
    let witness_hits = vec![
        make_witness("examples/command.roc", 0.87),
        make_witness("examples/bytes-stdin-stdout.roc", 0.86),
    ];
    let intent = HybridRankingIntent::from_query(
        "entry point main app package platform runtime tests bytes stdin command line examples benches benchmark",
    );
    assert!(intent.wants_entrypoint_build_flow);
    assert!(intent.wants_examples);
    assert!(intent.wants_test_witness_recall);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point main app package platform runtime tests bytes stdin command line examples benches benchmark",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"platform/main.roc"),
        "final paths: {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|path| surfaces::is_example_support_path(path)),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_mixed_support_replaces_generic_example_with_specific_witness() {
    let matches = vec![
        make_ranked("platform/main.roc", 0.97),
        make_ranked("tests/cmd-test.roc", 0.92),
        make_ranked("examples/temp-dir.roc", 0.90),
    ];
    let witness_hits = vec![
        make_witness("examples/command.roc", 0.89),
        make_witness("examples/dir.roc", 0.88),
    ];
    let intent = HybridRankingIntent::from_query(
        "tests fixtures integration entry point main app package platform runtime command dir examples benches benchmark",
    );
    assert!(intent.wants_entrypoint_build_flow);
    assert!(intent.wants_examples);
    assert!(intent.wants_test_witness_recall);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "tests fixtures integration entry point main app package platform runtime command dir examples benches benchmark",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"examples/command.roc") || paths.contains(&"examples/dir.roc"),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"examples/temp-dir.roc"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_recovers_runtime_anchor_test_for_entrypoint_queries() {
    let matches = vec![
        make_ranked("backend/app.py", 0.96),
        make_ranked("backend/cli.py", 0.92),
        make_ranked("backend/pyproject.toml", 0.89),
    ];
    let witness_hits = vec![make_witness("backend/tests/test_server.py", 0.84)];
    let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point bootstrap app startup cli main",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"backend/tests/test_server.py"),
        "final paths: {paths:?}"
    );
    assert!(paths.contains(&"backend/app.py"));
    assert!(paths.contains(&"backend/cli.py"));
    assert!(!paths.contains(&"backend/pyproject.toml"));
}

#[test]
fn post_selection_policy_entrypoint_queries_keep_runtime_config_when_inserting_companion_test() {
    let matches = vec![
        make_ranked("classic/original_autogpt/autogpt/app/main.py", 0.97),
        make_ranked("autogpt_platform/backend/backend/app.py", 0.95),
        make_ranked("autogpt_platform/backend/backend/cli.py", 0.94),
        make_ranked(
            "autogpt_platform/backend/backend/copilot/executor/__main__.py",
            0.92,
        ),
        make_ranked("autogpt_platform/backend/pyproject.toml", 0.90),
    ];
    let witness_hits = vec![make_witness(
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
        0.88,
    )];
    let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point bootstrap app startup cli main",
        5,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert_eq!(final_matches.len(), 5);
    assert!(
        paths.contains(&"autogpt_platform/backend/pyproject.toml"),
        "final paths: {paths:?}"
    );
    assert!(
        paths.contains(&"autogpt_platform/backend/backend/blocks/mcp/test_server.py"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_entrypoint_queries_prefer_prefix_python_tests_over_loose_suffix_tests() {
    let matches = vec![
        make_ranked(
            "autogpt_platform/backend/backend/copilot/executor/__main__.py",
            0.96,
        ),
        make_ranked("autogpt_platform/backend/backend/app.py", 0.92),
        make_ranked("autogpt_platform/backend/backend/cli.py", 0.89),
    ];
    let witness_hits = vec![
        make_witness(
            "autogpt_platform/backend/backend/copilot/service_test.py",
            0.90,
        ),
        make_witness(
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
            0.88,
        ),
    ];
    let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
    assert!(intent.wants_entrypoint_build_flow);
    assert!(!intent.wants_test_witness_recall);
    let ctx = PostSelectionContext::new(
        &intent,
        "entry point bootstrap app startup cli main",
        3,
        &matches,
        &witness_hits,
    );
    let state = selection_guardrail_state(&matches, &ctx);
    let preferred = hybrid_ranked_evidence_from_witness_hit(&witness_hits[1]);
    let loose = hybrid_ranked_evidence_from_witness_hit(&witness_hits[0]);
    assert!(selection_guardrail_cmp(&preferred, &loose, &state, &ctx).is_gt());
    let chosen_witness = witness_hits
        .iter()
        .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, &ctx))
        .expect("witness candidate should exist");
    assert_eq!(
        chosen_witness.document.path,
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py"
    );
    let inserted = insert_test_support_guardrail_candidate(
        matches.clone(),
        Some(hybrid_ranked_evidence_from_witness_hit(chosen_witness)),
        &ctx,
        test_rule_meta("post_selection.runtime_companion_tests"),
        None,
    );
    let inserted_paths: Vec<_> = inserted
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();
    assert!(
        inserted_paths.contains(&"autogpt_platform/backend/backend/blocks/mcp/test_server.py"),
        "inserted paths: {inserted_paths:?}"
    );

    let final_matches = apply_runtime_companion_test_visibility(
        matches.clone(),
        &ctx,
        test_rule_meta("post_selection.runtime_companion_tests"),
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"autogpt_platform/backend/backend/blocks/mcp/test_server.py"),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"autogpt_platform/backend/backend/copilot/service_test.py"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_test_focus_queries_recover_runtime_subject_surface_under_test_crowding()
 {
    let matches = vec![
        make_ranked("tests/unit/auth_controller_test.rs", 0.98),
        make_ranked("tests/unit/auth_controller_variant_00_test.rs", 0.97),
        make_ranked("tests/unit/auth_controller_variant_01_test.rs", 0.96),
    ];
    let candidate_pool = vec![make_ranked("src/auth_controller.rs", 0.90)];
    let intent = HybridRankingIntent::from_query("auth controller test coverage");
    assert!(intent.wants_test_witness_recall);

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "auth controller test coverage",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"src/auth_controller.rs"),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"tests/unit/auth_controller_variant_01_test.rs"),
        "runtime subject recovery should evict the weakest generic test crowding entry: {paths:?}"
    );
}

#[test]
fn post_selection_policy_test_focus_queries_recover_runtime_subject_surface_from_witness_hits() {
    let matches = vec![
        make_ranked("tests/unit/auth_controller_test.rs", 0.98),
        make_ranked("tests/unit/auth_controller_variant_00_test.rs", 0.97),
        make_ranked("tests/unit/auth_controller_variant_01_test.rs", 0.96),
    ];
    let witness_hits = vec![make_witness("src/auth_controller.rs", 0.32)];
    let intent = HybridRankingIntent::from_query("auth controller test coverage");
    assert!(intent.wants_test_witness_recall);

    let (final_matches, trace) = apply_context_with_trace(
        matches,
        &[],
        &witness_hits,
        &intent,
        "auth controller test coverage",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"src/auth_controller.rs"),
        "final paths: {paths:?}; trace events: {}",
        trace.events.len()
    );
    assert!(
        trace.events.iter().any(|event| {
            event.rule_id == "post_selection.runtime_companion_surface"
                && matches!(event.action, PostSelectionRepairAction::Replaced)
        }),
        "runtime companion surface rule should record a replacement repair"
    );
}

#[test]
fn post_selection_policy_entrypoint_queries_promote_cli_command_entrypoints_over_web_runtime_noise()
{
    let matches = vec![
        make_ranked("go.mod", 0.99),
        make_ranked(".github/workflows/build-and-push-image.yml", 0.98),
        make_ranked("web/frps/src/api/server.ts", 0.97),
        make_ranked("go.sum", 0.96),
        make_ranked("web/frps/src/types/server.ts", 0.95),
    ];
    let witness_hits = vec![
        make_witness("cmd/frpc/main.go", 0.92),
        make_witness("cmd/frps/root.go", 0.90),
    ];
    let intent =
        HybridRankingIntent::from_query("entry point bootstrap server api main cli command");
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point bootstrap server api main cli command",
        5,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths
            .iter()
            .any(|path| { matches!(*path, "cmd/frpc/main.go" | "cmd/frps/root.go") }),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"web/frps/src/types/server.ts"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_client_queries_do_not_trigger_cli_entrypoint_guardrail() {
    let matches = vec![
        make_ranked("src/main.rs", 0.99),
        make_ranked(".github/workflows/build.yml", 0.97),
        make_ranked("Cargo.toml", 0.95),
    ];
    let witness_hits = vec![make_witness("cmd/frpc/main.go", 0.94)];
    let intent = HybridRankingIntent::from_query("entry point bootstrap client server main");
    assert!(intent.wants_entrypoint_build_flow);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "entry point bootstrap client server main",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        !paths.contains(&"cmd/frpc/main.go"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_cli_command_queries_keep_specific_cli_test_witness_visible() {
    let matches = vec![
        make_ranked("crates/ruff/tests/cli/main.rs", 0.99),
        make_ranked("crates/ruff_linter/src/lib.rs", 0.98),
        make_ranked("crates/ruff/tests/cli/lint.rs", 0.97),
        make_ranked("crates/ruff/tests/cli/format.rs", 0.96),
        make_ranked(
            "crates/ruff_linter/resources/test/fixtures/isort/pyproject.toml",
            0.95,
        ),
    ];
    let witness_hits = vec![make_witness("crates/ruff/tests/cli/analyze_graph.rs", 0.94)];
    let intent = HybridRankingIntent::from_query("ruff analyze ruff cli entrypoint");
    assert!(intent.wants_entrypoint_build_flow);
    let query_context = SelectionQueryContext::new(&intent, "ruff analyze ruff cli entrypoint");
    assert!(query_context.query_mentions_cli);
    assert_eq!(
        query_context.specific_witness_terms,
        vec!["ruff".to_owned(), "analyze".to_owned()]
    );

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "ruff analyze ruff cli entrypoint",
        5,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"crates/ruff/tests/cli/analyze_graph.rs"),
        "final paths: {paths:?}"
    );
    assert!(
        paths.contains(&"crates/ruff/tests/cli/main.rs"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_recovers_runtime_anchor_test_for_runtime_config_queries() {
    let matches = vec![
        make_ranked("autogpt_platform/frontend/tutorial/helpers/index.ts", 0.97),
        make_ranked("backend/pyproject.toml", 0.95),
        make_ranked("backend/cli.py", 0.90),
    ];
    let witness_hits = vec![
        make_witness("backend/tests/test_helpers.py", 0.86),
        make_witness("backend/tests/test_server.py", 0.88),
    ];
    let intent = HybridRankingIntent::from_query("config setup pyproject tests helpers e2e");
    assert!(intent.wants_runtime_config_artifacts);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "config setup pyproject tests helpers e2e",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.iter().any(|path| is_plain_test_support_path(path)));
    assert!(paths.contains(&"backend/pyproject.toml"));
    assert!(paths.contains(&"backend/cli.py"));
    assert!(!paths.contains(&"autogpt_platform/frontend/tutorial/helpers/index.ts"));
}

#[test]
fn post_selection_policy_recovers_plain_test_for_explicit_test_focus_queries() {
    let matches = vec![
        make_ranked("autogpt_platform/frontend/tutorial/helpers/index.ts", 0.97),
        make_ranked("backend/pyproject.toml", 0.95),
        make_ranked("backend/cli.py", 0.90),
    ];
    let witness_hits = vec![
        make_witness("backend/tests/test_helpers.py", 0.86),
        make_witness("backend/tests/test_server.py", 0.80),
    ];
    let intent = HybridRankingIntent::from_query(
        "tests fixtures integration helpers e2e config setup pyproject",
    );

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "tests fixtures integration helpers e2e config setup pyproject",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.iter().any(|path| is_plain_test_support_path(path)));
    assert!(paths.contains(&"backend/pyproject.toml"));
    assert!(paths.contains(&"backend/cli.py"));
    assert!(!paths.contains(&"autogpt_platform/frontend/tutorial/helpers/index.ts"));
}

#[test]
fn post_selection_policy_replaces_weaker_existing_plain_test_with_stronger_family_match() {
    let matches = vec![
        make_ranked("autogpt_platform/backend/pyproject.toml", 0.95),
        make_ranked("autogpt_platform/backend/backend/cli.py", 0.90),
        make_ranked("classic/original_autogpt/tests/unit/test_config.py", 0.88),
    ];
    let witness_hits = vec![
        make_witness("autogpt_platform/backend/backend/api/test_helpers.py", 0.84),
        make_witness(
            "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
            0.82,
        ),
    ];
    let intent = HybridRankingIntent::from_query("config setup pyproject tests helpers e2e");

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "config setup pyproject tests helpers e2e",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(paths.contains(&"autogpt_platform/backend/backend/api/test_helpers.py"));
    assert!(!paths.contains(&"classic/original_autogpt/tests/unit/test_config.py"));
}

#[test]
fn post_selection_policy_explicit_test_queries_prefer_runtime_adjacent_python_tests() {
    let matches = vec![
        make_ranked("autogpt_platform/backend/pyproject.toml", 0.95),
        make_ranked("autogpt_platform/backend/backend/cli.py", 0.90),
        make_ranked("classic/original_autogpt/setup.py", 0.88),
    ];
    let witness_hits = vec![
        make_witness(
            "classic/original_autogpt/tests/integration/test_setup.py",
            0.90,
        ),
        make_witness("autogpt_platform/backend/backend/api/test_helpers.py", 0.86),
    ];
    let intent = HybridRankingIntent::from_query(
        "tests fixtures integration helpers e2e config setup pyproject",
    );

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "tests fixtures integration helpers e2e config setup pyproject",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"autogpt_platform/backend/backend/api/test_helpers.py"),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"classic/original_autogpt/tests/integration/test_setup.py"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_test_focus_queries_allow_package_local_test_trees_to_compete() {
    let matches = vec![
        make_ranked("autogpt_platform/backend/backend/app.py", 0.98),
        make_ranked("autogpt_platform/backend/backend/cli.py", 0.96),
        make_ranked("autogpt_platform/backend/pyproject.toml", 0.94),
    ];
    let witness_hits = vec![
        make_witness("tests/test_server.py", 0.91),
        make_witness("autogpt_platform/backend/test/sdk/test_server.py", 0.90),
    ];
    let intent = HybridRankingIntent::from_query("tests integration backend sdk");
    assert!(intent.wants_test_witness_recall);
    assert!(!intent.wants_entrypoint_build_flow);

    let ctx = PostSelectionContext::new(
        &intent,
        "tests integration backend sdk",
        3,
        &matches,
        &witness_hits,
    );
    let state = selection_guardrail_state(&matches, &ctx);
    let repo_root = hybrid_ranked_evidence_from_witness_hit(&witness_hits[0]);
    let package_local = hybrid_ranked_evidence_from_witness_hit(&witness_hits[1]);

    assert!(
        selection_guardrail_cmp(&package_local, &repo_root, &state, &ctx).is_gt(),
        "package-local test trees should outrank unrelated repo-root tests when runtime family affinity is stronger"
    );

    let final_matches = apply_runtime_companion_test_visibility(
        matches.clone(),
        &ctx,
        test_rule_meta("post_selection.runtime_companion_tests"),
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"autogpt_platform/backend/test/sdk/test_server.py"),
        "final paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"tests/test_server.py"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_explicit_test_queries_can_promote_specific_test_tree_witnesses() {
    let matches = vec![
        make_ranked("pyproject.toml", 0.99),
        make_ranked("src/transformers/cli/chat.py", 0.97),
        make_ranked(
            "examples/pytorch/audio-classification/requirements.txt",
            0.95,
        ),
        make_ranked("conftest.py", 0.94),
        make_ranked("setup.py", 0.92),
    ];
    let witness_hits = vec![
        make_witness("tests/causal_lm_tester.py", 0.91),
        make_witness("tests/cli/conftest.py", 0.90),
        make_witness("tests/cli/test_chat.py", 0.89),
    ];
    let intent = HybridRankingIntent::from_query(
        "tests fixtures integration causal lm chat examples benches benchmark pyproject",
    );
    assert!(intent.wants_test_witness_recall);
    assert!(intent.wants_runtime_config_artifacts);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "tests fixtures integration causal lm chat examples benches benchmark pyproject",
        5,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.iter().any(|path| {
            matches!(
                *path,
                "tests/causal_lm_tester.py" | "tests/cli/conftest.py" | "tests/cli/test_chat.py"
            )
        }),
        "final paths: {paths:?}"
    );
    assert!(!paths.contains(&"conftest.py"), "final paths: {paths:?}");
}

#[test]
fn post_selection_policy_explicit_test_queries_do_not_prefer_repo_root_test_trees() {
    let matches = vec![
        make_ranked("autogpt_platform/backend/pyproject.toml", 0.99),
        make_ranked("autogpt_platform/backend/backend/cli.py", 0.97),
        make_ranked("tests/cli/conftest.py", 0.95),
    ];
    let witness_hits = vec![
        make_witness("tests/cli/test_chat.py", 0.94),
        make_witness("autogpt_platform/backend/test/sdk/conftest.py", 0.92),
        make_witness("autogpt_platform/backend/test/sdk/test_client.py", 0.91),
    ];
    let intent =
        HybridRankingIntent::from_query("backend sdk conftest tests integration pyproject");
    assert!(intent.wants_test_witness_recall);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "backend sdk conftest tests integration pyproject",
        3,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.contains(&"autogpt_platform/backend/test/sdk/conftest.py")
            || paths.contains(&"autogpt_platform/backend/test/sdk/test_client.py"),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_runtime_config_queries_can_promote_test_tree_harnesses() {
    let matches = vec![
        make_ranked("pyproject.toml", 0.99),
        make_ranked("conftest.py", 0.96),
        make_ranked("setup.py", 0.95),
        make_ranked(
            "examples/pytorch/audio-classification/requirements.txt",
            0.94,
        ),
        make_ranked("tests/sagemaker/scripts/pytorch/requirements.txt", 0.93),
    ];
    let witness_hits = vec![
        make_witness("tests/cli/conftest.py", 0.92),
        make_witness("tests/cli/test_chat.py", 0.90),
        make_witness("tests/generation/__init__.py", 0.88),
    ];
    let intent = HybridRankingIntent::from_query(
        "config examples benches benchmark pyproject requirements tests",
    );
    assert!(intent.wants_runtime_config_artifacts);

    let final_matches = apply_context(
        matches,
        &[],
        &witness_hits,
        &intent,
        "config examples benches benchmark pyproject requirements tests",
        5,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert!(
        paths.iter().any(|path| {
            matches!(
                *path,
                "tests/cli/conftest.py" | "tests/cli/test_chat.py" | "tests/generation/__init__.py"
            )
        }),
        "final paths: {paths:?}"
    );
}

#[test]
fn post_selection_policy_laravel_ui_recovers_test_harness_without_displacing_existing_blade_surface()
 {
    let matches = vec![
        make_ranked("resources/views/components/button.blade.php", 0.95),
        make_ranked("app/Livewire/ButtonPanel.php", 0.88),
    ];
    let candidate_pool = vec![make_ranked("tests/TestCase.php", 0.84)];
    let intent = HybridRankingIntent::from_query("blade component button view");
    assert!(intent.wants_laravel_ui_witnesses);

    let final_matches = apply_context(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "blade component button harness",
        2,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert_eq!(final_matches.len(), 2);
    assert!(paths.contains(&"resources/views/components/button.blade.php"));
    assert!(paths.contains(&"tests/TestCase.php"));
    assert!(!paths.contains(&"app/Livewire/ButtonPanel.php"));
}

#[test]
fn post_selection_policy_laravel_harness_trace_records_replacement() {
    let matches = vec![
        make_ranked("resources/views/components/button.blade.php", 0.95),
        make_ranked("app/Livewire/ButtonPanel.php", 0.88),
    ];
    let candidate_pool = vec![make_ranked("tests/TestCase.php", 0.84)];
    let intent = HybridRankingIntent::from_query("blade component button view");

    let (final_matches, trace) = apply_context_with_trace(
        matches,
        &candidate_pool,
        &[],
        &intent,
        "blade component button harness",
        2,
    );
    let paths: Vec<_> = final_matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect();

    assert_eq!(final_matches.len(), 2);
    assert!(paths.contains(&"resources/views/components/button.blade.php"));
    assert!(paths.contains(&"tests/TestCase.php"));
    assert!(!paths.contains(&"app/Livewire/ButtonPanel.php"));
    assert_eq!(
        trace.events,
        vec![PostSelectionTraceEvent {
            rule_id: "post_selection.laravel_ui_test_harness",
            rule_stage: PolicyStage::PostSelectionLaravel,
            action: PostSelectionRepairAction::Replaced,
            candidate_path: "tests/TestCase.php".to_owned(),
            replaced_path: Some("app/Livewire/ButtonPanel.php".to_owned()),
        }]
    );
}
