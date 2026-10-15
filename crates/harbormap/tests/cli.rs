//! The CLI against the recorded "shop" machine. Debug builds read the scenario named
//! by `HARBORMAP_FIXTURE` instead of probing this computer.

use assert_cmd::Command;
use predicates::str::contains;

fn fixture() -> String {
    format!(
        "{}/../hm-project/tests/fixtures/shop.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn harbormap(args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("harbormap")
        .unwrap()
        .env("HARBORMAP_FIXTURE", fixture())
        .args(args)
        .assert()
}

fn stdout(args: &[&str]) -> String {
    let output = harbormap(args).success().get_output().stdout.clone();
    String::from_utf8(output).unwrap()
}

/// Runs harbormap on a copy of the shop scenario changed by `edit`.
fn run_patched(edit: impl FnOnce(&mut serde_json::Value), args: &[&str]) -> std::process::Output {
    let mut scenario: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
    edit(&mut scenario);
    // Tests run in parallel: every copy gets its own file.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("hm-scenario-{}-{number}.json", std::process::id()));
    std::fs::write(&path, scenario.to_string()).unwrap();
    let output = Command::cargo_bin("harbormap")
        .unwrap()
        .env("HARBORMAP_FIXTURE", &path)
        .args(args)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    output
}

/// Compares line by line first, so a failure points at the first differing line.
fn same_text(actual: &str, expected: &str) {
    for (number, (a, e)) in actual.lines().zip(expected.lines()).enumerate() {
        assert_eq!(a, e, "line {}", number + 1);
    }
    assert_eq!(actual, expected);
}

#[test]
fn prints_its_version() {
    Command::cargo_bin("harbormap")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("harbormap {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn status_shows_the_project_first_and_summarises_the_rest() {
    same_text(&stdout(&["status"]), STATUS);

    // When only a linked worktree runs something, its heading still says which one.
    let output = run_patched(
        |scenario| {
            let inventory = &mut scenario["inventory"];
            let main_only = [5173, 8000, 5280, 55432];
            inventory["listeners"]
                .as_array_mut()
                .unwrap()
                .retain(|listener| !main_only.contains(&listener["port"].as_u64().unwrap()));
            inventory["processes"]
                .as_array_mut()
                .unwrap()
                .retain(|process| process["pid"] != 970);
            inventory["containers"]
                .as_array_mut()
                .unwrap()
                .retain(|container| container["name"] != "shop-db-1");
        },
        &["status"],
    );
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.starts_with("shop  ~/code/shop · main\n  feature/login  ~/code/shop-feature\n    web"),
        "{text}"
    );
}

#[test]
fn status_machine_lists_everything() {
    same_text(&stdout(&["status", "--machine"]), STATUS_MACHINE);
}

#[test]
fn no_subcommand_means_status() {
    assert_eq!(stdout(&[]), stdout(&["status"]));
}

#[test]
fn status_json_is_the_snapshot() {
    let snapshot: hm_core::model::Snapshot =
        serde_json::from_str(&stdout(&["status", "--json"])).unwrap();
    assert_eq!(snapshot.schema, 1);
    assert_eq!(snapshot.services.len(), 8);
    assert!(
        snapshot
            .listeners
            .iter()
            .all(|listener| !listener.label.is_empty())
    );
}

#[test]
fn why_a_port_explains_it_for_beginners() {
    same_text(
        &stdout(&["why", "5173", "--level", "beginner"]),
        WHY_BEGINNER,
    );
}

#[test]
fn why_accepts_names_and_project_names() {
    let by_port = stdout(&["why", "55432"]);
    assert_eq!(stdout(&["why", "db"]), by_port);
    assert_eq!(stdout(&["why", "shop/db"]), by_port);
    assert!(by_port.starts_with(":55432 → db (PostgreSQL) · Docker\n"));
}

#[test]
fn why_prefers_the_worktree_it_was_started_in() {
    assert!(stdout(&["why", "web"]).starts_with(":5173 → web (Vite)\n"));

    // A linked worktree inside the main one: the deepest worktree is where it started.
    let nested = "/Users/me/code/shop/.worktrees/feature";
    let output = run_patched(
        |scenario| {
            let text = scenario
                .to_string()
                .replace("/Users/me/code/shop-feature", nested);
            *scenario = serde_json::from_str(&text).unwrap();
            scenario["launch_dir"] = nested.into();
        },
        &["why", "web"],
    );
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with(":5174 → web (Vite)\n")
    );
}

#[test]
fn why_json_has_title_and_lines() {
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&["why", ":5000", "--json"])).unwrap();
    assert_eq!(json["target"], ":5000");
    assert_eq!(json["cards"][0]["title"], ":5000 → AirPlay Receiver");
    assert!(json["cards"][0]["lines"].as_array().unwrap().len() >= 2);
}

#[test]
fn why_a_free_port_says_so() {
    assert_eq!(stdout(&["why", "4321"]), ":4321 → free\n  :4321 is free.\n");
}

#[test]
fn why_an_unknown_name_fails_with_a_hint() {
    harbormap(&["why", "nope"])
        .code(1)
        .stderr(contains("nothing called \"nope\" is running"));

    // Two services with one name, neither where Harbormap started: each is named by port.
    let output = run_patched(
        |scenario| scenario["launch_dir"] = "/Users/me".into(),
        &["why", "web"],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let hint = String::from_utf8(output.stderr).unwrap();
    assert!(
        hint.contains("harbormap why :5173   shop/web  (~/code/shop)"),
        "{hint}"
    );
    assert!(
        hint.contains("harbormap why :5174   shop/web  (~/code/shop-feature)"),
        "{hint}"
    );
}

#[test]
fn why_prints_no_control_characters() {
    // Names and arguments come from other programs; they must not drive the terminal.
    let output = run_patched(
        |scenario| {
            for process in scenario["inventory"]["processes"].as_array_mut().unwrap() {
                if process["pid"] == 802 {
                    process["name"] = "node\u{1b}[31m".into();
                    process["argv"][1] = "\u{1b}]0;pwned\u{7}".into();
                }
            }
        },
        &["why", "5173", "--level", "expert"],
    );
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text:?}"
    );
    assert!(text.contains("\u{FFFD}]0;pwned\u{FFFD}"), "{text:?}");
}

const STATUS: &str = r#"shop  ~/code/shop · main
  main  ~/code/shop
    Billing          ASP.NET Core       :5280   running · pid 951   http://localhost:5280
    db               PostgreSQL         :55432  running · healthy · container shop-db-1
    shop-api         FastAPI, probably  :8000   running · pid 900   http://localhost:8000
    shop-api-celery  Celery             –       running · pid 970
    web              Vite               :5173   running · pid 802   http://localhost:5173
  feature/login  ~/code/shop-feature
    web              Vite               :5174   running · pid 1300   http://localhost:5174

Also on this computer: 2 services elsewhere (read-only) and 6 system or unknown ports.
Run `harbormap status --machine` to list them.

Docker: Docker Desktop 29.2.1
Note: 1 listening port belongs to a program Harbormap can't see without admin rights.
"#;

const STATUS_MACHINE: &str = r#"shop  ~/code/shop · main
  main  ~/code/shop
    Billing          ASP.NET Core       :5280   running · pid 951   http://localhost:5280
    db               PostgreSQL         :55432  running · healthy · container shop-db-1
    shop-api         FastAPI, probably  :8000   running · pid 900   http://localhost:8000
    shop-api-celery  Celery             –       running · pid 970
    web              Vite               :5173   running · pid 802   http://localhost:5173
  feature/login  ~/code/shop-feature
    web              Vite               :5174   running · pid 1300   http://localhost:5174

Elsewhere on this computer (read-only)
  blog  ~/code/blog
    blog          Next.js  :3000  running · pid 1200   http://localhost:3000
  Containers outside projects
    lonely-redis  Redis    :6380  running · container lonely-redis

System & unknown
  :631         cupsd (pid 412)
  :5000 :7000  AirPlay Receiver
  :6000        unknown owner
  :9820        Google Chrome (pid 76915)
  :41234       Claude Code (pid 3000)

Docker: Docker Desktop 29.2.1
Note: 1 listening port belongs to a program Harbormap can't see without admin rights.
"#;

const WHY_BEGINNER: &str = r#"Port 5173: web
  web is part of the project shop.
  This is the live preview of your website while you work on it. It updates as soon as you save a file.
  Open it in your browser: http://localhost:5173
  Only this computer can open it.
  It has been running for 2 hours.
"#;
