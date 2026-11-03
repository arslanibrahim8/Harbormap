//! The committed panel bundle (`ui/learn/dist`) must be the build of the sources next
//! to it. These tests need no Node: they recompute the source hash exactly as
//! `ui/learn/build.mjs` does and compare it with `dist/SOURCE_HASH`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

/// The panel folder, `ui/learn` at the repository root.
fn ui() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/learn")
}

/// Every file under `dir` as a `/`-separated path relative to `ui`, in no order. A
/// stray `.DS_Store` is not source (`build.mjs` skips it too).
fn walk(ui: &Path, dir: &str, out: &mut Vec<String>) {
    for entry in fs::read_dir(ui.join(dir)).expect("ui/learn/src is readable") {
        let entry = entry.expect("a directory entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        if entry.file_type().expect("a file type").is_dir() {
            walk(ui, &path, out);
        } else if name != ".DS_Store" {
            out.push(path);
        }
    }
}

/// FNV-1a, 64 bits, over `path \0 content \0` for every source file in path order,
/// with every carriage return dropped from the content (so a CRLF checkout agrees).
fn source_hash(ui: &Path) -> String {
    let mut files = Vec::new();
    walk(ui, "src", &mut files);
    for name in [
        "build.mjs",
        "package.json",
        "package-lock.json",
        "tsconfig.json",
    ] {
        files.push(name.to_string());
    }
    files.sort();

    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |byte: u8| h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    for path in &files {
        path.bytes().for_each(&mut feed);
        feed(0);
        let content = fs::read(ui.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
        content
            .into_iter()
            .filter(|&b| b != b'\r')
            .for_each(&mut feed);
        feed(0);
    }
    format!("{h:016x}")
}

#[test]
fn the_committed_bundle_matches_its_source() {
    let ui = ui();
    let committed = fs::read_to_string(ui.join("dist/SOURCE_HASH"))
        .expect("ui/learn/dist/SOURCE_HASH is committed");
    assert_eq!(
        committed.trim(),
        source_hash(&ui),
        "ui/learn changed; run `npm --prefix ui/learn run build` and commit ui/learn/dist"
    );
}

#[test]
fn the_bundle_stays_under_150_kb_gzipped() {
    let js = fs::read(ui().join("dist/app.js")).expect("ui/learn/dist/app.js is committed");
    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    gz.write_all(&js).expect("gzip into memory");
    let size = gz.finish().expect("gzip finishes").len();
    assert!(
        size <= 150 * 1024,
        "app.js is {size} bytes gzipped, the limit is 150 KB"
    );
}
