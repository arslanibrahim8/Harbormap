mod common;

use std::collections::BTreeSet;
use std::fs;

use std::collections::BTreeMap;

use common::{named_edges_for, resolver_for};
use hm_index::facts::{FileFacts, HeritageFact};
use hm_index::lang::Lang;
use hm_index::ts::resolve::{
    ModuleResolver, ModuleTarget, Project, ResolvedFile, resolve_export, resolve_file, symbol_ids,
};
use hm_index::ts::tsconfig::{TsConfigs, load_tsconfigs};

fn file(p: &str) -> ModuleTarget {
    ModuleTarget::File(p.into())
}

/// Writes `files` (relative path, text) into a temporary folder and loads its tsconfigs.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, TsConfigs, ModuleResolver) {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let known: BTreeSet<String> = files.iter().map(|(rel, _)| rel.to_string()).collect();
    let configs = load_tsconfigs(dir.path(), &known);
    let resolver = ModuleResolver::new(known, configs.clone());
    (dir, configs, resolver)
}

#[test]
fn fixture_specifiers_resolve() {
    let r = resolver_for("ts-basic");
    assert_eq!(
        r.resolve("src/index.ts", "@/orders/service"),
        file("src/orders/service.ts")
    );
    assert_eq!(
        r.resolve("src/ui/Page.tsx", "@/orders"),
        file("src/orders/index.ts")
    );
    assert_eq!(
        r.resolve("src/orders/service.ts", "../db"),
        file("src/db.ts")
    );
    assert_eq!(
        r.resolve("src/ui/Page.tsx", "./Button"),
        file("src/ui/Button.tsx")
    );
    assert_eq!(
        r.resolve("src/orders/service.ts", "zod"),
        ModuleTarget::External("zod".into())
    );
    assert_eq!(
        r.resolve("src/x.ts", "@scope/pkg/sub"),
        ModuleTarget::External("@scope/pkg".into())
    );
    assert_eq!(
        r.resolve("src/index.ts", "./missing"),
        ModuleTarget::Unresolved
    );
}

#[test]
fn the_fixture_config_chain_is_read() {
    let r = resolver_for("ts-basic");
    let sources: Vec<String> = r.config_sources().into_iter().collect();
    assert_eq!(sources, ["tsconfig.base.json", "tsconfig.json"]);
}

#[test]
fn tsconfig_with_comments_and_an_extends_loop_loads() {
    let (_dir, configs, _) = project(&[
        (
            "a/tsconfig.json",
            r#"{
  // the loop closes here
  "extends": "../b/tsconfig.json",
  "compilerOptions": { "paths": { "~/*": ["lib/*"], }, },
}"#,
        ),
        ("b/tsconfig.json", r#"{ "extends": "../a/tsconfig.json" }"#),
    ]);
    // The targets are resolved against the folder of the file that set them.
    assert!(
        configs.by_dir["a"]
            .paths
            .contains(&("~/*".to_string(), vec!["a/lib/*".to_string()]))
    );
}

#[test]
fn paths_resolve_against_the_file_that_set_them() {
    let (_dir, _, r) = project(&[
        (
            "a/tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "~/*": ["lib/*"] } } }"#,
        ),
        ("a/lib/util.ts", ""),
        ("a/src/main.ts", ""),
    ]);
    assert_eq!(r.resolve("a/src/main.ts", "~/util"), file("a/lib/util.ts"));
}

#[test]
fn a_child_replaces_inherited_paths_and_keeps_the_base_url() {
    let (_dir, _, r) = project(&[
        (
            "tsconfig.base.json",
            r#"{ "compilerOptions": { "baseUrl": "src", "paths": { "@old/*": ["old/*"] } } }"#,
        ),
        (
            "tsconfig.json",
            r#"{ "extends": "./tsconfig.base.json",
                 "compilerOptions": { "paths": { "@new/*": ["new/*"] } } }"#,
        ),
        ("src/new/a.ts", ""),
        ("src/old/a.ts", ""),
        ("src/util.ts", ""),
        ("src/main.ts", ""),
    ]);
    assert_eq!(r.resolve("src/main.ts", "@new/a"), file("src/new/a.ts"));
    // "@old/*" was replaced, so "@old/a" is a package again.
    assert_eq!(
        r.resolve("src/main.ts", "@old/a"),
        ModuleTarget::External("@old/a".into())
    );
    // baseUrl still comes from the base file.
    assert_eq!(r.resolve("src/main.ts", "util"), file("src/util.ts"));
}

#[test]
fn a_matching_pattern_without_a_target_is_unresolved_not_external() {
    let (_dir, _, r) = project(&[
        (
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["src/*"] } } }"#,
        ),
        ("src/main.ts", ""),
    ]);
    assert_eq!(r.resolve("src/main.ts", "@/nope"), ModuleTarget::Unresolved);
}

#[test]
fn the_nearest_ancestor_config_wins() {
    let (_dir, _, r) = project(&[
        (
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["root/*"] } } }"#,
        ),
        (
            "pkg/jsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["src/*"] } } }"#,
        ),
        ("root/a.ts", ""),
        ("pkg/src/a.ts", ""),
        ("pkg/src/main.ts", ""),
        ("main.ts", ""),
    ]);
    assert_eq!(r.resolve("pkg/src/main.ts", "@/a"), file("pkg/src/a.ts"));
    assert_eq!(r.resolve("main.ts", "@/a"), file("root/a.ts"));
}

#[test]
fn a_package_extends_is_noted_and_not_read() {
    let (_dir, configs, r) = project(&[(
        "tsconfig.json",
        r#"{ "extends": "@tsconfig/node20/tsconfig.json" }"#,
    )]);
    assert_eq!(
        configs.notes,
        ["tsconfig.json extends @tsconfig/node20/tsconfig.json, which Harbormap does not read"]
    );
    let sources: Vec<String> = r.config_sources().into_iter().collect();
    assert_eq!(sources, ["tsconfig.json"]);
}

#[test]
fn extension_probing_follows_the_documented_order() {
    let (_dir, _, r) = project(&[
        ("a.ts", ""),
        ("a.js", ""),
        ("b.js", ""),
        ("c/index.tsx", ""),
        ("d.d.ts", ""),
        ("e.ts", ""),
        ("main.ts", ""),
    ]);
    assert_eq!(r.resolve("main.ts", "./a"), file("a.ts"));
    assert_eq!(r.resolve("main.ts", "./a.js"), file("a.js")); // exact wins
    assert_eq!(r.resolve("main.ts", "./b.js"), file("b.js"));
    assert_eq!(r.resolve("main.ts", "./c"), file("c/index.tsx"));
    assert_eq!(r.resolve("main.ts", "./d"), file("d.d.ts"));
    assert_eq!(r.resolve("main.ts", "./e.js"), file("e.ts")); // ESM-style TS import
    assert_eq!(r.resolve("main.ts", "./e.json"), ModuleTarget::Unresolved);
}

#[test]
fn a_relative_path_above_the_root_is_unresolved() {
    let r = resolver_for("ts-basic");
    assert_eq!(
        r.resolve("src/index.ts", "../../../x"),
        ModuleTarget::Unresolved
    );
}

#[test]
fn a_broken_tsconfig_is_still_a_config_source() {
    let (_dir, configs, r) = project(&[("tsconfig.json", r#"{ "compilerOptions": "#)]);
    assert!(!configs.by_dir.contains_key(""));
    let notes: Vec<&String> = configs
        .notes
        .iter()
        .filter(|n| n.contains("tsconfig.json"))
        .collect();
    assert_eq!(notes.len(), 1, "{:?}", configs.notes);
    assert!(r.config_sources().contains("tsconfig.json"));
}

#[test]
fn a_missing_extends_target_is_a_config_source() {
    let (_dir, configs, r) = project(&[
        (
            "a/tsconfig.json",
            r#"{ "extends": "./base.json",
                 "compilerOptions": { "paths": { "~/*": ["lib/*"] } } }"#,
        ),
        ("a/lib/util.ts", ""),
        ("a/src/main.ts", ""),
    ]);
    assert!(r.config_sources().contains("a/base.json"));
    assert!(configs.notes.iter().any(|n| n.contains("a/base.json")));
    assert_eq!(r.resolve("a/src/main.ts", "~/util"), file("a/lib/util.ts"));
}

#[test]
fn notes_are_not_repeated() {
    let (_dir, configs, _) = project(&[
        ("a/tsconfig.json", r#"{ "extends": "../shared/base.json" }"#),
        ("b/tsconfig.json", r#"{ "extends": "../shared/base.json" }"#),
        ("shared/base.json", r#"{ "extends": "@tsconfig/node20" }"#),
    ]);
    let lines = configs
        .notes
        .iter()
        .filter(|n| n.contains("@tsconfig/node20"))
        .count();
    assert_eq!(lines, 1, "{:?}", configs.notes);
}

/// Extracts and resolves `files` (relative path, text) without touching the disk.
fn resolve_all(files: &[(&str, &str)]) -> (BTreeMap<String, FileFacts>, ModuleResolver) {
    let facts = files
        .iter()
        .map(|(rel, text)| {
            let lang = Lang::from_path(std::path::Path::new(rel)).unwrap();
            (rel.to_string(), hm_index::ts::extract(text, lang))
        })
        .collect();
    let known = files.iter().map(|(rel, _)| rel.to_string()).collect();
    (facts, ModuleResolver::new(known, TsConfigs::default()))
}

type EdgeTuple = (String, String, String, String);
type RefTuple = (String, String, String);

/// The resolution of `rel` among `files`, with edges as (kind, src, dst, rule) in
/// readable names (`path#Owner.name`, `path` for a file) and refs as (kind, text, status).
fn resolved(files: &[(&str, &str)], rel: &str) -> (Vec<EdgeTuple>, Vec<RefTuple>) {
    let (facts, modules) = resolve_all(files);
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let mut names = std::collections::HashMap::new();
    for (r, f) in &facts {
        names.insert(hm_core::ids::file_id(r), r.clone());
        for (id, s) in symbol_ids(r, f).into_iter().zip(&f.symbols) {
            names.insert(id, hm_index::query::display_name(r, Some(s)));
        }
    }
    let ResolvedFile { edges, refs, .. } = resolve_file(&project, rel);
    (
        edges
            .into_iter()
            .map(|e| {
                (
                    e.kind.to_string(),
                    names[&e.src_id].clone(),
                    names[&e.dst_id].clone(),
                    e.rule.to_string(),
                )
            })
            .collect(),
        refs.into_iter()
            .map(|r| (r.kind.to_string(), r.text, r.status.to_string()))
            .collect(),
    )
}

fn edge(kind: &str, src: &str, dst: &str, rule: &str) -> EdgeTuple {
    (kind.into(), src.into(), dst.into(), rule.into())
}

fn reference(kind: &str, text: &str, status: &str) -> RefTuple {
    (kind.into(), text.into(), status.into())
}

#[test]
fn fixture_calls_resolve_to_the_right_symbols() {
    let names = named_edges_for("ts-basic");
    for e in [
        (
            "calls",
            "src/index.ts#main",
            "src/orders/service.ts#placeOrder",
        ),
        (
            "calls",
            "src/orders/service.ts#placeOrder",
            "src/orders/mail.ts#sendReceipt",
        ),
        (
            "calls",
            "src/ui/Page.tsx#Page",
            "src/orders/service.ts#OrderService.find",
        ),
        (
            "calls",
            "src/ui/Page.tsx#Page",
            "src/orders/mail.ts#sendReceipt",
        ),
        ("calls", "src/ui/Page.tsx#Page", "src/ui/Button.tsx#Button"),
        (
            "calls",
            "src/orders/service.ts#OrderService.total",
            "src/orders/service.ts#OrderService.sum",
        ),
        (
            "calls",
            "src/legacy.js#run",
            "src/orders/service.ts#placeOrder",
        ),
        (
            "calls",
            "src/orders/service.test.ts",
            "src/orders/service.ts#placeOrder",
        ),
        ("imports", "src/ui/Page.tsx", "src/orders/index.ts"),
        ("imports", "src/orders/index.ts", "src/orders/mail.ts"),
    ] {
        assert!(
            names.edges.contains(&(e.0.into(), e.1.into(), e.2.into())),
            "missing {e:?}"
        );
    }
    let count = |kind: &str| names.edges.iter().filter(|(k, _, _)| k == kind).count();
    assert_eq!((count("calls"), count("imports")), (8, 9));

    let r = |file: &str, kind: &str, text: &str, status: &str| {
        (file.into(), kind.into(), text.into(), status.into())
    };
    for expected in [
        r("src/orders/service.ts", "call", "db.insert", "unresolved"),
        r("src/orders/service.ts", "call", "db.get", "unresolved"),
        r("src/orders/service.ts", "import", "zod", "external"),
        r("src/orders/service.ts", "call", "z.object", "external"),
        r(
            "src/orders/service.ts",
            "call",
            "z.object({}).parse",
            "external",
        ),
    ] {
        assert!(names.refs.contains(&expected), "missing {expected:?}");
    }
    assert!(
        !names
            .edges
            .iter()
            .any(|(_, _, dst)| dst.contains("db.ts#insert")),
        "no guessing"
    );
}

#[test]
fn a_reexport_loop_stops() {
    let (facts, modules) = resolve_all(&[
        ("a.ts", "export * from \"./b\";"),
        ("b.ts", "export * from \"./a\";"),
    ]);
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    assert_eq!(resolve_export(&project, "a.ts", "nothing"), None);
}

#[test]
fn exports_are_followed_through_aliases_stars_and_default() {
    let (facts, modules) = resolve_all(&[
        (
            "src/impl.ts",
            "export function real() {}\nexport default class Main {}\nfunction hidden() {}",
        ),
        (
            "src/mid.ts",
            "export { real as alias, default as Renamed } from \"./impl\";\nexport { hidden } from \"./impl\";",
        ),
        (
            "src/top.ts",
            "export * from \"./mid\";\nexport { default } from \"./impl\";",
        ),
        (
            "src/local.ts",
            "import { real } from \"./impl\";\nfunction own() {}\nexport { real as viaImport, own };\nexport default own;",
        ),
        ("src/ns.ts", "export * as inner from \"./impl\";"),
        ("src/chain0.ts", "export * from \"./chain1\";"),
        ("src/chain1.ts", "export * from \"./chain2\";"),
        ("src/chain2.ts", "export * from \"./chain3\";"),
        ("src/chain3.ts", "export * from \"./chain4\";"),
        ("src/chain4.ts", "export * from \"./chain5\";"),
        ("src/chain5.ts", "export * from \"./chain6\";"),
        ("src/chain6.ts", "export function deep() {}"),
        ("src/near.ts", "export * from \"./chain6\";"),
    ]);
    let p = Project {
        facts: &facts,
        modules: &modules,
    };
    let name_at = |found: Option<(String, usize)>| {
        found.map(|(rel, i)| format!("{rel}#{}", facts[&rel].symbols[i].name))
    };
    let at = |rel: &str, name: &str| name_at(resolve_export(&p, rel, name));

    assert_eq!(at("src/mid.ts", "alias"), Some("src/impl.ts#real".into()));
    assert_eq!(at("src/mid.ts", "Renamed"), Some("src/impl.ts#Main".into()));
    // A name that the other module does not export stays unresolved.
    assert_eq!(at("src/mid.ts", "hidden"), None);
    assert_eq!(at("src/top.ts", "alias"), Some("src/impl.ts#real".into()));
    assert_eq!(at("src/top.ts", "default"), Some("src/impl.ts#Main".into()));
    // `export *` never carries the default export.
    assert_eq!(at("src/mid.ts", "default"), None);
    assert_eq!(
        at("src/local.ts", "viaImport"),
        Some("src/impl.ts#real".into())
    );
    assert_eq!(at("src/local.ts", "own"), Some("src/local.ts#own".into()));
    assert_eq!(
        at("src/local.ts", "default"),
        Some("src/local.ts#own".into())
    );
    assert_eq!(at("src/ns.ts", "inner"), None);
    // Five hops are followed, a sixth is not.
    assert_eq!(
        at("src/chain2.ts", "deep"),
        Some("src/chain6.ts#deep".into())
    );
    assert_eq!(
        at("src/chain1.ts", "deep"),
        Some("src/chain6.ts#deep".into())
    );
    assert_eq!(at("src/chain0.ts", "deep"), None);
    assert_eq!(at("src/near.ts", "deep"), Some("src/chain6.ts#deep".into()));
}

#[test]
fn symbol_ids_disambiguate_only_when_needed() {
    let facts = hm_index::ts::extract(
        "class C { get x() { return 1; } set x(v: number) {} }\n\
         function f() { return 1; }\n\
         function g() {}",
        Lang::TypeScript,
    );
    let ids = symbol_ids("a.ts", &facts);
    assert_eq!(ids.len(), facts.symbols.len());
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
    let id_of = |name: &str| {
        let i = facts.symbols.iter().position(|s| s.name == name).unwrap();
        ids[i].clone()
    };
    let file = hm_core::ids::file_id("a.ts");
    // A unique symbol has an empty disambiguator.
    assert_eq!(
        id_of("f"),
        hm_core::ids::symbol_id(&file, "", "f", "function", "")
    );

    // Two symbols with the same owner, name, kind and signature hash get `#2`.
    let twice = hm_index::ts::extract("function h() {}\nfunction h() {}", Lang::TypeScript);
    let ids = symbol_ids("a.ts", &twice);
    assert_eq!(twice.symbols[0].sig_hash, twice.symbols[1].sig_hash);
    let hash = &twice.symbols[0].sig_hash;
    assert_eq!(
        ids[0],
        hm_core::ids::symbol_id(&file, "", "h", "function", hash)
    );
    assert_eq!(
        ids[1],
        hm_core::ids::symbol_id(&file, "", "h", "function", &format!("{hash}#2"))
    );
}

#[test]
fn local_static_new_and_namespace_calls_resolve() {
    let files = [
        (
            "src/m.ts",
            "export class K { static make() {} run() {} }\nexport function util() {}",
        ),
        (
            "src/use.ts",
            "import { K } from \"./m\";\nimport * as m from \"./m\";\n\
             function local() {}\n\
             class Own { static s() {} }\n\
             export function go() {\n  local();\n  K.make();\n  Own.s();\n  new K();\n  m.util();\n  m.nothing();\n  K.missing();\n}\n",
        ),
    ];
    let (edges, refs) = resolved(&files, "src/use.ts");
    for expected in [
        edge(
            "calls",
            "src/use.ts#go",
            "src/use.ts#local",
            "ts.call.local",
        ),
        edge(
            "calls",
            "src/use.ts#go",
            "src/m.ts#K.make",
            "ts.call.static",
        ),
        edge(
            "calls",
            "src/use.ts#go",
            "src/use.ts#Own.s",
            "ts.call.static",
        ),
        edge("calls", "src/use.ts#go", "src/m.ts#K", "ts.call.new"),
        edge(
            "calls",
            "src/use.ts#go",
            "src/m.ts#util",
            "ts.call.namespace",
        ),
        edge("imports", "src/use.ts", "src/m.ts", "ts.import"),
    ] {
        assert!(
            edges.contains(&expected),
            "missing {expected:?} in {edges:?}"
        );
    }
    // One edge per import line: `K` and `m` are imported on two lines.
    assert_eq!(edges.len(), 7, "{edges:?}");
    assert_eq!(
        refs,
        vec![
            reference("call", "K.missing", "unresolved"),
            reference("call", "m.nothing", "unresolved"),
        ]
    );
}

#[test]
fn this_calls_and_ambiguous_owners_are_attributed_by_line() {
    let files = [(
        "a.ts",
        "export class C {\n\
         \x20 get x() { return this.helper(); }\n\
         \x20 set x(v: number) { this.other(); }\n\
         \x20 helper() {}\n\
         \x20 other() {}\n\
         }\n\
         export interface Foo { a: number }\n\
         export class Foo { make() { return this.nothing(); } }\n\
         helper();\n",
    )];
    let (edges, refs) = resolved(&files, "a.ts");
    let helper = edges
        .iter()
        .find(|e| e.2 == "a.ts#C.helper")
        .expect("helper edge");
    assert_eq!(helper.1, "a.ts#C.x");
    assert_eq!(helper.3, "ts.call.this");
    assert!(edges.iter().any(|e| e.2 == "a.ts#C.other"));
    // `this.nothing()` resolves to nothing; `helper` is a method, not a module-level
    // name, so the bare `helper()` at top level is a local ref.
    assert!(refs.contains(&reference("call", "this.nothing", "unresolved")));
    assert!(refs.contains(&reference("call", "helper", "local")));
    assert!(!edges.iter().any(|e| e.2.ends_with("#nothing")));
}

#[test]
fn getter_and_setter_calls_are_split_by_line() {
    let (facts, modules) = resolve_all(&[(
        "a.ts",
        "function t() {}\nfunction u() {}\nclass C {\n  get x() { return t(); }\n  set x(v: number) { u(); }\n}\n",
    )]);
    let ids = symbol_ids("a.ts", &facts["a.ts"]);
    let id_of = |name: &str, nth: usize| {
        let i = facts["a.ts"]
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name == name)
            .nth(nth)
            .unwrap()
            .0;
        ids[i].clone()
    };
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let edges: BTreeSet<_> = resolve_file(&project, "a.ts")
        .edges
        .into_iter()
        .map(|e| (e.src_id, e.dst_id))
        .collect();
    let expected: BTreeSet<_> = [
        (id_of("x", 0), id_of("t", 0)),
        (id_of("x", 1), id_of("u", 0)),
    ]
    .into();
    assert_eq!(edges, expected);
}

#[test]
fn require_jsx_extends_and_type_only_imports() {
    let files = [
        ("lib.js", "export function f() {}"),
        (
            "base.ts",
            "export class Base {}\nexport default class Def {}",
        ),
        ("types.ts", "export function typed() {}"),
        ("ui.tsx", "export function Card() { return <i />; }"),
        (
            "app.tsx",
            "import Def, { Base } from \"./base\";\n\
             import type { typed } from \"./types\";\n\
             import { Card } from \"./ui\";\n\
             import { Component } from \"react\";\n\
             import { gone } from \"./gone\";\n\
             const { f } = require(\"./lib\");\n\
             export class A extends Base {}\n\
             export class B extends Def {}\n\
             export class D extends Component {}\n\
             export class E extends Missing {}\n\
             export function App() { f(); typed(); gone(); return <Card />; }\n",
        ),
    ];
    let (edges, refs) = resolved(&files, "app.tsx");
    for expected in [
        edge("extends", "app.tsx#A", "base.ts#Base", "ts.extends"),
        edge("extends", "app.tsx#B", "base.ts#Def", "ts.extends"),
        edge("calls", "app.tsx#App", "ui.tsx#Card", "ts.jsx"),
        edge("calls", "app.tsx#App", "lib.js#f", "ts.call.require"),
        // A type-only import is still a dependency of the file.
        edge("imports", "app.tsx", "types.ts", "ts.import"),
    ] {
        assert!(
            edges.contains(&expected),
            "missing {expected:?} in {edges:?}"
        );
    }
    // ... but it never feeds call resolution.
    assert!(
        !edges
            .iter()
            .any(|e| e.0 == "calls" && e.2 == "types.ts#typed")
    );
    for expected in [
        reference("call", "typed", "unresolved"),
        reference("call", "gone", "unresolved"),
        reference("extends", "Component", "external"),
        reference("extends", "Missing", "local"),
        reference("import", "./gone", "unresolved"),
        reference("import", "react", "external"),
    ] {
        assert!(refs.contains(&expected), "missing {expected:?} in {refs:?}");
    }
}

#[test]
fn resolution_is_sorted_and_deduplicated() {
    let files = [(
        "a.ts",
        "function f() {}\nexport function g() { f(); f(); other.x(); other.x(); }\n",
    )];
    let (facts, modules) = resolve_all(&files);
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let r = resolve_file(&project, "a.ts");
    // Two identical calls on one line are one edge; on different lines they stay apart.
    assert_eq!(r.edges.len(), 1);
    assert_eq!(r.refs.len(), 1);
    assert!(r.imports_files.is_empty());

    let files = [
        ("b.ts", "export function t() {}"),
        (
            "a.ts",
            "import { t } from \"./b\";\nimport { t as u } from \"./b\";\nt();\nu();\n",
        ),
    ];
    let (facts, modules) = resolve_all(&files);
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let r = resolve_file(&project, "a.ts");
    assert_eq!(r.imports_files, ["b.ts".to_string()].into());
    let keys: Vec<_> = r
        .edges
        .iter()
        .map(|e| (e.kind, e.src_id.clone(), e.dst_id.clone(), e.line))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    sorted.dedup();
    assert_eq!(keys, sorted);
}

#[test]
fn an_unbound_extends_is_a_local_extends_ref() {
    let (edges, refs) = resolved(&[("a.ts", "class A extends Missing {}\n")], "a.ts");
    assert!(edges.is_empty(), "{edges:?}");
    assert_eq!(refs, [reference("extends", "Missing", "local")]);
}

#[test]
fn only_the_exported_name_resolves_through_an_alias() {
    let (facts, modules) = resolve_all(&[
        ("alias.ts", "function a() {}\nexport { a as b };\n"),
        (
            "swap.ts",
            "function a() {}\nfunction b() {}\nexport { a as b, b as a };\n",
        ),
        ("decl.ts", "export function c() {}\nexport { c as d };\n"),
    ]);
    let p = Project {
        facts: &facts,
        modules: &modules,
    };
    let at = |rel: &str, name: &str| {
        resolve_export(&p, rel, name).map(|(r, i)| format!("{r}#{}", facts[&r].symbols[i].name))
    };
    assert_eq!(at("alias.ts", "b"), Some("alias.ts#a".into()));
    assert_eq!(at("alias.ts", "a"), None);
    assert_eq!(at("swap.ts", "b"), Some("swap.ts#a".into()));
    assert_eq!(at("swap.ts", "a"), Some("swap.ts#b".into()));
    // Exported by its declaration and under a second name: both resolve.
    assert_eq!(at("decl.ts", "c"), Some("decl.ts#c".into()));
    assert_eq!(at("decl.ts", "d"), Some("decl.ts#c".into()));
}

#[test]
fn a_call_in_one_of_two_same_line_accessors_goes_to_the_file() {
    let files = [(
        "a.ts",
        "function f() {}\nclass C { get x() { return f(); } set x(v: number) { f(); } }\n",
    )];
    let (edges, refs) = resolved(&files, "a.ts");
    // Nothing says which accessor holds the call, so the file does, not a guess.
    assert_eq!(edges, [edge("calls", "a.ts", "a.ts#f", "ts.call.local")]);
    assert!(refs.is_empty(), "{refs:?}");
}

#[test]
fn an_extends_without_a_class_to_hold_it_is_a_ref() {
    let (mut facts, modules) = resolve_all(&[
        ("base.ts", "export class Base {}\n"),
        (
            "a.ts",
            "import { Base } from \"./base\";\nconst K = class extends Base {};\n",
        ),
    ]);
    // The extractor emits heritage only for a named top-level class, so a heritage
    // fact with no class symbol to hold it is built by hand.
    assert!(facts["a.ts"].heritage.is_empty());
    facts.get_mut("a.ts").unwrap().heritage.push(HeritageFact {
        class: "K".into(),
        extends: "Base".into(),
        line: 2,
    });
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let r = resolve_file(&project, "a.ts");
    let file = hm_core::ids::file_id("a.ts");
    assert!(
        !r.edges.iter().any(|e| e.kind == "extends"),
        "an extends edge starts at the file {file}: {:?}",
        r.edges
    );
    let refs: Vec<_> = r
        .refs
        .iter()
        .map(|x| (x.kind, x.text.as_str(), x.status))
        .collect();
    assert!(
        refs.contains(&("extends", "Base", "unresolved")),
        "{refs:?}"
    );
}

#[test]
fn an_extends_chain_cut_at_the_depth_limit_is_noted() {
    // tsconfig.json → c1.json → … → c12.json: more hops than are followed.
    let mut files: Vec<(String, String)> = vec![(
        "tsconfig.json".to_string(),
        r#"{ "extends": "./c1.json" }"#.to_string(),
    )];
    for k in 1..=12 {
        files.push((
            format!("c{k}.json"),
            format!(r#"{{ "extends": "./c{}.json" }}"#, k + 1),
        ));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(rel, text)| (rel.as_str(), text.as_str()))
        .collect();
    let (_dir, configs, _) = project(&files);
    assert!(
        configs
            .notes
            .iter()
            .any(|n| n.contains("tsconfig.json") && n.contains("10")),
        "{:?}",
        configs.notes
    );
}

#[test]
fn globals_are_builtin_refs() {
    let files = [(
        "a.ts",
        "export function run(s: string, f: () => void) {\n\
         \x20 JSON.parse(s);\n\
         \x20 new Date();\n\
         \x20 console.log(s);\n\
         \x20 setTimeout(f);\n\
         \x20 document.querySelector('a');\n\
         \x20 process.exit(1);\n\
         \x20 Object.keys(s).map(f);\n\
         }\n",
    )];
    let (edges, refs) = resolved(&files, "a.ts");
    assert!(edges.is_empty(), "{edges:?}");
    assert_eq!(
        refs,
        vec![
            reference("call", "Date", "builtin"),
            reference("call", "JSON.parse", "builtin"),
            reference("call", "Object.keys", "builtin"),
            reference("call", "Object.keys(s).map", "builtin"),
            reference("call", "console.log", "builtin"),
            reference("call", "document.querySelector", "builtin"),
            reference("call", "process.exit", "builtin"),
            reference("call", "setTimeout", "builtin"),
        ]
    );
}

#[test]
fn calls_on_local_values_are_local_refs() {
    let files = [(
        "a.ts",
        "export function f(items: number[]) {\n\
         \x20 items.map(g);\n\
         \x20 const h = () => 1;\n\
         \x20 h();\n\
         \x20 items.filter(Boolean).length.toString();\n\
         }\n\
         function g() {}\n",
    )];
    let (_, refs) = resolved(&files, "a.ts");
    for expected in [
        reference("call", "items.map", "local"),
        reference("call", "h", "local"),
    ] {
        assert!(refs.contains(&expected), "missing {expected:?} in {refs:?}");
    }
    assert!(
        refs.iter().all(|r| r.2 != "unresolved"),
        "no module-level name is involved: {refs:?}"
    );
}

#[test]
fn module_level_names_that_do_not_resolve_stay_unresolved() {
    let files = [(
        "a.ts",
        "import { a } from './missing';\n\
         import def, * as ns from './missing';\n\
         const db = make();\n\
         function make() { return { insert() {} }; }\n\
         export function go() {\n\
         \x20 a();\n\
         \x20 db.insert();\n\
         \x20 ns.thing();\n\
         \x20 def();\n\
         \x20 this.nothing();\n\
         }\n\
         class K extends Missing {}\n\
         class L extends Object {}\n\
         class M extends other.Base {}\n",
    )];
    let (_, refs) = resolved(&files, "a.ts");
    for expected in [
        reference("call", "a", "unresolved"),
        reference("call", "db.insert", "unresolved"),
        reference("call", "ns.thing", "unresolved"),
        reference("call", "def", "unresolved"),
        reference("call", "this.nothing", "unresolved"),
    ] {
        assert!(refs.contains(&expected), "missing {expected:?} in {refs:?}");
    }
    // `make` resolves to an edge, so it is not a ref at all.
    assert!(!refs.iter().any(|r| r.1 == "make"), "{refs:?}");
    // Heritage follows the same rules.
    assert!(refs.contains(&reference("extends", "Missing", "local")));
    assert!(refs.contains(&reference("extends", "Object", "builtin")));
    assert!(refs.contains(&reference("extends", "other.Base", "local")));
}

#[test]
fn a_binding_shadows_a_global_name() {
    let files = [(
        "a.ts",
        "import { fetch } from './missing';\n\
         function Date() {}\n\
         export const console = make();\n\
         function make() { return { log() {} }; }\n\
         export function go() {\n\
         \x20 fetch('/x');\n\
         \x20 console.log(1);\n\
         \x20 JSON.parse('1');\n\
         }\n",
    )];
    let (_, refs) = resolved(&files, "a.ts");
    assert!(refs.contains(&reference("call", "fetch", "unresolved")));
    assert!(refs.contains(&reference("call", "console.log", "unresolved")));
    assert!(refs.contains(&reference("call", "JSON.parse", "builtin")));
}

#[test]
fn names_bound_by_other_top_level_statements_stay_unresolved() {
    let files = [(
        "a.ts",
        "const { a, b: [c, ...d], e = 1, ...rest } = make();\n\
         let [x, , y = 2] = make();\n\
         var legacy = make();\n\
         const prisma = new PrismaClient();\n\
         const schema = z.object({});\n\
         enum Color { Red }\n\
         namespace N { export function inner() {} }\n\
         import fs = require('fs');\n\
         declare const ambient: any;\n\
         declare function amb(): void;\n\
         export const { exp } = make();\n\
         function make(): any { return {}; }\n\
         export function go(local: any) {\n\
         \x20 a(); c(); d(); e(); rest.f(); x(); y(); legacy.g(); prisma.h();\n\
         \x20 schema.parse(1); Color.Red.toString(); N.inner(); fs.readFile();\n\
         \x20 ambient.k(); amb(); exp.m(); local.n(); other();\n\
         }\n",
    )];
    let (_, refs) = resolved(&files, "a.ts");
    let status = |callee: &str| {
        refs.iter()
            .find(|r| r.0 == "call" && r.1 == callee)
            .map(|r| r.2.clone())
            .unwrap_or_else(|| panic!("no ref {callee} in {refs:?}"))
    };
    for callee in [
        "a",
        "c",
        "d",
        "e",
        "rest.f",
        "x",
        "y",
        "legacy.g",
        "prisma.h",
        "schema.parse",
        "Color.Red.toString",
        "fs.readFile",
        "ambient.k",
        "amb",
        "exp.m",
    ] {
        assert_eq!(status(callee), "unresolved", "{callee}");
    }
    // `N.inner` may resolve to an edge; it is never `local`.
    assert!(
        !refs.iter().any(|r| r.1 == "N.inner" && r.2 == "local"),
        "{refs:?}"
    );
    // Not bound at module level.
    assert_eq!(status("local.n"), "local");
    assert_eq!(status("other"), "local");
}

#[test]
fn a_module_name_shadows_a_global() {
    let files = [(
        "a.ts",
        "const { process } = make();\n\
         enum Date { A }\n\
         function make(): any { return {}; }\n\
         export function go() { process.exit(1); Date.A.toString(); JSON.parse('1'); }\n",
    )];
    let (_, refs) = resolved(&files, "a.ts");
    assert!(
        refs.contains(&reference("call", "process.exit", "unresolved")),
        "{refs:?}"
    );
    assert!(
        refs.contains(&reference("call", "Date.A.toString", "unresolved")),
        "{refs:?}"
    );
    assert!(
        refs.contains(&reference("call", "JSON.parse", "builtin")),
        "{refs:?}"
    );
}
