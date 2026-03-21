#![allow(clippy::panic)]

use super::support::*;
use crate::indexer::{StructuralQueryAnchorSelection, search_structural_grouped_in_source};

#[test]
fn structural_search_rust_returns_deterministic_captures() -> FriggResult<()> {
    let source = "pub fn first() {}\n\
             pub fn second() {}\n";
    let path = Path::new("fixtures/structural.rs");
    let query = "(function_item) @function";

    let first = search_structural_in_source(SymbolLanguage::Rust, path, source, query)?;
    let second = search_structural_in_source(SymbolLanguage::Rust, path, source, query)?;

    assert_eq!(first, second, "structural captures should be deterministic");
    assert_eq!(first.len(), 2);
    assert_eq!(
        first
            .iter()
            .map(|matched| {
                (
                    matched.path.clone(),
                    matched.span.start_line,
                    matched.span.start_column,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (PathBuf::from("fixtures/structural.rs"), 1, 1),
            (PathBuf::from("fixtures/structural.rs"), 2, 1),
        ]
    );
    Ok(())
}

#[test]
fn structural_search_typescript_tsx_uses_extension_aware_grammar() -> FriggResult<()> {
    let matches = search_structural_in_source(
        SymbolLanguage::TypeScript,
        Path::new("fixtures/component.tsx"),
        typescript_tsx_fixture(),
        "(jsx_self_closing_element) @jsx",
    )?;

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, PathBuf::from("fixtures/component.tsx"));
    assert_eq!(matches[0].span.start_line, 1);
    assert_eq!(matches[0].excerpt, "<Button />");

    Ok(())
}

#[test]
fn structural_search_python_returns_deterministic_captures() -> FriggResult<()> {
    let first = search_structural_in_source(
        SymbolLanguage::Python,
        Path::new("fixtures/python_symbols.py"),
        python_symbols_fixture(),
        "(function_definition) @fn",
    )?;
    let second = search_structural_in_source(
        SymbolLanguage::Python,
        Path::new("fixtures/python_symbols.py"),
        python_symbols_fixture(),
        "(function_definition) @fn",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].span.start_line, 3);
    assert_eq!(first[1].span.start_line, 6);

    Ok(())
}

#[test]
fn structural_search_go_returns_deterministic_captures() -> FriggResult<()> {
    let first = search_structural_in_source(
        SymbolLanguage::Go,
        Path::new("fixtures/go_symbols.go"),
        go_symbols_fixture(),
        "(function_declaration) @fn",
    )?;
    let second = search_structural_in_source(
        SymbolLanguage::Go,
        Path::new("fixtures/go_symbols.go"),
        go_symbols_fixture(),
        "(function_declaration) @fn",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].span.start_line, 6);

    Ok(())
}

#[test]
fn structural_search_kotlin_returns_deterministic_captures() -> FriggResult<()> {
    let first = search_structural_in_source(
        SymbolLanguage::Kotlin,
        Path::new("fixtures/kotlin_symbols.kt"),
        kotlin_symbols_fixture(),
        "(function_declaration) @fn",
    )?;
    let second = search_structural_in_source(
        SymbolLanguage::Kotlin,
        Path::new("fixtures/kotlin_symbols.kt"),
        kotlin_symbols_fixture(),
        "(function_declaration) @fn",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].span.start_line, 4);
    assert_eq!(first[1].span.start_line, 7);

    Ok(())
}

#[test]
fn structural_search_lua_returns_deterministic_captures() -> FriggResult<()> {
    let first = search_structural_in_source(
        SymbolLanguage::Lua,
        Path::new("fixtures/lua_symbols.lua"),
        lua_symbols_fixture(),
        "(function_declaration) @fn",
    )?;
    let second = search_structural_in_source(
        SymbolLanguage::Lua,
        Path::new("fixtures/lua_symbols.lua"),
        lua_symbols_fixture(),
        "(function_declaration) @fn",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].span.start_line, 1);
    assert_eq!(first[1].span.start_line, 4);

    Ok(())
}

#[test]
fn structural_search_nim_returns_deterministic_captures() -> FriggResult<()> {
    let first = search_structural_in_source(
        SymbolLanguage::Nim,
        Path::new("fixtures/nim_symbols.nim"),
        nim_symbols_fixture(),
        "(proc_declaration) @proc",
    )?;
    let second = search_structural_in_source(
        SymbolLanguage::Nim,
        Path::new("fixtures/nim_symbols.nim"),
        nim_symbols_fixture(),
        "(proc_declaration) @proc",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].span.start_line, 4);

    Ok(())
}

#[test]
fn structural_search_roc_returns_deterministic_captures() -> FriggResult<()> {
    let first = search_structural_in_source(
        SymbolLanguage::Roc,
        Path::new("fixtures/roc_symbols.roc"),
        roc_symbols_fixture(),
        "(value_declaration) @value",
    )?;
    let second = search_structural_in_source(
        SymbolLanguage::Roc,
        Path::new("fixtures/roc_symbols.roc"),
        roc_symbols_fixture(),
        "(value_declaration) @value",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].span.start_line, 2);
    assert_eq!(first[1].span.start_line, 4);

    Ok(())
}

#[test]
fn structural_search_rejects_invalid_query_with_typed_error() {
    let error = search_structural_in_source(
        SymbolLanguage::Rust,
        Path::new("fixtures/structural.rs"),
        "pub fn first() {}\n",
        "(function_item @broken",
    )
    .expect_err("invalid query must return error");

    match error {
        FriggError::InvalidInput(message) => {
            assert!(message.contains("invalid structural query"));
        }
        other => panic!("expected invalid input, got {other:?}"),
    }
}

#[test]
fn structural_search_grouped_mode_groups_multi_capture_matches() -> FriggResult<()> {
    let source = "pub fn first() {}\n\
             pub fn second() {}\n";
    let path = Path::new("fixtures/structural.rs");
    let query = "(function_item name: (identifier) @name) @match";

    let grouped =
        search_structural_grouped_in_source(SymbolLanguage::Rust, path, source, query, None)?;

    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped[0].anchor_capture_name.as_deref(), Some("match"));
    assert_eq!(
        grouped[0].anchor_selection,
        StructuralQueryAnchorSelection::MatchCapture
    );
    assert_eq!(grouped[0].captures.len(), 2);
    assert_eq!(grouped[0].captures[0].name, "match");
    assert_eq!(grouped[0].captures[1].name, "name");
    assert_eq!(grouped[0].excerpt, "pub fn first() {}");

    Ok(())
}

#[test]
fn structural_search_grouped_mode_honors_primary_capture() -> FriggResult<()> {
    let source = "pub fn first() {}\n";
    let path = Path::new("fixtures/structural.rs");
    let query = "(function_item name: (identifier) @name) @match";

    let grouped = search_structural_grouped_in_source(
        SymbolLanguage::Rust,
        path,
        source,
        query,
        Some("name"),
    )?;

    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].anchor_capture_name.as_deref(), Some("name"));
    assert_eq!(
        grouped[0].anchor_selection,
        StructuralQueryAnchorSelection::PrimaryCapture
    );
    assert_eq!(grouped[0].excerpt, "first");

    Ok(())
}

#[test]
fn generated_follow_up_structural_prefers_useful_ancestors_deterministically() -> FriggResult<()> {
    let source = "pub fn greet() {\n    helper();\n}\n\nfn helper() {}\n";
    let path = Path::new("fixtures/inspect.rs");
    let (inspection, first) = inspect_syntax_tree_with_follow_up_in_source(
        SymbolLanguage::Rust,
        path,
        "src/lib.rs",
        source,
        Some(2),
        Some(6),
        8,
        4,
        "repo-001",
    )?;

    let second = generated_follow_up_structural_at_location_in_source(
        SymbolLanguage::Rust,
        path,
        "src/lib.rs",
        source,
        2,
        6,
        "repo-001",
    )?;

    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    assert_eq!(first[0].params.query, "(call_expression) @match");
    assert_eq!(first[1].params.query, "(call_expression) @match");
    assert_eq!(first[2].params.query, "(function_item) @match");
    assert_eq!(
        first[0].params.path_regex.as_deref(),
        Some("^src/lib\\.rs$")
    );
    assert_eq!(inspection.focus.kind, "call_expression");
    assert_eq!(
        inspection.raw_focus.as_ref().map(|node| node.kind.as_str()),
        Some("identifier")
    );
    assert_eq!(first[0].basis.raw_focus_kind.as_deref(), Some("identifier"));

    Ok(())
}

#[test]
fn generated_follow_up_structural_omits_wrapper_only_candidates() -> FriggResult<()> {
    let source = "pub fn greet() {}\n";
    let path = Path::new("fixtures/wrapper_only.rs");
    let (_inspection, follow_ups) = inspect_syntax_tree_with_follow_up_in_source(
        SymbolLanguage::Rust,
        path,
        "src/lib.rs",
        source,
        None,
        None,
        8,
        4,
        "repo-001",
    )?;

    assert!(follow_ups.is_empty());
    Ok(())
}

#[test]
fn heuristic_references_combines_graph_hints_and_lexical_fallback_deterministically()
-> FriggResult<()> {
    let source = "pub struct User;\n\
             pub fn create_user() -> User { User }\n\
             pub fn use_user() { let _ = User; }\n\
             pub fn marker() { let _ = User; }\n\
             pub fn unrelated() { let _ = \"SuperUser\"; }\n";
    let path = PathBuf::from("fixtures/heuristic.rs");
    let symbols = extract_symbols_from_source(SymbolLanguage::Rust, &path, source)?;

    let target =
        find_symbol(&symbols, SymbolKind::Struct, "User", 1).expect("expected target symbol");
    let create_user = find_symbol(&symbols, SymbolKind::Function, "create_user", 2)
        .expect("expected create_user symbol");
    let use_user = find_symbol(&symbols, SymbolKind::Function, "use_user", 3)
        .expect("expected use_user symbol");

    let mut graph = SymbolGraph::default();
    register_symbol_definitions(&mut graph, "repo-001", &symbols);
    assert!(
        graph
            .add_relation(
                &create_user.stable_id,
                &target.stable_id,
                RelationKind::RefersTo
            )
            .expect("refers_to relation should be added")
    );
    assert!(
        graph
            .add_relation(&use_user.stable_id, &target.stable_id, RelationKind::Calls)
            .expect("calls relation should be added")
    );

    let mut sources = BTreeMap::new();
    sources.insert(path.clone(), source.to_owned());

    let first =
        resolve_heuristic_references("repo-001", &target.stable_id, &symbols, &graph, &sources);
    let second =
        resolve_heuristic_references("repo-001", &target.stable_id, &symbols, &graph, &sources);

    assert_eq!(
        first, second,
        "heuristic references should be deterministic"
    );
    assert_eq!(
        first
            .iter()
            .map(|reference| (reference.line, reference.confidence))
            .collect::<Vec<_>>(),
        vec![
            (2, HeuristicReferenceConfidence::High),
            (2, HeuristicReferenceConfidence::High),
            (2, HeuristicReferenceConfidence::High),
            (3, HeuristicReferenceConfidence::High),
            (3, HeuristicReferenceConfidence::High),
            (4, HeuristicReferenceConfidence::Low),
        ]
    );
    let line_two_columns = first
        .iter()
        .filter(|reference| reference.line == 2)
        .map(|reference| reference.column)
        .collect::<Vec<_>>();
    assert_eq!(line_two_columns.len(), 3);
    assert!(
        line_two_columns
            .windows(2)
            .all(|window| window[0] <= window[1]),
        "line-2 references should be ordered by column deterministically"
    );
    assert!(
        matches!(
            first[0].evidence,
            HeuristicReferenceEvidence::GraphRelation { .. }
        ),
        "highest-confidence hint should come from graph relation evidence"
    );
    assert!(
        !first.iter().any(|reference| reference.line == 5),
        "substring-only lexical tokens should not be returned"
    );

    Ok(())
}

#[test]
fn heuristic_references_false_positive_bound_for_substring_tokens() -> FriggResult<()> {
    let source = "<?php\n\
             class User {}\n\
             function true_ref(): void { $x = new User(); }\n\
             function noise(): void {\n\
                 $a = 'SuperUser';\n\
                 $b = 'UserService';\n\
             }\n";
    let path = PathBuf::from("fixtures/heuristic.php");
    let symbols = extract_symbols_from_source(SymbolLanguage::Php, &path, source)?;

    let target =
        find_symbol(&symbols, SymbolKind::Class, "User", 2).expect("expected class symbol");
    let true_ref = find_symbol(&symbols, SymbolKind::Function, "true_ref", 3)
        .expect("expected true_ref symbol");

    let mut graph = SymbolGraph::default();
    register_symbol_definitions(&mut graph, "repo-001", &symbols);
    assert!(
        graph
            .add_relation(
                &true_ref.stable_id,
                &target.stable_id,
                RelationKind::RefersTo
            )
            .expect("refers_to relation should be added")
    );

    let mut sources = BTreeMap::new();
    sources.insert(path, source.to_owned());
    let references =
        resolve_heuristic_references("repo-001", &target.stable_id, &symbols, &graph, &sources);

    assert_eq!(
        references
            .iter()
            .map(|reference| (reference.line, reference.confidence))
            .collect::<Vec<_>>(),
        vec![
            (3, HeuristicReferenceConfidence::High),
            (3, HeuristicReferenceConfidence::High),
        ],
        "same-line heuristic references should preserve both graph and lexical hits"
    );
    let same_line_columns = references
        .iter()
        .map(|reference| reference.column)
        .collect::<Vec<_>>();
    assert_eq!(same_line_columns.len(), 2);
    assert!(
        same_line_columns
            .windows(2)
            .all(|window| window[0] <= window[1]),
        "same-line references should be ordered by column"
    );
    let low_confidence = references
        .iter()
        .filter(|reference| reference.confidence == HeuristicReferenceConfidence::Low)
        .count();
    assert_eq!(
        low_confidence, 0,
        "false-positive lower bound violated: expected no low-confidence noise hits"
    );

    Ok(())
}

#[test]
fn heuristic_references_preserve_multiple_same_line_lexical_hits() -> FriggResult<()> {
    let source = "pub struct User;\n\
             pub fn use_user() { let _a = User; let _b = User; }\n";
    let path = PathBuf::from("fixtures/heuristic-same-line.rs");
    let symbols = extract_symbols_from_source(SymbolLanguage::Rust, &path, source)?;

    let target =
        find_symbol(&symbols, SymbolKind::Struct, "User", 1).expect("expected target symbol");
    let sources = BTreeMap::from([(path, source.to_owned())]);
    let references = resolve_heuristic_references(
        "repo-001",
        &target.stable_id,
        &symbols,
        &SymbolGraph::default(),
        &sources,
    );

    assert_eq!(
        references.len(),
        2,
        "same-line lexical references should retain both token hits"
    );
    assert_eq!(
        references
            .iter()
            .map(|reference| (reference.line, reference.column, reference.confidence))
            .collect::<Vec<_>>(),
        vec![
            (2, 30, HeuristicReferenceConfidence::Low),
            (2, 45, HeuristicReferenceConfidence::Low),
        ]
    );
    assert!(
        references.iter().all(|reference| matches!(
            reference.evidence,
            HeuristicReferenceEvidence::LexicalToken
        )),
        "same-line lexical hits should retain lexical evidence"
    );

    Ok(())
}
