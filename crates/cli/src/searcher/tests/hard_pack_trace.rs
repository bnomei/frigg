use serde::Serialize;

use crate::domain::{EvidenceChannel, FriggError, FriggResult};
use crate::searcher::policy;
use crate::searcher::{
    HybridRankedEvidence, HybridRankingIntent, SearchFilters, SearchHybridExecutionOutput,
    SearchHybridQuery, TextSearcher,
};
use crate::{settings::FriggConfig, settings::SemanticRuntimeCredentials};

use super::{
    HybridChannelWeights, PanicSemanticQueryEmbeddingExecutor, cleanup_workspace,
    prepare_workspace, temp_workspace_root,
};

#[derive(Debug, Serialize)]
struct HardPackTraceCase {
    #[serde(rename = "case_id")]
    case_id: &'static str,
    query_text: String,
    limit: usize,
    workspace_root: String,
    lexical_hits: Vec<HitSnapshot>,
    path_witness_hits: Vec<HitSnapshot>,
    ranked_hits: Vec<RankedHitSnapshot>,
    candidate_traces: Vec<CandidateTraceSnapshot>,
}

#[derive(Debug, Serialize)]
struct HitSnapshot {
    rank: usize,
    repository_id: String,
    path: String,
    line: usize,
    column: usize,
    score: f32,
    channel: String,
    excerpt: String,
}

#[derive(Debug, Serialize)]
struct RankedHitSnapshot {
    rank: usize,
    repository_id: String,
    path: String,
    line: usize,
    column: usize,
    blended_score: f32,
    lexical_score: f32,
    graph_score: f32,
    semantic_score: f32,
    excerpt: String,
}

#[derive(Debug, Serialize)]
struct CandidateTraceSnapshot {
    rank: usize,
    path: String,
    selection_rules: Vec<String>,
    path_witness_rules: Vec<String>,
    path_quality_rules: Vec<String>,
}

fn collect_channel_hits(
    output: &SearchHybridExecutionOutput,
    channel: EvidenceChannel,
    limit: usize,
) -> Vec<HitSnapshot> {
    output
        .channel_results
        .iter()
        .find(|result| result.channel == channel)
        .map(|result| {
            result
                .hits
                .iter()
                .take(limit)
                .enumerate()
                .map(|(index, hit)| HitSnapshot {
                    rank: index + 1,
                    repository_id: hit.document.repository_id.clone(),
                    path: hit.document.path.clone(),
                    line: hit.document.line,
                    column: hit.document.column,
                    score: hit.raw_score,
                    channel: hit.channel.as_str().to_owned(),
                    excerpt: hit.excerpt.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn collect_ranked_hits(
    output: &SearchHybridExecutionOutput,
    limit: usize,
) -> Vec<RankedHitSnapshot> {
    output
        .matches
        .iter()
        .take(limit)
        .enumerate()
        .map(|(index, hit)| RankedHitSnapshot {
            rank: index + 1,
            repository_id: hit.document.repository_id.clone(),
            path: hit.document.path.clone(),
            line: hit.document.line,
            column: hit.document.column,
            blended_score: hit.blended_score,
            lexical_score: hit.lexical_score,
            graph_score: hit.graph_score,
            semantic_score: hit.semantic_score,
            excerpt: hit.excerpt.clone(),
        })
        .collect()
}

fn run_candidate_traces(
    matches: &[HybridRankedEvidence],
    intent: &HybridRankingIntent,
    query_text: &str,
    limit: usize,
) -> Vec<CandidateTraceSnapshot> {
    let mut selected: Vec<HybridRankedEvidence> = Vec::new();
    let mut traces = Vec::new();

    for (index, candidate) in matches.iter().take(limit).enumerate() {
        let path = &candidate.document.path;
        traces.push(CandidateTraceSnapshot {
            rank: index + 1,
            path: path.clone(),
            selection_rules: policy::selection_rule_trace(
                candidate.clone(),
                &selected,
                intent,
                query_text,
            ),
            path_witness_rules: policy::path_witness_rule_trace(path, intent, query_text),
            path_quality_rules: policy::path_quality_rule_trace(path, intent),
        });
        selected.push(candidate.clone());
    }

    traces
}

fn run_hard_pack_trace_case(
    case_id: &'static str,
    workspace_name: &str,
    files: &[(&str, &str)],
    query: &str,
    limit: usize,
) -> FriggResult<HardPackTraceCase> {
    let root = temp_workspace_root(workspace_name);
    prepare_workspace(&root, files)?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let intent = HybridRankingIntent::from_query(query);

    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: query.to_owned(),
            limit,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let lexical_hits = collect_channel_hits(&output, EvidenceChannel::LexicalManifest, 10);
    let path_witness_hits = collect_channel_hits(&output, EvidenceChannel::PathSurfaceWitness, 10);
    let ranked_hits = collect_ranked_hits(&output, 10);
    let candidate_traces = run_candidate_traces(&output.matches, &intent, query, 10);

    cleanup_workspace(&root);

    Ok(HardPackTraceCase {
        case_id,
        query_text: query.to_owned(),
        limit,
        workspace_root: root.to_string_lossy().into_owned(),
        lexical_hits,
        path_witness_hits,
        ranked_hits,
        candidate_traces,
    })
}

fn run_and_print_case(trace: HardPackTraceCase) -> FriggResult<()> {
    let payload = serde_json::to_string_pretty(&trace)
        .map_err(|error| FriggError::Internal(error.to_string()))?;
    println!("hard_pack_trace={payload}");
    Ok(())
}

#[test]
#[ignore = "manual hard-pack trace harness"]
fn hard_pack_trace_n8n_editor_queries_demote_ci_workflow_noise() -> FriggResult<()> {
    let trace = run_hard_pack_trace_case(
        "hard_pack_n8n_editor_queries_demote_ci_workflow_noise",
        "hard-pack-n8n-editor-vs-workflow-noise",
        &[
            (
                "packages/editor-ui/src/components/canvas/NodeDetails.vue",
                "export const canvasNodeDetails = 'editor ui vue canvas workflow node details playwright';\n",
            ),
            (
                "packages/editor-ui/cypress/e2e/canvas/node-details.cy.ts",
                "describe('editor ui vue canvas workflow node details playwright', () => {});\n",
            ),
            (
                "packages/core/src/workflow_runner.ts",
                "export const workflowRunner = 'workflow runtime';\n",
            ),
            (
                ".github/workflows/build-base-image.yml",
                "name: build image\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/test-workflow-scripts-reusable.yml",
                "name: reusable workflow\njobs:\n  test:\n    steps:\n      - run: pnpm test\n",
            ),
        ],
        "editor ui vue canvas workflow node details playwright",
        5,
    )?;
    run_and_print_case(trace)
}

#[test]
#[ignore = "manual hard-pack trace harness"]
fn hard_pack_trace_supabase_runtime_queries_demote_repo_meta_and_template_noise() -> FriggResult<()>
{
    let trace = run_hard_pack_trace_case(
        "hard_pack_supabase_runtime_queries_demote_repo_meta_and_template_noise",
        "hard-pack-supabase-runtime-vs-meta-noise",
        &[
            (
                "supabase/functions/hello/index.ts",
                "export const hello = () => 'edge functions self hosted api runtime docker typescript';\n",
            ),
            (
                "apps/studio/pages/dashboard.tsx",
                "export default function Dashboard() { return <div>edge functions self hosted api runtime docker typescript</div>; }\n",
            ),
            (
                "apps/studio/tests/e2e/dashboard.spec.ts",
                "test('edge functions self hosted api runtime docker typescript', async () => {});\n",
            ),
            (
                "examples/auth/nextjs-full/lib/supabase/server.ts",
                "export const exampleServer = 'edge functions self hosted api runtime docker typescript';\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export const templateServer = 'edge functions self hosted api runtime docker typescript';\n",
            ),
            (
                "DEVELOPERS.md",
                "# Developers\nedge functions self hosted api runtime docker typescript\n",
            ),
            (
                "CONTRIBUTING.md",
                "# Contributing\nedge functions self hosted api runtime docker typescript\n",
            ),
            ("Makefile", "docker:\n\tdocker compose up\n"),
        ],
        "edge functions self hosted api runtime docker typescript",
        6,
    )?;
    run_and_print_case(trace)
}

#[test]
#[ignore = "manual hard-pack trace harness"]
fn hard_pack_trace_graphite_editor_subtree() -> FriggResult<()> {
    let trace = run_hard_pack_trace_case(
        "hard_pack_graphite_editor_subtree",
        "hard-pack-graphite-editor-subtree",
        &[
            (
                "editor/src/messages/panels/layers.rs",
                "pub fn canvas_panel_runtime() { let _ = \"editor panels canvas runtime\"; }\n",
            ),
            ("editor/tests/canvas_runtime.rs", "mod canvas_runtime {}\n"),
            (
                "node-graph/src/runtime.rs",
                "pub fn node_graph_runtime() { let _ = \"node graph runtime\"; }\n",
            ),
            (
                "desktop/wrapper/src/messages.rs",
                "pub fn desktop_wrapper_messages() { let _ = \"graphite editor panels canvas layout messages desktop wrapper svelte\"; }\n",
            ),
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"editor\", \"desktop\"]\n",
            ),
            ("Cargo.lock", "[[package]]\nname = \"graphite\"\n"),
            (
                "website/content/editor.md",
                "# Editor runtime\neditor panels canvas runtime\n",
            ),
        ],
        "graphite editor panels canvas layout messages desktop wrapper svelte",
        5,
    )?;
    run_and_print_case(trace)
}

#[test]
#[ignore = "manual hard-pack trace harness"]
fn hard_pack_trace_ruff_queries_keep_runtime_surfaces_above_docs_and_readme() -> FriggResult<()> {
    let trace = run_hard_pack_trace_case(
        "hard_pack_ruff_runtime_vs_docs",
        "hard-pack-ruff-runtime-vs-docs",
        &[
            (
                "crates/ruff_server/src/lib.rs",
                "pub fn formatter_server() { let _ = \"formatter server wasm flow\"; }\n",
            ),
            (
                "crates/ruff_wasm/src/lib.rs",
                "pub fn formatter_wasm() { let _ = \"formatter server wasm flow\"; }\n",
            ),
            ("README.md", "# Ruff\nformatter server wasm flow overview\n"),
            (
                "docs/formatter.md",
                "# Formatter\nformatter server wasm flow guide\n",
            ),
            (
                "CONTRIBUTING.md",
                "# Contributing\nformatter server wasm flow guide\n",
            ),
            ("Cargo.lock", "[[package]]\nname = \"ruff\"\n"),
        ],
        "formatter server wasm flow rust runtime",
        5,
    )?;
    run_and_print_case(trace)
}
