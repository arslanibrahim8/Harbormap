//! Who owns which edge rows. The resolvers own the rows under their rules
//! (`langs::resolver_rules`), `parsed` and `inferred` alike; derive owns every other
//! edge row. On a project where both write `inferred` edges (a Next.js client call,
//! a Go method-name call, a Python attribute call), edits that add and remove each kind
//! must leave the store equal to a cold index after every step.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use common::{dump, fresh_index_of};
use hm_index::index::Indexer;
use hm_index::langs::{is_resolver_rule, resolver_rules};
use hm_store::Store;

const FILES: &[(&str, &str)] = &[
    (
        "package.json",
        r#"{ "name": "web", "dependencies": { "next": "15.2.0", "react": "19.0.0" } }
"#,
    ),
    (
        "app/api/orders/route.ts",
        "export async function GET() {\n  return Response.json([])\n}\n",
    ),
    (
        "app/page.tsx",
        "export default async function Home() {\n  const res = await fetch('/api/orders')\n  return <main>{String(res.ok)}</main>\n}\n",
    ),
    ("go.mod", "module example.com/mix\n\ngo 1.22\n"),
    (
        "svc/shop.go",
        "package svc\n\ntype Notifier interface {\n\tNotify(msg string)\n}\n\ntype Email struct{}\n\nfunc (e Email) Notify(msg string) {}\n\ntype Shop struct {\n\tN Notifier\n}\n\nfunc (s *Shop) Place() {\n\ts.N.Notify(\"placed\")\n}\n",
    ),
    (
        "pyproject.toml",
        "[project]\nname = \"mix\"\nversion = \"0.1.0\"\n",
    ),
    ("src/mix/__init__.py", ""),
    (
        "src/mix/repo.py",
        "class Repo:\n    def add(self, item):\n        return item\n",
    ),
    (
        "src/mix/service.py",
        "from mix.repo import Repo\n\n\nclass Service:\n    repo: Repo\n\n    def __init__(self):\n        self.repo = Repo()\n\n    def place(self, order):\n        self.repo.add(order)\n",
    ),
];

const SMS: &str = "package svc\n\ntype SMS struct{}\n\nfunc (s SMS) Notify(msg string) {}\n";

/// The three inferred rules the test follows, one per owner and language.
const HTTP: &str = "ts.client.fetch";
const GO_METHOD: &str = "go.call.method";
const PY_ATTR: &str = "py.call.attr";

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn replace(dir: &Path, rel: &str, from: &str, to: &str) {
    common::edit(dir, rel, from, to);
}

/// The `[edge]` rows of a dump, as columns: kind, `src -> dst`, file, line, rule,
/// prov, conf.
fn edges(text: &str) -> BTreeSet<Vec<String>> {
    text.lines()
        .skip_while(|l| !l.starts_with("[edge] "))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .map(|l| l.split(" | ").map(str::to_string).collect())
        .collect()
}

fn has(text: &str, rule: &str) -> bool {
    edges(text).iter().any(|e| e[4] == rule)
}

struct Step {
    what: &'static str,
    changed: &'static [&'static str],
    /// Whether each of `HTTP`, `GO_METHOD`, `PY_ATTR` is stored after the step.
    expect: [bool; 3],
}

/// Checks the store after `step` against a cold index, the expected edges, the evidence
/// each owner writes, and the delta's edge counts against the rows that changed.
fn check(ix: &mut Indexer, dir: &Path, step: &Step, before: &str) -> String {
    let changed: BTreeSet<String> = step.changed.iter().map(|s| s.to_string()).collect();
    let delta = ix.apply(&changed).expect("apply the edit");
    let after = dump(ix.store());
    let cold = dump(fresh_index_of(dir).store());
    assert_eq!(after, cold, "incremental != cold after: {}", step.what);

    for (rule, want) in [HTTP, GO_METHOD, PY_ATTR].iter().zip(step.expect) {
        assert_eq!(has(&after, rule), want, "{rule} after: {}", step.what);
    }
    for e in edges(&after) {
        let (rule, prov, conf, file) = (&e[4], &e[5], &e[6], &e[2]);
        if is_resolver_rule(rule) {
            let inferred = [GO_METHOD, "py.call.inherited", PY_ATTR].contains(&rule.as_str());
            let want = if inferred {
                ("inferred", "probably")
            } else {
                ("parsed", "sure")
            };
            assert_eq!((prov.as_str(), conf.as_str()), want, "{e:?}");
        } else {
            // Derive writes only inferred rows, and only for TypeScript files so far.
            assert_eq!(
                (prov.as_str(), conf.as_str()),
                ("inferred", "probably"),
                "{e:?}"
            );
            assert!(file.ends_with(".ts") || file.ends_with(".tsx"), "{e:?}");
        }
    }

    let (old, new) = (edges(before), edges(&after));
    assert_eq!(
        (delta.edges_added, delta.edges_removed),
        (
            new.difference(&old).count() as u32,
            old.difference(&new).count() as u32
        ),
        "edge counts after: {}",
        step.what
    );
    after
}

#[test]
fn resolver_and_derived_edges_each_keep_their_own_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, text) in FILES {
        write(root, rel, text);
    }
    let mut ix = Indexer::open(root, Store::open_in_memory().unwrap()).unwrap();
    ix.full().unwrap();
    let mut before = dump(ix.store());
    assert_eq!(before, dump(fresh_index_of(root).store()));
    for rule in [HTTP, GO_METHOD, PY_ATTR] {
        assert!(has(&before, rule), "{rule} in the cold index:\n{before}");
    }

    let all = [true, true, true];
    type Edit = fn(&Path);
    let steps: Vec<(Step, Edit)> = vec![
        (
            Step {
                what: "the route stops answering GET",
                changed: &["app/api/orders/route.ts"],
                expect: [false, true, true],
            },
            |d| replace(d, "app/api/orders/route.ts", "GET", "POST"),
        ),
        (
            Step {
                what: "the route answers GET again",
                changed: &["app/api/orders/route.ts"],
                expect: all,
            },
            |d| replace(d, "app/api/orders/route.ts", "POST", "GET"),
        ),
        (
            Step {
                what: "a second Notifier implementation",
                changed: &["svc/sms.go"],
                expect: [true, false, true],
            },
            |d| write(d, "svc/sms.go", SMS),
        ),
        (
            Step {
                what: "the second implementation goes",
                changed: &["svc/sms.go"],
                expect: all,
            },
            |d| fs::remove_file(d.join("svc/sms.go")).unwrap(),
        ),
        (
            Step {
                what: "the attribute's class loses the method",
                changed: &["src/mix/repo.py"],
                expect: [true, true, false],
            },
            |d| replace(d, "src/mix/repo.py", "def add(", "def insert("),
        ),
        (
            Step {
                what: "the attribute's class has the method again",
                changed: &["src/mix/repo.py"],
                expect: all,
            },
            |d| replace(d, "src/mix/repo.py", "def insert(", "def add("),
        ),
        (
            Step {
                what: "every caller changes at once",
                changed: &["app/page.tsx", "svc/shop.go", "src/mix/service.py"],
                expect: [false, false, false],
            },
            |d| {
                replace(d, "app/page.tsx", "'/api/orders'", "'/api/none'");
                replace(d, "svc/shop.go", "s.N.Notify(", "s.N.Other(");
                replace(d, "src/mix/service.py", "self.repo.add(", "self.repo.put(");
            },
        ),
        (
            Step {
                what: "every caller changes back at once",
                changed: &["app/page.tsx", "svc/shop.go", "src/mix/service.py"],
                expect: all,
            },
            |d| {
                replace(d, "app/page.tsx", "'/api/none'", "'/api/orders'");
                replace(d, "svc/shop.go", "s.N.Other(", "s.N.Notify(");
                replace(d, "src/mix/service.py", "self.repo.put(", "self.repo.add(");
            },
        ),
        (
            Step {
                what: "the inferred edges' files are removed",
                changed: &["app/page.tsx", "svc/shop.go", "src/mix/service.py"],
                expect: [false, false, false],
            },
            |d| {
                for rel in ["app/page.tsx", "svc/shop.go", "src/mix/service.py"] {
                    fs::remove_file(d.join(rel)).unwrap();
                }
            },
        ),
        (
            Step {
                what: "the inferred edges' files come back",
                changed: &["app/page.tsx", "svc/shop.go", "src/mix/service.py"],
                expect: all,
            },
            |d| {
                for (rel, text) in FILES {
                    if ["app/page.tsx", "svc/shop.go", "src/mix/service.py"].contains(rel) {
                        write(d, rel, text);
                    }
                }
            },
        ),
    ];
    for (step, edit) in &steps {
        edit(root);
        before = check(&mut ix, root, step, &before);
    }

    // A rescan with nothing changed writes nothing, on either side.
    let delta = ix.rescan().unwrap();
    assert_eq!((delta.edges_added, delta.edges_removed), (0, 0));
    assert_eq!(dump(ix.store()), before);
}

#[test]
fn every_resolver_rule_is_listed_once_and_named_after_its_language() {
    let rules: Vec<&str> = resolver_rules().collect();
    let unique: BTreeSet<&str> = rules.iter().copied().collect();
    assert_eq!(rules.len(), unique.len(), "a rule is listed twice");
    for rule in rules {
        assert!(
            ["ts.", "go.", "py."].iter().any(|p| rule.starts_with(p)),
            "{rule}"
        );
    }
}
