//! `incremental == cold` for Go and Python (spec §11.3): after any edit,
//! `Indexer::apply` leaves the store equal to a cold index of the same tree.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use common::{dump, fresh_index_of};
use hm_index::index::Indexer;
use hm_index::synth::Edit;
use hm_index::{synth_go, synth_py};
use hm_store::Store;
use proptest::collection::vec;
use proptest::prelude::*;

type Project = fn(&Path, u64, u32);
type Apply = fn(&Path, u64, Edit) -> BTreeSet<String>;

const ALL_EDITS: [Edit; 8] = [
    Edit::BodyChange,
    Edit::SignatureChange,
    Edit::Rename,
    Edit::AddImport,
    Edit::RemoveImport,
    Edit::AddFile,
    Edit::DeleteFile,
    Edit::BreakSyntax,
];

fn edit_strategy() -> impl Strategy<Value = (Edit, u64)> {
    (proptest::sample::select(ALL_EDITS.to_vec()), any::<u64>())
}

/// Indexes the synthetic project of `seed` with `files` files, then applies `edits`
/// one by one and compares the store with a cold index of the same tree after each.
fn check(
    project: Project,
    apply: Apply,
    seed: u64,
    files: u32,
    edits: &[(Edit, u64)],
) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), seed, files);
    let mut ix = Indexer::open(dir.path(), Store::open_in_memory().unwrap()).unwrap();
    ix.full().unwrap();
    for (i, (edit, seed)) in edits.iter().enumerate() {
        let changed = apply(dir.path(), *seed, *edit);
        ix.apply(&changed).unwrap();
        prop_assert_eq!(
            dump(ix.store()),
            dump(fresh_index_of(dir.path()).store()),
            "after edit {} ({:?}, seed {}) of {:?}",
            i,
            edit,
            seed,
            changed
        );
    }
    drop(ix);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]
    #[test]
    fn go_incremental_equals_cold(edits in vec(edit_strategy(), 1..=12)) {
        check(synth_go::project, synth_go::apply_edit, 7, 24, &edits)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]
    #[test]
    fn python_incremental_equals_cold(edits in vec(edit_strategy(), 1..=12)) {
        check(synth_py::project, synth_py::apply_edit, 7, 24, &edits)?;
    }
}

/// Every file under `dir`: relative path and bytes.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, rel: &str, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(base).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            if entry.path().is_dir() {
                walk(&entry.path(), &rel, out);
            } else {
                out.insert(rel, fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, "", &mut out);
    out
}

/// The paths whose bytes differ between two trees, or that exist in one only.
fn diff(a: &BTreeMap<String, Vec<u8>>, b: &BTreeMap<String, Vec<u8>>) -> BTreeSet<String> {
    a.keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}

/// The rows of one `dump` section.
fn section(text: &str, name: &str) -> Vec<String> {
    let header = format!("[{name}] ");
    text.lines()
        .skip_while(|l| !l.starts_with(&header))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .map(str::to_string)
        .collect()
}

/// The stored edges with `rule`, as columns: kind, `src -> dst`, file, line, rule,
/// prov, conf.
fn edges_with(text: &str, rule: &str) -> Vec<Vec<String>> {
    section(text, "edge")
        .iter()
        .map(|e| e.split(" | ").map(str::to_string).collect::<Vec<_>>())
        .filter(|cols| cols[4] == rule)
        .collect()
}

fn cold_dump(dir: &Path) -> (String, hm_index::index::IndexReport) {
    let store = Store::open_in_memory().unwrap();
    let mut ix = Indexer::open(dir, store).unwrap();
    let report = ix.full().unwrap();
    (dump(ix.store()), report)
}

#[test]
fn go_and_python_projects_are_deterministic() {
    for (name, project, files) in [
        ("go", synth_go::project as Project, 24),
        ("python", synth_py::project as Project, 24),
    ] {
        let (a, b, c) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        project(a.path(), 7, files);
        project(b.path(), 7, files);
        project(c.path(), 8, files);
        let tree_a = tree(a.path());
        assert_eq!(tree_a, tree(b.path()), "{name}: one seed, one tree");
        assert_ne!(tree_a, tree(c.path()), "{name}: another seed, another tree");
        let (_, report) = cold_dump(a.path());
        assert_eq!(report.parsed_with_errors, 0, "{name}");
        assert!(report.parsed >= files, "{name}: {}", report.parsed);
    }
}

#[test]
fn the_go_project_has_every_edge_kind_the_property_test_needs() {
    let dir = tempfile::tempdir().unwrap();
    synth_go::project(dir.path(), 7, 24);
    let (text, _) = cold_dump(dir.path());
    for rule in [
        "go.import",
        "go.call.local",
        "go.call.package",
        "go.call.receiver",
        "go.call.method",
    ] {
        let found = if rule == "go.import" {
            section(&text, "edge")
                .iter()
                .any(|e| e.starts_with("imports | "))
        } else {
            !edges_with(&text, rule).is_empty()
        };
        assert!(found, "no {rule} edge");
    }
    // A method name that two types share stays a ref.
    let shared_refs = section(&text, "ref")
        .iter()
        .filter(|r| r.contains(" | call | ") && r.contains(".Sync") && r.ends_with("unresolved"))
        .count();
    assert!(shared_refs > 0, "no ref on a shared method name");
    let test_files = tree(dir.path())
        .keys()
        .filter(|k| k.ends_with("_test.go"))
        .count();
    assert_eq!(test_files, 2);
}

#[test]
fn the_python_project_has_every_edge_kind_the_property_test_needs() {
    let dir = tempfile::tempdir().unwrap();
    synth_py::project(dir.path(), 7, 24);
    let (text, _) = cold_dump(dir.path());
    for rule in [
        "py.call.local",
        "py.call.self",
        "py.call.attr",
        "py.call.inherited",
        "py.call.import",
        "py.call.module",
        "py.call.class",
        "py.call.static",
        "py.extends",
    ] {
        let found = if rule == "py.extends" {
            section(&text, "edge")
                .iter()
                .any(|e| e.starts_with("extends | "))
        } else {
            !edges_with(&text, rule).is_empty()
        };
        assert!(found, "no {rule} edge");
    }
    // A call by a name that only a package's `__init__.py` gives (its `as` alias)
    // resolves: the importer reaches the module through the re-export.
    let through_init = edges_with(&text, "py.call.import").iter().any(|cols| {
        let src = fs::read_to_string(dir.path().join(&cols[2])).unwrap();
        let line: usize = cols[3].parse().unwrap();
        src.lines()
            .nth(line - 1)
            .is_some_and(|l| l.trim_start().starts_with("total += g"))
    });
    assert!(
        through_init,
        "no call resolved through an aliased re-export"
    );
    // What a forwarding walk of a few files would miss (they need exact read sets).
    let edges = section(&text, "edge");
    let class_of = |id: &str| {
        id.rsplit_once('.')
            .map_or(id, |(class, _)| class)
            .to_string()
    };
    let direct: BTreeSet<(String, String)> = edges
        .iter()
        .filter(|e| e.starts_with("extends | "))
        .filter_map(|e| {
            let (src, dst) = e.split(" | ").nth(1)?.split_once(" -> ")?;
            Some((src.to_string(), dst.to_string()))
        })
        .collect();
    // A method of a base's base, called through `self`.
    let grand_base = edges_with(&text, "py.call.inherited").iter().any(|cols| {
        let (src, dst) = cols[1].split_once(" -> ").unwrap();
        !direct.contains(&(class_of(src), class_of(dst)))
    });
    assert!(grand_base, "no inherited call to a base's base");
    // An `__init__.py` that hands on a name of another package's `__init__.py`.
    let files = tree(dir.path());
    let init_chain = files.iter().any(|(rel, src)| {
        rel.ends_with("/__init__.py")
            && String::from_utf8_lossy(src)
                .lines()
                .any(|l| l.starts_with("from ..p") || l.starts_with("from synth.p"))
    });
    assert!(init_chain, "no __init__.py re-exports from another one");
    // A dotted call that walks from `synth` to a module no import names.
    let dotted = edges.iter().any(|e| {
        let cols: Vec<&str> = e.split(" | ").collect();
        cols[4] == "py.call.module" && {
            let src = String::from_utf8_lossy(&files[cols[2]]).into_owned();
            let line: usize = cols[3].parse().unwrap();
            src.lines()
                .nth(line - 1)
                .is_some_and(|l| l.contains("+= synth.p"))
        }
    });
    assert!(dotted, "no dotted call from `synth` resolved");
}

/// The files each edit changes are exactly the files whose bytes changed, and the
/// tree keeps parsing after every edit but `BreakSyntax`.
fn edits_change_what_they_say(project: Project, apply: Apply) {
    for edit in ALL_EDITS {
        for seed in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            project(dir.path(), 11, 30);
            let before = tree(dir.path());
            let changed = apply(dir.path(), seed, edit);
            assert!(!changed.is_empty(), "{edit:?} {seed}");
            assert_eq!(changed, diff(&before, &tree(dir.path())), "{edit:?} {seed}");
            let (_, report) = cold_dump(dir.path());
            let broken = u32::from(edit == Edit::BreakSyntax);
            assert_eq!(report.parsed_with_errors, broken, "{edit:?} {seed}");
        }
    }
}

#[test]
fn go_edits_change_what_they_say() {
    edits_change_what_they_say(synth_go::project, synth_go::apply_edit);
}

#[test]
fn python_edits_change_what_they_say() {
    edits_change_what_they_say(synth_py::project, synth_py::apply_edit);
}

#[test]
fn rename_renames_a_declaration_that_another_file_reaches() {
    for (project, apply) in [
        (synth_go::project as Project, synth_go::apply_edit as Apply),
        (synth_py::project, synth_py::apply_edit),
    ] {
        for seed in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            project(dir.path(), 11, 30);
            let (before, _) = cold_dump(dir.path());
            let changed = apply(dir.path(), seed, Edit::Rename);
            assert_eq!(changed.len(), 1, "seed {seed}");
            let (after, _) = cold_dump(dir.path());
            // The callers of the old name lose their edge, or a shared method name
            // turns unique and its callers gain one: the index changes.
            assert_ne!(before, after, "seed {seed}: {changed:?}");
        }
    }
}
