//! Client call and data-hook facts (Learn spec §6.3), read from the `client-hops` fixture
//! and from small inline sources.

mod common;

use hm_index::facts::{ArgFact, FileFacts, HookFact, UrlCallFact};
use hm_index::lang::Lang;

fn fixture_facts(rel: &str) -> FileFacts {
    let lang = Lang::from_path(std::path::Path::new(rel)).unwrap();
    hm_index::ts::extract(&common::read_fixture(&format!("client-hops/{rel}")), lang)
}

fn inline(src: &str, lang: Lang) -> FileFacts {
    hm_index::ts::extract(src, lang)
}

/// `(owner name, client, method, url)` of every URL call.
fn calls(f: &FileFacts) -> Vec<(String, String, Option<String>, String)> {
    f.web
        .url_calls
        .iter()
        .map(|c: &UrlCallFact| {
            (
                c.owner.as_ref().map_or(String::new(), |(owner, name)| {
                    if owner.is_empty() {
                        name.clone()
                    } else {
                        format!("{owner}.{name}")
                    }
                }),
                c.client.clone(),
                c.method.clone(),
                c.url.clone(),
            )
        })
        .collect()
}

fn call(
    owner: &str,
    client: &str,
    method: Option<&str>,
    url: &str,
) -> (String, String, Option<String>, String) {
    (
        owner.into(),
        client.into(),
        method.map(String::from),
        url.into(),
    )
}

fn hook(f: &FileFacts, hook: &str, key: &str) -> Option<ArgFact> {
    f.web
        .hooks
        .iter()
        .find(|h: &&HookFact| h.hook == hook && h.key == key)
        .map(|h| h.target.clone())
}

// axios call shapes: https://axios-http.com/docs/api_intro
#[test]
fn fetch_and_axios_urls_are_recorded() {
    let f = fixture_facts("lib/api.ts");
    let got = calls(&f);
    assert!(
        got.contains(&call("loadOrders", "fetch", Some("GET"), "/api/orders")),
        "{got:?}"
    );
    assert!(
        got.contains(&call(
            "removeOrder",
            "fetch",
            Some("DELETE"),
            "/api/orders/{}"
        )),
        "{got:?}"
    );
    assert!(
        got.contains(&call("createOrder", "axios", Some("POST"), "/api/orders")),
        "{got:?}"
    );
    assert!(
        got.contains(&call("elsewhere", "fetch", Some("GET"), "{}/api/orders")),
        "{got:?}"
    );
    assert!(got.iter().all(|c| c.0 != "fromVar"), "{got:?}");
    assert_eq!(got.len(), 4, "{got:?}");
}

// A request helper generated from an OpenAPI schema: the verb is the callee name.
#[test]
fn request_helper_takes_the_verb_from_the_callee() {
    let f = fixture_facts("lib/generated.ts");
    assert_eq!(
        calls(&f),
        vec![call(
            "OrdersService.list",
            "request_helper",
            Some("GET"),
            "/api/orders"
        )]
    );
}

// useMutation options: https://tanstack.com/query/latest/docs/framework/react/reference/useMutation
// useSWR(key, fetcher): https://swr.vercel.app/docs/api
#[test]
fn swr_keys_and_hooks_are_recorded() {
    let f = fixture_facts("ui/orders.tsx");
    assert_eq!(
        calls(&f),
        vec![call("Orders", "swr", Some("GET"), "/api/orders/7")]
    );
    assert_eq!(
        hook(&f, "useMutation", "mutationFn"),
        Some(ArgFact::Name("createOrder".into()))
    );
    assert_eq!(
        hook(&f, "useMutation", "onSuccess"),
        Some(ArgFact::Name("refresh".into()))
    );
    assert_eq!(
        hook(&f, "useQuery", "queryFn"),
        Some(ArgFact::Name("loadOrders".into()))
    );
    assert_eq!(
        hook(&f, "useSWR", "fetcher"),
        Some(ArgFact::Name("fetcher".into()))
    );
    assert_eq!(f.web.hooks.len(), 4, "{:?}", f.web.hooks);
    assert!(
        f.web
            .hooks
            .iter()
            .all(|h| h.owner == Some((String::new(), "Orders".into())))
    );
}

#[test]
fn more_fetch_and_axios_shapes() {
    let f = inline(
        r#"
export function a() { return window.fetch('/w', { method: 'post' }) }
export function b() { return fetch(`/plain`) }
export function c(m: string) { return fetch('/dyn', { method: m }) }
export function d() { return axios('/d') }
export function e() { return axios({ url: '/e', method: 'put' }) }
export function g() { return axios.request({ url: '/g' }) }
export function h() { return axios.delete('/h/' + 1) }
export function i() { return api.post('/i', {}) }
export function j() { return orderClient.get("/j") }
export function k() { return http.patch(`/k/${1}/x?y=2`) }
export function l() { return other.post('/nope') }
"#,
        Lang::TypeScript,
    );
    let got = calls(&f);
    let want = vec![
        call("a", "fetch", Some("POST"), "/w"),
        call("b", "fetch", Some("GET"), "/plain"),
        call("c", "fetch", None, "/dyn"),
        call("d", "axios", Some("GET"), "/d"),
        call("e", "axios", Some("PUT"), "/e"),
        call("g", "axios", Some("GET"), "/g"),
        call("i", "axios", Some("POST"), "/i"),
        call("j", "axios", Some("GET"), "/j"),
        call("k", "axios", Some("PATCH"), "/k/{}/x"),
    ];
    assert_eq!(got, want);
}

#[test]
fn request_helpers_and_swr_mutation() {
    let f = inline(
        r#"
export function a() { return client.post({ url: '/api/v1/login' }) }
export function b() { return client.request({ url: '/b', method: 'delete' }) }
export function c() { return doIt({ url: '/c' }) }
export function d() { return doIt({ path: '/d' }) }
export function e() { useSWRMutation('/e', sender); useSWR(key, f2); useSWR(['/x', 1], f3); useSWR('plain', f4) }
"#,
        Lang::TypeScript,
    );
    let got = calls(&f);
    let want = vec![
        call("a", "request_helper", Some("POST"), "/api/v1/login"),
        call("b", "request_helper", Some("DELETE"), "/b"),
        call("c", "request_helper", None, "/c"),
        call("e", "swr", None, "/e"),
    ];
    assert_eq!(got, want);
    assert_eq!(
        hook(&f, "useSWRMutation", "fetcher"),
        Some(ArgFact::Name("sender".into()))
    );
    assert_eq!(
        hook(&f, "useSWR", "fetcher"),
        Some(ArgFact::Name("f2".into()))
    );
}

#[test]
fn hook_options_read_inline_and_shorthand_values() {
    let f = inline(
        r#"
export function C() {
  useMutation({ mutationFn: async (v) => v, onError() {}, onSettled: svc.done })
  useInfiniteQuery({ queryFn, queryKey: ['k'] })
  useSuspenseQuery({ queryFn: 'x', onSuccess: () => 1 })
  useQuery(options)
}
"#,
        Lang::Tsx,
    );
    assert_eq!(
        hook(&f, "useMutation", "mutationFn"),
        Some(ArgFact::Inline(None))
    );
    assert_eq!(
        hook(&f, "useMutation", "onError"),
        Some(ArgFact::Inline(None))
    );
    assert_eq!(
        hook(&f, "useMutation", "onSettled"),
        Some(ArgFact::Name("svc.done".into()))
    );
    assert_eq!(
        hook(&f, "useInfiniteQuery", "queryFn"),
        Some(ArgFact::Name("queryFn".into()))
    );
    assert_eq!(
        hook(&f, "useSuspenseQuery", "queryFn"),
        Some(ArgFact::Str("x".into()))
    );
    assert_eq!(
        hook(&f, "useSuspenseQuery", "onSuccess"),
        Some(ArgFact::Inline(None))
    );
    assert_eq!(f.web.hooks.len(), 6, "{:?}", f.web.hooks);
}

#[test]
fn javascript_files_are_read_too() {
    let f = inline(
        "export const load = () => fetch('/api/x')",
        Lang::JavaScript,
    );
    assert_eq!(
        calls(&f),
        vec![call("load", "fetch", Some("GET"), "/api/x")]
    );
}

// https://axios-http.com/docs/instance: `axios.create(config)` makes an instance whose
// `baseURL` is joined before relative request URLs.
#[test]
fn axios_instances_and_their_receivers_are_recorded() {
    let src = "import axios from 'axios'\n\
               export const api = axios.create({ baseURL: 'https://Shop.Example/v1/' })\n\
               const a = axios.create({ ...defaults }), b = axios.create(config)\n\
               let c = axios.create({ baseURL: `${host}/api` })\n\
               var d = axios.create({ timeout: 5 })\n\
               const e = axios.create({ baseURL: '' })\n\
               function make() { const inner = axios.create({ baseURL: '/x' }); return inner }\n\
               export function f() { a.get('/one'); api.get('//cdn/x'); api.post('https://x/y'); return axios.get('/two') }\n";
    let f = inline(src, Lang::TypeScript);
    let instances: Vec<_> = f
        .web
        .axios_instances
        .iter()
        .map(|a| (a.name.as_str(), a.base_url.as_str(), a.line))
        .collect();
    assert_eq!(
        instances,
        [
            ("api", "//shop.example/v1", 2),
            ("a", "{}", 3),
            ("b", "{}", 3),
            ("c", "{}/api", 4),
            ("d", "", 5),
            ("e", "", 6),
        ]
    );
    // `a` is an instance of this file although its name says nothing; only a relative URL
    // on a name other than `axios` keeps its receiver.
    let receivers: Vec<_> = f
        .web
        .url_calls
        .iter()
        .map(|c| (c.url.as_str(), c.receiver.as_deref()))
        .collect();
    assert_eq!(
        receivers,
        [
            ("/one", Some("a")),
            ("//cdn/x", None),
            ("//x/y", None),
            ("/two", None),
        ]
    );
}
