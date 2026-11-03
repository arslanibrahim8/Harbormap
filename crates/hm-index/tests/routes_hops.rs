//! Client calls matched to routes, and the edges through data hooks and server-action
//! forms (Learn spec §6.3), derived from the `client-hops` and `next-app` fixtures and from
//! small temporary projects.
//!
//! Framework shapes checked against the official docs while building:
//! - `useMutation({ mutationFn, onSuccess, onError, onSettled })`:
//!   https://tanstack.com/query/latest/docs/framework/react/reference/useMutation
//! - `useQuery({ queryKey, queryFn })`:
//!   https://tanstack.com/query/latest/docs/framework/react/reference/useQuery
//! - `useSWR(key, fetcher)`: https://swr.vercel.app/docs/api
//! - `<form action={fn}>`, `formAction`, and `useActionState(fn, initial)` returning
//!   `[state, formAction]`: https://react.dev/reference/react/useActionState
//! - Server functions called from forms: https://react.dev/reference/rsc/use-server
//! - A `<form>` without `action` submits to the URL of its own page, and `formAction` /
//!   `formMethod` on a button or input override the form's own:
//!   https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/form

mod common;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use common::{copy_fixture, fixture};
use hm_index::lang::Lang;
use hm_index::query::display_name;
use hm_index::routes::{Derived, derive_tree};
use hm_index::ts::resolve::symbol_ids;

/// Readable names for every file id, symbol id and route id of `dir` and `d`: `path`,
/// `path#Owner.name` and `METHOD pattern`.
fn labels(dir: &Path, d: &Derived) -> HashMap<String, String> {
    let walked = hm_index::walk::walk(dir).expect("walk");
    let mut names = HashMap::new();
    for f in &walked.files {
        names.insert(hm_core::ids::file_id(&f.rel), f.rel.clone());
        let Some(lang) = f.lang.filter(Lang::parsed_in_l1) else {
            continue;
        };
        let facts = hm_index::ts::extract(&fs::read_to_string(&f.abs).expect("read"), lang);
        for (id, sym) in symbol_ids(&f.rel, &facts).into_iter().zip(&facts.symbols) {
            names.insert(id, display_name(&f.rel, Some(sym)));
        }
    }
    for r in &d.routes {
        names.insert(r.id.clone(), format!("{} {}", r.method, r.pattern));
    }
    names
}

/// `(source, destination, rule)` of every edge of `kind`, by name.
fn edges(
    d: &Derived,
    names: &HashMap<String, String>,
    kind: &str,
) -> BTreeSet<(String, String, String)> {
    let name = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));
    d.edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| (name(&e.src_id), name(&e.dst_id), e.rule.clone()))
        .collect()
}

fn edge(src: &str, dst: &str, rule: &str) -> (String, String, String) {
    (src.to_string(), dst.to_string(), rule.to_string())
}

/// The destinations (with rules) of the `http_calls` edges whose source is `src`.
fn calls_from(
    d: &Derived,
    names: &HashMap<String, String>,
    src: &str,
) -> BTreeSet<(String, String)> {
    edges(d, names, "http_calls")
        .into_iter()
        .filter(|(s, _, _)| s == src)
        .map(|(_, dst, rule)| (dst, rule))
        .collect()
}

fn pair(dst: &str, rule: &str) -> (String, String) {
    (dst.to_string(), rule.to_string())
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).expect("create dirs");
    fs::write(path, text).expect("write");
}

/// A temporary Next.js app with the given files.
fn app(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "package.json",
        r#"{ "name": "t", "dependencies": { "next": "15.2.0" } }"#,
    );
    for (rel, text) in files {
        write(dir.path(), rel, text);
    }
    dir
}

#[test]
fn fetch_axios_and_helpers_match_routes() {
    let dir = fixture("client-hops");
    let d = derive_tree(&dir);
    let names = labels(&dir, &d);
    let got = edges(&d, &names, "http_calls");
    let want: BTreeSet<_> = [
        edge(
            "lib/api.ts#loadOrders",
            "GET /api/orders",
            "ts.client.fetch",
        ),
        edge(
            "lib/api.ts#removeOrder",
            "DELETE /api/orders/{id}",
            "ts.client.fetch",
        ),
        edge(
            "lib/api.ts#createOrder",
            "POST /api/orders",
            "ts.client.axios",
        ),
        edge(
            "lib/generated.ts#OrdersService.list",
            "GET /api/orders",
            "ts.client.request_helper",
        ),
        edge(
            "ui/orders.tsx#Orders",
            "GET /api/orders/{id}",
            "ts.client.swr",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(got, want);
    for e in d.edges.iter().filter(|e| e.kind == "http_calls") {
        assert!(!e.file.is_empty() && e.line > 0, "{e:?}");
    }
}

#[test]
fn variable_built_and_ambiguous_urls_never_match() {
    let dir = fixture("client-hops");
    let d = derive_tree(&dir);
    let names = labels(&dir, &d);
    let sources: BTreeSet<_> = edges(&d, &names, "http_calls")
        .into_iter()
        .map(|(src, _, _)| src)
        .collect();
    assert!(!sources.contains("lib/api.ts#elsewhere"), "{sources:?}");
    assert!(!sources.contains("lib/api.ts#fromVar"), "{sources:?}");

    // Two routes that match the SWR call equally: no edge. `DELETE` has one candidate.
    let copy = copy_fixture("client-hops");
    write(
        copy.path(),
        "app/api/orders/[slug]/route.ts",
        "export async function GET() {\n  return Response.json({})\n}\n",
    );
    // Documented behaviour: a literal `https://host` is dropped, so an absolute URL matches
    // like its path.
    write(
        copy.path(),
        "lib/pay.ts",
        "export function pay() { return fetch('https://pay.example/api/orders') }\n",
    );
    let d = derive_tree(copy.path());
    let names = labels(copy.path(), &d);
    assert_eq!(
        calls_from(&d, &names, "lib/pay.ts#pay"),
        BTreeSet::from([pair("GET /api/orders", "ts.client.fetch")])
    );
    assert!(calls_from(&d, &names, "ui/orders.tsx#Orders").is_empty());
    assert_eq!(
        calls_from(&d, &names, "lib/api.ts#removeOrder"),
        BTreeSet::from([pair("DELETE /api/orders/{id}", "ts.client.fetch")])
    );
    // The same call still has a stored row.
    assert!(
        d.client_calls
            .iter()
            .any(|c| c.url == "/api/orders/7" && c.method == "GET")
    );
}

// A literal `http(s)://host` is dropped, so an absolute URL matches like its path; this is
// documented behaviour (the host is not checked). Anything that is not a path from the root
// never matches.
#[test]
fn absolute_relative_and_protocol_relative_urls() {
    let dir = app(&[
        (
            "app/api/orders/route.ts",
            "export async function GET() { return Response.json([]) }\n",
        ),
        (
            "lib/calls.ts",
            "export function absolute() { return fetch('https://pay.example/api/orders') }\n\
             export function upper() { return fetch('HTTPS://pay.example/api/orders') }\n\
             export function relative() { return fetch('api/orders') }\n\
             export function protocolRelative() { return fetch('//cdn.host/api/orders') }\n\
             export function protocolRelativeLocal() { return fetch('//api/orders') }\n",
        ),
    ]);
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    let source = |name: &str| format!("lib/calls.ts#{name}");
    for name in ["absolute", "upper"] {
        assert_eq!(
            calls_from(&d, &names, &source(name)),
            BTreeSet::from([pair("GET /api/orders", "ts.client.fetch")]),
            "{name}"
        );
    }
    for name in ["relative", "protocolRelative", "protocolRelativeLocal"] {
        assert!(calls_from(&d, &names, &source(name)).is_empty(), "{name}");
    }
    // All five calls are stored.
    assert_eq!(d.client_calls.len(), 5, "{:#?}", d.client_calls);
}

#[test]
fn client_urls_keep_what_never_matches_distinct() {
    use hm_index::ts::web::normalize_client_url as n;
    assert_eq!(n("HTTPS://pay.example/api/orders"), "/api/orders");
    assert_eq!(n("Http://pay.example"), "/");
    assert_eq!(n("//cdn.host/x"), "//cdn.host/x");
    assert_eq!(n("//cdn.host//x/"), "//cdn.host/x");
    assert_eq!(n("api/x"), "api/x");
    assert_eq!(n("/api//x/"), "/api/x");
    assert_eq!(n("{}/api/orders"), "{}/api/orders");
}

#[test]
fn client_calls_are_kept_without_a_route() {
    let dir = fixture("client-hops");
    let d = derive_tree(&dir);
    let names = labels(&dir, &d);
    let rows: BTreeSet<_> = d
        .client_calls
        .iter()
        .map(|c| {
            (
                names[&c.src_id].clone(),
                c.method.clone(),
                c.url.clone(),
                c.rule.clone(),
            )
        })
        .collect();
    let row = |src: &str, method: &str, url: &str, rule: &str| {
        (
            src.to_string(),
            method.to_string(),
            url.to_string(),
            rule.to_string(),
        )
    };
    let want: BTreeSet<_> = [
        row(
            "lib/api.ts#elsewhere",
            "GET",
            "{}/api/orders",
            "ts.client.fetch",
        ),
        row(
            "lib/api.ts#loadOrders",
            "GET",
            "/api/orders",
            "ts.client.fetch",
        ),
        row(
            "lib/api.ts#removeOrder",
            "DELETE",
            "/api/orders/{}",
            "ts.client.fetch",
        ),
        row(
            "lib/api.ts#createOrder",
            "POST",
            "/api/orders",
            "ts.client.axios",
        ),
        row(
            "lib/generated.ts#OrdersService.list",
            "GET",
            "/api/orders",
            "ts.client.request_helper",
        ),
        row(
            "ui/orders.tsx#Orders",
            "GET",
            "/api/orders/7",
            "ts.client.swr",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(rows, want);
    let elsewhere = d
        .client_calls
        .iter()
        .find(|c| c.url == "{}/api/orders")
        .unwrap();
    assert_eq!(elsewhere.file, "lib/api.ts");
    assert_eq!(elsewhere.line, 19);
    // Sorted, as every list is.
    let mut sorted = d.client_calls.clone();
    sorted.sort();
    assert_eq!(d.client_calls, sorted);
}

#[test]
fn an_unreadable_method_is_stored_as_any_and_matches_every_verb() {
    let dir = app(&[
        (
            "app/api/x/route.ts",
            "export async function PUT() { return new Response() }\n",
        ),
        (
            "lib/c.ts",
            "export function c(m: string) { return fetch('/api/x', { method: m }) }\n",
        ),
    ]);
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    assert_eq!(d.client_calls.len(), 1, "{:#?}", d.client_calls);
    assert_eq!(d.client_calls[0].method, "ANY");
    assert_eq!(
        calls_from(&d, &names, "lib/c.ts#c"),
        BTreeSet::from([pair("PUT /api/x", "ts.client.fetch")])
    );
}

#[test]
fn matching_prefers_literals_and_accepts_catch_alls_and_any_routes() {
    let dir = app(&[
        (
            "app/api/orders/new/route.ts",
            "export async function GET() { return new Response() }\n",
        ),
        (
            "app/api/orders/[id]/route.ts",
            "export async function GET() { return new Response() }\n",
        ),
        (
            "app/docs/[...slug]/page.tsx",
            "export default function D() { return null }\n",
        ),
        (
            "app/help/[[...topic]]/page.tsx",
            "export default function H() { return null }\n",
        ),
        ("pages/api/any.ts", "export default function handler() {}\n"),
        (
            "lib/c.ts",
            "export function literal() { return fetch('/api/orders/new') }\n\
             export function param(id: string) { return fetch(`/api/orders/${id}`) }\n\
             export function docs() { return fetch('/docs/a/b/c') }\n\
             export function docsNone() { return fetch('/docs') }\n\
             export function help() { return fetch('/help') }\n\
             export function helpDeep() { return fetch('/help/a/b') }\n\
             export function any() { return fetch('/api/any', { method: 'DELETE' }) }\n\
             export function deep() { return fetch('/api/orders/1/2') }\n\
             export function nearby() { return fetch('/api/orders/{}/x') }\n",
        ),
    ]);
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    let one = |name: &str| calls_from(&d, &names, &format!("lib/c.ts#{name}"));
    let fetch = |dst: &str| BTreeSet::from([pair(dst, "ts.client.fetch")]);
    assert_eq!(one("literal"), fetch("GET /api/orders/new"));
    // `{}` is a template part: it fits a parameter, never the literal `new`.
    assert_eq!(one("param"), fetch("GET /api/orders/{id}"));
    assert_eq!(one("docs"), fetch("GET /docs/{...slug}"));
    assert!(one("docsNone").is_empty());
    assert_eq!(one("help"), fetch("GET /help/{...topic?}"));
    assert_eq!(one("helpDeep"), fetch("GET /help/{...topic?}"));
    assert_eq!(one("any"), fetch("ANY /api/any"));
    assert!(one("deep").is_empty());
    assert!(one("nearby").is_empty());
}

#[test]
fn a_client_may_call_a_route_of_another_app() {
    let dir = tempfile::tempdir().unwrap();
    for (name, rel, text) in [
        (
            "web",
            "app/page.tsx",
            "export default function P() { return null }\n",
        ),
        (
            "web",
            "lib/c.ts",
            "export function c() { return fetch('/api/pay') }\n",
        ),
        (
            "pay",
            "app/api/pay/route.ts",
            "export async function GET() { return new Response() }\n",
        ),
    ] {
        write(dir.path(), &format!("apps/{name}/{rel}"), text);
    }
    for name in ["web", "pay"] {
        write(
            dir.path(),
            &format!("apps/{name}/package.json"),
            r#"{ "name": "x", "dependencies": { "next": "15.2.0" } }"#,
        );
    }
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    assert_eq!(
        calls_from(&d, &names, "apps/web/lib/c.ts#c"),
        BTreeSet::from([pair("GET /api/pay", "ts.client.fetch")])
    );
}

#[test]
fn react_query_and_swr_hops_are_inferred() {
    let dir = fixture("client-hops");
    let d = derive_tree(&dir);
    let names = labels(&dir, &d);
    let got = edges(&d, &names, "calls");
    let want: BTreeSet<_> = [
        edge(
            "ui/orders.tsx#Orders",
            "lib/api.ts#createOrder",
            "ts.react_query.fn",
        ),
        edge(
            "ui/orders.tsx#Orders",
            "lib/api.ts#loadOrders",
            "ts.react_query.fn",
        ),
        edge(
            "ui/orders.tsx#Orders",
            "ui/orders.tsx#refresh",
            "ts.react_query.back",
        ),
        edge(
            "ui/orders.tsx#Orders",
            "ui/orders.tsx#fetcher",
            "ts.swr.fetcher",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(got, want);
    for e in d.edges.iter().filter(|e| e.kind == "calls") {
        assert_eq!(e.file, "ui/orders.tsx");
        assert!(e.line > 0);
    }
}

#[test]
fn hook_targets_that_are_not_one_symbol_give_no_edge() {
    let dir = app(&[
        (
            "lib/svc.ts",
            "export class OrdersService {\n  static list() { return 1 }\n  static done() { return 2 }\n}\n\
             export function load() { return 3 }\n",
        ),
        (
            "ui/c.tsx",
            "import { useMutation, useQuery } from '@tanstack/react-query'\n\
             import { OrdersService } from '../lib/svc'\n\
             import { missing } from 'somewhere'\n\
             export function C() {\n\
             \x20 function login() {}\n\
             \x20 useMutation({ mutationFn: login, onError: missing, onSuccess: OrdersService.done })\n\
             \x20 useQuery({ queryFn: OrdersService.list, onSettled: async () => 1 })\n\
             \x20 useQuery({ queryFn: 'x' })\n\
             }\n",
        ),
    ]);
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    let got = edges(&d, &names, "calls");
    let want: BTreeSet<_> = [
        edge(
            "ui/c.tsx#C",
            "lib/svc.ts#OrdersService.done",
            "ts.react_query.back",
        ),
        edge(
            "ui/c.tsx#C",
            "lib/svc.ts#OrdersService.list",
            "ts.react_query.fn",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(got, want);
}

#[test]
fn server_action_forms_link_through_aliases() {
    let dir = fixture("next-app");
    let d = derive_tree(&dir);
    let names = labels(&dir, &d);
    let got = edges(&d, &names, "calls");
    let place = "app/lib/actions.ts#placeOrder";
    let cancel = "app/lib/actions.ts#cancelOrder";
    // The route-sourced `calls` (middleware) belong to another task; keep this test to ours.
    let ours: BTreeSet<_> = got
        .iter()
        .filter(|(_, _, rule)| rule.starts_with("ts.next.action"))
        .cloned()
        .collect();
    let want: BTreeSet<_> = [
        edge(
            "app/ui/order-form.tsx#OrderForm",
            place,
            "ts.next.action.form",
        ),
        edge(
            "app/ui/order-form.tsx#OrderForm",
            place,
            "ts.next.action.state",
        ),
        edge(
            "app/ui/cancel-button.tsx#CancelButton",
            cancel,
            "ts.next.action.form",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(ours, want, "{got:#?}");
    // `Inline` names a nested function: no edge, as the fact stays for L3.
    assert!(
        got.iter()
            .all(|(src, _, _)| src != "app/ui/inline.tsx#Inline")
    );

    let state = d
        .edges
        .iter()
        .find(|e| e.rule == "ts.next.action.state")
        .unwrap();
    assert_eq!(
        (state.file.as_str(), state.line),
        ("app/ui/order-form.tsx", 7)
    );
    let form = d
        .edges
        .iter()
        .find(|e| e.rule == "ts.next.action.form" && e.file == "app/ui/order-form.tsx")
        .unwrap();
    assert_eq!(form.line, 9);

    let search: Vec<_> = d
        .client_calls
        .iter()
        .filter(|c| c.file == "app/ui/search.tsx")
        .collect();
    assert_eq!(search.len(), 1, "{search:#?}");
    assert_eq!(
        (
            search[0].method.as_str(),
            search[0].url.as_str(),
            search[0].rule.as_str()
        ),
        ("GET", "/search", "ts.client.form")
    );
    assert_eq!(names[&search[0].src_id], "app/ui/search.tsx#Search");
    // `/search` is not a route of the fixture: stored, not matched.
    assert!(calls_from(&d, &names, "app/ui/search.tsx#Search").is_empty());
}

#[test]
fn an_action_bound_then_wrapped_by_use_action_state_gives_both_edges() {
    let dir = app(&[
        (
            "app/actions.ts",
            "'use server'\nexport async function save(id: string, prev: unknown, data: FormData) { return id }\n",
        ),
        (
            "app/form.tsx",
            "import { useActionState } from 'react'\n\
             import { save } from './actions'\n\
             import { other } from './plain'\n\
             export function F({ id }: { id: string }) {\n\
             \x20 const bound = save.bind(null, id)\n\
             \x20 const [s, act] = useActionState(bound, null)\n\
             \x20 const [t, act2] = useActionState(other, null)\n\
             \x20 return <form action={act}>\n\
             \x20 <button formAction={act2} formMethod=\"post\" />\n\
             \x20 <button formAction={save} /></form>\n\
             }\n",
        ),
        ("app/plain.ts", "export async function other() {}\n"),
    ]);
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    let got = edges(&d, &names, "calls");
    let want: BTreeSet<_> = [
        edge(
            "app/form.tsx#F",
            "app/actions.ts#save",
            "ts.next.action.form",
        ),
        edge(
            "app/form.tsx#F",
            "app/actions.ts#save",
            "ts.next.action.state",
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(got, want);
    // `<form action={act}>` and `<button formAction={save}>` are on different lines of the
    // source, so they are two edges; the state edge is at the `useActionState` line.
    let lines: BTreeSet<_> = d.edges.iter().map(|e| (e.rule.as_str(), e.line)).collect();
    assert_eq!(
        lines,
        BTreeSet::from([
            ("ts.next.action.state", 6),
            ("ts.next.action.form", 8),
            ("ts.next.action.form", 10),
        ])
    );
}

#[test]
fn forms_without_an_action_call_their_own_page() {
    let dir = app(&[
        (
            "app/orders/[id]/page.tsx",
            "export default function Order() {\n  return <form method=\"post\"><input name=\"q\" /></form>\n}\n",
        ),
        (
            "app/page.tsx",
            "export default function Home() {\n  return <div><form><input /></form><form onSubmit={() => 1}><input /></form></div>\n}\n",
        ),
        (
            "app/ui/loose.tsx",
            "export function Loose() {\n  return <form><input /></form>\n}\n",
        ),
        (
            "app/ui/btn.tsx",
            "export function Btn() {\n  return <form><button formAction=\"/go\" formMethod=\"post\" /><input type=\"submit\" formAction={'/x?y=2'} /></form>\n}\n",
        ),
        (
            "app/go/route.ts",
            "export async function POST() { return new Response() }\n",
        ),
    ]);
    let d = derive_tree(dir.path());
    let names = labels(dir.path(), &d);
    let rows: BTreeSet<_> = d
        .client_calls
        .iter()
        .map(|c| {
            (
                names[&c.src_id].clone(),
                c.method.clone(),
                c.url.clone(),
                c.rule.clone(),
            )
        })
        .collect();
    let row = |src: &str, method: &str, url: &str, rule: &str| {
        (
            src.to_string(),
            method.to_string(),
            url.to_string(),
            rule.to_string(),
        )
    };
    let want: BTreeSet<_> = [
        // The page's own pattern, with the form's own method.
        row(
            "app/orders/[id]/page.tsx#Order",
            "POST",
            "/orders/{id}",
            "ts.client.form_self",
        ),
        row("app/page.tsx#Home", "GET", "/", "ts.client.form_self"),
        row("app/ui/btn.tsx#Btn", "POST", "/go", "ts.client.form"),
        row("app/ui/btn.tsx#Btn", "GET", "/x", "ts.client.form"),
    ]
    .into_iter()
    .collect();
    assert_eq!(rows, want);

    // The self form of `/` finds its page; a POST to a page that only has GET does not
    // match; `formAction="/go"` matches the route handler.
    assert_eq!(
        calls_from(&d, &names, "app/page.tsx#Home"),
        BTreeSet::from([pair("GET /", "ts.client.form_self")])
    );
    assert!(calls_from(&d, &names, "app/orders/[id]/page.tsx#Order").is_empty());
    assert_eq!(
        calls_from(&d, &names, "app/ui/btn.tsx#Btn"),
        BTreeSet::from([pair("POST /go", "ts.client.form")])
    );
}
