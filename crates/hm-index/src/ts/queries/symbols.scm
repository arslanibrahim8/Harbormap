; Symbol candidates. Each pattern captures the declaration node; extract.rs reads the
; name, the export wrapper and the body from the tree. Only top-level declarations and
; class members are symbols; functions nested in other functions are part of them.
;
; Lines starting with ";;@" open a section: "ts" applies to TypeScript and TSX,
; "jsx" to JavaScript and TSX, "all" to every language.

;; functions
(program [(function_declaration) (generator_function_declaration)] @function)
(program (export_statement declaration: [(function_declaration) (generator_function_declaration)] @function))
(program (export_statement value: [(function_expression) (arrow_function)] @function))

;; module.exports = function f() {}
(program (expression_statement (assignment_expression
  left: (member_expression object: (identifier) @module property: (property_identifier) @exports)
  right: [(function_expression) (arrow_function)] @commonjs))
  (#eq? @module "module") (#eq? @exports "exports"))

;; const f = () => {}, export const db = {}
(program [
  (lexical_declaration (variable_declarator) @declarator)
  (variable_declaration (variable_declarator) @declarator)
  (export_statement declaration: [
    (lexical_declaration (variable_declarator) @declarator)
    (variable_declaration (variable_declarator) @declarator)])])

;; classes, their methods and their `extends`
(program [(class_declaration) @class (export_statement declaration: (class_declaration) @class)])
(class_body (method_definition) @method)
(class_heritage) @heritage

;;@ts
(program [(abstract_class_declaration) @class (export_statement declaration: (abstract_class_declaration) @class)])
(program [
  (interface_declaration) @interface
  (type_alias_declaration) @interface
  (export_statement declaration: [(interface_declaration) (type_alias_declaration)] @interface)])
