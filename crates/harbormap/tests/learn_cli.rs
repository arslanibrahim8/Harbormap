//! `harbormap index` and `harbormap card` against a copy of the ts-basic project.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn ts_basic() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../hm-index/tests/fixtures/ts-basic")
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A copy of the project and an empty data directory, both temporary.
struct Setup {
    project: TempDir,
    data: TempDir,
}

fn setup() -> Setup {
    let project = tempfile::tempdir().unwrap();
    copy_tree(&ts_basic(), project.path());
    Setup {
        project,
        data: tempfile::tempdir().unwrap(),
    }
}

impl Setup {
    fn harbormap(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("harbormap").unwrap();
        cmd.current_dir(self.project.path())
            .env("HARBORMAP_DATA_DIR", self.data.path())
            .args(args);
        cmd
    }

    fn stores(&self) -> Vec<PathBuf> {
        let Ok(dir) = fs::read_dir(self.data.path().join("learn")) else {
            return Vec::new();
        };
        dir.flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("w_"))
            })
            .map(|path| path.join("facts.sqlite"))
            .filter(|path| path.exists())
            .collect()
    }
}

/// Every file under `dir` with its path, size and bytes, sorted by path.
fn tree(dir: &Path) -> Vec<(String, u64, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let bytes = fs::read(&path).unwrap();
                let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push((rel.replace('\\', "/"), bytes.len() as u64, bytes));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn stdout_of(cmd: &mut Command) -> String {
    String::from_utf8(cmd.assert().success().get_output().stdout.clone()).unwrap()
}

#[test]
fn index_prints_counts_and_writes_the_store_outside_the_project() {
    let s = setup();
    let before = tree(s.project.path());
    let text = stdout_of(&mut s.harbormap(&["index"]));

    assert!(
        text.contains("TypeScript 7 · TSX 2 · JavaScript 1"),
        "{text}"
    );
    assert!(text.contains("not yet supported: Python 1"), "{text}");
    assert!(
        text.contains("Tests 1 · Generated 1 · Docs 1 · Infra 1"),
        "{text}"
    );
    assert!(text.contains("node_modules (1 folder)"), "{text}");
    assert!(text.contains("dist (1 folder)"), "{text}");
    assert!(
        text.contains("Symbols 12 · imports 9 · calls 8 · unresolved 2 · external 3"),
        "{text}"
    );
    assert!(text.contains("(16 files)"), "{text}");
    assert!(
        text.lines().last().unwrap().trim().starts_with("Took "),
        "{text}"
    );

    assert_eq!(s.stores().len(), 1, "one store under HARBORMAP_DATA_DIR");
    assert_eq!(tree(s.project.path()), before, "the project is untouched");
}

#[test]
fn index_json_is_the_report() {
    let s = setup();
    let text = stdout_of(&mut s.harbormap(&["index", "--json"]));
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["parsed"], 10);
    assert_eq!(report["edges"]["calls"], 8);
}

#[test]
fn index_takes_a_path() {
    let s = setup();
    let elsewhere = tempfile::tempdir().unwrap();
    Command::cargo_bin("harbormap")
        .unwrap()
        .current_dir(elsewhere.path())
        .env("HARBORMAP_DATA_DIR", s.data.path())
        .arg("index")
        .arg(s.project.path())
        .assert()
        .success()
        .stdout(contains("TypeScript 7"));
}

#[test]
fn card_without_an_index_indexes_first() {
    let s = setup();
    assert!(s.stores().is_empty());
    let text = stdout_of(&mut s.harbormap(&["card", "src/orders/service.ts"]));
    assert!(text.contains("src/orders/service.ts"), "{text}");
    let row = text
        .lines()
        .find(|line| line.contains("placeOrder"))
        .unwrap_or_else(|| panic!("a placeOrder row in\n{text}"));
    assert!(row.contains("called from 3"), "{row}");
    assert_eq!(s.stores().len(), 1);

    s.harbormap(&["card", "src/nope.ts"])
        .assert()
        .code(1)
        .stderr(
            "harbormap: src/nope.ts is not in the index \
             (it may be ignored, skipped or not saved yet)\n",
        );
}

#[test]
fn card_shows_a_symbol_and_json() {
    let s = setup();
    let text = stdout_of(&mut s.harbormap(&["card", "src/orders/service.ts#placeOrder"]));
    assert!(text.contains("src/orders/service.ts#placeOrder"), "{text}");
    assert!(text.contains("src/index.ts#main"), "{text}");
    assert!(text.contains("db.insert"), "{text}");

    let json = stdout_of(&mut s.harbormap(&["card", "src/orders/service.ts#placeOrder", "--json"]));
    let card: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(card["callees"][0], "src/orders/mail.ts#sendReceipt");

    let json = stdout_of(&mut s.harbormap(&["card", "src/orders/service.ts", "--json"]));
    let card: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(card["used_by"][0], "src/index.ts");

    s.harbormap(&["card", "src/orders/service.ts#nope"])
        .assert()
        .code(1)
        .stderr(contains("no symbol called nope"));
}

#[test]
fn card_accepts_absolute_paths() {
    let s = setup();
    let abs = s.project.path().join("src/db.ts");
    Command::cargo_bin("harbormap")
        .unwrap()
        .current_dir(s.project.path())
        .env("HARBORMAP_DATA_DIR", s.data.path())
        .arg("card")
        .arg(abs)
        .assert()
        .success()
        .stdout(contains("src/db.ts"));
}

#[test]
fn card_run_from_a_subfolder_finds_the_git_worktree() {
    let s = setup();
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(s.project.path())
        .status()
        .expect("git is installed");
    assert!(git.success());
    s.harbormap(&["card", "src/db.ts"]).assert().success();
    let mut cmd = s.harbormap(&["card", "orders/service.ts"]);
    cmd.current_dir(s.project.path().join("src"))
        .assert()
        .success()
        .stdout(contains("src/orders/service.ts"));
    assert_eq!(s.stores().len(), 1, "both runs used the same store");
}

fn git_init(dir: &Path) {
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .expect("git is installed");
    assert!(git.success());
}

#[test]
fn card_from_a_plain_parent_folder_uses_the_targets_worktree() {
    let parent = tempfile::tempdir().unwrap();
    let proj = parent.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    copy_tree(&ts_basic(), &proj);
    git_init(&proj);
    let other = parent.path().join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("x.ts"), "export const x = 1;\n").unwrap();
    let data = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let mut cmd = Command::cargo_bin("harbormap").unwrap();
        cmd.current_dir(parent.path())
            .env("HARBORMAP_DATA_DIR", data.path())
            .args(args);
        stdout_of(&mut cmd)
    };

    let text = run(&["card", "proj/src/orders/service.ts"]);
    assert!(text.contains("placeOrder"), "{text}");
    let json = run(&["card", "proj/src/orders/service.ts", "--json"]);
    let card: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(card["path"], "src/orders/service.ts");
    let stores = fs::read_dir(data.path().join("learn"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("facts.sqlite").exists())
        .count();
    assert_eq!(stores, 1, "only proj was indexed, not the parent");
}

#[test]
fn card_from_the_home_folder_finds_a_project_under_it() {
    let home = tempfile::tempdir().unwrap();
    let proj = home.path().join("Projects").join("shop");
    fs::create_dir_all(&proj).unwrap();
    copy_tree(&ts_basic(), &proj);
    git_init(&proj);
    let data = tempfile::tempdir().unwrap();
    for target in [
        proj.join("src/db.ts"),
        PathBuf::from("Projects/shop/src/db.ts"),
    ] {
        Command::cargo_bin("harbormap")
            .unwrap()
            .current_dir(home.path())
            .env("HARBORMAP_DATA_DIR", data.path())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .arg("card")
            .arg(&target)
            .assert()
            .success()
            .stdout(contains("src/db.ts"));
    }
}

#[test]
fn index_refuses_the_home_folder() {
    let s = setup();
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("harbormap")
        .unwrap()
        .current_dir(s.project.path())
        .env("HARBORMAP_DATA_DIR", s.data.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .arg("index")
        .arg(home.path())
        .assert()
        .code(1)
        .stderr(contains("is not a project folder"));
    assert!(s.stores().is_empty());
}

#[test]
fn status_still_works() {
    let fixture = format!(
        "{}/../hm-project/tests/fixtures/shop.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = stdout_of(
        Command::cargo_bin("harbormap")
            .unwrap()
            .env("HARBORMAP_FIXTURE", fixture)
            .args(["status", "--json"]),
    );
    serde_json::from_str::<serde_json::Value>(&text).unwrap();
}
