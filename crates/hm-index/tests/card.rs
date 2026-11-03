mod common;

use common::indexed;
use hm_index::query::{
    dependencies, external_imports, file_card, import_counts, manifests, role_label, routes,
    routes_for_file, routes_for_symbol, symbol_card,
};

#[test]
fn service_card_lists_symbols_callers_and_users() {
    let (ix, _dir) = indexed("ts-basic");
    let card = file_card(ix.store(), "src/orders/service.ts")
        .unwrap()
        .expect("the file is indexed");

    assert_eq!(card.path, "src/orders/service.ts");
    assert_eq!(card.lang.as_deref(), Some("typescript"));
    assert_eq!(card.class, "source");
    assert_eq!(card.parse_status, "parsed");
    assert_eq!(card.lines, 21);

    let place = card
        .symbols
        .iter()
        .find(|s| s.name == "placeOrder")
        .expect("placeOrder is listed");
    assert_eq!(place.kind, "function");
    assert!(place.exported);
    assert_eq!(place.called_from, 3); // main, run, the test file's top level
    assert_eq!((place.l0, place.l1), (5, 9));
    // Sorted by position; methods carry their class name.
    let names: Vec<&str> = card.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "placeOrder",
            "OrderService",
            "OrderService.find",
            "OrderService.total",
            "OrderService.sum"
        ]
    );

    assert_eq!(
        card.used_by.first().map(String::as_str),
        Some("src/index.ts")
    );
    assert_eq!(
        card.used_by.last().map(String::as_str),
        Some("src/orders/service.test.ts")
    );
    for want in ["src/db.ts", "src/orders/mail.ts", "zod (external)"] {
        assert!(
            card.uses.iter().any(|u| u == want),
            "{want} in {:?}",
            card.uses
        );
    }
    // Files first, then packages.
    assert_eq!(card.uses.last().map(String::as_str), Some("zod (external)"));
    assert!(card.unresolved >= 1);
}

#[test]
fn symbol_card_shows_callers_and_unresolved_calls() {
    let (ix, _dir) = indexed("ts-basic");
    let card = symbol_card(ix.store(), "src/orders/service.ts", "placeOrder")
        .unwrap()
        .expect("the symbol exists");

    assert_eq!(card.name, "placeOrder");
    assert_eq!(card.kind, "function");
    assert_eq!(card.path, "src/orders/service.ts");
    assert_eq!((card.l0, card.l1), (5, 9));
    assert!(card.sig.contains("placeOrder"));
    assert_eq!(
        card.callers,
        [
            "src/index.ts#main",
            "src/legacy.js#run",
            "src/orders/service.test.ts"
        ]
    );
    assert_eq!(card.callees, ["src/orders/mail.ts#sendReceipt"]);
    assert!(card.unresolved_calls.iter().any(|c| c == "db.insert"));
}

#[test]
fn unknown_files_and_symbols_are_none() {
    let (ix, _dir) = indexed("ts-basic");
    assert!(file_card(ix.store(), "src/nope.ts").unwrap().is_none());
    assert!(
        symbol_card(ix.store(), "src/orders/service.ts", "nope")
            .unwrap()
            .is_none()
    );
    assert!(
        symbol_card(ix.store(), "src/nope.ts", "placeOrder")
            .unwrap()
            .is_none()
    );
    let method = symbol_card(ix.store(), "src/orders/service.ts", "OrderService.total")
        .unwrap()
        .expect("a method is found by Owner.name");
    assert_eq!(method.name, "OrderService.total");
    assert_eq!(method.callees, ["src/orders/service.ts#OrderService.sum"]);
}

#[test]
fn file_card_lists_routes() {
    let (ix, _dir) = indexed("client-hops");
    let card = file_card(ix.store(), "app/api/orders/route.ts")
        .unwrap()
        .expect("the file is indexed");
    let labels: Vec<&str> = card.routes.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["API · GET /api/orders", "API · POST /api/orders"]);
    assert_eq!(
        card.routes[0].handler.as_deref(),
        Some("app/api/orders/route.ts#GET")
    );
    assert_eq!(card.routes[0].file, "app/api/orders/route.ts");

    // A file that declares no route has none.
    let plain = file_card(ix.store(), "lib/api.ts").unwrap().unwrap();
    assert!(plain.routes.is_empty());
}

#[test]
fn symbol_card_shows_its_role() {
    let (ix, _dir) = indexed("client-hops");
    let post = symbol_card(ix.store(), "app/api/orders/route.ts", "POST")
        .unwrap()
        .expect("the handler exists");
    assert_eq!(post.roles, ["API · POST /api/orders"]);
    // Each handler has its own role.
    let get = symbol_card(ix.store(), "app/api/orders/route.ts", "GET")
        .unwrap()
        .unwrap();
    assert_eq!(get.roles, ["API · GET /api/orders"]);
}

#[test]
fn routes_are_sorted_and_found_by_file_and_symbol() {
    let (ix, _dir) = indexed("client-hops");
    let all = routes(ix.store()).unwrap();
    let keys: Vec<(&str, &str, &str, &str)> = all
        .iter()
        .map(|r| {
            (
                r.root.as_str(),
                r.entry_kind.as_str(),
                r.pattern.as_str(),
                r.method.as_str(),
            )
        })
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert!(all.len() >= 4, "{keys:?}");
    let post = all
        .iter()
        .find(|r| r.method == "POST" && r.pattern == "/api/orders")
        .expect("POST /api/orders");
    assert_eq!(post.entry_kind, "http");
    assert_eq!(post.rule, "ts.next.app.route");
    assert!(
        post.handler_id
            .as_deref()
            .is_some_and(|h| h.starts_with("y_"))
    );

    let in_file = routes_for_file(
        ix.store(),
        &hm_core::ids::file_id("app/api/orders/route.ts"),
    )
    .unwrap();
    assert_eq!(in_file.len(), 2);
    let of_symbol = routes_for_symbol(ix.store(), post.handler_id.as_deref().unwrap()).unwrap();
    assert_eq!(of_symbol.len(), 1);
    assert_eq!(of_symbol[0].id, post.id);
    assert!(
        routes_for_symbol(ix.store(), "y_nothing")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn role_labels_read_plainly() {
    assert_eq!(
        role_label("http", "POST", "/api/auth/login"),
        "API · POST /api/auth/login"
    );
    assert_eq!(
        role_label("page", "GET", "/orders/{id}"),
        "Page · /orders/{id}"
    );
    assert_eq!(
        role_label("server_action", "ACTION", "placeOrder"),
        "Server action · placeOrder"
    );
    assert_eq!(
        role_label("middleware", "ANY", "/account/:path*"),
        "Runs before /account/:path*"
    );
    assert_eq!(
        role_label("middleware", "ANY", "/*"),
        "Runs before many routes"
    );
    assert_eq!(role_label("cli", "BIN", "shop"), "Command · shop");
    assert_eq!(
        role_label("main", "START", "start"),
        "Start script · npm start"
    );
    assert_eq!(role_label("main", "MAIN", "index.js"), "Entry · index.js");
}

#[test]
fn a_middleware_that_is_not_evaluated_runs_before_many_routes() {
    let dir = common::copy_fixture("client-hops");
    std::fs::write(
        dir.path().join("middleware.ts"),
        "export function middleware() {}\nexport const config = { matcher: ['/((?!api|_next).*)'] }\n",
    )
    .unwrap();
    let store = hm_store::Store::open_in_memory().unwrap();
    let mut ix = hm_index::index::Indexer::open(dir.path(), store).unwrap();
    ix.full().unwrap();
    let found = routes(ix.store()).unwrap();
    let mw = found
        .iter()
        .find(|r| r.entry_kind == "middleware")
        .expect("a middleware route");
    assert_eq!(mw.label, "Runs before many routes");
    assert!(
        mw.detail
            .as_deref()
            .is_some_and(|d| d.contains("\"evaluated\":false"))
    );
}

#[test]
fn manifest_and_import_queries_answer_per_root() {
    let (ix, _dir) = indexed("npm-locks/pnpm");
    let found = manifests(ix.store()).unwrap();
    let paths: Vec<&str> = found.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(paths, ["apps/web/package.json", "package.json"]);
    let web = &found[0];
    assert_eq!(web.root, "apps/web");
    assert_eq!(web.name.as_deref(), Some("@shop/web"));
    assert_eq!(
        web.bin,
        [("shop".to_string(), "./bin/shop.js".to_string(), 4)]
    );
    assert_eq!(web.main, Some(("index.js".to_string(), 6)));
    assert_eq!(web.scripts.len(), 2);
    assert!(web.lockfile.is_some());

    let deps = dependencies(ix.store(), Some("apps/web")).unwrap();
    let next = deps
        .iter()
        .find(|d| d.name == "next")
        .expect("next is declared");
    assert_eq!(next.resolved.as_deref(), Some("15.2.1"));
    assert_eq!(next.declared, "^15.0.0");
    assert_eq!(next.manifest, "apps/web/package.json");
    assert!(!next.dev);
    assert!(deps.iter().all(|d| d.root == "apps/web"));
    let everything = dependencies(ix.store(), None).unwrap();
    assert!(everything.len() > deps.len());

    let (hops, _dir) = indexed("client-hops");
    let counts = import_counts(hops.store(), "").unwrap();
    assert_eq!(counts.get("axios"), Some(&1), "{counts:?}");
    assert_eq!(import_counts(hops.store(), "apps/web").unwrap().len(), 0);
    let imports = external_imports(hops.store()).unwrap();
    assert!(
        imports
            .iter()
            .any(|i| i.package == "axios" && i.root.is_empty())
    );
}

#[test]
fn imports_are_counted_for_the_nearest_manifest_root() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, text: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    for root in ["apps/web", "apps/webshop", "apps/web/packages/ui"] {
        write(
            &format!("{root}/package.json"),
            r#"{"name":"x","dependencies":{"zod":"3.0.0"}}"#,
        );
    }
    let uses_zod = "import { z } from 'zod';\nexport const a = z.string();\n";
    write("apps/web/a.ts", uses_zod);
    write("apps/web/b.ts", uses_zod);
    write("apps/webshop/a.ts", uses_zod);
    write("apps/web/packages/ui/a.ts", uses_zod);
    write("apps/web/packages/ui/b.ts", uses_zod);
    write("apps/web/packages/ui/c.ts", uses_zod);
    write("loose.ts", uses_zod);

    let ix = common::fresh_index_of(dir.path());
    let counts = |root: &str| import_counts(ix.store(), root).unwrap().get("zod").copied();
    // `apps/web` is not a prefix of `apps/webshop`, and the nested root keeps its own.
    assert_eq!(counts("apps/web"), Some(2));
    assert_eq!(counts("apps/webshop"), Some(1));
    assert_eq!(counts("apps/web/packages/ui"), Some(3));
    assert_eq!(counts(""), Some(1)); // the file no manifest covers
    assert_eq!(counts("apps"), None);

    let roots: Vec<(String, String)> = external_imports(ix.store())
        .unwrap()
        .into_iter()
        .map(|i| (i.file, i.root))
        .collect();
    let root_of = |file: &str| roots.iter().find(|(f, _)| f == file).unwrap().1.clone();
    assert_eq!(root_of("apps/web/a.ts"), "apps/web");
    assert_eq!(root_of("apps/webshop/a.ts"), "apps/webshop");
    assert_eq!(root_of("apps/web/packages/ui/b.ts"), "apps/web/packages/ui");
    assert_eq!(root_of("loose.ts"), "");
}

#[test]
fn a_route_is_found_from_the_file_of_its_handler() {
    let (ix, _dir) = indexed("express-api");
    let handler_file = hm_core::ids::file_id("src/orders.ts");
    let found = routes_for_file(ix.store(), &handler_file).unwrap();
    let list = found
        .iter()
        .find(|r| r.method == "GET" && r.pattern == "/api/orders")
        .unwrap_or_else(|| panic!("GET /api/orders in {found:?}"));
    // Declared in one file, handled in another.
    assert_eq!(list.file, "src/routes/orders.ts");
    assert_eq!(list.handler.as_deref(), Some("src/orders.ts#listOrders"));
    let card = file_card(ix.store(), "src/orders.ts").unwrap().unwrap();
    assert!(card.routes.iter().any(|r| r.id == list.id));
}

#[test]
fn a_bin_script_shows_its_command() {
    let dir = common::copy_fixture("client-hops");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"shop","bin":{"shop":"./cli.js"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("cli.js"), "console.log('shop');\n").unwrap();
    let ix = common::fresh_index_of(dir.path());
    let card = file_card(ix.store(), "cli.js").unwrap().unwrap();
    let labels: Vec<&str> = card.routes.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["Command · shop"]);
    assert_eq!(card.routes[0].handler.as_deref(), Some("cli.js"));
    assert_eq!(card.routes[0].file, "package.json");
}
