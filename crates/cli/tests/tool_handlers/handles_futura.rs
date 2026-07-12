//! STALE_HANDLE / MIXED_HANDLE integration coverage for `read_match`.
//!
//! Drives real `FriggMcpServer` handlers: obtain handles via `search_text`, then
//! assert structured recovery when handles are missing or cross-paired.

use super::*;
use frigg::mcp::types::{
    NextAction, NextActionOrigin, NextActionTarget, ReadMatchResponse, ReplayOriginTarget,
    TargetRef,
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

#[tokio::test]
async fn result_targets_reject_equal_local_ids_from_another_session_before_lookup() {
    let workspace_root = fresh_fixture_root("result-target-cross-session-equal-ids");
    let server_a = server_for_workspace_root(&workspace_root).await;
    let server_b = server_for_workspace_root(&workspace_root).await;
    let repository_id = public_repository_id(&server_a).await;

    async fn issue_target(
        server: &FriggMcpServer,
        repository_id: &str,
    ) -> (String, String, TargetRef) {
        let response = server
            .search_symbol(Parameters(SearchSymbolParams {
                query: "greeting".to_owned(),
                repository_id: Some(repository_id.to_owned()),
                path_class: None,
                path_regex: Some(r"^src/lib\.rs$".to_owned()),
                limit: Some(5),
                continuation: None,
                response_mode: Some(ResponseMode::Compact),
            }))
            .await
            .expect("each session should issue a symbol target")
            .0;
        let row = response
            .matches
            .first()
            .expect("fixture should expose greeting");
        (
            response
                .result_handle
                .expect("symbol search should issue a handle"),
            row.match_id
                .clone()
                .expect("symbol row should issue a match id"),
            row.target_ref
                .clone()
                .expect("symbol row should issue a target ref"),
        )
    }

    let (handle_a, match_a, target_a) = issue_target(&server_a, &repository_id).await;
    let (handle_b, match_b, target_b) = issue_target(&server_b, &repository_id).await;
    assert_eq!(
        handle_a, handle_b,
        "session-local handle counters must coincide"
    );
    assert_eq!(
        match_a, match_b,
        "session-local match counters must coincide"
    );
    assert_ne!(
        target_a.target_scope(),
        target_b.target_scope(),
        "each MCP session must own a distinct opaque target scope"
    );

    let error = match server_b
        .find_references(Parameters(FindReferencesParams {
            target: Some(target_a),
            repository_id: Some(repository_id.clone()),
            include_definition: Some(true),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a foreign target must fail before equal local IDs can rebind it"),
    };
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("TARGET_SCOPE_MISMATCH"));
    let data = error
        .data
        .expect("scope mismatch must include recovery data");
    assert!(data["correction_hint"].is_string());
    assert!(data["related_tools"].is_array());
    assert_eq!(data["next_actions"], serde_json::json!([]));

    server_b
        .find_references(Parameters(FindReferencesParams {
            target: Some(target_b),
            repository_id: Some(repository_id),
            include_definition: Some(true),
            limit: Some(5),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .expect("the same-session target should remain executable");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn result_targets_preserve_stale_mixed_and_capacity_expiry_failures() {
    let workspace_root = fresh_fixture_root("result-target-handle-failures");
    let server = server_for_workspace_root(&workspace_root).await;
    let repository_id = public_repository_id(&server).await;
    let symbol = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("symbol search should issue a result target")
        .0;
    assert_eq!(symbol.handle_expires.as_deref(), Some("session"));
    let target = symbol.matches[0]
        .target_ref
        .clone()
        .expect("symbol match should carry a target");
    let TargetRef::ResultMatch {
        result_handle,
        match_id,
        target_scope,
    } = target
    else {
        panic!("handle-bound symbols must issue result_match targets");
    };

    let stale = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(TargetRef::ResultMatch {
                result_handle: "result-handle-not-present".to_owned(),
                match_id: match_id.clone(),
                target_scope: target_scope.clone(),
            }),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("an absent target handle must remain STALE_HANDLE"),
    };
    assert_handle_recovery_fields(&stale, "STALE_HANDLE");

    let text = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("text search should issue a second target pair")
        .0;
    let TargetRef::ResultMatch {
        match_id: text_match_id,
        ..
    } = text.matches[0]
        .target_ref
        .clone()
        .expect("text match should carry a target")
    else {
        panic!("handle-bound text rows must issue result_match targets");
    };
    let mixed = match server
        .find_references(Parameters(FindReferencesParams {
            target: Some(TargetRef::ResultMatch {
                result_handle,
                match_id: text_match_id,
                target_scope,
            }),
            include_definition: Some(true),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("a cross-paired navigation target must remain MIXED_HANDLE"),
    };
    assert_handle_recovery_fields(&mixed, "MIXED_HANDLE");
    cleanup_workspace_root(&workspace_root);

    let capacity_root = fresh_fixture_root("result-target-capacity-expiry");
    let capacity_server = server_for_workspace_root(&capacity_root).await;
    let capacity_repository_id = public_repository_id(&capacity_server).await;
    let first = capacity_server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some(capacity_repository_id.clone()),
            path_class: None,
            path_regex: Some(r"^src/lib\.rs$".to_owned()),
            limit: Some(5),
            continuation: None,
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .expect("capacity fixture should issue the oldest target")
        .0;
    let oldest_target = first.matches[0]
        .target_ref
        .clone()
        .expect("capacity fixture should expose a target");
    for _ in 0..64 {
        capacity_server
            .search_text(Parameters(SearchTextParams {
                query: "hello from fixture".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: Some(capacity_repository_id.clone()),
                path_regex: Some(r"^src/lib\.rs$".to_owned()),
                limit: Some(5),
                ..Default::default()
            }))
            .await
            .expect("each capacity probe should issue a fresh handle");
    }
    let evicted = match capacity_server
        .go_to_definition(Parameters(GoToDefinitionParams {
            target: Some(oldest_target),
            ..Default::default()
        }))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("the oldest target must expire when the session cache reaches capacity"),
    };
    assert_handle_recovery_fields(&evicted, "STALE_HANDLE");
    cleanup_workspace_root(&capacity_root);
}
