//! Server-side web facts: Express routes and bindings, NestJS decorators and constructor
//! fields, dynamic-code gaps and environment reads (Learn spec §6.3).

mod common;

use common::read_fixture;
use hm_index::facts::{ArgFact, FileFacts, SymbolKind};
use hm_index::lang::Lang;
use hm_index::ts::extract;

fn facts(rel: &str) -> FileFacts {
    extract(&read_fixture(rel), Lang::TypeScript)
}

fn ts(src: &str) -> FileFacts {
    extract(src, Lang::TypeScript)
}

fn s(text: &str) -> ArgFact {
    ArgFact::Str(text.to_string())
}

fn name(text: &str) -> ArgFact {
    ArgFact::Name(text.to_string())
}

fn owner(class: &str, member: &str) -> Option<(String, String)> {
    Some((class.to_string(), member.to_string()))
}

// Routing forms: https://expressjs.com/en/guide/routing.html (`app.METHOD(path, handler…)`,
// `app.use(…)`, `express.Router()`).
#[test]
fn express_calls_and_bindings_are_recorded() {
    let app = facts("express-api/src/app.ts");
    let bindings: Vec<_> = app
        .web
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.init.as_str(), b.owner.clone()))
        .collect();
    assert_eq!(bindings, vec![("app", "express", None)]);
    let calls: Vec<_> = app
        .web
        .route_calls
        .iter()
        .map(|c| (c.receiver.as_str(), c.method.as_str(), c.args.clone()))
        .collect();
    assert_eq!(
        calls,
        vec![
            ("app", "use", vec![name("requireUser")]),
            ("app", "use", vec![s("/api"), name("orders")]),
            (
                "app",
                "get",
                vec![s("/health"), ArgFact::Inline(Some("GET /health".into()))]
            ),
        ]
    );
    assert_eq!(app.web.route_calls[2].line, 9);

    let orders = facts("express-api/src/routes/orders.ts");
    let bindings: Vec<_> = orders
        .web
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.init.as_str()))
        .collect();
    assert_eq!(bindings, vec![("router", "Router")]);
    let calls: Vec<_> = orders
        .web
        .route_calls
        .iter()
        .map(|c| (c.method.as_str(), c.args.clone()))
        .collect();
    assert_eq!(
        calls,
        vec![
            ("get", vec![s("/orders"), name("listOrders")]),
            (
                "post",
                vec![s("/orders"), name("validate"), name("placeOrder")]
            ),
            (
                "delete",
                vec![
                    s("/orders/:id"),
                    ArgFact::Inline(Some("DELETE /orders/:id".into()))
                ]
            ),
        ]
    );
}

#[test]
fn top_level_inline_callbacks_become_symbols() {
    let orders = facts("express-api/src/routes/orders.ts");
    let callback = orders
        .symbols
        .iter()
        .find(|y| y.name == "DELETE /orders/:id")
        .expect("the callback is a symbol");
    assert_eq!(callback.kind, SymbolKind::Function);
    assert_eq!(callback.owner, "");
    assert_eq!(callback.sig, "DELETE /orders/:id");
    assert!(!callback.exported && !callback.default_export);
    assert_eq!((callback.l0, callback.l1, callback.lines), (16, 19, 4));
    assert!(!callback.sig_hash.is_empty() && !callback.body_hash.is_empty());
    let call = orders
        .calls
        .iter()
        .find(|c| c.callee == "removeOrder")
        .expect("the call inside the callback");
    assert_eq!(
        call.owner,
        Some((String::new(), "DELETE /orders/:id".into()))
    );

    let app = facts("express-api/src/app.ts");
    assert!(app.symbols.iter().any(|y| y.name == "GET /health"));
}

// Only the last argument of a top-level HTTP-verb call is a callback symbol; `ALL` stands for
// `app.all`; `app.use` and nested calls make none, and the fact says so with `Inline(None)`.
#[test]
fn callbacks_follow_the_same_rule_as_the_facts() {
    let f = ts("import express from 'express';
const app = express();
app.all('/x', (req, res) => {});
app.use('/y', (req, res, next) => next());
app.get('/z', (req, res, next) => next(), (req, res) => {});
function late() {
  app.get('/inner', (req, res) => {});
}
");
    let names: Vec<_> = f.symbols.iter().map(|y| y.name.as_str()).collect();
    assert_eq!(names, vec!["ALL /x", "GET /z", "late"]);
    let args: Vec<_> = f.web.route_calls.iter().map(|c| c.args.clone()).collect();
    assert_eq!(
        args,
        vec![
            vec![s("/x"), ArgFact::Inline(Some("ALL /x".into()))],
            vec![s("/y"), ArgFact::Inline(None)],
            vec![
                s("/z"),
                ArgFact::Inline(None),
                ArgFact::Inline(Some("GET /z".into()))
            ],
            vec![s("/inner"), ArgFact::Inline(None)],
        ]
    );
}

// Decorators: https://docs.nestjs.com/controllers (`@Controller('cats')`, `@Get(':id')`,
// `@Post()`).
#[test]
fn nest_decorators_and_ctor_fields_are_recorded() {
    let c = facts("nest-api/src/orders/orders.controller.ts");
    let decorators: Vec<_> = c
        .web
        .decorators
        .iter()
        .map(|d| (d.target.clone(), d.name.as_str(), d.args.clone()))
        .collect();
    assert_eq!(
        decorators,
        vec![
            (
                ("".to_string(), "OrdersController".to_string()),
                "Controller",
                vec![s("orders")]
            ),
            (("OrdersController".into(), "list".into()), "Get", vec![]),
            (
                ("OrdersController".into(), "one".into()),
                "Get",
                vec![s(":id")]
            ),
            (("OrdersController".into(), "place".into()), "Post", vec![]),
            (
                ("OrdersController".into(), "place".into()),
                "UseGuards",
                vec![name("AuthGuard")]
            ),
        ]
    );
    assert_eq!(c.web.decorators[0].line, 5);
    assert_eq!(c.web.ctor_fields.len(), 1);
    let field = &c.web.ctor_fields[0];
    assert_eq!(
        (
            field.class.as_str(),
            field.field.as_str(),
            field.type_name.as_str()
        ),
        ("OrdersController", "orders", "OrdersService")
    );
    let call = c.calls.iter().find(|x| x.callee == "findAll").unwrap();
    assert_eq!(call.receiver.as_deref(), Some("this.orders"));

    let service = facts("nest-api/src/orders/orders.service.ts");
    assert_eq!(service.web.decorators.len(), 1);
    assert_eq!(
        service.web.decorators[0].target,
        ("".to_string(), "OrdersService".to_string())
    );
    assert_eq!(service.web.decorators[0].name, "Injectable");

    let main = facts("nest-api/src/main.ts");
    assert_eq!(main.web.bindings.len(), 1);
    let b = &main.web.bindings[0];
    assert_eq!(
        (b.name.as_str(), b.init.as_str()),
        ("app", "NestFactory.create")
    );
    assert_eq!(b.owner, owner("", "bootstrap"));
    assert_eq!(main.web.route_calls.len(), 1);
    let r = &main.web.route_calls[0];
    assert_eq!(
        (r.receiver.as_str(), r.method.as_str(), r.args.clone()),
        ("app", "setGlobalPrefix", vec![s("api")])
    );
    assert_eq!(r.owner, owner("", "bootstrap"));
}

#[test]
fn decorator_object_arguments_and_exported_classes() {
    let f = ts("@Controller({ path: 'orders' })
export class A {}
export @Injectable() class B {}
@Controller({ host: 'x' })
class C {}
");
    let got: Vec<_> = f
        .web
        .decorators
        .iter()
        .map(|d| (d.target.1.as_str(), d.name.as_str(), d.args.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "A",
                "Controller",
                vec![ArgFact::Object {
                    path: Some("orders".into())
                }]
            ),
            ("B", "Injectable", vec![]),
            ("C", "Controller", vec![ArgFact::Object { path: None }]),
        ]
    );
}

#[test]
fn only_plain_type_parameter_properties_are_ctor_fields() {
    let f = ts("class A {
  constructor(private readonly a: Aa, public b: Bb, c: Cc, private d: Dd<X>, private e) {}
}
");
    let got: Vec<_> = f
        .web
        .ctor_fields
        .iter()
        .map(|c| (c.field.as_str(), c.type_name.as_str()))
        .collect();
    assert_eq!(got, vec![("a", "Aa"), ("b", "Bb")]);
}

#[test]
fn gaps_and_env_reads_are_recorded() {
    let f = facts("express-api/src/plugins.ts");
    let gaps: Vec<_> = f.web.gaps.iter().map(|g| g.kind.as_str()).collect();
    assert_eq!(gaps, vec!["eval", "dynamic_import"]);
    assert_eq!(f.web.gaps[0].owner, owner("", "runPlugin"));
    assert_eq!(f.web.gaps[0].line, 2);
    let env: Vec<_> = f.web.env.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(env, vec!["SHOP_KEY"]);
    assert_eq!(f.web.env[0].line, 3);
}

#[test]
fn every_gap_kind_and_env_spelling() {
    let f = ts("eval(a);
new Function('return 1');
import(x);
import('./literal');
require(y);
require('fs');
const k = process.env['A_KEY'] + process.env.B_KEY + import.meta.env.C_KEY;
const d = process.env[dynamic];
");
    let gaps: Vec<_> = f.web.gaps.iter().map(|g| g.kind.as_str()).collect();
    assert_eq!(
        gaps,
        vec!["eval", "new_function", "dynamic_import", "dynamic_require"]
    );
    let env: Vec<_> = f.web.env.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(env, vec!["A_KEY", "B_KEY", "C_KEY"]);
}

// The receiver is not checked here: Task 9 keeps only receivers bound to express.
#[test]
fn a_map_get_with_a_key_is_not_a_route_call() {
    let f = ts("const cache = new Map();
cache.get('k');
cache.get('/k');
cache.use(1);
cache.setGlobalPrefix(2);
cache.get(key);
");
    let got: Vec<_> = f
        .web
        .route_calls
        .iter()
        .map(|c| (c.method.as_str(), c.args.clone()))
        .collect();
    assert_eq!(
        got,
        vec![("get", vec![s("/k")]), ("use", vec![ArgFact::Other])]
    );
}

#[test]
fn bindings_cover_await_new_and_member_callees() {
    let f = ts("const a = express();
const b = await NestFactory.create(M);
const c = new Router();
const d = express.Router();
const e = other();
const { f } = express();
");
    let got: Vec<_> = f
        .web
        .bindings
        .iter()
        .map(|b| (b.name.as_str(), b.init.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("a", "express"),
            ("b", "NestFactory.create"),
            ("c", "Router"),
            ("d", "express.Router"),
        ]
    );
}

#[test]
fn javascript_files_work_too() {
    let f = extract(
        "const router = require('express').Router();
const app = express();
app.get('/ping', function (req, res) { res.send(process.env.PONG); });
",
        Lang::JavaScript,
    );
    assert_eq!(f.web.bindings.len(), 1);
    assert_eq!(f.web.route_calls.len(), 1);
    assert!(f.symbols.iter().any(|y| y.name == "GET /ping"));
    assert_eq!(f.web.env[0].name, "PONG");
    assert_eq!(f.web.env[0].owner, owner("", "GET /ping"));
}

#[test]
fn a_file_broken_mid_typing_still_yields_what_parsed() {
    let f = ts("const app = express();
app.get('/ok', (req, res) => {});
app.get('/broken', (req, res) => {
const k = process.env.KEY;
");
    assert_eq!(f.web.bindings.len(), 1);
    assert!(f.web.route_calls.iter().any(|c| c.method == "get"));
    assert!(f.symbols.iter().any(|y| y.name == "GET /ok"));
    assert!(f.web.env.iter().any(|e| e.name == "KEY"));
}
