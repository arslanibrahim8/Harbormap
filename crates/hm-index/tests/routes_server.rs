//! Express and NestJS routes, what runs on the way in (Express `use`, Nest guards) and the
//! calls through constructor injection, derived from the facts of a whole tree (Learn spec
//! §6.3).
//!
//! Rules checked against the official docs while building:
//! - `app.METHOD(path, callback…)`, `app.all`, several callbacks per route, `express.Router`
//!   mounted with `app.use('/birds', birds)`; `:name` parameters, Express 5 `*name` and
//!   `{*name}` wildcards and `{…}` optional parts.
//!   https://expressjs.com/en/guide/routing.html
//! - `use([path,] fn…)`: the path defaults to `/` and matches its sub-paths; middleware runs
//!   in the order it is declared; a router is valid middleware.
//!   https://expressjs.com/en/5x/api.html#router (https://expressjs.com/en/5x/api/router/,
//!   https://expressjs.com/en/5x/api/application/)
//! - `@Controller('prefix')`, `@Get() @Post() @Put() @Delete() @Patch() @Options() @Head()
//!   @All()`, `:id` parameters. https://docs.nestjs.com/controllers
//! - `app.setGlobalPrefix('v1')` prefixes every route. https://docs.nestjs.com/faq/global-prefix
//! - `@UseGuards(G)` on a method or its controller; guards run after middleware, before
//!   the handler. https://docs.nestjs.com/guards

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{copy_fixture, edit, fixture, read_fixture};
use hm_index::lang::Lang;
use hm_index::routes::{Derived, Route, derive_tree};
use hm_index::ts::resolve::symbol_ids;

/// `(method, pattern)` of every route.
fn shapes(d: &Derived) -> BTreeSet<(String, String)> {
    d.routes
        .iter()
        .map(|r| (r.method.clone(), r.pattern.clone()))
        .collect()
}

fn shape(method: &str, pattern: &str) -> (String, String) {
    (method.to_string(), pattern.to_string())
}

fn find<'a>(d: &'a Derived, method: &str, pattern: &str) -> &'a Route {
    d.routes
        .iter()
        .find(|r| r.method == method && r.pattern == pattern)
        .unwrap_or_else(|| panic!("no {method} {pattern} in {:#?}", d.routes))
}

/// The id of symbol `owner.name` (`owner` empty at top level) of `rel` under `root`.
fn symbol_in(root: &std::path::Path, rel: &str, owner: &str, name: &str) -> String {
    let lang = Lang::from_path(std::path::Path::new(rel)).expect("a source file");
    let text = fs::read_to_string(root.join(rel)).expect("read a source file");
    let facts = hm_index::ts::extract(&text, lang);
    let at = facts
        .symbols
        .iter()
        .position(|s| s.owner == owner && s.name == name)
        .unwrap_or_else(|| panic!("no symbol {owner}.{name} in {rel}"));
    symbol_ids(rel, &facts).swap_remove(at)
}

/// The id of symbol `owner.name` of fixture file `fixture_name/rel`.
fn symbol(fixture_name: &str, rel: &str, owner: &str, name: &str) -> String {
    // Read through the fixture helper so a missing file names the fixture.
    let _ = read_fixture(&format!("{fixture_name}/{rel}"));
    symbol_in(&fixture(fixture_name), rel, owner, name)
}

/// `("METHOD pattern", dst)` of every edge with `rule` whose source is a route.
fn route_edges(d: &Derived, rule: &str) -> BTreeSet<(String, String)> {
    d.edges
        .iter()
        .filter(|e| e.rule == rule)
        .map(|e| {
            let r = d
                .routes
                .iter()
                .find(|r| r.id == e.src_id)
                .unwrap_or_else(|| panic!("edge from a missing route: {e:?}"));
            (format!("{} {}", r.method, r.pattern), e.dst_id.clone())
        })
        .collect()
}

/// Writes `files` (relative path, text) under a fresh temporary folder.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create a folder");
        fs::write(path, text).expect("write a file");
    }
    dir
}

const EXPRESS_PACKAGE: &str = "{\"dependencies\": {\"express\": \"5.1.0\"}}\n";

#[test]
fn express_routes_join_mount_prefixes() {
    // https://expressjs.com/en/guide/routing.html (`express.Router`, `app.use('/birds',
    // birds)`, `:id`)
    let d = derive_tree(&fixture("express-api"));
    let want: BTreeSet<_> = [
        shape("GET", "/api/orders"),
        shape("POST", "/api/orders"),
        shape("DELETE", "/api/orders/{id}"),
        shape("GET", "/health"),
    ]
    .into_iter()
    .collect();
    assert_eq!(shapes(&d), want);
    assert!(
        d.routes.iter().all(|r| r.entry_kind == "http"
            && r.rule == "ts.express.route"
            && r.prov == "matched"
            && r.conf == "probably"
            && r.root.is_empty()
            && r.detail.is_none()),
        "{:#?}",
        d.routes
    );

    let get = find(&d, "GET", "/api/orders");
    assert_eq!((get.file.as_str(), get.line), ("src/routes/orders.ts", 14));
    let list = symbol("express-api", "src/orders.ts", "", "listOrders");
    assert_eq!(get.handler_id.as_deref(), Some(list.as_str()));
    let place = symbol("express-api", "src/orders.ts", "", "placeOrder");
    assert_eq!(
        find(&d, "POST", "/api/orders").handler_id.as_deref(),
        Some(place.as_str())
    );
    let delete = symbol(
        "express-api",
        "src/routes/orders.ts",
        "",
        "DELETE /orders/:id",
    );
    assert_eq!(
        find(&d, "DELETE", "/api/orders/{id}").handler_id.as_deref(),
        Some(delete.as_str())
    );
    let health = find(&d, "GET", "/health");
    assert_eq!((health.file.as_str(), health.line), ("src/app.ts", 9));
    let inline = symbol("express-api", "src/app.ts", "", "GET /health");
    assert_eq!(health.handler_id.as_deref(), Some(inline.as_str()));
    assert!(d.notes.is_empty(), "{:?}", d.notes);
}

#[test]
fn express_use_runs_on_the_way_in() {
    // https://expressjs.com/en/5x/api/application/ (`app.use` without a path runs for every
    // request); https://expressjs.com/en/guide/routing.html (several callbacks per route)
    let d = derive_tree(&fixture("express-api"));
    let require_user = symbol("express-api", "src/auth.ts", "", "requireUser");
    let validate = symbol("express-api", "src/routes/orders.ts", "", "validate");
    let want: BTreeSet<(String, String)> = [
        ("GET /api/orders", &require_user),
        ("POST /api/orders", &require_user),
        ("DELETE /api/orders/{id}", &require_user),
        ("GET /health", &require_user),
        ("POST /api/orders", &validate),
    ]
    .into_iter()
    .map(|(r, dst)| (r.to_string(), dst.clone()))
    .collect();
    assert_eq!(route_edges(&d, "ts.express.use"), want);
    let edges: Vec<_> = d
        .edges
        .iter()
        .filter(|e| e.rule == "ts.express.use")
        .collect();
    assert!(edges.iter().all(|e| e.kind == "calls"), "{edges:#?}");
    let from_use = edges.iter().find(|e| e.dst_id == require_user).unwrap();
    assert_eq!((from_use.file.as_str(), from_use.line), ("src/app.ts", 7));
    let from_route = edges.iter().find(|e| e.dst_id == validate).unwrap();
    assert_eq!(
        (from_route.file.as_str(), from_route.line),
        ("src/routes/orders.ts", 15)
    );
}

#[test]
fn an_unmounted_router_keeps_its_paths() {
    // https://expressjs.com/en/guide/routing.html: a router's paths are relative to where it
    // is mounted; without the mount it is not reachable from the app.
    let dir = copy_fixture("express-api");
    edit(dir.path(), "src/app.ts", "app.use('/api', orders);\n", "");
    let d = derive_tree(dir.path());
    let want: BTreeSet<_> = [
        shape("GET", "/orders"),
        shape("POST", "/orders"),
        shape("DELETE", "/orders/{id}"),
        shape("GET", "/health"),
    ]
    .into_iter()
    .collect();
    assert_eq!(shapes(&d), want);
    let get = find(&d, "GET", "/orders");
    assert_eq!(get.detail.as_deref(), Some(r#"{"mounted":false}"#));
    assert_eq!(find(&d, "GET", "/health").detail, None);
    // `requireUser` is on the app only; the router is not mounted on it.
    let require_user = symbol_in(dir.path(), "src/auth.ts", "", "requireUser");
    let to_user: BTreeSet<String> = route_edges(&d, "ts.express.use")
        .into_iter()
        .filter(|(_, dst)| *dst == require_user)
        .map(|(r, _)| r)
        .collect();
    assert_eq!(to_user, ["GET /health".to_string()].into_iter().collect());
}

#[test]
fn a_get_on_a_map_is_not_a_route() {
    // Only receivers bound to `express()`, `express.Router()` or `Router` from `express`.
    let dir = tree(&[
        ("package.json", EXPRESS_PACKAGE),
        (
            "src/cache.ts",
            "import $ from 'jquery';
function Router() {
  return new Map();
}
const cache = new Map();
const local = Router();
cache.get('/k', load);
local.get('/x', load);
$.get('/u', load);
function load() {}
",
        ),
    ]);
    let d = derive_tree(dir.path());
    assert!(d.routes.is_empty(), "{:#?}", d.routes);
    assert!(d.edges.is_empty(), "{:#?}", d.edges);
}

#[test]
fn express_5_wildcards_and_optional_parts() {
    // https://expressjs.com/en/guide/routing.html: `/files/*filepath`, `/{*splat}` (also the
    // root), `/order{/:id}` (the braced part is optional; the pattern keeps the parameter).
    let dir = tree(&[
        ("package.json", EXPRESS_PACKAGE),
        (
            "src/app.js",
            "const express = require('express');
const app = express();
app.get('/files/*filepath', files);
app.get('/order{/:id}', order);
app.all('/{*splat}', fallback);
app.get('/legacy/*', legacy);
function files() {}
function order() {}
function fallback() {}
function legacy() {}
",
        ),
    ]);
    let d = derive_tree(dir.path());
    let want: BTreeSet<_> = [
        shape("GET", "/files/{...filepath}"),
        shape("GET", "/order/{id}"),
        shape("ANY", "/{...splat}"),
        shape("GET", "/legacy/{...}"),
    ]
    .into_iter()
    .collect();
    assert_eq!(shapes(&d), want);
    let order = symbol_in(dir.path(), "src/app.js", "", "order");
    assert_eq!(
        find(&d, "GET", "/order/{id}").handler_id.as_deref(),
        Some(order.as_str())
    );
}

#[test]
fn a_router_mounted_twice_gives_both_sets_and_use_paths_filter() {
    // https://expressjs.com/en/5x/api/application/: `app.use('/apple', …)` matches `/apple`
    // and its sub-paths only.
    let dir = tree(&[
        ("package.json", EXPRESS_PACKAGE),
        (
            "src/app.ts",
            "import express from 'express';
import { items } from './items';
import { audit } from './audit';
const app = express();
app.use('/shop', items);
app.use('/admin', audit);
app.use('/admin', items);
",
        ),
        (
            "src/items.ts",
            "import { Router } from 'express';
export const items = Router();
items.get('/items/:id', show);
function show() {}
",
        ),
        ("src/audit.ts", "export function audit() {}\n"),
    ]);
    let d = derive_tree(dir.path());
    let want: BTreeSet<_> = [
        shape("GET", "/shop/items/{id}"),
        shape("GET", "/admin/items/{id}"),
    ]
    .into_iter()
    .collect();
    assert_eq!(shapes(&d), want);
    assert!(d.routes.iter().all(|r| r.detail.is_none()));
    let audit = symbol_in(dir.path(), "src/audit.ts", "", "audit");
    assert_eq!(
        route_edges(&d, "ts.express.use"),
        [("GET /admin/items/{id}".to_string(), audit)]
            .into_iter()
            .collect()
    );
}

#[test]
fn a_mount_cycle_keeps_the_paths_with_a_note() {
    // Two routers mounted on each other are reachable from no app: the paths are kept
    // without a prefix, and the walk ends.
    let dir = tree(&[
        ("package.json", EXPRESS_PACKAGE),
        (
            "src/r.ts",
            "import { Router } from 'express';
const a = Router();
const b = Router();
a.use('/b', b);
b.use('/a', a);
b.get('/x', h);
function h() {}
",
        ),
    ]);
    let d = derive_tree(dir.path());
    assert_eq!(shapes(&d), [shape("GET", "/x")].into_iter().collect());
    assert_eq!(
        find(&d, "GET", "/x").detail.as_deref(),
        Some(r#"{"mounted":false}"#)
    );
    assert_eq!(
        d.notes,
        vec![
            "src/r.ts:3: router `b` is mounted only through a cycle or more than 5 mounts \
             deep; its paths are kept without a prefix"
                .to_string()
        ]
    );
}

#[test]
fn nest_routes_use_the_global_prefix() {
    // https://docs.nestjs.com/controllers, https://docs.nestjs.com/faq/global-prefix
    let d = derive_tree(&fixture("nest-api"));
    let want: BTreeSet<_> = [
        shape("GET", "/api/orders"),
        shape("GET", "/api/orders/{id}"),
        shape("POST", "/api/orders"),
    ]
    .into_iter()
    .collect();
    assert_eq!(shapes(&d), want);
    let controller = "src/orders/orders.controller.ts";
    for (method, pattern, name, line) in [
        ("GET", "/api/orders", "list", 9),
        ("GET", "/api/orders/{id}", "one", 14),
        ("POST", "/api/orders", "place", 19),
    ] {
        let r = find(&d, method, pattern);
        let handler = symbol("nest-api", controller, "OrdersController", name);
        assert_eq!(r.handler_id.as_deref(), Some(handler.as_str()), "{r:?}");
        assert_eq!((r.file.as_str(), r.line), (controller, line));
        assert_eq!(
            (
                r.entry_kind.as_str(),
                r.rule.as_str(),
                r.prov.as_str(),
                r.conf.as_str()
            ),
            ("http", "ts.nest.route", "matched", "sure")
        );
    }
    assert!(d.notes.is_empty(), "{:?}", d.notes);
}

#[test]
fn nest_guard_is_on_the_way_in() {
    // https://docs.nestjs.com/guards (`@UseGuards(RolesGuard)` on a method)
    let d = derive_tree(&fixture("nest-api"));
    let guard = symbol("nest-api", "src/auth.guard.ts", "", "AuthGuard");
    assert_eq!(
        route_edges(&d, "ts.nest.guard"),
        [("POST /api/orders".to_string(), guard)]
            .into_iter()
            .collect()
    );
    let edge = d.edges.iter().find(|e| e.rule == "ts.nest.guard").unwrap();
    assert_eq!(edge.kind, "calls");
    assert_eq!(
        (edge.file.as_str(), edge.line),
        ("src/orders/orders.controller.ts", 20)
    );
}

#[test]
fn nest_class_guards_and_two_global_prefixes() {
    // https://docs.nestjs.com/guards: a guard on the controller covers every handler.
    // https://docs.nestjs.com/controllers: `@Controller({ path })`, `@All()`.
    let dir = tree(&[
        (
            "package.json",
            "{\"dependencies\": {\"@nestjs/common\": \"11.0.0\", \"@nestjs/core\": \"11.0.0\"}}\n",
        ),
        (
            "src/main.ts",
            "import { NestFactory } from '@nestjs/core';
async function a() {
  const app = await NestFactory.create(M);
  app.setGlobalPrefix('v1');
}
async function b() {
  const app = await NestFactory.create(M);
  app.setGlobalPrefix('v2');
}
",
        ),
        (
            "src/cart.controller.ts",
            "import { All, Controller, Delete, UseGuards } from '@nestjs/common';
import { Get } from './not-nest';
class Gate {}
@Controller({ path: 'cart' })
@UseGuards(Gate)
export class CartController {
  @All('*')
  any() {}

  @Delete(':id')
  drop() {}

  @Get('x')
  notARoute() {}
}
",
        ),
        ("src/not-nest.ts", "export function Get(p: string) {}\n"),
    ]);
    let d = derive_tree(dir.path());
    let want: BTreeSet<_> = [shape("ANY", "/cart/{...}"), shape("DELETE", "/cart/{id}")]
        .into_iter()
        .collect();
    assert_eq!(shapes(&d), want);
    let gate = symbol_in(dir.path(), "src/cart.controller.ts", "", "Gate");
    assert_eq!(
        route_edges(&d, "ts.nest.guard"),
        [
            ("ANY /cart/{...}".to_string(), gate.clone()),
            ("DELETE /cart/{id}".to_string(), gate),
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        d.notes,
        vec![
            "src/main.ts:8: a second global prefix `v2` (also `v1` at src/main.ts:4); \
             no global prefix is applied"
                .to_string()
        ]
    );
}

#[test]
fn constructor_injection_links_the_service_method() {
    // https://docs.nestjs.com/providers (constructor-based injection)
    let d = derive_tree(&fixture("nest-api"));
    let controller = "src/orders/orders.controller.ts";
    let list = symbol("nest-api", controller, "OrdersController", "list");
    let find_all = symbol(
        "nest-api",
        "src/orders/orders.service.ts",
        "OrdersService",
        "findAll",
    );
    let di: Vec<_> = d
        .edges
        .iter()
        .filter(|e| e.rule == "ts.di.constructor")
        .map(|e| {
            (
                e.kind.as_str(),
                e.src_id.as_str(),
                e.dst_id.as_str(),
                e.file.as_str(),
                e.line,
            )
        })
        .collect();
    assert_eq!(
        di,
        vec![("calls", list.as_str(), find_all.as_str(), controller, 11)]
    );
}

#[test]
fn an_interface_type_gives_no_injection_edge() {
    let dir = tree(&[(
        "src/a.ts",
        "interface Repo {
  load(): void;
}
class Store {
  load() {}
}
export class A {
  constructor(private repo: Repo, private store: Store) {}
  run() {
    this.repo.load();
    this.store.load();
  }
}
",
    )]);
    let d = derive_tree(dir.path());
    let run = symbol_in(dir.path(), "src/a.ts", "A", "run");
    let load = symbol_in(dir.path(), "src/a.ts", "Store", "load");
    let di: Vec<_> = d
        .edges
        .iter()
        .filter(|e| e.rule == "ts.di.constructor")
        .map(|e| (e.src_id.clone(), e.dst_id.clone(), e.line))
        .collect();
    assert_eq!(di, vec![(run, load, 11)]);
}
