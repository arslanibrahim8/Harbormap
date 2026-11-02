mod common;

use std::fs;

use common::{copy_fixture, fixture};
use hm_index::lang::Lang;
use hm_index::walk::{FileClass, MAX_PARSE_BYTES, Skip, classify, walk};

#[test]
fn the_fixture_walks_with_classes_and_skips() {
    let r = walk(&fixture("ts-basic")).unwrap();
    let class = |p: &str| r.files.iter().find(|f| f.rel == p).map(|f| f.class);
    assert_eq!(class("src/orders/service.ts"), Some(FileClass::Source));
    assert_eq!(class("src/orders/service.test.ts"), Some(FileClass::Test));
    assert_eq!(class("src/api.gen.ts"), Some(FileClass::Generated));
    assert_eq!(class("docs/guide.md"), Some(FileClass::Doc));
    assert_eq!(class("Dockerfile"), Some(FileClass::Infra));
    assert_eq!(class("src/scripts/tool.py"), Some(FileClass::Source));
    assert!(class("ignored/secret.ts").is_none()); // .gitignore
    assert!(class("dist/bundle.js").is_none());
    assert_eq!(r.skipped_dirs.get("node_modules"), Some(&1));
    assert_eq!(r.skipped_dirs.get("dist"), Some(&1));
    let rels: Vec<_> = r.files.iter().map(|f| f.rel.clone()).collect();
    let mut sorted = rels.clone();
    sorted.sort();
    assert_eq!(rels, sorted);

    let tool = r
        .files
        .iter()
        .find(|f| f.rel == "src/scripts/tool.py")
        .unwrap();
    assert_eq!(tool.lang, Some(Lang::Python));
    assert!(tool.size > 0 && tool.mtime_ns > 0 && tool.skip.is_none());
    assert!(r.dirs.contains(&String::new()) && r.dirs.contains(&"src/orders".to_string()));
    assert!(
        !r.dirs
            .iter()
            .any(|d| d.starts_with("node_modules") || d.starts_with("ignored"))
    );
}

#[test]
fn minified_and_binary_files_are_counted_not_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join("big.js"), "a=1;".repeat(512 * 1024)).unwrap();
    assert!(fs::metadata(p.join("big.js")).unwrap().len() > MAX_PARSE_BYTES);
    fs::write(p.join("app.min.js"), "a".repeat(600_000)).unwrap();
    fs::write(p.join("logo.js"), [0x89, b'P', b'N', b'G', 0, 0]).unwrap();
    fs::write(p.join("ok.js"), "const a = 1;\n").unwrap();

    let r = walk(p).unwrap();
    let find = |n: &str| r.files.iter().find(|f| f.rel == n).unwrap();
    assert_eq!(find("big.js").skip, Some(Skip::TooLarge));
    assert_eq!(find("app.min.js").skip, Some(Skip::TooLarge));
    assert_eq!(find("logo.js").skip, Some(Skip::Binary));
    for name in ["big.js", "app.min.js", "logo.js"] {
        assert_eq!(find(name).class, FileClass::Generated, "{name}");
        assert_eq!(find(name).lang, Some(Lang::JavaScript), "{name}");
    }
    assert_eq!(find("ok.js").skip, None);
    assert_eq!(find("ok.js").class, FileClass::Source);
}

#[test]
fn a_short_file_with_long_lines_is_not_minified() {
    // A generated barrel: four `export { Name0, Name1, ... } from` lines of about
    // 800 bytes are no minified bundle.
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..100).map(|i| format!("Name{i}")).collect();
    let line = format!("export {{ {} }} from './x';\n", names.join(", "));
    assert!(line.len() > 500);
    fs::write(dir.path().join("index.ts"), line.repeat(4)).unwrap();

    let r = walk(dir.path()).unwrap();
    let barrel = r.files.iter().find(|f| f.rel == "index.ts").unwrap();
    assert_eq!(barrel.skip, None);
    assert_eq!(barrel.class, FileClass::Source);
}

#[test]
fn ignore_rules_apply_in_a_copy_without_git() {
    let dir = copy_fixture("ts-basic"); // a tempdir, not a git repository
    let r = walk(dir.path()).unwrap();
    assert!(!r.files.iter().any(|f| f.rel == "ignored/secret.ts"));
    assert_eq!(r.ignore_files, vec![".gitignore"]);
    assert!(!r.rules.admits("ignored/secret.ts") && !r.rules.admits("node_modules/x.js"));
    assert!(r.rules.admits("src/new.ts"));
}

#[test]
fn harbormapignore_and_nested_ignore_files_apply() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join("a/b")).unwrap();
    fs::create_dir_all(p.join("keep")).unwrap();
    fs::write(p.join(".harbormapignore"), "secret.ts\n").unwrap();
    fs::write(p.join("a/.gitignore"), "*.log\n!b/keep.log\n").unwrap();
    for f in [
        "secret.ts",
        "a/secret.ts",
        "a/x.log",
        "a/b/keep.log",
        "a/b/c.ts",
        "keep/d.ts",
    ] {
        fs::write(p.join(f), "x\n").unwrap();
    }
    let r = walk(p).unwrap();
    let rels: Vec<&str> = r.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(
        rels,
        [
            ".harbormapignore",
            "a/.gitignore",
            "a/b/c.ts",
            "a/b/keep.log",
            "keep/d.ts"
        ]
    );
    assert_eq!(r.ignore_files, vec![".harbormapignore", "a/.gitignore"]);
    // The rules answer the same as the walk, without walking.
    for f in &r.files {
        assert!(r.rules.admits(&f.rel), "{}", f.rel);
    }
    for hidden in [
        "secret.ts",
        "a/secret.ts",
        "a/x.log",
        "a/b/x.log",
        "a/b/node_modules/y.js",
        ".git/config",
    ] {
        assert!(!r.rules.admits(hidden), "{hidden}");
    }
    assert!(r.rules.admits("a/other.ts") && r.rules.admits("secret.tsx"));
}

#[test]
fn paths_are_slash_separated_and_symlinks_stay_inside() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join("a/b")).unwrap();
    fs::write(p.join("a/b/c.ts"), "export const c = 1;\n").unwrap();

    let r = walk(p).unwrap();
    let rels: Vec<&str> = r.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(rels, ["a/b/c.ts"]);
    assert_eq!(r.dirs, ["", "a", "a/b"]);

    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("leak.ts"), "export {};\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), p.join("out")).unwrap();
        std::os::unix::fs::symlink(p.join("a"), p.join("inner")).unwrap();
        let r = walk(p).unwrap();
        let rels: Vec<&str> = r.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, ["a/b/c.ts"]);
        assert_eq!(r.dirs, ["", "a", "a/b"]);
    }
}

#[test]
fn classify_follows_the_rules() {
    let c = |rel: &str| classify(rel, b"");
    for t in [
        "tests/helpers.ts",
        "src/__tests__/a.ts",
        "a.spec.tsx",
        "pkg/x_test.go",
        "pkg/test_x.py",
        "src/FooTest.java",
        "src/FooTests.cs",
    ] {
        assert_eq!(c(t), FileClass::Test, "{t}");
    }
    for g in [
        "a.gen.ts",
        "Foo.g.cs",
        "x_pb2.py",
        "x.pb.go",
        "lib/app.min.js",
    ] {
        assert_eq!(c(g), FileClass::Generated, "{g}");
    }
    for d in [
        "README.md",
        "a.mdx",
        "b.rst",
        "c.txt",
        "d.adoc",
        "docs/diagram.png",
    ] {
        assert_eq!(c(d), FileClass::Doc, "{d}");
    }
    for i in [
        "Dockerfile",
        "Dockerfile.dev",
        "docker-compose.yml",
        "compose.yaml",
        ".github/workflows/ci.yml",
        "main.tf",
        "Procfile",
        "fly.toml",
        "vercel.json",
        "render.yaml",
    ] {
        assert_eq!(c(i), FileClass::Infra, "{i}");
    }
    for s in [
        "src/a.ts",
        "docs/example.ts",
        "package.json",
        "src/latest.rs",
    ] {
        assert_eq!(c(s), FileClass::Source, "{s}");
    }
}

#[test]
fn a_generated_marker_in_the_first_five_lines_counts() {
    let near = b"// header\n// @generated by tool\nexport {};\n";
    assert_eq!(classify("src/a.ts", near), FileClass::Generated);
    assert_eq!(
        classify("a.go", b"// Code generated by x. DO NOT EDIT.\n"),
        FileClass::Generated
    );
    assert_eq!(
        classify("a.py", b"# auto-generated\n"),
        FileClass::Generated
    );
    let far = b"1\n2\n3\n4\n5\n// @generated\n";
    assert_eq!(classify("src/a.ts", far), FileClass::Source);
}

#[test]
fn ignored_skip_folders_are_still_counted() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::write(p.join(".gitignore"), "node_modules/\ndist/\n").unwrap();
    fs::create_dir_all(p.join("node_modules/x")).unwrap();
    fs::create_dir_all(p.join("dist")).unwrap();
    fs::create_dir_all(p.join("pkg/node_modules")).unwrap();
    fs::create_dir_all(p.join(".git")).unwrap();
    fs::write(p.join("node_modules/x/i.js"), "x\n").unwrap();
    fs::write(p.join("dist/b.js"), "x\n").unwrap();
    fs::write(p.join("pkg/node_modules/j.js"), "x\n").unwrap();
    fs::write(p.join("a.ts"), "x\n").unwrap();

    let r = walk(p).unwrap();
    assert_eq!(r.skipped_dirs.get("node_modules"), Some(&2));
    assert_eq!(r.skipped_dirs.get("dist"), Some(&1));
    assert_eq!(r.skipped_dirs.len(), 2); // `.git` is not counted
    let rels: Vec<&str> = r.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(rels, [".gitignore", "a.ts"]);
}

#[test]
fn admits_dir_answers_for_folders() {
    let dir = copy_fixture("ts-basic");
    let r = walk(dir.path()).unwrap();
    for hidden in [
        "dist",
        "node_modules",
        "ignored",
        "src/node_modules",
        ".git",
    ] {
        assert!(!r.rules.admits_dir(hidden), "{hidden}");
    }
    for shown in ["src/orders", "src", "docs", ""] {
        assert!(r.rules.admits_dir(shown), "{shown}");
    }
    // `admits` still takes the last component for a file.
    assert!(r.rules.admits("dist") && r.rules.admits("ignored"));
    assert!(!r.rules.admits("ignored/secret.ts") && !r.rules.admits("dist/bundle.js"));
    assert!(r.rules.admits("src/new.ts"));
}

#[cfg(unix)]
#[test]
fn unreadable_folders_and_files_are_counted_and_the_walk_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join("locked/inner")).unwrap();
    fs::write(p.join("locked/a.ts"), "export const a = 1;\n").unwrap();
    fs::write(p.join("secret.test.ts"), "export const s = 1;\n").unwrap();
    fs::write(p.join("ok.ts"), "export const o = 1;\n").unwrap();
    let Some(_folder) = common::lock(&p.join("locked")) else {
        return;
    };
    let Some(_file) = common::lock(&p.join("secret.test.ts")) else {
        return;
    };

    let r = walk(p).unwrap();
    assert_eq!(r.unreadable, ["locked"]);
    assert_eq!(r.dirs, [""], "an unreadable folder is not admitted");
    let rels: Vec<&str> = r.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(rels, ["ok.ts", "secret.test.ts"]);
    let secret = &r.files[1];
    assert_eq!(secret.skip, Some(Skip::Unreadable));
    // Nothing was read, so the path alone gives the class.
    assert_eq!(secret.class, FileClass::Test);
    assert_eq!(r.files[0].skip, None);
}
