//! Next.js routes, server actions, middleware and `package.json` entries, derived from the
//! facts of a whole tree (Learn spec §6.3).
//!
//! Rules checked against the official docs while building:
//! - App router: a route is public when a `page` (`.js .jsx .tsx`, and `.ts` through the
//!   default `pageExtensions`) or `route` (`.js .ts`) file exists in its folder; `(group)`
//!   folders leave the URL, `_private` folders opt their whole subtree out, `@slot` is a
//!   named slot and `(.)x`, `(..)x`, `(...)x` intercept another route.
//!   https://nextjs.org/docs/app/getting-started/project-structure
//! - `route.js` exports one handler per verb: `GET POST PUT PATCH DELETE HEAD OPTIONS`.
//!   https://nextjs.org/docs/app/api-reference/file-conventions/route
//! - `[id]` is one segment, `[...slug]` a catch-all, `[[...slug]]` an optional catch-all.
//!   https://nextjs.org/docs/app/api-reference/file-conventions/dynamic-routes
//! - Route groups are omitted from the URL.
//!   https://nextjs.org/docs/app/api-reference/file-conventions/route-groups
//! - `src/app` and `src/pages` are used when `app` and `pages` are not at the root; the
//!   proxy then sits in `src` too.
//!   https://nextjs.org/docs/app/api-reference/file-conventions/src-folder
//! - Every file under `pages/api` is an API endpoint that takes every method.
//!   https://nextjs.org/docs/pages/building-your-application/routing/api-routes
//! - `proxy.ts` (formerly `middleware.ts`) at the root or in `src` exports one function,
//!   named `proxy` (or `middleware`) or as the default export; matcher sources are anchored
//!   to the start of the path (`/about` matches `/about/team`), `:p*` is zero or more
//!   segments, and a regex in parentheses is allowed.
//!   https://nextjs.org/docs/app/api-reference/file-conventions/proxy

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{copy_fixture, fixture, read_fixture};
use hm_index::lang::Lang;
use hm_index::routes::{Derived, Route, derive_tree};
use hm_index::ts::resolve::symbol_ids;

/// `(entry_kind, method, pattern)` of every route whose file is under `prefix`.
fn shapes(d: &Derived, prefix: &str) -> BTreeSet<(String, String, String)> {
    d.routes
        .iter()
        .filter(|r| r.file.starts_with(prefix))
        .map(|r| (r.entry_kind.clone(), r.method.clone(), r.pattern.clone()))
        .collect()
}

fn shape(kind: &str, method: &str, pattern: &str) -> (String, String, String) {
    (kind.to_string(), method.to_string(), pattern.to_string())
}

fn find<'a>(d: &'a Derived, kind: &str, method: &str, pattern: &str) -> &'a Route {
    d.routes
        .iter()
        .find(|r| r.entry_kind == kind && r.method == method && r.pattern == pattern)
        .unwrap_or_else(|| panic!("no {kind} {method} {pattern} in {:#?}", d.routes))
}

/// The id of the top-level symbol `name` of fixture file `fixture_name/rel`.
fn symbol(fixture_name: &str, rel: &str, name: &str) -> String {
    let lang = Lang::from_path(std::path::Path::new(rel)).expect("a source file");
    let facts = hm_index::ts::extract(&read_fixture(&format!("{fixture_name}/{rel}")), lang);
    let at = facts
        .symbols
        .iter()
        .position(|s| s.owner.is_empty() && s.name == name)
        .unwrap_or_else(|| panic!("no symbol {name} in {rel}"));
    symbol_ids(rel, &facts).swap_remove(at)
}

#[test]
fn app_router_pages_and_verbs() {
    // https://nextjs.org/docs/app/getting-started/project-structure
    // https://nextjs.org/docs/app/api-reference/file-conventions/route
    // https://nextjs.org/docs/app/api-reference/file-conventions/dynamic-routes
    // https://nextjs.org/docs/app/api-reference/file-conventions/route-groups
    let d = derive_tree(&fixture("next-app"));
    let want: BTreeSet<_> = [
        shape("page", "GET", "/"),
        shape("page", "GET", "/orders"),
        shape("page", "GET", "/orders/{id}"),
        shape("page", "GET", "/docs/{...slug}"),
        shape("page", "GET", "/help/{...topic?}"),
        shape("page", "GET", "/account"),
        shape("http", "GET", "/api/orders"),
        shape("http", "POST", "/api/orders"),
        shape("http", "DELETE", "/api/orders/{id}"),
    ]
    .into_iter()
    .collect();
    let got: BTreeSet<_> = shapes(&d, "app/")
        .into_iter()
        .filter(|(kind, _, _)| kind == "page" || kind == "http")
        .collect();
    assert_eq!(got, want);
    assert!(
        d.routes
            .iter()
            .all(|r| !r.file.contains("_components") && !r.file.contains("(.)")),
        "{:#?}",
        d.routes
    );

    let delete = find(&d, "http", "DELETE", "/api/orders/{id}");
    assert_eq!(
        delete.handler_id.as_deref(),
        Some(symbol("next-app", "app/api/orders/[id]/route.ts", "remove").as_str())
    );
    assert_eq!(delete.rule, "ts.next.app.route");
    assert_eq!(
        (delete.prov.as_str(), delete.conf.as_str()),
        ("matched", "sure")
    );
    assert_eq!(delete.root, "");

    let post = find(&d, "http", "POST", "/api/orders");
    assert_eq!(
        post.handler_id.as_deref(),
        Some(symbol("next-app", "app/api/orders/route.ts", "POST").as_str())
    );
    assert_eq!(post.line, 5);

    let order = find(&d, "page", "GET", "/orders/{id}");
    assert_eq!(order.rule, "ts.next.app.page");
    assert_eq!(order.file, "app/(shop)/orders/[id]/page.tsx");
    assert_eq!(
        order.handler_id.as_deref(),
        Some(symbol("next-app", "app/(shop)/orders/[id]/page.tsx", "OrderPage").as_str())
    );
    assert_eq!(
        order.id,
        hm_core::ids::route_id("", "page", "GET", "/orders/{id}")
    );
}

#[test]
fn pages_router_under_src() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/src-folder
    // https://nextjs.org/docs/pages/building-your-application/routing/api-routes
    let d = derive_tree(&fixture("next-pages"));
    let want: BTreeSet<_> = [
        shape("page", "GET", "/"),
        shape("page", "GET", "/orders/{id}"),
        shape("http", "ANY", "/api/orders"),
    ]
    .into_iter()
    .collect();
    assert_eq!(shapes(&d, "src/pages/"), want);
    assert!(d.routes.iter().all(|r| !r.file.ends_with("_app.tsx")));

    let api = find(&d, "http", "ANY", "/api/orders");
    assert_eq!(api.rule, "ts.next.pages.api");
    assert_eq!(
        api.handler_id.as_deref(),
        Some(symbol("next-pages", "src/pages/api/orders.ts", "handler").as_str())
    );
    assert_eq!(find(&d, "page", "GET", "/").rule, "ts.next.pages.page");
}

#[test]
fn server_actions_from_directives() {
    // https://nextjs.org/docs/app/api-reference/directives/use-server
    let d = derive_tree(&fixture("next-app"));
    let actions: BTreeSet<(String, String, String)> = d
        .routes
        .iter()
        .filter(|r| r.entry_kind == "server_action")
        .map(|r| (r.method.clone(), r.pattern.clone(), r.rule.clone()))
        .collect();
    let want: BTreeSet<_> = [
        (
            "ACTION",
            "app/lib/actions.ts#placeOrder",
            "ts.next.action.file",
        ),
        (
            "ACTION",
            "app/lib/actions.ts#cancelOrder",
            "ts.next.action.file",
        ),
        (
            "ACTION",
            "app/ui/inline.tsx#Inline›save",
            "ts.next.action.fn",
        ),
    ]
    .into_iter()
    .map(|(m, p, r)| (m.to_string(), p.to_string(), r.to_string()))
    .collect();
    assert_eq!(actions, want);

    let place = find(
        &d,
        "server_action",
        "ACTION",
        "app/lib/actions.ts#placeOrder",
    );
    assert_eq!(
        place.handler_id.as_deref(),
        Some(symbol("next-app", "app/lib/actions.ts", "placeOrder").as_str())
    );
    let save = find(
        &d,
        "server_action",
        "ACTION",
        "app/ui/inline.tsx#Inline›save",
    );
    assert_eq!(
        save.handler_id.as_deref(),
        Some(symbol("next-app", "app/ui/inline.tsx", "Inline").as_str())
    );
    assert_eq!(save.line, 3);
}

/// `(source pattern, rule)` of every `ts.next.middleware` edge, by the route it starts at.
fn middleware_sources(d: &Derived) -> BTreeSet<String> {
    d.edges
        .iter()
        .filter(|e| e.rule == "ts.next.middleware")
        .map(|e| {
            let route = d
                .routes
                .iter()
                .find(|r| r.id == e.src_id)
                .unwrap_or_else(|| panic!("edge from an unknown route: {e:?}"));
            format!("{} {}", route.method, route.pattern)
        })
        .collect()
}

#[test]
fn middleware_matcher_reaches_matching_routes() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/proxy
    let d = derive_tree(&fixture("next-app"));
    let mw = find(&d, "middleware", "ANY", "/account/:path*");
    assert_eq!(mw.file, "middleware.ts");
    assert_eq!(mw.rule, "ts.next.middleware");
    assert_eq!(mw.conf, "sure");
    let detail: serde_json::Value =
        serde_json::from_str(mw.detail.as_deref().expect("a detail")).expect("JSON detail");
    assert_eq!(
        detail,
        serde_json::json!({"matcher": ["/account/:path*", "/checkout"], "evaluated": true})
    );
    let handler = symbol("next-app", "middleware.ts", "middleware");
    assert_eq!(mw.handler_id.as_deref(), Some(handler.as_str()));

    assert_eq!(
        middleware_sources(&d),
        ["GET /account".to_string()].into_iter().collect()
    );
    let edge = d
        .edges
        .iter()
        .find(|e| e.rule == "ts.next.middleware")
        .expect("one edge");
    assert_eq!(edge.kind, "calls");
    assert_eq!(edge.dst_id, handler);
    assert_eq!((edge.file.as_str(), edge.line), ("middleware.ts", mw.line));
}

#[test]
fn an_unevaluated_matcher_applies_to_every_route() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/proxy (negative matching)
    let dir = copy_fixture("next-app");
    let path = dir.path().join("middleware.ts");
    let src = fs::read_to_string(&path).expect("read middleware.ts");
    let src = src.replace("['/account/:path*', '/checkout']", "['/((?!api|_next).*)']");
    fs::write(&path, src).expect("write middleware.ts");

    let d = derive_tree(dir.path());
    let mw = find(&d, "middleware", "ANY", "/((?!api|_next).*)");
    assert_eq!(mw.conf, "probably");
    let detail: serde_json::Value =
        serde_json::from_str(mw.detail.as_deref().expect("a detail")).expect("JSON detail");
    assert_eq!(detail["evaluated"], serde_json::json!(false));

    let every: BTreeSet<String> = d
        .routes
        .iter()
        .filter(|r| r.entry_kind == "page" || r.entry_kind == "http")
        .map(|r| format!("{} {}", r.method, r.pattern))
        .collect();
    assert_eq!(every.len(), 9);
    assert_eq!(middleware_sources(&d), every);
}

#[test]
fn a_pages_folder_without_next_is_not_a_route_tree() {
    // A `pages/` folder only routes under a `package.json` that declares `next`.
    let d = derive_tree(&fixture("next-mono"));
    assert!(
        d.routes
            .iter()
            .all(|r| !r.file.starts_with("packages/ui/") || r.rule.starts_with("npm.")),
        "{:#?}",
        d.routes
    );
}

#[test]
fn two_next_apps_keep_their_own_routes() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/route
    let d = derive_tree(&fixture("next-mono"));
    let rows: Vec<&Route> = d
        .routes
        .iter()
        .filter(|r| r.rule.starts_with("ts.next."))
        .collect();
    let got: BTreeSet<(String, String, String)> = rows
        .iter()
        .map(|r| (r.root.clone(), r.method.clone(), r.pattern.clone()))
        .collect();
    let want: BTreeSet<_> = [
        ("apps/admin", "GET", "/"),
        ("apps/admin", "GET", "/api/health"),
        ("apps/shop", "GET", "/"),
        ("apps/shop", "GET", "/api/health"),
    ]
    .into_iter()
    .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
    .collect();
    assert_eq!(got, want);
    let ids: BTreeSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), 4);
    assert!(d.notes.is_empty(), "{:?}", d.notes);
}

#[test]
fn start_script_is_an_entry() {
    // https://docs.npmjs.com/cli/v11/configuring-npm/package-json
    let d = derive_tree(&fixture("next-app"));
    let start = find(&d, "main", "START", "start");
    assert_eq!(start.rule, "npm.start");
    assert_eq!(
        (start.prov.as_str(), start.conf.as_str()),
        ("config", "sure")
    );
    assert_eq!(start.file, "package.json");
    assert_eq!(start.line, 4);
    assert_eq!(start.handler_id, None);
    let detail: serde_json::Value =
        serde_json::from_str(start.detail.as_deref().expect("a detail")).expect("JSON detail");
    assert_eq!(detail, serde_json::json!({"command": "next start"}));
}

#[test]
fn derive_output_is_sorted_and_stable() {
    for name in ["next-app", "next-pages", "next-mono"] {
        let a = derive_tree(&fixture(name));
        let b = derive_tree(&fixture(name));
        assert_eq!(a, b, "{name}");
        assert!(a.routes.is_sorted(), "{name}");
        assert!(a.client_calls.is_sorted(), "{name}");
        assert!(a.edges.is_sorted(), "{name}");
        assert!(a.patterns.is_sorted(), "{name}");
        assert!(a.gaps.is_sorted(), "{name}");
        assert!(a.notes.is_sorted(), "{name}");
    }
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

#[test]
fn bin_and_main_are_entries() {
    // https://docs.npmjs.com/cli/v11/configuring-npm/package-json (`bin`, `main`)
    let dir = tree(&[
        (
            "tools/package.json",
            "{\n  \"name\": \"shop-tools\",\n  \"main\": \"./lib/index.js\",\n  \"bin\": {\n    \"shop\": \"./cli/shop.js\",\n    \"shop-admin\": \"./bin/admin.js\"\n  },\n  \"scripts\": {\n    \"start\": \"NODE_ENV=production node ./cli/shop.js --port 3000\"\n  }\n}\n",
        ),
        ("tools/cli/shop.js", "console.log('shop')\n"),
        ("tools/lib/index.js", "module.exports = {}\n"),
        ("tools/bin/admin.js", "console.log('admin')\n"),
    ]);
    let d = derive_tree(dir.path());
    let shop = find(&d, "cli", "BIN", "shop");
    assert_eq!(shop.rule, "npm.bin");
    assert_eq!(shop.root, "tools");
    assert_eq!((shop.file.as_str(), shop.line), ("tools/package.json", 5));
    let cli = hm_core::ids::file_id("tools/cli/shop.js");
    assert_eq!(shop.handler_id.as_deref(), Some(cli.as_str()));
    // The walker never enters `bin/`, so that target is not a known file.
    assert_eq!(find(&d, "cli", "BIN", "shop-admin").handler_id, None);

    let main = find(&d, "main", "MAIN", "./lib/index.js");
    assert_eq!(main.rule, "npm.main");
    assert_eq!(main.line, 3);
    assert_eq!(
        main.handler_id.as_deref(),
        Some(hm_core::ids::file_id("tools/lib/index.js").as_str())
    );
    let start = find(&d, "main", "START", "start");
    assert_eq!(start.handler_id.as_deref(), Some(cli.as_str()));
    assert!(
        d.routes
            .iter()
            .all(|r| r.prov == "config" && r.conf == "sure")
    );
}

#[test]
fn a_route_declared_twice_is_dropped_with_a_note() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/route-groups (conflicting
    // paths): two groups that give the same URL keep one row.
    let page = "export default function Page() {\n  return null\n}\n";
    let dir = tree(&[
        (
            "package.json",
            "{\"dependencies\": {\"next\": \"15.2.0\"}}\n",
        ),
        ("app/(marketing)/about/page.tsx", page),
        ("app/(shop)/about/page.tsx", page),
    ]);
    let d = derive_tree(dir.path());
    let about: Vec<&Route> = d.routes.iter().filter(|r| r.pattern == "/about").collect();
    assert_eq!(about.len(), 1, "{about:#?}");
    assert_eq!(about[0].file, "app/(marketing)/about/page.tsx");
    assert_eq!(
        d.notes,
        vec![
            "app/(shop)/about/page.tsx:1: GET /about is also declared at \
             app/(marketing)/about/page.tsx:1"
                .to_string()
        ]
    );
}

#[test]
fn a_proxy_under_src_with_an_expression_default_is_the_file() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/proxy: the function may be
    // the default export; https://nextjs.org/docs/app/api-reference/file-conventions/src-folder:
    // with `src/app`, the proxy sits in `src`.
    let dir = tree(&[
        (
            "package.json",
            "{\"dependencies\": {\"next\": \"16.0.0\"}}\n",
        ),
        (
            "src/proxy.ts",
            "import NextAuth from 'next-auth'\nimport { authConfig } from './auth.config'\n\nexport default NextAuth(authConfig).auth\n\nexport const config = { matcher: '/dashboard/:path+' }\n",
        ),
        ("src/auth.config.ts", "export const authConfig = {}\n"),
        (
            "src/app/dashboard/page.tsx",
            "export default function Page() {\n  return null\n}\n",
        ),
        (
            "src/app/dashboard/[id]/page.tsx",
            "export default function Page() {\n  return null\n}\n",
        ),
    ]);
    let d = derive_tree(dir.path());
    let mw = find(&d, "middleware", "ANY", "/dashboard/:path+");
    assert_eq!(mw.file, "src/proxy.ts");
    let file = hm_core::ids::file_id("src/proxy.ts");
    assert_eq!(mw.handler_id.as_deref(), Some(file.as_str()));
    // `:path+` needs one more segment: `/dashboard` itself is not covered.
    assert_eq!(
        middleware_sources(&d),
        ["GET /dashboard/{id}".to_string()].into_iter().collect()
    );
}

#[test]
fn a_middleware_without_matcher_runs_for_every_route() {
    // https://nextjs.org/docs/app/api-reference/file-conventions/proxy: "Without a
    // `matcher`, Proxy runs on every request".
    let dir = tree(&[
        (
            "package.json",
            "{\"dependencies\": {\"next\": \"16.0.0\"}}\n",
        ),
        (
            "proxy.js",
            "export function proxy(request) {\n  return undefined\n}\n",
        ),
        (
            "app/page.jsx",
            "export default function Page() {\n  return null\n}\n",
        ),
        ("app/api/ping/route.js", "export function GET() {}\n"),
    ]);
    let d = derive_tree(dir.path());
    let mw = find(&d, "middleware", "ANY", "/*");
    assert_eq!(mw.conf, "sure");
    let detail: serde_json::Value =
        serde_json::from_str(mw.detail.as_deref().expect("a detail")).expect("JSON detail");
    assert_eq!(
        detail,
        serde_json::json!({"matcher": [], "evaluated": true})
    );
    assert_eq!(
        middleware_sources(&d),
        ["GET /", "GET /api/ping"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    );
}
