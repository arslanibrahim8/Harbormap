; Candidates for the Next.js and React facts of next.rs, which reads names, attributes and
; values from the tree.
;
; Lines starting with ";;@" open a section: "ts" applies to TypeScript and TSX,
; "jsx" to JavaScript and TSX, "all" to every language.

;; a function-like node: its body may start with a directive
[
  (function_declaration)
  (generator_function_declaration)
  (function_expression)
  (generator_function)
  (arrow_function)
  (method_definition)
] @function

;; const L = X.bind(…), const [a, L] = useActionState(X, …)
(variable_declarator
  name: [(identifier) (array_pattern)]
  value: (call_expression)) @declarator

;;@jsx
;; <form>, <button> and <input> elements
(jsx_opening_element name: (identifier) @tag
  (#any-of? @tag "form" "button" "input")) @element
(jsx_self_closing_element name: (identifier) @tag
  (#any-of? @tag "form" "button" "input")) @element
