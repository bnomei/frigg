use super::*;
use frigg::mcp::types::{ExploreResponse, ReadFileResponse, ReadMatchResponse};

#[tokio::test]
async fn core_read_file_returns_typed_not_found_error() {
    let server = server_for_fixture();
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "missing-file.txt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: None,
        }))
        .await
    {
        Ok(_) => panic!("missing file should return typed error"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(error_code_tag(&error), Some("resource_not_found"));
    assert_eq!(retryable_tag(&error), Some(false));
}

#[tokio::test]
async fn core_read_file_returns_repository_relative_canonical_path() {
    let workspace_root = fresh_fixture_root("tool-handlers-core-read-file");
    let server = server_for_workspace_root(&workspace_root);
    let repository_id = public_repository_id(&server).await;
    let absolute_path = workspace_root.join("src/lib.rs");
    let absolute_response = server
        .read_file(Parameters(ReadFileParams {
            path: absolute_path.display().to_string(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: Some(ReadPresentationMode::Json),
        }))
        .await
        .map(structured_tool_result::<ReadFileResponse>)
        .expect("absolute read_file path under workspace root should resolve");
    assert_eq!(absolute_response.repository_id, repository_id);
    assert_eq!(absolute_response.path, "src/lib.rs");
    assert!(
        !Path::new(&absolute_response.path).is_absolute(),
        "read_file path contract must be repository-relative"
    );

    let relative_response = server
        .read_file(Parameters(ReadFileParams {
            path: "./src/../src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: Some(ReadPresentationMode::Json),
        }))
        .await
        .map(structured_tool_result::<ReadFileResponse>)
        .expect("relative read_file path under workspace root should resolve");
    assert_eq!(
        relative_response.repository_id,
        public_repository_id(&server).await
    );
    assert_eq!(relative_response.path, "src/lib.rs");
    assert_eq!(relative_response.path, absolute_response.path);
}

#[tokio::test]
async fn core_read_file_supports_line_range_slicing() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let response = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(128),
            line_start: Some(2),
            line_end: Some(2),
            presentation_mode: Some(ReadPresentationMode::Json),
        }))
        .await
        .map(structured_tool_result::<ReadFileResponse>)
        .expect("line-range slice should succeed");

    assert_eq!(response.repository_id, repository_id);
    assert_eq!(response.path, "src/lib.rs");
    assert_eq!(response.content, "    \"hello from fixture\"");
    assert_eq!(response.bytes, response.content.len());
}

#[tokio::test]
async fn core_read_file_defaults_to_text_first_output() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let result = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(128),
            line_start: Some(2),
            line_end: Some(2),
            presentation_mode: None,
        }))
        .await
        .expect("default read_file should succeed");

    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("repository_id"))
            .and_then(|value| value.as_str()),
        Some(repository_id.as_str())
    );
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("path"))
            .and_then(|value| value.as_str()),
        Some("src/lib.rs")
    );
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("line_start"))
            .and_then(|value| value.as_u64()),
        Some(2)
    );
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("line_end"))
            .and_then(|value| value.as_u64()),
        Some(2)
    );
    assert!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("content"))
            .is_none(),
        "default text-first read_file should not duplicate the file body in structured_content"
    );
    let text = tool_result_text(&result);
    assert!(text.contains(&format!("repository_id: {repository_id}")));
    assert!(text.contains("path: src/lib.rs"));
    assert!(text.contains("line_window: 2-2"));
    assert!(text.ends_with("    \"hello from fixture\""));
}

#[tokio::test]
async fn core_read_file_line_range_can_bypass_full_file_size_limit() {
    let workspace_root = temp_workspace_root("read-file-line-range-max-bytes");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "abcdefghijklmnopqrstuvwxyz\nok\nabcdefghijklmnopqrstuvwxyz\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(8),
            line_start: Some(2),
            line_end: Some(2),
            presentation_mode: Some(ReadPresentationMode::Json),
        }))
        .await
        .map(structured_tool_result::<ReadFileResponse>)
        .expect("line-range slice should apply max_bytes to returned slice content");

    assert_eq!(response.content, "ok");
    assert_eq!(response.bytes, 2);
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_read_file_line_range_preserves_lossy_utf8_behavior() {
    let workspace_root = temp_workspace_root("read-file-line-range-lossy-utf8");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        b"alpha\nbeta \xFF\nomega\n".as_slice(),
    )
    .expect("failed to seed temporary fixture source");

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(64),
            line_start: Some(2),
            line_end: Some(2),
            presentation_mode: Some(ReadPresentationMode::Json),
        }))
        .await
        .map(structured_tool_result::<ReadFileResponse>)
        .expect("lossy utf8 line-range slice should succeed");

    assert_eq!(response.content, "beta \u{fffd}");
    assert_eq!(response.bytes, response.content.len());
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_read_file_rejects_invalid_line_range_payload() {
    let server = server_for_fixture();
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(128),
            line_start: Some(3),
            line_end: Some(2),
            presentation_mode: None,
        }))
        .await
    {
        Ok(_) => panic!("invalid line range should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error.message, "line_end must be greater than or equal to line_start",
        "invalid read_file line ranges should preserve the typed invalid_params message"
    );
}

#[tokio::test]
async fn core_search_text_literal_scoped_to_repository() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("literal search should succeed")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.total_matches, 1);
    assert_eq!(response.matches[0].repository_id, repository_id);
    assert_eq!(response.matches[0].path, "src/lib.rs");
}

#[tokio::test]
async fn core_search_text_regex_mode_executes_regex_search() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "hello\\s+from\\s+fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Regex),
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("regex mode should execute search")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, repository_id);
    assert_eq!(response.matches[0].path, "src/lib.rs");
}

#[tokio::test]
async fn core_search_text_defaults_to_compact_and_supports_read_match_handles() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("compact search_text should succeed")
        .0;

    assert!(response.metadata.is_none());
    let handle = response
        .result_handle
        .clone()
        .expect("compact search_text should return a result handle");
    let match_id = response.matches[0]
        .match_id
        .clone()
        .expect("compact search_text matches should expose match ids");

    let opened = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: handle,
            match_id,
            before: None,
            after: None,
            presentation_mode: Some(ReadPresentationMode::Json),
        }))
        .await
        .map(structured_tool_result::<ReadMatchResponse>)
        .expect("read_match should reopen a search hit");

    assert_eq!(opened.repository_id, repository_id);
    assert_eq!(opened.path, "src/lib.rs");
    assert_eq!(opened.line, 2);
    assert_eq!(opened.column, Some(6));
    assert_eq!(opened.line_start, 1);
    assert!(opened.content.contains("pub fn greeting()"));
    assert!(opened.content.contains("\"hello from fixture\""));
}

#[tokio::test]
async fn core_read_match_defaults_to_text_first_output() {
    let server = server_for_fixture();
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("compact search_text should succeed")
        .0;
    let handle = response
        .result_handle
        .clone()
        .expect("compact search_text should return a result handle");
    let match_id = response.matches[0]
        .match_id
        .clone()
        .expect("compact search_text matches should expose match ids");

    let opened = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: handle,
            match_id,
            before: None,
            after: None,
            presentation_mode: None,
        }))
        .await
        .expect("default read_match should succeed");

    assert_eq!(
        opened
            .structured_content
            .as_ref()
            .and_then(|value| value.get("path"))
            .and_then(|value| value.as_str()),
        Some("src/lib.rs")
    );
    assert_eq!(
        opened
            .structured_content
            .as_ref()
            .and_then(|value| value.get("line_start"))
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        opened
            .structured_content
            .as_ref()
            .and_then(|value| value.get("line_end"))
            .and_then(|value| value.as_u64()),
        Some(3)
    );
    assert!(
        opened
            .structured_content
            .as_ref()
            .and_then(|value| value.get("content"))
            .is_none()
    );
    let text = tool_result_text(&opened);
    assert!(text.contains("path: src/lib.rs"));
    assert!(text.contains("line_window: 1-3"));
    assert!(text.contains("pub fn greeting()"));
    assert!(text.contains("\"hello from fixture\""));
}

#[tokio::test]
async fn core_search_text_context_lines_and_per_file_limits_shape_results() {
    let workspace_root = temp_workspace_root("search-text-context-and-per-file-limit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "fn alpha() {\n    target();\n}\n\nfn beta() {\n    target();\n}\n",
    )
    .expect("failed to seed first source file");
    fs::write(
        src_root.join("other.rs"),
        "fn gamma() {\n    target();\n}\n\nfn delta() {\n    target();\n}\n",
    )
    .expect("failed to seed second source file");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "target".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"^src/.*\.rs$".to_owned()),
            limit: Some(10),
            context_lines: Some(1),
            max_matches_per_file: Some(1),
            collapse_by_file: Some(false),
            ..Default::default()
        }))
        .await
        .expect("search_text shaping should succeed")
        .0;

    assert_eq!(response.total_matches, 4);
    assert_eq!(response.matches.len(), 2);
    assert!(response.metadata.is_none());
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.match_id.is_some()),
        "shaped compact results should still expose match ids"
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.excerpt.contains('\n')),
        "context_lines should expand inline excerpts beyond a single line"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_search_text_collapse_by_file_returns_one_hit_per_path() {
    let workspace_root = temp_workspace_root("search-text-collapse-by-file");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "fn alpha() { target(); }\nfn beta() { target(); }\n",
    )
    .expect("failed to seed first source file");
    fs::write(
        src_root.join("other.rs"),
        "fn gamma() { target(); }\nfn delta() { target(); }\n",
    )
    .expect("failed to seed second source file");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "target".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"^src/.*\.rs$".to_owned()),
            limit: Some(10),
            collapse_by_file: Some(true),
            ..Default::default()
        }))
        .await
        .expect("collapse-by-file search_text should succeed")
        .0;

    assert_eq!(response.total_matches, 4);
    assert_eq!(response.matches.len(), 2);
    let unique_paths = response
        .matches
        .iter()
        .map(|matched| matched.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique_paths.len(), 2);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_search_hybrid_returns_deterministic_matches_and_metadata_only() {
    let server = server_for_fixture();
    let repository_id = public_repository_id(&server).await;
    let first = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("search_hybrid should succeed")
        .0;
    let second = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("search_hybrid should be deterministic")
        .0;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].repository_id, repository_id);
    assert_eq!(first.matches[0].path, "src/lib.rs");
    assert_eq!(first.matches[0].line, 2);
    assert_eq!(first.matches[0].column, 6);
    assert_eq!(
        first.matches[0]
            .anchor
            .as_ref()
            .map(|anchor| anchor.start_line),
        Some(2)
    );
    assert_eq!(
        first.matches[0]
            .anchor
            .as_ref()
            .map(|anchor| anchor.start_column),
        Some(6)
    );
    assert_eq!(first.semantic_requested, None);
    assert_eq!(first.semantic_enabled, None);
    assert_eq!(first.semantic_status, None);
    assert_eq!(first.semantic_hit_count, None);
    assert_eq!(first.semantic_match_count, None);
    assert_eq!(first.semantic_reason, None);
    assert_eq!(first.warning, None);
    assert_eq!(first.note, None);
    assert!(
        first.matches[0].blended_score >= 0.0,
        "hybrid blended score should be non-negative"
    );

    let structured: serde_json::Value =
        serde_json::to_value(&first).expect("search_hybrid response should serialize");
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("requested_language"))
            .and_then(|value| value.as_str()),
        Some("rust")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("semantic_chunking"))
            .and_then(|value| value.as_str()),
        Some("optional_accelerator")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("capabilities"))
            .and_then(|value| value.get("symbol_corpus"))
            .and_then(|value| value.as_str()),
        Some("core")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("tier"))
            .and_then(|value| value.as_str()),
        Some("optional_accelerator")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str()),
        Some("disabled_by_request")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("channels"))
            .and_then(|value| value.get("lexical_manifest"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("ok")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("channels"))
            .and_then(|value| value.get("path_surface_witness"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("filtered")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("channels"))
            .and_then(|value| value.get("semantic"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("disabled")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("disabled")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_enabled"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_requested"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_candidate_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_hit_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_match_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_reason"))
            .and_then(|value| value.as_str()),
        Some("semantic channel disabled by request toggle")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("warning"))
            .and_then(|value| value.as_str()),
        Some(
            "semantic retrieval is disabled; results are ranked from lexical and graph signals only (semantic channel disabled by request toggle)"
        )
    );
    for field in [
        "semantic_requested",
        "semantic_enabled",
        "semantic_status",
        "semantic_reason",
        "semantic_hit_count",
        "semantic_match_count",
        "warning",
        "note",
    ] {
        assert!(
            structured.get(field).is_none(),
            "search_hybrid should omit duplicate top-level field `{field}` when metadata is present"
        );
    }
    assert!(
        structured
            .get("metadata")
            .and_then(|value| value.get("stage_attribution"))
            .and_then(|value| value.get("candidate_intake"))
            .and_then(|value| value.get("output_count"))
            .and_then(|value| value.as_u64())
            .is_some_and(|value| value >= 1),
        "search_hybrid metadata should expose candidate intake counts"
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("stage_attribution"))
            .and_then(|value| value.get("semantic_retrieval"))
            .and_then(|value| value.get("output_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert!(
        structured
            .get("metadata")
            .and_then(|value| value.get("stage_attribution"))
            .and_then(|value| value.get("scan"))
            .and_then(|value| value.get("elapsed_us"))
            .and_then(|value| value.as_u64())
            .is_some(),
        "search_hybrid metadata should expose additive stage attribution"
    );
    let second_metadata = second
        .metadata
        .as_ref()
        .map(|metadata| serde_json::to_value(metadata).expect("metadata should serialize"));
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        second_metadata
            .as_ref()
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        "metadata-only semantic status should remain deterministic"
    );
    let freshness_cacheable = structured
        .get("metadata")
        .and_then(|value| value.get("freshness_basis"))
        .and_then(|value| value.get("cacheable"))
        .and_then(|value| value.as_bool());
    if freshness_cacheable == Some(true) {
        assert_eq!(
            structured
                .get("metadata")
                .and_then(|value| value.get("stage_attribution")),
            second_metadata
                .as_ref()
                .and_then(|value| value.get("stage_attribution")),
            "cacheable search_hybrid responses should keep stage attribution stable within the session"
        );
    } else {
        assert!(
            second_metadata
                .as_ref()
                .and_then(|value| value.get("stage_attribution"))
                .and_then(|value| value.get("scan"))
                .and_then(|value| value.get("elapsed_us"))
                .and_then(|value| value.as_u64())
                .is_some(),
            "non-cacheable search_hybrid responses should still report stage attribution on repeated calls"
        );
    }
}

#[tokio::test]
async fn core_search_hybrid_defaults_to_compact_with_handles() {
    let server = server_for_fixture();
    let response = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
            response_mode: None,
        }))
        .await
        .expect("compact search_hybrid should succeed")
        .0;

    assert!(response.metadata.is_none());
    assert!(response.note.is_none());
    assert!(
        response.result_handle.is_some(),
        "compact search_hybrid should return a result handle"
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.match_id.is_some()),
        "compact search_hybrid matches should expose match ids"
    );
}

#[tokio::test]
async fn core_search_hybrid_code_shaped_queries_surface_exact_assistance_and_rank_reasons() {
    let workspace_root = temp_workspace_root("search-hybrid-code-shaped-assistance");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create source dir");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create tests dir");
    fs::create_dir_all(workspace_root.join(".git")).expect("failed to create git dir");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn capture_screen() -> &'static str {\n    \"capture_screen\"\n}\n",
    )
    .expect("failed to seed runtime source");
    fs::write(
        workspace_root.join("tests/capture_screen_flow.rs"),
        "#[test]\nfn smoke_test() {\n    assert!(true);\n}\n",
    )
    .expect("failed to seed witness-style test file");
    fs::write(workspace_root.join(".gitignore"), "*.tmp\n").expect("failed to seed ignore file");

    let server = server_for_workspace_root(&workspace_root);
    let full = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "capture_screen".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("full search_hybrid should succeed for code-shaped query")
        .0;

    let metadata = full
        .metadata
        .as_ref()
        .expect("full response should expose metadata");
    assert_eq!(
        metadata.query_shape,
        Some(SearchHybridQueryShape::CodeShaped)
    );
    assert_eq!(metadata.lexical_only_mode, Some(true));
    let exact_assistance = metadata
        .exact_pivot_assistance
        .as_ref()
        .expect("code-shaped lexical-only query should report exact assistance");
    assert!(exact_assistance.applied);
    assert!(exact_assistance.exact_symbol_hit_count >= 1);
    assert!(exact_assistance.exact_text_hit_count >= 1);
    assert!(exact_assistance.boosted_match_count >= 1);
    assert_eq!(full.matches[0].path, "src/lib.rs");
    assert!(
        full.matches[0]
            .rank_reasons
            .contains(&SearchHybridRankReason::ExactTextMatch)
    );
    assert!(
        full.matches[0]
            .rank_reasons
            .contains(&SearchHybridRankReason::StrongLexicalAnchor)
    );

    let compact = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "capture_screen".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
            response_mode: None,
        }))
        .await
        .expect("compact search_hybrid should succeed for code-shaped query")
        .0;

    assert!(compact.metadata.is_none());
    assert_eq!(compact.matches[0].path, "src/lib.rs");
    assert_eq!(
        compact.matches[0].rank_reasons,
        full.matches[0].rank_reasons
    );
    assert!(
        compact.matches[0].match_id.is_some(),
        "compact search_hybrid should still expose match ids"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_search_hybrid_rejects_empty_query_with_typed_invalid_params() {
    let server = server_for_fixture();
    let error = match server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "   ".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: None,
            limit: Some(10),
            weights: None,
            semantic: None,
            response_mode: Some(ResponseMode::Full),
        }))
        .await
    {
        Ok(_) => panic!("empty search_hybrid query should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error.message, "query must not be empty",
        "empty search_hybrid queries should return the typed invalid_params message"
    );
}

#[tokio::test]
async fn core_search_hybrid_surfaces_degraded_warning_when_semantic_runtime_fails_non_strict() {
    let workspace_root = fresh_fixture_root("tool-handlers-core-hybrid-degraded");
    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("fixture root must produce valid config");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(true),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("non-strict semantic startup failure should degrade, not hard-fail")
        .0;

    assert_eq!(response.semantic_requested, None);
    assert_eq!(response.semantic_enabled, None);
    assert_eq!(response.semantic_status, None);
    assert_eq!(response.semantic_hit_count, None);
    assert_eq!(response.semantic_match_count, None);
    assert_eq!(response.note, None);
    let metadata = serde_json::to_value(
        response
            .metadata
            .as_ref()
            .expect("search_hybrid should emit structured metadata"),
    )
    .expect("metadata should serialize");
    assert_eq!(
        metadata
            .get("channels")
            .and_then(|value| value.get("semantic"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("degraded")
    );
    assert_eq!(
        metadata
            .get("semantic_status")
            .and_then(|value| value.as_str()),
        Some("degraded")
    );
    assert_eq!(
        metadata
            .get("semantic_enabled")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        metadata
            .get("semantic_requested")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("semantic_candidate_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        metadata
            .get("semantic_hit_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        metadata
            .get("semantic_match_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert!(
        metadata
            .get("semantic_reason")
            .and_then(|value| value.as_str())
            .is_some_and(|reason| reason.contains("semantic runtime validation failed")),
        "degraded semantic reason should explain the validation failure"
    );
    assert!(
        metadata
            .get("warning")
            .and_then(|value| value.as_str())
            .is_some_and(|warning| warning.starts_with(
                "semantic retrieval is degraded; semantic contribution may be partial"
            )),
        "degraded search_hybrid response should emit a clear warning"
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_chunking"))
            .and_then(|value| value.as_str()),
        Some("optional_accelerator")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str()),
        Some("degraded_runtime")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("degraded")
    );
}

#[tokio::test]
async fn core_search_hybrid_marks_unsupported_semantic_language_filters_as_unavailable() {
    let workspace_root = fresh_fixture_root("tool-handlers-core-hybrid-unavailable");
    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("fixture root must produce valid config");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("typescript".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(true),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
        .expect("unsupported semantic language filters should degrade to metadata, not fail")
        .0;

    let metadata = serde_json::to_value(
        response
            .metadata
            .as_ref()
            .expect("search_hybrid should emit structured metadata"),
    )
    .expect("metadata should serialize");
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_chunking"))
            .and_then(|value| value.as_str()),
        Some("unsupported")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("capabilities"))
            .and_then(|value| value.get("symbol_corpus"))
            .and_then(|value| value.as_str()),
        Some("core")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("tier"))
            .and_then(|value| value.as_str()),
        Some("unsupported")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str()),
        Some("unsupported_language")
    );
    assert_eq!(
        metadata
            .get("semantic_status")
            .and_then(|value| value.as_str()),
        Some("unavailable")
    );
    assert_eq!(
        metadata
            .get("semantic_reason")
            .and_then(|value| value.as_str()),
        Some("requested language filter 'typescript' does not support semantic_chunking")
    );
    assert!(
        metadata
            .get("warning")
            .and_then(|value| value.as_str())
            .is_some_and(|warning| warning.contains("semantic retrieval is unavailable")),
        "unsupported semantic language filters should surface an unavailable warning"
    );
}

#[tokio::test]
async fn core_search_hybrid_strict_semantic_requires_startup_credentials() {
    let workspace_root = fresh_fixture_root("tool-handlers-core-hybrid-strict");
    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("fixture root must produce valid config");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: true,
    };
    let server = server_for_config(config);

    let error = match server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: None,
            limit: Some(10),
            weights: None,
            semantic: Some(true),
            response_mode: Some(ResponseMode::Full),
        }))
        .await
    {
        Ok(_) => panic!("strict semantic startup failure should return typed error"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(error_code_tag(&error), Some("unavailable"));
    assert_eq!(retryable_tag(&error), Some(true));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("strict_failure")
    );
}

#[tokio::test]
async fn core_search_text_rejects_abusive_path_regex_with_typed_invalid_params() {
    let server = server_for_fixture();
    let abusive_path_regex = "a".repeat(600);
    let error = match server
        .search_text(Parameters(SearchTextParams {
            query: "hello".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(abusive_path_regex.clone()),
            limit: Some(10),
            ..Default::default()
        }))
        .await
    {
        Ok(_) => panic!("abusive path_regex should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("invalid path_regex"),
        "unexpected error message: {}",
        error.message
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("path_regex"))
            .and_then(|value| value.as_str()),
        Some(abusive_path_regex.as_str())
    );
}

#[tokio::test]
async fn core_read_file_enforces_effective_max_bytes_clamp() {
    let workspace_root = temp_workspace_root("read-file-max-clamp");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(src_root.join("lib.rs"), "0123456789")
        .expect("failed to seed temporary fixture source");

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 4);
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(1024),
            line_start: None,
            line_end: None,
            presentation_mode: None,
        }))
        .await
    {
        Ok(_) => panic!("effective max clamp should reject oversized file"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("max_bytes"))
            .and_then(|value| value.as_u64()),
        Some(4)
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("suggested_max_bytes"))
            .and_then(|value| value.as_u64()),
        Some(4)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn extended_explore_probe_zoom_and_refine_are_deterministic() {
    let workspace_root = temp_workspace_root("explore-deterministic");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create explorer fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn demo() {\n\
         \x20\x20\x20\x20let needle_alpha = 1;\n\
         \x20\x20\x20\x20let helper_alpha = needle_alpha;\n\
         \x20\x20\x20\x20let needle_beta = 2;\n\
         \x20\x20\x20\x20let helper_beta = needle_beta;\n\
         \x20\x20\x20\x20let needle_gamma = 3;\n\
         }\n",
    )
    .expect("failed to seed explorer fixture");

    let server = extended_runtime_server_for_workspace_root(&workspace_root);
    let probe_params = ExploreParams {
        path: "src/lib.rs".to_owned(),
        repository_id: Some("repo-001".to_owned()),
        operation: ExploreOperation::Probe,
        query: Some("let needle_".to_owned()),
        pattern_type: Some(SearchPatternType::Literal),
        anchor: None,
        context_lines: Some(1),
        max_matches: Some(2),
        resume_from: None,
        presentation_mode: None,
    };

    let first: ExploreResponse = structured_tool_result(
        server
            .explore(Parameters(probe_params.clone()))
            .await
            .expect("explore probe should succeed"),
    );
    let second: ExploreResponse = structured_tool_result(
        server
            .explore(Parameters(probe_params))
            .await
            .expect("explore probe should be deterministic"),
    );
    assert_eq!(first, second);
    assert_eq!(first.total_lines, 7);
    assert_eq!(first.total_matches, 3);
    assert_eq!(first.matches.len(), 2);
    assert!(first.truncated);
    assert_eq!(
        first.resume_from.as_ref().map(|cursor| cursor.line),
        Some(6)
    );
    assert_eq!(
        first.resume_from.as_ref().map(|cursor| cursor.column),
        Some(5)
    );
    assert_eq!(first.matches[0].window.start_line, 1);
    assert_eq!(first.matches[0].window.end_line, 3);
    assert_eq!(first.matches[1].window.start_line, 3);
    assert_eq!(first.matches[1].window.end_line, 5);

    let resumed: ExploreResponse = structured_tool_result(
        server
            .explore(Parameters(ExploreParams {
                path: "src/lib.rs".to_owned(),
                repository_id: Some("repo-001".to_owned()),
                operation: ExploreOperation::Probe,
                query: Some("let needle_".to_owned()),
                pattern_type: Some(SearchPatternType::Literal),
                anchor: None,
                context_lines: Some(1),
                max_matches: Some(2),
                resume_from: first.resume_from.clone(),
                presentation_mode: None,
            }))
            .await
            .expect("explore probe resume should succeed"),
    );
    assert_eq!(resumed.total_matches, 1);
    assert_eq!(resumed.matches.len(), 1);
    assert!(!resumed.truncated);
    assert_eq!(resumed.matches[0].start_line, 6);

    let anchor = first.matches[1].anchor.clone();
    let zoom: ExploreResponse = structured_tool_result(
        server
            .explore(Parameters(ExploreParams {
                path: "src/lib.rs".to_owned(),
                repository_id: Some("repo-001".to_owned()),
                operation: ExploreOperation::Zoom,
                query: None,
                pattern_type: None,
                anchor: Some(anchor.clone()),
                context_lines: Some(1),
                max_matches: None,
                resume_from: None,
                presentation_mode: Some(ReadPresentationMode::Json),
            }))
            .await
            .expect("explore zoom should succeed"),
    );
    assert_eq!(zoom.total_matches, 0);
    assert!(zoom.matches.is_empty());
    assert!(!zoom.truncated);
    assert_eq!(
        zoom.window.as_ref().map(|window| window.start_line),
        Some(3)
    );
    assert_eq!(zoom.window.as_ref().map(|window| window.end_line), Some(5));

    let refine: ExploreResponse = structured_tool_result(
        server
            .explore(Parameters(ExploreParams {
                path: "src/lib.rs".to_owned(),
                repository_id: Some("repo-001".to_owned()),
                operation: ExploreOperation::Refine,
                query: Some("helper_".to_owned()),
                pattern_type: Some(SearchPatternType::Literal),
                anchor: Some(anchor),
                context_lines: Some(1),
                max_matches: Some(5),
                resume_from: None,
                presentation_mode: None,
            }))
            .await
            .expect("explore refine should succeed"),
    );
    assert_eq!(refine.scan_scope.start_line, 3);
    assert_eq!(refine.scan_scope.end_line, 5);
    assert_eq!(refine.total_matches, 2);
    assert_eq!(refine.matches.len(), 2);
    assert_eq!(refine.matches[0].start_line, 3);
    assert_eq!(refine.matches[1].start_line, 5);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn extended_explore_zoom_defaults_to_text_first_output() {
    let workspace_root = temp_workspace_root("explore-zoom-text-default");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create explorer fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn demo() {\n    let alpha = 1;\n    let beta = alpha;\n}\n",
    )
    .expect("failed to seed explorer fixture");

    let server = extended_runtime_server_for_workspace_root(&workspace_root);
    let response = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Zoom,
            query: None,
            pattern_type: None,
            anchor: Some(ExploreAnchor {
                start_line: 2,
                start_column: 9,
                end_line: 2,
                end_column: 14,
            }),
            context_lines: Some(1),
            max_matches: None,
            resume_from: None,
            presentation_mode: None,
        }))
        .await
        .expect("default explore zoom should succeed");

    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("path"))
            .and_then(|value| value.as_str()),
        Some("src/lib.rs")
    );
    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("line_start"))
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("line_end"))
            .and_then(|value| value.as_u64()),
        Some(3)
    );
    assert!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("content"))
            .is_none()
    );
    let text = tool_result_text(&response);
    assert!(text.contains("path: src/lib.rs"));
    assert!(text.contains("line_window: 1-3"));
    assert!(text.contains("let alpha = 1;"));
    assert!(text.contains("let beta = alpha;"));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn extended_explore_rejects_invalid_mode_payloads() {
    let workspace_root = temp_workspace_root("explore-invalid-payloads");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create explorer invalid fixture");
    fs::write(src_root.join("lib.rs"), "pub fn demo() {}\n")
        .expect("failed to seed explorer invalid fixture");

    let server = extended_runtime_server_for_workspace_root(&workspace_root);
    let probe_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: None,
            pattern_type: None,
            anchor: None,
            context_lines: None,
            max_matches: None,
            resume_from: None,
            presentation_mode: None,
        }))
        .await
        .err()
        .expect("probe without query should fail");
    assert_eq!(probe_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&probe_error), Some("invalid_params"));
    assert_eq!(probe_error.message, "query must not be empty");

    let zoom_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Zoom,
            query: Some("demo".to_owned()),
            pattern_type: None,
            anchor: None,
            context_lines: None,
            max_matches: None,
            resume_from: None,
            presentation_mode: None,
        }))
        .await
        .err()
        .expect("zoom with query should fail");
    assert_eq!(zoom_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&zoom_error), Some("invalid_params"));
    assert_eq!(zoom_error.message, "query is not allowed for zoom");

    let refine_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Refine,
            query: Some("demo".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: Some(ExploreAnchor {
                start_line: 1,
                start_column: 8,
                end_line: 1,
                end_column: 12,
            }),
            context_lines: Some(0),
            max_matches: Some(1),
            resume_from: Some(ExploreCursor { line: 2, column: 1 }),
            presentation_mode: None,
        }))
        .await
        .err()
        .expect("refine with resume_from outside scan scope should fail");
    assert_eq!(refine_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&refine_error), Some("invalid_params"));
    assert_eq!(
        refine_error.message,
        "resume_from must stay within the refine scan scope"
    );

    let text_probe_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: Some("demo".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: None,
            max_matches: Some(1),
            resume_from: None,
            presentation_mode: Some(ReadPresentationMode::Text),
        }))
        .await
        .err()
        .expect("probe text mode should fail");
    assert_eq!(text_probe_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&text_probe_error), Some("invalid_params"));
    assert_eq!(
        text_probe_error.message,
        "presentation_mode=text is only supported for zoom"
    );

    cleanup_workspace_root(&workspace_root);
}
