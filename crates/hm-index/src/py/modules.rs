//! Finds the Python module a dotted import names, from the file layout alone: the
//! package roots are the repository root, each folder with a `pyproject.toml`,
//! `setup.py`, `setup.cfg`, `requirements*.txt`, `Pipfile` or `manage.py`, and the
//! `src/` folder of each of these. The contents of those files are not read.

use std::collections::{BTreeSet, HashSet};

/// The files that make a folder a package root, besides `requirements*.txt`.
const ROOT_MARKERS: &[&str] = &[
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "Pipfile",
    "manage.py",
];

/// Whether a file named `name` makes its folder a package root.
fn is_root_marker(name: &str) -> bool {
    ROOT_MARKERS.contains(&name)
        || name
            .strip_prefix("requirements")
            .is_some_and(|rest| rest.ends_with(".txt"))
}

/// A module or package: its file (`a/b.py`, `a/b/__init__.py`) and, for a package, its
/// folder. A namespace package has a folder and no file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PyModule {
    pub file: Option<String>,
    pub package_dir: Option<String>,
}

/// Where an import specifier points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyTarget {
    Module(PyModule),
    /// No root has the first segment: a package from outside, by that segment.
    External(String),
    /// A root claims the first segment, or the import is relative, but nothing matches.
    Unresolved,
}

/// The known files of a worktree, indexed once for module lookups.
#[derive(Debug, Clone)]
pub struct PyModules {
    known: BTreeSet<String>,
    /// Every folder with a `.py` or `.pyi` file at any depth below it; `""` is the root.
    py_dirs: HashSet<String>,
    /// Sorted; `""` is the repository root.
    roots: Vec<String>,
}

fn is_python(rel: &str) -> bool {
    rel.ends_with(".py") || rel.ends_with(".pyi")
}

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `dir/rest`, or `rest` when `dir` is the root, or `dir` when `rest` is empty.
fn join(dir: &str, rest: &str) -> String {
    match (dir.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (false, true) => dir.to_string(),
        (false, false) => format!("{dir}/{rest}"),
    }
}

/// Whether the folder `root` holds `rel` at any depth.
fn contains(root: &str, rel: &str) -> bool {
    root.is_empty()
        || rel
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl PyModules {
    /// `known` is the set of relative file paths that exist in the worktree, of any kind.
    pub fn new(known: BTreeSet<String>) -> PyModules {
        let mut py_dirs = HashSet::new();
        for rel in known.iter().filter(|r| is_python(r)) {
            let mut dir = parent(rel);
            // Stop at the first folder already seen: its ancestors are in too.
            while py_dirs.insert(dir.to_string()) && !dir.is_empty() {
                dir = parent(dir);
            }
        }
        let mut bases: BTreeSet<String> = BTreeSet::from([String::new()]);
        for rel in &known {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            if is_root_marker(name) {
                bases.insert(parent(rel).to_string());
            }
        }
        let mut roots = bases.clone();
        for base in &bases {
            let src = join(base, "src");
            if py_dirs.contains(&src) {
                roots.insert(src);
            }
        }
        PyModules {
            known,
            py_dirs,
            roots: roots.into_iter().collect(),
        }
    }

    /// The package roots, sorted; `""` is the repository root.
    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    /// Resolves `specifier` (`a.b`, `.`, `..x`) as imported from the file `from_rel`.
    /// The first root (in `from_rel`'s order) that has the first segment decides.
    pub fn resolve(&self, from_rel: &str, specifier: &str) -> PyTarget {
        if specifier.starts_with('.') {
            return self.resolve_relative(from_rel, specifier);
        }
        let Some(path) = dotted_path(specifier) else {
            return PyTarget::Unresolved;
        };
        let first = specifier.split('.').next().unwrap_or(specifier);
        for root in self.roots_for(from_rel) {
            if self.probe(&join(root, first)).is_some() {
                return match self.probe(&join(root, &path)) {
                    Some(m) => PyTarget::Module(m),
                    None => PyTarget::Unresolved,
                };
            }
        }
        PyTarget::External(first.to_string())
    }

    /// The submodule or subpackage `name` of the package in `package_dir`.
    pub fn submodule(&self, package_dir: &str, name: &str) -> Option<PyModule> {
        if !is_segment(name) {
            return None;
        }
        self.probe(&join(package_dir, name))
    }

    /// The roots that hold `from_rel`, deepest first, then the others in sorted order.
    fn roots_for<'a>(&'a self, from_rel: &str) -> impl Iterator<Item = &'a str> {
        let mut holding: Vec<&str> = self
            .roots
            .iter()
            .map(String::as_str)
            .filter(|r| contains(r, from_rel))
            .collect();
        holding.sort_by_key(|r| std::cmp::Reverse(r.len()));
        let from_rel = from_rel.to_string();
        let others = self
            .roots
            .iter()
            .map(String::as_str)
            .filter(move |r| !contains(r, &from_rel));
        holding.into_iter().chain(others)
    }

    /// `from_rel`'s folder, one folder up per dot after the first, then the rest.
    /// Going above the repository root is unresolved.
    fn resolve_relative(&self, from_rel: &str, specifier: &str) -> PyTarget {
        let rest = specifier.trim_start_matches('.');
        let ups = specifier.len() - rest.len() - 1;
        let mut base = Some(parent(from_rel));
        for _ in 0..ups {
            base = base.and_then(|b| (!b.is_empty()).then(|| parent(b)));
        }
        let Some(base) = base else {
            return PyTarget::Unresolved;
        };
        let path = if rest.is_empty() {
            Some(String::new())
        } else {
            dotted_path(rest)
        };
        match path.and_then(|p| self.probe(&join(base, &p))) {
            Some(m) => PyTarget::Module(m),
            None => PyTarget::Unresolved,
        }
    }

    /// The module at the folder-or-stem `path`, in this order: a package's
    /// `__init__.py`, a module `.py`, a package's `__init__.pyi`, a stub `.pyi`, and a
    /// namespace package (a folder with Python files below it).
    fn probe(&self, path: &str) -> Option<PyModule> {
        let package = |file: String| PyModule {
            file: Some(file),
            package_dir: Some(path.to_string()),
        };
        let module = |file: String| PyModule {
            file: Some(file),
            package_dir: None,
        };
        let init = join(path, "__init__.py");
        if self.known.contains(&init) {
            return Some(package(init));
        }
        let file = format!("{path}.py");
        if !path.is_empty() && self.known.contains(&file) {
            return Some(module(file));
        }
        let init = join(path, "__init__.pyi");
        if self.known.contains(&init) {
            return Some(package(init));
        }
        let file = format!("{path}.pyi");
        if !path.is_empty() && self.known.contains(&file) {
            return Some(module(file));
        }
        self.py_dirs.contains(path).then(|| PyModule {
            file: None,
            package_dir: Some(path.to_string()),
        })
    }
}

fn is_segment(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\\', '.'])
}

/// `a.b.c` as the path `a/b/c`; `None` when a segment is empty.
fn dotted_path(specifier: &str) -> Option<String> {
    let segments: Vec<&str> = specifier.split('.').collect();
    segments
        .iter()
        .all(|s| is_segment(s))
        .then(|| segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules(files: &[&str]) -> PyModules {
        PyModules::new(files.iter().map(|f| f.to_string()).collect())
    }

    fn module(file: Option<&str>, dir: Option<&str>) -> PyTarget {
        PyTarget::Module(PyModule {
            file: file.map(str::to_string),
            package_dir: dir.map(str::to_string),
        })
    }

    #[test]
    fn a_package_init_beats_a_module_and_a_stub_comes_last() {
        let m = modules(&["a/__init__.py", "a.py", "b.pyi", "c/__init__.pyi", "c.py"]);
        assert_eq!(
            m.resolve("x.py", "a"),
            module(Some("a/__init__.py"), Some("a"))
        );
        assert_eq!(m.resolve("x.py", "b"), module(Some("b.pyi"), None));
        assert_eq!(m.resolve("x.py", "c"), module(Some("c.py"), None));
    }

    #[test]
    fn a_namespace_package_has_a_folder_and_no_file() {
        let m = modules(&["ns/sub/m.py", "ns/README.md"]);
        assert_eq!(m.resolve("x.py", "ns"), module(None, Some("ns")));
        assert_eq!(
            m.resolve("x.py", "ns.sub.m"),
            module(Some("ns/sub/m.py"), None)
        );
        assert_eq!(m.resolve("x.py", "ns.other"), PyTarget::Unresolved);
        // A folder without Python files is no package.
        let m = modules(&["docs/a.md"]);
        assert_eq!(m.resolve("x.py", "docs"), PyTarget::External("docs".into()));
    }

    #[test]
    fn the_roots_holding_the_file_come_first_deepest_first() {
        let m = modules(&[
            "pyproject.toml",
            "lib/setup.cfg",
            "lib/src/util/__init__.py",
            "util/__init__.py",
        ]);
        assert_eq!(m.roots(), ["", "lib", "lib/src"]);
        assert_eq!(
            m.resolve("lib/src/util/x.py", "util"),
            module(Some("lib/src/util/__init__.py"), Some("lib/src/util"))
        );
        assert_eq!(
            m.resolve("app.py", "util"),
            module(Some("util/__init__.py"), Some("util"))
        );
    }

    #[test]
    fn relative_imports_climb_one_folder_per_extra_dot() {
        let m = modules(&["a/__init__.py", "a/b/c.py", "a/d.py", "e.py"]);
        assert_eq!(m.resolve("a/b/c.py", "..d"), module(Some("a/d.py"), None));
        assert_eq!(
            m.resolve("a/b/c.py", ".."),
            module(Some("a/__init__.py"), Some("a"))
        );
        assert_eq!(m.resolve("a/b/c.py", "...e"), module(Some("e.py"), None));
        assert_eq!(m.resolve("a/b/c.py", "....e"), PyTarget::Unresolved);
        assert_eq!(m.resolve("a/b/c.py", ".missing"), PyTarget::Unresolved);
    }
}
