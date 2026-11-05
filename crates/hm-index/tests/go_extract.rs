mod common;

use common::read_fixture;
use hm_index::facts::{ImportKind, LabelFact, ParseStatus, SymbolKind};
use hm_index::go::extract;

#[test]
fn the_go_grammar_loads_under_the_pinned_core() {
    assert_eq!(
        extract("package a\n\nfunc F() {}\n").status,
        ParseStatus::Parsed
    );
}

#[test]
fn server_go_yields_its_package_symbols_and_calls() {
    let f = extract(&read_fixture("go-basic/http/server.go"));
    assert_eq!(f.status, ParseStatus::Parsed);
    assert_eq!(f.package.as_deref(), Some("web"));

    let names: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("", "Server", SymbolKind::Class),
            ("", "NewServer", SymbolKind::Function),
            ("Server", "routes", SymbolKind::Method),
            ("Server", "ServeHTTP", SymbolKind::Method),
            ("Server", "handleOrder", SymbolKind::Method),
        ]
    );
    let new_server = &f.symbols[1];
    assert_eq!(
        (new_server.l0, new_server.l1, new_server.exported),
        (15, 19, true)
    );
    assert!(!f.symbols[2].exported);
    assert!(f.symbols[3].exported);

    let imports: Vec<_> = f
        .imports
        .iter()
        .map(|i| (i.specifier.as_str(), i.names[0].local.as_str()))
        .collect();
    assert_eq!(imports, vec![("net/http", ""), ("example.com/shop", "")]);
    assert!(f.imports.iter().all(|i| i.kind == ImportKind::Static));
    assert_eq!(f.imports[0].names[0].imported, "*");

    let calls: Vec<_> = f
        .calls
        .iter()
        .map(|c| (c.callee.as_str(), c.receiver.as_deref(), c.line))
        .collect();
    for want in [
        ("routes", Some("s"), 17),
        ("handleOrder", Some("s"), 26),
        ("Validate", Some("shop"), 31),
        ("Save", Some("s.Store"), 35),
        ("Notify", Some("s.Notifier"), 36),
        ("writeJSON", None, 37),
    ] {
        assert!(calls.contains(&want), "missing {want:?} in {calls:?}");
    }
    let write = f.calls.iter().find(|c| c.callee == "writeJSON").unwrap();
    assert_eq!(
        write.owner,
        Some(("Server".to_string(), "handleOrder".to_string()))
    );
    assert_eq!(write.args, 2);

    let handle = f.symbols.iter().find(|s| s.name == "handleOrder").unwrap();
    assert_eq!(handle.branches, 1);
}

#[test]
fn kinds_and_receivers_in_shop_and_store() {
    let shop = extract(&read_fixture("go-basic/shop.go"));
    let kinds: Vec<_> = shop
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind))
        .collect();
    // An interface's method specs are methods owned by the interface.
    assert_eq!(
        kinds,
        vec![
            ("", "Order", SymbolKind::Class),
            ("", "OrderStore", SymbolKind::Interface),
            ("OrderStore", "Save", SymbolKind::Method),
            ("OrderStore", "Find", SymbolKind::Method),
            ("", "Notifier", SymbolKind::Interface),
            ("Notifier", "Notify", SymbolKind::Method),
            ("", "Validate", SymbolKind::Function),
        ]
    );
    let save = &shop.symbols[2];
    assert_eq!(save.sig, "Save(o *Order) error");
    // No body: every method spec has the hash of nothing.
    assert_eq!(save.body_hash, shop.symbols[3].body_hash);
    assert_eq!(save.body_hash, shop.symbols[5].body_hash);
    assert!(save.exported);
    assert_eq!((save.l0, save.l1), (14, 14));

    let store = extract(&read_fixture("go-basic/store/memory.go"));
    let methods: Vec<_> = store
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Method)
        .map(|s| (s.owner.as_str(), s.name.as_str()))
        .collect();
    assert_eq!(
        methods,
        vec![("Memory", "Save"), ("Memory", "Find"), ("Memory", "lookup")]
    );
    let lookup = store.symbols.iter().find(|s| s.name == "lookup").unwrap();
    assert_eq!(lookup.sig, "func (m *Memory) lookup(id int) *shop.Order");
    assert!(!lookup.exported);
}

#[test]
fn a_closure_assigned_to_a_name_is_a_label() {
    let f = extract(&read_fixture("go-basic/cmd/shopd/main.go"));
    assert_eq!(
        f.labels,
        vec![LabelFact {
            owner: Some((String::new(), "main".to_string())),
            name: "handler".to_string(),
            l0: 14,
            l1: 17,
        }]
    );
    let serve = f.calls.iter().find(|c| c.callee == "ServeHTTP").unwrap();
    assert_eq!(serve.receiver.as_deref(), Some("srv"));
    assert_eq!(serve.line, 16);
    assert_eq!(serve.owner, Some((String::new(), "main".to_string())));
    let shop_http = f
        .imports
        .iter()
        .find(|i| i.specifier == "example.com/shop/http")
        .unwrap();
    assert_eq!(shop_http.names[0].local, "");
}

#[test]
fn labels_cover_keyed_values_and_skip_unnamed_literals() {
    let src = "package a\n\nvar h = func() {}\n\nfunc f() {\n\tx := T{Run: func() {}}\n\tgo func() {}()\n\t_ = x\n}\n";
    let f = extract(src);
    assert_eq!(
        f.labels,
        vec![
            LabelFact {
                owner: None,
                name: "h".to_string(),
                l0: 3,
                l1: 3,
            },
            LabelFact {
                owner: Some((String::new(), "f".to_string())),
                name: "Run".to_string(),
                l0: 6,
                l1: 6,
            },
        ]
    );
}

#[test]
fn aliases_dot_and_blank_imports_keep_their_names() {
    let f = extract("package a\n\nimport (\n\tweb \"a/http\"\n\t. \"a/b\"\n\t_ \"a/c\"\n)\n");
    let locals: Vec<_> = f
        .imports
        .iter()
        .map(|i| (i.specifier.as_str(), i.names[0].local.as_str()))
        .collect();
    assert_eq!(locals, vec![("a/http", "web"), ("a/b", "."), ("a/c", "_")]);
}

#[test]
fn a_go_syntax_error_keeps_the_functions_that_parse() {
    let f = extract("package a\n\nfunc Ok() {}\n\nfunc Half( {\n");
    assert_eq!(f.status, ParseStatus::ParsedWithErrors);
    assert!(f.symbols.iter().any(|s| s.name == "Ok"));
}

#[test]
fn a_comment_edit_keeps_the_go_body_hash() {
    let a = extract("package a\n\nfunc F() string {\n\treturn \"x\"\n}\n");
    let b = extract("package a\n\nfunc F() string {\n\t// note\n\treturn \"x\"\n}\n");
    let c = extract("package a\n\nfunc F() string {\n\treturn \"y\"\n}\n");
    assert_eq!(a.symbols[0].body_hash, b.symbols[0].body_hash);
    assert_eq!(a.symbols[0].sig_hash, b.symbols[0].sig_hash);
    assert_ne!(a.symbols[0].body_hash, c.symbols[0].body_hash);
    assert_eq!(a.symbols[0].sig_hash, c.symbols[0].sig_hash);
}

#[test]
fn generic_receivers_and_aliases_name_their_type() {
    let f = extract(
        "package a\n\ntype List[T any] struct{}\n\ntype Id = int\n\nfunc (l *List[T]) Len() int { return 0 }\n",
    );
    let got: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind))
        .collect();
    assert_eq!(
        got,
        vec![
            ("", "List", SymbolKind::Class),
            ("", "Id", SymbolKind::Class),
            ("List", "Len", SymbolKind::Method),
        ]
    );
}

#[test]
fn interface_method_specs_are_methods_and_embedded_types_are_not() {
    let f = extract(
        "package a\n\nimport \"io\"\n\ntype RW interface {\n\tio.Reader\n\tClose() error\n\tlower(n int)\n}\n\ntype T struct{}\n\nfunc (t T) Close() error { return nil }\n",
    );
    let got: Vec<_> = f
        .symbols
        .iter()
        .map(|s| (s.owner.as_str(), s.name.as_str(), s.kind, s.exported))
        .collect();
    assert_eq!(
        got,
        vec![
            ("", "RW", SymbolKind::Interface, true),
            ("RW", "Close", SymbolKind::Method, true),
            ("RW", "lower", SymbolKind::Method, false),
            ("", "T", SymbolKind::Class, true),
            ("T", "Close", SymbolKind::Method, true),
        ]
    );
}
