//! STALE_HANDLE / MIXED_HANDLE integration coverage for `read_match`.
//!
//! Drives real `FriggMcpServer` handlers: obtain handles via `search_text`, then
//! assert structured recovery when handles are missing or cross-paired.

use super::*;
use frigg::mcp::types::ReadMatchResponse;

fn handle_error_code(error: &rmcp::ErrorData) -> String {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned()
}

fn assert_handle_recovery_fields(error: &rmcp::ErrorData, expected_code: &str) {
    let code = handle_error_code(error);
    assert!(
        code.contains(expected_code)
            || (code == "resource_not_found"
                && error
                    .data
                    .as_ref()
                    .and_then(|v| v.get("correction_hint"))
                    .is_some()),
        "expected error_code containing {expected_code:?} (or resource_not_found with recovery), got {code:?}; data={:?}",
        error.data
    );
    let data = error
        .data
        .as_ref()
        .expect("handle failure must include structured error data");
    assert!(
        data.get("correction_hint").is_some()
            || data.get("suggested_next").is_some()
            || data.get("related_tools").is_some(),
        "STALE/MIXED handle errors must include recovery guidance: {data:?}"
    );
    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
}

#[tokio::test]
async fn read_match_wrong_or_missing_handle_returns_stale_handle() {
    let server = server_for_fixture().await;
    let search = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some("^src/".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search_text should find fixture string")
        .0;
    let match_id = search
        .matches
        .first()
        .and_then(|m| m.match_id.clone())
        .expect("search should expose match_id");
    let _live_handle = search
        .result_handle
        .clone()
        .expect("search should expose result_handle");

    // Completely missing handle.
    let missing = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: "result-does-not-exist".to_owned(),
            match_id: match_id.clone(),
            before: None,
            after: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect_err("missing result_handle should fail");
    assert_handle_recovery_fields(&missing, "STALE_HANDLE");

    // Wrong/garbage handle string with a real-looking match_id.
    let wrong = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: "result-000000-stale".to_owned(),
            match_id: "search:m1".to_owned(),
            before: None,
            after: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect_err("wrong result_handle should fail");
    assert_handle_recovery_fields(&wrong, "STALE_HANDLE");
}

#[tokio::test]
async fn read_match_cross_search_handle_and_match_id_returns_mixed_handle() {
    let server = server_for_fixture().await;

    // Search A: text (match_ids scoped as search:mN).
    let search_a = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search A (text) should succeed")
        .0;
    let handle_a = search_a
        .result_handle
        .clone()
        .expect("search A should return result_handle");
    let match_a = search_a
        .matches
        .first()
        .and_then(|m| m.match_id.clone())
        .expect("search A should expose match_id");
    assert!(
        match_a.starts_with("search:"),
        "text search match_id should be search-scoped: {match_a}"
    );

    // Search B: symbol (match_ids scoped as symbols:mN) so ids cannot collide with A.
    let search_b = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("search B (symbol) should succeed")
        .0;
    let handle_b = search_b
        .result_handle
        .clone()
        .expect("search B should return result_handle");
    let match_b = search_b
        .matches
        .first()
        .and_then(|m| m.match_id.clone())
        .expect("search B should expose match_id");
    assert!(
        match_b.starts_with("symbols:"),
        "symbol search match_id should be symbols-scoped: {match_b}"
    );

    assert_ne!(
        handle_a, handle_b,
        "two searches should mint distinct result handles"
    );
    assert_ne!(
        match_a, match_b,
        "cross-tool match_ids must differ so MIXED_HANDLE is observable"
    );

    // Same-handle pairing still works (sanity).
    let ok: ReadMatchResponse = structured_tool_result(
        server
            .read_match(Parameters(ReadMatchParams {
                result_handle: handle_a.clone(),
                match_id: match_a.clone(),
                before: Some(0),
                after: Some(0),
                presentation_mode: Some(ReadPresentationMode::Json),
                include_context_efficiency: None,
            }))
            .await
            .expect("matched handle/match_id pair should succeed"),
    );
    assert_eq!(ok.path, "src/lib.rs");

    // match_id from A with handle from B → MIXED_HANDLE.
    let mixed = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: handle_b.clone(),
            match_id: match_a.clone(),
            before: None,
            after: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect_err("cross-paired handle/match_id should fail");

    assert_handle_recovery_fields(&mixed, "MIXED_HANDLE");
    let data = mixed
        .data
        .as_ref()
        .expect("mixed handle failure must include structured error data");
    // When the foreign handle still holds match_a, recovery may name it.
    if let Some(foreign) = data.get("foreign_handle").and_then(|v| v.as_str()) {
        assert_eq!(
            foreign, handle_a,
            "foreign_handle should point at the search that owns match_a"
        );
    }

    // Inverse: symbols match_id with text handle.
    let mixed_inverse = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: handle_a,
            match_id: match_b,
            before: None,
            after: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect_err("inverse cross-paired handle/match_id should fail");
    assert_handle_recovery_fields(&mixed_inverse, "MIXED_HANDLE");
}
