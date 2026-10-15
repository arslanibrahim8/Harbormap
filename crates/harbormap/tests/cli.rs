//! The CLI against the recorded "shop" machine. Debug builds read the scenario named
//! by `HARBORMAP_FIXTURE` instead of probing this computer.

use assert_cmd::Command;

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

const STATUS: &str = r#"shop  ~/code/shop · main
  main  ~/code/shop
    Billing          ASP.NET Core  :5280   running · pid 951   http://localhost:5280
    db               PostgreSQL    :55432  running · healthy · container shop-db-1
    shop-api         FastAPI       :8000   running · pid 900   http://localhost:8000
    shop-api-celery  Celery        –       running · pid 970
    web              Vite          :5173   running · pid 802   http://localhost:5173
  feature/login  ~/code/shop-feature
    web              Vite          :5174   running · pid 1300   http://localhost:5174

Also on this computer: 2 services elsewhere (read-only) and 6 system or unknown ports.
Run `harbormap status --machine` to list them.

Docker: Docker Desktop 29.2.1
Note: 1 listening port belongs to a program Harbormap can't see without admin rights.
"#;

const STATUS_MACHINE: &str = r#"shop  ~/code/shop · main
  main  ~/code/shop
    Billing          ASP.NET Core  :5280   running · pid 951   http://localhost:5280
    db               PostgreSQL    :55432  running · healthy · container shop-db-1
    shop-api         FastAPI       :8000   running · pid 900   http://localhost:8000
    shop-api-celery  Celery        –       running · pid 970
    web              Vite          :5173   running · pid 802   http://localhost:5173
  feature/login  ~/code/shop-feature
    web              Vite          :5174   running · pid 1300   http://localhost:5174

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
