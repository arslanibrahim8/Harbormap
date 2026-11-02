; Call sites. `require(...)` is an import fact and is left out here.
;
; Lines starting with ";;@" open a section: "ts" applies to TypeScript and TSX,
; "jsx" to JavaScript and TSX, "all" to every language.

(call_expression function: (identifier) @callee (#not-eq? @callee "require")) @call
(call_expression function: (member_expression
  object: (_) @receiver
  property: [(property_identifier) (private_property_identifier)] @callee)) @call
(new_expression constructor: (identifier) @callee) @call
(new_expression constructor: (member_expression object: (_) @receiver property: (property_identifier) @callee)) @call

;;@jsx
(jsx_opening_element name: (identifier) @callee) @jsx
(jsx_self_closing_element name: (identifier) @callee) @jsx
(jsx_opening_element name: (member_expression object: (_) @receiver property: (property_identifier) @callee)) @jsx
(jsx_self_closing_element name: (member_expression object: (_) @receiver property: (property_identifier) @callee)) @jsx
