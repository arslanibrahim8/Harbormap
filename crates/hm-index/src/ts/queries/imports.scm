; Module boundaries: imports, `export` statements (which also give re-exports),
; dynamic `import("s")` and `require("s")` with a literal specifier.

(import_statement source: (string) @source) @import
(program (export_statement) @export)
(call_expression function: (import) arguments: (arguments . (string) @source)) @dynamic
(call_expression function: (identifier) @fn arguments: (arguments . (string) @source) (#eq? @fn "require")) @require
