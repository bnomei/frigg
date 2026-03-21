; Function calls
(function_call_pnc_expr
    caller: (variable_expr (identifier) ))

(function_call_pnc_expr
  caller: (field_access_expr (identifier) .))

;function definition:
 (value_declaration
    (decl_left
      (identifier_pattern
        (identifier)
        ))
    body: (expr_body(anon_fun_expr)))
