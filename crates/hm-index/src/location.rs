//! Where a worktree's store lives and which folder counts as the worktree (Learn
//! spec §4). Nothing here writes: the store sits in the platform data directory,
//! never inside the user's repository.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use hm_core::ids::worktree_key;
use hm_project::git::{RealWorld, discover};
use hm_project::paths::{is_boundary, within};
use hm_project::world::World;

/// Why a folder cannot be indexed.
#[derive(Debug)]
pub enum LocationError {
    /// The folder is the home folder, `/` or a drive root.
    NotAProject(PathBuf),
    /// The folder cannot be opened.
    Unreadable(PathBuf, io::Error),
}

impl fmt::Display for LocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocationError::NotAProject(path) => write!(
                f,
                "{} is not a project folder (it is your home folder or a drive root)",
                path.display()
            ),
            LocationError::Unreadable(path, err) => {
                write!(f, "cannot open {}: {err}", path.display())
            }
        }
    }
}

impl std::error::Error for LocationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LocationError::NotAProject(_) => None,
            LocationError::Unreadable(_, err) => Some(err),
        }
    }
}

/// The worktree that holds `path`: the git worktree whose folder contains it, or else
/// `path` itself, canonical. The home folder, `/` and a drive root are refused, as in
/// the project view.
pub fn worktree_root(path: &Path) -> Result<PathBuf, LocationError> {
    let canonical = dunce::canonicalize(path)
        .map_err(|err| LocationError::Unreadable(path.to_path_buf(), err))?;
    let root = discover(&canonical)
        .and_then(|repo| {
            repo.worktrees
                .into_iter()
                .map(|worktree| worktree.path)
                .filter(|root| within(&canonical, root, cfg!(windows)))
                // A linked worktree can sit inside another one: the deepest wins.
                .max_by_key(|root| root.as_os_str().len())
        })
        .unwrap_or(canonical);
    if is_boundary(&root, RealWorld::new().home().as_deref(), cfg!(windows)) {
        return Err(LocationError::NotAProject(root));
    }
    Ok(root)
}

/// The facts store of a worktree: `<data>/learn/<worktree_key>/facts.sqlite`. `<data>`
/// is `$HARBORMAP_DATA_DIR` when that is set, otherwise the platform data directory
/// for `harbormap`.
pub fn facts_db(worktree_root: &Path) -> PathBuf {
    data_dir()
        .join("learn")
        .join(worktree_key(worktree_root))
        .join("facts.sqlite")
}

fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HARBORMAP_DATA_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    directories::ProjectDirs::from("", "", "harbormap")
        .map(|dirs| dirs.data_dir().to_path_buf())
        // No home folder to anchor a data directory: the temp folder is the last resort.
        .unwrap_or_else(|| std::env::temp_dir().join("harbormap"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_path_is_keyed_by_the_worktree() {
        let a = facts_db(Path::new("/work/shop"));
        let b = facts_db(Path::new("/work/blog"));
        assert_ne!(a, b);
        assert!(a.ends_with("facts.sqlite"));
        let key = a
            .parent()
            .and_then(Path::file_name)
            .unwrap()
            .to_string_lossy();
        assert!(key.starts_with("w_") && key.len() == 14);
        assert_eq!(
            a.parent()
                .and_then(Path::parent)
                .unwrap()
                .file_name()
                .unwrap(),
            "learn"
        );
    }

    #[test]
    fn a_plain_folder_is_its_own_worktree_and_the_filesystem_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("a");
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(
            worktree_root(&sub).unwrap(),
            dunce::canonicalize(&sub).unwrap()
        );
        let root = std::env::current_dir()
            .unwrap()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();
        assert!(matches!(
            worktree_root(&root),
            Err(LocationError::NotAProject(_))
        ));
        assert!(matches!(
            worktree_root(&dir.path().join("missing")),
            Err(LocationError::Unreadable(..))
        ));
    }
}
