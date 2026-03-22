#![allow(clippy::panic)]

use super::*;

#[test]
fn strict_semantic_failure_maps_to_unavailable_error_code() {
    let error = FriggMcpServer::map_frigg_error(FriggError::StrictSemanticFailure {
        reason: "provider outage".to_owned(),
    });

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("unavailable".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_class"))
            .and_then(|value| value.as_str()),
        Some("semantic")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("strict_failure")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("semantic_reason"))
            .and_then(|value| value.as_str()),
        Some("provider outage")
    );
}

#[test]
fn invalid_input_maps_to_invalid_params_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::InvalidInput("bad input".to_owned()));

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("invalid_params".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn not_found_maps_to_resource_not_found_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::NotFound("missing".to_owned()));

    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("resource_not_found".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn access_denied_maps_to_access_denied_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::AccessDenied("blocked".to_owned()));

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("access_denied".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(error.message, "blocked");
}

#[test]
fn io_error_maps_to_internal_error_class() {
    use std::io::Error as IoError;

    let error = FriggMcpServer::map_frigg_error(FriggError::Io(IoError::new(
        std::io::ErrorKind::PermissionDenied,
        "denied",
    )));

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("internal".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_class"))
            .and_then(|value| value.as_str()),
        Some("io")
    );
}

#[test]
fn internal_error_maps_to_internal_error_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::Internal("boom".to_owned()));

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("internal".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(error.message, "boom");
}

#[test]
fn search_hybrid_warning_surfaces_semantic_ok_empty_channel() {
    let warning = FriggMcpServer::search_hybrid_warning(
        "capture_screen",
        crate::mcp::server::search_tools::SearchHybridWarningContext {
            lexical_only_mode: false,
            query_shape: crate::mcp::types::SearchHybridQueryShape::CodeShaped,
            semantic_status: Some(crate::domain::ChannelHealthStatus::Ok),
            semantic_reason: None,
            semantic_hit_count: Some(0),
            semantic_match_count: Some(0),
            exact_pivot_assistance: None,
            witness_demotion_applied: false,
        },
    );

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
        )
    );
}

#[test]
fn search_hybrid_warning_surfaces_semantic_ok_noncontributing_hits() {
    let warning = FriggMcpServer::search_hybrid_warning(
        "capture_screen",
        crate::mcp::server::search_tools::SearchHybridWarningContext {
            lexical_only_mode: false,
            query_shape: crate::mcp::types::SearchHybridQueryShape::CodeShaped,
            semantic_status: Some(crate::domain::ChannelHealthStatus::Ok),
            semantic_reason: None,
            semantic_hit_count: Some(3),
            semantic_match_count: Some(0),
            exact_pivot_assistance: None,
            witness_demotion_applied: false,
        },
    );

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval retained semantic hits, but none contributed to the returned top results; ranking is effectively lexical and graph for this result set"
        )
    );
}

#[test]
fn search_hybrid_warning_escalates_broad_queries_in_lexical_only_mode() {
    let warning = FriggMcpServer::search_hybrid_warning(
        "where is capture request flow handled after tool layer",
        crate::mcp::server::search_tools::SearchHybridWarningContext {
            lexical_only_mode: true,
            query_shape: crate::mcp::types::SearchHybridQueryShape::BroadNaturalLanguage,
            semantic_status: Some(crate::domain::ChannelHealthStatus::Disabled),
            semantic_reason: None,
            semantic_hit_count: Some(0),
            semantic_match_count: Some(0),
            exact_pivot_assistance: None,
            witness_demotion_applied: false,
        },
    );

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval is disabled; broad natural-language ranking is weaker in lexical-only mode, so use results as candidate pivots and switch to exact tools"
        )
    );
}

#[test]
fn search_hybrid_warning_mentions_exact_assistance_for_code_shaped_lexical_only_queries() {
    let warning = FriggMcpServer::search_hybrid_warning(
        "setNavigationContext",
        crate::mcp::server::search_tools::SearchHybridWarningContext {
            lexical_only_mode: true,
            query_shape: crate::mcp::types::SearchHybridQueryShape::CodeShaped,
            semantic_status: Some(crate::domain::ChannelHealthStatus::Disabled),
            semantic_reason: Some("semantic channel disabled by request toggle"),
            semantic_hit_count: Some(0),
            semantic_match_count: Some(0),
            exact_pivot_assistance: Some(&crate::mcp::types::SearchHybridExactPivotAssistance {
                applied: true,
                exact_symbol_hit_count: 1,
                exact_text_hit_count: 1,
                boosted_match_count: 1,
            }),
            witness_demotion_applied: true,
        },
    );

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval is disabled; results are ranked from lexical and graph signals only (semantic channel disabled by request toggle); code-shaped exact symbol/text pivots were preferred for direct matches; weak witness-only matches were demoted"
        )
    );
}
