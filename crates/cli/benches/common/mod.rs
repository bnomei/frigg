#![allow(dead_code, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::indexer::ManifestBuilder;
use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{WorkspaceAttachParams, WorkspaceResolveMode};
use frigg::settings::{FriggConfig, LexicalBackendMode, SemanticRuntimeCredentials};
use frigg::test_support::config_for;
use protobuf::{EnumOrUnknown, Message};
use rmcp::handler::server::wrapper::Parameters;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    SymbolInformation as ScipSymbolInformationProto,
};
use tokio::runtime::{Builder, Runtime};

pub(crate) struct BenchServerSession {
    pub runtime: Runtime,
    pub server: FriggMcpServer,
    pub repository_id: String,
}

pub(crate) fn fixture_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "frigg-bench-fixture-{nonce}-{}",
            std::process::id()
        ));
        write_fixture_workspace(&root);
        root
    })
}

pub(crate) fn manifest_source_paths(root: &Path) -> Vec<PathBuf> {
    let manifest = ManifestBuilder::default()
        .build_metadata_with_diagnostics(root)
        .expect("benchmark manifest should build");
    let mut paths = manifest
        .entries
        .into_iter()
        .map(|entry| entry.path)
        .filter(|path| is_symbol_source_path(path))
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

pub(crate) fn native_search_config(root: &Path) -> FriggConfig {
    let mut config = config_for(root);
    config.lexical_runtime.backend = LexicalBackendMode::Native;
    config.lexical_runtime.ripgrep_executable = None;
    config
}

pub(crate) fn auto_search_config(root: &Path) -> FriggConfig {
    config_for(root)
}

pub(crate) fn ripgrep_search_config(root: &Path, executable: PathBuf) -> FriggConfig {
    let mut config = config_for(root);
    config.lexical_runtime.backend = LexicalBackendMode::Ripgrep;
    config.lexical_runtime.ripgrep_executable = Some(executable);
    config
}

pub(crate) fn semantic_runtime_credentials() -> SemanticRuntimeCredentials {
    SemanticRuntimeCredentials::default()
}

pub(crate) fn fresh_fixture_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "frigg-bench-fixture-{label}-{nonce}-{}",
        std::process::id()
    ));
    write_fixture_workspace(&root);
    root
}

pub(crate) fn attached_server_session(config: FriggConfig, root: &Path) -> BenchServerSession {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime should initialize");
    let server = FriggMcpServer::new(config);
    let attached = runtime
        .block_on(server.workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
        })))
        .expect("benchmark workspace attach should succeed")
        .0;
    BenchServerSession {
        runtime,
        server,
        repository_id: attached.repository.repository_id,
    }
}

pub(crate) fn attached_fixture_server_session() -> BenchServerSession {
    attached_server_session(native_search_config(fixture_root()), fixture_root())
}

pub(crate) fn rg_executable() -> Option<PathBuf> {
    let candidates = ["rg", "/opt/homebrew/bin/rg", "/usr/local/bin/rg"];
    candidates.into_iter().find_map(|candidate| {
        let output = Command::new(candidate).arg("--version").output().ok()?;
        output.status.success().then(|| PathBuf::from(candidate))
    })
}

pub(crate) fn rewrite_file_with_new_mtime(path: &Path, contents: &str) {
    let before = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(path, contents).expect("benchmark file rewrite should persist");
        let after = fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        if after != before {
            return;
        }
    }
    panic!("benchmark file mtime did not advance after rewrite");
}

pub(crate) fn benchmark_db_path(root: &Path) -> PathBuf {
    let state_dir = root.join(".frigg");
    fs::create_dir_all(&state_dir).expect("benchmark state dir should exist");
    state_dir.join("storage.sqlite3")
}

pub(crate) fn write_scip_protobuf_fixture(workspace_root: &Path, file_name: &str) {
    let fixture_dir = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&fixture_dir).expect("benchmark scip dir should exist");

    let mut index = ScipIndexProto::new();
    let mut document = ScipDocumentProto::new();
    document.relative_path = "src/module_000.rs".to_owned();

    let mut definition = ScipOccurrenceProto::new();
    definition.symbol = "scip-rust pkg fixture#Widget0".to_owned();
    definition.range = vec![0, 11, 18];
    definition.symbol_roles = 1;
    document.occurrences.push(definition);

    let mut reference = ScipOccurrenceProto::new();
    reference.symbol = "scip-rust pkg fixture#Widget0".to_owned();
    reference.range = vec![6, 29, 36];
    reference.symbol_roles = 8;
    document.occurrences.push(reference);

    let mut symbol = ScipSymbolInformationProto::new();
    symbol.symbol = "scip-rust pkg fixture#Widget0".to_owned();
    symbol.display_name = "Widget0".to_owned();
    symbol.kind = EnumOrUnknown::from_i32(7);
    document.symbols.push(symbol);

    index.documents.push(document);
    let payload = index
        .write_to_bytes()
        .expect("benchmark scip payload should serialize");
    fs::write(fixture_dir.join(file_name), payload).expect("benchmark scip payload should persist");
}

fn is_symbol_source_path(path: &Path) -> bool {
    let path_text = path.to_string_lossy();
    if path_text.ends_with(".blade.php") {
        return true;
    }
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(
            "rs" | "php"
                | "ts"
                | "tsx"
                | "py"
                | "go"
                | "kt"
                | "kts"
                | "lua"
                | "roc"
                | "nim"
                | "nims"
        )
    )
}

fn write_fixture_workspace(root: &Path) {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root.join(".git")).expect("benchmark fixture git dir should exist");
    fs::write(root.join(".gitignore"), "*.tmp\n").expect("benchmark fixture gitignore");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("benchmark fixture cargo manifest");
    fs::write(
        root.join("package.json"),
        "{\n  \"name\": \"fixture\",\n  \"private\": true,\n  \"version\": \"0.1.0\"\n}\n",
    )
    .expect("benchmark fixture package manifest");
    fs::write(
        root.join("tsconfig.json"),
        "{\n  \"compilerOptions\": { \"jsx\": \"react-jsx\" }\n}\n",
    )
    .expect("benchmark fixture tsconfig");
    fs::write(
        root.join("composer.json"),
        "{\n  \"name\": \"fixture/app\"\n}\n",
    )
    .expect("benchmark fixture composer manifest");

    for dir in [
        root.join("src"),
        root.join("app/Models"),
        root.join("resources/views/components"),
        root.join("tests"),
        root.join("web"),
    ] {
        fs::create_dir_all(&dir).expect("benchmark fixture directory should exist");
    }

    for index in 0..96 {
        let rust_path = root.join(format!("src/module_{index:03}.rs"));
        let rust_source = format!(
            "pub struct Widget{index};\n\
             impl Widget{index} {{\n\
                 pub fn handle_checkout_request(&self, user_id: usize) -> usize {{ user_id + {index} }}\n\
                 pub fn render_summary(&self) -> &'static str {{ \"checkout-widget\" }}\n\
             }}\n\
             pub fn build_service_{index}(input: usize) -> usize {{\n\
                 let widget = Widget{index};\n\
                 widget.handle_checkout_request(input)\n\
             }}\n"
        );
        fs::write(rust_path, rust_source).expect("benchmark rust file should be writable");
    }

    for index in 0..32 {
        let ts_path = root.join(format!("web/component_{index:03}.tsx"));
        let ts_source = format!(
            "export function CheckoutComponent{index}(props: {{ orderId: string }}) {{\n\
             const label = `checkout-{index}`;\n\
             return <button data-order={{props.orderId}}>{{label}}</button>;\n\
             }}\n"
        );
        fs::write(ts_path, ts_source).expect("benchmark tsx file should be writable");
    }

    for index in 0..24 {
        let php_path = root.join(format!("app/Models/Order{index}.php"));
        let php_source = format!(
            "<?php\n\
             namespace App\\\\Models;\n\
             final class Order{index} {{\n\
                 public function handleCheckout(string $orderId): string {{ return $orderId . '-{index}'; }}\n\
             }}\n"
        );
        fs::write(php_path, php_source).expect("benchmark php file should be writable");
    }

    for index in 0..16 {
        let blade_path = root.join(format!("resources/views/components/card_{index}.blade.php"));
        let blade_source =
            format!("<div>\n  <x-button label=\"checkout-{index}\" />\n  {{ $slot }}\n</div>\n");
        fs::write(blade_path, blade_source).expect("benchmark blade file should be writable");
    }

    for index in 0..24 {
        let test_path = root.join(format!("tests/module_{index:03}_test.rs"));
        let test_source = format!(
            "#[test]\nfn checkout_flow_{index}() {{ assert_eq!({}, {}); }}\n",
            index, index
        );
        fs::write(test_path, test_source).expect("benchmark test file should be writable");
    }
}
