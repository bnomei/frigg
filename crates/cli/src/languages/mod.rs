mod blade;
mod go;
mod kotlin;
mod lua;
mod nim;
mod php;
mod python;
mod registry;
mod roc;
mod rust;
mod typescript;

#[allow(unused_imports)]
pub(crate) use blade::{
    BladeRelationEvidence, BladeRelationKind, BladeSourceEvidence, FLUX_REGISTRY_VERSION,
    FluxComponentHint, collect_symbols_from_source as collect_blade_symbols_from_source,
    extract_source_evidence_from_source as extract_blade_source_evidence_from_source,
    mark_local_flux_overlays,
    resolve_relation_evidence_edges as resolve_blade_relation_evidence_edges,
};
#[allow(unused_imports)]
pub(crate) use php::{
    PhpDeclarationRelation, PhpGraphSourceAnalysis, PhpLiteralEvidence, PhpSourceEvidence,
    PhpSymbolLookup, PhpTargetEvidence, PhpTargetEvidenceKind, PhpTypeEvidence,
    PhpTypeEvidenceKind,
    declaration_relation_edges_for_file as php_declaration_relation_edges_for_file,
    declaration_relation_edges_for_relations as php_declaration_relation_edges_for_relations,
    declaration_relation_edges_for_source as php_declaration_relation_edges_for_source,
    extract_declaration_relations_from_source as extract_php_declaration_relations_from_source,
    extract_graph_analysis_from_source as extract_php_graph_analysis_from_source,
    extract_source_evidence_from_source as extract_php_source_evidence_from_source,
    heuristic_implementation_candidates_for_target as php_heuristic_implementation_candidates_for_target,
    php_relation_targets_symbol_name, resolve_php_declaration_relation_indices,
    resolve_target_evidence_edges as resolve_php_target_evidence_edges,
    symbol_indices_by_lower_name as php_symbol_indices_by_lower_name,
    symbol_indices_by_name as php_symbol_indices_by_name,
};
pub(crate) use registry::{
    HeuristicImplementationStrategy, LanguageCapability, LanguageSupportCapability, SymbolLanguage,
    heuristic_implementation_strategy, parse_supported_language, parser_for_path,
    semantic_chunk_language_for_path, supported_language_for_path, symbol_from_node,
    tree_sitter_language_for_path,
};
#[allow(unused_imports)]
pub(crate) use rust::{
    RustEnclosingSymbolContext, RustNavigationQueryHint,
    enclosing_symbol_context as rust_enclosing_symbol_context,
    heuristic_implementation_candidates as heuristic_rust_implementation_candidates,
    navigation_query_hint_from_source as rust_navigation_query_hint_from_source,
    parse_impl_signature as parse_rust_impl_signature,
    relative_path_module_segments as rust_relative_path_module_segments,
    source_suffix_looks_like_call as rust_source_suffix_looks_like_call,
};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::blade::{blade_component_name_for_path, blade_view_name_for_path};
    use super::php::php_class_like_name_candidates;
    use super::php::{PhpNameResolutionContext, php_name_resolution_context_from_root};
    use super::registry::LanguageCapabilityTier;
    use super::{
        HeuristicImplementationStrategy, LanguageCapability, LanguageSupportCapability,
        SymbolLanguage, heuristic_implementation_strategy, parse_supported_language,
        semantic_chunk_language_for_path, supported_language_for_path,
    };
    use tree_sitter::Parser;

    #[test]
    fn capability_parsing_uses_shared_alias_table() {
        assert_eq!(
            parse_supported_language("rs", LanguageCapability::DocumentSymbols),
            Some(SymbolLanguage::Rust)
        );
        assert_eq!(
            parse_supported_language("php", LanguageCapability::StructuralSearch),
            Some(SymbolLanguage::Php)
        );
        assert_eq!(
            parse_supported_language("blade", LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Blade)
        );
        assert_eq!(
            parse_supported_language("ts", LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::TypeScript)
        );
        assert_eq!(
            parse_supported_language("tsx", LanguageCapability::StructuralSearch),
            Some(SymbolLanguage::TypeScript)
        );
        assert_eq!(
            parse_supported_language("py", LanguageCapability::DocumentSymbols),
            Some(SymbolLanguage::Python)
        );
        assert_eq!(
            parse_supported_language("go", LanguageCapability::StructuralSearch),
            Some(SymbolLanguage::Go)
        );
        assert_eq!(
            parse_supported_language("kt", LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Kotlin)
        );
        assert_eq!(
            parse_supported_language("lua", LanguageCapability::DocumentSymbols),
            Some(SymbolLanguage::Lua)
        );
        assert_eq!(
            parse_supported_language("roc", LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Roc)
        );
        assert_eq!(
            parse_supported_language("nim", LanguageCapability::DocumentSymbols),
            Some(SymbolLanguage::Nim)
        );
        assert_eq!(
            parse_supported_language("golang", LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Go)
        );
        assert_eq!(
            parse_supported_language("java", LanguageCapability::SymbolCorpus),
            None
        );
    }

    #[test]
    fn path_support_filters_use_capability_tables() {
        assert_eq!(
            supported_language_for_path(Path::new("src/lib.rs"), LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Rust)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("src/server.php"),
                LanguageCapability::DocumentSymbols
            ),
            Some(SymbolLanguage::Php)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("resources/views/welcome.blade.php"),
                LanguageCapability::StructuralSearch
            ),
            Some(SymbolLanguage::Blade)
        );
        assert_eq!(
            supported_language_for_path(Path::new("src/app.ts"), LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::TypeScript)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("src/app.tsx"),
                LanguageCapability::StructuralSearch
            ),
            Some(SymbolLanguage::TypeScript)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("server/main.py"),
                LanguageCapability::DocumentSymbols
            ),
            Some(SymbolLanguage::Python)
        );
        assert_eq!(
            supported_language_for_path(Path::new("cmd/main.go"), LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Go)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("app/main.kts"),
                LanguageCapability::StructuralSearch
            ),
            Some(SymbolLanguage::Kotlin)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("scripts/init.lua"),
                LanguageCapability::DocumentSymbols
            ),
            Some(SymbolLanguage::Lua)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("src/Main.roc"),
                LanguageCapability::StructuralSearch
            ),
            Some(SymbolLanguage::Roc)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("tools/config.nims"),
                LanguageCapability::DocumentSymbols
            ),
            Some(SymbolLanguage::Nim)
        );
    }

    #[test]
    fn capability_tiers_distinguish_core_and_optional_accelerators() {
        assert_eq!(
            SymbolLanguage::Rust.capability_tier(LanguageSupportCapability::DocumentSymbols),
            LanguageCapabilityTier::Core
        );
        assert_eq!(
            SymbolLanguage::Rust.capability_tier(LanguageSupportCapability::PreciseArtifactAssist),
            LanguageCapabilityTier::OptionalAccelerator
        );
        assert_eq!(
            SymbolLanguage::Blade.capability_tier(LanguageSupportCapability::SemanticChunking),
            LanguageCapabilityTier::OptionalAccelerator
        );
        assert_eq!(
            SymbolLanguage::TypeScript.capability_tier(LanguageSupportCapability::SemanticChunking),
            LanguageCapabilityTier::Unsupported
        );
    }

    #[test]
    fn semantic_chunk_language_labels_follow_the_registry() {
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("src/lib.rs")),
            Some("rust")
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("resources/views/welcome.blade.php")),
            Some("blade")
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("src/app.tsx")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("app/main.py")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("cmd/main.go")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("app/main.kts")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("scripts/init.lua")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("src/Main.roc")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("tools/config.nims")),
            None
        );
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("docs/overview.md")),
            Some("markdown")
        );
    }

    #[test]
    fn heuristic_implementation_dispatch_stays_centralized() {
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Rust),
            Some(HeuristicImplementationStrategy::RustImplBlocks)
        );
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Php),
            Some(HeuristicImplementationStrategy::PhpDeclarationRelations)
        );
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Blade),
            None
        );
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::TypeScript),
            None
        );
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Python),
            None
        );
        assert_eq!(heuristic_implementation_strategy(SymbolLanguage::Go), None);
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Kotlin),
            None
        );
        assert_eq!(heuristic_implementation_strategy(SymbolLanguage::Lua), None);
        assert_eq!(heuristic_implementation_strategy(SymbolLanguage::Roc), None);
        assert_eq!(heuristic_implementation_strategy(SymbolLanguage::Nim), None);
    }

    #[test]
    fn blade_path_helpers_normalize_view_and_component_names() {
        assert_eq!(
            blade_view_name_for_path(Path::new("resources/views/dashboard/index.blade.php")),
            Some("dashboard.index".to_owned())
        );
        assert_eq!(
            blade_component_name_for_path(Path::new(
                "resources/views/components/forms/input.blade.php"
            )),
            Some("forms.input".to_owned())
        );
    }

    #[test]
    fn php_name_resolution_context_resolves_aliases_grouped_imports_and_namespace_relative_names() {
        let source = "<?php\n\
            namespace App\\Http\\Controllers;\n\
            use App\\Contracts\\Handler as ContractHandler;\n\
            use App\\Support\\{Mailer, Logger as ActivityLogger};\n";
        let mut parser = Parser::new();
        let language = tree_sitter_php::LANGUAGE_PHP.into();
        parser
            .set_language(&language)
            .expect("php parser should configure");
        let tree = parser.parse(source, None).expect("php source should parse");
        let context = php_name_resolution_context_from_root(source, tree.root_node());

        assert_eq!(
            context,
            PhpNameResolutionContext {
                namespace: Some("App\\Http\\Controllers".to_owned()),
                class_like_aliases: [
                    (
                        "contracthandler".to_owned(),
                        "App\\Contracts\\Handler".to_owned(),
                    ),
                    ("mailer".to_owned(), "App\\Support\\Mailer".to_owned()),
                    (
                        "activitylogger".to_owned(),
                        "App\\Support\\Logger".to_owned(),
                    ),
                ]
                .into_iter()
                .collect(),
            }
        );
        assert_eq!(
            context.resolve_class_like_name("ContractHandler", None),
            Some("App\\Contracts\\Handler".to_owned())
        );
        assert_eq!(
            context.resolve_class_like_name("Mailer", None),
            Some("App\\Support\\Mailer".to_owned())
        );
        assert_eq!(
            context.resolve_class_like_name("namespace\\Responder", None),
            Some("App\\Http\\Controllers\\Responder".to_owned())
        );
        assert_eq!(
            php_class_like_name_candidates(Some(&context), "ActivityLogger", None),
            vec![
                "App\\Support\\Logger".to_owned(),
                "ActivityLogger".to_owned()
            ]
        );
    }
}
