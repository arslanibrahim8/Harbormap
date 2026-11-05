//! Reads `go.mod` files and maps Go import paths to folders of the worktree. Only the
//! `module` line is read; `require`, `replace` and `go.work` are not (Learn L2b plan).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Where an import path points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoTarget {
    /// A folder of the worktree that holds at least one non-test `.go` file.
    Package(String),
    /// A path under no module of the worktree, by import path.
    External(String),
    /// Under a module of the worktree, but its folder holds no non-test `.go` file.
    Unresolved,
}

/// The module path a `go.mod` declares: the value of its `module` line, quoted or
/// not, with `//` comments allowed. `None` when there is no such line.
pub fn module_path(go_mod: &str) -> Option<String> {
    go_mod.lines().find_map(|line| {
        let line = line.split_once("//").map_or(line, |(code, _)| code).trim();
        let rest = line.strip_prefix("module")?;
        if !(rest.starts_with(char::is_whitespace) || rest.starts_with('"')) {
            return None;
        }
        let path = rest.trim().trim_matches(|c| c == '"' || c == '`');
        (!path.is_empty()).then(|| path.to_string())
    })
}

/// The modules of a worktree and the `.go` files of each folder.
#[derive(Debug, Clone, Default)]
pub struct GoModules {
    /// Folder to the `.go` files directly in it, sorted.
    go_files: BTreeMap<String, Vec<String>>,
    /// `(go.mod folder, module path)`, longest module path first.
    modules: Vec<(String, String)>,
    /// Every `go.mod` path of the known files.
    sources: BTreeSet<String>,
    notes: Vec<String>,
}

impl GoModules {
    /// `known` is the set of relative file paths of the worktree; `modules` maps a
    /// `go.mod` folder (`""` for the root) to its module path.
    pub fn new(
        known: BTreeSet<String>,
        modules: BTreeMap<String, String>,
        notes: Vec<String>,
    ) -> GoModules {
        let mut go_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut sources = BTreeSet::new();
        for rel in known {
            let (dir, name) = split(&rel);
            if name == "go.mod" {
                sources.insert(rel);
            } else if name.ends_with(".go") {
                go_files.entry(dir.to_string()).or_default().push(rel);
            }
        }
        let mut modules: Vec<(String, String)> = modules.into_iter().collect();
        modules.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.cmp(b)));
        GoModules {
            go_files,
            modules,
            sources,
            notes,
        }
    }

    /// Reads every `go.mod` in `known` from under `root`. A file that cannot be read
    /// or has no `module` line adds a note and is not a module; nothing is an error.
    pub fn load(root: &Path, known: &BTreeSet<String>) -> GoModules {
        let mut modules = BTreeMap::new();
        let mut notes = Vec::new();
        for rel in known {
            let (dir, name) = split(rel);
            if name != "go.mod" {
                continue;
            }
            let Ok(text) = fs::read_to_string(root.join(rel)) else {
                notes.push(format!("go.mod at {rel} is missing or unreadable"));
                continue;
            };
            match module_path(&text) {
                Some(path) => {
                    modules.insert(dir.to_string(), path);
                }
                None => notes.push(format!("go.mod at {rel} has no module line")),
            }
        }
        GoModules::new(known.clone(), modules, notes)
    }

    /// Resolves an import path: the longest module whose path equals it or is a
    /// prefix of it at a `/` boundary names the folder.
    pub fn resolve(&self, import_path: &str) -> GoTarget {
        let hit = self.modules.iter().find_map(|(dir, module)| {
            let rest = if import_path == module {
                ""
            } else {
                import_path
                    .strip_prefix(module.as_str())?
                    .strip_prefix('/')?
            };
            Some(join(dir, rest))
        });
        let Some(folder) = hit else {
            return GoTarget::External(import_path.to_string());
        };
        if self.files_in(&folder).iter().any(|f| !is_test(f)) {
            GoTarget::Package(folder)
        } else {
            GoTarget::Unresolved
        }
    }

    /// The `.go` files directly in `folder`, sorted.
    pub fn files_in(&self, folder: &str) -> Vec<&str> {
        self.go_files
            .get(folder)
            .map(|files| files.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Every `go.mod` path; the modules must be rebuilt when one of them changes.
    pub fn config_sources(&self) -> BTreeSet<String> {
        self.sources.clone()
    }

    /// What was not read and why, one line each.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }
}

/// Whether `rel` is a Go test file.
pub(crate) fn is_test(rel: &str) -> bool {
    rel.ends_with("_test.go")
}

/// Whether the `go` tool leaves `rel` out of every build: a folder on its path is
/// `testdata` or starts with `_` or `.`.
pub(crate) fn outside_builds(rel: &str) -> bool {
    let (dir, _) = split(rel);
    !dir.is_empty()
        && dir
            .split('/')
            .any(|s| s == "testdata" || s.starts_with(['_', '.']))
}

/// The folder and file name of a relative path; a root file's folder is `""`.
pub(crate) fn split(rel: &str) -> (&str, &str) {
    rel.rsplit_once('/').unwrap_or(("", rel))
}

fn join(dir: &str, rest: &str) -> String {
    match (dir.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_string(),
        (false, true) => dir.to_string(),
        (false, false) => format!("{dir}/{rest}"),
    }
}
