//! Current SQLite schema for repository storage.
//!
//! Frigg treats `.frigg/storage.sqlite3` as regenerable local index state. Empty databases are
//! initialized with this schema; existing databases with another schema version are rejected so the
//! user can delete and rebuild them instead of running migrations.

/// Latest durable storage schema version written to empty SQLite databases.
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 11;

/// Full SQLite DDL for the current durable storage schema.
pub(crate) const CURRENT_SCHEMA_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS schema_version (
      id INTEGER PRIMARY KEY CHECK (id = 1),
      version INTEGER NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS repository (
      repository_id TEXT PRIMARY KEY,
      root_path TEXT NOT NULL,
      display_name TEXT NOT NULL,
      created_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS snapshot (
      snapshot_id TEXT PRIMARY KEY,
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      kind TEXT NOT NULL,
      revision TEXT,
      created_at TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_snapshot_repository_created_snapshot
    ON snapshot (repository_id, created_at DESC, snapshot_id DESC);

    CREATE TABLE IF NOT EXISTS file_manifest (
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      path TEXT NOT NULL,
      sha256 TEXT NOT NULL,
      size_bytes INTEGER NOT NULL,
      mtime_ns INTEGER,
      PRIMARY KEY (snapshot_id, path)
    );

    CREATE TABLE IF NOT EXISTS semantic_head (
      repository_id TEXT NOT NULL,
      provider TEXT NOT NULL,
      model TEXT NOT NULL,
      covered_snapshot_id TEXT NOT NULL,
      live_chunk_count INTEGER NOT NULL DEFAULT 0,
      last_refresh_reason TEXT,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, provider, model)
    );

    CREATE INDEX IF NOT EXISTS idx_semantic_head_repo_snapshot
    ON semantic_head (repository_id, covered_snapshot_id, provider, model);

    CREATE TABLE IF NOT EXISTS semantic_chunk (
      repository_id TEXT NOT NULL,
      provider TEXT NOT NULL,
      model TEXT NOT NULL,
      chunk_id TEXT NOT NULL,
      snapshot_id TEXT NOT NULL,
      path TEXT NOT NULL,
      language TEXT NOT NULL,
      chunk_index INTEGER NOT NULL,
      start_line INTEGER NOT NULL,
      end_line INTEGER NOT NULL,
      content_hash_blake3 TEXT NOT NULL,
      content_text TEXT NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, provider, model, chunk_id)
    );

    CREATE INDEX IF NOT EXISTS idx_semantic_chunk_repo_model_snapshot_path_chunk
    ON semantic_chunk (repository_id, provider, model, snapshot_id, path, chunk_index, chunk_id);

    CREATE INDEX IF NOT EXISTS idx_semantic_chunk_repo_snapshot_path_model
    ON semantic_chunk (repository_id, snapshot_id, path, provider, model, chunk_id);

    CREATE TABLE IF NOT EXISTS semantic_chunk_embedding (
      repository_id TEXT NOT NULL,
      provider TEXT NOT NULL,
      model TEXT NOT NULL,
      chunk_id TEXT NOT NULL,
      snapshot_id TEXT NOT NULL,
      trace_id TEXT,
      embedding_blob BLOB NOT NULL,
      dimensions INTEGER NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, provider, model, chunk_id)
    );

    CREATE INDEX IF NOT EXISTS idx_semantic_chunk_embedding_repo_model_snapshot_chunk
    ON semantic_chunk_embedding (repository_id, provider, model, snapshot_id, chunk_id);

    CREATE INDEX IF NOT EXISTS idx_semantic_chunk_embedding_repo_snapshot_model_chunk
    ON semantic_chunk_embedding (repository_id, snapshot_id, provider, model, chunk_id);

    CREATE TABLE IF NOT EXISTS path_witness_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      path TEXT NOT NULL,
      path_class TEXT NOT NULL,
      source_class TEXT NOT NULL,
      path_terms_json TEXT NOT NULL,
      flags_json TEXT NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      file_stem TEXT NOT NULL DEFAULT '',
      subtree_root TEXT,
      family_bits INTEGER NOT NULL DEFAULT 0,
      heuristic_version INTEGER NOT NULL DEFAULT 0,
      PRIMARY KEY (repository_id, snapshot_id, path)
    );

    CREATE INDEX IF NOT EXISTS idx_path_witness_projection_repo_snapshot_path
    ON path_witness_projection (repository_id, snapshot_id, path);

    CREATE TABLE IF NOT EXISTS test_subject_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      test_path TEXT NOT NULL,
      subject_path TEXT NOT NULL,
      shared_terms_json TEXT NOT NULL,
      score_hint INTEGER NOT NULL,
      flags_json TEXT NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, test_path, subject_path)
    );

    CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_test
    ON test_subject_projection (repository_id, snapshot_id, test_path, subject_path);

    CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_subject
    ON test_subject_projection (repository_id, snapshot_id, subject_path, test_path);

    CREATE TABLE IF NOT EXISTS entrypoint_surface_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      path TEXT NOT NULL,
      path_class TEXT NOT NULL,
      source_class TEXT NOT NULL,
      path_terms_json TEXT NOT NULL,
      surface_terms_json TEXT NOT NULL,
      flags_json TEXT NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, path)
    );

    CREATE INDEX IF NOT EXISTS idx_entrypoint_surface_projection_repo_snapshot_path
    ON entrypoint_surface_projection (repository_id, snapshot_id, path);

    CREATE TABLE IF NOT EXISTS retrieval_projection_head (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      family TEXT NOT NULL,
      heuristic_version INTEGER NOT NULL,
      input_modes_json TEXT NOT NULL,
      row_count INTEGER NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, family)
    );

    CREATE INDEX IF NOT EXISTS idx_retrieval_projection_head_repo_snapshot_family
    ON retrieval_projection_head (repository_id, snapshot_id, family);

    CREATE TABLE IF NOT EXISTS path_relation_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      src_path TEXT NOT NULL,
      dst_path TEXT NOT NULL,
      relation_kind TEXT NOT NULL,
      evidence_source TEXT NOT NULL,
      src_symbol_id TEXT,
      dst_symbol_id TEXT,
      src_family_bits INTEGER NOT NULL DEFAULT 0,
      dst_family_bits INTEGER NOT NULL DEFAULT 0,
      shared_terms_json TEXT NOT NULL,
      score_hint INTEGER NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, src_path, dst_path, relation_kind)
    );

    CREATE INDEX IF NOT EXISTS idx_path_relation_projection_repo_snapshot_src
    ON path_relation_projection (repository_id, snapshot_id, src_path, relation_kind, dst_path);

    CREATE INDEX IF NOT EXISTS idx_path_relation_projection_repo_snapshot_dst
    ON path_relation_projection (repository_id, snapshot_id, dst_path, relation_kind, src_path);

    CREATE TABLE IF NOT EXISTS subtree_coverage_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      subtree_root TEXT NOT NULL,
      family TEXT NOT NULL,
      path_count INTEGER NOT NULL,
      exemplar_path TEXT NOT NULL,
      exemplar_score_hint INTEGER NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, subtree_root, family)
    );

    CREATE INDEX IF NOT EXISTS idx_subtree_coverage_projection_repo_snapshot_subtree
    ON subtree_coverage_projection (repository_id, snapshot_id, subtree_root, family);

    CREATE TABLE IF NOT EXISTS path_surface_term_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      path TEXT NOT NULL,
      term_weights_json TEXT NOT NULL,
      exact_terms_json TEXT NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, path)
    );

    CREATE INDEX IF NOT EXISTS idx_path_surface_term_projection_repo_snapshot_path
    ON path_surface_term_projection (repository_id, snapshot_id, path);

    CREATE TABLE IF NOT EXISTS path_anchor_sketch_projection (
      repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
      snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
      path TEXT NOT NULL,
      anchor_rank INTEGER NOT NULL,
      line INTEGER NOT NULL,
      anchor_kind TEXT NOT NULL,
      excerpt TEXT NOT NULL,
      terms_json TEXT NOT NULL,
      score_hint INTEGER NOT NULL,
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      PRIMARY KEY (repository_id, snapshot_id, path, anchor_rank)
    );

    CREATE INDEX IF NOT EXISTS idx_path_anchor_sketch_projection_repo_snapshot_path
    ON path_anchor_sketch_projection (repository_id, snapshot_id, path, anchor_rank);
"#;

/// Tables that must exist before storage accepts an existing database file.
pub(crate) const REQUIRED_TABLES: &[&str] = &[
    "schema_version",
    "repository",
    "snapshot",
    "file_manifest",
    "semantic_head",
    "semantic_chunk",
    "semantic_chunk_embedding",
    "path_witness_projection",
    "test_subject_projection",
    "entrypoint_surface_projection",
    "retrieval_projection_head",
    "path_relation_projection",
    "subtree_coverage_projection",
    "path_surface_term_projection",
    "path_anchor_sketch_projection",
];
