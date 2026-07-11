#![allow(clippy::panic)]

//! Integration tests for session adoption boundaries and repository escape prevention.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    ExploreOperation, ExploreParams, FindReferencesParams, GoToDefinitionParams,
    ListRepositoriesParams, PUBLIC_READ_ONLY_TOOL_NAMES, PUBLIC_SESSION_STATEFUL_TOOL_NAMES,
    PUBLIC_TOOL_NAMES, PUBLIC_WRITE_TOOL_NAMES, ReadFileParams, ReadFileResponse,
    ReadPresentationMode, SearchPatternType, SearchSymbolParams, SearchTextParams,
    WRITE_CONFIRM_PARAM, WorkspaceParams,
};
use frigg::searcher::MAX_REGEX_QUANTIFIERS;
use frigg::settings::FriggConfig;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use serde_json::from_value;

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "frigg-mcp-security-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".git")).expect("fixture git marker should be creatable");
    root
}

async fn build_server_for_repo(repo_root: &Path) -> FriggMcpServer {
    build_server_for_roots(vec![repo_root.to_path_buf()]).await
}

async fn build_server_for_roots(roots: Vec<PathBuf>) -> FriggMcpServer {
    let config =
        FriggConfig::from_workspace_roots(roots).expect("workspace root must produce valid config");
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;
    server
}

async fn build_extended_server_for_roots(roots: Vec<PathBuf>) -> FriggMcpServer {
    let config =
        FriggConfig::from_workspace_roots(roots).expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, true);
    attach_session_repositories(&server).await;
    server
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

async fn attach_session_repositories(server: &FriggMcpServer) {
    for repository_id in public_repository_ids(server).await {
        server
            .workspace(Parameters(WorkspaceParams {
                path: None,
                repository_id: Some(repository_id),
                set_default: Some(true),
                resolve_mode: None,
            }))
            .await
            .expect("workspace should adopt the startup repository");
    }
}

async fn public_repository_ids(server: &FriggMcpServer) -> Vec<String> {
    server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
        .expect("list_repositories should succeed")
        .0
        .repositories
        .into_iter()
        .map(|repository| repository.repository_id)
        .collect()
}

fn error_code_tag(error: &rmcp::ErrorData) -> Option<&str> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
}

fn retryable_tag(error: &rmcp::ErrorData) -> Option<bool> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("retryable"))
        .and_then(|value| value.as_bool())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolAnnotationFlags {
    name: String,
    gated_feature: Option<String>,
    read_only_hint: Option<bool>,
    destructive_hint: Option<bool>,
}

fn mcp_source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/mcp")
}

fn collect_rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut paths = Vec::new();

    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", current.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|err| panic!("failed to read directory entry: {err}"));
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension() == Some(OsStr::new("rs")) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    paths
}

fn parse_feature_cfg(trimmed: &str) -> Option<String> {
    let remainder = trimmed.strip_prefix("#[cfg(feature = \"")?;
    let feature = remainder.split_once('"')?.0;
    Some(feature.to_owned())
}

fn parse_cfg_attr_feature(block: &str) -> Option<String> {
    let remainder = block.split_once("feature = \"")?.1;
    let feature = remainder.split_once('"')?.0;
    Some(feature.to_owned())
}

fn parse_tool_annotation_blocks(source: &str) -> Vec<(Option<String>, String)> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut in_block = false;
    let mut pending_feature = None;
    let mut current_feature = None;

    for line in source.lines() {
        let trimmed = line.trim();
        if !in_block && (trimmed.starts_with("#[tool(") || trimmed.starts_with("#[cfg_attr(")) {
            in_block = true;
            current_feature = pending_feature.take();
            current.clear();
        }
        if !in_block {
            pending_feature = parse_feature_cfg(trimmed);
        }
        if !in_block {
            continue;
        }

        current.push_str(trimmed);
        current.push('\n');
        if trimmed.ends_with(")]") {
            if current.contains("tool(") {
                let gated_feature = current_feature
                    .take()
                    .or_else(|| parse_cfg_attr_feature(&current));
                blocks.push((gated_feature, current.clone()));
            }
            in_block = false;
        }
    }

    blocks
}

fn parse_string_assignment(block: &str, key: &str) -> Option<String> {
    let marker = format!("{key} = \"");
    let remainder = block.split_once(&marker)?.1;
    let value = remainder.split_once('"')?.0;
    Some(value.to_owned())
}

fn parse_bool_assignment(block: &str, key: &str) -> Option<bool> {
    let marker = format!("{key} = ");
    let remainder = block.split_once(&marker)?.1;
    if remainder.starts_with("true") {
        Some(true)
    } else if remainder.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn parse_tool_annotation_flags() -> Vec<ToolAnnotationFlags> {
    let source_root = mcp_source_root();
    let mut parsed = Vec::new();

    for source_path in collect_rust_sources(&source_root) {
        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", source_path.display()));
        for (gated_feature, block) in parse_tool_annotation_blocks(&source) {
            parsed.push(ToolAnnotationFlags {
                name: parse_string_assignment(&block, "name").unwrap_or_else(|| {
                    panic!(
                        "missing `name = ...` in #[tool(...)] block from {}:\n{block}",
                        source_path.display()
                    )
                }),
                gated_feature,
                read_only_hint: parse_bool_assignment(&block, "read_only_hint"),
                destructive_hint: parse_bool_assignment(&block, "destructive_hint"),
            });
        }
    }

    parsed
}

fn tool_feature_enabled(feature: &str) -> bool {
    !matches!(feature, "playbook") || cfg!(feature = "playbook")
}

#[test]
fn security_public_tool_surface_remains_non_destructive_and_explicit() {
    let parsed = parse_tool_annotation_flags();

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty config should be valid");
    let actual_names = FriggMcpServer::new(config)
        .runtime_registered_tool_names()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let expected_names = PUBLIC_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_names, expected_names,
        "public MCP tool surface drifted; update security policy/tests intentionally before adding tools"
    );

    let read_only_names = PUBLIC_READ_ONLY_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        read_only_names, expected_names,
        "all public MCP tools must be source-read-only; .frigg and session-state changes stay on the read-only hint surface"
    );

    for entry in parsed.into_iter().filter(|entry| {
        expected_names.contains(&entry.name)
            && entry
                .gated_feature
                .as_deref()
                .map(tool_feature_enabled)
                .unwrap_or(true)
    }) {
        assert_eq!(
            entry.read_only_hint,
            Some(true),
            "tool `{}` must declare read_only_hint = true",
            entry.name
        );
        assert_eq!(
            entry.destructive_hint,
            Some(false),
            "tool `{}` must declare destructive_hint = false",
            entry.name
        );
    }
}

#[tokio::test]
async fn security_read_only_tool_calls_do_not_require_confirm_param() {
    let workspace = temp_workspace_root("read-only-no-confirm-required");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn greeting() -> &'static str { \"hello\" }\n",
    )
    .expect("failed to seed fixture file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");

    let workspace_result = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await;
    if let Err(error) = &workspace_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "workspace must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    workspace_result.expect("workspace should succeed");

    let read_result = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(repository_id.clone()),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await;
    if let Err(error) = &read_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "read_file must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    read_result.expect("read_file should succeed");

    let search_text_result = server
        .search_text(Parameters(SearchTextParams {
            query: "hello".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await;
    if let Err(error) = &search_text_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "search_text must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    search_text_result.expect("search_text should succeed");

    let search_symbol_result = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await;
    if let Err(error) = &search_symbol_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "search_symbol must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    search_symbol_result.expect("search_symbol should succeed");

    let find_references_result = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some(repository_id.clone()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await;
    if let Err(error) = &find_references_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "find_references must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    find_references_result.expect("find_references should succeed");

    let extended_server = build_extended_server_for_roots(vec![repo_root.clone()]).await;
    let extended_repository_id = public_repository_ids(&extended_server)
        .await
        .into_iter()
        .next()
        .expect("extended server should expose one repository");
    let explore_result = extended_server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(extended_repository_id),
            operation: ExploreOperation::Probe,
            query: Some("hello".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
            continuation: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await;
    if let Err(error) = &explore_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "explore must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    explore_result.expect("explore should succeed");

    cleanup_workspace(&workspace);
}

#[test]
fn security_session_state_tools_are_public_read_only_hinted_and_classified() {
    let tool_name = "workspace";
    assert!(
        PUBLIC_TOOL_NAMES.contains(&tool_name),
        "{tool_name} must be part of the public tool surface"
    );
    assert!(
        PUBLIC_READ_ONLY_TOOL_NAMES.contains(&tool_name),
        "{tool_name} must be declared read-only at the MCP hint layer"
    );
    assert!(
        PUBLIC_SESSION_STATEFUL_TOOL_NAMES.contains(&tool_name),
        "{tool_name} must be classified as session-stateful"
    );
    assert!(
        !PUBLIC_WRITE_TOOL_NAMES.contains(&tool_name),
        "{tool_name} must not be classified as confirm-gated .frigg maintenance"
    );
}

#[test]
fn security_public_surface_exposes_no_confirm_gated_maintenance_tools() {
    assert!(
        PUBLIC_WRITE_TOOL_NAMES.is_empty(),
        "maintenance refreshes should stay out of the public MCP tool surface"
    );
}

#[tokio::test]
async fn security_extended_explore_enforces_workspace_boundary() {
    let workspace = temp_workspace_root("explore-workspace-boundary");
    let repo_root = workspace.join("repo");
    let outside_root = workspace.join("outside");
    fs::create_dir_all(repo_root.join("src")).expect("failed to create repo root");
    fs::create_dir_all(&outside_root).expect("failed to create outside root");
    fs::write(repo_root.join("src/lib.rs"), "pub fn inside() {}\n")
        .expect("failed to seed repo file");
    fs::write(outside_root.join("escape.rs"), "pub fn outside() {}\n")
        .expect("failed to seed outside file");

    let server = build_extended_server_for_roots(vec![repo_root.clone()]).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let escaped_path = outside_root.join("escape.rs");
    let error = server
        .explore(Parameters(ExploreParams {
            path: escaped_path.display().to_string(),
            repository_id: Some(repository_id),
            operation: ExploreOperation::Probe,
            query: Some("outside".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
            continuation: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
        .expect_err("explore should reject paths outside workspace roots");

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST, "{error:?}");
    assert_eq!(error_code_tag(&error), Some("access_denied"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("outside workspace roots"),
        "explore should preserve the workspace-boundary denial message"
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
async fn security_extended_explore_rejects_abusive_regex_patterns() {
    let workspace = temp_workspace_root("explore-regex-abuse");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create repo root");
    fs::write(src_root.join("lib.rs"), "pub fn needle() {}\n").expect("failed to seed repo file");

    let server = build_extended_server_for_roots(vec![repo_root.clone()]).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let abusive = "needle+".repeat(MAX_REGEX_QUANTIFIERS + 1);
    let error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(repository_id),
            operation: ExploreOperation::Probe,
            query: Some(abusive),
            pattern_type: Some(SearchPatternType::Regex),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
            continuation: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
        .expect_err("explore should reject abusive regex patterns");

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert!(
        error.message.contains("invalid query regex"),
        "unexpected explore regex abuse error: {}",
        error.message
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("regex_error_code"))
            .and_then(|value| value.as_str()),
        Some("regex_too_many_quantifiers")
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
async fn security_read_file_rejects_relative_path_traversal_outside_workspace() {
    let workspace = temp_workspace_root("relative-traversal");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(src_root.join("lib.rs"), "pub fn safe() {}\n").expect("failed to seed fixture file");
    fs::write(workspace.join("outside.txt"), "secret\n").expect("failed to seed outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "../outside.txt".to_owned(),
            repository_id: Some(repository_id),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
    {
        Ok(_) => panic!("relative traversal path should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(error_code_tag(&error), Some("access_denied"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("outside workspace roots"),
        "unexpected traversal error message: {}",
        error.message
    );

    cleanup_workspace(&workspace);
}

#[cfg(unix)]
#[tokio::test]
async fn security_read_file_rejects_symlink_escape_outside_workspace() {
    let workspace = temp_workspace_root("symlink-traversal");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    let outside_path = workspace.join("outside.txt");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(src_root.join("lib.rs"), "pub fn safe() {}\n").expect("failed to seed fixture file");
    fs::write(&outside_path, "secret\n").expect("failed to seed outside file");
    std::os::unix::fs::symlink(&outside_path, src_root.join("linked-outside.txt"))
        .expect("failed to create fixture symlink");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/linked-outside.txt".to_owned(),
            repository_id: Some(repository_id),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
    {
        Ok(_) => panic!("symlink traversal path should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(error_code_tag(&error), Some("access_denied"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("outside workspace roots"),
        "unexpected traversal error message: {}",
        error.message
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
async fn security_read_file_rejects_absolute_path_outside_workspace() {
    let workspace = temp_workspace_root("absolute-path");
    let repo_root = workspace.join("repo");
    fs::create_dir_all(&repo_root).expect("failed to create fixture repo root");
    let outside_path = workspace.join("outside.txt");
    fs::write(&outside_path, "secret\n").expect("failed to seed outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: outside_path.display().to_string(),
            repository_id: Some(repository_id),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
    {
        Ok(_) => panic!("absolute path outside workspace should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(error_code_tag(&error), Some("access_denied"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("outside workspace roots"),
        "unexpected boundary error message: {}",
        error.message
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
async fn security_go_to_definition_rejects_relative_path_traversal_outside_workspace() {
    let workspace = temp_workspace_root("navigation-relative-traversal");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn outside_secret_token() {}\n",
    )
    .expect("failed to seed fixture file");
    fs::write(
        workspace.join("outside.rs"),
        "pub fn outside_secret_token() {}\n",
    )
    .expect("failed to seed outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");

    let result = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some(repository_id),
            path: Some("../outside.rs".to_owned()),
            line: Some(1),
            column: Some(12),
            include_follow_up_structural: None,
            limit: None,
            response_mode: None,
        }))
        .await;

    match result {
        Ok(response) => panic!(
            "navigation must not read files outside workspace roots: resolved {} match(es)",
            response.0.matches.len()
        ),
        Err(error) => {
            assert_ne!(
                error_code_tag(&error),
                Some("confirmation_required"),
                "traversal rejection must not masquerade as a confirmation prompt"
            );
        }
    }

    cleanup_workspace(&workspace);
}

#[cfg(unix)]
#[tokio::test]
async fn security_go_to_definition_rejects_absolute_path_outside_workspace() {
    let workspace = temp_workspace_root("navigation-absolute-traversal");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn outside_secret_token() {}\n",
    )
    .expect("failed to seed fixture file");
    let outside_path = workspace.join("outside.rs");
    fs::write(&outside_path, "pub fn outside_secret_token() {}\n")
        .expect("failed to seed outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");

    let result = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some(repository_id),
            path: Some(outside_path.display().to_string()),
            line: Some(1),
            column: Some(12),
            include_follow_up_structural: None,
            limit: None,
            response_mode: None,
        }))
        .await;

    if let Ok(response) = result {
        panic!(
            "navigation must not read absolute paths outside workspace roots: resolved {} match(es)",
            response.0.matches.len()
        );
    }

    cleanup_workspace(&workspace);
}

#[tokio::test]
async fn security_read_file_resolves_absolute_path_under_later_workspace_root() {
    let workspace = temp_workspace_root("absolute-multi-root");
    let first_root = workspace.join("repo-a");
    let second_root = workspace.join("repo-b");
    fs::create_dir_all(first_root.join("src")).expect("failed to create first fixture repo root");
    fs::create_dir_all(second_root.join("src")).expect("failed to create second fixture repo root");
    fs::write(first_root.join("src/lib.rs"), "pub fn first() {}\n")
        .expect("failed to seed first root fixture file");
    fs::write(second_root.join("src/lib.rs"), "pub fn second() {}\n")
        .expect("failed to seed second root fixture file");

    let server = build_server_for_roots(vec![first_root.clone(), second_root.clone()]).await;
    let second_repository_id = frigg::domain::model::stable_repository_id_for_root(&second_root).0;
    let response = server
        .read_file(Parameters(ReadFileParams {
            path: second_root.join("src/lib.rs").display().to_string(),
            repository_id: None,
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect("absolute path under second root should resolve")
        .structured_content
        .expect("read_file json mode should return structured_content");
    let response: ReadFileResponse =
        from_value(response).expect("structured read_file response should deserialize");

    assert_eq!(response.repository_id, second_repository_id);
    assert_eq!(response.path, "src/lib.rs");
    assert!(
        !Path::new(&response.path).is_absolute(),
        "read_file path contract must be repository-relative"
    );
    assert!(
        response.content.contains("second"),
        "unexpected file content: {}",
        response.content
    );

    cleanup_workspace(&workspace);
}

#[tokio::test]
async fn security_read_file_outside_workspace_denial_is_uniform_for_existing_and_missing_paths() {
    let workspace = temp_workspace_root("outside-uniform");
    let repo_root = workspace.join("repo");
    fs::create_dir_all(&repo_root).expect("failed to create fixture repo root");
    let outside_existing_path = workspace.join("outside-existing.txt");
    let outside_missing_path = workspace.join("outside-missing.txt");
    fs::write(&outside_existing_path, "secret\n").expect("failed to seed outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let existing_error = match server
        .read_file(Parameters(ReadFileParams {
            path: outside_existing_path.display().to_string(),
            repository_id: Some(repository_id.clone()),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
    {
        Ok(_) => panic!("existing outside path should be rejected"),
        Err(error) => error,
    };
    let missing_error = match server
        .read_file(Parameters(ReadFileParams {
            path: outside_missing_path.display().to_string(),
            repository_id: Some(repository_id),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
    {
        Ok(_) => panic!("missing outside path should be rejected"),
        Err(error) => error,
    };

    assert_eq!(existing_error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(missing_error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(error_code_tag(&existing_error), Some("access_denied"));
    assert_eq!(error_code_tag(&missing_error), Some("access_denied"));
    assert_eq!(retryable_tag(&existing_error), Some(false));
    assert_eq!(retryable_tag(&missing_error), Some(false));
    assert_eq!(existing_error.message, missing_error.message);

    cleanup_workspace(&workspace);
}

#[cfg(unix)]
#[tokio::test]
async fn security_read_file_rejects_symlink_escape_inside_workspace() {
    let workspace = temp_workspace_root("symlink-traversal");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(src_root.join("lib.rs"), "pub fn safe() {}\n").expect("failed to seed fixture file");
    let outside_path = workspace.join("outside-secret.txt");
    fs::write(&outside_path, "secret\n").expect("failed to seed outside file");
    std::os::unix::fs::symlink(&outside_path, src_root.join("outside-link.txt"))
        .expect("failed to create symlink to outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/outside-link.txt".to_owned(),
            repository_id: Some(repository_id),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: None,
            include_context_efficiency: None,
        }))
        .await
    {
        Ok(_) => panic!("symlink escape path should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(error_code_tag(&error), Some("access_denied"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("outside workspace roots"),
        "unexpected symlink traversal error message: {}",
        error.message
    );

    cleanup_workspace(&workspace);
}

#[cfg(unix)]
#[tokio::test]
async fn security_search_text_rejects_symlink_escape() {
    let workspace = temp_workspace_root("search-text-symlink-escape");
    let repo_root = workspace.join("repo");
    let src_root = repo_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create fixture repo root");
    fs::write(src_root.join("lib.rs"), "pub fn safe() {}\n").expect("failed to seed fixture file");
    let outside_path = workspace.join("outside_secret.rs");
    fs::write(&outside_path, "pub fn secret_token() {}\n").expect("failed to seed outside file");
    std::os::unix::fs::symlink(&outside_path, src_root.join("leak.rs"))
        .expect("failed to create symlink to outside file");

    let server = build_server_for_repo(&repo_root).await;
    let repository_id = public_repository_ids(&server)
        .await
        .into_iter()
        .next()
        .expect("server should expose one repository");
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "secret_token".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id),
            context_lines: Some(2),
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search_text should not expose symlink escape matches");
    assert!(
        response.0.matches.is_empty(),
        "search_text must not return context excerpts for symlink escape matches"
    );

    cleanup_workspace(&workspace);
}

#[cfg(unix)]
#[tokio::test]
async fn security_storage_rejects_symlink_escape_before_write() {
    let workspace = temp_workspace_root("storage-symlink-escape");
    let repo_root = workspace.join("repo");
    let escaped_store = workspace.join("escaped-store");
    fs::create_dir_all(&repo_root).expect("failed to create fixture repo root");
    fs::create_dir_all(&escaped_store).expect("failed to create escaped storage fixture");
    std::os::unix::fs::symlink(&escaped_store, repo_root.join(".frigg"))
        .expect("failed to create symlinked storage fixture");

    let config = FriggConfig::from_workspace_roots(vec![repo_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new(config);
    let response = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await
        .expect("workspace should succeed even when storage path is unsafe")
        .0;

    assert!(
        !response.repositories.is_empty(),
        "workspace should still return configured repositories"
    );
    assert!(
        !escaped_store.join("storage.sqlite3").exists(),
        "storage writes should not escape through symlinked .frigg directory"
    );

    cleanup_workspace(&workspace);
}
