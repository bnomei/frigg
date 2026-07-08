//! Multi-language gate (`surface=lang`) over real non-Rust fixture trees.
//!
//! Trees live under `tests/fixtures/futura_lang/{php,ts,python}/` — multi-file
//! application-shaped fixtures (not stray single files inside the Rust monorepo).

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use frigg::mcp::types::{ResponseMode, SearchPatternType, SearchSymbolParams, SearchTextParams};
use rmcp::handler::server::wrapper::Parameters;

use crate::harness::{
    self, Surface, cleanup_workspace_root, fixtures_root, materialize_fixture_workspace, require,
    server_for_root,
};

pub async fn run_all(report: &Mutex<harness::BenchReport>) {
    run_php_board(report).await;
    run_ts_board(report).await;
    run_python_board(report).await;
}

async fn run_php_board(report: &Mutex<harness::BenchReport>) {
    let fixture = fixtures_root().join("futura_lang/php");
    assert!(
        fixture.join("src/OrderService.php").is_file(),
        "missing PHP lang fixture"
    );

    let started = Instant::now();
    let root = materialize_fixture_workspace(&fixture, "lang-php");
    let outcome = async {
        let server = server_for_root(&root).await;

        let text = server
            .search_text(Parameters(SearchTextParams {
                query: "FUTURA_LANG_PHP_RESTOCK_MARKER".to_owned(),
                pattern_type: Some(SearchPatternType::Literal),
                repository_id: None,
                path_regex: Some("^src/".to_owned()),
                limit: Some(10),
                response_mode: Some(ResponseMode::Compact),
                ..Default::default()
            }))
            .await
            .map_err(|e| format!("PHP search_text failed: {e}"))?
            .0;
        require(
            text.total_matches > 0,
            format!("PHP text marker miss: {text:?}"),
        )?;
        require(
            text.matches.iter().any(|m| m.path.ends_with(".php")),
            format!("PHP text hit should be .php: {:?}", text.matches),
        )?;

        let symbols = server
            .search_symbol(Parameters(SearchSymbolParams {
                query: "OrderService".to_owned(),
                repository_id: None,
                path_class: None,
                path_regex: Some("^src/".to_owned()),
                limit: Some(20),
                response_mode: Some(ResponseMode::Compact),
            }))
            .await
            .map_err(|e| format!("PHP search_symbol failed: {e}"))?
            .0;
        require(
            !symbols.matches.is_empty(),
            format!("PHP OrderService symbol miss: {symbols:?}"),
        )?;
        Ok(())
    }
    .await;
    cleanup_workspace_root(&root);
    report
        .lock()
        .unwrap()
        .record("lang_php_text_and_symbol", Surface::Lang, started, outcome);
}

async fn run_ts_board(report: &Mutex<harness::BenchReport>) {
    let fixture = fixtures_root().join("futura_lang/ts");
    let started = Instant::now();
    let root = materialize_fixture_workspace(&fixture, "lang-ts");
    let outcome = lang_text_and_symbol(
        &root,
        "FUTURA_LANG_TS_USER_SERVICE_MARKER",
        "UserService",
        ".ts",
        "TS",
    )
    .await;
    cleanup_workspace_root(&root);
    report
        .lock()
        .unwrap()
        .record("lang_ts_text_and_symbol", Surface::Lang, started, outcome);
}

async fn run_python_board(report: &Mutex<harness::BenchReport>) {
    let fixture = fixtures_root().join("futura_lang/python");
    let started = Instant::now();
    let root = materialize_fixture_workspace(&fixture, "lang-python");
    let outcome = lang_text_and_symbol(
        &root,
        "FUTURA_LANG_PY_INVENTORY_MARKER",
        "InventoryService",
        ".py",
        "Python",
    )
    .await;
    cleanup_workspace_root(&root);
    report.lock().unwrap().record(
        "lang_python_text_and_symbol",
        Surface::Lang,
        started,
        outcome,
    );
}

async fn lang_text_and_symbol(
    root: &Path,
    text_marker: &str,
    symbol: &str,
    ext: &str,
    label: &str,
) -> Result<(), String> {
    let server = server_for_root(root).await;

    let text = server
        .search_text(Parameters(SearchTextParams {
            query: text_marker.to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(10),
            response_mode: Some(ResponseMode::Compact),
            ..Default::default()
        }))
        .await
        .map_err(|e| format!("{label} search_text failed: {e}"))?
        .0;
    require(
        text.total_matches > 0,
        format!("{label} text marker miss: {text:?}"),
    )?;
    require(
        text.matches.iter().any(|m| m.path.ends_with(ext)),
        format!("{label} hit extension mismatch ({ext}): {:?}", text.matches),
    )?;

    let symbols = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: symbol.to_owned(),
            repository_id: None,
            path_class: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(20),
            response_mode: Some(ResponseMode::Compact),
        }))
        .await
        .map_err(|e| format!("{label} search_symbol failed: {e}"))?
        .0;
    require(
        !symbols.matches.is_empty(),
        format!("{label} symbol `{symbol}` miss: {symbols:?}"),
    )?;
    Ok(())
}
