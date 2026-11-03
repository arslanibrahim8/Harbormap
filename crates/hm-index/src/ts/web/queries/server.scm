; Server-side candidates: Express and NestJS wiring, dynamic-code gaps and environment
; reads. Each pattern captures one node; server.rs reads names, arguments and owners from
; the tree and applies the rules (which receivers, which methods, which arguments).
;
; Lines starting with ";;@" open a section: "ts" applies to TypeScript and TSX,
; "jsx" to JavaScript and TSX, "all" to every language.

;; receiver.method(...): app.get('/x', h), router.use(r), app.setGlobalPrefix('api')
(call_expression
  function: (member_expression
    object: (identifier)
    property: (property_identifier))
  arguments: (arguments)) @route

;; const app = express(), await NestFactory.create(M), new Router()
(variable_declarator
  name: (identifier)
  value: [
    (call_expression)
    (await_expression (call_expression))
    (new_expression)]) @binding

;; @Controller('x'), @Injectable, @Get(':id')
(decorator) @decorator

;; constructor(private readonly orders: OrdersService) {}
(method_definition
  name: (property_identifier) @ctor_name
  (#eq? @ctor_name "constructor")) @ctor

;; eval(x), import(x), require(x)
(call_expression function: [(identifier) (import)] @callee) @gap_call

;; new Function(...)
(new_expression constructor: (identifier) @callee) @gap_new

;; process.env.NAME, import.meta.env.NAME, process.env['NAME']
(member_expression
  object: (member_expression)
  property: (property_identifier)) @env_member
(subscript_expression
  object: (member_expression)
  index: (string)) @env_subscript
