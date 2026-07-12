//! STALE_HANDLE / MIXED_HANDLE integration coverage for `read_match`.
//!
//! Drives real `FriggMcpServer` handlers: obtain handles via `search_text`, then
//! assert structured recovery when handles are missing or cross-paired.

use super::*;
use frigg::mcp::types::{
    NextAction, NextActionOrigin, NextActionTarget, ReadMatchResponse, ReplayOriginTarget,
};

fn assert_handle_recovery_fields(error: &rmcp::ErrorData, expected_code: &str) {
    assert_eq!(error_code_tag(error), Some(expected_code));
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

fn search_text_origin(params: SearchTextParams) -> NextActionOrigin {
    NextActionOrigin(ReplayOriginTarget::SearchText(params))
}

fn assert_exact_origin_retry(
    error: &rmcp::ErrorData,
    expected: &SearchTextParams,
) -> SearchTextParams {
    let data = error
        .data
        .as_ref()
        .expect("origin-bearing handle failure must include structured data");
    let actions = data["next_actions"]
        .as_array()
        .expect("origin-bearing failure must expose canonical retry actions");
    assert_eq!(
        actions.len(),
        1,
        "one exact producer retry is expected: {data:?}"
    );
    let action: NextAction = serde_json::from_value(actions[0].clone())
        .expect("canonical retry action must deserialize");
    let NextActionTarget::SearchText(actual) = &action.target else {
        panic!("origin retry must target search_text: {action:?}");
    };
    assert_eq!(
        serde_json::to_value(actual).expect("retry params serialize"),
        serde_json::to_value(expected).expect("expected origin serializes"),
        "origin retry must preserve the exact producer arguments"
    );
    let serialized = serde_json::to_value(&action).expect("retry action serializes");
    assert!(
        !serialized.to_string().contains("search:m"),
        "retry must not reuse a stale match id: {serialized:?}"
    );
    actual.clone()
}

#[tokio::test]
async fn read_match_wrong_or_missing_handle_returns_stale_handle() {
    let server = server_for_fixture().await;
    let origin_params = SearchTextParams {
        query: "hello from fixture".to_owned(),
        pattern_type: Some(SearchPatternType::Literal),
        repository_id: Some("repo-001".to_owned()),
        path_regex: Some("^src/".to_owned()),
        limit: Some(5),
        ..Default::default()
    };
    let search = server
        .search_text(Parameters(origin_params.clone()))
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
            origin: Some(search_text_origin(origin_params.clone())),
        }))
        .await
        .expect_err("missing result_handle should fail");
    assert_handle_recovery_fields(&missing, "STALE_HANDLE");

    let retry_params = assert_exact_origin_retry(&missing, &origin_params);
    let retried = server
        .search_text(Parameters(retry_params))
        .await
        .expect("exact origin retry should replay through its named handler")
        .0;
    assert!(
        retried.result_handle.is_some() && !retried.matches.is_empty(),
        "origin retry should mint a fresh proof-read pair: {retried:?}"
    );

    // Wrong/garbage handle string with a real-looking match_id.
    let wrong = server
        .read_match(Parameters(ReadMatchParams {
            result_handle: "result-000000-stale".to_owned(),
            match_id: "search:m1".to_owned(),
            before: None,
            after: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
            origin: None,
        }))
        .await
        .expect_err("wrong result_handle should fail");
    assert_handle_recovery_fields(&wrong, "STALE_HANDLE");
    assert!(
        wrong
            .data
            .as_ref()
            .and_then(|data| data.get("next_actions"))
            .is_none_or(|actions| actions.as_array().is_none_or(Vec::is_empty)),
        "originless stale recovery must not fabricate a canonical retry"
    );
}

#[tokio::test]
async fn read_match_cross_search_handle_and_match_id_returns_mixed_handle() {
    let server = server_for_fixture().await;
    let origin_params = SearchTextParams {
        query: "hello from fixture".to_owned(),
        pattern_type: Some(SearchPatternType::Literal),
        repository_id: Some("repo-001".to_owned()),
        path_regex: Some(r"src/lib\.rs$".to_owned()),
        limit: Some(5),
        ..Default::default()
    };

    // Search A: text (match_ids scoped as search:mN).
    let search_a = server
        .search_text(Parameters(origin_params.clone()))
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
            continuation: None,
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
                origin: None,
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
            origin: Some(search_text_origin(origin_params.clone())),
        }))
        .await
        .expect_err("cross-paired handle/match_id should fail");

    assert_handle_recovery_fields(&mixed, "MIXED_HANDLE");
    let retry_params = assert_exact_origin_retry(&mixed, &origin_params);
    let retried = server
        .search_text(Parameters(retry_params))
        .await
        .expect("mixed origin retry should replay through its named handler")
        .0;
    let fresh_handle = retried
        .result_handle
        .expect("replayed search returns handle");
    let fresh_match = retried
        .matches
        .first()
        .and_then(|match_| match_.match_id.clone())
        .expect("replayed search returns match id");
    assert_ne!(
        fresh_handle, handle_b,
        "retry must not reuse rejected handle"
    );
    let fresh: ReadMatchResponse = structured_tool_result(
        server
            .read_match(Parameters(ReadMatchParams {
                result_handle: fresh_handle,
                match_id: fresh_match,
                before: Some(0),
                after: Some(0),
                presentation_mode: Some(ReadPresentationMode::Json),
                include_context_efficiency: None,
                origin: None,
            }))
            .await
            .expect("replayed producer must yield a valid fresh proof pair"),
    );
    assert_eq!(fresh.path, "src/lib.rs");
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
            origin: None,
        }))
        .await
        .expect_err("inverse cross-paired handle/match_id should fail");
    assert_handle_recovery_fields(&mixed_inverse, "MIXED_HANDLE");
}
