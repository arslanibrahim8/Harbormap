//! Picks the extractor and the resolver of each file by its language, and says what
//! a change to one file can reach in files that hold no `reverse_import` row to it:
//! Go package siblings and Go method-name callers. A Python file needs neither: its
//! rows name every file its resolution read.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::facts::{FileFacts, ParseStatus};
use crate::go::gomod::GoModules;
use crate::go::resolve::{
    GoProject, MethodIndex, calls_any_method, method_names, package_siblings,
};
use crate::lang::Lang;
use crate::py::modules::PyModules;
use crate::py::resolve::PyProject;
use crate::ts::resolve::{ModuleResolver, Project, ResolvedFile, symbol_ids};
use crate::ts::tsconfig::load_tsconfigs;
use crate::{go, py, ts};

fn lang_of(rel: &str) -> Option<Lang> {
    Lang::from_path(Path::new(rel))
}

/// The facts of `src` in `lang`; a language without an extractor gives no facts.
pub fn extract(src: &str, lang: Lang) -> FileFacts {
    match lang {
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => ts::extract(src, lang),
        Lang::Go => go::extract(src),
        Lang::Python => py::extract(src),
        _ => FileFacts::empty(ParseStatus::ParsedWithErrors),
    }
}

/// The module lookups of every language, built from one set of known files.
pub struct Resolvers {
    pub ts: ModuleResolver,
    pub go: GoModules,
    pub py: PyModules,
    /// What the tsconfigs could not say, kept from loading them.
    ts_notes: Vec<String>,
}

impl Resolvers {
    /// Loads the tsconfigs and `go.mod` files among `known` from under `root`.
    pub fn load(root: &Path, known: BTreeSet<String>) -> Resolvers {
        let tsconfigs = load_tsconfigs(root, &known);
        let ts_notes = tsconfigs.notes.clone();
        let go = GoModules::load(root, &known);
        let py = PyModules::new(known.clone());
        Resolvers {
            ts: ModuleResolver::new(known, tsconfigs),
            go,
            py,
            ts_notes,
        }
    }

    /// The tsconfig and `go.mod` files: a change to one rebuilds the resolvers.
    pub fn config_sources(&self) -> BTreeSet<String> {
        let mut sources = self.ts.config_sources();
        sources.extend(self.go.config_sources());
        sources
    }

    /// The tsconfig and `go.mod` notes, sorted.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = self.ts_notes.clone();
        notes.extend(self.go.notes().iter().cloned());
        notes.sort();
        notes
    }
}

/// The facts of a worktree with its resolvers, as each language's resolver takes them.
pub struct Workspace<'a> {
    facts: &'a BTreeMap<String, FileFacts>,
    resolvers: &'a Resolvers,
    /// Built on the first Go resolve, so a pass builds it at most once.
    methods: OnceCell<MethodIndex>,
}

impl<'a> Workspace<'a> {
    pub fn new(facts: &'a BTreeMap<String, FileFacts>, resolvers: &'a Resolvers) -> Workspace<'a> {
        Workspace {
            facts,
            resolvers,
            methods: OnceCell::new(),
        }
    }

    pub fn ts(&self) -> Project<'a> {
        Project {
            facts: self.facts,
            modules: &self.resolvers.ts,
        }
    }

    pub fn go(&self) -> GoProject<'_> {
        GoProject {
            facts: self.facts,
            modules: &self.resolvers.go,
            methods: self.methods.get_or_init(|| MethodIndex::build(self.facts)),
        }
    }

    pub fn py(&self) -> PyProject<'a> {
        PyProject {
            facts: self.facts,
            modules: &self.resolvers.py,
        }
    }
}

/// Resolves the file at `rel` with the resolver of its language.
pub fn resolve_file(w: &Workspace, rel: &str) -> ResolvedFile {
    match lang_of(rel) {
        Some(Lang::TypeScript | Lang::Tsx | Lang::JavaScript) => {
            ts::resolve::resolve_file(&w.ts(), rel)
        }
        Some(Lang::Go) => go::resolve::resolve_file(&w.go(), rel),
        Some(Lang::Python) => py::resolve::resolve_file(&w.py(), rel),
        _ => ResolvedFile::default(),
    }
}

/// The files that read `rel`'s facts without importing it: for Go, the other files
/// of its package, which read its symbols and its package clause.
pub fn implicit_dependents(w: &Workspace, rel: &str) -> Vec<String> {
    match lang_of(rel) {
        Some(Lang::Go) => package_siblings(w.facts, rel),
        _ => Vec::new(),
    }
}

/// The names that calls in any file may reach by name alone: for Go, the method
/// names of `facts`.
pub fn inferred_names(rel: &str, facts: &FileFacts) -> BTreeSet<String> {
    match lang_of(rel) {
        Some(Lang::Go) => method_names(facts),
        _ => BTreeSet::new(),
    }
}

/// Whether resolving `facts` reads what any file declares under one of `names`:
/// for Go, a call with a receiver to one of them.
pub fn reads_names(rel: &str, facts: &FileFacts, names: &BTreeSet<String>) -> bool {
    match lang_of(rel) {
        Some(Lang::Go) => calls_any_method(facts, names),
        _ => false,
    }
}

/// The [`inferred_names`] of `rel` that the change from `old` to `new` facts
/// gained, lost or gave another symbol id or package clause: a name-match edge
/// stores the id of the one symbol of that name, so a method whose receiver changes
/// moves it too, and a Go method counts as a target or not by its file's package
/// (`main` elsewhere is another binary). An interface's method specs are among the
/// names: they decide whether a name is inferred at all.
pub fn inferred_changes(
    rel: &str,
    old: Option<&FileFacts>,
    new: Option<&FileFacts>,
) -> BTreeSet<String> {
    let named = |facts: Option<&FileFacts>| -> BTreeSet<(String, String, Option<String>)> {
        let Some(facts) = facts else {
            return BTreeSet::new();
        };
        let names = inferred_names(rel, facts);
        if names.is_empty() {
            return BTreeSet::new();
        }
        symbol_ids(rel, facts)
            .into_iter()
            .zip(&facts.symbols)
            .filter(|(_, s)| names.contains(&s.name))
            .map(|(id, s)| (s.name.clone(), id, facts.package.clone()))
            .collect()
    };
    named(old)
        .symmetric_difference(&named(new))
        .map(|(name, ..)| name.clone())
        .collect()
}
