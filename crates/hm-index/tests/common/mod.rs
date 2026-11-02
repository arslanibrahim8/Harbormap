//! Helpers shared by the integration tests; later tasks add to it.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The folder of fixture `name` under `tests/fixtures`.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The text of a fixture file; `rel` is relative to `tests/fixtures`, as in
/// `ts-basic/src/db.ts`.
pub fn read_fixture(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A copy of fixture `name` in a temporary folder that is not a git repository. Every
/// file on disk comes along, including the ignored and skipped ones.
pub fn copy_fixture(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    copy_tree(&fixture(name), dir.path()).expect("copy the fixture");
    dir
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
