(identifier) @variable



[
  (concrete_type)
  (tag_type)] @type



(module) @type



;;
;; Lower-priorty queries
;;



(argument_patterns                (identifier_pattern (identifier) @variable))
(argument_patterns (_             (identifier_pattern (identifier) @variable)))
(argument_patterns (_ (_          (identifier_pattern (identifier) @variable))))
(argument_patterns (_ (_ (_       (identifier_pattern (identifier) @variable)))))
(argument_patterns (_ (_ (_ (_    (identifier_pattern (identifier) @variable))))))
(argument_patterns (_ (_ (_ (_ (_ (identifier_pattern (identifier) @variable)))))))
(spread_pattern                                       (identifier) @variable)
(match_branch pattern: (_       (identifier_pattern (identifier) @variable)))

; N/A
; @variable.other.member.private

(field_name)                         @property
; Note: This query matches the second identifier and all subsequent ones.
(field_access_expr      (identifier) @property)
; Note: This query highlights module members as records instead of free variables,
;       which avoids highlighting them as out-of-scope vars.
(variable_expr (module) (identifier) @property)

; N/A
; @variable.other

; N/A
; @variable.builtin

(record_field_pattern (_ (identifier) @variable))

; Note: See the lower-priority queries below for a `@variable` query.



(inferred) @type

(bound_variable) @type

(tag_type) @variant

; N/A
; @type.enum

; Opinion: Type defs cross into documentation
;          and should be highlighted differently from normal code.
(opaque_type_def (_ (concrete_type) @type))

((concrete_type) @type
  (#match? @type "^(Dec|F(32|64))"))
((concrete_type) @type
  (#match? @type "^[IU](8|16|32|64|128)"))
((concrete_type) @type
  (#match? @type "^(Bool|Box|Dec|Decode|Dict|Encode|Hash|Inspect|Int|List|Num|Result|Set|Str)"))

; Note: See the lower-priority queries below for a `@type` query.



; N/A
; @tag.builtin

; N/A (We use `@constructor` and `@type.enum.variant` for "tags".)
; @tag



(app_header (packages_list (platform_ref ((package_uri) @link_uri))))




(app_header (packages_list (platform_ref ((package_uri) @link_uri))))

; N/A
; @string.special.symbol

; N/A
; @string.special.path

; N/A
; @string.special

; N/A
; @string.regexp

(string) @string
(multiline_string) @string



; TODO: Differentiate between values, functions, and types.
(import_expr (exposing ((ident) @label)))

(app_header (packages_list ((platform_ref) @label)))

; TODO: Differentiate between values, functions, and types.
(app_header (provides_list ((identifier) @label)))

; N/A
; @special



[
  (interpolation_char)
] @punctuation.special

[
  ","
  ":"
  (arrow)
  (fat_arrow)
] @punctuation.delimiter

[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
  "|" ; TODO: This conflicts with the `"|" @operator` query, so improve both.
] @punctuation.bracket

; N/A
; @punctuation



[
  "="
  "."
  "&"
  ; "|" ; TODO: This conflicts with the `"|" @punctuation.bracket` query, so improve both.
  "<-"
  "->"
  ".."
  "!"
  "*"
  "-"
  "^"
  (wildcard_pattern)
  (operator)
] @operator



; N/A
; @label



; TODO: Implement this for `var`.
; @keyword.storage.type

; N/A
; @keyword.storage.modifier

; TODO: Implement this for `and`, `or`, and any others.
[
   (suffix_operator)
  ] @operator

; N/A
; @keyword.function

; N/A
; @keyword.directive

; TODO: Also implement this for `return`.
[(suffix_operator ) "return"]@keyword

; TODO: Implement this for `for` and `while`.
; @keyword.control.repeat

[
  "import"
] @keyword

; N/A
; @keyword.control.exception

[
  "else"
  "if"

  (match)
] @keyword

[
  "app"
  (as)
  "as"
  "expect"
  "exposing"
  "module"
  "package"
  "platform"
  (to)
  "var"
  (where)
] @keyword

; N/A
;
; @keyword



[
  "dbg"
] @function

(value_declaration (decl_left (identifier_pattern  (identifier) @function))
  (expr_body (anon_fun_expr)))
(function_call_pnc_expr caller: (variable_expr     (identifier) @function))
(function_call_pnc_expr caller: (field_access_expr (identifier) @function .))
(bin_op_expr (operator "->") (variable_expr        (identifier) @function))
(annotation_type_def (annotation_pre_colon         (identifier) @function)
  (function_type))



  (tag (identifier)@constructor)




[
  (decimal)
  (float)
] @number

[
  (iint)
  (int)
  (natural)
  (uint)
  (xint)
] @number

; N/A
; @constant.numeric

(escape_char) @string.escape

(char) @string

(tag_expr(tag (module) @variable.special "." (identifier)@boolean)
  (#eq? @boolean "True") (#eq? @variable.special "Bool"))
(tag_expr (tag(module) @type "." (identifier)@boolean)
  (#eq? @boolean "False") (#eq? @type "Bool"))

; N/A
; @constant.builtin

; N/A
; @constant



(line_comment) @comment

(doc_comment) @comment.doc

; N/A
; @comment.block

; N/A
; @comment



; N/A
; @attribute



(record_field_type (field_name) @property)


(function_type "," @punctuation.delimiter)
(record_type   "," @punctuation.delimiter)
(tuple_type    "," @punctuation.delimiter)

(parenthesized_type ["(" ")"] @punctuation.bracket)
(record_type        ["{" "}"] @punctuation.bracket)
(tags_type          ["[" "]"] @punctuation.bracket)
(tuple_type         ["(" ")"] @punctuation.bracket)

(static_dispatch_target
(identifier)@function)


((module) @type
  (#match? @type "^(Bool|Box|Decode|Dict|Encode|Hash|Inspect|List|Num|Result|Set|Str)"))
; TODO(bugfix): `Set` yields an ERROR in `expect Set.from_list(paths_as_str) == Set.from_list(["nested-dir/a", "nested-dir/child"])`



;;
;; Higher-priorty queries
;;



;; Highlight names (like `@comment.block.documentation`) are arbitrary.
;; However, some text editors encourage a standard set in their themes.
;; For consistency and quality, these queries assign the highlight names that Helix uses:
;; see https://docs.helix-editor.com/themes.html#scopes
