mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

use common::fixture;
use hm_core::ids::file_id;
use hm_index::facts::FileFacts;
use hm_index::go::gomod::{GoModules, GoTarget, module_path};
use hm_index::go::resolve::{
    GoProject, MethodIndex, calls_any_method, method_names, package_siblings, resolve_file,
};
use hm_index::query::display_name;
use hm_index::ts::resolve::symbol_ids;

/// Resolved edges and refs with ids mapped back to readable names.
#[derive(Debug, Default)]
struct Named {
    /// (kind, source, destination, rule, inferred).
    edges: BTreeSet<(String, String, String, String, bool)>,
    /// (file, kind, text, status).
    refs: BTreeSet<(String, String, String, String)>,
    /// (file, imported file).
    imports_files: BTreeSet<(String, String)>,
}

impl Named {
    fn has_edge(&self, kind: &str, src: &str, dst: &str, rule: &str, inferred: bool) -> bool {
        self.edges
            .contains(&(kind.into(), src.into(), dst.into(), rule.into(), inferred))
    }

    fn has_ref(&self, file: &str, kind: &str, text: &str, status: &str) -> bool {
        self.refs
            .contains(&(file.into(), kind.into(), text.into(), status.into()))
    }
}

/// Resolves every `.go` file of `facts` and names the ids.
fn resolve_all(facts: &BTreeMap<String, FileFacts>, modules: &GoModules) -> Named {
    let mut names: HashMap<String, String> = HashMap::new();
    for (rel, file_facts) in facts {
        names.insert(file_id(rel), display_name(rel, None));
        for (id, sym) in symbol_ids(rel, file_facts)
            .into_iter()
            .zip(&file_facts.symbols)
        {
            names.insert(id, display_name(rel, Some(sym)));
        }
    }
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let methods = MethodIndex::build(facts);
    let project = GoProject {
        facts,
        modules,
        methods: &methods,
    };
    let mut out = Named::default();
    for rel in facts.keys().filter(|r| r.ends_with(".go")) {
        let resolved = resolve_file(&project, rel);
        for e in resolved.edges {
            out.edges.insert((
                e.kind.into(),
                name_of(&e.src_id),
                name_of(&e.dst_id),
                e.rule.into(),
                e.inferred,
            ));
        }
        for r in resolved.refs {
            out.refs
                .insert((rel.clone(), r.kind.into(), r.text, r.status.into()));
        }
        for f in resolved.imports_files {
            out.imports_files.insert((rel.clone(), f));
        }
    }
    out
}

/// Walks fixture `name`, extracts every `.go` file, loads its `go.mod` files and
/// resolves every Go file.
fn resolve_fixture(name: &str) -> Named {
    let root = fixture(name);
    let walked = hm_index::walk::walk(&root).expect("walk the fixture");
    let known: BTreeSet<String> = walked.files.iter().map(|f| f.rel.clone()).collect();
    let mut facts = BTreeMap::new();
    for f in walked.files.iter().filter(|f| f.rel.ends_with(".go")) {
        let src = fs::read_to_string(&f.abs).expect("read a fixture file");
        facts.insert(f.rel.clone(), hm_index::go::extract(&src));
    }
    let modules = GoModules::load(&root, &known);
    assert!(modules.notes().is_empty(), "{:?}", modules.notes());
    resolve_all(&facts, &modules)
}

/// Extracts `files` (relative path, source) and resolves them under one module
/// `example.com/shop` at the root.
fn resolve_sources(files: &[(&str, &str)]) -> Named {
    let facts: BTreeMap<String, FileFacts> = files
        .iter()
        .map(|(rel, src)| (rel.to_string(), hm_index::go::extract(src)))
        .collect();
    let mut known: BTreeSet<String> = facts.keys().cloned().collect();
    known.insert("go.mod".into());
    let modules = GoModules::new(
        known,
        BTreeMap::from([(String::new(), "example.com/shop".to_string())]),
        Vec::new(),
    );
    resolve_all(&facts, &modules)
}

#[test]
fn go_basic_resolves_package_receiver_and_method_calls() {
    let n = resolve_fixture("go-basic");
    let sure = [
        ("imports", "store/memory.go", "shop.go", "go.import"),
        (
            "imports",
            "cmd/shopd/main.go",
            "http/server.go",
            "go.import",
        ),
        ("imports", "cmd/shopd/main.go", "http/json.go", "go.import"),
        (
            "calls",
            "http/server.go#Server.handleOrder",
            "shop.go#Validate",
            "go.call.package",
        ),
        (
            "calls",
            "http/server.go#Server.handleOrder",
            "http/json.go#writeJSON",
            "go.call.local",
        ),
        (
            "calls",
            "http/server.go#Server.ServeHTTP",
            "http/server.go#Server.handleOrder",
            "go.call.receiver",
        ),
        (
            "calls",
            "store/memory.go#Memory.Find",
            "store/memory.go#Memory.lookup",
            "go.call.receiver",
        ),
        (
            "calls",
            "notify/email.go#Email.Notify",
            "notify/sms.go#send",
            "go.call.local",
        ),
        (
            "calls",
            "cmd/shopd/main.go#main",
            "store/memory.go#NewMemory",
            "go.call.package",
        ),
        (
            "calls",
            "cmd/shopd/main.go#main",
            "http/server.go#NewServer",
            "go.call.package",
        ),
        (
            "calls",
            "store/memory_test.go#TestSave",
            "store/memory.go#NewMemory",
            "go.call.local",
        ),
        (
            "calls",
            "store/memory_test.go#TestSave",
            "store/memory_test.go#helper",
            "go.call.local",
        ),
    ];
    for (kind, src, dst, rule) in sure {
        assert!(
            n.has_edge(kind, src, dst, rule, false),
            "missing {kind} {src} -> {dst} ({rule}) in {:#?}",
            n.edges
        );
    }
    let probably = [
        ("http/server.go#NewServer", "http/server.go#Server.routes"),
        (
            "http/server.go#Server.handleOrder",
            "store/memory.go#Memory.Save",
        ),
        ("cmd/shopd/main.go#main", "http/server.go#Server.ServeHTTP"),
    ];
    for (src, dst) in probably {
        assert!(
            n.has_edge("calls", src, dst, "go.call.method", true),
            "missing method call {src} -> {dst} in {:#?}",
            n.edges
        );
    }
    // Only `go.call.method` is inferred.
    for (_, _, _, rule, inferred) in &n.edges {
        assert_eq!(*inferred, rule == "go.call.method", "{rule}");
    }

    for (file, kind, text, status) in [
        ("http/server.go", "call", "s.Notifier.Notify", "unresolved"),
        ("http/server.go", "call", "s.mux.HandleFunc", "unresolved"),
        ("http/server.go", "call", "http.Error", "external"),
        ("http/server.go", "import", "net/http", "external"),
        ("http/json.go", "call", "json.NewEncoder", "external"),
    ] {
        assert!(
            n.has_ref(file, kind, text, status),
            "missing ref {file} {kind} {text} {status} in {:#?}",
            n.refs
        );
    }
    assert!(
        n.refs.iter().all(|(_, _, text, _)| text != "make"),
        "{:#?}",
        n.refs
    );
    assert!(
        n.edges
            .iter()
            .all(|(_, src, dst, _, _)| !src.contains("make") && !dst.contains("make"))
    );
    assert!(
        !n.edges.iter().any(|(_, src, dst, _, _)| {
            src.starts_with("http/server.go") && dst == "shop.go#Order"
        }),
        "{:#?}",
        n.edges
    );
    assert!(
        n.imports_files
            .contains(&("cmd/shopd/main.go".into(), "http/json.go".into()))
    );
}

#[test]
fn module_prefix_matches_only_at_a_path_boundary() {
    let known = BTreeSet::from(["tools/gen/a.go".to_string(), "go.mod".to_string()]);
    let modules = GoModules::new(
        known,
        BTreeMap::from([
            (String::new(), "example.com/shop".to_string()),
            ("tools".to_string(), "example.com/shop/tools".to_string()),
        ]),
        Vec::new(),
    );
    assert_eq!(
        modules.resolve("example.com/shopping/x"),
        GoTarget::External("example.com/shopping/x".into())
    );
    assert_eq!(
        modules.resolve("example.com/shop/tools/gen"),
        GoTarget::Package("tools/gen".into())
    );
    assert_eq!(
        modules.resolve("example.com/shop/missing"),
        GoTarget::Unresolved
    );
    assert_eq!(modules.files_in("tools/gen"), vec!["tools/gen/a.go"]);
    assert_eq!(
        modules.config_sources(),
        BTreeSet::from(["go.mod".to_string()])
    );
}

#[test]
fn module_lines_allow_quotes_and_comments() {
    assert_eq!(
        module_path("// top\nmodule example.com/a // the module\n\ngo 1.22\n").as_deref(),
        Some("example.com/a")
    );
    assert_eq!(
        module_path("module \"example.com/b\"\n").as_deref(),
        Some("example.com/b")
    );
    assert_eq!(module_path("go 1.22\n"), None);
}

#[test]
fn same_named_functions_in_two_main_packages_stay_apart() {
    let n = resolve_sources(&[
        (
            "cmd/a/main.go",
            "package main\n\nfunc main() {\n\trun()\n}\n",
        ),
        (
            "cmd/b/main.go",
            "package main\n\nfunc main() {\n\trun()\n}\n\nfunc run() {}\n",
        ),
    ]);
    assert!(
        n.has_edge(
            "calls",
            "cmd/b/main.go#main",
            "cmd/b/main.go#run",
            "go.call.local",
            false
        ),
        "{:#?}",
        n.edges
    );
    assert!(n.has_ref("cmd/a/main.go", "call", "run", "unresolved"));
    assert!(
        !n.edges
            .iter()
            .any(|(_, src, _, _, _)| src.starts_with("cmd/a/"))
    );
}

#[test]
fn a_test_package_and_dot_imports() {
    let n = resolve_sources(&[
        (
            "shop.go",
            "package shop\n\nfunc Validate() error { return nil }\n",
        ),
        (
            "shop_test.go",
            "package shop_test\n\nimport \"example.com/shop\"\n\nfunc TestValidate() {\n\tshop.Validate()\n}\n",
        ),
        (
            "store/memory.go",
            "package store\n\nfunc NewMemory() int { return 0 }\n",
        ),
        (
            "cmd/x/main.go",
            "package main\n\nimport . \"example.com/shop/store\"\n\nfunc main() {\n\tNewMemory()\n}\n",
        ),
    ]);
    assert!(
        n.has_edge(
            "calls",
            "shop_test.go#TestValidate",
            "shop.go#Validate",
            "go.call.package",
            false
        ),
        "{:#?}",
        n.edges
    );
    assert!(n.has_edge("imports", "shop_test.go", "shop.go", "go.import", false));
    assert!(
        n.has_edge(
            "calls",
            "cmd/x/main.go#main",
            "store/memory.go#NewMemory",
            "go.call.package",
            false
        ),
        "{:#?}",
        n.edges
    );
}

#[test]
fn a_method_name_on_two_types_is_a_ref_and_test_methods_stay_home() {
    let n = resolve_sources(&[
        (
            "a/a.go",
            "package a\n\ntype F struct{}\n\nfunc (f *F) Close() {}\n\ntype G struct{}\n\nfunc (g G) Close() {}\n\nfunc Use(x any) {\n\tx.Close()\n}\n",
        ),
        (
            "x/x.go",
            "package x\n\ntype Conn struct{}\n\nfunc (c *Conn) Shut() {}\n",
        ),
        (
            "x/fake_test.go",
            "package x\n\ntype fake struct{}\n\nfunc (f fake) Shut() {}\n",
        ),
        ("y/y.go", "package y\n\nfunc Use(c any) {\n\tc.Shut()\n}\n"),
    ]);
    assert!(n.has_ref("a/a.go", "call", "x.Close", "unresolved"));
    assert!(
        !n.edges.iter().any(|(_, src, _, _, _)| src == "a/a.go#Use"),
        "{:#?}",
        n.edges
    );
    assert!(
        n.has_edge(
            "calls",
            "y/y.go#Use",
            "x/x.go#Conn.Shut",
            "go.call.method",
            true
        ),
        "{:#?}",
        n.edges
    );
}

#[test]
fn go_mod_without_a_module_line_is_noted() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("go.mod"), "go 1.22\n").unwrap();
    let known = BTreeSet::from(["go.mod".to_string()]);
    let modules = GoModules::load(dir.path(), &known);
    assert_eq!(modules.notes(), ["go.mod at go.mod has no module line"]);
    assert_eq!(
        modules.config_sources(),
        BTreeSet::from(["go.mod".to_string()])
    );
    assert_eq!(
        modules.resolve("example.com/x"),
        GoTarget::External("example.com/x".into())
    );
}

#[test]
fn siblings_and_method_names_for_propagation() {
    let facts: BTreeMap<String, FileFacts> = [
        (
            "p/a.go",
            "package p\n\ntype T struct{}\n\nfunc (t T) Run() {}\n\nfunc F() {}\n",
        ),
        ("p/b.go", "package p\n\nfunc G(x any) {\n\tx.Run()\n}\n"),
        ("p/sub/c.go", "package sub\n"),
        ("q.go", "package q\n"),
    ]
    .iter()
    .map(|(rel, src)| (rel.to_string(), hm_index::go::extract(src)))
    .collect();
    assert_eq!(package_siblings(&facts, "p/a.go"), vec!["p/b.go"]);
    assert!(package_siblings(&facts, "q.go").is_empty());
    let names = method_names(&facts["p/a.go"]);
    assert_eq!(names, BTreeSet::from(["Run".to_string()]));
    assert!(calls_any_method(&facts["p/b.go"], &names));
    assert!(!calls_any_method(&facts["p/a.go"], &names));
}
