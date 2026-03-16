use super::*;
use crate::searcher::RepositoryCandidateUniverse;

fn base_path_witness_seed_paths(
    repository: &RepositoryCandidateUniverse,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
    lexical_limit: usize,
) -> Vec<String> {
    let per_repository_limit = lexical_limit.saturating_div(2).saturating_add(4).max(10);
    let mut scored = repository
        .candidates
        .iter()
        .filter_map(|candidate| {
            let score =
                hybrid_path_witness_recall_score(&candidate.relative_path, intent, query_context)?;
            Some((
                score,
                candidate.relative_path.clone(),
                candidate.absolute_path.clone(),
            ))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    scored
        .into_iter()
        .take(per_repository_limit)
        .map(|(_, path, _)| path)
        .collect()
}

#[test]
fn hybrid_test_subject_projection_materializes_manifest_projection_rows() -> FriggResult<()> {
    let root = temp_workspace_root("test-subject-projection-materialization");
    prepare_workspace(
        &root,
        &[
            ("src/user_service.rs", "pub fn user_service() {}\n"),
            (
                "tests/unit/user_service_test.rs",
                "#[test]\nfn user_service_test() {}\n",
            ),
            ("src/auth.py", "def auth():\n    return True\n"),
            (
                "tests/integration/auth_spec.py",
                "def test_auth():\n    assert auth()\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "src/user_service.rs",
            "tests/unit/user_service_test.rs",
            "src/auth.py",
            "tests/integration/auth_spec.py",
        ],
    )?;

    let db_path = resolve_provenance_db_path(&root)?;
    let storage = Storage::new(db_path);
    assert!(
        storage
            .load_test_subject_projections_for_repository_snapshot("repo-001", "snapshot-001")?
            .is_empty(),
        "test subject projection rows should start empty before the first overlay load"
    );

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 32,
            },
            &normalize_search_filters(SearchFilters::default())?,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");

    let projections = searcher
        .load_or_build_test_subject_projections_for_repository(repository, "snapshot-001")
        .expect("test subject projections should build");
    assert_eq!(projections.len(), 2);

    let rows = storage
        .load_test_subject_projections_for_repository_snapshot("repo-001", "snapshot-001")?;
    assert_eq!(rows.len(), 2);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_test_subject_projection_rebuilds_partial_snapshot_rows() -> FriggResult<()> {
    let root = temp_workspace_root("test-subject-projection-partial-rebuild");
    prepare_workspace(
        &root,
        &[
            ("src/user_service.rs", "pub fn user_service() {}\n"),
            (
                "tests/unit/user_service_test.rs",
                "#[test]\nfn user_service_test() {}\n",
            ),
            ("src/auth.py", "def auth():\n    return True\n"),
            (
                "tests/integration/auth_spec.py",
                "def test_auth():\n    assert auth()\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "src/user_service.rs",
            "tests/unit/user_service_test.rs",
            "src/auth.py",
            "tests/integration/auth_spec.py",
        ],
    )?;

    let db_path = resolve_provenance_db_path(&root)?;
    let storage = Storage::new(db_path);
    storage.replace_test_subject_projections_for_repository_snapshot(
        "repo-001",
        "snapshot-001",
        &[crate::storage::TestSubjectProjection {
            test_path: "tests/unit/user_service_test.rs".to_owned(),
            subject_path: "src/user_service.rs".to_owned(),
            shared_terms: vec!["user".to_owned(), "service".to_owned()],
            score_hint: 19,
            flags_json: r#"{"exact_stem_match":true}"#.to_owned(),
        }],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 32,
            },
            &normalize_search_filters(SearchFilters::default())?,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");

    let projections = searcher
        .load_or_build_test_subject_projections_for_repository(repository, "snapshot-001")
        .expect("partial test subject rows should rebuild");
    assert_eq!(projections.len(), 2);

    let rows = storage
        .load_test_subject_projections_for_repository_snapshot("repo-001", "snapshot-001")?;
    assert_eq!(rows.len(), 2);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_entrypoint_surface_projection_materializes_and_reuses_snapshot_cache() -> FriggResult<()>
{
    let root = temp_workspace_root("entrypoint-surface-projection-cache");
    prepare_workspace(
        &root,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.rs", "fn main() {}\n"),
            (".github/workflows/ci.yml", "name: ci\n"),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["Cargo.toml", "src/main.rs", ".github/workflows/ci.yml"],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    assert_eq!(searcher.entrypoint_surface_projection_cache_len(), 0);

    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 32,
            },
            &normalize_search_filters(SearchFilters::default())?,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");

    let first = searcher
        .load_or_build_entrypoint_surface_projections_for_repository(repository, "snapshot-001")
        .expect("entrypoint surfaces should build");
    assert!(
        first
            .iter()
            .any(|projection| projection.path == "Cargo.toml"),
        "entrypoint surface projection should retain runtime config artifacts"
    );
    assert!(
        first
            .iter()
            .any(|projection| projection.path == "src/main.rs"),
        "entrypoint surface projection should retain runtime entrypoints"
    );
    assert!(
        first
            .iter()
            .any(|projection| projection.path == ".github/workflows/ci.yml"),
        "entrypoint surface projection should retain workflow surfaces"
    );
    assert_eq!(searcher.entrypoint_surface_projection_cache_len(), 1);

    let second = searcher
        .load_or_build_entrypoint_surface_projections_for_repository(repository, "snapshot-001")
        .expect("cached entrypoint surfaces should load");
    assert_eq!(&*first, &*second);

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn projected_path_witness_candidates_apply_test_subject_overlay_boosts() -> FriggResult<()> {
    let root = temp_workspace_root("test-subject-overlay-candidate-boosts");
    prepare_workspace(
        &root,
        &[
            ("src/user_service.rs", "pub fn user_service() {}\n"),
            ("src/other.rs", "pub fn other() {}\n"),
            (
                "tests/unit/user_service_test.rs",
                "#[test]\nfn user_service_test() {}\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            "src/user_service.rs",
            "src/other.rs",
            "tests/unit/user_service_test.rs",
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 32,
            },
            &normalize_search_filters(SearchFilters::default())?,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");
    let intent = HybridRankingIntent::from_query("user service tests");
    let query_context = HybridPathWitnessQueryContext::from_query_text("user service tests");
    let candidates = searcher
        .projected_path_witness_candidates_for_repository(
            repository,
            Some(repository),
            &intent,
            &query_context,
        )
        .expect("projected path witness candidates should load");

    let test_candidate = candidates
        .iter()
        .find(|candidate| candidate.rel_path == "tests/unit/user_service_test.rs")
        .expect("test companion should be present");
    let baseline_test_score = hybrid_path_witness_recall_score_for_projection(
        "tests/unit/user_service_test.rs",
        &StoredPathWitnessProjection::from_path("tests/unit/user_service_test.rs"),
        &intent,
        &query_context,
    )
    .expect("baseline test witness score should exist");
    assert!(
        test_candidate.score > baseline_test_score,
        "test companion should receive an overlay boost beyond baseline recall"
    );
    assert!(
        test_candidate
            .witness_provenance_ids
            .iter()
            .any(|id| id.starts_with("overlay:test_subject:test:")),
        "test companion should carry test-subject overlay provenance"
    );

    let subject_candidate = candidates
        .iter()
        .find(|candidate| candidate.rel_path == "src/user_service.rs")
        .expect("runtime subject should be present");
    assert!(
        subject_candidate
            .witness_provenance_ids
            .iter()
            .any(|id| id.starts_with("overlay:test_subject:subject:")),
        "runtime subject should carry reverse test-subject overlay provenance"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn projected_path_witness_candidates_apply_entrypoint_surface_overlay_boosts() -> FriggResult<()> {
    let root = temp_workspace_root("entrypoint-surface-overlay-candidate-boosts");
    prepare_workspace(
        &root,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.rs", "fn main() {}\n"),
            ("README.md", "# docs\n"),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["Cargo.toml", "src/main.rs", "README.md"],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 32,
            },
            &normalize_search_filters(SearchFilters::default())?,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");
    let intent = HybridRankingIntent::from_query("runtime config manifest settings");
    let query_context =
        HybridPathWitnessQueryContext::from_query_text("runtime config manifest settings");
    let candidates = searcher
        .projected_path_witness_candidates_for_repository(
            repository,
            Some(repository),
            &intent,
            &query_context,
        )
        .expect("projected path witness candidates should load");

    let cargo_candidate = candidates
        .iter()
        .find(|candidate| candidate.rel_path == "Cargo.toml")
        .expect("runtime config artifact should be present");
    let baseline_cargo_score = hybrid_path_witness_recall_score_for_projection(
        "Cargo.toml",
        &StoredPathWitnessProjection::from_path("Cargo.toml"),
        &intent,
        &query_context,
    )
    .expect("baseline Cargo.toml witness score should exist");
    assert!(
        cargo_candidate.score > baseline_cargo_score,
        "runtime config artifact should receive an entrypoint-surface overlay boost"
    );
    assert!(
        cargo_candidate
            .witness_provenance_ids
            .iter()
            .any(|id| id.starts_with("overlay:entrypoint_surface:config:")),
        "runtime config artifact should carry entrypoint-surface overlay provenance"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn overlay_aware_path_witness_seed_universe_promotes_reverse_subject_companions() -> FriggResult<()>
{
    let root = temp_workspace_root("overlay-seed-reverse-subject-recall");
    let mut files = vec![
        (
            "src/auth_controller.rs".to_owned(),
            "pub fn auth_controller() {}\n".to_owned(),
        ),
        (
            "tests/unit/auth_controller_test.rs".to_owned(),
            "#[test]\nfn auth_controller_test() {}\n".to_owned(),
        ),
    ];
    let mut manifest_paths = vec![
        "src/auth_controller.rs".to_owned(),
        "tests/unit/auth_controller_test.rs".to_owned(),
    ];
    for index in 0..12 {
        let path = format!("tests/unit/auth_controller_variant_{index:02}_test.rs");
        files.push((
            path.clone(),
            format!("#[test]\nfn auth_controller_variant_{index:02}_test() {{}}\n"),
        ));
        manifest_paths.push(path);
    }
    let borrowed_files = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let borrowed_paths = manifest_paths
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    prepare_workspace(&root, &borrowed_files)?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &borrowed_paths)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let filters = normalize_search_filters(SearchFilters::default())?;
    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 8,
            },
            &filters,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");
    let intent = HybridRankingIntent::from_query("auth controller test coverage");
    let query_context =
        HybridPathWitnessQueryContext::from_query_text("auth controller test coverage");

    let baseline_paths = base_path_witness_seed_paths(repository, &intent, &query_context, 8);
    assert!(
        !baseline_paths
            .iter()
            .any(|path| path == "src/auth_controller.rs"),
        "baseline seed selection should crowd out the related runtime subject: {baseline_paths:?}"
    );

    let overlay_seed_universe = searcher
        .build_overlay_aware_path_witness_seed_universe(
            &universe,
            &filters,
            &intent,
            &query_context,
            8,
        )
        .expect("overlay-aware seed universe should build");
    let overlay_paths = overlay_seed_universe.repositories[0]
        .candidates
        .iter()
        .map(|candidate| candidate.relative_path.clone())
        .collect::<Vec<_>>();
    assert!(
        overlay_paths
            .iter()
            .any(|path| path == "src/auth_controller.rs"),
        "overlay-aware seed selection should recover the related runtime subject: {overlay_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_search_recalls_reverse_subject_runtime_files_for_test_focus_queries() -> FriggResult<()> {
    let root = temp_workspace_root("overlay-hybrid-reverse-subject-recall");
    let mut files = vec![
        (
            "src/auth_controller.rs".to_owned(),
            "pub fn auth_controller() {}\n".to_owned(),
        ),
        (
            "tests/unit/auth_controller_test.rs".to_owned(),
            "#[test]\nfn auth_controller_test() {}\n".to_owned(),
        ),
    ];
    let mut manifest_paths = vec![
        "src/auth_controller.rs".to_owned(),
        "tests/unit/auth_controller_test.rs".to_owned(),
    ];
    for index in 0..12 {
        let path = format!("tests/unit/auth_controller_variant_{index:02}_test.rs");
        files.push((
            path.clone(),
            format!("#[test]\nfn auth_controller_variant_{index:02}_test() {{}}\n"),
        ));
        manifest_paths.push(path);
    }
    let borrowed_files = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let borrowed_paths = manifest_paths
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    prepare_workspace(&root, &borrowed_files)?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &borrowed_paths)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid(SearchHybridQuery {
        query: "auth controller test coverage".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;
    let witness_paths = output
        .channel_results
        .iter()
        .find(|result| result.channel == crate::domain::EvidenceChannel::PathSurfaceWitness)
        .map(|result| {
            result
                .hits
                .iter()
                .map(|hit| hit.document.path.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    assert!(
        witness_paths
            .iter()
            .any(|path| path == "src/auth_controller.rs"),
        "path witness recall should materialize the reverse runtime subject companion: {witness_paths:?}"
    );

    assert!(
        output
            .matches
            .iter()
            .any(|entry| entry.document.path == "src/auth_controller.rs"),
        "overlay-aware hybrid search should retain reverse runtime subject companions for test-focus queries: {:?}",
        output
            .matches
            .iter()
            .map(|entry| entry.document.path.clone())
            .collect::<Vec<_>>()
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn overlay_aware_path_witness_seed_universe_recalls_hidden_ci_workflows() -> FriggResult<()> {
    let root = temp_workspace_root("overlay-seed-ci-workflow-recall");
    let mut files = vec![
        ("src/main.rs".to_owned(), "fn main() {}\n".to_owned()),
        (
            ".github/workflows/ci.yml".to_owned(),
            "name: ci\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n".to_owned(),
        ),
    ];
    let mut manifest_paths = vec![
        "src/main.rs".to_owned(),
        ".github/workflows/ci.yml".to_owned(),
    ];
    for index in 0..12 {
        let path = format!("docs/build_pipeline_{index:02}.md");
        files.push((
            path.clone(),
            format!("# build pipeline {index:02}\nautomation notes\n"),
        ));
        manifest_paths.push(path);
    }
    let borrowed_files = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let borrowed_paths = manifest_paths
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    prepare_workspace(&root, &borrowed_files)?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &borrowed_paths)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let filters = normalize_search_filters(SearchFilters::default())?;
    let universe = searcher
        .build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 8,
            },
            &filters,
        )
        .universe;
    let repository = universe
        .repositories
        .first()
        .expect("expected manifest-backed repository");
    let intent = HybridRankingIntent::from_query("build pipeline automation");
    let query_context = HybridPathWitnessQueryContext::from_query_text("build pipeline automation");
    let baseline_paths = base_path_witness_seed_paths(repository, &intent, &query_context, 8);
    assert!(
        !baseline_paths
            .iter()
            .any(|path| path == ".github/workflows/ci.yml"),
        "baseline seed selection should crowd out the CI workflow artifact: {baseline_paths:?}"
    );

    let overlay_seed_universe = searcher
        .build_overlay_aware_path_witness_seed_universe(
            &universe,
            &filters,
            &intent,
            &query_context,
            8,
        )
        .expect("overlay-aware seed universe should build");
    let overlay_paths = overlay_seed_universe.repositories[0]
        .candidates
        .iter()
        .map(|candidate| candidate.relative_path.clone())
        .collect::<Vec<_>>();
    assert!(
        overlay_paths
            .iter()
            .any(|path| path == ".github/workflows/ci.yml"),
        "overlay-aware seed selection should recover hidden CI workflows: {overlay_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_search_recalls_ci_workflows_for_build_pipeline_queries() -> FriggResult<()> {
    let root = temp_workspace_root("overlay-hybrid-ci-workflow-recall");
    let mut files = vec![
        ("src/main.rs".to_owned(), "fn main() {}\n".to_owned()),
        (
            ".github/workflows/ci.yml".to_owned(),
            "name: ci\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n".to_owned(),
        ),
    ];
    let mut manifest_paths = vec![
        "src/main.rs".to_owned(),
        ".github/workflows/ci.yml".to_owned(),
    ];
    for index in 0..12 {
        let path = format!("docs/build_pipeline_{index:02}.md");
        files.push((
            path.clone(),
            format!("# build pipeline {index:02}\nautomation notes\n"),
        ));
        manifest_paths.push(path);
    }
    let borrowed_files = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect::<Vec<_>>();
    let borrowed_paths = manifest_paths
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    prepare_workspace(&root, &borrowed_files)?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &borrowed_paths)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid(SearchHybridQuery {
        query: "build pipeline automation".to_owned(),
        limit: 5,
        weights: HybridChannelWeights::default(),
        semantic: Some(false),
    })?;

    assert!(
        output
            .matches
            .iter()
            .any(|entry| entry.document.path == ".github/workflows/ci.yml"),
        "overlay-aware hybrid search should retain CI workflows for build pipeline queries: {:?}",
        output
            .matches
            .iter()
            .map(|entry| entry.document.path.clone())
            .collect::<Vec<_>>()
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn path_witness_hits_preserve_score_hints_and_overlay_provenance() {
    let intent = HybridRankingIntent::from_query("runtime config manifest settings");
    let hits = build_hybrid_path_witness_hits_with_intent(
        &[TextMatch {
            repository_id: "repo-001".to_owned(),
            path: "Cargo.toml".to_owned(),
            line: 1,
            column: 1,
            excerpt: "[package]".to_owned(),
            witness_score_hint_millis: Some(1750),
            witness_provenance_ids: Some(vec![
                "overlay:entrypoint_surface:config:Cargo.toml".to_owned(),
            ]),
        }],
        &intent,
        "runtime config manifest settings",
    );

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].raw_score, 1.75);
    assert!(
        hits[0]
            .provenance_ids
            .iter()
            .any(|id| id == "path_witness:Cargo.toml:1:1"),
        "path witness hits must keep the canonical witness provenance marker"
    );
    assert!(
        hits[0]
            .provenance_ids
            .iter()
            .any(|id| id == "overlay:entrypoint_surface:config:Cargo.toml"),
        "path witness hits should append overlay provenance instead of replacing it"
    );
}
