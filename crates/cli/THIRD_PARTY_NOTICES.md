This package vendors a small set of tree-sitter grammar sources so the published `frigg` crate
does not depend on unpublished path crates. It also vendors the `sqlite-vec` C extension sources
so release builds do not depend on an external crate build that currently breaks on musl targets.

Included third-party sources:

- `vendor-grammars/tree-sitter-blade/*`
  - Upstream: <https://github.com/EmranMR/tree-sitter-blade>
  - License: MIT
- `vendor-grammars/tree-sitter-kotlin/*`
  - Upstream: <https://github.com/fwcd/tree-sitter-kotlin>
  - License: MIT
- `vendor-grammars/tree-sitter-roc/*`
  - Upstream: <https://github.com/faldor20/tree-sitter-roc>
  - License: MIT
- `vendor-grammars/tree-sitter-nim/*`
  - Upstream: <https://github.com/tree-sitter/tree-sitter-nim>
  - License: MPL-2.0 for the vendored Nim grammar artifacts included in this package
- `vendor-sqlite-vec/*`
  - Upstream: <https://github.com/asg017/sqlite-vec>
  - License: MIT OR Apache-2.0 upstream; the vendored copy in this package is redistributed under the MIT option

License texts shipped with this crate:

- `LICENSES/MIT.txt`
- `LICENSES/MPL-2.0.txt`
- `vendor-grammars/tree-sitter-nim/LICENSE.txt`
- `vendor-grammars/tree-sitter-nim/LICENSES/*`
