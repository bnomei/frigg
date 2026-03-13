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
