use std::path::{Path, PathBuf};

fn main() {
    compile_tree_sitter_blade();
    compile_tree_sitter_kotlin();
    compile_tree_sitter_nim();
    compile_tree_sitter_roc();
}

fn compile_tree_sitter_blade() {
    let src_dir = Path::new("vendor-grammars/tree-sitter-blade/src");
    let mut build = cc::Build::new();
    build.std("c11").include(src_dir);
    add_common_flags(&mut build);
    build.file(src_dir.join("parser.c"));
    build.file(src_dir.join("scanner.c"));
    build.compile("frigg-tree-sitter-blade");

    rerun_paths(&[
        src_dir.join("parser.c"),
        src_dir.join("scanner.c"),
        src_dir.join("tag.h"),
        src_dir.join("tree_sitter/alloc.h"),
        src_dir.join("tree_sitter/array.h"),
        src_dir.join("tree_sitter/parser.h"),
    ]);
}

fn compile_tree_sitter_nim() {
    let src_dir = Path::new("vendor-grammars/tree-sitter-nim/src");
    let mut build = cc::Build::new();
    build.include(src_dir);
    add_common_flags(&mut build);
    build.file(src_dir.join("parser.c"));
    build.file(src_dir.join("scanner.c"));
    build.compile("frigg-tree-sitter-nim");

    rerun_paths(&[
        src_dir.join("parser.c"),
        src_dir.join("parser.c.license"),
        src_dir.join("scanner.c"),
        src_dir.join("node-types.json"),
        src_dir.join("node-types.json.license"),
        src_dir.join("tree_sitter/alloc.h"),
        src_dir.join("tree_sitter/array.h"),
        src_dir.join("tree_sitter/parser.h"),
    ]);
}

fn compile_tree_sitter_kotlin() {
    let src_dir = Path::new("vendor-grammars/tree-sitter-kotlin/src");
    let mut build = cc::Build::new();
    build.include(src_dir);
    add_common_flags(&mut build);
    build.file(src_dir.join("parser.c"));
    build.file(src_dir.join("scanner.c"));
    build.compile("frigg-tree-sitter-kotlin");

    rerun_paths(&[
        src_dir.join("parser.c"),
        src_dir.join("scanner.c"),
        src_dir.join("node-types.json"),
        src_dir.join("tree_sitter/alloc.h"),
        src_dir.join("tree_sitter/array.h"),
        src_dir.join("tree_sitter/parser.h"),
    ]);
}

fn compile_tree_sitter_roc() {
    let src_dir = Path::new("vendor-grammars/tree-sitter-roc/src");
    let mut build = cc::Build::new();
    build.include(src_dir);
    add_common_flags(&mut build);
    build.flag_if_supported("-Wno-unused-variable");
    build.flag_if_supported("-Wno-unused-function");
    build.file(src_dir.join("parser.c"));
    build.file(src_dir.join("scanner.c"));
    build.compile("frigg-tree-sitter-roc");

    rerun_paths(&[
        src_dir.join("parser.c"),
        src_dir.join("scanner.c"),
        src_dir.join("node-types.json"),
        src_dir.join("tree_sitter/alloc.h"),
        src_dir.join("tree_sitter/array.h"),
        src_dir.join("tree_sitter/parser.h"),
    ]);
}

fn add_common_flags(build: &mut cc::Build) {
    build
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");

    #[cfg(target_env = "msvc")]
    build.flag("-utf-8");
}

fn rerun_paths(paths: &[PathBuf]) {
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
