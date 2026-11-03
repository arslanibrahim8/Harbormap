;; Top-level definitions, class-body methods and inner names. The Rust side reads
;; names, decorators and bodies from the captured nodes.
(module (function_definition) @function)
(module (decorated_definition definition: (function_definition) @function))
(module (class_definition) @class)
(module (decorated_definition definition: (class_definition) @class))
(module (expression_statement (assignment left: (identifier)) @variable))
(class_definition body: (block (function_definition) @method))
(class_definition body: (block (decorated_definition definition: (function_definition) @method)))
(function_definition) @inner
(assignment left: (identifier) right: (lambda)) @lambda
