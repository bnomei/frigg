This package vendors a small set of tree-sitter grammar sources so the published `frigg` crate
does not depend on unpublished path crates.

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

License texts shipped with this crate:

- `LICENSES/MIT.txt`
- `LICENSES/MPL-2.0.txt`
- `vendor-grammars/tree-sitter-nim/LICENSE.txt`
- `vendor-grammars/tree-sitter-nim/LICENSES/*`
