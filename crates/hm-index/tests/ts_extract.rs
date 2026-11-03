mod common;

use common::read_fixture;
use hm_index::facts::{ExportFact, ImportKind, ImportedName, ParseStatus, SymbolKind};
use hm_index::hash::{facts_hash, interface_hash};
use hm_index::lang::Lang;
use hm_index::ts::extract;

#[test]
fn grammars_load_under_the_pinned_core() {
    for (src, lang) in [
        ("const a = 1 satisfies number;", Lang::TypeScript),
        ("export default () => <div/>;", Lang::Tsx),
        ("export const f = (a) => a?.b ?? 1;", Lang::JavaScript),
    ] {
        assert_eq!(extract(src, lang).status, ParseStatus::Parsed, "{lang:?}");
    }
}

#[test]
fn service_ts_yields_its_symbols_imports_and_calls() {
    let f = extract(
        &read_fixture("ts-basic/src/orders/service.ts"),
        Lang::TypeScript,
    );
    let names: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("", "placeOrder", SymbolKind::Function),
            ("", "OrderService", SymbolKind::Class),
            ("OrderService", "find", SymbolKind::Method),
            ("OrderService", "total", SymbolKind::Method),
            ("OrderService", "sum", SymbolKind::Method)
        ]
    );
    let place = &f.symbols[0];
    assert_eq!((place.l0, place.l1, place.exported), (5, 9, true));
    let specs: Vec<_> = f.imports.iter().map(|i| i.specifier.as_str()).collect();
    assert_eq!(specs, vec!["zod", "../db", "./mail"]);
    assert_eq!(
        f.imports[2].names,
        vec![ImportedName {
            imported: "*".into(),
            local: "mail".into()
        }]
    );
    let calls: Vec<_> = f
        .calls
        .iter()
        .map(|c| (c.callee.as_str(), c.receiver.as_deref(), c.line))
        .collect();
    assert!(calls.contains(&("insert", Some("db"), 6)));
    assert!(calls.contains(&("sendReceipt", Some("mail"), 7)));
    assert!(calls.contains(&("sum", Some("this"), 16)));
}

#[test]
fn a_comment_edit_keeps_the_body_hash_and_a_literal_edit_changes_it() {
    let a = extract("function f() { return \"x\"; }", Lang::TypeScript);
    let b = extract(
        "function f() { /* note */ return \"x\"; }",
        Lang::TypeScript,
    );
    let c = extract("function f() { return \"y\"; }", Lang::TypeScript);
    assert_eq!(a.symbols[0].body_hash, b.symbols[0].body_hash);
    assert_ne!(a.symbols[0].body_hash, c.symbols[0].body_hash);
    assert_eq!(a.symbols[0].sig_hash, c.symbols[0].sig_hash);
    assert_eq!(facts_hash(&a), facts_hash(&c));
}

#[test]
fn a_syntax_error_keeps_the_symbols_that_parse() {
    let f = extract(
        "export function ok() { return 1; }\nexport function half( {",
        Lang::TypeScript,
    );
    assert_eq!(f.status, ParseStatus::ParsedWithErrors);
    assert!(f.symbols.iter().any(|s| s.name == "ok" && s.exported));
}

#[test]
fn jsx_and_require_and_reexports_are_seen() {
    let page = extract(&read_fixture("ts-basic/src/ui/Page.tsx"), Lang::Tsx);
    assert!(
        page.calls.iter().any(|c| c.callee == "Button"
            && c.jsx
            && c.owner == Some((String::new(), "Page".into())))
    );

    let legacy = extract(&read_fixture("ts-basic/src/legacy.js"), Lang::JavaScript);
    let require = &legacy.imports[0];
    assert_eq!(require.kind, ImportKind::Require);
    assert_eq!(require.specifier, "./orders/service");
    assert_eq!(
        require.names,
        vec![ImportedName {
            imported: "placeOrder".into(),
            local: "placeOrder".into()
        }]
    );
    assert!(
        legacy
            .symbols
            .iter()
            .any(|s| s.name == "run" && s.kind == SymbolKind::Function)
    );
    assert!(legacy.calls.iter().all(|c| c.callee != "require"));

    let index = extract(
        &read_fixture("ts-basic/src/orders/index.ts"),
        Lang::TypeScript,
    );
    assert!(index.exports.contains(&ExportFact {
        name: "*".into(),
        local: None,
        from: Some("./service".into()),
        star: true,
        line: 1
    }));
    assert!(index.exports.contains(&ExportFact {
        name: "receipt".into(),
        local: Some("sendReceipt".into()),
        from: Some("./mail".into()),
        star: false,
        line: 2
    }));
    assert!(index.imports.iter().all(|i| i.kind == ImportKind::ReExport));
}

#[test]
fn function_forms_and_nesting() {
    let src = "\
const a = () => 1;
const b = function () { return 2; };
export default function () {}
function outer() {
  const inner = async () => { helper(); };
  inner();
}
export const db = { insert() { run(); } };
export const answer = 42;
const hidden = 1;
interface I { x: number }
type T = string;
";
    let f = extract(src, Lang::TypeScript);
    let got: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.name.as_str(), s.kind, s.exported, s.default_export))
        .collect();
    assert_eq!(
        got,
        vec![
            ("a", SymbolKind::Function, false, false),
            ("b", SymbolKind::Function, false, false),
            ("default", SymbolKind::Function, true, true),
            ("outer", SymbolKind::Function, false, false),
            ("db", SymbolKind::Variable, true, false),
            ("answer", SymbolKind::Variable, true, false),
            ("I", SymbolKind::Interface, false, false),
            ("T", SymbolKind::Interface, false, false),
        ]
    );
    // calls in the nested arrow belong to `outer`, those in the object method to `db`
    let owned = |callee: &str| {
        f.calls
            .iter()
            .find(|c| c.callee == callee)
            .and_then(|c| c.owner.clone())
            .map(|(_, n)| n)
    };
    assert_eq!(owned("helper").as_deref(), Some("outer"));
    assert_eq!(owned("inner").as_deref(), Some("outer"));
    assert_eq!(owned("run").as_deref(), Some("db"));
    let outer = &f.symbols[3];
    assert_eq!((outer.l0, outer.l1, outer.lines), (4, 7, 4));
    assert_eq!(f.symbols[0].sig, "const a = () =>");
}

#[test]
fn imports_exports_and_calls_in_their_forms() {
    let src = r#"
import type { A } from "types";
import D, { b as c, type E } from "mod";
import "side";
const lazy = () => import("./lazy");
const { x, y: z } = require("cjs");
const whole = require("whole");
export { c as renamed };
export default D;
export * as ns from "star";
const w = new Widget(1, 2);
z.object({}).parse(1);
a?.b();
"#;
    let f = extract(src, Lang::TypeScript);
    let imports: Vec<_> = f
        .imports
        .iter()
        .map(|i| (i.specifier.as_str(), i.kind, i.type_only, i.names.len()))
        .collect();
    assert_eq!(
        imports,
        vec![
            ("types", ImportKind::Static, true, 1),
            ("mod", ImportKind::Static, false, 3),
            ("side", ImportKind::Static, false, 0),
            ("./lazy", ImportKind::Dynamic, false, 0),
            ("cjs", ImportKind::Require, false, 2),
            ("whole", ImportKind::Require, false, 1),
            ("star", ImportKind::ReExport, false, 1),
        ]
    );
    assert_eq!(f.imports[1].names[0].imported, "default");
    assert_eq!(f.imports[4].names[1].imported, "y");
    assert_eq!(f.imports[4].names[1].local, "z");
    let exports: Vec<_> = f
        .exports
        .iter()
        .map(|e| (e.name.as_str(), e.local.as_deref(), e.from.as_deref()))
        .collect();
    assert_eq!(
        exports,
        vec![
            ("renamed", Some("c"), None),
            ("default", Some("D"), None),
            ("ns", Some("*"), Some("star")),
        ]
    );
    let calls: Vec<_> = f
        .calls
        .iter()
        .map(|c| (c.callee.as_str(), c.receiver.as_deref(), c.args))
        .collect();
    assert!(calls.contains(&("Widget", None, 2)));
    assert!(calls.contains(&("parse", Some("z.object({})"), 1)));
    assert!(calls.contains(&("object", Some("z"), 1)));
    assert!(calls.contains(&("b", Some("a"), 0)));
}

#[test]
fn heritage_and_branches() {
    let src = "\
class A extends B {
  m(x: number) {
    if (x && x > 1) { return x ? 1 : 2; }
    for (const i of [1]) { while (i) { break; } }
    switch (x) { case 1: break; }
    try { run(); } catch (e) { return 0; }
    return 3;
  }
}
";
    let f = extract(src, Lang::TypeScript);
    assert_eq!(f.heritage.len(), 1);
    assert_eq!(
        (
            f.heritage[0].class.as_str(),
            f.heritage[0].extends.as_str(),
            f.heritage[0].line
        ),
        ("A", "B", 1)
    );
    let m = f.symbols.iter().find(|s| s.name == "m").unwrap();
    // if, &&, ?:, for, while, case, catch
    assert_eq!(m.branches, 7);
    let js = extract("class A extends B {}", Lang::JavaScript);
    assert_eq!(js.heritage.len(), 1);
}

#[test]
fn a_signature_edit_changes_the_interface_hash_and_a_body_edit_does_not() {
    let base = extract(
        "export function f(a: number) { return a; }",
        Lang::TypeScript,
    );
    let body = extract(
        "export function f(a: number) { return a + 1; }",
        Lang::TypeScript,
    );
    let sig = extract(
        "export function f(a: string) { return a; }",
        Lang::TypeScript,
    );
    assert_eq!(interface_hash(&base), interface_hash(&body));
    assert_ne!(interface_hash(&base), interface_hash(&sig));
    assert_ne!(facts_hash(&base), facts_hash(&sig));
}

#[test]
fn a_local_export_clause_marks_the_symbol_exported() {
    let f = extract("function a() {}\nexport { a as b };", Lang::TypeScript);
    assert!(f.symbols[0].exported);
}

#[test]
fn implements_alone_is_not_heritage() {
    let alone = extract("class A implements I {}", Lang::TypeScript);
    assert!(alone.heritage.is_empty(), "{:?}", alone.heritage);

    let both = extract("class A extends B implements I {}", Lang::TypeScript);
    assert_eq!(both.heritage.len(), 1);
    assert_eq!(both.heritage[0].extends, "B");

    let js = extract("class A extends B {}", Lang::JavaScript);
    assert_eq!(js.heritage.len(), 1);
    assert_eq!(js.heritage[0].extends, "B");
}

#[test]
fn module_names_hold_the_top_level_bindings_that_are_not_symbols() {
    let f = extract(
        "const { a, b: [c, ...d], e = 1, ...rest } = make();\n\
         let [x, , y = 2] = make();\n\
         var legacy = make();\n\
         const prisma = new PrismaClient(), schema = z.object({});\n\
         const fn1 = () => 1;\n\
         export const exported = make();\n\
         export const { fromExport } = make();\n\
         enum Color { Red }\n\
         declare enum Dec { A }\n\
         namespace N { export const hidden = 1; }\n\
         declare namespace Amb { const hidden2: number; }\n\
         module Old {}\n\
         import fs = require('fs');\n\
         declare const ambient: any;\n\
         declare function amb(): void;\n\
         function make(): any { return {}; }\n\
         function inner() { const nested = 1; }\n\
         class K { m() { const alsoNested = 1; } }\n",
        Lang::TypeScript,
    );
    assert_eq!(
        f.module_names,
        [
            "Amb",
            "Color",
            "Dec",
            "N",
            "Old",
            "a",
            "amb",
            "ambient",
            "c",
            "d",
            "e",
            "fromExport",
            "fs",
            "legacy",
            "prisma",
            "rest",
            "schema",
            "x",
            "y"
        ]
    );
    // A file with no such names keeps the field out of its JSON.
    let plain = extract("export function f() {}\n", Lang::TypeScript);
    assert!(plain.module_names.is_empty());
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("module_names")
    );
}
