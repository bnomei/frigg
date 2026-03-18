use crate::languages::resolve_php_target_evidence_edges;

use super::support::*;

#[test]
fn symbols_rust_php_extracts_rust_definition_metadata() -> FriggResult<()> {
    let symbols = extract_symbols_from_source(
        SymbolLanguage::Rust,
        Path::new("fixtures/rust_symbols.rs"),
        rust_symbols_fixture(),
    )?;

    assert!(
        find_symbol(&symbols, SymbolKind::Module, "api", 1).is_some(),
        "expected rust module symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Struct, "User", 2).is_some(),
        "expected rust struct symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Enum, "Role", 3).is_some(),
        "expected rust enum symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Trait, "Repo", 4).is_some(),
        "expected rust trait symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Impl, "impl Repo for User", 5).is_some(),
        "expected rust impl symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Const, "LIMIT", 6).is_some(),
        "expected rust const symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Static, "NAME", 7).is_some(),
        "expected rust static symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::TypeAlias, "UserId", 8).is_some(),
        "expected rust type alias symbol"
    );
    let helper = find_symbol(&symbols, SymbolKind::Function, "helper", 9)
        .expect("expected rust function symbol");
    assert!(
        helper.stable_id.starts_with("sym-"),
        "expected stable symbol id prefix"
    );
    assert_eq!(helper.path, PathBuf::from("fixtures/rust_symbols.rs"));
    assert_eq!(helper.line, helper.span.start_line);

    Ok(())
}

#[test]
fn symbols_rust_php_extracts_php_definition_metadata() -> FriggResult<()> {
    let symbols = extract_symbols_from_source(
        SymbolLanguage::Php,
        Path::new("fixtures/php_symbols.php"),
        php_symbols_fixture(),
    )?;

    assert!(
        find_symbol(&symbols, SymbolKind::Module, "App\\Models", 2).is_some(),
        "expected php namespace module symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "top_level", 3).is_some(),
        "expected php top-level function symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Class, "User", 4).is_some(),
        "expected php class symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Property, "$name", 5).is_some(),
        "expected php property symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "save", 6).is_some(),
        "expected php method symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Constant, "LIMIT", 7).is_some(),
        "expected php constant symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Interface, "Repo", 9).is_some(),
        "expected php interface symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::PhpTrait, "Logs", 10).is_some(),
        "expected php trait symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::PhpEnum, "Status", 11).is_some(),
        "expected php enum symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::EnumCase, "Active", 12).is_some(),
        "expected php enum case symbol"
    );

    Ok(())
}

#[test]
fn symbols_blade_extracts_view_component_and_template_metadata() -> FriggResult<()> {
    let path = Path::new("resources/views/components/dashboard/panel.blade.php");
    let symbols =
        extract_symbols_from_source(SymbolLanguage::Blade, path, blade_symbols_fixture())?;

    assert!(
        find_symbol(
            &symbols,
            SymbolKind::Module,
            "components.dashboard.panel",
            1
        )
        .is_some(),
        "expected blade view module symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Component, "dashboard.panel", 1).is_some(),
        "expected blade anonymous component symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Section, "hero", 1).is_some(),
        "expected blade section symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Property, "$title", 2).is_some(),
        "expected blade props symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Property, "$tone", 3).is_some(),
        "expected blade aware symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Slot, "icon", 4).is_some(),
        "expected blade named slot symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Component, "alert.banner", 5).is_some(),
        "expected x-component symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Component, "livewire:orders.table", 6).is_some(),
        "expected livewire tag symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Component, "livewire:stats-card", 7).is_some(),
        "expected @livewire directive symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Component, "flux:button", 8).is_some(),
        "expected flux tag symbol"
    );

    Ok(())
}

#[test]
fn symbols_typescript_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/typescript_symbols.ts");
    let symbols = extract_symbols_from_source(
        SymbolLanguage::TypeScript,
        path,
        typescript_symbols_fixture(),
    )?;

    assert!(
        find_symbol(&symbols, SymbolKind::Module, "Api", 1).is_some(),
        "expected TypeScript namespace/module symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Class, "User", 2).is_some(),
        "expected TypeScript class symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Property, "id", 3).is_some(),
        "expected TypeScript class field symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "save", 4).is_some(),
        "expected TypeScript method symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Interface, "Repository", 6).is_some(),
        "expected TypeScript interface symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "find", 7).is_some(),
        "expected TypeScript interface method symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Property, "status", 8).is_some(),
        "expected TypeScript interface property symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Enum, "Role", 10).is_some(),
        "expected TypeScript enum symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::TypeAlias, "UserId", 11).is_some(),
        "expected TypeScript type alias symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "renderUser", 12).is_some(),
        "expected TypeScript arrow-function binding symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Const, "LIMIT", 13).is_some(),
        "expected TypeScript const binding symbol"
    );

    let tsx_symbols = extract_symbols_from_source(
        SymbolLanguage::TypeScript,
        Path::new("fixtures/component.tsx"),
        typescript_tsx_fixture(),
    )?;
    assert!(
        find_symbol(&tsx_symbols, SymbolKind::Function, "App", 1).is_some(),
        "expected TSX component binding to be discoverable as a function symbol"
    );

    Ok(())
}

#[test]
fn symbols_python_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/python_symbols.py");
    let symbols =
        extract_symbols_from_source(SymbolLanguage::Python, path, python_symbols_fixture())?;

    assert!(
        find_symbol(&symbols, SymbolKind::TypeAlias, "Alias", 1).is_some(),
        "expected Python type alias symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Class, "Service", 2).is_some(),
        "expected Python class symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "run", 3).is_some(),
        "expected Python method symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "helper", 6).is_some(),
        "expected Python function symbol"
    );

    Ok(())
}

#[test]
fn symbols_go_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/go_symbols.go");
    let symbols = extract_symbols_from_source(SymbolLanguage::Go, path, go_symbols_fixture())?;

    assert!(
        find_symbol(&symbols, SymbolKind::Module, "main", 1).is_some(),
        "expected Go package symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Struct, "Service", 2).is_some(),
        "expected Go struct symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Interface, "Runner", 3).is_some(),
        "expected Go interface symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::TypeAlias, "ID", 4).is_some(),
        "expected Go type alias symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Const, "Limit", 5).is_some(),
        "expected Go const symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "helper", 6).is_some(),
        "expected Go function symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "Run", 7).is_some(),
        "expected Go method symbol"
    );

    Ok(())
}

#[test]
fn symbols_kotlin_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/kotlin_symbols.kt");
    let symbols =
        extract_symbols_from_source(SymbolLanguage::Kotlin, path, kotlin_symbols_fixture())?;

    assert!(
        find_symbol(&symbols, SymbolKind::Enum, "Role", 1).is_some(),
        "expected Kotlin enum symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Class, "Service", 2).is_some(),
        "expected Kotlin class symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Property, "name", 3).is_some(),
        "expected Kotlin property symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "run", 4).is_some(),
        "expected Kotlin method symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::TypeAlias, "Alias", 6).is_some(),
        "expected Kotlin type alias symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "helper", 7).is_some(),
        "expected Kotlin top-level function symbol"
    );

    Ok(())
}

#[test]
fn symbols_lua_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/lua_symbols.lua");
    let symbols = extract_symbols_from_source(SymbolLanguage::Lua, path, lua_symbols_fixture())?;

    assert!(
        find_symbol(&symbols, SymbolKind::Function, "run", 1).is_some(),
        "expected Lua dotted function symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "save", 4).is_some(),
        "expected Lua method symbol"
    );

    Ok(())
}

#[test]
fn symbols_nim_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/nim_symbols.nim");
    let symbols = extract_symbols_from_source(SymbolLanguage::Nim, path, nim_symbols_fixture())?;

    assert!(
        find_symbol(&symbols, SymbolKind::Struct, "Service", 1).is_some(),
        "expected Nim object symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Enum, "Mode", 2).is_some(),
        "expected Nim enum-like type symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "helper", 4).is_some(),
        "expected Nim proc symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Method, "run", 6).is_some(),
        "expected Nim method symbol"
    );

    Ok(())
}

#[test]
fn symbols_roc_extracts_definition_metadata() -> FriggResult<()> {
    let path = Path::new("fixtures/roc_symbols.roc");
    let symbols = extract_symbols_from_source(SymbolLanguage::Roc, path, roc_symbols_fixture())?;

    assert!(
        find_symbol(&symbols, SymbolKind::TypeAlias, "UserId", 1).is_some(),
        "expected Roc nominal type symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Const, "id", 2).is_some(),
        "expected Roc value symbol"
    );
    assert!(
        find_symbol(&symbols, SymbolKind::Function, "greet", 4).is_some(),
        "expected Roc function value symbol"
    );

    Ok(())
}

#[test]
fn php_source_evidence_extracts_canonical_type_target_and_literal_metadata() -> FriggResult<()> {
    let path = Path::new("src/OrderListener.php");
    let source = php_source_evidence_fixture();
    let symbols = extract_symbols_from_source(SymbolLanguage::Php, path, source)?;
    let evidence = extract_php_source_evidence_from_source(path, source, &symbols)?;

    let class_symbol = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "OrderListener")
        .expect("expected class symbol for php evidence fixture");
    let method_symbol = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "boot")
        .expect("expected method symbol for php evidence fixture");

    assert_eq!(
        evidence
            .canonical_names_by_stable_id
            .get(&class_symbol.stable_id),
        Some(&"App\\Listeners\\OrderListener".to_owned())
    );
    assert_eq!(
        evidence
            .canonical_names_by_stable_id
            .get(&method_symbol.stable_id),
        Some(&"App\\Listeners\\OrderListener::boot".to_owned())
    );
    assert!(
        evidence.type_evidence.iter().any(|entry| {
            entry.kind == PhpTypeEvidenceKind::PromotedProperty
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
        }),
        "expected promoted-property type evidence for aliased contract handler"
    );
    assert!(
        evidence.type_evidence.iter().any(|entry| {
            entry.kind == PhpTypeEvidenceKind::Parameter
                && entry.target_canonical_name == "App\\Contracts\\Dispatcher"
        }),
        "expected parameter type evidence for imported dispatcher type"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::Attribute
                && entry.target_canonical_name == "App\\Attributes\\AsListener"
        }),
        "expected attribute target evidence for class and method attributes"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::Instantiation
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
        }),
        "expected instantiation target evidence"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::ClassString
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
        }),
        "expected class-string target evidence"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::CallableLiteral
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
                && entry.target_member_name.as_deref() == Some("handle")
        }),
        "expected callable-literal target evidence"
    );
    assert!(
        evidence.literal_evidence.iter().any(|entry| {
            entry.array_keys == vec!["queue".to_owned()] && entry.named_arguments.is_empty()
        }),
        "expected literal array-key evidence"
    );
    assert!(
        evidence.literal_evidence.iter().any(|entry| {
            entry.array_keys.is_empty() && entry.named_arguments == vec!["handler".to_owned()]
        }),
        "expected named-argument evidence"
    );

    Ok(())
}

#[test]
fn blade_source_evidence_extracts_relations_livewire_wire_and_flux_hints() -> FriggResult<()> {
    let path = Path::new("resources/views/dashboard/show.blade.php");
    let source = blade_source_evidence_fixture();
    let symbols = extract_symbols_from_source(SymbolLanguage::Blade, path, source)?;
    let mut evidence = extract_blade_source_evidence_from_source(source, &symbols);

    let overlay_path = Path::new("resources/views/components/flux/button.blade.php");
    let overlay_symbols = extract_symbols_from_source(SymbolLanguage::Blade, overlay_path, "")?;
    let mut combined_symbols = symbols.clone();
    combined_symbols.extend(overlay_symbols);
    let mut symbol_indices_by_name = BTreeMap::new();
    for (index, symbol) in combined_symbols.iter().enumerate() {
        symbol_indices_by_name
            .entry(symbol.name.clone())
            .or_insert_with(Vec::new)
            .push(index);
    }
    mark_local_flux_overlays(&mut evidence, &combined_symbols, &symbol_indices_by_name);

    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Extends
                && relation.target_name == "layouts.app"
                && relation.target_symbol_kind == SymbolKind::Module
        }),
        "expected @extends relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Include
                && relation.target_name == "partials.flash"
                && relation.target_symbol_kind == SymbolKind::Module
        }),
        "expected @include relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Component
                && relation.target_name == "alert.banner"
                && relation.target_symbol_kind == SymbolKind::Component
        }),
        "expected x-component relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::DynamicComponent
                && relation.target_name == "panels.metric"
                && relation.target_symbol_kind == SymbolKind::Component
        }),
        "expected normalized dynamic-component relation evidence"
    );
    assert_eq!(
        evidence.livewire_components,
        vec!["orders.table".to_owned(), "stats-card".to_owned()]
    );
    assert_eq!(
        evidence.wire_directives,
        vec!["wire:click".to_owned(), "wire:model.live".to_owned()]
    );
    assert_eq!(evidence.flux_components, vec!["flux:button".to_owned()]);
    assert!(
        evidence
            .flux_hints
            .get("flux:button")
            .is_some_and(|hint| hint.local_overlay),
        "expected local overlay discovery to enrich flux component hints"
    );

    Ok(())
}

#[test]
fn php_declaration_relations_extract_extends_and_implements_deterministically() -> FriggResult<()> {
    let source = "<?php\n\
             interface ProviderInterface {}\n\
             interface ExtendedProviderInterface extends ProviderInterface, BaseProviderInterface {}\n\
             class ListCompletionProvider implements ProviderInterface {}\n\
             class EnumCompletionProvider extends ListCompletionProvider implements ProviderInterface {}\n\
             enum UserIdCompletionProvider implements ProviderInterface {}\n";
    let relations = extract_php_declaration_relations_from_source(
        Path::new("fixtures/php_relations.php"),
        source,
    )?;

    assert_eq!(
        relations,
        vec![
            PhpDeclarationRelation {
                source_kind: SymbolKind::Class,
                source_name: "EnumCompletionProvider".to_owned(),
                source_line: 5,
                target_name: "ListCompletionProvider".to_owned(),
                relation: RelationKind::Extends,
            },
            PhpDeclarationRelation {
                source_kind: SymbolKind::Class,
                source_name: "EnumCompletionProvider".to_owned(),
                source_line: 5,
                target_name: "ProviderInterface".to_owned(),
                relation: RelationKind::Implements,
            },
            PhpDeclarationRelation {
                source_kind: SymbolKind::Class,
                source_name: "ListCompletionProvider".to_owned(),
                source_line: 4,
                target_name: "ProviderInterface".to_owned(),
                relation: RelationKind::Implements,
            },
            PhpDeclarationRelation {
                source_kind: SymbolKind::Interface,
                source_name: "ExtendedProviderInterface".to_owned(),
                source_line: 3,
                target_name: "BaseProviderInterface".to_owned(),
                relation: RelationKind::Extends,
            },
            PhpDeclarationRelation {
                source_kind: SymbolKind::Interface,
                source_name: "ExtendedProviderInterface".to_owned(),
                source_line: 3,
                target_name: "ProviderInterface".to_owned(),
                relation: RelationKind::Extends,
            },
            PhpDeclarationRelation {
                source_kind: SymbolKind::PhpEnum,
                source_name: "UserIdCompletionProvider".to_owned(),
                source_line: 6,
                target_name: "ProviderInterface".to_owned(),
                relation: RelationKind::Implements,
            },
        ]
    );

    Ok(())
}

#[test]
fn php_source_evidence_extracts_canonical_names_types_targets_and_literals() -> FriggResult<()> {
    let path = Path::new("src/OrderListener.php");
    let source = php_source_evidence_fixture();
    let symbols = extract_symbols_from_source(SymbolLanguage::Php, path, source)?;
    let evidence = extract_php_source_evidence_from_source(path, source, &symbols)?;

    let canonical_names = evidence
        .canonical_names_by_stable_id
        .values()
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        canonical_names
            .iter()
            .any(|name| name == "App\\Listeners\\OrderListener"),
        "expected canonical class name in php evidence"
    );
    assert!(
        canonical_names
            .iter()
            .any(|name| name == "App\\Listeners\\OrderListener::boot"),
        "expected canonical method name in php evidence"
    );
    assert!(
        canonical_names
            .iter()
            .any(|name| name == "App\\Listeners\\OrderListener::$dispatcher"),
        "expected canonical property name in php evidence"
    );

    let type_targets = evidence
        .type_evidence
        .iter()
        .map(|entry| entry.target_canonical_name.as_str())
        .collect::<Vec<_>>();
    assert!(
        type_targets.contains(&"App\\Contracts\\Dispatcher"),
        "expected dispatcher type evidence"
    );
    assert!(
        type_targets.contains(&"App\\Handlers\\OrderHandler"),
        "expected handler type evidence"
    );
    assert!(
        type_targets.contains(&"App\\Exceptions\\OrderException"),
        "expected catch type evidence"
    );

    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::Attribute
                && entry.target_canonical_name == "App\\Attributes\\AsListener"
        }),
        "expected attribute target evidence"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::ClassString
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
        }),
        "expected class-string target evidence"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::Instantiation
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
        }),
        "expected instantiation target evidence"
    );
    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::CallableLiteral
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
                && entry.target_member_name.as_deref() == Some("handle")
        }),
        "expected callable-literal target evidence"
    );

    assert!(
        evidence
            .literal_evidence
            .iter()
            .any(|entry| { entry.array_keys == vec!["queue".to_owned()] }),
        "expected literal array-key evidence"
    );
    assert!(
        evidence
            .literal_evidence
            .iter()
            .any(|entry| { entry.named_arguments == vec!["handler".to_owned()] }),
        "expected named-argument evidence"
    );

    Ok(())
}

#[test]
fn php_source_evidence_extracts_callable_literal_targets_from_nested_listener_arrays()
-> FriggResult<()> {
    let path = Path::new("src/OrderListener.php");
    let source = "<?php\n\
         namespace App\\Listeners;\n\
         use App\\Handlers\\OrderHandler;\n\
         class OrderListener {\n\
             public function handlers(): array {\n\
                 return [[OrderHandler::class, 'handle']];\n\
             }\n\
         }\n";
    let symbols = extract_symbols_from_source(SymbolLanguage::Php, path, source)?;
    let evidence = extract_php_source_evidence_from_source(path, source, &symbols)?;

    assert!(
        evidence.target_evidence.iter().any(|entry| {
            entry.kind == PhpTargetEvidenceKind::CallableLiteral
                && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
                && entry.target_member_name.as_deref() == Some("handle")
        }),
        "expected callable-literal target evidence for nested listener arrays"
    );

    Ok(())
}

#[test]
fn php_source_evidence_resolves_nested_callable_literal_edges() -> FriggResult<()> {
    let handler_path = Path::new("src/Handlers/OrderHandler.php");
    let handler_source = "<?php\n\
         namespace App\\Handlers;\n\
         class OrderHandler {\n\
             public function handle(): void {}\n\
         }\n";
    let listener_path = Path::new("src/Listeners/OrderListener.php");
    let listener_source = "<?php\n\
         namespace App\\Listeners;\n\
         use App\\Handlers\\OrderHandler;\n\
         class OrderListener {\n\
             public function handlers(): array {\n\
                 return [[OrderHandler::class, 'handle']];\n\
             }\n\
         }\n";

    let mut symbols =
        extract_symbols_from_source(SymbolLanguage::Php, handler_path, handler_source)?;
    symbols.extend(extract_symbols_from_source(
        SymbolLanguage::Php,
        listener_path,
        listener_source,
    )?);
    let handler_evidence =
        extract_php_source_evidence_from_source(handler_path, handler_source, &symbols)?;
    let listener_evidence =
        extract_php_source_evidence_from_source(listener_path, listener_source, &symbols)?;

    let symbol_index_by_stable_id = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| (symbol.stable_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut canonical_names_by_stable_id = handler_evidence.canonical_names_by_stable_id.clone();
    canonical_names_by_stable_id.extend(listener_evidence.canonical_names_by_stable_id.clone());
    let mut symbol_indices_by_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    let mut symbol_indices_by_lower_canonical_name = BTreeMap::<String, Vec<usize>>::new();
    for (stable_id, canonical_name) in &canonical_names_by_stable_id {
        let Some(symbol_index) = symbol_index_by_stable_id.get(stable_id).copied() else {
            continue;
        };
        symbol_indices_by_canonical_name
            .entry(canonical_name.clone())
            .or_default()
            .push(symbol_index);
        symbol_indices_by_lower_canonical_name
            .entry(canonical_name.to_ascii_lowercase())
            .or_default()
            .push(symbol_index);
    }

    let edges = resolve_php_target_evidence_edges(
        &symbols,
        &symbol_index_by_stable_id,
        &symbol_indices_by_canonical_name,
        &symbol_indices_by_lower_canonical_name,
        &listener_evidence,
    );
    let handler_method = symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "handle"
                && symbol.path == handler_path
        })
        .expect("expected handler method symbol");
    let listener_method = symbols
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::Method
                && symbol.name == "handlers"
                && symbol.path == listener_path
        })
        .expect("expected listener method symbol");

    assert!(
        edges
            .iter()
            .any(|(source_symbol_index, target_symbol_index, relation)| {
                *relation == RelationKind::RefersTo
                    && symbols[*source_symbol_index].stable_id == listener_method.stable_id
                    && symbols[*target_symbol_index].stable_id == handler_method.stable_id
            }),
        "expected nested callable-literal evidence to resolve listener->handler method edge"
    );

    Ok(())
}

#[test]
fn blade_source_evidence_extracts_relations_and_ui_metadata() -> FriggResult<()> {
    let path = Path::new("resources/views/dashboard/index.blade.php");
    let source = blade_source_evidence_fixture();
    let symbols = extract_symbols_from_source(SymbolLanguage::Blade, path, source)?;
    let evidence = extract_blade_source_evidence_from_source(source, &symbols);

    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Extends
                && relation.target_name == "layouts.app"
                && relation.target_symbol_kind == SymbolKind::Module
        }),
        "expected @extends relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Include && relation.target_name == "partials.flash"
        }),
        "expected @include relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Yield
                && relation.target_name == "hero"
                && relation.target_symbol_kind == SymbolKind::Section
        }),
        "expected @yield relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::Component
                && relation.target_name == "alert.banner"
                && relation.target_symbol_kind == SymbolKind::Component
        }),
        "expected x-component relation evidence"
    );
    assert!(
        evidence.relations.iter().any(|relation| {
            relation.kind == BladeRelationKind::DynamicComponent
                && relation.target_name == "panels.metric"
                && relation.target_symbol_kind == SymbolKind::Component
        }),
        "expected x-dynamic-component relation evidence"
    );
    assert_eq!(
        evidence.livewire_components,
        vec!["orders.table".to_owned(), "stats-card".to_owned()]
    );
    assert_eq!(
        evidence.wire_directives,
        vec!["wire:click".to_owned(), "wire:model.live".to_owned()]
    );
    assert_eq!(evidence.flux_components, vec!["flux:button".to_owned()]);
    assert!(
        evidence.flux_hints.contains_key("flux:button"),
        "expected offline flux registry hints for flux:button"
    );

    Ok(())
}

#[test]
fn symbols_supported_language_extraction_is_deterministic() -> FriggResult<()> {
    let first = extract_symbols_from_source(
        SymbolLanguage::Rust,
        Path::new("fixtures/rust_symbols.rs"),
        rust_symbols_fixture(),
    )?;
    let second = extract_symbols_from_source(
        SymbolLanguage::Rust,
        Path::new("fixtures/rust_symbols.rs"),
        rust_symbols_fixture(),
    )?;
    let third = extract_symbols_from_source(
        SymbolLanguage::Php,
        Path::new("fixtures/php_symbols.php"),
        php_symbols_fixture(),
    )?;
    let fourth = extract_symbols_from_source(
        SymbolLanguage::Php,
        Path::new("fixtures/php_symbols.php"),
        php_symbols_fixture(),
    )?;
    let fifth = extract_symbols_from_source(
        SymbolLanguage::TypeScript,
        Path::new("fixtures/typescript_symbols.ts"),
        typescript_symbols_fixture(),
    )?;
    let sixth = extract_symbols_from_source(
        SymbolLanguage::TypeScript,
        Path::new("fixtures/typescript_symbols.ts"),
        typescript_symbols_fixture(),
    )?;
    let seventh = extract_symbols_from_source(
        SymbolLanguage::Python,
        Path::new("fixtures/python_symbols.py"),
        python_symbols_fixture(),
    )?;
    let eighth = extract_symbols_from_source(
        SymbolLanguage::Python,
        Path::new("fixtures/python_symbols.py"),
        python_symbols_fixture(),
    )?;
    let ninth = extract_symbols_from_source(
        SymbolLanguage::Go,
        Path::new("fixtures/go_symbols.go"),
        go_symbols_fixture(),
    )?;
    let tenth = extract_symbols_from_source(
        SymbolLanguage::Go,
        Path::new("fixtures/go_symbols.go"),
        go_symbols_fixture(),
    )?;
    let eleventh = extract_symbols_from_source(
        SymbolLanguage::Kotlin,
        Path::new("fixtures/kotlin_symbols.kt"),
        kotlin_symbols_fixture(),
    )?;
    let twelfth = extract_symbols_from_source(
        SymbolLanguage::Kotlin,
        Path::new("fixtures/kotlin_symbols.kt"),
        kotlin_symbols_fixture(),
    )?;
    let thirteenth = extract_symbols_from_source(
        SymbolLanguage::Lua,
        Path::new("fixtures/lua_symbols.lua"),
        lua_symbols_fixture(),
    )?;
    let fourteenth = extract_symbols_from_source(
        SymbolLanguage::Lua,
        Path::new("fixtures/lua_symbols.lua"),
        lua_symbols_fixture(),
    )?;
    let fifteenth = extract_symbols_from_source(
        SymbolLanguage::Nim,
        Path::new("fixtures/nim_symbols.nim"),
        nim_symbols_fixture(),
    )?;
    let sixteenth = extract_symbols_from_source(
        SymbolLanguage::Nim,
        Path::new("fixtures/nim_symbols.nim"),
        nim_symbols_fixture(),
    )?;
    let seventeenth = extract_symbols_from_source(
        SymbolLanguage::Roc,
        Path::new("fixtures/roc_symbols.roc"),
        roc_symbols_fixture(),
    )?;
    let eighteenth = extract_symbols_from_source(
        SymbolLanguage::Roc,
        Path::new("fixtures/roc_symbols.roc"),
        roc_symbols_fixture(),
    )?;

    assert_eq!(first, second);
    assert_eq!(third, fourth);
    assert_eq!(fifth, sixth);
    assert_eq!(seventh, eighth);
    assert_eq!(ninth, tenth);
    assert_eq!(eleventh, twelfth);
    assert_eq!(thirteenth, fourteenth);
    assert_eq!(fifteenth, sixteenth);
    assert_eq!(seventeenth, eighteenth);
    Ok(())
}

#[test]
fn symbols_rust_php_path_batch_reports_diagnostics_and_continues() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("symbols-rust-php-batch");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/lib.rs", rust_symbols_fixture()),
            ("src/known.php", php_symbols_fixture()),
        ],
    )?;
    let missing_path = workspace_root.join("src/missing.php");
    let paths = vec![
        missing_path.clone(),
        workspace_root.join("src/lib.rs"),
        workspace_root.join("src/known.php"),
    ];

    let output = extract_symbols_for_paths(&paths);

    assert!(
        output.symbols.iter().any(|symbol| {
            symbol.path == workspace_root.join("src/lib.rs")
                && symbol.kind == SymbolKind::Function
                && symbol.name == "helper"
        }),
        "expected rust symbols from existing file"
    );
    assert!(
        output.symbols.iter().any(|symbol| {
            symbol.path == workspace_root.join("src/known.php")
                && symbol.kind == SymbolKind::Function
                && symbol.name == "top_level"
        }),
        "expected php symbols from existing file"
    );
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(output.diagnostics[0].path, missing_path);
    assert_eq!(output.diagnostics[0].language, Some(SymbolLanguage::Php));

    cleanup_workspace(&workspace_root);
    Ok(())
}
