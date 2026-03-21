/// <reference types="tree-sitter-cli/dsl" />
// @ts-check
const PREC = {
  PATTERN: 0,
  FIELD_ACCESS_START: 1,
  WHERE_IMPLEMENTS: 1,
  TAG: 1,
  FUNCTION_START: 1,
  PART: 1,
  PREFIX_EXPR: 1,
  TYPEALIAS: 2,
  CASE_OF_BRANCH: 6,
  FUNC: 10,
  IMPORT: 20,
  ARGS: 20,
};

module.exports = grammar({
  name: "roc",

  // The external scanner (scanner.cc) allows us to inject "dummy" tokens into the grammar.
  // These tokens are used to track the indentation-based scoping used in F#

  externals: ($) => [
    $._newline,
    $._end_newline,
    $._indent,
    $._dedent,

    // Mark comments as external tokens so that the external scanner is always
    // invoked, even if no external token is expected. This allows for better
    // error recovery, because the external scanner can maintain the overall
    // structure by returning dedent tokens whenever a dedent occurs, even
    // if no dedent is expected.
    $.comment,

    // Allow the external scanner to check for the validity of closing brackets
    // so that it can avoid returning dedent tokens between brackets.
    "]",
    ")",
    "}",
    "except",
  ],

  extras: ($) => [
    $.line_comment,
    $.doc_comment,
    /[ \s\f\uFEFF\u2060\u200B]|\\\r?n/,
  ],

  conflicts: ($) => [
    // [$.function_call_expr],
    //
    //
    // === conflicts that must exist:
    //Expressions and patterns will always need to be in conflict because we except expressions in the top level so it's impossible to tell if a list is a list experssion or a list destructuring untill you get to the =
    [$._pattern, $._atom_expr],
    [$._atomic_pattern, $._atom_expr],
    [$.body_expression, $.record_expr],
    [$.tag_pattern, $.tag_expr],
    //records are ambiguous with expresion bodies with parens this all makes sense:
    [$.record_field_pattern, $.record_field_expr],
    [$.record_field_pattern, $.record_field_expr, $.annotation_pre_colon],

    [$.record_field_expr, $.annotation_pre_colon],
    [$.record_expr, $.body_expression, $.record_pattern],

    // ===== conflicts that maybe don't need to exist ====
    [$._tags_only],
    [$.identifier_pattern, $.long_identifier],

    [$.list_pattern, $.list_expr],
    [$._module_elem, $.value_declaration],
    [$._module_elem, $.var_declaration],

    // [$.record_type],
  ],
  words: ($) => /\s+/,
  word: ($) => $._lower_identifier,

  inline: ($) => [
    //ELI: temporary while we work out if these two can just go
    // $.expr_body,
    // $.expr_body_terminal,

    $._non_atomic_type,
    // $.module,
    // $.tag,
    $.field_name,
    $.bound_variable,
    $.operator,
    $.suffix_operator,
    // $.variable_expr,
    $.inferred,
  ],

  // supertypes: ($) => [$._module_elem, $._pattern, $._expr_inner],

  rules: {
    file: ($) => seq(optional($._header), repeat1($._module_elem)),
    //TODO i could make a different version of this for when the module is an interface
    _module_elem: ($) =>
      choice(
        $.annotation_type_def,
        $.alias_type_def,
        $.opaque_type_def,
        $.nominal_type_def,
        $.expect,
        $.value_declaration,
        $.var_declaration,
        $.expr_body,
        $.import_expr,
        $.import_file_expr,
      ),

    expect: ($) => prec(1, seq("expect", field("body", $.expr_body))),
    value_declaration: ($) =>
      seq(
        //TODO i should be able to find a better solution that this silly /n
        optional(seq($.annotation_type_def)),

        // $._newline,
        alias($._assignment_pattern, $.decl_left),
        "=",
        field("body", alias($.expr_body_terminal, $.expr_body)),
      ),

    // Mutable variable binding: `var $name = expr`
    // Supports optional type annotations for parity with value declarations.
    var_declaration: ($) =>
      seq(
        optional(seq($.annotation_type_def)),
        "var",
        field("name", $.identifier),
        "=",
        field("body", alias($.expr_body_terminal, $.expr_body)),
      ),

    /**
      Expressions that can appear anywhere in the body of an expression.
      */
    body_expression: ($) =>
      seq(
        "{",
        repeat(choice($.value_declaration, $.var_declaration, $._expr_inner)),
        "}",
      ),

    expr_body: ($) => $._expr_inner,
    expr_body_terminal: ($) => $._expr_inner,

    /**
    atomic expressions can be used as function args without being wrapped in parens
    */
    _atom_expr: ($) =>
      choice(
        $.anon_fun_expr,
        $.const,
        $.record_expr,
        $.record_builder_expr,
        $._variable_expr,
        $.parenthesized_expr,
        $.body_expression,
        $.operator_as_function_expr,
        $.tag_expr,
        $.tuple_expr,
        $.list_expr,
        $.field_access_expr,
        $.todo_expr,
        $.function_call_pnc_expr,
        $.suffix_op_expr,
        $.prefixed_expression,
      ),

    _expr_inner: ($) =>
      choice(
        $.bin_op_expr,
        $._atom_expr,
        $.for_expr,
        $.if_expr,
        $.match_expr,
        $.early_return_expr,
        $.dbg_expr,
        // $.chain_expr,
      ),

    //orginally this had all operators, but it was making the parser almost twice as large so I cut the list down
    prefixed_expression: ($) =>
      prec(
        PREC.PREFIX_EXPR,
        seq(
          choice("!", "*", "-", "^"),
          choice(
            $.const,
            $.parenthesized_expr,
            $.field_access_expr,
            $._variable_expr,
            $.function_call_pnc_expr,
          ),
        ),
      ),
    dbg_expr: ($) => seq("dbg", alias($.expr_body_terminal, $.expr_body)),

    // `for` loop expression: `for pattern in iterable { ... }`
    for_expr: ($) =>
      seq(
        "for",
        field("pattern", $._pattern),
        "in",
        field("iterable", $._expr_inner),
        field("body", $._expr_inner),
      ),
    early_return_expr: ($) => seq("return", field("body", $.expr_body)),

    _variable_expr: ($) =>
      alias($.long_identifier, $.variable_expr),
    parenthesized_expr: ($) => seq("(", field("expression", $.expr_body), ")"),

    if_expr: ($) =>
      seq(
        "if",
        field("guard", $._expr_inner),
        $.then,
        repeat($.else_if),
        $.else,
      ),
    else: ($) => seq("else", $._expr_inner),
    // biome-ignore lint/suspicious/noThenProperty: <explanation>
    then: ($) => seq(field("then", $._expr_inner)),
    else_if: ($) =>
      prec.left(seq("else", "if", field("guard", $._expr_inner), $.then)),

    field_access_expr: ($) =>
      prec.right(
        PREC.FIELD_ACCESS_START,
        seq(
          field("target", $._atom_expr),
          repeat1(seq(".", $.identifier)),
        ),
      ),

    // chain_expr: ($) =>
    //   prec(
    //     5,
    //     seq(
    //       $.function_call_pnc_expr,
    //       repeat1(prec.right(5, seq(".", $.function_call_pnc_expr))),
    //     ),
    //   ),

    function_call_pnc_expr: ($) =>
      prec.right(
        PREC.FUNC,
        seq(
          field("caller", $._atom_expr),
          seq(imm("("), field("args", sep_tail($._expr_inner, ",")), ")"),
        ),
      ),

    operator_as_function_expr: ($) => $._operator_as_function_inner,

    _operator_as_function_inner: ($) =>
      seq("(", field("operator", $.operator_identifier), ")"),


    //OPERTATOR CALLING
    bin_op_expr: ($) =>
      field(
        "part",
        prec(
          PREC.PART,
          seq($._atom_expr, prec.right(repeat1(seq($.operator, $._atom_expr)))),
        ),
      ),
    suffix_op_expr: ($) =>
      field(
        "part",
        prec.left(PREC.PART + 1,
          seq($._atom_expr, $.suffix_operator)),
      ),

    //PATTERN MATCHING
    _match_start: ($) => seq(alias("match", $.match), $._expr_inner),

    match_expr: ($) =>
      seq(
        $._match_start,
        "{",
        repeat1(field("branch", $.match_branch)),
        "}",
      ),

    match_branch: ($) =>
      seq(
        field("pattern", $._pattern),
        optional(seq("if", alias($._expr_inner, $.if))),
        $.fat_arrow,
        //TODO: evaluate what options can got here
        field("expr", $.expr_body),
        optional(",")
      ),
    tag_expr: ($) =>
      prec.left(seq($.tag, repeat(seq("(", $._atom_expr, ")")))),
    anon_fun_expr: ($) =>
      prec.left(seq("|",
        field("args", optional($.argument_patterns)), "|",
        field("body", $.expr_body))),

    //RECORDS

    record_field_expr: ($) =>
      prec.right(seq($.field_name, optional(seq(":", $.expr_body)))),

    record_expr: ($) =>
      seq("{", sep_tail(choice($.record_field_expr, $.spread_expr), ","), "}"),

    record_builder_expr: ($) =>
      seq("{", $.identifier, "<-", sep1_tail($.record_field_expr, ","), "}"),

    //LISTS

    _list_body: ($) =>
      sep1_tail(field("exprList", choice($._expr_inner, $.spread_expr)), ","),
    list_expr: ($) => seq("[", optional($._list_body), "]"),

    spread_expr: ($) => seq("..", $._expr_inner),

    _tuple_body: ($) =>
      seq(
        field("expr", $._expr_inner),
        ",",
        sep1_tail(field("expr", $._expr_inner), ","),
      ),
    tuple_expr: ($) => seq("(", $._tuple_body, ")"),
    todo_expr: ($) => "...",

    //####---------###
    //#### PATTERN ###
    //####---------###
    // Pattern rules (BEGIN)
    _pattern: ($) =>
      choice(
        alias("_", $.wildcard_pattern),
        alias($.const, $.const_pattern),
        $.identifier_pattern,
        $.disjunct_pattern,
        $.conjunct_pattern,
        $.cons_pattern,
        $.paren_pattern,
        $.list_pattern,
        $.tag_pattern,
        $.record_pattern,
        $.tuple_pattern,
        $.spread_pattern,
      ),

    identifier_pattern: ($) => prec(
      PREC.FIELD_ACCESS_START + 1
      , $.identifier),
    cons_pattern: ($) => prec.left(0, seq($._pattern, "::", $._pattern)),
    disjunct_pattern: ($) => prec.left(0, seq($._pattern, "|", $._pattern)),
    conjunct_pattern: ($) => prec.left(0, seq($._pattern, "&", $._pattern)),

    paren_pattern: ($) => seq("(", $._pattern, ")"),
    spread_pattern: ($) =>
      prec.left(seq("..", optional(seq("as", $.identifier)))),

    tag_pattern: ($) =>
      prec.left(seq($.tag, optional(seq('(', field("args", sep_tail($._atomic_pattern, ",")), ')')))),
    tuple_pattern: ($) =>
      prec.right(
        seq(
          "(",
          $._atomic_pattern,
          ",",
          repeat(prec.right(seq($._atomic_pattern, ","))),
          $._atomic_pattern,
          ")",
        ),
      ),

    argument_patterns: ($) =>
      seq($._atomic_pattern, repeat(seq(",", $._atomic_pattern))),
    _atomic_pattern: ($) =>
      choice(
        "null",
        alias("_", $.wildcard_pattern),
        $.const,
        $.identifier_pattern,
        $.list_pattern,
        $.tuple_pattern,
        $.record_pattern,
        $.tag_pattern,
        //TODO: this shhouldn't realy be here
        $.spread_pattern,
        seq("(", $._pattern, ")"),

        // :? atomic_type
      ),
    _assignment_pattern: ($) =>
      choice(
        alias("_", $.wildcard_pattern),
        $.identifier_pattern,
        $.list_pattern,
        $.tuple_pattern,
        $.record_pattern,
      ),

    list_pattern: ($) =>
      choice(
        seq("[", "]"),
        seq("[", $._atomic_pattern, repeat(seq(",", $._atomic_pattern)), "]"),
      ),

    record_pattern: ($) =>
      seq(
        "{",
        sep_tail(
          choice(
            // $.record_field_type,
            $.spread_pattern,
            $.record_field_pattern,
            // $.identifier_pattern,
          ),
          ",",
        ),
        "}",
      ),

    record_field_pattern: ($) =>
      seq($.field_name, optional(seq(":", $._atomic_pattern))),
    //###--------####
    //### HEADER ####
    //###--------###

    _header: ($) =>
      choice(
        $.app_header,
        $.platform_header,
        $.module_header,
        $.package_header,
      ),
    package_header: ($) => seq("package", $.provides_list, $.packages_list),
    app_header: ($) => seq("app", $.provides_list, $.packages_list),
    //TODO make this a function for app and platform
    platform_header: ($) =>
      seq("platform", alias($.string, $.name), $.platform_header_body),
    platform_header_body: ($) =>
      sep1(
        choice($.requires, $.exposes, $.packages, $.provides, $.effects),
        "\n",
      ),

    module_header: ($) => seq("module", $.exposes_list),

    //TODO: should this actually be a record_pattern?
    packages: ($) => seq("packages", $.record_pattern),

    packages_list: ($) =>
      seq("{", sep_tail(choice($.package_ref, $.platform_ref), ","), "}"),

    package_ref: ($) => seq($.identifier, ":", $.string),
    platform_ref: ($) =>
      seq($.identifier, ":", "platform", alias($.string, $.package_uri)),

    exposed_list: ($) => seq("{", sep_tail($.ident, ","), "}"),
    exposes: ($) => seq("exposes", $.exposes_list),
    exposes_list: ($) => seq("exposing", seq("[", sep_tail($.ident, ","), "]")),
    import_ident: ($) =>
      seq(optional(seq($.identifier, ".")), sep1($.module, ".")),
    _import_body: ($) =>
      seq(
        $.import_ident,
        optional(
          choice(
            alias($.exposes_list, $.exposing),
            seq(alias("as", $.as), $.module),
          ),
        ),
      ),
    import_expr: ($) => prec(PREC.IMPORT, seq("import", $._import_body)),
    import_file_expr: ($) =>
      prec(
        PREC.IMPORT,
        seq(
          "import",
          $.string,
          seq(alias("as", $.as), $.identifier, ":", $.concrete_type),
        ),
      ),
    //TODO make a function for all these comma separated trailing comma things
    to: ($) => "to",
    provides: ($) =>
      seq(
        "provides",
        "[",
        optional($.identifier),
        repeat(seq(",", $.identifier)),
        optional(","),
        "]",
        optional(seq($.to, choice($.string, $.ident))),
      ),
    provides_list: ($) =>
      seq(
        "[",
        optional($.identifier),
        repeat(seq(",", $.identifier)),
        optional(","),
        "]",
      ),
    requires: ($) =>
      seq("requires", $.requires_rigids, "{", $.typed_ident, "}"),

    requires_rigids: ($) =>
      choice(
        seq(
          "{",
          optional(
            seq(
              $.requires_rigid,
              repeat(seq(",", $.requires_rigid)),
              optional(","),
            ),
          ),
          "}",
        ),
      ),

    requires_rigid: ($) =>
      seq($.identifier, optional(seq("=>", $._upper_identifier))),

    //####-------###
    //#### TYPES ###
    //####-------###

    annotation_type_def: ($) =>
      seq($.annotation_pre_colon, ":", $._type_annotation),
    alias_type_def: ($) =>
      seq($.apply_type, ":", field("body", $._type_annotation)),

    opaque_type_def: ($) =>
      seq(
        $.apply_type,
        alias("::", $.double_colon),
        $._type_annotation,
        optional($.nominal_methods),
      ),

    // Nominal types: `Name := Type.{ methods }`
    // Use a tight ".{" token so this doesn't conflict with `Type.Module` paths.
    nominal_type_def: ($) =>
      seq(
        $.apply_type,
        alias(":=", $.colon_equals),
        $._type_annotation,
        optional($.nominal_methods),
      ),

    // Nominal method blocks: `.{ ... }`
    // Allow any top-level items (defs, types, expects, expressions) inside.
    // Keep the grammar tight to avoid conflicts with module paths like `Type.Module`.
    nominal_methods: ($) =>
      seq(token.immediate(".{"), repeat($._module_elem), "}"),

    _type_annotation: ($) =>
      prec.left(
        choice(
          $.where_implements,
          $._non_atomic_type
        ),
      ),
    _non_atomic_type: $ =>
      choice($.function_type, $._atomic_type),

    _atomic_type: ($) =>
      choice(
        $.parenthesized_type,
        $.record_type,
        $.apply_type,
        // $.where_implements,
        // $.implements_implementation,
        $.tags_type,
        $.bound_variable,
        $.inferred,
        "*",
        $.tuple_type,
      ),

    function_type: ($) =>
      seq(
        sep1(field("param", $._atomic_type), ","),
        choice($.arrow, $.fat_arrow),
        $._atomic_type,
      ),


    parenthesized_type: ($) => seq("(", $._type_annotation, ")"),
    tuple_type: ($) =>
      seq(
        "(",
        $._type_annotation,
        ",",
        sep1_tail($._type_annotation, ","),
        ")",
      ),

    // Static dispatch constraints: `Type where [a.to_str : a -> b]`.
    // This attaches a constraint list to any type annotation or function type.
    where_implements: ($) =>
      prec.right(
        seq(
          field("type", choice($._atomic_type, $.function_type)),
          alias("where", $.where),
          field("implements", $.static_dispatch_list),
        ),
      ),

    static_dispatch_list: ($) =>
      seq("[", sep_tail($.static_dispatch, ","), "]"),

    static_dispatch: ($) => seq($.static_dispatch_target, ":", $.function_type),

    static_dispatch_target: ($) => seq($.bound_variable, ".", $.identifier),
    spread_type: $ =>
      seq("..", optional($.type_variable)),
    tags_type: ($) =>
      seq(
        "[",
        sep_tail(choice($._tags_only, $.spread_type), ","),
        "]",
      ),

    _tags_only: ($) => seq(sep1(choice($.tag_type), ",")),

    tag_type: ($) =>
      seq(field("name", $._upper_identifier), optional($._apply_type_args)),
    type_variable: ($) => choice($.bound_variable),

    bound_variable: ($) => alias($._lower_identifier, $.bound_variable),

    inferred: ($) => alias("_", $.inferred),

    apply_type: ($) =>
      prec.right(seq($.concrete_type, optional($._apply_type_args))),

    //GOOD
    concrete_type: ($) =>
      prec.right(
        PREC.TYPEALIAS,
        seq(
          $._upper_identifier,
          repeat(prec(PREC.TYPEALIAS, seq(".", $._upper_identifier))),
        ),
      ),

    //we need a n optional \n to stop this eating the value that follows it
    _apply_type_args: ($) =>
      field(
        "type_args",
        prec.right(
          seq(
            imm("("),
            prec.right(PREC.ARGS, sep1_tail($.apply_type_arg, ",")),
            ")",
          ),
        ),
      ),

    apply_type_arg: ($) => prec.left($._atomic_type),

    typed_ident: ($) => seq($.identifier, ":", $._type_annotation),

    record_type: ($) =>
      seq(
        "{",
        sep_tail(choice($.record_field_type, $.spread_type), ","),
        "}",
      ),

    record_field_type: ($) => seq($.field_name, ":", $._type_annotation),
    /** can be used to make tag unions or records open*/

    annotation_pre_colon: ($) =>
      choice(
        //TODO implimeent apply $.apply,
        //tag seems not needed when we have alias
        // $.tag,
        $.identifier,
      ),

    effects: ($) =>
      seq(
        // '__',
        "effects",
        $.effect_name,
        $.record_type,
      ),

    effect_name: ($) => seq($.identifier, ".", $._upper_identifier),
    //##------------##
    //##-- consts --##
    //##------------##

    const: ($) =>
      choice(
        // Dot-suffix patterns must come before generic patterns
        $.uint_dot,
        $.iint_dot,
        $.decimal_dot,
        $.xint_dot,
        $.float,
        $.xint,
        $.decimal,
        $.natural,
        $.uint,
        $.iint,

        $.char,
        $.string,
        $.multiline_string,
        $.int,
        "false",
        "true",
        // $.unit,
      ),

    //STRINGS
    string: ($) =>
      seq(
        '"',
        repeat(
          choice(imm(prec(0, /[^\n\\"]/)), $.interpolation_char, $.escape_char),
        ),
        '"',
      ),

    multiline_string: ($) =>
      prec.right(
        repeat1(
          seq(
            "\\\\",
            repeat(
              choice(
                imm(prec(0, /[^\\\n]/)),
                $.interpolation_char,
                $.escape_char,
              ),
            ),
            $._newline,
          ),
        ),
      ),

    escape_char: ($) => imm(/\\([\\"\'ntbrafv]|(\$\{))|(\\u\([0-9A-F]{1,8}\))/),
    interpolation_char: ($) =>
      seq(
        imm("${"), //This is the new interpolation syntax
        $._expr_inner,
        "}",
      ),
    _simple_string_char: ($) => /[^\t\r\u0008\a\f\v\\"]/,
    _simple_char_char: ($) => imm(/[^\n\t\r\u0008\a\f\v'\\]/),
    char: ($) => seq("'", choice($.escape_char, $._simple_char_char), imm("'")),

    //NUMBERS
    int: ($) => token(/[0-9][0-9_]*/),

    //ROC - Dot-suffix format (new syntax)
    uint_dot: ($) =>
      token(seq(/[0-9][0-9_]*/, imm(/\./), imm(/U(8|16|32|64|128)/))),
    iint_dot: ($) =>
      token(seq(/[0-9][0-9_]*/, imm(/\./), imm(/I(8|16|32|64|128)/))),
    decimal_dot: ($) => token(seq(/[0-9][0-9_]*/, imm(/\./), imm(/Dec/))),
    xint_dot: ($) =>
      token(
        seq(
          choice(
            seq(/0[x]/, /[0-9abcdefABCDEF][0-9abcdefABCDEF_]*/),
            seq(/0[b]/, /[01][01_]*/),
          ),
          imm(/\./),
          imm(/[UI](8|16|32|64|128)/),
        ),
      ),

    //ROC - Immediate suffix format (old syntax, still supported)
    uint: ($) => token(seq(/[0-9][0-9_]*/, imm(/u(32|8|16|64|128)/))),
    iint: ($) => token(seq(/[0-9][0-9_]*/, imm(/i(32|8|16|64|128)/))),
    decimal: ($) => token(/[0-9]+(\.)?[0-9]*(dec)/),
    natural: ($) => token(/[0-9]+(nat)/),

    float: ($) => token(/[0-9]+(\.)?[0-9]*(e-?[0-9]*)?((f32)|(f64))?/),
    _hex_int: ($) => token(/0[x][0-9abcdefABCDEF]*/),
    _ocal_int: ($) => token(/0[o][0-7]*/),
    _binary_int: ($) => token(seq(/0[b]/, /[01][01_]*/)),
    xint: ($) => choice($._binary_int, $._hex_int, $._ocal_int),

    //PRIMATIVES
    back_arrow: ($) => "<-",
    arrow: ($) => "->",
    fat_arrow: ($) => "=>",
    field_name: ($) => alias($.identifier, $.field_name),

    long_identifier: ($) => prec.right(
      PREC.FIELD_ACCESS_START + 1
      , seq(repeat(seq($.module, imm("."))), $.identifier)),
    _long_upper_identifier: ($) =>
      prec.right(
        seq(
          repeat(seq($.module, imm("."))),
          alias($._upper_identifier, $.identifier),
        ),
      ),
    ident: ($) => choice($.identifier, $.module),

    identifier: ($) =>
      prec(
        100,
        seq(
          optional("$"),
          optional("_"),
          $._lower_identifier,
          optional(imm("!")),
        ),
      ),

    _lower_identifier: ($) => /[\p{Ll}][\p{XID_Continue}]*/,

    _upper_identifier: ($) => /[\p{Lu}][\p{XID_Continue}]*/,
    tag: ($) => $._long_upper_identifier,
    module: ($) => $._upper_identifier,
    backslash: ($) => "\\",

    doc_comment: ($) => token(prec(-1, /##[^\n]*/)),
    line_comment: ($) => token(prec(-1, /#[^\n]*/)),

    suffix_operator: ($) =>
      alias($.suffix_operator_identifier, $.suffix_operator),
    suffix_operator_identifier: ($) => imm("?"),

    operator: ($) => alias($.operator_identifier, $.operator),
    operator_identifier: ($) =>
      choice(
        "and",
        "or",
        "&&",
        "||",
        "+",
        "*",
        "-",
        "//",
        "/",
        "<=",
        "<",
        ">=",
        ">",
        "^",
        "%",
        "->",
        "==",
        "!="
      ),
  },
});

function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)));
}
function sep1end(rule, separator, end) {
  return seq(repeat(seq(rule, separator)), end);
}
function sep1_tail(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)), optional(separator));
}
function sep_tail(rule, separator) {
  return optional(sep1_tail(rule, separator));
}

function imm(x) {
  return token.immediate(x);
}
