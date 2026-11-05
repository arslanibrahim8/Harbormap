//! `harbormap index` and `harbormap card` against a copy of the ts-basic project.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-index/tests/fixtures")
        .join(name)
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
    setup_with("ts-basic")
}

/// Like [`setup`], with a copy of the fixture project `name`.
fn setup_with(name: &str) -> Setup {
    let project = tempfile::tempdir().unwrap();
    copy_tree(&fixture(name), project.path());
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
    // Ref statuses are printed once, on their own line.
    assert!(
        text.contains("\n  Symbols 12 · imports 9 · calls 8\n"),
        "{text}"
    );
    assert!(
        text.contains("\n  refs: 2 unresolved · 3 external"),
        "{text}"
    );
    assert_eq!(text.matches("unresolved").count(), 1, "{text}");
    assert!(!text.contains("inferred links"), "{text}");
    assert!(text.contains("(16 files)"), "{text}");
    assert!(text.contains("\n  Ignore files: .gitignore\n"), "{text}");
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
    assert_eq!(report["ignore_files"], serde_json::json!([".gitignore"]));
    assert_eq!(report["notes"], serde_json::json!([]));
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
fn card_sees_an_edit_made_after_the_last_index() {
    let s = setup();
    let text = stdout_of(&mut s.harbormap(&["card", "src/db.ts"]));
    assert!(!text.contains("later"), "{text}");
    let db = s.project.path().join("src/db.ts");
    let source = fs::read_to_string(&db).unwrap();
    fs::write(&db, format!("{source}\nexport function later() {{}}\n")).unwrap();
    let text = stdout_of(&mut s.harbormap(&["card", "src/db.ts"]));
    assert!(text.contains("later"), "{text}");
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
    copy_tree(&fixture("ts-basic"), &proj);
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
    copy_tree(&fixture("ts-basic"), &proj);
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

#[cfg(unix)]
#[test]
fn index_counts_an_unreadable_folder_and_goes_on() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup();
    let orders = s.project.path().join("src/orders");
    fs::set_permissions(&orders, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&orders).is_ok();
    let out = s.harbormap(&["index"]).output().unwrap();
    fs::set_permissions(&orders, fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        eprintln!("skipped: permissions do not stop this user (root?)");
        return;
    }
    let text = String::from_utf8(out.stdout).unwrap();
    let errors = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{errors}");
    assert!(text.contains("1 unreadable file or folder"), "{text}");
}

/// `harbormap index --watch --json` running on a project, its lines on a channel.
struct Watching {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<serde_json::Value>,
}

impl Watching {
    fn start(s: &Setup) -> Watching {
        use std::io::BufRead;
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("harbormap"))
            .current_dir(s.project.path())
            .env("HARBORMAP_DATA_DIR", s.data.path())
            .args(["index", "--watch", "--json"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                let value = serde_json::from_str(&line).expect("one JSON object per line");
                if send.send(value).is_err() {
                    return;
                }
            }
        });
        Watching { child, lines }
    }

    /// The first line that `accept` takes within `within`, skipping the others.
    fn wait_for(
        &self,
        within: std::time::Duration,
        accept: impl Fn(&serde_json::Value) -> bool,
    ) -> Option<serde_json::Value> {
        let end = std::time::Instant::now() + within;
        while let Some(left) = end.checked_duration_since(std::time::Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(line) if accept(&line) => return Some(line),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Whether a batch line names `rel` among its paths.
fn names(line: &serde_json::Value, rel: &str) -> bool {
    line["paths"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|p| p == rel))
}

#[test]
fn watch_follows_a_folder_that_an_ignore_edit_brings_in() {
    let s = setup();
    let wait = std::time::Duration::from_secs(10);
    let w = Watching::start(&s);
    assert!(w.wait_for(wait, |l| l.get("parsed").is_some()).is_some());
    // The watcher is running with the rules of the first pass.
    std::thread::sleep(std::time::Duration::from_secs(1));

    // `ignored/` is hidden by .gitignore; this edit brings it in.
    fs::write(s.project.path().join(".gitignore"), "# nothing\n").unwrap();
    let line = w.wait_for(wait, |l| names(l, ".gitignore"));
    let line = line.expect("no batch for .gitignore");
    assert_eq!(line["routes_added"], serde_json::json!([]));
    assert_eq!(line["routes_removed"], serde_json::json!([]));
    // Let the watcher settle, then save inside the folder that is now on the map.
    std::thread::sleep(std::time::Duration::from_millis(500));
    fs::write(
        s.project.path().join("ignored/secret.ts"),
        "export const x = 1;\nexport function later() {}\n",
    )
    .unwrap();
    let line = w.wait_for(wait, |l| names(l, "ignored/secret.ts"));
    assert!(line.is_some(), "the save in ignored/ was not seen");
    drop(w);

    let store = hm_store::Store::open(&s.stores()[0]).unwrap();
    let later: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM symbol WHERE name = 'later'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(later, 1, "the new symbol is in the store");
}

#[test]
fn index_prints_routes_summary() {
    let s = setup_with("client-hops");
    let text = stdout_of(&mut s.harbormap(&["index"]));
    assert!(text.contains("\n  Routes: 4 API\n"), "{text}");
    // `calls` counts the calls in the code; the links the rules infer (here the
    // `http_calls` from client code to the routes) have their own count.
    let store = hm_store::Store::open(&s.stores()[0]).unwrap();
    let count = |sql: &str| -> i64 { store.conn().query_row(sql, [], |row| row.get(0)).unwrap() };
    let parsed = count("SELECT count(*) FROM edge WHERE kind = 'calls' AND prov = 'parsed'");
    let inferred = count("SELECT count(*) FROM edge WHERE prov = 'inferred'");
    assert!(inferred > 0);
    assert!(
        text.contains(&format!(" · calls {parsed} · inferred links {inferred}\n")),
        "{text}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout_of(&mut s.harbormap(&["index", "--json"]))).unwrap();
    // A kind without edges is absent.
    assert_eq!(json["edges"]["calls"].as_i64().unwrap_or(0), parsed);
    assert_eq!(
        json["inferred_edges"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_i64().unwrap())
            .sum::<i64>(),
        inferred
    );
    assert!(
        text.contains("\n  API · GET /api/orders → app/api/orders/route.ts#GET\n"),
        "{text}"
    );
    assert!(
        text.contains("\n  Manifests: 1 (4 dependencies)\n"),
        "{text}"
    );
    assert!(text.contains("\n  Calls to the server: "), "{text}");
    assert!(text.contains(" matched to a route\n"), "{text}");
    assert!(!text.contains("more (harbormap index --json"), "{text}");
    assert!(
        text.lines().last().unwrap().trim().starts_with("Took "),
        "{text}"
    );

    // A project without routes says so and keeps its other lines.
    let plain = stdout_of(&mut setup().harbormap(&["index"]));
    assert!(plain.contains("\n  No routes found\n"), "{plain}");
    assert!(!plain.contains("Entries:"), "{plain}");
    assert!(plain.contains("\n  refs: "), "{plain}");
}

#[test]
fn index_names_entries_and_cuts_a_long_list() {
    let s = setup_with("client-hops");
    fs::write(
        s.project.path().join("package.json"),
        r#"{"name":"shop","bin":{"shop":"./cli.js"},"scripts":{"start":"node server.js"},
           "dependencies":{"next":"15.2.0"}}"#,
    )
    .unwrap();
    for n in 0..13 {
        let dir = s.project.path().join(format!("app/p{n:02}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("page.tsx"),
            "export default function P() { return null }\n",
        )
        .unwrap();
    }
    let text = stdout_of(&mut s.harbormap(&["index"]));
    assert!(text.contains("\n  Routes: 13 pages · 4 API\n"), "{text}");
    assert!(
        text.contains("\n  Entries: 1 start script · 1 command\n"),
        "{text}"
    );
    // 4 + 13 routes and 2 entries; 15 are listed.
    assert!(
        text.contains("\n  +4 more (harbormap index --json lists all)\n"),
        "{text}"
    );
    let listed = text
        .lines()
        .filter(|l| {
            ["  API · ", "  Page · ", "  Command · ", "  Start script · "]
                .iter()
                .any(|kind| l.starts_with(kind))
        })
        .count();
    assert_eq!(listed, 15, "{text}");
}

#[test]
fn index_json_has_routes() {
    let s = setup_with("client-hops");
    let text = stdout_of(&mut s.harbormap(&["index", "--json"]));
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    let routes = report["routes"].as_array().expect("routes is an array");
    assert_eq!(routes.len(), 4);
    assert!(
        routes.iter().all(|r| r["entry_kind"] == "http"),
        "{routes:?}"
    );
    assert_eq!(report["route_counts"]["http"], 4);
}

#[test]
fn card_prints_role() {
    let s = setup_with("client-hops");
    let symbol = stdout_of(&mut s.harbormap(&["card", "app/api/orders/route.ts#POST"]));
    assert!(
        symbol.contains("\n  Role: API · POST /api/orders\n"),
        "{symbol}"
    );
    let file = stdout_of(&mut s.harbormap(&["card", "app/api/orders/route.ts"]));
    assert!(
        file.contains("\nRoutes (2)\n  API · GET /api/orders\n  API · POST /api/orders\n"),
        "{file}"
    );
    let other = stdout_of(&mut s.harbormap(&["card", "lib/api.ts"]));
    assert!(!other.contains("Routes ("), "{other}");
}
