#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    ExploreOperation, ExploreParams, FindReferencesParams, ListRepositoriesParams,
    PUBLIC_READ_ONLY_TOOL_NAMES, PUBLIC_SESSION_STATEFUL_TOOL_NAMES, PUBLIC_TOOL_NAMES,
    PUBLIC_WRITE_TOOL_NAMES, ReadFileParams, SearchPatternType, SearchSymbolParams,
    SearchTextParams, WRITE_CONFIRM_PARAM,
};
use frigg::searcher::MAX_REGEX_QUANTIFIERS;
use frigg::settings::FriggConfig;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-mcp-security-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn build_server_for_repo(repo_root: &Path) -> FriggMcpServer {
    build_server_for_roots(vec![repo_root.to_path_buf()])
}

fn build_server_for_roots(roots: Vec<PathBuf>) -> FriggMcpServer {
    let config =
        FriggConfig::from_workspace_roots(roots).expect("workspace root must produce valid config");
    FriggMcpServer::new(config)
}

fn build_extended_server_for_roots(roots: Vec<PathBuf>) -> FriggMcpServer {
    let config =
        FriggConfig::from_workspace_roots(roots).expect("workspace root must produce valid config");
    FriggMcpServer::new_with_runtime_options(config, false, true)
}

fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
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

fn parse_tool_annotation_blocks(source: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut in_block = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if !in_block && trimmed.starts_with("#[tool(") {
            in_block = true;
            current.clear();
        }
        if !in_block {
            continue;
        }

        current.push_str(trimmed);
        current.push('\n');
        if trimmed == ")]" {
            blocks.push(current.clone());
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
        for block in parse_tool_annotation_blocks(&source) {
            parsed.push(ToolAnnotationFlags {
                name: parse_string_assignment(&block, "name").unwrap_or_else(|| {
                    panic!(
                        "missing `name = ...` in #[tool(...)] block from {}:\n{block}",
                        source_path.display()
                    )
                }),
                read_only_hint: parse_bool_assignment(&block, "read_only_hint"),
                destructive_hint: parse_bool_assignment(&block, "destructive_hint"),
            });
        }
    }

    parsed
}

#[test]
fn security_public_tool_surface_remains_non_destructive_and_explicit() {
    let parsed = parse_tool_annotation_flags();

    let actual_names = parsed
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<BTreeSet<_>>();
    let expected_names = PUBLIC_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_names, expected_names,
        "public MCP tool surface drifted; update security policy/tests intentionally before adding tools"
    );

    for entry in parsed {
        if PUBLIC_READ_ONLY_TOOL_NAMES.contains(&entry.name.as_str()) {
            assert_eq!(
                entry.read_only_hint,
                Some(true),
                "tool `{}` must declare read_only_hint = true",
                entry.name
            );
        } else if PUBLIC_SESSION_STATEFUL_TOOL_NAMES.contains(&entry.name.as_str()) {
            assert_eq!(
                entry.read_only_hint,
                Some(false),
                "tool `{}` must declare read_only_hint = false because it mutates session state",
                entry.name
            );
        } else if PUBLIC_WRITE_TOOL_NAMES.contains(&entry.name.as_str()) {
            assert_eq!(
                entry.read_only_hint,
                Some(false),
                "tool `{}` must declare read_only_hint = false because it mutates workspace state",
                entry.name
            );
        } else {
            panic!("unexpected public MCP tool `{}`", entry.name);
        }
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

    let server = build_server_for_repo(&repo_root);

    let list_result = server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await;
    if let Err(error) = &list_result {
        assert_ne!(
            error_code_tag(error),
            Some("confirmation_required"),
            "list_repositories must not require `{}` on the public non-destructive tool surface",
            WRITE_CONFIRM_PARAM
        );
    }
    list_result.expect("list_repositories should succeed");

    let read_result = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
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
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(5),
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
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
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
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(5),
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

    let extended_server = build_extended_server_for_roots(vec![repo_root.clone()]);
    let explore_result = extended_server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: Some("hello".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
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
fn security_confirmed_write_tools_are_public_and_not_misclassified() {
    for tool_name in ["workspace_prepare", "workspace_reindex"] {
        assert!(
            PUBLIC_TOOL_NAMES.contains(&tool_name),
            "{tool_name} must be part of the public tool surface"
        );
        assert!(
            PUBLIC_WRITE_TOOL_NAMES.contains(&tool_name),
            "{tool_name} must be classified as a confirmed write tool"
        );
        assert!(
            !PUBLIC_READ_ONLY_TOOL_NAMES.contains(&tool_name),
            "{tool_name} must not appear on the read-only public tool surface"
        );
        assert!(
            !PUBLIC_SESSION_STATEFUL_TOOL_NAMES.contains(&tool_name),
            "{tool_name} must not be misclassified as session-state-only"
        );
    }
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

    let server = build_extended_server_for_roots(vec![repo_root.clone()]);
    let escaped_path = outside_root.join("escape.rs");
    let error = server
        .explore(Parameters(ExploreParams {
            path: escaped_path.display().to_string(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: Some("outside".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
        }))
        .await
        .err()
        .expect("explore should reject paths outside workspace roots");

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
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

    let server = build_extended_server_for_roots(vec![repo_root.clone()]);
    let abusive = "needle+".repeat(MAX_REGEX_QUANTIFIERS + 1);
    let error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: Some(abusive),
            pattern_type: Some(SearchPatternType::Regex),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
        }))
        .await
        .err()
        .expect("explore should reject abusive regex patterns");

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

    let server = build_server_for_repo(&repo_root);
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "../outside.txt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
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

    let server = build_server_for_repo(&repo_root);
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/linked-outside.txt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
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

    let server = build_server_for_repo(&repo_root);
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: outside_path.display().to_string(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
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

    let server = build_server_for_roots(vec![first_root.clone(), second_root.clone()]);
    let response = server
        .read_file(Parameters(ReadFileParams {
            path: second_root.join("src/lib.rs").display().to_string(),
            repository_id: None,
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
        .expect("absolute path under second root should resolve")
        .0;

    assert_eq!(response.repository_id, "repo-002");
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

    let server = build_server_for_repo(&repo_root);
    let existing_error = match server
        .read_file(Parameters(ReadFileParams {
            path: outside_existing_path.display().to_string(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
    {
        Ok(_) => panic!("existing outside path should be rejected"),
        Err(error) => error,
    };
    let missing_error = match server
        .read_file(Parameters(ReadFileParams {
            path: outside_missing_path.display().to_string(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
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

    let server = build_server_for_repo(&repo_root);
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/outside-link.txt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
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
async fn security_provenance_rejects_symlink_escape_before_write() {
    let workspace = temp_workspace_root("provenance-symlink-escape");
    let repo_root = workspace.join("repo");
    let escaped_store = workspace.join("escaped-store");
    fs::create_dir_all(&repo_root).expect("failed to create fixture repo root");
    fs::create_dir_all(&escaped_store).expect("failed to create escaped storage fixture");
    std::os::unix::fs::symlink(&escaped_store, repo_root.join(".frigg"))
        .expect("failed to create symlinked provenance storage fixture");

    let config = FriggConfig::from_workspace_roots(vec![repo_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_provenance_best_effort(config, true);
    let response = server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
        .expect("list_repositories should succeed even when provenance path is unsafe")
        .0;

    assert!(
        !response.repositories.is_empty(),
        "list_repositories should still return configured repositories"
    );
    assert!(
        !escaped_store.join("storage.sqlite3").exists(),
        "provenance write should not escape through symlinked .frigg directory"
    );

    cleanup_workspace(&workspace);
}
