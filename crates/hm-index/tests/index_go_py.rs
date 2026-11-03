//! Go and Python files in the full pass and in incremental updates: what is stored,
//! and that every update leaves the store equal to a cold index (spec §5.4, §11.3).

mod common;

use std::fs;
use std::path::Path;

use common::{dump, edit, fresh_index, fresh_index_of, indexed, set};
use hm_index::index::Indexer;

/// The rows of one `dump` section.
fn section(ix: &Indexer, name: &str) -> Vec<String> {
    let text = dump(ix.store());
    let header = format!("[{name}] ");
    text.lines()
        .skip_while(|l| !l.starts_with(&header))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .map(str::to_string)
        .collect()
}

/// `(rule, prov, conf)` of the stored edges of `kind` from `from` to `to`.
fn evidence(ix: &Indexer, kind: &str, from: &str, to: &str) -> Vec<(String, String, String)> {
    let prefix = format!("{kind} | {from} -> {to} |");
    section(ix, "edge")
        .iter()
        .filter(|e| e.starts_with(&prefix))
        .map(|e| {
            let cols: Vec<&str> = e.split(" | ").collect();
            let n = cols.len();
            (
                cols[n - 3].to_string(),
                cols[n - 2].to_string(),
                cols[n - 1].to_string(),
            )
        })
        .collect()
}

fn has_edge(ix: &Indexer, kind: &str, from: &str, to: &str) -> bool {
    !evidence(ix, kind, from, to).is_empty()
}

fn has_ref(ix: &Indexer, file: &str, text: &str, status: &str) -> bool {
    section(ix, "ref").iter().any(|r| {
        let cols: Vec<&str> = r.split(" | ").collect();
        cols[0] == file && cols[2] == text && cols[4] == status
    })
}

fn assert_cold(ix: &Indexer, dir: &Path) {
    assert_eq!(dump(ix.store()), dump(fresh_index_of(dir).store()));
}

fn ev(rule: &str, prov: &str, conf: &str) -> Vec<(String, String, String)> {
    vec![(rule.into(), prov.into(), conf.into())]
}

const HANDLE: &str = "http/server.go#Server.handleOrder";

#[test]
fn go_basic_indexes_with_evidence() {
    let (mut ix, _dir) = common::indexer_for("go-basic");
    let r = ix.full().unwrap();
    assert_eq!(r.files.get("go"), Some(&9));
    assert_eq!(r.parsed, 9);
    assert_eq!(r.parsed_with_errors, 0);
    assert_eq!(r.not_supported_yet, 0);
    assert_eq!(r.classes.get("test"), Some(&1));
    assert_eq!(r.classes.get("generated"), Some(&1));
    assert_eq!(
        evidence(&ix, "calls", HANDLE, "store/memory.go#Memory.Save"),
        ev("go.call.method", "inferred", "probably")
    );
    assert_eq!(
        evidence(&ix, "calls", HANDLE, "shop.go#Validate"),
        ev("go.call.package", "parsed", "sure")
    );
    assert_eq!(dump(ix.store()), dump(fresh_index("go-basic").store()));
}

#[test]
fn py_basic_indexes_with_evidence() {
    let (mut ix, _dir) = common::indexer_for("py-basic");
    let r = ix.full().unwrap();
    assert_eq!(r.files.get("python"), Some(&11));
    assert_eq!(r.parsed_with_errors, 0);
    assert_eq!(r.not_supported_yet, 0);
    assert_eq!(r.classes.get("test"), Some(&3));
    assert_eq!(r.classes.get("generated"), Some(&1));
    assert_eq!(
        evidence(
            &ix,
            "calls",
            "src/shop/models.py#Order.persist",
            "src/shop/models.py#Base.save"
        ),
        ev("py.call.inherited", "inferred", "probably")
    );
    assert_eq!(dump(ix.store()), dump(fresh_index("py-basic").store()));
}

#[test]
fn renaming_a_go_method_updates_inferred_callers_elsewhere() {
    let (mut ix, dir) = indexed("go-basic");
    edit(
        dir.path(),
        "store/memory.go",
        "func (m *Memory) Save(",
        "func (m *Memory) Put(",
    );
    let d = ix.apply(&set(["store/memory.go"])).unwrap();
    // `http/server.go` does not import `store`: only the method name links them.
    assert!(
        d.reresolved.contains(&"http/server.go".to_string()),
        "{d:?}"
    );
    assert!(
        d.reresolved.contains(&"store/memory_test.go".to_string()),
        "{d:?}"
    );
    assert!(!has_edge(
        &ix,
        "calls",
        HANDLE,
        "store/memory.go#Memory.Save"
    ));
    assert!(has_ref(&ix, "http/server.go", "s.Store.Save", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_go_sibling_change_reresolves_the_package() {
    let (mut ix, dir) = indexed("go-basic");
    edit(
        dir.path(),
        "http/json.go",
        "func writeJSON(",
        "func writeBody(",
    );
    let d = ix.apply(&set(["http/json.go"])).unwrap();
    assert!(
        d.reresolved.contains(&"http/server.go".to_string()),
        "{d:?}"
    );
    assert!(has_ref(&ix, "http/server.go", "writeJSON", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_python_reexport_chain_reresolves_importers() {
    let (mut ix, dir) = indexed("py-basic");
    edit(
        dir.path(),
        "src/shop/service.py",
        "def place_order(",
        "def place(",
    );
    let d = ix.apply(&set(["src/shop/service.py"])).unwrap();
    assert!(
        d.reresolved.contains(&"src/shop/api.py".to_string()),
        "{d:?}"
    );
    assert!(has_ref(&ix, "src/shop/api.py", "place_order", "unresolved"));
    assert_cold(&ix, dir.path());

    // Without its first line, `__init__.py` no longer hands `place_order` on.
    let init = dir.path().join("src/shop/__init__.py");
    let text = fs::read_to_string(&init).unwrap();
    let rest = text.split_once('\n').unwrap().1.to_string();
    fs::write(&init, rest).unwrap();
    ix.apply(&set(["src/shop/__init__.py"])).unwrap();
    assert_cold(&ix, dir.path());
}

#[test]
fn a_python_base_class_change_reresolves_subclasses_in_other_files() {
    let (mut ix, dir) = indexed("py-basic");
    fs::write(
        dir.path().join("src/shop/premium.py"),
        "from .models import Order\n\nclass Premium(Order):\n    def go(self):\n        return self.save()\n",
    )
    .unwrap();
    ix.apply(&set(["src/shop/premium.py"])).unwrap();
    assert_cold(&ix, dir.path());

    edit(dir.path(), "src/shop/models.py", "def save(", "def store(");
    let d = ix.apply(&set(["src/shop/models.py"])).unwrap();
    assert!(
        d.reresolved.contains(&"src/shop/premium.py".to_string()),
        "{d:?}"
    );
    assert_cold(&ix, dir.path());
}

// What the brief's six changes alone would miss.

#[test]
fn a_go_method_that_changes_its_receiver_moves_inferred_edges() {
    let (mut ix, dir) = indexed("go-basic");
    // Same method name, new owner: the name set is unchanged, the target id is not.
    edit(
        dir.path(),
        "store/memory.go",
        "func (m *Memory) Save(",
        "func (m *Disk) Save(",
    );
    let d = ix.apply(&set(["store/memory.go"])).unwrap();
    assert!(
        d.reresolved.contains(&"http/server.go".to_string()),
        "{d:?}"
    );
    assert!(has_edge(&ix, "calls", HANDLE, "store/memory.go#Disk.Save"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_python_heritage_change_reaches_subclasses_elsewhere() {
    let (mut ix, dir) = indexed("py-basic");
    fs::write(
        dir.path().join("src/shop/premium.py"),
        "from .models import Order\n\nclass Premium(Order):\n    def go(self):\n        return self.save()\n",
    )
    .unwrap();
    ix.apply(&set(["src/shop/premium.py"])).unwrap();

    // `Order` stops extending `Base`: `Premium.go`'s inherited `save` goes away.
    edit(
        dir.path(),
        "src/shop/models.py",
        "class Order(Base):",
        "class Order:",
    );
    let d = ix.apply(&set(["src/shop/models.py"])).unwrap();
    assert!(
        d.reresolved.contains(&"src/shop/premium.py".to_string()),
        "{d:?}"
    );
    assert_cold(&ix, dir.path());
}

#[test]
fn a_package_init_change_reaches_files_that_import_its_submodules() {
    let (mut ix, dir) = indexed("py-basic");
    // `service.py` imports the submodule `models` with `from . import models`, so it
    // reads `__init__.py` without importing it. Binding `models` there changes that.
    let init = dir.path().join("src/shop/__init__.py");
    let text = fs::read_to_string(&init).unwrap();
    fs::write(&init, format!("{text}models = None\n")).unwrap();
    let d = ix.apply(&set(["src/shop/__init__.py"])).unwrap();
    assert!(
        d.reresolved.contains(&"src/shop/service.py".to_string()),
        "{d:?}"
    );
    assert_cold(&ix, dir.path());
}

#[test]
fn a_dotted_call_through_a_bound_package_follows_its_submodules() {
    let (mut ix, dir) = indexed("py-basic");
    // `import shop.models` binds `shop`; the call walks to `shop.mailer`, which the
    // file does not import.
    fs::write(
        dir.path().join("src/shop/extra.py"),
        "import shop.models\n\n\ndef run():\n    return shop.mailer.Mailer()\n",
    )
    .unwrap();
    ix.apply(&set(["src/shop/extra.py"])).unwrap();
    assert_cold(&ix, dir.path());

    edit(
        dir.path(),
        "src/shop/mailer.py",
        "class Mailer:",
        "class Sender:",
    );
    let d = ix.apply(&set(["src/shop/mailer.py"])).unwrap();
    assert!(
        d.reresolved.contains(&"src/shop/extra.py".to_string()),
        "{d:?}"
    );
    assert_cold(&ix, dir.path());
}

#[test]
fn a_go_file_that_gains_its_facts_reaches_the_package_importers() {
    let (mut ix, dir) = indexed("go-basic");
    // `shop.go` turns binary: its facts go, and so do its importers' edges to it.
    let path = dir.path().join("shop.go");
    let text = fs::read(&path).unwrap();
    fs::write(&path, b"\0\x01\x02").unwrap();
    ix.apply(&set(["shop.go"])).unwrap();
    assert_cold(&ix, dir.path());

    // Back as text: importers hold no row for a file that had no facts.
    fs::write(&path, text).unwrap();
    ix.apply(&set(["shop.go"])).unwrap();
    assert!(has_edge(&ix, "calls", HANDLE, "shop.go#Validate"));
    assert_cold(&ix, dir.path());
}

#[test]
fn go_and_python_body_edits_reach_no_other_file() {
    let (mut ix, dir) = indexed("go-basic");
    edit(dir.path(), "shop.go", "return ErrBad", "return nil");
    let d = ix.apply(&set(["shop.go"])).unwrap();
    assert!(d.reresolved.is_empty(), "{d:?}");
    assert_cold(&ix, dir.path());

    let (mut ix, dir) = indexed("py-basic");
    edit(
        dir.path(),
        "src/shop/mailer.py",
        "print(order)",
        "print(order, order)",
    );
    let d = ix.apply(&set(["src/shop/mailer.py"])).unwrap();
    assert!(d.reresolved.is_empty(), "{d:?}");
    assert_cold(&ix, dir.path());
}

/// A copy of `files` in a new folder, indexed into an in-memory store. The `TempDir`
/// comes last, as in `common::indexed`.
fn indexed_tree(files: &[(&str, &str)]) -> (Indexer, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("make a temp dir");
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let store = hm_store::Store::open_in_memory().expect("open a store");
    let mut ix = Indexer::open(dir.path(), store).expect("open the indexer");
    ix.full().expect("index the tree");
    (ix, dir)
}

#[test]
fn a_python_method_four_bases_away_through_package_inits_stays_cold_equal() {
    // `A` -> `B1` -> `B2` -> `B3` -> `B4`, each base in its own package and handed
    // on by that package's `__init__`: more files than any forwarding walk follows.
    let mut files = vec![
        ("app/__init__.py".to_string(), String::new()),
        (
            "app/a.py".to_string(),
            "from lib1 import B1\n\n\nclass A(B1):\n    def go(self):\n        return self.m()\n"
                .to_string(),
        ),
    ];
    for n in 1..=4 {
        files.push((
            format!("lib{n}/__init__.py"),
            format!("from .base import B{n}\n"),
        ));
        let base = if n < 4 {
            format!(
                "from lib{next} import B{next}\n\n\nclass B{n}(B{next}):\n    pass\n",
                next = n + 1
            )
        } else {
            "class B4:\n    def m(self):\n        pass\n".to_string()
        };
        files.push((format!("lib{n}/base.py"), base));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(r, t)| (r.as_str(), t.as_str()))
        .collect();
    let (mut ix, dir) = indexed_tree(&files);
    assert_eq!(
        evidence(&ix, "calls", "app/a.py#A.go", "lib4/base.py#B4.m"),
        ev("py.call.inherited", "inferred", "probably")
    );

    edit(dir.path(), "lib4/base.py", "def m(", "def n(");
    let d = ix.apply(&set(["lib4/base.py"])).unwrap();
    assert!(d.reresolved.contains(&"app/a.py".to_string()), "{d:?}");
    assert!(
        !section(&ix, "edge")
            .iter()
            .any(|e| e.starts_with("calls | app/a.py#A.go ->")),
        "{:#?}",
        section(&ix, "edge")
    );
    assert!(has_ref(&ix, "app/a.py", "self.m", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_dotted_call_through_a_submodule_binding_elsewhere_stays_cold_equal() {
    // `a.b.f()` walks into `a/b.py`, which binds `f` from `x/y.py`: `main.py` reads
    // `x/y.py` with no import of it and no package walk over its folder.
    let (mut ix, dir) = indexed_tree(&[
        ("a/__init__.py", ""),
        ("a/b.py", "from x.y import f\n"),
        ("a/c.py", ""),
        ("x/__init__.py", ""),
        ("x/y.py", "def f():\n    pass\n"),
        ("main.py", "import a.c\n\na.b.f()\n"),
    ]);
    assert_eq!(
        evidence(&ix, "calls", "main.py", "x/y.py#f"),
        ev("py.call.module", "parsed", "sure")
    );

    edit(dir.path(), "x/y.py", "def f(", "def g(");
    let d = ix.apply(&set(["x/y.py"])).unwrap();
    assert!(d.reresolved.contains(&"main.py".to_string()), "{d:?}");
    assert!(!has_edge(&ix, "calls", "main.py", "x/y.py#f"));
    assert!(has_ref(&ix, "main.py", "a.b.f", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn an_interface_method_name_decides_inferred_calls_elsewhere() {
    let (mut ix, dir) = indexed("go-basic");
    // `store/memory_test.go` calls `m.Save` on a local: only the name links it to
    // `Memory.Save`, and only while `OrderStore` declares `Save`. It does not import
    // `shop.go`.
    let test = "store/memory_test.go#TestSave";
    assert!(has_edge(&ix, "calls", test, "store/memory.go#Memory.Save"));
    edit(dir.path(), "shop.go", "\tSave(o *Order) error\n", "");
    let d = ix.apply(&set(["shop.go"])).unwrap();
    assert!(
        d.reresolved.contains(&"store/memory_test.go".to_string()),
        "{d:?}"
    );
    assert!(!has_edge(&ix, "calls", test, "store/memory.go#Memory.Save"));
    assert!(has_ref(&ix, "store/memory_test.go", "m.Save", "unresolved"));
    assert_cold(&ix, dir.path());

    edit(
        dir.path(),
        "shop.go",
        "\tFind(id int)",
        "\tSave(o *Order) error\n\tFind(id int)",
    );
    ix.apply(&set(["shop.go"])).unwrap();
    assert!(has_edge(&ix, "calls", test, "store/memory.go#Memory.Save"));
    assert_cold(&ix, dir.path());
}
