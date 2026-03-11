use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use frigg::indexer::{
    FileDigest, ManifestBuilder, SemanticChunkBenchmarkSummary,
    benchmark_build_file_semantic_chunks, benchmark_build_semantic_chunk_candidates,
};

const BENCH_SOURCE_LINES: usize = 320;
const BENCH_MARKDOWN_SECTIONS: usize = 36;
const BENCH_MANIFEST_FILE_COUNT: usize = 96;

static HOT_RUST_SOURCE: OnceLock<String> = OnceLock::new();
static HOT_MARKDOWN_SOURCE: OnceLock<String> = OnceLock::new();
static MANIFEST_FIXTURE: OnceLock<ManifestChunkFixture> = OnceLock::new();

struct ManifestChunkFixture {
    root: PathBuf,
    manifest: Vec<FileDigest>,
}

fn semantic_chunk_hot_path_benchmarks(c: &mut Criterion) {
    let rust_source = HOT_RUST_SOURCE.get_or_init(build_large_rust_source);
    let markdown_source = HOT_MARKDOWN_SOURCE.get_or_init(build_heading_heavy_markdown_source);
    let fixture = MANIFEST_FIXTURE.get_or_init(prepare_manifest_fixture);

    assert_large_rust_chunk_workload(rust_source);
    assert_markdown_heading_workload(markdown_source);
    assert_manifest_chunk_batch_workload(fixture);

    let mut group = c.benchmark_group("semantic_chunk_hot_paths");
    group.sample_size(30);

    group.bench_function(BenchmarkId::new("file", "rust-large-split"), |b| {
        b.iter(|| {
            let summary = benchmark_build_file_semantic_chunks(
                "repo-001",
                "snapshot-001",
                "src/hot_path.rs",
                "rust",
                rust_source,
            );
            criterion::black_box(summary);
        });
    });

    group.bench_function(BenchmarkId::new("file", "markdown-heading-fanout"), |b| {
        b.iter(|| {
            let summary = benchmark_build_file_semantic_chunks(
                "repo-001",
                "snapshot-001",
                "docs/hot_path.md",
                "markdown",
                markdown_source,
            );
            criterion::black_box(summary);
        });
    });

    group.bench_function(BenchmarkId::new("manifest", "mixed-language-batch"), |b| {
        b.iter(|| {
            let summary = benchmark_build_semantic_chunk_candidates(
                "repo-001",
                &fixture.root,
                "snapshot-001",
                &fixture.manifest,
            )
            .expect("manifest semantic chunk benchmark should not fail");
            criterion::black_box(summary);
        });
    });

    group.finish();
}

fn build_large_rust_source() -> String {
    let mut lines = Vec::with_capacity(BENCH_SOURCE_LINES * 3);
    lines.push("// deterministic semantic chunk benchmark fixture".to_owned());
    for line_idx in 0..BENCH_SOURCE_LINES {
        lines.push(format!(
            "pub fn hot_path_{line_idx:03}() -> &'static str {{"
        ));
        lines.push(format!(
            "    \"semantic chunk hotspot line {line_idx:03} {} {} {} {} {} {}\"",
            "allocation", "hash", "segment", "boundary", "semantic", "chunk"
        ));
        lines.push("}".to_owned());
    }
    lines.join("\n")
}

fn build_heading_heavy_markdown_source() -> String {
    let mut lines = Vec::with_capacity(BENCH_MARKDOWN_SECTIONS * 5);
    for section_idx in 0..BENCH_MARKDOWN_SECTIONS {
        lines.push(format!("# Section {section_idx:02}"));
        lines.push(format!(
            "semantic chunk heading boundary section {section_idx:02} replay benchmark budget"
        ));
        lines.push(format!(
            "This section repeats enough prose to force allocation-heavy joins and hashes for section {section_idx:02}."
        ));
        lines.push(format!(
            "Budget metrics and benchmark evidence stay deterministic in section {section_idx:02}."
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn prepare_manifest_fixture() -> ManifestChunkFixture {
    let root =
        std::env::temp_dir().join(format!("frigg-semantic-chunk-bench-{}", std::process::id()));
    if root.exists() {
        let _ = fs::remove_dir_all(&root);
    }

    populate_manifest_fixture(&root);
    let manifest = ManifestBuilder::default()
        .build(&root)
        .expect("semantic chunk manifest benchmark fixture should build");

    ManifestChunkFixture { root, manifest }
}

fn populate_manifest_fixture(root: &Path) {
    for rel_path in ["src", "app", "docs", "config", "notes", "playbooks"] {
        fs::create_dir_all(root.join(rel_path))
            .expect("semantic chunk benchmark fixture directory should be creatable");
    }

    for file_idx in 0..BENCH_MANIFEST_FILE_COUNT {
        let (relative_path, content) = build_manifest_fixture_file(file_idx);
        fs::write(root.join(relative_path), content)
            .expect("semantic chunk benchmark fixture file should be writable");
    }

    for playbook_idx in 0..8 {
        fs::write(
            root.join(format!("playbooks/ignored_{playbook_idx:02}.md")),
            format!(
                "# Ignored {playbook_idx:02}\nsemantic chunk playbook files are skipped during candidate construction\n"
            ),
        )
        .expect("semantic chunk playbook fixture should be writable");
    }
}

fn build_manifest_fixture_file(file_idx: usize) -> (String, String) {
    match file_idx % 6 {
        0 => (
            format!("src/module_{file_idx:03}.rs"),
            build_repeated_source_file("rust", file_idx),
        ),
        1 => (
            format!("app/module_{file_idx:03}.php"),
            build_repeated_source_file("php", file_idx),
        ),
        2 => (
            format!("docs/guide_{file_idx:03}.md"),
            build_repeated_markdown_file(file_idx),
        ),
        3 => (
            format!("config/module_{file_idx:03}.json"),
            build_json_file(file_idx),
        ),
        4 => (
            format!("config/module_{file_idx:03}.toml"),
            build_toml_file(file_idx),
        ),
        _ => (
            format!("notes/module_{file_idx:03}.txt"),
            build_text_file(file_idx),
        ),
    }
}

fn build_repeated_source_file(language: &str, file_idx: usize) -> String {
    let mut lines = Vec::with_capacity(60);
    for line_idx in 0..60 {
        lines.push(format!(
            "// {language} semantic chunk benchmark file={file_idx:03} line={line_idx:03}"
        ));
        lines.push(format!(
            "const VALUE_{file_idx:03}_{line_idx:03}: &str = \"semantic chunk workload {language} {file_idx:03} {line_idx:03} allocation hashing segmentation\";"
        ));
    }
    lines.join("\n")
}

fn build_repeated_markdown_file(file_idx: usize) -> String {
    let mut lines = Vec::with_capacity(40);
    for section_idx in 0..18 {
        lines.push(format!("## Guide {file_idx:03}.{section_idx:02}"));
        lines.push(format!(
            "semantic chunk markdown guide {file_idx:03}.{section_idx:02} benchmark evidence"
        ));
        lines.push(format!(
            "This markdown section keeps headings dense so the chunker flushes frequently for guide {file_idx:03}.{section_idx:02}."
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn build_json_file(file_idx: usize) -> String {
    let entries = (0..24)
        .map(|value_idx| {
            format!(
                "\"key_{value_idx:02}\": \"semantic chunk json workload {file_idx:03} {value_idx:02}\""
            )
        })
        .collect::<Vec<_>>();
    format!("{{\n  {}\n}}", entries.join(",\n  "))
}

fn build_toml_file(file_idx: usize) -> String {
    let mut lines = Vec::with_capacity(30);
    lines.push(format!("title = \"semantic chunk toml {file_idx:03}\""));
    for value_idx in 0..24 {
        lines.push(format!(
            "key_{value_idx:02} = \"semantic chunk toml workload {file_idx:03} {value_idx:02}\""
        ));
    }
    lines.join("\n")
}

fn build_text_file(file_idx: usize) -> String {
    let mut lines = Vec::with_capacity(40);
    for line_idx in 0..40 {
        lines.push(format!(
            "semantic chunk text workload file={file_idx:03} line={line_idx:03} allocation hashing segmentation"
        ));
    }
    lines.join("\n")
}

fn assert_large_rust_chunk_workload(source: &str) {
    let summary = benchmark_build_file_semantic_chunks(
        "repo-001",
        "snapshot-001",
        "src/hot_path.rs",
        "rust",
        source,
    );
    assert_summary(summary, 4, 8_000);
}

fn assert_markdown_heading_workload(source: &str) {
    let summary = benchmark_build_file_semantic_chunks(
        "repo-001",
        "snapshot-001",
        "docs/hot_path.md",
        "markdown",
        source,
    );
    assert_summary(summary, 12, 2_000);
}

fn assert_manifest_chunk_batch_workload(fixture: &ManifestChunkFixture) {
    let summary = benchmark_build_semantic_chunk_candidates(
        "repo-001",
        &fixture.root,
        "snapshot-001",
        &fixture.manifest,
    )
    .expect("semantic chunk batch benchmark fixture should be searchable");
    assert_summary(summary, 48, 24_000);
}

fn assert_summary(
    summary: SemanticChunkBenchmarkSummary,
    min_chunk_count: usize,
    min_total_bytes: usize,
) {
    assert!(
        summary.chunk_count >= min_chunk_count,
        "expected at least {min_chunk_count} semantic chunks, got {summary:?}"
    );
    assert!(
        summary.total_content_bytes >= min_total_bytes,
        "expected at least {min_total_bytes} semantic chunk bytes, got {summary:?}"
    );
    assert!(
        summary.max_chunk_bytes > 0,
        "semantic chunk benchmark summary should report non-empty chunks: {summary:?}"
    );
}

criterion_group!(
    name = benches;
    config = Criterion::default().configure_from_args();
    targets = semantic_chunk_hot_path_benchmarks
);
criterion_main!(benches);
