#![allow(unsafe_code)]

//! Package-local tree-sitter wrappers keep the published crate self-contained without requiring
//! unpublished registry dependencies for vendored grammars.

use tree_sitter_language::LanguageFn;

/// Vendored tree-sitter grammar bindings for Blade templates.
pub(crate) mod tree_sitter_blade {
    use super::LanguageFn;

    unsafe extern "C" {
        fn tree_sitter_blade() -> *const ();
    }

    pub(crate) const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_blade) };
    #[allow(dead_code)]
    pub(crate) const NODE_TYPES: &str =
        include_str!("../vendor-grammars/tree-sitter-blade/src/node-types.json");
    #[allow(dead_code)]
    pub(crate) const HIGHLIGHTS_QUERY: &str =
        include_str!("../vendor-grammars/tree-sitter-blade/queries/highlights.scm");
    #[allow(dead_code)]
    pub(crate) const INJECTIONS_QUERY: &str =
        include_str!("../vendor-grammars/tree-sitter-blade/queries/injections.scm");
}

/// Vendored tree-sitter grammar bindings for Nim sources.
pub(crate) mod tree_sitter_nim {
    use super::LanguageFn;

    unsafe extern "C" {
        fn tree_sitter_nim() -> *const ();
    }

    pub(crate) const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_nim) };
    pub(crate) const NODE_TYPES: &str =
        include_str!("../vendor-grammars/tree-sitter-nim/src/node-types.json");
}

/// Vendored tree-sitter grammar bindings for Kotlin and KTS sources.
pub(crate) mod tree_sitter_kotlin {
    use super::LanguageFn;

    unsafe extern "C" {
        fn tree_sitter_kotlin() -> *const ();
    }

    pub(crate) const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_kotlin) };
    #[allow(dead_code)]
    pub(crate) const NODE_TYPES: &str =
        include_str!("../vendor-grammars/tree-sitter-kotlin/src/node-types.json");
    #[allow(dead_code)]
    pub(crate) const HIGHLIGHTS_QUERY: &str =
        include_str!("../vendor-grammars/tree-sitter-kotlin/queries/highlights.scm");
}

/// Vendored tree-sitter grammar bindings for Roc sources.
pub(crate) mod tree_sitter_roc {
    use super::LanguageFn;

    unsafe extern "C" {
        fn tree_sitter_roc() -> *const ();
    }

    pub(crate) const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_roc) };
    pub(crate) const NODE_TYPES: &str =
        include_str!("../vendor-grammars/tree-sitter-roc/src/node-types.json");
}
