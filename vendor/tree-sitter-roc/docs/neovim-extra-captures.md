# Neovim-only captures (vs. base/Helix)

This document lists capture names used in the Neovim query files that do not
appear in the base query files under [queries](queries), which define the Helix
mapping surface.

## Highlights

Neovim highlights in [neovim/queries/highlights.scm](neovim/queries/highlights.scm) include these captures not present in [queries/highlights.scm](queries/highlights.scm):

- `@boolean`
- `@character`
- `@comment`
- `@comment.documentation`
- `@constant.builtin`
- `@function.call`
- `@keyword`
- `@keyword.conditional`
- `@keyword.debug`
- `@keyword.import`
- `@number`
- `@number.float`
- `@spell`
- `@variable.member`

## Indents

Neovim indents in [neovim/queries/indents.scm](neovim/queries/indents.scm) include these captures not present in [queries/indents.scm](queries/indents.scm):

- `@indent.align`
- `@indent.begin`
- `@indent.branch`
- `@indent.ignore`

## Injections

No Neovim-only captures. Both files use `@injection.content` and `@injection.language`.

## Locals

Neovim locals in [neovim/queries/locals.scm](neovim/queries/locals.scm) include these captures not present in [queries/locals.scm](queries/locals.scm):

- `@local.definition`
- `@local.definition.function`
- `@local.definition.type`
- `@local.definition.var`

## Textobjects

Neovim textobjects in [neovim/queries/textobjects.scm](neovim/queries/textobjects.scm) include these captures not present in [queries/textobjects.scm](queries/textobjects.scm):

- `@class.inner`
- `@class.outer`
- `@comment.outer`
- `@function.inner`
- `@function.outer`
- `@parameter.inner`
- `@parameter.outer`