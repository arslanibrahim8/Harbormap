; Call sites with an identifier or a selector as the function. Generic instantiations,
; call results and function literals are not matched.

(call_expression function: (identifier) @callee) @call
(call_expression function: (selector_expression
  operand: (_) @receiver
  field: (field_identifier) @callee)) @call
