//! Next.js and React facts: directives, forms, aliases and the middleware matcher.
//!
//! Rules checked against the official docs while building:
//! - `'use server'` / `'use client'` / `'use cache'` count only at the very beginning of a
//!   file or of a function body, above any other code including imports, and comments above
//!   them are fine; they are written with single or double quotes, not backticks.
//!   https://react.dev/reference/rsc/use-server and
//!   https://nextjs.org/docs/app/api-reference/directives/use-server
//! - The middleware `config.matcher` is a string, an array of strings, or an array of objects
//!   whose `source` is the path; only constants are read, dynamic values are ignored.
//!   https://nextjs.org/docs/app/api-reference/file-conventions/proxy (`middleware` is its
//!   deprecated name; the matcher rules are the same)

mod common;

use common::read_fixture;
use hm_index::facts::{ArgFact, FileFacts};
use hm_index::lang::Lang;

fn fixture_facts(rel: &str, lang: Lang) -> FileFacts {
    hm_index::ts::extract(&read_fixture(&format!("next-app/{rel}")), lang)
}

fn tsx(src: &str) -> FileFacts {
    hm_index::ts::extract(src, Lang::Tsx)
}

fn owner(name: &str) -> Option<(String, String)> {
    Some((String::new(), name.to_string()))
}

#[test]
fn file_directive_is_found() {
    let f = fixture_facts("app/lib/actions.ts", Lang::TypeScript);
    assert_eq!(f.web.directives.len(), 1, "{:?}", f.web.directives);
    let d = &f.web.directives[0];
    assert_eq!(d.owner, None);
    assert_eq!(d.function, None);
    assert_eq!(d.text, "use server");
    assert_eq!(d.line, 1);
}

#[test]
fn function_directive_names_its_function() {
    let f = fixture_facts("app/ui/inline.tsx", Lang::Tsx);
    assert_eq!(f.web.directives.len(), 1, "{:?}", f.web.directives);
    let d = &f.web.directives[0];
    assert_eq!(d.function.as_deref(), Some("save"));
    assert_eq!(d.owner, owner("Inline"));
    assert_eq!(d.text, "use server");
    assert_eq!(d.line, 3);
}

#[test]
fn forms_and_aliases_are_recorded() {
    let f = fixture_facts("app/ui/order-form.tsx", Lang::Tsx);
    assert_eq!(f.web.aliases.len(), 1, "{:?}", f.web.aliases);
    let a = &f.web.aliases[0];
    assert_eq!(a.owner, owner("OrderForm"));
    assert_eq!(
        (a.local.as_str(), a.target.as_str(), a.via.as_str()),
        ("formAction", "placeOrder", "useActionState")
    );
    assert_eq!(f.web.forms.len(), 1, "{:?}", f.web.forms);
    let form = &f.web.forms[0];
    assert_eq!(form.element, "form");
    assert_eq!(form.action, Some(ArgFact::Name("formAction".into())));
    assert_eq!(form.owner, owner("OrderForm"));
    assert!(!form.on_submit);

    let f = fixture_facts("app/ui/cancel-button.tsx", Lang::Tsx);
    assert_eq!(f.web.aliases.len(), 1, "{:?}", f.web.aliases);
    let a = &f.web.aliases[0];
    assert_eq!(
        (a.local.as_str(), a.target.as_str(), a.via.as_str()),
        ("cancelWithId", "cancelOrder", "bind")
    );
    assert_eq!(
        f.web.forms[0].action,
        Some(ArgFact::Name("cancelWithId".into()))
    );

    let f = fixture_facts("app/ui/search.tsx", Lang::Tsx);
    assert_eq!(f.web.forms.len(), 2, "{:?}", f.web.forms);
    let get = &f.web.forms[0];
    assert_eq!(get.action, Some(ArgFact::Str("/search".into())));
    assert_eq!(get.method.as_deref(), Some("GET"));
    assert!(!get.on_submit);
    let submit = &f.web.forms[1];
    assert_eq!(submit.action, None);
    assert_eq!(submit.method, None);
    assert!(submit.on_submit);
}

#[test]
fn middleware_matcher_strings_are_read() {
    let f = fixture_facts("middleware.ts", Lang::TypeScript);
    let m = f.web.matcher.expect("a matcher");
    assert_eq!(m.values, ["/account/:path*", "/checkout"]);
    assert_eq!(m.line, 11);
}

#[test]
fn a_directive_after_code_is_not_a_directive() {
    let f = hm_index::ts::extract("const a = 1; 'use server';", Lang::TypeScript);
    assert!(f.web.directives.is_empty(), "{:?}", f.web.directives);
    let f = hm_index::ts::extract(
        "import x from 'y'\n'use client'\nexport const a = 1",
        Lang::TypeScript,
    );
    assert!(f.web.directives.is_empty(), "{:?}", f.web.directives);
    let f = hm_index::ts::extract(
        "export function a() { go(); 'use server'; }",
        Lang::TypeScript,
    );
    assert!(f.web.directives.is_empty(), "{:?}", f.web.directives);
}

#[test]
fn comments_may_come_before_a_directive_and_backticks_do_not_count() {
    let f = hm_index::ts::extract(
        "// shop actions\n/* more */\n\"use server\"\nexport const a = 1",
        Lang::TypeScript,
    );
    assert_eq!(f.web.directives.len(), 1, "{:?}", f.web.directives);
    assert_eq!(f.web.directives[0].line, 3);
    assert_eq!(f.web.directives[0].text, "use server");

    let f = hm_index::ts::extract("`use server`\nexport const a = 1", Lang::TypeScript);
    assert!(f.web.directives.is_empty(), "{:?}", f.web.directives);
}

#[test]
fn only_the_three_directives_count_and_they_may_follow_another_prologue_string() {
    let f = hm_index::ts::extract(
        "'use strict'\n'use client'\nexport const a = 1",
        Lang::JavaScript,
    );
    let texts: Vec<_> = f.web.directives.iter().map(|d| d.text.as_str()).collect();
    assert_eq!(texts, ["use client"]);

    let f = hm_index::ts::extract("'use cache'\nexport const a = 1", Lang::TypeScript);
    assert_eq!(f.web.directives[0].text, "use cache");
}

#[test]
fn function_directives_cover_arrows_methods_and_anonymous_functions() {
    let f = tsx("export function Shop() {\n\
         \x20 const save = async () => { 'use server' }\n\
         \x20 const run = async function () { 'use server' }\n\
         \x20 return <Form action={async () => { 'use server' }} />\n\
         }\n\
         export default async function () { 'use server' }\n\
         class A { async go() { 'use server' } }\n");
    let got: Vec<_> = f
        .web
        .directives
        .iter()
        .map(|d| (d.function.clone().unwrap(), d.line))
        .collect();
    assert_eq!(
        got,
        [
            ("save".to_string(), 2),
            ("run".to_string(), 3),
            (String::new(), 4),
            (String::new(), 6),
            ("go".to_string(), 7),
        ]
    );
    assert_eq!(f.web.directives[0].owner, owner("Shop"));
}

#[test]
fn form_actions_are_classified() {
    let f = tsx("export function A({ ui }) {\n\
         \x20 return <div>\n\
         \x20   <form action={ui.send} method=\"post\"></form>\n\
         \x20   <form action={'/x'}></form>\n\
         \x20   <form action={() => go()}></form>\n\
         \x20   <form action={`/x/${id}`}></form>\n\
         \x20   <form action=\"/y\" />\n\
         \x20   <form></form>\n\
         \x20   <Form action=\"/skipped\" />\n\
         \x20   <button formAction={doIt}>Go</button>\n\
         \x20   <input type=\"submit\" formAction=\"/z\" />\n\
         \x20   <button onClick={go}>Plain</button>\n\
         \x20 </div>\n\
         }\n");
    let got: Vec<_> = f
        .web
        .forms
        .iter()
        .map(|x| (x.element.as_str(), x.action.clone(), x.method.clone()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "form",
                Some(ArgFact::Name("ui.send".into())),
                Some("POST".to_string())
            ),
            ("form", Some(ArgFact::Str("/x".into())), None),
            ("form", Some(ArgFact::Other), None),
            ("form", Some(ArgFact::Other), None),
            ("form", Some(ArgFact::Str("/y".into())), None),
            ("form", None, None),
            ("button", Some(ArgFact::Name("doIt".into())), None),
            ("input", Some(ArgFact::Str("/z".into())), None),
        ]
    );
}

#[test]
fn aliases_need_a_function_an_identifier_target_and_the_second_array_slot() {
    let f = tsx("const top = send.bind(null, 1)\n\
         export function A({ id }) {\n\
         \x20 const [s1, act1, pending] = useActionState(createItem, null)\n\
         \x20 const [s2, act2] = useFormState(updateItem, null)\n\
         \x20 const [s3, act3] = useActionState(api.create, null)\n\
         \x20 const [s4, act4] = useState(createItem)\n\
         \x20 const bound = remove.bind(null, id)\n\
         \x20 const viaMember = api.remove.bind(null, id)\n\
         \x20 const [only] = useActionState(createItem, null)\n\
         }\n");
    let got: Vec<_> = f
        .web
        .aliases
        .iter()
        .map(|a| (a.local.as_str(), a.target.as_str(), a.via.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("act1", "createItem", "useActionState"),
            ("act2", "updateItem", "useActionState"),
            ("bound", "remove", "bind"),
        ]
    );
}

#[test]
fn matcher_forms_are_read_in_source_order() {
    let m = |src: &str| hm_index::ts::extract(src, Lang::TypeScript).web.matcher;

    let one = m("export const config = { matcher: '/about/:path*' }").unwrap();
    assert_eq!(one.values, ["/about/:path*"]);

    let objects = m("export const config = {\n  matcher: [\n    { source: '/api/:path*', has: [{ type: 'header', key: 'x' }] },\n    { source: '/b', locale: false },\n    '/c',\n  ],\n}")
        .unwrap();
    assert_eq!(objects.values, ["/api/:path*", "/b", "/c"]);

    let typed = m("export const config = { matcher: ['/a'] as const } satisfies Config").unwrap();
    assert_eq!(typed.values, ["/a"]);

    assert!(m("export const config = { runtime: 'nodejs' }").is_none());
    assert!(m("const config = { matcher: '/a' }").is_none());
    assert!(m("export const other = { matcher: '/a' }").is_none());
    assert!(m("export const config = { matcher: PATHS }").is_none());
}

#[test]
fn a_directive_function_is_named_by_a_binding_not_by_a_property_key() {
    let f = tsx("export function Shop() {\n\
         \x20 const save = async () => { 'use server' }\n\
         \x20 const actions = { run: async () => { 'use server' } }\n\
         \x20 return null\n\
         }\n");
    let got: Vec<_> = f
        .web
        .directives
        .iter()
        .map(|d| (d.function.clone().unwrap(), d.line))
        .collect();
    assert_eq!(got, [("save".to_string(), 2), (String::new(), 3)]);
}

#[test]
fn array_pattern_holes_keep_their_slot() {
    let f = tsx("export function A() {\n\
         \x20 const [s, , pending] = useActionState(f, null)\n\
         \x20 const [, act] = useActionState(g, null)\n\
         \x20 const [, , third] = useFormState(h, null)\n\
         }\n");
    let got: Vec<_> = f
        .web
        .aliases
        .iter()
        .map(|a| (a.local.as_str(), a.target.as_str()))
        .collect();
    assert_eq!(got, [("act", "g")]);
}

#[test]
fn a_hashbang_line_is_not_code_before_a_directive() {
    let f = hm_index::ts::extract(
        "#!/usr/bin/env node\n'use server'\nexport const a = 1",
        Lang::JavaScript,
    );
    assert_eq!(f.web.directives.len(), 1, "{:?}", f.web.directives);
    assert_eq!(f.web.directives[0].line, 2);
}

#[test]
fn an_optional_chain_action_is_other() {
    let f = tsx("export function A({ actions }) { return <form action={actions?.save} /> }");
    assert_eq!(f.web.forms[0].action, Some(ArgFact::Other));
}
