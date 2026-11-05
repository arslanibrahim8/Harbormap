; Top-level declarations and function literals. The code in `extract.rs` reads names,
; receivers and bodies from the captured nodes and drops the declarations that are not
; at the top level of the file.

(function_declaration) @function
(method_declaration) @method
(type_spec) @type
(type_alias) @type
(func_literal) @literal
