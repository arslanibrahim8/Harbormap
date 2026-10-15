//! The read-only view of the filesystem and git that resolution needs. `RealWorld`
//! (in `git.rs`) reads the machine; `FixtureWorld` replays a recorded JSON world so
//! resolution can be tested on any OS.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hm_core::model::Worktree;
use hm_core::raw::RawInventory;

use crate::paths::within;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoInfo {
    /// Root of the main worktree; this is the project root.
    pub root: PathBuf,
    /// Every worktree of the repository, the main one first.
    pub worktrees: Vec<Worktree>,
}

pub trait World {
    /// The repository whose worktree contains `dir`, if any.
    fn repo_for(&self, dir: &Path) -> Option<RepoInfo>;
    fn read_to_string(&self, path: &Path) -> Option<String>;
    /// Names of the files directly inside `dir`.
    fn file_names(&self, dir: &Path) -> Vec<String>;
    fn home(&self) -> Option<PathBuf>;
    fn canonicalize(&self, path: &Path) -> PathBuf;
    /// True when paths compare case-insensitively (Windows).
    fn windows(&self) -> bool;
}

/// A recorded world: repositories, file contents and the home directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixtureWorld {
    pub home: Option<PathBuf>,
    pub windows: bool,
    pub repos: Vec<RepoInfo>,
    /// File path → contents.
    pub files: BTreeMap<PathBuf, String>,
}

impl World for FixtureWorld {
    fn repo_for(&self, dir: &Path) -> Option<RepoInfo> {
        self.repos
            .iter()
            .filter_map(|repo| {
                repo.worktrees
                    .iter()
                    .filter(|worktree| within(dir, &worktree.path, self.windows))
                    .map(|worktree| worktree.path.as_os_str().len())
                    .max()
                    .map(|depth| (depth, repo))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, repo)| repo.clone())
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn file_names(&self, dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .keys()
            .filter(|path| path.parent() == Some(dir))
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    fn windows(&self) -> bool {
        self.windows
    }
}

/// A recorded machine: what the probes saw, the world around it and the folder
/// Harbormap was started in. Tests use it, and so does the CLI in debug builds when
/// `HARBORMAP_FIXTURE` points at one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub launch_dir: PathBuf,
    pub world: FixtureWorld,
    pub inventory: RawInventory,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> FixtureWorld {
        serde_json::from_str(
            r#"{
              "home": "/home/me",
              "repos": [
                {"root": "/work/shop", "worktrees": [
                  {"path": "/work/shop", "branch": "main", "head": "a1"},
                  {"path": "/work/shop-feature", "branch": "feature", "head": "b2"}
                ]},
                {"root": "/work/shop/vendor/lib", "worktrees": [
                  {"path": "/work/shop/vendor/lib", "branch": "main", "head": "c3"}
                ]}
              ],
              "files": {
                "/work/shop/apps/web/package.json": "{\"name\": \"web\"}",
                "/work/shop/apps/web/vite.config.ts": ""
              }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn finds_the_innermost_repository_including_linked_worktrees() {
        let world = world();
        assert_eq!(
            world
                .repo_for(Path::new("/work/shop/apps/web"))
                .unwrap()
                .root,
            Path::new("/work/shop")
        );
        assert_eq!(
            world
                .repo_for(Path::new("/work/shop-feature/api"))
                .unwrap()
                .root,
            Path::new("/work/shop")
        );
        assert_eq!(
            world
                .repo_for(Path::new("/work/shop/vendor/lib/src"))
                .unwrap()
                .root,
            Path::new("/work/shop/vendor/lib")
        );
        assert!(world.repo_for(Path::new("/tmp")).is_none());
    }

    #[test]
    fn lists_files_of_a_directory() {
        assert_eq!(
            world().file_names(Path::new("/work/shop/apps/web")),
            vec!["package.json", "vite.config.ts"]
        );
    }
}
