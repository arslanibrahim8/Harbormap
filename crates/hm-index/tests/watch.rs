mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::copy_fixture;
use hm_index::walk::walk;
use hm_index::watch::Watcher;

/// Batches until one holds `rel` or `within` passes. FSEvents may first report the
/// writes that built the copy, so batches without `rel` are not a failure.
fn wait_for(w: &Watcher, rel: &str, within: Duration) -> bool {
    let end = Instant::now() + within;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Some(batch) = w.recv_timeout(left)
            && batch.paths.contains(rel)
        {
            return true;
        }
    }
    false
}

#[test]
fn a_write_arrives_in_a_batch_and_skipped_folders_stay_silent() {
    let dir = copy_fixture("ts-basic");
    let walked = walk(dir.path()).unwrap();
    let w = Watcher::start(dir.path(), walked.rules.clone(), &walked.dirs).unwrap();

    fs::write(dir.path().join("src/db.ts"), "export const db = 2;\n").unwrap();
    assert!(wait_for(&w, "src/db.ts", Duration::from_secs(3)));

    // Let late duplicates of the first write settle, then write into a skip folder.
    while w.recv_timeout(Duration::from_millis(300)).is_some() {}
    fs::create_dir_all(dir.path().join("node_modules/left-pad")).unwrap();
    fs::write(dir.path().join("node_modules/left-pad/x.js"), "x\n").unwrap();
    let end = Instant::now() + Duration::from_millis(600);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Some(batch) = w.recv_timeout(left) {
            assert!(
                batch.paths.iter().all(|p| !p.contains("node_modules")),
                "{:?}",
                batch.paths
            );
        }
    }
    drop(w); // before the TempDir
}

#[test]
fn a_new_folder_is_watched_and_its_files_are_reported() {
    let dir = copy_fixture("ts-basic");
    let walked = walk(dir.path()).unwrap();
    let w = Watcher::start(dir.path(), walked.rules.clone(), &walked.dirs).unwrap();

    fs::create_dir_all(dir.path().join("src/fresh")).unwrap();
    fs::write(dir.path().join("src/fresh/a.ts"), "export const a = 1;\n").unwrap();
    assert!(wait_for(&w, "src/fresh/a.ts", Duration::from_secs(3)));

    // A file written later, once the folder has its own watch.
    while w.recv_timeout(Duration::from_millis(300)).is_some() {}
    fs::write(dir.path().join("src/fresh/b.ts"), "export const b = 1;\n").unwrap();
    assert!(wait_for(&w, "src/fresh/b.ts", Duration::from_secs(3)));
    drop(w);
}

#[test]
fn a_changed_git_exclude_asks_for_a_rescan() {
    let dir = copy_fixture("ts-basic");
    fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    let walked = walk(dir.path()).unwrap();
    let w = Watcher::start(dir.path(), walked.rules.clone(), &walked.dirs).unwrap();

    fs::write(dir.path().join(".git/info/exclude"), "src/legacy.js\n").unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    let mut rescan = false;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if w.recv_timeout(left).is_some_and(|batch| batch.rescan) {
            rescan = true;
            break;
        }
    }
    assert!(rescan, "no rescan batch for .git/info/exclude");
    drop(w);
}
