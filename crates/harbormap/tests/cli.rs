use assert_cmd::Command;

#[test]
fn prints_its_version() {
    Command::cargo_bin("harbormap")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("harbormap {}\n", env!("CARGO_PKG_VERSION")));
}
