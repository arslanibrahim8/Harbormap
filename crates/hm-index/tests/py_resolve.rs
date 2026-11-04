mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

use common::fixture;
use hm_core::ids::file_id;
use hm_index::facts::FileFacts;
use hm_index::py::modules::{PyModule, PyModules, PyTarget};
use hm_index::py::resolve::{PyProject, resolve_file};
use hm_index::query::display_name;
use hm_index::ts::resolve::symbol_ids;

/// What resolving every Python file gave, with ids mapped back to readable names.
#[derive(Debug, Default)]
struct Named {
    /// (kind, source, destination, rule, inferred), as `path` or `path#Owner.name`.
    edges: BTreeSet<(String, String, String, String, bool)>,
    /// (file, kind, text, status).
    refs: BTreeSet<(String, String, String, String)>,
}

impl Named {
    fn has_edge(&self, kind: &str, src: &str, dst: &str, rule: &str, inferred: bool) -> bool {
        self.edges
            .contains(&(kind.into(), src.into(), dst.into(), rule.into(), inferred))
    }

    fn assert_edge(&self, kind: &str, src: &str, dst: &str, rule: &str, inferred: bool) {
        assert!(
            self.has_edge(kind, src, dst, rule, inferred),
            "missing edge {kind} {src} -> {dst} ({rule}, inferred {inferred}) in {:#?}",
            self.edges
        );
    }

    fn assert_ref(&self, file: &str, kind: &str, text: &str, status: &str) {
        let want = (file.into(), kind.into(), text.into(), status.into());
        assert!(
            self.refs.contains(&want),
            "missing ref {want:?} in {:#?}",
            self.refs
        );
    }
}

fn is_python(rel: &str) -> bool {
    rel.ends_with(".py") || rel.ends_with(".pyi")
}

/// Extracts the Python files of `sources`, resolves each one with `known` as the
/// files of the tree, and names the ids.
fn resolve_all(known: BTreeSet<String>, sources: &BTreeMap<String, String>) -> Named {
    let facts: BTreeMap<String, FileFacts> = sources
        .iter()
        .filter(|(rel, _)| is_python(rel))
        .map(|(rel, src)| (rel.clone(), hm_index::py::extract(src)))
        .collect();
    let modules = PyModules::new(known);

    let mut names: HashMap<String, String> = HashMap::new();
    for (rel, file_facts) in &facts {
        names.insert(file_id(rel), display_name(rel, None));
        for (id, sym) in symbol_ids(rel, file_facts)
            .into_iter()
            .zip(&file_facts.symbols)
        {
            names.insert(id, display_name(rel, Some(sym)));
        }
    }
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let project = PyProject {
        facts: &facts,
        modules: &modules,
    };
    let mut out = Named::default();
    for rel in facts.keys() {
        let resolved = resolve_file(&project, rel);
        for e in &resolved.edges {
            out.edges.insert((
                e.kind.into(),
                name_of(&e.src_id),
                name_of(&e.dst_id),
                e.rule.into(),
                e.inferred,
            ));
            if e.kind == "imports" {
                let dst = name_of(&e.dst_id);
                assert!(
                    resolved.imports_files.contains(&dst),
                    "{rel}: imports_files lacks {dst}"
                );
            }
        }
        for r in resolved.refs {
            out.refs
                .insert((rel.clone(), r.kind.into(), r.text, r.status.into()));
        }
    }
    out
}

/// Walks fixture `name` and resolves its Python files.
fn named_for_fixture(name: &str) -> Named {
    let walked = hm_index::walk::walk(&fixture(name)).expect("walk the fixture");
    let known: BTreeSet<String> = walked.files.iter().map(|f| f.rel.clone()).collect();
    let sources: BTreeMap<String, String> = walked
        .files
        .iter()
        .filter(|f| is_python(&f.rel))
        .map(|f| {
            (
                f.rel.clone(),
                fs::read_to_string(&f.abs).expect("read a fixture file"),
            )
        })
        .collect();
    resolve_all(known, &sources)
}

/// Resolves an in-memory tree of (path, text).
fn named_for(files: &[(&str, &str)]) -> Named {
    let known: BTreeSet<String> = files.iter().map(|(rel, _)| rel.to_string()).collect();
    let sources: BTreeMap<String, String> = files
        .iter()
        .map(|(rel, src)| (rel.to_string(), src.to_string()))
        .collect();
    resolve_all(known, &sources)
}

fn known(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn py_basic_resolves_imports_calls_and_inheritance() {
    let n = named_for_fixture("py-basic");
    let service = "src/shop/service.py";
    let api = "src/shop/api.py";

    for dst in [
        "src/shop/repo.py",
        "src/shop/models.py",
        "src/shop/mailer.py",
    ] {
        n.assert_edge("imports", service, dst, "py.import", false);
    }
    for dst in [
        "src/shop/__init__.py",
        "src/shop/models.py",
        "src/shop/fast.pyi",
    ] {
        n.assert_edge("imports", api, dst, "py.import", false);
    }
    n.assert_edge(
        "imports",
        "tests/test_service.py",
        "src/shop/service.py",
        "py.import",
        false,
    );

    n.assert_edge(
        "calls",
        "src/shop/api.py#create",
        "src/shop/service.py#place_order",
        "py.call.import",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/api.py#create",
        "src/shop/fast.pyi#speedup",
        "py.call.import",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/api.py#create",
        "src/shop/models.py#Order",
        "py.call.class",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/api.py",
        "src/shop/api.py#route",
        "py.call.local",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#Service.__init__",
        "src/shop/repo.py#Repo",
        "py.call.class",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#Service.place",
        "src/shop/models.py#Order",
        "py.call.class",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#Service.place",
        "src/shop/service.py#Service._audit",
        "py.call.self",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#Service.place",
        "src/shop/repo.py#Repo.add",
        "py.call.attr",
        true,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#Service.place",
        "src/shop/mailer.py#Mailer.send",
        "py.call.attr",
        true,
    );
    n.assert_edge(
        "calls",
        "src/shop/models.py#Base.save",
        "src/shop/models.py#Base.validate",
        "py.call.self",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/models.py#Order.persist",
        "src/shop/models.py#Base.save",
        "py.call.inherited",
        true,
    );
    n.assert_edge(
        "extends",
        "src/shop/models.py#Order",
        "src/shop/models.py#Base",
        "py.extends",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#place_order",
        "src/shop/service.py#Service",
        "py.call.class",
        false,
    );
    n.assert_edge(
        "calls",
        "src/shop/service.py#place_order",
        "src/shop/mailer.py#Mailer",
        "py.call.class",
        false,
    );

    n.assert_ref(service, "import", "logging", "external");
    n.assert_ref(service, "call", "logging.getLogger", "external");
    n.assert_ref(service, "call", "log.info", "unresolved");
    n.assert_ref(service, "call", "check", "unresolved");
    n.assert_ref(service, "call", "Service(Mailer()).place", "unresolved");
    n.assert_ref("src/shop/repo.py", "call", "order.save", "unresolved");
    n.assert_ref("tests/conftest.py", "import", "pytest", "external");

    for builtin in ["ValueError", "print"] {
        assert!(
            !n.edges
                .iter()
                .any(|(_, s, d, _, _)| s.contains(builtin) || d.contains(builtin)),
            "an edge names {builtin}"
        );
        assert!(
            !n.refs.iter().any(|(_, _, text, _)| text.contains(builtin)),
            "a ref names {builtin}: {:#?}",
            n.refs
        );
    }
}

#[test]
fn roots_follow_pyproject_and_src_layout() {
    let walked = hm_index::walk::walk(&fixture("py-basic")).expect("walk the fixture");
    let all: BTreeSet<String> = walked.files.iter().map(|f| f.rel.clone()).collect();
    assert_eq!(PyModules::new(all).roots(), ["", "src"]);

    let backend = known(&[
        "backend/pyproject.toml",
        "backend/app/main.py",
        "tools/x.py",
    ]);
    assert_eq!(PyModules::new(backend).roots(), ["", "backend"]);

    let backend = known(&[
        "backend/pyproject.toml",
        "backend/app/main.py",
        "backend/app/core/__init__.py",
        "tools/x.py",
    ]);
    let modules = PyModules::new(backend);
    assert_eq!(modules.roots(), ["", "backend"]);
    assert_eq!(
        modules.resolve("backend/app/main.py", "app.core"),
        PyTarget::Module(PyModule {
            file: Some("backend/app/core/__init__.py".into()),
            package_dir: Some("backend/app/core".into()),
        })
    );
    assert_eq!(
        modules.resolve("backend/app/main.py", "fastapi.routing"),
        PyTarget::External("fastapi".into())
    );
    assert_eq!(
        modules.resolve("backend/app/main.py", "app.missing"),
        PyTarget::Unresolved
    );
    assert_eq!(
        modules.resolve("backend/app/core/__init__.py", ".."),
        PyTarget::Module(PyModule {
            file: None,
            package_dir: Some("backend/app".into()),
        })
    );
    assert_eq!(
        modules.submodule("backend/app", "main"),
        Some(PyModule {
            file: Some("backend/app/main.py".into()),
            package_dir: None,
        })
    );
}

#[test]
fn a_name_in_init_beats_a_submodule_of_the_same_name() {
    let n = named_for(&[
        ("p/__init__.py", "def x():\n    pass\n"),
        ("p/x.py", "def f():\n    pass\n"),
        ("main.py", "from p import x\nx()\n"),
    ]);
    n.assert_edge("imports", "main.py", "p/__init__.py", "py.import", false);
    n.assert_edge(
        "calls",
        "main.py",
        "p/__init__.py#x",
        "py.call.import",
        false,
    );
    assert!(
        !n.edges
            .iter()
            .any(|(_, _, d, _, _)| d.starts_with("p/x.py"))
    );

    let n = named_for(&[
        ("p/__init__.py", ""),
        ("p/x.py", "def f():\n    pass\n"),
        ("main.py", "from p import x\nx.f()\n"),
    ]);
    n.assert_edge("imports", "main.py", "p/x.py", "py.import", false);
    n.assert_edge("calls", "main.py", "p/x.py#f", "py.call.module", false);
}

#[test]
fn a_relative_import_above_the_root_is_unresolved() {
    let n = named_for(&[("a/b.py", "from ...x import y\n"), ("x.py", "y = 1\n")]);
    n.assert_ref("a/b.py", "import", "...x", "unresolved");
    assert!(n.edges.is_empty(), "{:#?}", n.edges);

    let n = named_for(&[("a/b.py", "from ..x import y\n"), ("x.py", "y = 1\n")]);
    n.assert_edge("imports", "a/b.py", "x.py", "py.import", false);
}

#[test]
fn a_py_module_beats_its_stub() {
    let n = named_for(&[
        ("m.py", "def f():\n    pass\n"),
        ("m.pyi", "def f() -> None: ...\n"),
        ("main.py", "from m import f\nf()\n"),
    ]);
    n.assert_edge("imports", "main.py", "m.py", "py.import", false);
    n.assert_edge("calls", "main.py", "m.py#f", "py.call.import", false);
    assert!(
        !n.edges
            .iter()
            .any(|(_, s, d, _, _)| s == "main.py" && d.starts_with("m.pyi")),
        "{:#?}",
        n.edges
    );
}

#[test]
fn ambiguous_attr_types_and_star_imports_are_refs() {
    let n = named_for(&[(
        "c.py",
        "class A:\n    def y(self):\n        pass\n\n\
         class B:\n    def y(self):\n        pass\n\n\
         class C:\n    def __init__(self, flag):\n        if flag:\n            self.x = A()\n        else:\n            self.x = B()\n\n    def run(self):\n        self.x.y()\n",
    )]);
    n.assert_ref("c.py", "call", "self.x.y", "unresolved");
    assert!(
        !n.edges.iter().any(|(_, s, _, _, _)| s == "c.py#C.run"),
        "{:#?}",
        n.edges
    );

    let n = named_for(&[
        ("s1.py", "def f():\n    pass\n"),
        ("s2.py", "def f():\n    pass\n"),
        ("main.py", "from s1 import *\nfrom s2 import *\nf()\n"),
    ]);
    n.assert_ref("main.py", "call", "f", "unresolved");
    assert!(
        !n.edges.iter().any(|(k, _, _, _, _)| k == "calls"),
        "{:#?}",
        n.edges
    );

    // One star that offers the name is enough; a private name is never carried.
    let n = named_for(&[
        ("s1.py", "def f():\n    pass\n\ndef _g():\n    pass\n"),
        ("main.py", "from s1 import *\nf()\n_g()\n"),
    ]);
    n.assert_edge("calls", "main.py", "s1.py#f", "py.call.import", false);
    n.assert_ref("main.py", "call", "_g", "unresolved");
}

#[test]
fn super_cls_static_external_and_builtin_receivers() {
    let n = named_for(&[
        (
            "a.py",
            "import os\nfrom b import K\n\n\
             class Base:\n    def hello(self):\n        pass\n\n\
             class C(Base, os.PathLike, Exception):\n    @classmethod\n    def make(cls):\n        return cls.hello()\n\n\
             \x20   def run(self):\n        super().hello()\n        K.build()\n        os.path.join(\"a\")\n        str.join(\"\", [])\n        Base.hello(self)\n",
        ),
        (
            "b.py",
            "class K:\n    @staticmethod\n    def build():\n        pass\n",
        ),
    ]);
    n.assert_edge(
        "calls",
        "a.py#C.make",
        "a.py#Base.hello",
        "py.call.inherited",
        true,
    );
    n.assert_edge(
        "calls",
        "a.py#C.run",
        "a.py#Base.hello",
        "py.call.inherited",
        true,
    );
    n.assert_edge(
        "calls",
        "a.py#C.run",
        "b.py#K.build",
        "py.call.static",
        false,
    );
    n.assert_edge(
        "calls",
        "a.py#C.run",
        "a.py#Base.hello",
        "py.call.static",
        false,
    );
    n.assert_edge("extends", "a.py#C", "a.py#Base", "py.extends", false);
    n.assert_ref("a.py", "call", "os.path.join", "external");
    n.assert_ref("a.py", "extends", "os.PathLike", "external");
    assert!(
        !n.refs
            .iter()
            .any(|(_, _, text, _)| text == "Exception" || text == "str.join"),
        "{:#?}",
        n.refs
    );
}

/// `imports_files` of each file in `files`, resolved together.
fn read_sets(files: &[(&str, &str)]) -> BTreeMap<String, BTreeSet<String>> {
    let facts: BTreeMap<String, FileFacts> = files
        .iter()
        .map(|(rel, src)| (rel.to_string(), hm_index::py::extract(src)))
        .collect();
    let modules = PyModules::new(files.iter().map(|(rel, _)| rel.to_string()).collect());
    let p = PyProject {
        facts: &facts,
        modules: &modules,
    };
    facts
        .keys()
        .map(|rel| (rel.clone(), resolve_file(&p, rel).imports_files))
        .collect()
}

#[test]
fn imports_files_name_every_file_the_resolution_read() {
    let sets = read_sets(&[
        ("a/__init__.py", ""),
        ("a/b.py", "from x.y import f\n"),
        ("a/c.py", ""),
        ("x/__init__.py", ""),
        ("x/y.py", "def f():\n    pass\n"),
        ("main.py", "import a.c\n\na.b.f()\n"),
        ("k/__init__.py", "from .base import Base\n"),
        (
            "k/base.py",
            "from k2 import Root\n\n\nclass Base(Root):\n    pass\n",
        ),
        ("k2/__init__.py", "from .root import Root\n"),
        (
            "k2/root.py",
            "class Root:\n    def m(self):\n        pass\n",
        ),
        (
            "sub.py",
            "from k import Base\n\n\nclass S(Base):\n    def go(self):\n        self.m()\n",
        ),
    ]);
    // `a.b.f()` reads `a/b.py` and, through its binding, `x/y.py`: neither is imported.
    let want =
        |files: &[&str]| -> BTreeSet<String> { files.iter().map(|f| f.to_string()).collect() };
    assert_eq!(
        sets["main.py"],
        want(&["a/__init__.py", "a/b.py", "a/c.py", "x/y.py"])
    );
    // The base chain reads every `__init__` and class file on the way to `Root.m`.
    assert_eq!(
        sets["sub.py"],
        want(&["k/__init__.py", "k/base.py", "k2/__init__.py", "k2/root.py"])
    );
    // A file never lists itself.
    assert!(sets.iter().all(|(rel, read)| !read.contains(rel)));
}

#[test]
fn a_package_importing_its_own_submodule_binds_the_submodule() {
    let tree = |main: &'static str| {
        named_for(&[
            ("p/__init__.py", "from . import sub\n"),
            ("p/sub.py", "def f():\n    pass\n"),
            ("main.py", main),
        ])
    };
    for main in ["import p\np.sub.f()\n", "from p import sub\nsub.f()\n"] {
        let n = tree(main);
        n.assert_edge("calls", "main.py", "p/sub.py#f", "py.call.module", false);
        n.assert_edge("imports", "p/__init__.py", "p/sub.py", "py.import", false);
        assert!(
            !n.edges
                .iter()
                .any(|(_, s, d, _, _)| s == "p/__init__.py" && d == "p/__init__.py"),
            "a self-edge: {:#?}",
            n.edges
        );
        assert!(n.refs.is_empty(), "{main}: {:#?}", n.refs);
    }
    // `from p import sub` names the submodule, so the edge goes to it.
    tree("from p import sub\nsub.f()\n").assert_edge(
        "imports",
        "main.py",
        "p/sub.py",
        "py.import",
        false,
    );
}

#[test]
fn an_attr_type_that_is_not_a_class_makes_the_call_a_ref() {
    let n = named_for(&[(
        "c.py",
        "from unk import Unk\n\n\
         class A:\n    def y(self):\n        pass\n\n\
         class C:\n    def __init__(self, flag):\n        if flag:\n            self.x = A()\n        else:\n            self.x = Unk()\n\n    def run(self):\n        self.x.y()\n",
    )]);
    n.assert_ref("c.py", "call", "self.x.y", "unresolved");
    assert!(
        !n.edges.iter().any(|(_, s, _, _, _)| s == "c.py#C.run"),
        "{:#?}",
        n.edges
    );

    // The same on a base: the base's facts decide, all of them.
    let n = named_for(&[(
        "c.py",
        "class A:\n    def y(self):\n        pass\n\n\
         class B:\n    def __init__(self):\n        self.x = A()\n        self.x = make()\n\n\
         class C(B):\n    def run(self):\n        self.x.y()\n",
    )]);
    n.assert_ref("c.py", "call", "self.x.y", "unresolved");
}

#[test]
fn a_name_chain_is_followed_for_five_hops() {
    // main -> c1 -> ... -> c<last>, each re-exporting `f`; the last one defines it.
    let chain = |last: u32| {
        let mut files: Vec<(String, String)> = (1..last)
            .map(|i| (format!("c{i}.py"), format!("from c{} import f\n", i + 1)))
            .collect();
        files.push((format!("c{last}.py"), "def f():\n    pass\n".into()));
        files.push(("main.py".into(), "from c1 import f\nf()\n".into()));
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        named_for(&files)
    };
    chain(5).assert_edge("calls", "main.py", "c5.py#f", "py.call.import", false);
    let n = chain(6);
    n.assert_ref("main.py", "call", "f", "unresolved");
    assert!(!n.edges.iter().any(|(k, _, _, _, _)| k == "calls"));
}

#[test]
fn an_aliased_dotted_import_binds_the_full_module() {
    let n = named_for(&[
        ("a/__init__.py", "def f():\n    pass\n"),
        ("a/b.py", "def f():\n    pass\n"),
        (
            "main.py",
            "import a.b as c\nimport a.b\nc.f()\na.f()\na.b.f()\n",
        ),
    ]);
    n.assert_edge("imports", "main.py", "a/b.py", "py.import", false);
    n.assert_edge("calls", "main.py", "a/b.py#f", "py.call.module", false);
    n.assert_edge(
        "calls",
        "main.py",
        "a/__init__.py#f",
        "py.call.module",
        false,
    );
    assert!(n.refs.is_empty(), "{:#?}", n.refs);
}

#[test]
fn a_dense_star_import_graph_resolves_quickly_and_stays_correct() {
    // 30 modules that each star-import 5 siblings, all star-imported by the package,
    // and a caller with 200 calls to names that nothing binds: every lookup searches
    // the whole graph. Without memoising it takes seconds per file.
    const N: usize = 30;
    const F: usize = 5;
    let mut files: Vec<(String, String)> = Vec::new();
    let init: String = (1..=N).map(|k| format!("from .m{k} import *\n")).collect();
    files.push(("pkg/__init__.py".into(), init));
    for k in 1..=N {
        let mut src: String = (1..=F)
            .map(|d| format!("from .m{} import *\n", (k + d - 1) % N + 1))
            .collect();
        src.push_str(&format!("\n\ndef f{k}():\n    pass\n"));
        // Two modules define `dup`: a call to it has two hits.
        if k <= 2 {
            src.push_str("\n\ndef dup():\n    pass\n");
        }
        files.push((format!("pkg/m{k}.py"), src));
    }
    let mut app = String::from("from pkg import *\n\n\ndef main(cb, items):\n");
    for i in 0..200 {
        app.push_str(&format!("    cb{}(items)\n", i % 20));
    }
    app.push_str("    f3(items)\n    dup(items)\n");
    files.push(("app.py".into(), app));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(r, t)| (r.as_str(), t.as_str()))
        .collect();

    let start = std::time::Instant::now();
    let n = named_for(&files);
    let took = start.elapsed();
    assert!(took.as_millis() < 1000, "resolving took {took:?}");
    n.assert_edge(
        "calls",
        "app.py#main",
        "pkg/m3.py#f3",
        "py.call.import",
        false,
    );
    n.assert_ref("app.py", "call", "dup", "unresolved");
    n.assert_ref("app.py", "call", "cb7", "unresolved");
}

#[test]
fn requirements_pipfile_and_manage_py_mark_package_roots() {
    for marker in [
        "requirements.txt",
        "requirements-dev.txt",
        "requirements_test.txt",
        "Pipfile",
        "manage.py",
    ] {
        let marker_path = format!("backend/{marker}");
        let n = named_for(&[
            (marker_path.as_str(), ""),
            ("backend/app/__init__.py", ""),
            ("backend/app/y.py", "def z():\n    pass\n"),
            ("backend/app/x.py", "from app.y import z\n\nz()\n"),
        ]);
        assert!(
            n.has_edge(
                "calls",
                "backend/app/x.py",
                "backend/app/y.py#z",
                "py.call.import",
                false
            ),
            "{marker}: {:#?} {:#?}",
            n.edges,
            n.refs
        );
    }
    // The `src/` folder of such a root is a root too.
    let n = named_for(&[
        ("svc/requirements.txt", ""),
        ("svc/src/core/__init__.py", "def run():\n    pass\n"),
        ("svc/src/main.py", "from core import run\n\nrun()\n"),
    ]);
    n.assert_edge(
        "calls",
        "svc/src/main.py",
        "svc/src/core/__init__.py#run",
        "py.call.import",
        false,
    );
}

#[test]
fn a_base_from_outside_hides_the_bases_after_it() {
    let n = named_for(&[
        (
            "lib.py",
            "class Repo:\n    def save(self):\n        pass\n\n\nclass LocalMixin:\n    def __init__(self):\n        self.repo = Repo()\n\n    def dispatch(self):\n        pass\n",
        ),
        (
            "v.py",
            "from external import ExternalBase\nfrom lib import LocalMixin\n\n\nclass V(ExternalBase, LocalMixin):\n    def go(self):\n        self.dispatch()\n        self.repo.save()\n\n\nclass W(LocalMixin, ExternalBase):\n    def go(self):\n        self.dispatch()\n        self.repo.save()\n\n\nclass U(ExternalBase, LocalMixin):\n    def dispatch(self):\n        pass\n\n    def go(self):\n        self.dispatch()\n",
        ),
    ]);
    // Python's method order reaches `ExternalBase` first: it may hold either name.
    for dst in ["lib.py#LocalMixin.dispatch", "lib.py#Repo.save"] {
        assert!(
            !n.edges
                .iter()
                .any(|(_, src, d, _, _)| src == "v.py#V.go" && d == dst),
            "{:#?}",
            n.edges
        );
    }
    n.assert_ref("v.py", "call", "self.dispatch", "unresolved");
    n.assert_ref("v.py", "call", "self.repo.save", "unresolved");
    // A project base before it, or the class's own method, still decides.
    n.assert_edge(
        "calls",
        "v.py#W.go",
        "lib.py#LocalMixin.dispatch",
        "py.call.inherited",
        true,
    );
    n.assert_edge(
        "calls",
        "v.py#W.go",
        "lib.py#Repo.save",
        "py.call.attr",
        true,
    );
    n.assert_edge(
        "calls",
        "v.py#U.go",
        "v.py#U.dispatch",
        "py.call.self",
        false,
    );
}
