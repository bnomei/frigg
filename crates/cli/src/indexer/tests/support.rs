pub(super) use std::collections::BTreeMap;
pub(super) use std::env;
pub(super) use std::future::Future;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::pin::Pin;
pub(super) use std::sync::{Arc, Mutex};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};
pub(super) use std::{fs, iter};

#[cfg(unix)]
pub(super) use std::os::unix::fs::PermissionsExt;

pub(super) use super::super::{
    BladeRelationKind, FileDigest, Hasher, HeuristicReferenceConfidence,
    HeuristicReferenceEvidence, ManifestBuilder, ManifestDiagnosticKind, ManifestStore,
    PhpDeclarationRelation, PhpTargetEvidenceKind, PhpTypeEvidenceKind, ReindexMode,
    RuntimeSemanticEmbeddingExecutor, SEMANTIC_CHUNK_MAX_CHARS, SemanticRefreshMode,
    SemanticRuntimeEmbeddingExecutor, SourceSpan, SymbolDefinition, SymbolKind, SymbolLanguage,
    build_file_semantic_chunks, build_reindex_plan_for_tests, build_semantic_chunk_candidates,
    diff, extract_blade_source_evidence_from_source, extract_php_declaration_relations_from_source,
    extract_php_source_evidence_from_source, extract_symbols_for_paths,
    extract_symbols_from_source, file_digest_order,
    generated_follow_up_structural_at_location_in_source,
    inspect_syntax_tree_with_follow_up_in_source, mark_local_flux_overlays,
    navigation_symbol_target_rank, register_symbol_definitions, reindex_repository,
    reindex_repository_with_runtime_config, reindex_repository_with_semantic_executor,
    resolve_heuristic_references, search_structural_in_source, semantic_chunk_language_for_path,
};
pub(super) use crate::domain::{FriggError, FriggResult};
pub(super) use crate::graph::{RelationKind, SymbolGraph};
pub(super) use crate::settings::{
    SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider,
};
pub(super) use crate::storage::{DEFAULT_RETAINED_MANIFEST_SNAPSHOTS, Storage};

#[derive(Debug, Default)]
pub(super) struct FixtureSemanticEmbeddingExecutor;

impl SemanticRuntimeEmbeddingExecutor for FixtureSemanticEmbeddingExecutor {
    fn embed_documents<'a>(
        &'a self,
        _provider: SemanticRuntimeProvider,
        _model: &'a str,
        input: Vec<String>,
        _trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(input
                .into_iter()
                .enumerate()
                .map(|(index, text)| deterministic_fixture_embedding(&text, index))
                .collect::<Vec<_>>())
        })
    }
}

#[derive(Debug, Default, Clone)]
pub(super) struct CountingSemanticEmbeddingExecutor {
    inputs: Arc<Mutex<Vec<String>>>,
}

impl CountingSemanticEmbeddingExecutor {
    pub(super) fn observed_inputs(&self) -> Vec<String> {
        self.inputs
            .lock()
            .expect("counting semantic executor mutex poisoned")
            .clone()
    }
}

impl SemanticRuntimeEmbeddingExecutor for CountingSemanticEmbeddingExecutor {
    fn embed_documents<'a>(
        &'a self,
        _provider: SemanticRuntimeProvider,
        _model: &'a str,
        input: Vec<String>,
        _trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
        let inputs = self.inputs.clone();
        Box::pin(async move {
            inputs
                .lock()
                .expect("counting semantic executor mutex poisoned")
                .extend(input.iter().cloned());
            Ok(input
                .into_iter()
                .enumerate()
                .map(|(index, text)| deterministic_fixture_embedding(&text, index))
                .collect::<Vec<_>>())
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct FailingSemanticEmbeddingExecutor;

impl SemanticRuntimeEmbeddingExecutor for FailingSemanticEmbeddingExecutor {
    fn embed_documents<'a>(
        &'a self,
        _provider: SemanticRuntimeProvider,
        _model: &'a str,
        _input: Vec<String>,
        _trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
        Box::pin(async move {
            Err(FriggError::Internal(
                "synthetic semantic provider failure request_context{model=text-embedding-3-small, inputs=1, input_chars_total=23, max_input_chars=23, body_bytes=96, body_blake3=test-hash, trace_id=trace-test}".to_owned(),
            ))
        })
    }
}

pub(super) fn deterministic_fixture_embedding(text: &str, index: usize) -> Vec<f32> {
    let mut hasher = Hasher::new();
    hasher.update(index.to_string().as_bytes());
    hasher.update(&[0]);
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut embedding = digest
        .as_bytes()
        .chunks_exact(4)
        .take(8)
        .map(|chunk| {
            let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            (value as f32) / (u32::MAX as f32)
        })
        .collect::<Vec<_>>();
    embedding.resize(crate::storage::DEFAULT_VECTOR_DIMENSIONS, 0.0);
    embedding
}

pub(super) fn prepare_manifest_fixture_workspace(test_name: &str) -> FriggResult<PathBuf> {
    let root = temp_workspace_root(test_name);
    prepare_workspace(
        &root,
        &[
            (
                "README.md",
                "# Manifest Determinism Fixture\n\nThis fixture is used by `indexer` determinism tests.\n",
            ),
            (
                "src/lib.rs",
                "pub fn greeting() -> &'static str {\n    \"hello from fixture\"\n}\n",
            ),
            ("src/nested/data.txt", "alpha\nbeta\ngamma\n"),
            ("src/ignored.tmp", "temporary artifact\n"),
            (
                "logs/build.log",
                "this log file should be ignored by .gitignore\n",
            ),
        ],
    )?;
    fs::write(root.join(".gitignore"), "*.tmp\n*.log\n.DS_Store\n").map_err(FriggError::Io)?;
    fs::create_dir_all(root.join(".git")).map_err(FriggError::Io)?;
    Ok(root)
}

pub(super) fn manifest_relative_paths(
    entries: &[FileDigest],
    root: &Path,
) -> FriggResult<Vec<PathBuf>> {
    entries
        .iter()
        .map(|entry| {
            entry
                .path
                .strip_prefix(root)
                .map(|path| path.to_path_buf())
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to relativize fixture path {} against {}: {err}",
                        entry.path.display(),
                        root.display()
                    ))
                })
        })
        .collect()
}

pub(super) fn semantic_runtime_enabled_openai() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    }
}

pub(super) fn digest(path: &str, size_bytes: u64, mtime_ns: Option<u64>, hash: &str) -> FileDigest {
    FileDigest {
        path: PathBuf::from(path),
        size_bytes,
        mtime_ns,
        hash_blake3_hex: hash.to_owned(),
    }
}

pub(super) fn mutate_manifest_for_incremental_roundtrip(
    manifest: &[FileDigest],
    fixture_root: &Path,
) -> FriggResult<Vec<FileDigest>> {
    let mut next = manifest.to_vec();
    let modified_path = fixture_root.join("README.md");
    let deleted_path = fixture_root.join("src/nested/data.txt");

    let modified_entry = next
        .iter_mut()
        .find(|entry| entry.path == modified_path)
        .ok_or_else(|| {
            FriggError::Internal(format!(
                "fixture manifest missing expected file for modification: {}",
                modified_path.display()
            ))
        })?;
    modified_entry.size_bytes += 1;
    modified_entry.mtime_ns = Some(modified_entry.mtime_ns.unwrap_or(0) + 1);
    modified_entry.hash_blake3_hex = "roundtrip-modified-hash".to_string();

    let previous_len = next.len();
    next.retain(|entry| entry.path != deleted_path);
    if next.len() == previous_len {
        return Err(FriggError::Internal(format!(
            "fixture manifest missing expected file for deletion: {}",
            deleted_path.display()
        )));
    }

    next.extend(iter::once(FileDigest {
        path: fixture_root.join("src/incremental-new.rs"),
        size_bytes: 17,
        mtime_ns: Some(17_000),
        hash_blake3_hex: "roundtrip-added-hash".to_string(),
    }));
    next.sort_by(file_digest_order);

    Ok(next)
}

pub(super) fn temp_db_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!(
        "frigg-indexer-{test_name}-{nonce}-{}.sqlite3",
        std::process::id()
    ))
}

pub(super) fn cleanup_db(path: &Path) {
    let _ = fs::remove_file(path);
}

pub(super) fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!(
        "frigg-indexer-{test_name}-{nonce}-{}",
        std::process::id()
    ))
}

pub(super) fn prepare_workspace(root: &Path, files: &[(&str, &str)]) -> FriggResult<()> {
    fs::create_dir_all(root).map_err(FriggError::Io)?;
    for (relative_path, contents) in files {
        let file_path = root.join(relative_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(FriggError::Io)?;
        }
        fs::write(file_path, contents).map_err(FriggError::Io)?;
    }

    Ok(())
}

#[cfg(unix)]
pub(super) fn set_file_mode(path: &Path, mode: u32) -> FriggResult<()> {
    let mut permissions = fs::metadata(path).map_err(FriggError::Io)?.permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions).map_err(FriggError::Io)
}

pub(super) fn cleanup_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

pub(super) fn find_symbol<'a>(
    symbols: &'a [SymbolDefinition],
    kind: SymbolKind,
    name: &str,
    line: usize,
) -> Option<&'a SymbolDefinition> {
    symbols
        .iter()
        .find(|symbol| symbol.kind == kind && symbol.name == name && symbol.line == line)
}

pub(super) fn rust_symbols_fixture() -> &'static str {
    "pub mod api {}\n\
         pub struct User;\n\
         pub enum Role { Admin }\n\
         pub trait Repo { fn save(&self); }\n\
         impl Repo for User { fn save(&self) {} }\n\
         pub const LIMIT: usize = 32;\n\
         pub static NAME: &str = \"frigg\";\n\
         pub type UserId = u64;\n\
         pub fn helper() {}\n"
}

pub(super) fn php_symbols_fixture() -> &'static str {
    "<?php\n\
         namespace App\\Models;\n\
         function top_level(): void {}\n\
         class User {\n\
             public string $name;\n\
             public function save(): void {}\n\
             const LIMIT = 10;\n\
         }\n\
         interface Repo { public function find(): ?User; }\n\
         trait Logs { public function logMessage(): void {} }\n\
         enum Status: string {\n\
             case Active = 'active';\n\
         }\n"
}

pub(super) fn blade_symbols_fixture() -> &'static str {
    "@section('hero')\n\
         @props(['title' => 'Dashboard'])\n\
         @aware(['tone'])\n\
         <x-slot:icon />\n\
         <x-alert.banner />\n\
         <livewire:orders.table />\n\
         @livewire('stats-card')\n\
         <flux:button variant=\"primary\">Save</flux:button>\n"
}

pub(super) fn typescript_symbols_fixture() -> &'static str {
    "namespace Api {}\n\
         export class User {\n\
             readonly id: string;\n\
             save(): void {}\n\
         }\n\
         export interface Repository {\n\
             find(id: string): User;\n\
             status: string;\n\
         }\n\
         export enum Role { Admin }\n\
         export type UserId = string;\n\
         export const renderUser = (user: User) => user.id;\n\
         const LIMIT = 10;\n"
}

pub(super) fn typescript_tsx_fixture() -> &'static str {
    "export const App = () => <Button />;\n"
}

pub(super) fn python_symbols_fixture() -> &'static str {
    concat!(
        "type Alias = str\n",
        "class Service:\n",
        "    def run(self) -> None:\n",
        "        pass\n",
        "\n",
        "def helper() -> Alias:\n",
        "    return \"ok\"\n",
    )
}

pub(super) fn go_symbols_fixture() -> &'static str {
    concat!(
        "package main\n",
        "type Service struct{}\n",
        "type Runner interface{ Run() }\n",
        "type ID = string\n",
        "const Limit = 10\n",
        "func helper() string { return \"ok\" }\n",
        "func (s *Service) Run() string { return \"ok\" }\n",
    )
}

pub(super) fn kotlin_symbols_fixture() -> &'static str {
    concat!(
        "enum class Role { Admin }\n",
        "class Service {\n",
        "    val name: String = \"ok\"\n",
        "    fun run(): String = name\n",
        "}\n",
        "typealias Alias = String\n",
        "fun helper(): Alias = \"ok\"\n",
    )
}

pub(super) fn java_symbols_fixture() -> &'static str {
    concat!(
        "package com.example.app;\n",
        "public enum Role { Admin }\n",
        "public class Service {\n",
        "    public static final int LIMIT = 10;\n",
        "    private String name = \"ok\";\n",
        "    public Service() {}\n",
        "    public String run() { return name; }\n",
        "}\n",
        "public interface Runner {\n",
        "    String find();\n",
        "}\n",
        "public @interface Marker {\n",
        "    String value();\n",
        "}\n",
        "public record Alias(String name) {}\n",
    )
}

pub(super) fn lua_symbols_fixture() -> &'static str {
    concat!(
        "function Service.run()\n",
        "    return \"ok\"\n",
        "end\n",
        "function Service:save()\n",
        "    return true\n",
        "end\n",
    )
}

pub(super) fn nim_symbols_fixture() -> &'static str {
    concat!(
        "type Service = object\n",
        "type Mode = enum\n",
        "  Ready\n",
        "proc helper(): string =\n",
        "  \"ok\"\n",
        "method run(self: Service): string =\n",
        "  \"ok\"\n",
    )
}

pub(super) fn roc_symbols_fixture() -> &'static str {
    concat!(
        "UserId := U64\n",
        "id : U64\n",
        "id = 1\n",
        "greet = \\name -> name\n",
    )
}

pub(super) fn php_source_evidence_fixture() -> &'static str {
    "<?php\n\
         namespace App\\Listeners;\n\
         use App\\Attributes\\AsListener;\n\
         use App\\Contracts\\Dispatcher;\n\
         use App\\Exceptions\\OrderException;\n\
         use App\\Handlers\\OrderHandler as Handler;\n\
         #[AsListener]\n\
         class OrderListener {\n\
             public Dispatcher $dispatcher;\n\
             public function __construct(public Handler $handler) {}\n\
             public function boot(Handler $handler, Dispatcher $dispatcher): Handler {\n\
                 $meta = ['queue' => 'high'];\n\
                 $dispatcher->map(handler: Handler::class);\n\
                 $callable = [Handler::class, 'handle'];\n\
                 $fresh = new Handler();\n\
                 try {} catch (OrderException $e) {}\n\
                 return $handler;\n\
             }\n\
         }\n"
}

pub(super) fn blade_source_evidence_fixture() -> &'static str {
    "@extends('layouts.app')\n\
         @includeIf('partials.flash')\n\
         @yield('hero')\n\
         <x-alert.banner />\n\
         <x-dynamic-component :component=\"'panels.metric'\" />\n\
         <livewire:orders.table />\n\
         @livewire('stats-card')\n\
         <flux:button wire:click=\"save\" wire:model.live=\"state\" />\n"
}
