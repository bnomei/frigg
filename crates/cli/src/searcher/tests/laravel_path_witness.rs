use super::*;
use crate::searcher::PATH_WITNESS_PROJECTION_HEURISTIC_VERSION;
use crate::searcher::path_witness_projection::family_bits_for_projection;

#[test]
fn hybrid_ranking_semantic_laravel_ui_queries_surface_livewire_and_blade_witnesses()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-laravel-ui-witnesses");
    prepare_workspace(
        &root,
        &[
            ("app/Livewire/Dashboard.php", "<?php\nclass Dashboard {}\n"),
            (
                "app/Livewire/ActivityMonitor.php",
                "<?php\nclass ActivityMonitor {}\n",
            ),
            (
                "resources/views/livewire/subscription/show.blade.php",
                "<div>subscription</div>\n",
            ),
            (
                "resources/views/livewire/dashboard.blade.php",
                "<div>dashboard</div>\n",
            ),
            (
                "resources/views/layouts/simple.blade.php",
                "<x-layouts.simple />\n",
            ),
            (
                "resources/views/layouts/app.blade.php",
                "<x-app-layout />\n",
            ),
            ("resources/views/components/navbar.blade.php", "<nav />\n"),
            (
                "resources/views/components/applications/advanced.blade.php",
                "<x-applications.advanced />\n",
            ),
            (
                "resources/views/auth/verify-email.blade.php",
                "<x-auth.verify-email />\n",
            ),
            ("TECH_STACK.md", "# Tech Stack\nLaravel Livewire Flux\n"),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/livewire/subscription/show.blade.php",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/layouts/simple.blade.php",
                0,
                vec![0.99, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/components/navbar.blade.php",
                0,
                vec![0.985, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/livewire/dashboard.blade.php",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/layouts/app.blade.php",
                0,
                vec![0.97, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "TECH_STACK.md",
                0,
                vec![0.965, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/Livewire/Dashboard.php",
                0,
                vec![0.90, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/Livewire/ActivityMonitor.php",
                0,
                vec![0.89, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/components/applications/advanced.blade.php",
                0,
                vec![0.87, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/auth/verify-email.blade.php",
                0,
                vec![0.86, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "blade livewire flux component view slot section".to_owned(),
            limit: 8,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        },
        &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths
            .iter()
            .any(|path| *path == "app/Livewire/Dashboard.php"
                || *path == "app/Livewire/ActivityMonitor.php"),
        "Laravel UI ranking should keep a Livewire component witness in top-k: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().any(|path| {
            *path == "resources/views/components/applications/advanced.blade.php"
                || *path == "resources/views/auth/verify-email.blade.php"
        }),
        "Laravel UI ranking should keep a Blade view witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
#[test]
fn hybrid_ranking_laravel_ui_queries_avoid_unit3d_component_only_collapse() -> FriggResult<()> {
    let query = "blade livewire flux component view slot section";
    let make_hit = |path: &str, raw_score: f32, excerpt: &str| HybridChannelHit {
        channel: crate::domain::EvidenceChannel::LexicalManifest,
        document: HybridDocumentRef {
            repository_id: "repo-001".to_owned(),
            path: path.to_owned(),
            line: 1,
            column: 1,
        },
        anchor: crate::domain::EvidenceAnchor::new(
            crate::domain::EvidenceAnchorKind::TextSpan,
            1,
            1,
            1,
            1,
        ),
        raw_score,
        excerpt: excerpt.to_owned(),
        provenance_ids: vec![format!("lexical::{path}")],
    };
    let lexical = vec![
        make_hit(
            "resources/views/components/forum/post.blade.php",
            1.00,
            "@props(['post'])\n<article class=\"post\" x-data>\n",
        ),
        make_hit(
            "resources/views/components/torrent/row.blade.php",
            0.99,
            "@props(['torrent'])\n<tr data-torrent-id=\"1\">\n",
        ),
        make_hit(
            "resources/views/components/forum/topic-listing.blade.php",
            0.98,
            "<section class=\"topic-listing\"></section>\n",
        ),
        make_hit(
            "resources/views/components/torrent/comment-listing.blade.php",
            0.97,
            "<section class=\"comment-listing\"></section>\n",
        ),
        make_hit(
            "resources/views/components/tv/card.blade.php",
            0.96,
            "<x-card><x-slot:title>TV</x-slot:title></x-card>\n",
        ),
        make_hit(
            "resources/views/components/forum/subforum-listing.blade.php",
            0.95,
            "<section class=\"subforum-listing\"></section>\n",
        ),
        make_hit(
            "resources/views/components/user-tag.blade.php",
            0.94,
            "<x-user-tag />\n",
        ),
        make_hit(
            "resources/views/components/playlist/card.blade.php",
            0.93,
            "<x-card><x-slot:title>Playlist</x-slot:title></x-card>\n",
        ),
        make_hit(
            "resources/views/Staff/announce/index.blade.php",
            0.91,
            "@section('main')\n    @livewire('announce-search')\n@endsection\n",
        ),
        make_hit(
            "resources/views/Staff/application/index.blade.php",
            0.90,
            "@section('main')\n    @livewire('application-search')\n@endsection\n",
        ),
        make_hit(
            "resources/views/livewire/announce-search.blade.php",
            0.89,
            "<section class=\"panelV2\">\n    <input wire:model.live=\"torrentId\" />\n</section>\n",
        ),
        make_hit(
            "resources/views/livewire/apikey-search.blade.php",
            0.88,
            "<section class=\"panelV2\">\n    <input wire:model.live=\"apikey\" />\n</section>\n",
        ),
        make_hit(
            "app/Http/Livewire/AnnounceSearch.php",
            0.87,
            "<?php class AnnounceSearch extends Component {}\n",
        ),
    ];

    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        8,
        query,
    )?;
    let ranked_paths = ranked
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .any(|path| path.starts_with("resources/views/Staff/")),
        "Laravel UI ranking should keep a non-component Blade view witness in top-k: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .any(|path| path.starts_with("resources/views/livewire/")),
        "Laravel UI ranking should keep a Livewire Blade view witness in top-k: {ranked_paths:?}"
    );

    Ok(())
}

#[test]
fn hybrid_ranking_semantic_laravel_route_queries_surface_route_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-laravel-route-witnesses");
    prepare_workspace(
        &root,
        &[
            ("composer.lock", "{\n  \"packages\": []\n}\n"),
            (
                "tests/Feature/TrustHostsMiddlewareTest.php",
                "<?php\nclass TrustHostsMiddlewareTest {}\n",
            ),
            (
                "tests/Feature/CommandInjectionSecurityTest.php",
                "<?php\nclass CommandInjectionSecurityTest {}\n",
            ),
            (
                "app/Providers/FortifyServiceProvider.php",
                "<?php\nclass FortifyServiceProvider {}\n",
            ),
            (
                "app/Providers/RouteServiceProvider.php",
                "<?php\nclass RouteServiceProvider {}\n",
            ),
            (
                "app/Providers/ConfigurationServiceProvider.php",
                "<?php\nclass ConfigurationServiceProvider {}\n",
            ),
            ("routes/web.php", "<?php\nRoute::get('/', fn () => 'ok');\n"),
            (
                "routes/api.php",
                "<?php\nRoute::get('/api', fn () => 'ok');\n",
            ),
            (
                "routes/webhooks.php",
                "<?php\nRoute::post('/webhooks', fn () => 'ok');\n",
            ),
            ("bootstrap/app.php", "<?php\nreturn 'bootstrap';\n"),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "tests/Feature/CommandInjectionSecurityTest.php",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "tests/Feature/TrustHostsMiddlewareTest.php",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/Providers/FortifyServiceProvider.php",
                0,
                vec![0.92, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/Providers/RouteServiceProvider.php",
                0,
                vec![0.90, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/Providers/ConfigurationServiceProvider.php",
                0,
                vec![0.89, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "composer.lock",
                0,
                vec![0.87, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "routes/web.php",
                0,
                vec![0.82, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "routes/api.php",
                0,
                vec![0.81, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "routes/webhooks.php",
                0,
                vec![0.80, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "bootstrap/app.php",
                0,
                vec![0.79, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "bootstrap providers routes middleware app entrypoint".to_owned(),
            limit: 8,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        },
        &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths.iter().any(|path| matches!(
            *path,
            "routes/web.php" | "routes/api.php" | "routes/webhooks.php"
        )),
        "Laravel route ranking should keep a routes witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_semantic_laravel_linkstack_queries_recover_layouts_and_blade_views()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-laravel-linkstack-layouts-and-views");
    prepare_workspace(
        &root,
        &[
            (
                "app/View/Components/AppLayout.php",
                "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass AppLayout extends Component {\n    public function render() {\n        return view('layouts.app');\n    }\n}\n",
            ),
            (
                "app/View/Components/GuestLayout.php",
                "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass GuestLayout extends Component {\n    public function render() {\n        return view('layouts.guest');\n    }\n}\n",
            ),
            (
                "app/View/Components/Modal.php",
                "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass Modal extends Component {\n    public function render() {\n        return view('components.modal');\n    }\n}\n",
            ),
            (
                "app/View/Components/PageItemDisplay.php",
                "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass PageItemDisplay extends Component {\n    public function render() {\n        return view('components.page-item-display');\n    }\n}\n",
            ),
            (
                "app/Models/Page.php",
                "<?php\nnamespace App\\Models;\nclass Page {}\n",
            ),
            (
                "resources/views/components/finishing.blade.php",
                "<x-alert>\n<x-slot name=\"title\">blade layout component slot section render page navigation</x-slot>\n<div>blade layout component slot section render page navigation blade layout component slot section render page navigation</div>\n</x-alert>\n",
            ),
            (
                "resources/views/components/alert.blade.php",
                "<div class=\"alert\">blade component layout slot section view render</div>\n",
            ),
            (
                "resources/views/components/auth-card.blade.php",
                "<section class=\"auth-card\">blade component layout slot section view render</section>\n",
            ),
            (
                "resources/views/layouts/app.blade.php",
                "@include('layouts.analytics')\n@include('layouts.navigation')\n<header>{{ $header }}</header>\n<main>{{ $slot }}</main>\n",
            ),
            (
                "resources/views/layouts/guest.blade.php",
                "<main class=\"guest-layout\">{{ $slot }}</main>\n",
            ),
            (
                "resources/views/layouts/analytics.blade.php",
                "<script>window.analytics = true;</script>\n",
            ),
            (
                "resources/views/layouts/navigation.blade.php",
                "<nav class=\"main-nav\">navigation</nav>\n",
            ),
            (
                "resources/views/auth/forgot-password.blade.php",
                "<x-guest-layout>\n@include('layouts.lang')\n<x-auth-card>\n<x-slot name=\"logo\"></x-slot>\n@section('content') blade component layout slot section view render @endsection\n</x-auth-card>\n</x-guest-layout>\n",
            ),
            (
                "resources/views/auth/login.blade.php",
                "<x-guest-layout>\n<x-auth-card>\n<x-slot name=\"logo\"></x-slot>\n@section('content') blade component layout slot section view render @endsection\n</x-auth-card>\n</x-guest-layout>\n",
            ),
            (
                "resources/views/admin/linktype/index.blade.php",
                "@extends('layouts.app')\n@section('content')\n<a href=\"/admin/linktype/create\">blade component layout slot section view render</a>\n@endsection\n",
            ),
            (
                "TECH_STACK.md",
                "Blade Laravel view component layout reference.\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/components/finishing.blade.php",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/Models/Page.php",
                0,
                vec![0.99, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/View/Components/AppLayout.php",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/View/Components/GuestLayout.php",
                0,
                vec![0.97, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/View/Components/PageItemDisplay.php",
                0,
                vec![0.96, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/components/alert.blade.php",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "app/View/Components/Modal.php",
                0,
                vec![0.94, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/components/auth-card.blade.php",
                0,
                vec![0.93, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/auth/forgot-password.blade.php",
                0,
                vec![0.92, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/auth/login.blade.php",
                0,
                vec![0.91, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/admin/linktype/index.blade.php",
                0,
                vec![0.90, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/layouts/app.blade.php",
                0,
                vec![0.89, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/layouts/guest.blade.php",
                0,
                vec![0.88, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "resources/views/layouts/analytics.blade.php",
                0,
                vec![0.87, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "TECH_STACK.md",
                0,
                vec![0.86, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    let searcher = TextSearcher::new(config);
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

    let layout_output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "blade layout component slot section render page navigation".to_owned(),
            limit: 8,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &executor,
    )?;
    let layout_paths = layout_output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        layout_paths.iter().any(|path| matches!(
            *path,
            "resources/views/layouts/app.blade.php"
                | "resources/views/layouts/guest.blade.php"
                | "resources/views/layouts/analytics.blade.php"
        )),
        "Laravel UI ranking should keep a Blade layout witness in top-k under component-class pressure: {layout_paths:?}"
    );

    let blade_view_output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "blade component layout slot section view render".to_owned(),
            limit: 8,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &credentials,
        &executor,
    )?;
    let blade_view_paths = blade_view_output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        blade_view_paths.iter().any(|path| matches!(
            *path,
            "resources/views/auth/forgot-password.blade.php"
                | "resources/views/auth/login.blade.php"
                | "resources/views/admin/linktype/index.blade.php"
        )),
        "Laravel UI ranking should keep a concrete Blade page witness in top-k under component-class pressure: {blade_view_paths:?}"
    );
    assert!(
        blade_view_paths.iter().any(|path| matches!(
            *path,
            "resources/views/components/alert.blade.php"
                | "resources/views/components/auth-card.blade.php"
                | "resources/views/components/finishing.blade.php"
        )),
        "Laravel UI ranking should still keep a Blade component witness in top-k: {blade_view_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_uses_live_fallback_without_persisting_projection_rows()
-> FriggResult<()> {
    let root = temp_workspace_root("path-witness-live-fallback-no-persist");
    prepare_workspace(
        &root,
        &[
            (
                "tests/CreatesApplication.php",
                "<?php\n\ntrait CreatesApplication {}\n",
            ),
            (
                "tests/DuskTestCase.php",
                "<?php\n\nabstract class DuskTestCase {}\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["tests/CreatesApplication.php", "tests/DuskTestCase.php"],
    )?;

    let db_path = resolve_provenance_db_path(&root)?;
    let storage = Storage::new(db_path);
    assert!(
        storage
            .load_path_witness_projections_for_repository_snapshot("repo-001", "snapshot-001")?
            .is_empty(),
        "path witness projection rows should start empty before the first search"
    );

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);
    let query = "tests fixtures integration tests createsapplication dusktestcase";
    let intent = HybridRankingIntent::from_query(query);
    let output = searcher.search_path_witness_recall_with_filters(
        query,
        &SearchFilters::default(),
        8,
        &intent,
    )?;

    assert_eq!(output.matches.len(), 2);
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);
    let second_output = searcher.search_path_witness_recall_with_filters(
        query,
        &SearchFilters::default(),
        8,
        &intent,
    )?;
    assert_eq!(output.matches, second_output.matches);
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);

    let rows = storage
        .load_path_witness_projections_for_repository_snapshot("repo-001", "snapshot-001")?;
    assert!(
        rows.is_empty(),
        "query-time live fallback should not materialize path witness rows into storage"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_treats_partial_snapshot_rows_as_read_only_fallback() -> FriggResult<()>
{
    let root = temp_workspace_root("path-witness-partial-projection-readonly-fallback");
    prepare_workspace(
        &root,
        &[
            (
                "tests/CreatesApplication.php",
                "<?php\n\ntrait CreatesApplication {}\n",
            ),
            (
                "tests/DuskTestCase.php",
                "<?php\n\nabstract class DuskTestCase {}\n",
            ),
        ],
    )?;
    seed_manifest_snapshot(
        &root,
        "repo-001",
        "snapshot-001",
        &["tests/CreatesApplication.php", "tests/DuskTestCase.php"],
    )?;

    let db_path = resolve_provenance_db_path(&root)?;
    let storage = Storage::new(db_path);
    let authoritative_projection =
        StoredPathWitnessProjection::from_path("tests/CreatesApplication.php");
    storage.replace_retrieval_projection_bundle_for_repository_snapshot(
        "repo-001",
        "snapshot-001",
        crate::storage::RetrievalProjectionBundle {
            heads: vec![crate::storage::RetrievalProjectionHeadRecord {
                family: "path_witness".to_owned(),
                heuristic_version: PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
                input_modes: vec!["path".to_owned()],
                row_count: 2,
            }],
            path_witness: vec![crate::storage::PathWitnessProjection {
                path: "tests/CreatesApplication.php".to_owned(),
                path_class: authoritative_projection.path_class,
                source_class: authoritative_projection.source_class,
                file_stem: authoritative_projection.file_stem.clone(),
                path_terms: authoritative_projection.path_terms.clone(),
                subtree_root: authoritative_projection.subtree_root.clone(),
                family_bits: family_bits_for_projection(&authoritative_projection),
                flags_json: serde_json::to_string(&authoritative_projection.flags)
                    .expect("valid authoritative flags json"),
                heuristic_version: PATH_WITNESS_PROJECTION_HEURISTIC_VERSION,
            }],
            test_subject: Vec::new(),
            entrypoint_surface: Vec::new(),
            path_relations: Vec::new(),
            subtree_coverage: Vec::new(),
            path_surface_terms: Vec::new(),
            path_anchor_sketches: Vec::new(),
        },
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);

    let query = "tests fixtures integration tests createsapplication dusktestcase";
    let intent = HybridRankingIntent::from_query(query);
    let first = searcher.search_path_witness_recall_with_filters(
        query,
        &SearchFilters::default(),
        8,
        &intent,
    )?;
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);

    let second = searcher.search_path_witness_recall_with_filters(
        query,
        &SearchFilters::default(),
        8,
        &intent,
    )?;
    assert_eq!(first.matches, second.matches);
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);
    let rows = storage
        .load_path_witness_projections_for_repository_snapshot("repo-001", "snapshot-001")?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "tests/CreatesApplication.php");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_uses_authoritative_current_snapshot_projection_rows()
-> FriggResult<()> {
    let root = temp_workspace_root("path-witness-authoritative-current-head");
    prepare_workspace(&root, &[("src/main.rs", "fn main() {}\n")])?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/main.rs"])?;

    let db_path = resolve_provenance_db_path(&root)?;
    let storage = Storage::new(db_path);
    let authoritative_projection = StoredPathWitnessProjection::from_path("src/main.rs");
    storage.replace_retrieval_projection_bundle_for_repository_snapshot(
        "repo-001",
        "snapshot-001",
        crate::storage::RetrievalProjectionBundle {
            heads: vec![crate::storage::RetrievalProjectionHeadRecord {
                family: "path_witness".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            }],
            path_witness: vec![crate::storage::PathWitnessProjection {
                path: "src/main.rs".to_owned(),
                path_class: authoritative_projection.path_class,
                source_class: authoritative_projection.source_class,
                file_stem: authoritative_projection.file_stem.clone(),
                path_terms: vec!["bespokeprojectionterm".to_owned()],
                subtree_root: authoritative_projection.subtree_root.clone(),
                family_bits: family_bits_for_projection(&authoritative_projection),
                flags_json: serde_json::to_string(&authoritative_projection.flags)
                    .expect("valid authoritative flags json"),
                heuristic_version: 1,
            }],
            test_subject: Vec::new(),
            entrypoint_surface: Vec::new(),
            path_relations: Vec::new(),
            subtree_coverage: Vec::new(),
            path_surface_terms: Vec::new(),
            path_anchor_sketches: Vec::new(),
        },
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    assert_eq!(searcher.path_witness_projection_cache_len(), 0);
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
        .load_or_build_path_witness_projections_for_repository(repository, "snapshot-001")
        .expect("expected authoritative path witness projections");
    assert_eq!(searcher.path_witness_projection_cache_len(), 1);
    let cached_projections = searcher
        .load_or_build_path_witness_projections_for_repository(repository, "snapshot-001")
        .expect("expected cached authoritative path witness projections");
    assert!(std::sync::Arc::ptr_eq(&projections, &cached_projections));
    assert_eq!(searcher.path_witness_projection_cache_len(), 1);

    assert!(
        projections.iter().any(|projection| {
            projection.path == "src/main.rs"
                && projection.path_terms == vec!["bespokeprojectionterm".to_owned()]
        }),
        "current-head snapshots should trust authoritative stored path witness terms instead of rebuilding live path terms"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_prefers_authoritative_anchor_sketch_excerpt() -> FriggResult<()> {
    let root = temp_workspace_root("path-witness-authoritative-anchor-sketch");
    prepare_workspace(&root, &[("src/main.rs", "fn main() {}\n")])?;
    seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/main.rs"])?;

    let db_path = resolve_provenance_db_path(&root)?;
    let storage = Storage::new(db_path);
    let authoritative_projection = StoredPathWitnessProjection::from_path("src/main.rs");
    storage.replace_retrieval_projection_bundle_for_repository_snapshot(
        "repo-001",
        "snapshot-001",
        crate::storage::RetrievalProjectionBundle {
            heads: vec![
                crate::storage::RetrievalProjectionHeadRecord {
                    family: "path_witness".to_owned(),
                    heuristic_version: 1,
                    input_modes: vec!["path".to_owned()],
                    row_count: 1,
                },
                crate::storage::RetrievalProjectionHeadRecord {
                    family: "path_anchor_sketch".to_owned(),
                    heuristic_version: 1,
                    input_modes: vec!["path".to_owned()],
                    row_count: 1,
                },
            ],
            path_witness: vec![crate::storage::PathWitnessProjection {
                path: "src/main.rs".to_owned(),
                path_class: authoritative_projection.path_class,
                source_class: authoritative_projection.source_class,
                file_stem: authoritative_projection.file_stem.clone(),
                path_terms: vec![
                    "server".to_owned(),
                    "bootstrap".to_owned(),
                    "runtime".to_owned(),
                ],
                subtree_root: authoritative_projection.subtree_root.clone(),
                family_bits: family_bits_for_projection(&authoritative_projection),
                flags_json: serde_json::to_string(&authoritative_projection.flags)
                    .expect("valid authoritative flags json"),
                heuristic_version: 1,
            }],
            test_subject: Vec::new(),
            entrypoint_surface: Vec::new(),
            path_relations: Vec::new(),
            subtree_coverage: Vec::new(),
            path_surface_terms: Vec::new(),
            path_anchor_sketches: vec![crate::storage::PathAnchorSketchProjection {
                path: "src/main.rs".to_owned(),
                anchor_rank: 0,
                line: 7,
                anchor_kind: "line_excerpt".to_owned(),
                excerpt: "server bootstrap runtime".to_owned(),
                terms: vec![
                    "server".to_owned(),
                    "bootstrap".to_owned(),
                    "runtime".to_owned(),
                ],
                score_hint: 24,
            }],
        },
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let intent = HybridRankingIntent::from_query("server bootstrap runtime");
    let output = searcher.search_path_witness_recall_with_filters(
        "server bootstrap runtime",
        &SearchFilters::default(),
        8,
        &intent,
    )?;

    let top = output
        .matches
        .first()
        .expect("expected authoritative anchor-sketch hit");
    assert_eq!(top.path, "src/main.rs");
    assert_eq!(top.line, 7);
    assert_eq!(top.excerpt, "server bootstrap runtime");

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_prefers_exact_php_test_harness_excerpt() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-php-test-harness-excerpt");
    prepare_workspace(
        &root,
        &[
            (
                "tests/CreatesApplication.php",
                "<?php\n\ntrait CreatesApplication {}\n",
            ),
            (
                "tests/DuskTestCase.php",
                "<?php\n\nabstract class DuskTestCase {}\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let query = "tests fixtures integration tests createsapplication dusktestcase";
    let intent = HybridRankingIntent::from_query(query);
    let output = searcher.search_path_witness_recall_with_filters(
        query,
        &SearchFilters::default(),
        8,
        &intent,
    )?;

    let creates_application = output
        .matches
        .iter()
        .find(|entry| entry.path == "tests/CreatesApplication.php")
        .expect("CreatesApplication path witness should be returned");
    let dusk_test_case = output
        .matches
        .iter()
        .find(|entry| entry.path == "tests/DuskTestCase.php")
        .expect("DuskTestCase path witness should be returned");

    assert!(
        creates_application.excerpt.contains("CreatesApplication"),
        "path witness recall should choose the exact harness line, got {:?}",
        creates_application.excerpt
    );
    assert!(
        dusk_test_case.excerpt.contains("DuskTestCase"),
        "path witness recall should choose the exact harness line, got {:?}",
        dusk_test_case.excerpt
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_uses_live_entrypoint_detection_for_stale_typescript_projections()
-> FriggResult<()> {
    let query = "entry point bootstrap server app cli router main";
    let intent = HybridRankingIntent::from_query(query);
    let query_context = HybridPathWitnessQueryContext::from_query_text(query);
    let mut stale_entrypoint = StoredPathWitnessProjection::from_path("packages/cli/src/server.ts");
    stale_entrypoint.flags.is_entrypoint_runtime = false;
    let competing_router = StoredPathWitnessProjection::from_path(
        "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
    );

    let stale_score = hybrid_path_witness_recall_score_for_projection(
        "packages/cli/src/server.ts",
        &stale_entrypoint,
        &intent,
        &query_context,
    )
    .expect("live path detection should recover stale TypeScript entrypoint projections");
    let router_score = hybrid_path_witness_recall_score_for_projection(
        "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
        &competing_router,
        &intent,
        &query_context,
    )
    .expect("router path should still receive a score from query overlap");

    assert!(
        stale_score > router_score,
        "canonical src/server.ts should outrank non-src router noise even when the stored projection is stale"
    );
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_uses_live_roc_entrypoint_detection_for_stale_projections()
-> FriggResult<()> {
    let query = "entry point main app package platform runtime";
    let intent = HybridRankingIntent::from_query(query);
    let query_context = HybridPathWitnessQueryContext::from_query_text(query);
    let mut stale_entrypoint = StoredPathWitnessProjection::from_path("platform/main.roc");
    stale_entrypoint.flags.is_entrypoint_runtime = false;
    let competing_host_lib = StoredPathWitnessProjection::from_path("crates/roc_host/src/lib.rs");

    let stale_score = hybrid_path_witness_recall_score_for_projection(
        "platform/main.roc",
        &stale_entrypoint,
        &intent,
        &query_context,
    )
    .expect("live path detection should recover stale Roc platform entrypoints");
    let host_lib_score = hybrid_path_witness_recall_score_for_projection(
        "crates/roc_host/src/lib.rs",
        &competing_host_lib,
        &intent,
        &query_context,
    )
    .expect("host runtime libraries should still receive a score from query overlap");

    assert!(
        stale_score > host_lib_score,
        "platform/main.roc should outrank generic host runtime libraries even when the stored Roc projection is stale"
    );
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_uses_live_kotlin_entrypoint_detection_for_stale_projections()
-> FriggResult<()> {
    let query = "entry point bootstrap app activity navigation main cli";
    let intent = HybridRankingIntent::from_query(query);
    let query_context = HybridPathWitnessQueryContext::from_query_text(query);
    let mut stale_entrypoint = StoredPathWitnessProjection::from_path(
        "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoActivity.kt",
    );
    stale_entrypoint.flags.is_entrypoint_runtime = false;
    let competing_runtime = StoredPathWitnessProjection::from_path(
        "app/src/main/java/com/example/android/architecture/blueprints/todoapp/util/CoroutinesUtils.kt",
    );

    let stale_score = hybrid_path_witness_recall_score_for_projection(
        "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoActivity.kt",
        &stale_entrypoint,
        &intent,
        &query_context,
    )
    .expect("live path detection should recover stale Kotlin entrypoint projections");
    let competing_score = hybrid_path_witness_recall_score_for_projection(
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/util/CoroutinesUtils.kt",
            &competing_runtime,
            &intent,
            &query_context,
        )
        .expect("runtime-adjacent Kotlin modules should still receive a score from query overlap");

    assert!(
        stale_score > competing_score,
        "Android activity entrypoints should outrank generic Kotlin utility modules even when the stored projection is stale"
    );
    Ok(())
}

#[test]
fn hybrid_path_witness_recall_uses_live_pytest_detection_for_stale_python_test_projections()
-> FriggResult<()> {
    let query = "tests fixtures integration helpers e2e config setup pyproject";
    let intent = HybridRankingIntent::from_query(query);
    let query_context = HybridPathWitnessQueryContext::from_query_text(query);
    let mut stale_test = StoredPathWitnessProjection::from_path(
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
    );
    stale_test.flags.is_python_test_witness = false;
    stale_test.flags.is_test_support = false;
    stale_test.source_class = HybridSourceClass::Project;
    let generic_server =
        StoredPathWitnessProjection::from_path("autogpt_platform/backend/backend/server.py");

    let stale_score = hybrid_path_witness_recall_score_for_projection(
        "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
        &stale_test,
        &intent,
        &query_context,
    )
    .expect("live path detection should recover stale pytest projections");
    let generic_server_score = hybrid_path_witness_recall_score_for_projection(
        "autogpt_platform/backend/backend/server.py",
        &generic_server,
        &intent,
        &query_context,
    );

    assert!(
        stale_score > 0.0,
        "stale pytest projections should still receive a live witness score"
    );
    assert!(
        generic_server_score.is_none(),
        "non-test runtime helpers without query overlap should not be recalled for the packet query"
    );
    Ok(())
}
