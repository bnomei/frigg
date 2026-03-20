#[derive(Debug, Clone, Copy)]
pub(crate) struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: r#"
            CREATE TABLE IF NOT EXISTS repository (
              repository_id TEXT PRIMARY KEY,
              root_path TEXT NOT NULL,
              display_name TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS snapshot (
              snapshot_id TEXT PRIMARY KEY,
              repository_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              revision TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_manifest (
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              sha256 TEXT NOT NULL,
              size_bytes INTEGER NOT NULL,
              mtime_ns INTEGER,
              PRIMARY KEY (snapshot_id, path)
            );

            CREATE TABLE IF NOT EXISTS provenance_event (
              trace_id TEXT NOT NULL,
              tool_name TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              PRIMARY KEY (trace_id, tool_name, created_at)
            );
        "#,
    },
    Migration {
        version: 2,
        sql: r#"
            CREATE INDEX IF NOT EXISTS idx_snapshot_repository_created_snapshot
            ON snapshot (repository_id, created_at DESC, snapshot_id DESC);

            CREATE INDEX IF NOT EXISTS idx_provenance_tool_created_trace
            ON provenance_event (tool_name, created_at DESC, trace_id DESC);
        "#,
    },
    Migration {
        version: 3,
        sql: r#"
            CREATE TABLE IF NOT EXISTS semantic_chunk_embedding (
              chunk_id TEXT PRIMARY KEY,
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              language TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_line INTEGER NOT NULL,
              end_line INTEGER NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              trace_id TEXT,
              content_hash_blake3 TEXT NOT NULL,
              content_text TEXT NOT NULL,
              embedding_blob BLOB NOT NULL,
              dimensions INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_semantic_chunk_embedding_repo_snapshot_path_chunk
            ON semantic_chunk_embedding (repository_id, snapshot_id, path, chunk_index, chunk_id);

            CREATE INDEX IF NOT EXISTS idx_semantic_chunk_embedding_repo_chunk
            ON semantic_chunk_embedding (repository_id, chunk_id);
        "#,
    },
    Migration {
        version: 4,
        sql: r#"
            ALTER TABLE semantic_chunk_embedding RENAME TO semantic_chunk_embedding_v3_legacy;

            CREATE TABLE semantic_chunk (
              chunk_id TEXT NOT NULL,
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              language TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_line INTEGER NOT NULL,
              end_line INTEGER NOT NULL,
              content_text TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, chunk_id)
            );

            CREATE INDEX idx_semantic_chunk_repo_snapshot_path_chunk
            ON semantic_chunk (repository_id, snapshot_id, path, chunk_index, chunk_id);

            CREATE TABLE semantic_chunk_embedding (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              chunk_id TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              trace_id TEXT,
              content_hash_blake3 TEXT NOT NULL,
              embedding_blob BLOB NOT NULL,
              dimensions INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, chunk_id, provider, model)
            );

            CREATE INDEX idx_semantic_chunk_embedding_repo_snapshot_model_chunk
            ON semantic_chunk_embedding (repository_id, snapshot_id, provider, model, chunk_id);

            CREATE INDEX idx_semantic_chunk_embedding_repo_model_snapshot_chunk
            ON semantic_chunk_embedding (repository_id, provider, model, snapshot_id, chunk_id);

            INSERT INTO semantic_chunk (
              chunk_id,
              repository_id,
              snapshot_id,
              path,
              language,
              chunk_index,
              start_line,
              end_line,
              content_text,
              created_at
            )
            SELECT DISTINCT
              chunk_id,
              repository_id,
              snapshot_id,
              path,
              language,
              chunk_index,
              start_line,
              end_line,
              content_text,
              created_at
            FROM semantic_chunk_embedding_v3_legacy;

            INSERT INTO semantic_chunk_embedding (
              repository_id,
              snapshot_id,
              chunk_id,
              provider,
              model,
              trace_id,
              content_hash_blake3,
              embedding_blob,
              dimensions,
              created_at
            )
            SELECT
              repository_id,
              snapshot_id,
              chunk_id,
              provider,
              model,
              trace_id,
              content_hash_blake3,
              embedding_blob,
              dimensions,
              created_at
            FROM semantic_chunk_embedding_v3_legacy;

            DROP TABLE semantic_chunk_embedding_v3_legacy;
        "#,
    },
    Migration {
        version: 5,
        sql: r#"
            CREATE TABLE IF NOT EXISTS path_witness_projection (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              path_class TEXT NOT NULL,
              source_class TEXT NOT NULL,
              path_terms_json TEXT NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_path_witness_projection_repo_snapshot_path
            ON path_witness_projection (repository_id, snapshot_id, path);
        "#,
    },
    Migration {
        version: 6,
        sql: r#"
            DROP TABLE IF EXISTS semantic_chunk_embedding;
            DROP TABLE IF EXISTS semantic_chunk;
            DROP TABLE IF EXISTS semantic_head;

            CREATE TABLE semantic_head (
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

            CREATE INDEX idx_semantic_head_repo_snapshot
            ON semantic_head (repository_id, covered_snapshot_id, provider, model);

            CREATE TABLE semantic_chunk (
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

            CREATE INDEX idx_semantic_chunk_repo_model_snapshot_path_chunk
            ON semantic_chunk (repository_id, provider, model, snapshot_id, path, chunk_index, chunk_id);

            CREATE INDEX idx_semantic_chunk_repo_snapshot_path_model
            ON semantic_chunk (repository_id, snapshot_id, path, provider, model, chunk_id);

            CREATE TABLE semantic_chunk_embedding (
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

            CREATE INDEX idx_semantic_chunk_embedding_repo_model_snapshot_chunk
            ON semantic_chunk_embedding (repository_id, provider, model, snapshot_id, chunk_id);

            CREATE INDEX idx_semantic_chunk_embedding_repo_snapshot_model_chunk
            ON semantic_chunk_embedding (repository_id, snapshot_id, provider, model, chunk_id);
        "#,
    },
    Migration {
        version: 7,
        sql: r#"
            CREATE TABLE IF NOT EXISTS test_subject_projection (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
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
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
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
        "#,
    },
    Migration {
        version: 8,
        sql: r#"
            ALTER TABLE snapshot RENAME TO snapshot_v8;

            INSERT INTO repository (repository_id, root_path, display_name, created_at)
            SELECT DISTINCT
                snapshot_v8.repository_id,
                '/legacy-import',
                snapshot_v8.repository_id,
                CURRENT_TIMESTAMP
            FROM snapshot_v8
            WHERE NOT EXISTS (
                SELECT 1
                FROM repository
                WHERE repository.repository_id = snapshot_v8.repository_id
            );

            CREATE TABLE snapshot (
              snapshot_id TEXT PRIMARY KEY,
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              kind TEXT NOT NULL,
              revision TEXT,
              created_at TEXT NOT NULL
            );

            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            SELECT snapshot_id, repository_id, kind, revision, created_at
            FROM snapshot_v8;

            DROP TABLE snapshot_v8;

            CREATE INDEX IF NOT EXISTS idx_snapshot_repository_created_snapshot
            ON snapshot (repository_id, created_at DESC, snapshot_id DESC);

            ALTER TABLE file_manifest RENAME TO file_manifest_v8;

            CREATE TABLE IF NOT EXISTS file_manifest (
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              sha256 TEXT NOT NULL,
              size_bytes INTEGER NOT NULL,
              mtime_ns INTEGER,
              PRIMARY KEY (snapshot_id, path)
            );

            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            SELECT snapshot_id, path, sha256, size_bytes, mtime_ns
            FROM file_manifest_v8;

            DROP TABLE file_manifest_v8;

            ALTER TABLE path_witness_projection RENAME TO path_witness_projection_v8;

            CREATE TABLE IF NOT EXISTS path_witness_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              path_class TEXT NOT NULL,
              source_class TEXT NOT NULL,
              path_terms_json TEXT NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            INSERT INTO path_witness_projection (
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                flags_json,
                created_at
            )
            SELECT
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                flags_json,
                created_at
            FROM path_witness_projection_v8;

            DROP TABLE path_witness_projection_v8;

            CREATE INDEX IF NOT EXISTS idx_path_witness_projection_repo_snapshot_path
            ON path_witness_projection (repository_id, snapshot_id, path);

            ALTER TABLE test_subject_projection RENAME TO test_subject_projection_v8;

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

            INSERT INTO test_subject_projection (
                repository_id,
                snapshot_id,
                test_path,
                subject_path,
                shared_terms_json,
                score_hint,
                flags_json,
                created_at
            )
            SELECT
                repository_id,
                snapshot_id,
                test_path,
                subject_path,
                shared_terms_json,
                score_hint,
                flags_json,
                created_at
            FROM test_subject_projection_v8;

            DROP TABLE test_subject_projection_v8;

            CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_test
            ON test_subject_projection (repository_id, snapshot_id, test_path, subject_path);

            CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_subject
            ON test_subject_projection (repository_id, snapshot_id, subject_path, test_path);

            ALTER TABLE entrypoint_surface_projection RENAME TO entrypoint_surface_projection_v8;

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

            INSERT INTO entrypoint_surface_projection (
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                surface_terms_json,
                flags_json,
                created_at
            )
            SELECT
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                surface_terms_json,
                flags_json,
                created_at
            FROM entrypoint_surface_projection_v8;

            DROP TABLE entrypoint_surface_projection_v8;

            CREATE INDEX IF NOT EXISTS idx_entrypoint_surface_projection_repo_snapshot_path
            ON entrypoint_surface_projection (repository_id, snapshot_id, path);
        "#,
    },
    Migration {
        version: 9,
        sql: r#"
            ALTER TABLE path_witness_projection
            ADD COLUMN file_stem TEXT NOT NULL DEFAULT '';

            ALTER TABLE path_witness_projection
            ADD COLUMN subtree_root TEXT;

            ALTER TABLE path_witness_projection
            ADD COLUMN family_bits INTEGER NOT NULL DEFAULT 0;

            ALTER TABLE path_witness_projection
            ADD COLUMN heuristic_version INTEGER NOT NULL DEFAULT 0;

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
        "#,
    },
];

pub(crate) const REQUIRED_TABLES: &[&str] = &[
    "schema_version",
    "repository",
    "snapshot",
    "file_manifest",
    "provenance_event",
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
