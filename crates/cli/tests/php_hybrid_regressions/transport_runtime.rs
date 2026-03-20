use super::*;

#[test]
fn php_hybrid_transport_onboarding_queries_surface_readme_witnesses() {
    let workspace_root = temp_workspace_root("onboarding-readme");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Quickstart\nInstall and setup the calculator server.\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder wiring discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            ("docs/install.md", "# Install\nCalculator setup guide.\n"),
            ("src/Calculator.php", "<?php\nclass Calculator {}\n"),
            (
                "src/Server.php",
                "<?php\nclass Server { public function boot(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "examples/server.php",
                "<?php\n// calculator quickstart setup example server\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "quickstart setup calculator", 5);

    assert!(
        paths.iter().any(|path| path == "README.md"),
        "expected onboarding query to surface README witness, got {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path == "examples/server.php"),
        "expected onboarding query to surface concrete example server witness, got {paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_exact_anchor_graph_hits_surface_php_implementer_witnesses() {
    let workspace_root = temp_workspace_root("exact-anchor-graph-implementers");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/ProviderInterface.php",
                "<?php\ninterface ProviderInterface {}\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "docs/completion-overview.md",
                "completion providers completion providers completion providers\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "ProviderInterface completion providers", 5);
    let implementation = output
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/ListCompletionProvider.php")
        .expect("implementation runtime witness should surface in top-k");

    assert!(
        implementation.graph_score > 0.0,
        "implementation runtime witness should receive graph evidence once bounded exact-anchor expansion is active: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_exact_anchor_prefers_canonical_runtime_symbol_over_reference_sibling() {
    let workspace_root = temp_workspace_root("canonical-vs-reference");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/Prompt.php", "<?php\nclass Prompt {}\n"),
            (
                "src/PromptReference.php",
                "<?php\nclass PromptReference {\n\
                 public Prompt $primaryPrompt;\n\
                 public Prompt $fallbackPrompt;\n\
                 public Prompt $activePrompt;\n\
                 }\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "Prompt", 3);

    assert_eq!(
        output.matches[0].document.path, "src/Prompt.php",
        "the canonical runtime definition should outrank its noisier reference sibling: {:?}",
        output.matches
    );
    assert!(
        output.matches[0].graph_score > 0.0,
        "the canonical definition should receive graph evidence for exact-anchor queries: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_onboarding_queries_surface_readme_witnesses() {
    let workspace_root = temp_workspace_root("onboarding-readme");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Quickstart\nUse StdioTransport for setup and install the server wiring.\n",
            ),
            (
                "examples/server.php",
                "<?php\n// stdio transport setup install configure server example\n",
            ),
            ("src/StdioTransport.php", "<?php\nclass StdioTransport {}\n"),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "quickstart stdio transport setup install", 3);
    eprintln!("{:#?}", output.matches);
    let paths = output
        .matches
        .iter()
        .map(|matched| matched.document.path.clone())
        .collect::<Vec<_>>();

    assert!(
        paths.iter().any(|path| path == "README.md"),
        "onboarding queries should surface the README witness in top-k: {paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_initialize_queries_surface_runtime_and_example_witnesses() {
    let workspace_root = temp_workspace_root("initialize-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "docs/initialize.md",
                "initialize server capabilities initialize result initialize server capabilities initialize result\n",
            ),
            (
                "composer.json",
                "{\n  \"description\": \"initialize server capabilities initialize result initialize server capabilities\"\n}\n",
            ),
            (
                "src/ServerCapabilities.php",
                "<?php\nclass ServerCapabilities {}\n",
            ),
            (
                "src/InitializeResult.php",
                "<?php\nclass InitializeResult {}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server {\n    public function initialize(): InitializeResult {\n        return new InitializeResult();\n    }\n}\n",
            ),
            (
                "examples/initialize-server.php",
                "<?php\n// initialize server wiring example\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "initialize server capabilities result", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-initialize",
                &[
                    "src/InitializeResult.php",
                    "src/ServerCapabilities.php",
                    "src/Server.php",
                ],
            ),
            ("support-example", &["examples/initialize-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_subscription_queries_surface_runtime_and_test_witnesses() {
    let workspace_root = temp_workspace_root("subscriptions-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "docs/subscriptions.md",
                "resource updated subscription notification resource updated subscription notification\n",
            ),
            (
                "composer.json",
                "{\n  \"description\": \"resource updated subscription notification resource updated\"\n}\n",
            ),
            (
                "src/SessionSubscriptionManager.php",
                "<?php\n// subscription manager runtime witness\nclass SessionSubscriptionManager {}\n",
            ),
            (
                "src/SubscribeRequest.php",
                "<?php\n// subscription request runtime witness\nclass SubscribeRequest {}\n",
            ),
            (
                "src/ResourceUpdatedNotification.php",
                "<?php\n// resource updated notification runtime witness\nclass ResourceUpdatedNotification {}\n",
            ),
            (
                "tests/ResourceUpdatedNotificationTest.php",
                "<?php\n// resource updated notification test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "resource updated subscription notification", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-subscriptions",
                &[
                    "src/SessionSubscriptionManager.php",
                    "src/SubscribeRequest.php",
                    "src/ResourceUpdatedNotification.php",
                ],
            ),
            (
                "tests-subscriptions",
                &["tests/ResourceUpdatedNotificationTest.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_custom_handler_queries_surface_runtime_and_example_witnesses() {
    let workspace_root = temp_workspace_root("custom-handler-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "docs/handlers.md",
                "custom method handlers custom method handlers custom method handlers\n",
            ),
            (
                "composer.json",
                "{\n  \"description\": \"custom method handlers custom method handlers\"\n}\n",
            ),
            (
                "src/CustomMethodHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass CustomMethodHandler {}\n",
            ),
            (
                "src/HealthCheckHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass HealthCheckHandler extends CustomMethodHandler {}\n",
            ),
            (
                "examples/custom-method-server.php",
                "<?php\n// custom method handlers example server\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "custom method handlers", 4);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-handler",
                &["src/CustomMethodHandler.php", "src/HealthCheckHandler.php"],
            ),
            ("support-example", &["examples/custom-method-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_initialize_queries_surface_runtime_and_support_witnesses() {
    let workspace_root = temp_workspace_root("initialize-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Initialize\ninitialize server capabilities example initialize result initialize server capabilities example\n",
            ),
            (
                "docs/index.md",
                "# Initialize\ninitialize server capabilities example initialize result initialize server capabilities example\n",
            ),
            (
                "docs/overview.md",
                "# Overview\ninitialize server capabilities example initialize result initialize server capabilities example\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"initialize server capabilities example initialize result initialize server capabilities example\"\n}\n",
            ),
            (
                "src/InitializeResult.php",
                "<?php\n// initialize result runtime witness\nclass InitializeResult {}\n",
            ),
            (
                "src/ServerCapabilities.php",
                "<?php\n// initialize server capabilities runtime witness\nclass ServerCapabilities {}\n",
            ),
            (
                "examples/initialize.php",
                "<?php\n// initialize server capabilities example runtime wiring\n",
            ),
            (
                "tests/InitializeResultTest.php",
                "<?php\n// initialize result example test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "initialize server capabilities example", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-initialize",
                &["src/InitializeResult.php", "src/ServerCapabilities.php"],
            ),
            (
                "support-initialize",
                &["examples/initialize.php", "tests/InitializeResultTest.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_completion_provider_queries_surface_runtime_and_example_witnesses() {
    let workspace_root = temp_workspace_root("completion-provider-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Completion Providers\ncompletion providers example completion providers example completion providers example\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "docs/index.md",
                "# Completion Providers\ncompletion providers example completion providers example completion providers example\n",
            ),
            (
                "docs/overview.md",
                "# Providers\ncompletion providers example completion providers example completion providers example\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"completion providers example completion providers example\"\n}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function build(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/ProviderInterface.php",
                "<?php\n// completion providers runtime witness\ninterface ProviderInterface {}\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\n// completion providers runtime witness\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "examples/providers.php",
                "<?php\n// completion providers example runtime wiring\n",
            ),
            (
                "tests/ListCompletionProviderTest.php",
                "<?php\n// completion providers example test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "completion providers example", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-provider",
                &[
                    "src/ProviderInterface.php",
                    "src/ListCompletionProvider.php",
                ],
            ),
            (
                "support-provider",
                &[
                    "examples/providers.php",
                    "tests/ListCompletionProviderTest.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_subscription_queries_surface_runtime_update_witnesses() {
    let workspace_root = temp_workspace_root("subscription-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Resource Subscriptions\nresource subscriptions notification resource updated resource subscriptions notification resource updated\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "docs/index.md",
                "# Resource Subscriptions\nresource subscriptions notification resource updated resource subscriptions notification resource updated\n",
            ),
            (
                "docs/overview.md",
                "# Overview\nresource subscriptions notification resource updated resource subscriptions notification resource updated\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"resource subscriptions notification resource updated resource subscriptions notification resource updated\"\n}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function initialize(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/SessionSubscriptionManager.php",
                "<?php\n// resource subscriptions runtime witness\nclass SessionSubscriptionManager {}\n",
            ),
            (
                "src/SubscribeRequest.php",
                "<?php\n// resource subscriptions request runtime witness\nclass SubscribeRequest {}\n",
            ),
            (
                "src/ResourceUpdatedNotification.php",
                "<?php\n// resource updated notification runtime witness\nclass ResourceUpdatedNotification {}\n",
            ),
            (
                "examples/resource-updates.php",
                "<?php\n// resource updated notification example wiring\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "resource subscriptions notification resource updated",
        5,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-subscriptions",
                &[
                    "src/SessionSubscriptionManager.php",
                    "src/SubscribeRequest.php",
                    "src/ResourceUpdatedNotification.php",
                ],
            ),
            ("support-subscriptions", &["examples/resource-updates.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_custom_handler_queries_surface_handler_and_example_witnesses() {
    let workspace_root = temp_workspace_root("custom-handler-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Custom Method Handlers\ncustom method handlers example custom method handlers example custom method handlers example\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "docs/index.md",
                "# Custom Method Handlers\ncustom method handlers example custom method handlers example custom method handlers example\n",
            ),
            (
                "docs/overview.md",
                "# Overview\ncustom method handlers example custom method handlers example custom method handlers example\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"custom method handlers example custom method handlers example\"\n}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function wire(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/CustomMethodHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass CustomMethodHandler {}\n",
            ),
            (
                "src/CustomNotificationHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass CustomNotificationHandler {}\n",
            ),
            (
                "examples/custom-handlers.php",
                "<?php\n// custom method handlers example wiring\n",
            ),
            (
                "tests/CustomMethodHandlerTest.php",
                "<?php\n// custom method handlers example test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "custom method handlers example", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-handlers",
                &[
                    "src/CustomMethodHandler.php",
                    "src/CustomNotificationHandler.php",
                ],
            ),
            (
                "support-handlers",
                &[
                    "examples/custom-handlers.php",
                    "tests/CustomMethodHandlerTest.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_client_communication_queries_surface_gateway_and_example_witnesses() {
    let workspace_root = temp_workspace_root("client-communication-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Client Communication\nclient communication back to client client communication back to client\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function run(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/ClientGateway.php",
                "<?php\n// client communication runtime witness\nclass ClientGateway {}\n",
            ),
            (
                "examples/client-aware-server.php",
                "<?php\n// client communication example witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "client communication back to client", 5);

    assert_witness_groups(
        &paths,
        &[
            ("runtime-client", &["src/ClientGateway.php"]),
            ("support-client", &["examples/client-aware-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_conformance_queries_surface_test_and_transport_witnesses() {
    let workspace_root = temp_workspace_root("conformance-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Conformance\nconformance inspector stdio http conformance inspector stdio http\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function serve(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "tests/Conformance/server.php",
                "<?php\n// conformance stdio server witness\n",
            ),
            (
                "tests/Inspector/http_transport_test.php",
                "<?php\n// inspector http transport witness\n",
            ),
            (
                "examples/stdio-server.php",
                "<?php\n// stdio server example witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "conformance inspector stdio http", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "conformance-test",
                &[
                    "tests/Conformance/server.php",
                    "tests/Inspector/http_transport_test.php",
                ],
            ),
            ("support-transport", &["examples/stdio-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_graph_channel_uses_php_target_evidence_edges() {
    let workspace_root = temp_workspace_root("target-evidence-graph-edges");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                 namespace App\\Handlers;\n\
                 class OrderHandler {\n\
                     public function handle(): void {}\n\
                 }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                 namespace App\\Listeners;\n\
                 use App\\Handlers\\OrderHandler;\n\
                 class OrderListener {\n\
                     public function handlers(): array {\n\
                         return [[OrderHandler::class, 'handle']];\n\
                     }\n\
                 }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder handler listener overview.\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "OrderHandler handle listener", 5);
    let listener_match = output
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/Listeners/OrderListener.php")
        .unwrap_or_else(|| {
            panic!(
                "listener runtime witness should surface in top-k; matches={:?}; ranked_anchors={:?}; channel_results={:?}",
                output.matches,
                output.ranked_anchors,
                output.channel_results
            )
        });

    assert!(
        listener_match.graph_score > 0.0,
        "callable-literal target evidence should add graph score for listener witness: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_graph_channel_seeds_from_canonical_runtime_paths_without_exact_symbol_terms() {
    let workspace_root = temp_workspace_root("canonical-runtime-path-seeds");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/handlers/OrderHandler.php",
                "<?php\n\
                 namespace App\\Handlers;\n\
                 class OrderHandler {\n\
                     public function handle(): void {}\n\
                 }\n",
            ),
            (
                "src/handlers/OrderDispatcher.php",
                "<?php\n\
                 namespace App\\Handlers;\n\
                 class OrderDispatcher {\n\
                     public function dispatch(): void {}\n\
                 }\n",
            ),
            (
                "src/Domain/Orders/OrderHandler.php",
                "<?php\n\
                 namespace App\\Domain\\Orders;\n\
                 class OrderHandler {\n\
                     public function handle(): void {}\n\
                 }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder handler runtime overview.\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "order handle runtime", 5);
    assert!(
        output
            .matches
            .iter()
            .any(|entry| entry.document.path == "src/handlers/OrderHandler.php"),
        "canonical runtime path should still seed graph channel matches: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}
