; Call sites that can be a client call or a data hook. The code reads the callee name,
; the receiver root and the arguments; `new` expressions and tagged templates are not
; calls of this kind.

(call_expression function: (identifier) @name arguments: (arguments) @args) @call
(call_expression
  function: (member_expression
    object: (_) @receiver
    property: (property_identifier) @name)
  arguments: (arguments) @args) @call
