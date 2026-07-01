//! Regression tests for provenance event append, load-for-tool replay, and trace ordering in SQLite storage.

use super::support::*;

#[test]
fn provenance_append_and_load_for_tool() -> FriggResult<()> {
    let db_path = temp_db_path("provenance-append-load");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.append_provenance_event(
        "trace-read-file-001",
        "read_file",
        &json!({
            "tool_name": "read_file",
            "params": { "path": "src/lib.rs" },
        }),
    )?;
    storage.append_provenance_event(
        "trace-read-file-002",
        "read_file",
        &json!({
            "tool_name": "read_file",
            "params": { "path": "src/main.rs" },
        }),
    )?;
    storage.append_provenance_event(
        "trace-search-text-001",
        "search_text",
        &json!({
            "tool_name": "search_text",
            "params": { "query": "hello" },
        }),
    )?;

    let rows = storage.load_provenance_events_for_tool("read_file", 5)?;
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().all(|row| row.tool_name == "read_file"),
        "expected only read_file provenance rows"
    );
    assert!(
        rows.iter()
            .all(|row| row.payload_json.contains("\"tool_name\":\"read_file\"")),
        "expected serialized payloads to include the tool_name marker"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn provenance_path_resolution_for_write_creates_parent_within_canonical_root() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("provenance-path-safe");
    fs::create_dir_all(&workspace_root).map_err(FriggError::Io)?;

    let db_path = ensure_provenance_db_parent_dir(&workspace_root)?;
    let canonical_root = workspace_root.canonicalize().map_err(FriggError::Io)?;
    let expected = canonical_root
        .join(PROVENANCE_STORAGE_DIR)
        .join(PROVENANCE_STORAGE_DB_FILE);

    assert_eq!(db_path, expected);
    let parent = db_path
        .parent()
        .expect("resolved provenance db path should always have a parent");
    assert!(
        parent.is_dir(),
        "expected provenance storage parent directory to exist"
    );

    let resolved = resolve_provenance_db_path(&workspace_root)?;
    assert_eq!(resolved, expected);

    cleanup_workspace(&workspace_root);
    Ok(())
}

#[test]
fn workspace_write_path_resolution_creates_nested_parent_inside_workspace() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("workspace-write-valid-nested");
    fs::create_dir_all(&workspace_root).map_err(FriggError::Io)?;

    let target = resolve_workspace_relative_write_path(
        &workspace_root,
        Path::new(".github/workflows/frigg.yml"),
    )?;
    let canonical_root = workspace_root.canonicalize().map_err(FriggError::Io)?;

    assert_eq!(target, canonical_root.join(".github/workflows/frigg.yml"));
    assert!(
        target
            .parent()
            .expect("resolved write target should have a parent")
            .is_dir(),
        "valid nested write target should create missing parent directories"
    );
    assert!(
        target.starts_with(&canonical_root),
        "resolved write target must stay inside the canonical workspace root"
    );

    cleanup_workspace(&workspace_root);
    Ok(())
}

#[test]
fn workspace_write_path_resolution_rejects_absolute_target_path() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("workspace-write-absolute");
    fs::create_dir_all(&workspace_root).map_err(FriggError::Io)?;
    let outside_target = env::temp_dir().join("frigg-absolute-target.yml");

    let err = resolve_workspace_relative_write_path(&workspace_root, &outside_target)
        .expect_err("absolute target path should be rejected");

    assert!(
        matches!(err, FriggError::AccessDenied(_)),
        "expected access denied for absolute target path, got {err}"
    );

    cleanup_workspace(&workspace_root);
    Ok(())
}

#[test]
fn workspace_write_path_resolution_rejects_parent_traversal() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("workspace-write-parent-traversal");
    fs::create_dir_all(&workspace_root).map_err(FriggError::Io)?;

    let err = resolve_workspace_relative_write_path(
        &workspace_root,
        Path::new(".github/../outside/frigg.yml"),
    )
    .expect_err("parent traversal target path should be rejected");

    assert!(
        matches!(err, FriggError::AccessDenied(_)),
        "expected access denied for parent traversal target path, got {err}"
    );
    assert!(
        !workspace_root.join("outside").exists(),
        "parent traversal rejection must not create escaped directories"
    );

    cleanup_workspace(&workspace_root);
    Ok(())
}

#[cfg(unix)]
#[test]
fn provenance_path_resolution_rejects_symlink_escape_before_write() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("provenance-path-symlink-escape");
    let repo_root = workspace_root.join("repo");
    let escaped_root = workspace_root.join("escaped-store");
    fs::create_dir_all(&repo_root).map_err(FriggError::Io)?;
    fs::create_dir_all(&escaped_root).map_err(FriggError::Io)?;

    let provenance_link = repo_root.join(PROVENANCE_STORAGE_DIR);
    create_dir_symlink(&escaped_root, &provenance_link)?;

    let resolve_err = resolve_provenance_db_path(&repo_root)
        .expect_err("symlink escape should be rejected while resolving provenance db path");
    assert!(
        matches!(resolve_err, FriggError::AccessDenied(_)),
        "expected access denied for symlink escape, got {resolve_err}"
    );

    let prepare_err = ensure_provenance_db_parent_dir(&repo_root)
        .expect_err("symlink escape should be rejected before creating storage parent dir");
    assert!(
        matches!(
            prepare_err,
            FriggError::AccessDenied(ref message)
                if message.contains("escapes canonical workspace root boundary")
        ),
        "expected access denied for symlink escape, got {prepare_err}"
    );

    assert!(
        !escaped_root.join(PROVENANCE_STORAGE_DB_FILE).exists(),
        "provenance db write should not escape via symlinked storage directory"
    );

    let _ = fs::remove_file(&provenance_link);
    cleanup_workspace(&workspace_root);
    Ok(())
}

#[cfg(unix)]
#[test]
fn workspace_write_path_resolution_rejects_symlinked_adopt_parent_escapes() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("workspace-write-symlink-escapes");
    let repo_root = workspace_root.join("repo");
    let escaped_root = workspace_root.join("escaped-target");
    fs::create_dir_all(&repo_root).map_err(FriggError::Io)?;
    fs::create_dir_all(&escaped_root).map_err(FriggError::Io)?;
    fs::write(escaped_root.join("sentinel.txt"), "unchanged").map_err(FriggError::Io)?;

    let fixture_targets = [
        (".github", "workflows/frigg.yml", "workflows"),
        (".claude", "commands/frigg.md", "commands"),
        (".cursor", "rules/frigg.mdc", "rules"),
    ];

    for (symlink_dir, child_path, escaped_child_dir) in fixture_targets {
        let link_path = repo_root.join(symlink_dir);
        create_dir_symlink(&escaped_root, &link_path)?;

        let err = resolve_workspace_relative_write_path(
            &repo_root,
            &Path::new(symlink_dir).join(child_path),
        )
        .expect_err("symlinked adopt parent escape should be rejected before writes");

        assert!(
            matches!(
                err,
                FriggError::AccessDenied(ref message)
                    if message.contains("escapes canonical workspace root boundary")
            ),
            "expected access denied for symlinked {symlink_dir} escape, got {err}"
        );
        assert!(
            !escaped_root.join(child_path).exists(),
            "resolver must not create escaped target file for {symlink_dir}"
        );
        assert!(
            !escaped_root.join(escaped_child_dir).exists(),
            "resolver must not create escaped target directory for {symlink_dir}"
        );
        assert_eq!(
            fs::read_to_string(escaped_root.join("sentinel.txt")).map_err(FriggError::Io)?,
            "unchanged",
            "resolver must not modify existing escaped target state for {symlink_dir}"
        );

        fs::remove_file(&link_path).map_err(FriggError::Io)?;
    }

    cleanup_workspace(&workspace_root);
    Ok(())
}
