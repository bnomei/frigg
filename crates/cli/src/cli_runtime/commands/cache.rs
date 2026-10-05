//! Portable cache creation, validation, relocation, and installation for release-seeded workspaces.
//!
//! The ZIP archive is an external-state trust boundary: every entry is allowlisted, size-bounded,
//! and content-addressed before its staged SQLite database receives full storage validation. Load
//! localizes the sole repository partition to the destination checkout, then installs database and
//! SCIP state with online SQLite backup and coordinated error rollback.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use frigg::domain::model::stable_repository_id_for_root;
use frigg::searcher::stable_cache_fingerprint_hex;
use frigg::settings::FriggConfig;
use frigg::storage::{
    PROVENANCE_STORAGE_DIR, Storage, ensure_provenance_db_parent_dir,
    latest_storage_schema_version, resolve_provenance_db_path,
    resolve_workspace_relative_write_path,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::cli_runtime::{CliOutput, OutputLevel, field};

const CACHE_FORMAT_VERSION: u32 = 1;
const CACHE_MANIFEST_PATH: &str = "manifest.json";
const CACHE_DB_PATH: &str = "storage.sqlite3";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PAYLOAD_FILES: usize = 100_000;
const MAX_PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct CacheManifest {
    format_version: u32,
    frigg_version: String,
    compatibility_fingerprint: String,
    storage_schema_version: i64,
    source_commit: String,
    repository_id: String,
    semantic_partitions: Vec<SemanticPartition>,
    includes_scip: bool,
    files: Vec<CacheFile>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SemanticPartition {
    provider: String,
    model: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    path: String,
    blake3: String,
    size_bytes: u64,
}

/// Creates a consistent portable archive for the command's single workspace.
///
/// SQLite is captured through the online-backup API, while semantic rows and staged SCIP files are
/// included automatically. The archive records the source commit and build compatibility contract.
pub(crate) fn run_cache_make_command(
    config: &FriggConfig,
    archive_path: &Path,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let root = single_workspace_root(config)?;
    let db_path = resolve_provenance_db_path(root)?;
    if !db_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Frigg storage does not exist at {}; run `frigg index` first",
                db_path.display()
            ),
        )
        .into());
    }

    let scratch = root.join(format!(".frigg-cache-make-{}", Uuid::now_v7()));
    fs::create_dir(&scratch)?;
    let result = make_archive(root, &db_path, archive_path, &scratch);
    let _ = fs::remove_dir_all(&scratch);
    result?;

    output.summary_event(
        OutputLevel::Ok,
        "cache",
        "made",
        &[
            field("status", "ok"),
            field("archive", archive_path.display()),
        ],
        None,
    )?;
    Ok(())
}

fn make_archive(
    root: &Path,
    db_path: &Path,
    archive_path: &Path,
    scratch: &Path,
) -> Result<(), Box<dyn Error>> {
    let backup_path = scratch.join(CACHE_DB_PATH);
    backup_database(db_path, &backup_path)?;

    let storage = Storage::new(&backup_path);
    storage.verify_portable_cache()?;
    let connection = Connection::open(&backup_path)?;
    let schema_version = storage_schema_version(&connection)?;
    if schema_version != latest_storage_schema_version() {
        return Err(io::Error::other(format!(
            "storage schema version {schema_version} is not current version {}",
            latest_storage_schema_version()
        ))
        .into());
    }
    let repository_id = sole_repository_id(&connection)?;
    let semantic_partitions = semantic_partitions(&connection, &repository_id)?;
    drop(connection);

    let mut payloads = vec![(CACHE_DB_PATH.to_owned(), backup_path)];
    let scip_root = root.join(PROVENANCE_STORAGE_DIR).join("scip");
    if scip_root.is_dir() {
        let staged_scip_root = scratch.join("scip");
        for entry in WalkDir::new(&scip_root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry.path().strip_prefix(&scip_root)?;
            let archive_name = Path::new("scip").join(relative);
            let staged_path = staged_scip_root.join(relative);
            if let Some(parent) = staged_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &staged_path)?;
            payloads.push((zip_path(&archive_name)?, staged_path));
        }
    }
    payloads.sort_by(|left, right| left.0.cmp(&right.0));

    let files = payloads
        .iter()
        .map(|(path, source)| {
            Ok(CacheFile {
                path: path.clone(),
                blake3: blake3::hash(&fs::read(source)?).to_hex().to_string(),
                size_bytes: source.metadata()?.len(),
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let manifest = CacheManifest {
        format_version: CACHE_FORMAT_VERSION,
        frigg_version: env!("CARGO_PKG_VERSION").to_owned(),
        compatibility_fingerprint: stable_cache_fingerprint_hex(),
        storage_schema_version: schema_version,
        source_commit: git_commit(root)?,
        repository_id,
        semantic_partitions,
        includes_scip: payloads.iter().any(|(path, _)| path.starts_with("scip/")),
        files,
    };

    let archive_parent = archive_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(archive_parent)?;
    let temporary_archive = archive_parent.join(format!(".frigg-cache-{}.tmp", Uuid::now_v7()));
    let archive_file = File::create(&temporary_archive)?;
    let mut zip = ZipWriter::new(archive_file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(CACHE_MANIFEST_PATH, options)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for (path, source) in payloads {
        zip.start_file(path, options)?;
        io::copy(&mut File::open(source)?, &mut zip)?;
    }
    zip.finish()?.sync_all()?;
    if archive_path.exists() {
        fs::remove_file(archive_path)?;
    }
    fs::rename(temporary_archive, archive_path)?;
    Ok(())
}

/// Validates and installs a portable archive into the command's single workspace.
///
/// No destination state is replaced until all archive metadata, payloads, and staged storage pass
/// validation. Installation errors trigger restoration of the previous database and SCIP directory;
/// rollback failures are preserved in the returned error.
pub(crate) fn run_cache_load_command(
    config: &FriggConfig,
    archive_path: &Path,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let root = single_workspace_root(config)?;
    let scratch = root.join(format!(".frigg-cache-load-{}", Uuid::now_v7()));
    fs::create_dir(&scratch)?;
    let result = load_archive(root, archive_path, &scratch);
    let _ = fs::remove_dir_all(&scratch);
    let manifest = result?;

    output.summary_event(
        OutputLevel::Ok,
        "cache",
        "loaded",
        &[
            field("status", "ok"),
            field("archive", archive_path.display()),
            field("source_commit", manifest.source_commit),
        ],
        None,
    )?;
    Ok(())
}

fn load_archive(
    root: &Path,
    archive_path: &Path,
    scratch: &Path,
) -> Result<CacheManifest, Box<dyn Error>> {
    let mut archive = ZipArchive::new(File::open(archive_path)?)?;
    if archive.len() > MAX_PAYLOAD_FILES.saturating_add(1) {
        return Err(io::Error::other("cache archive contains too many entries").into());
    }
    let entries = archive_entries(&mut archive)?;
    let manifest_index = *entries
        .get(CACHE_MANIFEST_PATH)
        .ok_or_else(|| io::Error::other("cache archive does not contain manifest.json"))?;
    let manifest: CacheManifest = {
        let mut file = archive.by_index(manifest_index)?;
        let bytes = read_bounded(&mut file, MAX_MANIFEST_BYTES, CACHE_MANIFEST_PATH)?;
        serde_json::from_slice(&bytes)?
    };
    validate_manifest(&manifest)?;

    let expected_paths = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(std::iter::once(CACHE_MANIFEST_PATH))
        .collect::<BTreeSet<_>>();
    let actual_paths = entries.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if actual_paths != expected_paths {
        return Err(
            io::Error::other("cache archive entries do not exactly match the manifest").into(),
        );
    }

    for expected in &manifest.files {
        let destination = archive_destination(scratch, &expected.path)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let entry_index = *entries
            .get(&expected.path)
            .ok_or_else(|| io::Error::other("cache payload is missing"))?;
        let mut source = archive.by_index(entry_index)?;
        if !source.is_file() {
            return Err(
                io::Error::other(format!("cache entry is not a file: {}", expected.path)).into(),
            );
        }
        let mut destination_file = File::create(&destination)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            destination_file.write_all(&buffer[..read])?;
            if destination_file.stream_position()? > expected.size_bytes {
                return Err(io::Error::other(format!(
                    "cache payload exceeds declared size: {}",
                    expected.path
                ))
                .into());
            }
        }
        if destination_file.stream_position()? != expected.size_bytes {
            return Err(io::Error::other(format!(
                "cache payload size mismatch for {}",
                expected.path
            ))
            .into());
        }
        if hasher.finalize().to_hex().as_str() != expected.blake3 {
            return Err(
                io::Error::other(format!("cache checksum mismatch for {}", expected.path)).into(),
            );
        }
    }

    let loaded_db = scratch.join(CACHE_DB_PATH);
    let loaded_storage = Storage::new(&loaded_db);
    loaded_storage.verify_portable_cache()?;
    let connection =
        Connection::open_with_flags(&loaded_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let schema_version = storage_schema_version(&connection)?;
    if schema_version != manifest.storage_schema_version {
        return Err(io::Error::other("cache database schema does not match its manifest").into());
    }
    if sole_repository_id(&connection)? != manifest.repository_id {
        return Err(
            io::Error::other("cache database repository does not match its manifest").into(),
        );
    }
    if semantic_partitions(&connection, &manifest.repository_id)? != manifest.semantic_partitions {
        return Err(io::Error::other(
            "cache database semantic partitions do not match its manifest",
        )
        .into());
    }
    drop(connection);

    let destination_repository_id = stable_repository_id_for_root(root).0;
    rebase_repository_partition(
        &loaded_db,
        &manifest.repository_id,
        &destination_repository_id,
        root,
    )?;
    loaded_storage.verify_portable_cache()?;

    let loaded_scip = scratch.join("scip");
    install_payloads(
        root,
        &loaded_db,
        loaded_scip.is_dir().then_some(&loaded_scip),
        scratch,
    )?;
    Ok(manifest)
}

fn validate_manifest(manifest: &CacheManifest) -> io::Result<()> {
    if manifest.format_version != CACHE_FORMAT_VERSION {
        return Err(io::Error::other(format!(
            "unsupported Frigg cache format {}",
            manifest.format_version
        )));
    }
    if manifest.frigg_version != env!("CARGO_PKG_VERSION") {
        return Err(io::Error::other(format!(
            "cache requires Frigg {}, but this is Frigg {}",
            manifest.frigg_version,
            env!("CARGO_PKG_VERSION")
        )));
    }
    if manifest.compatibility_fingerprint != stable_cache_fingerprint_hex()
        || manifest.storage_schema_version != latest_storage_schema_version()
    {
        return Err(io::Error::other(
            "cache is incompatible with this Frigg build",
        ));
    }
    if !matches!(manifest.source_commit.len(), 40 | 64)
        || !manifest
            .source_commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(io::Error::other(
            "cache source commit is not a full Git object id",
        ));
    }
    if !manifest.files.iter().any(|file| file.path == CACHE_DB_PATH) {
        return Err(io::Error::other(
            "cache manifest does not contain storage.sqlite3",
        ));
    }
    if manifest.files.len() > MAX_PAYLOAD_FILES {
        return Err(io::Error::other(
            "cache manifest contains too many payloads",
        ));
    }
    let total_size = manifest.files.iter().try_fold(0_u64, |total, file| {
        validate_archive_path(&file.path)?;
        if file.blake3.len() != 64 || !file.blake3.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::other(format!(
                "cache payload has invalid BLAKE3 digest: {}",
                file.path
            )));
        }
        total
            .checked_add(file.size_bytes)
            .ok_or_else(|| io::Error::other("cache payload size overflow"))
    })?;
    if total_size > MAX_PAYLOAD_BYTES {
        return Err(io::Error::other("cache payloads exceed the size limit"));
    }
    let mut paths = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(io::Error::other(
            "cache manifest contains duplicate payload paths",
        ));
    }
    let has_scip = manifest
        .files
        .iter()
        .any(|file| file.path.starts_with("scip/"));
    if has_scip != manifest.includes_scip {
        return Err(io::Error::other(
            "cache manifest SCIP metadata does not match its payload",
        ));
    }
    Ok(())
}

fn single_workspace_root(config: &FriggConfig) -> io::Result<&Path> {
    match config.workspace_roots.as_slice() {
        [root] => Ok(root),
        roots => Err(io::Error::other(format!(
            "cache commands require exactly one workspace root, found {}",
            roots.len()
        ))),
    }
}

fn backup_database(source_path: &Path, destination_path: &Path) -> rusqlite::Result<()> {
    let source = Connection::open(source_path)?;
    let mut destination = Connection::open(destination_path)?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
    backup.run_to_completion(128, Duration::from_millis(10), None)
}

fn sole_repository_id(connection: &Connection) -> io::Result<String> {
    let mut statement = connection
        .prepare("SELECT repository_id FROM repository ORDER BY repository_id")
        .map_err(io::Error::other)?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(io::Error::other)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        _ => Err(io::Error::other(format!(
            "portable caches require exactly one stored repository, found {}",
            ids.len()
        ))),
    }
}

fn storage_schema_version(connection: &Connection) -> io::Result<i64> {
    connection
        .query_row(
            "SELECT version FROM schema_version WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(io::Error::other)
}

fn semantic_partitions(
    connection: &Connection,
    repository_id: &str,
) -> io::Result<Vec<SemanticPartition>> {
    let mut statement = connection
        .prepare("SELECT provider, model FROM semantic_head WHERE repository_id = ?1 ORDER BY provider, model")
        .map_err(io::Error::other)?;
    statement
        .query_map([repository_id], |row| {
            Ok(SemanticPartition {
                provider: row.get(0)?,
                model: row.get(1)?,
            })
        })
        .map_err(io::Error::other)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)
}

fn git_commit(root: &Path) -> io::Result<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "failed to resolve the workspace Git commit",
        ));
    }
    let commit = String::from_utf8(output.stdout).map_err(io::Error::other)?;
    Ok(commit.trim().to_owned())
}

fn zip_path(path: &Path) -> io::Result<String> {
    let parts = path
        .components()
        .map(|component| match component {
            std::path::Component::Normal(part) => part
                .to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other("cache paths must be valid UTF-8")),
            _ => Err(io::Error::other("cache paths must be relative")),
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(parts.join("/"))
}

fn validate_archive_path(path: &str) -> io::Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains('\0')
        || Path::new(path).is_absolute()
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(io::Error::other(format!(
            "unsafe cache archive path: {path}"
        )));
    }
    if path != CACHE_DB_PATH && !path.starts_with("scip/") {
        return Err(io::Error::other(format!(
            "unexpected cache archive path: {path}"
        )));
    }
    Ok(())
}

fn archive_entries<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> io::Result<BTreeMap<String, usize>> {
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(io::Error::other)?;
        let name = entry.name().to_owned();
        if name != CACHE_MANIFEST_PATH {
            validate_archive_path(&name)?;
        }
        if !entry.is_file() {
            return Err(io::Error::other(format!(
                "cache archive entry is not a file: {name}"
            )));
        }
        if entries.insert(name.clone(), index).is_some() {
            return Err(io::Error::other(format!(
                "cache archive contains duplicate entry: {name}"
            )));
        }
    }
    Ok(entries)
}

fn read_bounded(reader: &mut impl Read, limit: u64, label: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(io::Error::other(format!(
            "cache entry exceeds size limit: {label}"
        )));
    }
    Ok(bytes)
}

fn archive_destination(root: &Path, archive_path: &str) -> io::Result<PathBuf> {
    validate_archive_path(archive_path)?;
    Ok(archive_path
        .split('/')
        .fold(root.to_path_buf(), |path, component| path.join(component)))
}

/// Rewrites every repository-scoped relational key for the destination checkout.
///
/// sqlite-vec partition keys cannot be updated in place, so the relational transaction commits
/// first and the vector projection is then rebuilt from the validated semantic embedding rows.
fn rebase_repository_partition(
    db_path: &Path,
    source_repository_id: &str,
    destination_repository_id: &str,
    destination_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut connection = Connection::open(db_path)?;
    let transaction = connection.transaction()?;
    transaction.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
    for table in [
        "snapshot",
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
    ] {
        transaction.execute(
            &format!("UPDATE {table} SET repository_id = ?1 WHERE repository_id = ?2"),
            [destination_repository_id, source_repository_id],
        )?;
    }
    let display_name = destination_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(destination_repository_id);
    let changed = transaction.execute(
        "UPDATE repository
         SET repository_id = ?1, root_path = ?2, display_name = ?3
         WHERE repository_id = ?4",
        rusqlite::params![
            destination_repository_id,
            destination_root.display().to_string(),
            display_name,
            source_repository_id,
        ],
    )?;
    if changed != 1 {
        return Err(io::Error::other("cache repository partition disappeared during load").into());
    }
    transaction.commit()?;
    Storage::new(db_path).repair_semantic_vector_store()?;
    Ok(())
}

/// Installs database and SCIP payloads as one rollback-coordinated operation.
///
/// The database uses SQLite online backup so open WAL-mode connections remain valid. This protects
/// against reported I/O errors; it is not a crash-atomic filesystem transaction.
fn install_payloads(
    root: &Path,
    loaded_db: &Path,
    loaded_scip: Option<&Path>,
    scratch: &Path,
) -> io::Result<()> {
    install_payloads_with(root, loaded_db, loaded_scip, scratch, copy_database)
}

fn install_payloads_with<F>(
    root: &Path,
    loaded_db: &Path,
    loaded_scip: Option<&Path>,
    scratch: &Path,
    install_database: F,
) -> io::Result<()>
where
    F: FnOnce(&Path, &Path) -> io::Result<()>,
{
    let destination_db = ensure_provenance_db_parent_dir(root).map_err(io::Error::other)?;
    let destination_scip = resolve_workspace_relative_write_path(
        root,
        &Path::new(PROVENANCE_STORAGE_DIR).join("scip"),
    )
    .map_err(io::Error::other)?;
    if destination_db.exists() && !destination_db.is_file() {
        return Err(io::Error::other(format!(
            "storage destination is not a file: {}",
            destination_db.display()
        )));
    }

    let previous_db = scratch.join("previous-storage.sqlite3");
    let had_database = destination_db.is_file();
    if had_database {
        copy_database(&destination_db, &previous_db)?;
    }

    let frigg_dir = destination_db
        .parent()
        .ok_or_else(|| io::Error::other("storage destination has no parent"))?;
    let nonce = Uuid::now_v7();
    let staged_scip = frigg_dir.join(format!(".cache-scip-new-{nonce}"));
    let previous_scip = frigg_dir.join(format!(".cache-scip-old-{nonce}"));
    if let Some(loaded_scip) = loaded_scip {
        fs::rename(loaded_scip, &staged_scip)?;
    }
    let had_scip = destination_scip.exists();
    if had_scip {
        fs::rename(&destination_scip, &previous_scip)?;
    }
    if staged_scip.exists()
        && let Err(error) = fs::rename(&staged_scip, &destination_scip)
    {
        if had_scip {
            let _ = fs::rename(&previous_scip, &destination_scip);
        }
        return Err(error);
    }

    if let Err(install_error) = install_database(loaded_db, &destination_db) {
        let database_rollback = if had_database {
            copy_database(&previous_db, &destination_db)
        } else {
            remove_sqlite_database_files(&destination_db)
        };
        let scip_rollback = rollback_scip(&destination_scip, &previous_scip, had_scip);
        if let Err(rollback_error) = database_rollback.and(scip_rollback) {
            return Err(io::Error::other(format!(
                "cache installation failed ({install_error}); rollback also failed ({rollback_error})"
            )));
        }
        return Err(install_error);
    }

    if previous_scip.exists() {
        let _ = remove_path(&previous_scip);
    }
    Ok(())
}

fn rollback_scip(destination: &Path, previous: &Path, had_previous: bool) -> io::Result<()> {
    if destination.exists() {
        remove_path(destination)?;
    }
    if had_previous {
        fs::rename(previous, destination)?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn remove_sqlite_database_files(db_path: &Path) -> io::Result<()> {
    for path in [
        db_path.to_path_buf(),
        PathBuf::from(format!("{}-wal", db_path.display())),
        PathBuf::from(format!("{}-shm", db_path.display())),
        PathBuf::from(format!("{}-journal", db_path.display())),
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn copy_database(source: &Path, destination: &Path) -> io::Result<()> {
    backup_database(source, destination).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use frigg::storage::{ManifestEntry, SemanticChunkEmbeddingRecord, Storage};

    #[test]
    fn cache_round_trip_moves_database_and_scip_between_checkout_paths() {
        let base = std::env::temp_dir().join(format!("frigg-cache-test-{}", Uuid::now_v7()));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        fs::create_dir_all(&source_root).expect("create source root");
        fs::create_dir_all(&destination_root).expect("create destination root");
        seed_git_repository(&source_root);

        let source_db = source_root.join(PROVENANCE_STORAGE_DIR).join(CACHE_DB_PATH);
        fs::create_dir_all(source_db.parent().expect("database parent"))
            .expect("create database parent");
        let storage = Storage::new(&source_db);
        storage.initialize().expect("initialize storage");
        storage
            .upsert_manifest(
                "repo-001",
                "snapshot-001",
                &[ManifestEntry {
                    path: "fixture.rs".to_owned(),
                    sha256: "fixture-hash".to_owned(),
                    size_bytes: 20,
                    mtime_ns: Some(1),
                }],
            )
            .expect("seed manifest");
        storage
            .replace_semantic_embeddings_for_repository(
                "repo-001",
                "snapshot-001",
                "openai",
                "text-embedding-3-small",
                &[SemanticChunkEmbeddingRecord {
                    chunk_id: "chunk-fixture".to_owned(),
                    repository_id: "repo-001".to_owned(),
                    snapshot_id: "snapshot-001".to_owned(),
                    path: "fixture.rs".to_owned(),
                    language: "rust".to_owned(),
                    chunk_index: 0,
                    start_line: 1,
                    end_line: 1,
                    provider: "openai".to_owned(),
                    model: "text-embedding-3-small".to_owned(),
                    trace_id: Some("trace-fixture".to_owned()),
                    content_hash_blake3: "fixture-content".to_owned(),
                    content_text: "pub struct Fixture;".to_owned(),
                    embedding: vec![0.5; frigg::storage::DEFAULT_VECTOR_DIMENSIONS],
                }],
            )
            .expect("seed semantic partition");
        let source_scip = source_root.join(PROVENANCE_STORAGE_DIR).join("scip");
        fs::create_dir_all(&source_scip).expect("create SCIP directory");
        fs::write(source_scip.join("rust.scip"), b"portable-scip").expect("write SCIP fixture");

        let archive = base.join("frigg-cache.zip");
        let make_scratch = base.join("make");
        fs::create_dir(&make_scratch).expect("create make scratch");
        make_archive(&source_root, &source_db, &archive, &make_scratch).expect("make cache");

        let load_scratch = base.join("load");
        fs::create_dir(&load_scratch).expect("create load scratch");
        let manifest =
            load_archive(&destination_root, &archive, &load_scratch).expect("load cache");

        assert_eq!(manifest.repository_id, "repo-001");
        assert!(manifest.includes_scip);
        assert_eq!(
            manifest.semantic_partitions,
            vec![SemanticPartition {
                provider: "openai".to_owned(),
                model: "text-embedding-3-small".to_owned(),
            }]
        );
        assert_eq!(manifest.source_commit.len(), 40);
        assert_eq!(
            fs::read(
                destination_root
                    .join(PROVENANCE_STORAGE_DIR)
                    .join("scip/rust.scip")
            )
            .expect("read loaded SCIP"),
            b"portable-scip"
        );
        let connection = Connection::open(
            destination_root
                .join(PROVENANCE_STORAGE_DIR)
                .join(CACHE_DB_PATH),
        )
        .expect("open loaded database");
        assert_eq!(
            sole_repository_id(&connection).expect("inspect loaded repository"),
            stable_repository_id_for_root(&destination_root).0
        );
        drop(connection);
        let destination_repository_id = stable_repository_id_for_root(&destination_root).0;
        let loaded_semantic = Storage::new(
            destination_root
                .join(PROVENANCE_STORAGE_DIR)
                .join(CACHE_DB_PATH),
        )
        .load_semantic_embeddings_for_repository_model_chunk_ids(
            &destination_repository_id,
            "openai",
            "text-embedding-3-small",
            &["chunk-fixture".to_owned()],
        )
        .expect("load moved semantic state");
        assert_eq!(loaded_semantic.len(), 1);
        assert_eq!(
            loaded_semantic["chunk-fixture"].repository_id,
            destination_repository_id
        );

        let second_destination_root = base.join("destination-two");
        fs::create_dir(&second_destination_root).expect("create second destination root");
        let second_load_scratch = base.join("load-two");
        fs::create_dir(&second_load_scratch).expect("create second load scratch");
        load_archive(&second_destination_root, &archive, &second_load_scratch)
            .expect("load same cache into second checkout");
        let second_repository_id = Storage::new(
            second_destination_root
                .join(PROVENANCE_STORAGE_DIR)
                .join(CACHE_DB_PATH),
        )
        .sole_repository_id()
        .expect("inspect second repository identity")
        .expect("second repository identity");
        assert_ne!(destination_repository_id, second_repository_id);

        let destination_db = destination_root
            .join(PROVENANCE_STORAGE_DIR)
            .join(CACHE_DB_PATH);
        let active_connection = Connection::open(&destination_db).expect("open active destination");
        active_connection
            .execute_batch("PRAGMA journal_mode = WAL;")
            .expect("enable destination WAL");
        let wal_scratch = base.join("wal-load");
        fs::create_dir(&wal_scratch).expect("create WAL load scratch");
        load_archive(&destination_root, &archive, &wal_scratch)
            .expect("load through SQLite backup while another connection is active");
        assert_eq!(
            active_connection
                .query_row("SELECT COUNT(*) FROM repository", [], |row| row
                    .get::<_, i64>(0))
                .expect("query active connection after cache load"),
            1
        );
        drop(active_connection);

        let existing_digest = blake3::hash(&fs::read(&destination_db).expect("read loaded DB"));
        let corrupt_archive = base.join("corrupt.zip");
        corrupt_storage_payload(&archive, &corrupt_archive);
        let corrupt_scratch = base.join("corrupt-load");
        fs::create_dir(&corrupt_scratch).expect("create corrupt-load scratch");
        let error = load_archive(&destination_root, &corrupt_archive, &corrupt_scratch)
            .expect_err("corrupt payload must be rejected");
        assert!(error.to_string().contains("size"));
        assert_eq!(
            blake3::hash(&fs::read(&destination_db).expect("read preserved DB")),
            existing_digest,
            "a rejected cache must not replace existing storage"
        );

        let malformed_archive = base.join("malformed.zip");
        rewrite_archive_with_missing_table(&archive, &malformed_archive, &base);
        let malformed_scratch = base.join("malformed-load");
        fs::create_dir(&malformed_scratch).expect("create malformed-load scratch");
        let error = load_archive(&destination_root, &malformed_archive, &malformed_scratch)
            .expect_err("malformed database with valid checksums must be rejected");
        assert!(error.to_string().contains("missing required table"));
        assert_eq!(
            blake3::hash(&fs::read(&destination_db).expect("read preserved DB")),
            existing_digest,
            "an invalid database must not replace existing storage"
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn cache_manifest_rejects_wrong_version_and_unsafe_paths() {
        let mut manifest = CacheManifest {
            format_version: CACHE_FORMAT_VERSION,
            frigg_version: "0.0.0-wrong".to_owned(),
            compatibility_fingerprint: stable_cache_fingerprint_hex(),
            storage_schema_version: latest_storage_schema_version(),
            source_commit: "a".repeat(40),
            repository_id: "repo-001".to_owned(),
            semantic_partitions: Vec::new(),
            includes_scip: false,
            files: vec![CacheFile {
                path: CACHE_DB_PATH.to_owned(),
                blake3: "a".repeat(64),
                size_bytes: 1,
            }],
        };
        assert!(validate_manifest(&manifest).is_err());

        manifest.frigg_version = env!("CARGO_PKG_VERSION").to_owned();
        assert!(validate_manifest(&manifest).is_ok());
        assert!(validate_archive_path("../storage.sqlite3").is_err());
        assert!(validate_archive_path(r"scip\..\..\victim").is_err());
        assert!(validate_archive_path("context.jsonl").is_err());
    }

    #[test]
    fn cache_install_rolls_back_database_and_scip_together() {
        let base = std::env::temp_dir().join(format!("frigg-cache-rollback-{}", Uuid::now_v7()));
        let root = base.join("workspace");
        let scratch = base.join("scratch");
        fs::create_dir_all(root.join(PROVENANCE_STORAGE_DIR).join("scip"))
            .expect("create destination state");
        fs::create_dir_all(&scratch).expect("create scratch");
        let destination_db = root.join(PROVENANCE_STORAGE_DIR).join(CACHE_DB_PATH);
        let storage = Storage::new(&destination_db);
        storage
            .initialize()
            .expect("initialize destination storage");
        storage
            .upsert_repository("old-repository", &root, "old")
            .expect("seed destination repository");
        fs::write(
            root.join(PROVENANCE_STORAGE_DIR).join("scip/old.scip"),
            b"old-scip",
        )
        .expect("seed destination SCIP");

        let loaded_db = scratch.join("loaded.sqlite3");
        let loaded_storage = Storage::new(&loaded_db);
        loaded_storage
            .initialize()
            .expect("initialize loaded storage");
        loaded_storage
            .upsert_repository("new-repository", &root, "new")
            .expect("seed loaded repository");
        let loaded_scip = scratch.join("loaded-scip");
        fs::create_dir(&loaded_scip).expect("create loaded SCIP");
        fs::write(loaded_scip.join("new.scip"), b"new-scip").expect("seed loaded SCIP");

        let error = install_payloads_with(
            &root,
            &loaded_db,
            Some(&loaded_scip),
            &scratch,
            |_source, _destination| Err(io::Error::other("injected database install failure")),
        )
        .expect_err("injected install failure must propagate");
        assert!(
            error
                .to_string()
                .contains("injected database install failure")
        );
        assert_eq!(
            Storage::new(&destination_db)
                .sole_repository_id()
                .expect("inspect rolled-back database")
                .as_deref(),
            Some("old-repository")
        );
        assert_eq!(
            fs::read(root.join(PROVENANCE_STORAGE_DIR).join("scip/old.scip"))
                .expect("read rolled-back SCIP"),
            b"old-scip"
        );
        assert!(
            !root
                .join(PROVENANCE_STORAGE_DIR)
                .join("scip/new.scip")
                .exists()
        );

        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn cache_install_rejects_symlinked_frigg_directory() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("frigg-cache-symlink-{}", Uuid::now_v7()));
        let root = base.join("workspace");
        let outside = base.join("outside");
        let scratch = base.join("scratch");
        fs::create_dir_all(&root).expect("create workspace");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::create_dir_all(&scratch).expect("create scratch");
        fs::write(outside.join("sentinel"), b"outside").expect("write outside sentinel");
        symlink(&outside, root.join(PROVENANCE_STORAGE_DIR)).expect("symlink .frigg outside");
        let loaded_db = scratch.join("loaded.sqlite3");
        fs::write(&loaded_db, b"not reached").expect("write staged fixture");

        let error = install_payloads(&root, &loaded_db, None, &scratch)
            .expect_err("symlinked storage boundary must be rejected");
        assert!(
            error
                .to_string()
                .contains("escapes canonical workspace root boundary")
        );
        assert_eq!(
            fs::read(outside.join("sentinel")).expect("read outside sentinel"),
            b"outside"
        );

        let _ = fs::remove_dir_all(base);
    }

    fn seed_git_repository(root: &Path) {
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "Frigg Test"],
            vec!["config", "user.email", "frigg@example.invalid"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(root)
                    .status()
                    .expect("run git")
                    .success()
            );
        }
        fs::write(root.join("fixture.rs"), "pub struct Fixture;\n").expect("write Git fixture");
        assert!(
            Command::new("git")
                .args(["add", "fixture.rs"])
                .current_dir(root)
                .status()
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "--quiet", "-m", "fixture"])
                .current_dir(root)
                .status()
                .expect("git commit")
                .success()
        );
    }

    fn corrupt_storage_payload(source: &Path, destination: &Path) {
        let mut source = ZipArchive::new(File::open(source).expect("open source archive"))
            .expect("read source archive");
        let mut destination =
            ZipWriter::new(File::create(destination).expect("create bad archive"));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for index in 0..source.len() {
            let mut entry = source.by_index(index).expect("read archive entry");
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read entry bytes");
            if name == CACHE_DB_PATH {
                bytes.push(0);
            }
            destination.start_file(name, options).expect("start entry");
            destination.write_all(&bytes).expect("write entry");
        }
        destination.finish().expect("finish corrupt archive");
    }

    fn rewrite_archive_with_missing_table(source: &Path, destination: &Path, scratch: &Path) {
        let mut source = ZipArchive::new(File::open(source).expect("open source archive"))
            .expect("read source archive");
        let mut entries = BTreeMap::new();
        for index in 0..source.len() {
            let mut entry = source.by_index(index).expect("read source entry");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read source bytes");
            entries.insert(entry.name().to_owned(), bytes);
        }
        let malformed_db = scratch.join("malformed.sqlite3");
        fs::write(
            &malformed_db,
            entries.get(CACHE_DB_PATH).expect("source DB payload"),
        )
        .expect("write malformed DB fixture");
        Connection::open(&malformed_db)
            .expect("open malformed DB fixture")
            .execute_batch("DROP TABLE file_manifest;")
            .expect("drop required table");
        let malformed_bytes = fs::read(&malformed_db).expect("read malformed DB fixture");
        let mut manifest: CacheManifest = serde_json::from_slice(
            entries
                .get(CACHE_MANIFEST_PATH)
                .expect("source cache manifest"),
        )
        .expect("decode source manifest");
        let db_file = manifest
            .files
            .iter_mut()
            .find(|file| file.path == CACHE_DB_PATH)
            .expect("manifest DB payload");
        db_file.size_bytes = malformed_bytes.len() as u64;
        db_file.blake3 = blake3::hash(&malformed_bytes).to_hex().to_string();
        entries.insert(
            CACHE_MANIFEST_PATH.to_owned(),
            serde_json::to_vec_pretty(&manifest).expect("encode modified manifest"),
        );
        entries.insert(CACHE_DB_PATH.to_owned(), malformed_bytes);

        let mut destination =
            ZipWriter::new(File::create(destination).expect("create malformed archive"));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in entries {
            destination.start_file(name, options).expect("start entry");
            destination.write_all(&bytes).expect("write entry");
        }
        destination.finish().expect("finish malformed archive");
    }
}
