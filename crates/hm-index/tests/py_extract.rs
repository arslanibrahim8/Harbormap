mod common;

use common::read_fixture;
use hm_index::facts::{HeritageFact, ImportKind, ImportedName, LabelFact, ParseStatus, SymbolKind};
use hm_index::py::extract;

fn name(imported: &str, local: &str) -> ImportedName {
    ImportedName {
        imported: imported.into(),
        local: local.into(),
    }
}

#[test]
fn the_python_grammar_loads_under_the_pinned_core() {
    assert_eq!(extract("def f():\n    pass\n").status, ParseStatus::Parsed);
}

#[test]
fn service_py_yields_symbols_imports_calls_attrs_and_labels() {
    let f = extract(&read_fixture("py-basic/src/shop/service.py"));
    assert_eq!(f.status, ParseStatus::Parsed);
    let symbols: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind))
        .collect();
    assert_eq!(
        symbols,
        vec![
            ("", "log", SymbolKind::Variable),
            ("", "Service", SymbolKind::Class),
            ("Service", "__init__", SymbolKind::Method),
            ("Service", "place", SymbolKind::Method),
            ("Service", "_audit", SymbolKind::Method),
            ("", "place_order", SymbolKind::Function),
        ]
    );
    let place_order = f.symbols.last().unwrap();
    assert_eq!(
        (place_order.l0, place_order.l1, place_order.exported),
        (28, 34, true)
    );
    assert_eq!(place_order.sig, "def place_order(total):");

    let imports: Vec<_> = f
        .imports
        .iter()
        .map(|i| (i.specifier.as_str(), i.names.clone(), i.kind))
        .collect();
    assert_eq!(
        imports,
        vec![
            ("logging", vec![name("*", "logging")], ImportKind::Static),
            ("shop.repo", vec![name("Repo", "Repo")], ImportKind::Static),
            (".", vec![name("models", "models")], ImportKind::Static),
            (
                ".mailer",
                vec![name("Mailer", "Mailer")],
                ImportKind::Static
            ),
        ]
    );

    let calls: Vec<_> = f
        .calls
        .iter()
        .map(|c| (c.callee.as_str(), c.receiver.as_deref(), c.line))
        .collect();
    for want in [
        ("Repo", None, 14),
        ("Order", Some("models"), 18),
        ("add", Some("self.repo"), 19),
        ("send", Some("self.mailer"), 20),
        ("_audit", Some("self"), 21),
        ("info", Some("log"), 25),
        ("check", None, 32),
        ("ValueError", None, 33),
        ("place", Some("Service(Mailer())"), 34),
    ] {
        assert!(calls.contains(&want), "missing call {want:?} in {calls:?}");
    }

    let attrs: Vec<_> = f
        .attrs
        .iter()
        .map(|a| {
            (
                a.class.as_str(),
                a.attr.as_str(),
                a.type_name.as_str(),
                a.line,
            )
        })
        .collect();
    assert_eq!(
        attrs,
        vec![
            ("Service", "repo", "Repo", 11),
            ("Service", "repo", "Repo", 14),
            ("Service", "mailer", "Mailer", 15),
        ]
    );

    assert_eq!(
        f.labels,
        vec![LabelFact {
            owner: Some(("".into(), "place_order".into())),
            name: "check".into(),
            l0: 29,
            l1: 30,
        }]
    );
    assert!(f.exports.is_empty());
    assert_eq!(f.package, None);
}

#[test]
fn models_py_has_heritage_and_init_attrs() {
    let f = extract(&read_fixture("py-basic/src/shop/models.py"));
    assert_eq!(
        f.heritage,
        vec![HeritageFact {
            class: "Order".into(),
            extends: "Base".into(),
            line: 9
        }]
    );
    assert!(f.attrs.iter().any(|a| (
        a.class.as_str(),
        a.attr.as_str(),
        a.type_name.as_str(),
        a.line
    ) == ("Order", "total", "int", 11)));
}

#[test]
fn decorator_calls_belong_to_the_defining_scope() {
    let f = extract(&read_fixture("py-basic/src/shop/api.py"));
    let create = f.symbols.iter().find(|s| s.name == "create").unwrap();
    assert_eq!((create.l0, create.l1), (13, 17));
    assert!(
        create
            .sig
            .starts_with("@route(\"/orders\") def create(total):")
    );
    let route = f
        .calls
        .iter()
        .find(|c| c.callee == "route" && c.line == 13)
        .expect("the decorator call");
    assert_eq!(route.owner, None);
    assert!(f.labels.contains(&LabelFact {
        owner: Some(("".into(), "route".into())),
        name: "wrap".into(),
        l0: 7,
        l1: 8,
    }));
    assert_eq!(f.imports[0].specifier, "shop.models");
    assert_eq!(f.imports[0].names, vec![name("*", "shop")]);
    let inner = f
        .calls
        .iter()
        .find(|c| c.callee == "place_order")
        .expect("the call in the body");
    assert_eq!(inner.owner, Some(("".into(), "create".into())));
}

#[test]
fn bases_skip_keywords_and_annotations_unwrap_optional() {
    let f = extract(
        "class U(Base, mixins.M, table=True):\n    a: Optional[Repo] = None\n    b: \"Repo\"\n    c: Repo | None\n",
    );
    let extends: Vec<_> = f.heritage.iter().map(|h| h.extends.as_str()).collect();
    assert_eq!(extends, vec!["Base", "mixins.M"]);
    let attrs: Vec<_> = f
        .attrs
        .iter()
        .map(|a| (a.attr.as_str(), a.type_name.as_str()))
        .collect();
    assert_eq!(attrs, vec![("a", "Repo"), ("c", "Repo")]);
}

#[test]
fn relative_star_and_future_imports() {
    let f = extract(
        "from __future__ import annotations\nfrom ..pkg.mod import a as b, c\nfrom x import *\nimport os, sys as system\n",
    );
    let imports: Vec<_> = f
        .imports
        .iter()
        .map(|i| (i.specifier.as_str(), i.names.clone()))
        .collect();
    assert_eq!(
        imports,
        vec![
            ("..pkg.mod", vec![name("a", "b"), name("c", "c")]),
            ("x", vec![name("*", "*")]),
            ("os", vec![name("*", "os")]),
            ("sys", vec![name("*", "system")]),
        ]
    );
}

#[test]
fn a_python_syntax_error_keeps_the_functions_that_parse() {
    let f = extract("def ok():\n    return 1\n\ndef half(:\n");
    assert_eq!(f.status, ParseStatus::ParsedWithErrors);
    assert!(f.symbols.iter().any(|s| s.name == "ok"));
}

#[test]
fn a_stub_file_parses() {
    let f = extract(&read_fixture("py-basic/src/shop/fast.pyi"));
    assert_eq!(f.status, ParseStatus::Parsed);
    let s = f
        .symbols
        .iter()
        .find(|s| s.name == "speedup")
        .expect("speedup");
    assert_eq!(s.kind, SymbolKind::Function);
    assert_eq!(s.sig, "def speedup(x: int) -> int:");
}

#[test]
fn lambdas_async_and_decorated_methods() {
    let f = extract(
        "f = lambda x: x\n_hidden = 1\nX: int = 1\n\nasync def g():\n    h = lambda: 1\n\nclass C:\n    @deco(1)\n    def m(self):\n        pass\n",
    );
    let symbols: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind, s.l0, s.l1))
        .collect();
    assert_eq!(
        symbols,
        vec![
            ("", "f", SymbolKind::Function, 1, 1),
            ("", "X", SymbolKind::Variable, 3, 3),
            ("", "g", SymbolKind::Function, 5, 6),
            ("", "C", SymbolKind::Class, 8, 11),
            ("C", "m", SymbolKind::Method, 9, 11),
        ]
    );
    assert_eq!(f.symbols[1].sig, "X: int =");
    assert_eq!(f.symbols[2].sig, "async def g():");
    assert_eq!(f.labels.len(), 1);
    assert_eq!(f.labels[0].name, "h");
    let deco = f.calls.iter().find(|c| c.callee == "deco").unwrap();
    assert_eq!(deco.owner, Some(("".into(), "C".into())));
}
