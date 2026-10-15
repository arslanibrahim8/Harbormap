//! The real world: git through gix (no `git` binary needed) and the local filesystem.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hm_core::model::Worktree;

use crate::world::{RepoInfo, World};

/// Reads the machine. Repository lookups are cached per directory.
#[derive(Debug, Default)]
pub struct RealWorld {
    repos: RefCell<HashMap<PathBuf, Option<RepoInfo>>>,
}

impl RealWorld {
    pub fn new() -> Self {
        Self::default()
    }
}

impl World for RealWorld {
    fn repo_for(&self, dir: &Path) -> Option<RepoInfo> {
        if let Some(cached) = self.repos.borrow().get(dir) {
            return cached.clone();
        }
        let found = discover(dir);
        self.repos
            .borrow_mut()
            .insert(dir.to_path_buf(), found.clone());
        found
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn file_names(&self, dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::home_dir().map(|home| self.canonicalize(&home))
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn windows(&self) -> bool {
        cfg!(windows)
    }
}

/// The repository containing `dir`: main worktree root plus every linked worktree.
pub fn discover(dir: &Path) -> Option<RepoInfo> {
    let repo = gix::discover(dir).ok()?;
    let main = repo.main_repo().ok()?;
    let mut worktrees = Vec::new();
    let root = match main.workdir() {
        Some(workdir) => {
            let root = canonical(workdir);
            worktrees.push(Worktree {
                path: root.clone(),
                branch: branch(&main),
                head: head(&main),
            });
            root
        }
        // A bare repository holds no files: it is the root, and only its linked
        // worktrees are worktrees.
        None => bare_root(main.git_dir()),
    };
    for proxy in main.worktrees().ok()? {
        let Ok(base) = proxy.base() else {
            continue;
        };
        let linked = proxy.into_repo_with_possibly_inaccessible_worktree().ok();
        worktrees.push(Worktree {
            path: canonical(&base),
            branch: linked.as_ref().and_then(branch),
            head: linked.as_ref().and_then(head),
        });
    }
    (!worktrees.is_empty()).then_some(RepoInfo { root, worktrees })
}

/// `shop.git` is its own root; a `.bare` or `.git` folder stands for the folder holding it
/// (the layout where worktrees sit next to it).
fn bare_root(git_dir: &Path) -> PathBuf {
    let git_dir = canonical(git_dir);
    match git_dir.file_name().and_then(|name| name.to_str()) {
        Some(".bare" | ".git") => git_dir
            .parent()
            .map_or_else(|| git_dir.clone(), Path::to_path_buf),
        _ => git_dir,
    }
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn branch(repo: &gix::Repository) -> Option<String> {
    repo.head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string())
}

fn head(repo: &gix::Repository) -> Option<String> {
    repo.head_id().ok().map(|id| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Runs git without the developer's own configuration (signing, hooks, templates) and
    /// without the repository a git hook running these tests points at.
    fn git(dir: &Path, args: &[&str]) -> bool {
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let mut command = Command::new("git");
        for variable in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
        ] {
            command.env_remove(variable);
        }
        command
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", null)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn discovers_root_branch_and_linked_worktrees() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("shop");
        std::fs::create_dir_all(repo.join("apps/web")).unwrap();
        if !git(temp.path(), &["--version"]) {
            eprintln!("git is not installed; skipping");
            return;
        }
        assert!(git(&repo, &["init", "-q", "-b", "main"]));
        assert!(git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]));
        assert!(git(
            &repo,
            &["worktree", "add", "-q", "-b", "feature", "../shop-feature"]
        ));

        let info = discover(&repo.join("apps/web")).unwrap();
        assert_eq!(info.root, canonical(&repo));
        assert_eq!(info.worktrees.len(), 2);
        assert_eq!(info.worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(
            info.worktrees[1].path,
            canonical(&temp.path().join("shop-feature"))
        );
        assert_eq!(info.worktrees[1].branch.as_deref(), Some("feature"));
        assert_eq!(info.worktrees[0].head.as_ref().map(String::len), Some(40));

        let from_linked = discover(&temp.path().join("shop-feature")).unwrap();
        assert_eq!(from_linked.root, info.root);

        // A bare repository holds no files itself: it is the root, its linked worktrees
        // are the worktrees.
        let bare = temp.path().join("shop.git");
        let (from, to) = (repo.to_str().unwrap(), bare.to_str().unwrap());
        assert!(git(temp.path(), &["clone", "-q", "--bare", from, to]));
        assert!(git(
            &bare,
            &["worktree", "add", "-q", "../shop-main", "main"]
        ));
        let from_bare = discover(&temp.path().join("shop-main")).unwrap();
        assert_eq!(from_bare.root, canonical(&bare));
        assert_eq!(from_bare.worktrees.len(), 1);
        assert_eq!(
            from_bare.worktrees[0].path,
            canonical(&temp.path().join("shop-main"))
        );
        assert_eq!(from_bare.worktrees[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn a_folder_without_git_has_no_repository() {
        let temp = tempfile::tempdir().unwrap();
        // The temp dir might sit inside a repository on a developer machine; only
        // assert when it does not.
        if gix::discover(temp.path()).is_err() {
            assert!(discover(temp.path()).is_none());
        }
    }
}
