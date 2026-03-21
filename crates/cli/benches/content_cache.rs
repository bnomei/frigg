#[path = "common/mod.rs"]
mod support;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::mcp::types::{
    ExploreOperation, ExploreParams, ExploreResponse, ReadFileParams, ReadFileResponse,
    ReadPresentationMode, SearchPatternType,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;

fn structured_tool_result<T: serde::de::DeserializeOwned>(result: CallToolResult) -> T {
    let structured = result
        .structured_content
        .expect("benchmark tool call should return structured content");
    serde_json::from_value(structured).expect("benchmark structured_content should deserialize")
}

fn bench_content_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_cache");

    group.bench_function(
        BenchmarkId::from_parameter("read_file_cold_window_end_to_end"),
        |b| {
            b.iter_batched(
                || {
                    let root = support::fresh_fixture_root("read-file-cold");
                    support::attached_server_session(support::native_search_config(&root), &root)
                },
                |session| {
                    let response = session
                        .runtime
                        .block_on(session.server.read_file(Parameters(ReadFileParams {
                            path: "src/module_000.rs".to_owned(),
                            repository_id: Some(session.repository_id.clone()),
                            max_bytes: Some(2_048),
                            line_start: Some(1),
                            line_end: Some(8),
                            presentation_mode: Some(ReadPresentationMode::Json),
                        })))
                        .expect("cold read_file benchmark should succeed");
                    let response: ReadFileResponse = structured_tool_result(response);
                    criterion::black_box(response.bytes);
                },
                BatchSize::SmallInput,
            );
        },
    );

    let hot_session = support::attached_fixture_server_session();
    group.bench_function(BenchmarkId::from_parameter("read_file_hot_window"), |b| {
        b.iter(|| {
            let response = hot_session
                .runtime
                .block_on(hot_session.server.read_file(Parameters(ReadFileParams {
                    path: "src/module_000.rs".to_owned(),
                    repository_id: Some(hot_session.repository_id.clone()),
                    max_bytes: Some(2_048),
                    line_start: Some(1),
                    line_end: Some(8),
                    presentation_mode: Some(ReadPresentationMode::Json),
                })))
                .expect("hot read_file benchmark should succeed");
            let response: ReadFileResponse = structured_tool_result(response);
            criterion::black_box(response.bytes);
        });
    });

    group.bench_function(BenchmarkId::from_parameter("explore_hot_probe"), |b| {
        b.iter(|| {
            let response = hot_session
                .runtime
                .block_on(hot_session.server.explore(Parameters(ExploreParams {
                    path: "src/module_000.rs".to_owned(),
                    repository_id: Some(hot_session.repository_id.clone()),
                    operation: ExploreOperation::Probe,
                    query: Some("handle_checkout_request".to_owned()),
                    pattern_type: Some(SearchPatternType::Literal),
                    anchor: None,
                    context_lines: Some(2),
                    max_matches: Some(8),
                    resume_from: None,
                    presentation_mode: Some(ReadPresentationMode::Json),
                })))
                .expect("hot explore benchmark should succeed");
            let response: ExploreResponse = structured_tool_result(response);
            criterion::black_box(response.total_matches);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_content_cache);
criterion_main!(benches);
