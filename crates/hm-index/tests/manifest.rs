mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{copy_fixture, fixture};
use hm_index::manifest::{
    LOCKFILES, ManifestCache, Manifests, NpmManifest, parse_lockfile, parse_package_json,
};
use hm_index::walk::walk;

/// The walked relative paths of fixture folder `npm-locks/<project>`.
fn known_of(root: &std::path::Path) -> BTreeSet<String> {
    walk(root)
        .expect("walk the fixture")
        .files
        .into_iter()
        .map(|f| f.rel)
        .collect()
}

fn load_project(project: &str) -> Manifests {
    let root = fixture(&format!("npm-locks/{project}"));
    let known = known_of(&root);
    Manifests::load(&root, &known, &mut ManifestCache::default())
}

fn dep<'a>(m: &'a NpmManifest, name: &str) -> &'a hm_index::manifest::Dependency {
    m.deps
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("{} declares no {name}", m.path))
}

// https://docs.npmjs.com/cli/v11/configuring-npm/package-json (`bin`, `main`, `scripts`)
#[test]
fn package_json_fields_and_lines_are_read() {
    let m = load_project("pnpm");
    let web = m.npm.iter().find(|n| n.path == "apps/web/package.json");
    let web = web.expect("apps/web/package.json is loaded");
    assert_eq!(web.root, "apps/web");
    assert_eq!(web.name.as_deref(), Some("@shop/web"));
    assert_eq!(
        web.bin,
        vec![("shop".to_string(), "./bin/shop.js".to_string(), 4)]
    );
    assert_eq!(web.main, Some(("index.js".to_string(), 6)));
    assert_eq!(
        web.scripts,
        vec![
            ("dev".to_string(), "next dev".to_string(), 8),
            ("start".to_string(), "next start".to_string(), 9)
        ]
    );
    let next = dep(web, "next");
    assert_eq!(next.section, "dependencies");
    assert_eq!(next.declared, "^15.0.0");
    assert_eq!(next.line, 12);
    assert!(!next.dev);

    let root = m.npm.iter().find(|n| n.path == "package.json").unwrap();
    assert_eq!(root.root, "");
    let ts = dep(root, "typescript");
    assert_eq!(ts.section, "devDependencies");
    assert!(ts.dev);
    assert_eq!(ts.line, 5);
    assert!(m.notes.is_empty(), "{:?}", m.notes);
}

#[test]
fn a_string_bin_is_named_after_the_package_without_its_scope() {
    let m = parse_package_json(
        "package.json",
        "{\n  \"name\": \"@shop/cli\",\n  \"bin\": \"./cli.js\"\n}\n",
    );
    assert_eq!(m.bin, vec![("cli".to_string(), "./cli.js".to_string(), 3)]);
    let plain = parse_package_json("package.json", r#"{"name": "shop-cli", "bin": "x.js"}"#);
    assert_eq!(plain.bin[0].0, "shop-cli");
    // A string bin of a package without a name has nothing to be named after.
    let nameless = parse_package_json("package.json", r#"{"bin": "x.js"}"#);
    assert!(nameless.bin.is_empty() && nameless.notes.len() == 1);
}

#[test]
fn lists_are_sorted_and_every_section_is_read() {
    let m = parse_package_json(
        "apps/web/package.json",
        r#"{
  "devDependencies": {"zod": "^3", "jest": "^29"},
  "dependencies": {"react": "^19", "next": "^15"},
  "peerDependencies": {"react-dom": "^19"},
  "optionalDependencies": {"fsevents": "^2"},
  "scripts": {"start": "a", "build": "b"},
  "bin": {"zeta": "z.js", "alpha": "a.js"}
}"#,
    );
    let got: Vec<(&str, &str, bool)> = m
        .deps
        .iter()
        .map(|d| (d.section.as_str(), d.name.as_str(), d.dev))
        .collect();
    assert_eq!(
        got,
        vec![
            ("dependencies", "next", false),
            ("dependencies", "react", false),
            ("devDependencies", "jest", true),
            ("devDependencies", "zod", true),
            ("optionalDependencies", "fsevents", false),
            ("peerDependencies", "react-dom", false),
        ]
    );
    assert_eq!(m.scripts[0].0, "build");
    assert_eq!(m.bin[0].0, "alpha");
    assert!(m.deps.iter().all(|d| d.resolved.is_none()));
    assert!(m.lockfile.is_none());
}

// https://docs.npmjs.com/cli/v11/configuring-npm/package-lock-json
#[test]
fn pnpm_importers_resolve_per_manifest_root() {
    let m = load_project("pnpm");
    let web = m.npm.iter().find(|n| n.root == "apps/web").unwrap();
    let root = m.npm.iter().find(|n| n.root.is_empty()).unwrap();
    assert_eq!(dep(web, "next").resolved.as_deref(), Some("15.2.1"));
    assert_eq!(dep(root, "typescript").resolved.as_deref(), Some("5.6.3"));
    assert_eq!(web.lockfile.as_deref(), Some("pnpm-lock.yaml"));
    assert_eq!(root.lockfile.as_deref(), Some("pnpm-lock.yaml"));
}

#[test]
fn a_pnpm_name_resolves_from_its_own_importer() {
    let text = fs::read_to_string(fixture("npm-locks/pnpm/pnpm-lock.yaml")).unwrap();
    let lock = parse_lockfile("pnpm-lock.yaml", &text).unwrap();
    assert_eq!(
        lock.resolved(".", "typescript", "^5.6.0").as_deref(),
        Some("5.6.3")
    );
    assert_eq!(
        lock.resolved("apps/web", "typescript", "^5.0.0").as_deref(),
        Some("5.0.4")
    );
    assert_eq!(lock.resolved("apps/other", "typescript", "^5"), None);
    assert_eq!(lock.resolved(".", "next", "^15.0.0"), None);
    // `packages:` and `snapshots:` are not importers.
    assert_eq!(lock.resolved(".", "next@15.2.1", ""), None);
}

#[test]
fn npm_v1_v3_and_yarn_lockfiles_resolve() {
    for (project, lockfile) in [
        ("npm", "package-lock.json"),
        ("npm-v1", "package-lock.json"),
        ("yarn-v1", "yarn.lock"),
        ("yarn-berry", "yarn.lock"),
    ] {
        let m = load_project(project);
        assert!(m.notes.is_empty(), "{project}: {:?}", m.notes);
        let root = &m.npm[0];
        assert_eq!(root.lockfile.as_deref(), Some(lockfile), "{project}");
        assert_eq!(
            dep(root, "express").resolved.as_deref(),
            Some("4.21.2"),
            "{project}"
        );
    }
}

#[test]
fn yarn_blocks_match_any_key_of_a_joined_header() {
    let text = fs::read_to_string(fixture("npm-locks/yarn-v1/yarn.lock")).unwrap();
    let lock = parse_lockfile("yarn.lock", &text).unwrap();
    assert_eq!(
        lock.resolved(".", "express", "^4.20.0").as_deref(),
        Some("4.21.2")
    );
    assert_eq!(
        lock.resolved(".", "@shop/ui", "^1.0.0").as_deref(),
        Some("1.0.3")
    );
    assert_eq!(lock.resolved(".", "express", "^3.0.0"), None);
    // Berry writes the `npm:` protocol into the key.
    let text = fs::read_to_string(fixture("npm-locks/yarn-berry/yarn.lock")).unwrap();
    let berry = parse_lockfile("yarn.lock", &text).unwrap();
    assert_eq!(
        berry.resolved(".", "express", "^4.21.0").as_deref(),
        Some("4.21.2")
    );
    assert_eq!(berry.resolved(".", "express", "^4.0.0"), None);
}

#[test]
fn npm_workspace_packages_resolve_from_the_nearest_node_modules() {
    let lock = r#"{
  "lockfileVersion": 3,
  "packages": {
    "": {"name": "shop"},
    "node_modules/react": {"version": "19.0.0"},
    "node_modules/@shop/ui": {"resolved": "packages/ui", "link": true},
    "apps/web/node_modules/react": {"version": "18.3.1"},
    "apps/web": {"name": "@shop/web", "version": "0.1.0"}
  }
}"#;
    let lock = parse_lockfile("package-lock.json", lock).unwrap();
    assert_eq!(
        lock.resolved("apps/web", "react", "^18").as_deref(),
        Some("18.3.1")
    );
    assert_eq!(
        lock.resolved(".", "react", "^19").as_deref(),
        Some("19.0.0")
    );
    // No folder of its own: the hoisted one.
    assert_eq!(
        lock.resolved("apps/admin", "react", "^19").as_deref(),
        Some("19.0.0")
    );
    // A workspace link has no version.
    assert_eq!(lock.resolved(".", "@shop/ui", "*"), None);
}

#[test]
fn a_broken_package_json_is_a_note_not_an_error() {
    let m = load_project("broken");
    let broken = &m.npm[0];
    // Comments and trailing commas parse.
    assert_eq!(broken.name.as_deref(), Some("shop-broken"));
    assert_eq!(dep(broken, "left-pad").declared, "^1.3.0");
    // `"bin": 5` is a note at its line, and no bin.
    assert!(broken.bin.is_empty());
    assert_eq!(
        m.notes,
        vec!["package.json: \"bin\" is not a string or an object (line 4)".to_string()]
    );

    // No closing brace: a note with the line, and an empty manifest.
    let cut = parse_package_json(
        "apps/web/package.json",
        "{\n  \"name\": \"x\",\n  \"scripts\": {\n    \"dev\": \"next dev\",\n",
    );
    assert_eq!(cut.path, "apps/web/package.json");
    assert_eq!(cut.root, "apps/web");
    assert!(cut.name.is_none() && cut.scripts.is_empty() && cut.deps.is_empty());
    assert_eq!(cut.notes.len(), 1);
    assert!(
        cut.notes[0].starts_with("apps/web/package.json: not valid JSON (line "),
        "{:?}",
        cut.notes
    );

    // Not an object, empty, and a byte-order mark.
    let list = parse_package_json("package.json", "[]");
    assert_eq!(list.notes, vec!["package.json: not a JSON object (line 1)"]);
    let empty = parse_package_json("package.json", "");
    assert_eq!(empty.notes.len(), 1);
    let bom = parse_package_json("package.json", "\u{feff}{\"name\": \"shop\"}");
    assert_eq!(bom.name.as_deref(), Some("shop"));
    assert!(bom.notes.is_empty());
}

#[test]
fn wrong_types_inside_known_fields_are_notes() {
    let m = parse_package_json(
        "package.json",
        r#"{
  "name": 7,
  "main": ["a.js"],
  "scripts": {"dev": 1, "ok": "x"},
  "dependencies": {"next": 15, "react": "^19"},
  "devDependencies": []
}"#,
    );
    assert!(m.name.is_none() && m.main.is_none());
    assert_eq!(m.scripts, vec![("ok".to_string(), "x".to_string(), 4)]);
    assert_eq!(m.deps.len(), 1);
    assert_eq!(m.deps[0].name, "react");
    assert_eq!(m.notes.len(), 5, "{:?}", m.notes);
    assert!(m.notes.iter().all(|n| n.starts_with("package.json: ")));
}

#[test]
fn peer_suffix_and_link_versions() {
    let lock = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      \
                next:\n        specifier: latest\n        version: 16.0.10(react@19.0.0)\n      \
                '@shop/ui':\n        specifier: workspace:*\n        version: link:../ui\n      \
                'tool':\n        specifier: file:../tool\n        version: file:../tool\n      \
                pkg:\n        specifier: workspace:^\n        version: workspace:../pkg\n";
    let lock = parse_lockfile("pnpm-lock.yaml", lock).unwrap();
    assert_eq!(
        lock.resolved(".", "next", "latest").as_deref(),
        Some("16.0.10")
    );
    assert_eq!(lock.resolved(".", "@shop/ui", "workspace:*"), None);
    assert_eq!(lock.resolved(".", "tool", "file:../tool"), None);
    assert_eq!(lock.resolved(".", "pkg", "workspace:^"), None);
}

#[test]
fn values_that_are_not_versions_resolve_to_nothing() {
    // pnpm writes an alias as `name@version` and a git or tarball dependency as its URL;
    // npm writes a git URL as `version` too. None of them is a version.
    let pnpm = "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      \
                width:\n        specifier: npm:string-width@^4.2.0\n        version: string-width@4.2.3\n      \
                gitdep:\n        specifier: github:shop/gitdep\n        version: https://codeload.github.com/shop/gitdep/tar.gz/0123abc\n      \
                tar:\n        specifier: https://shop.example/tar-1.0.0.tgz\n        version: https://shop.example/tar-1.0.0.tgz\n      \
                plain:\n        specifier: ^1.0.0\n        version: 1.0.4\n";
    let lock = parse_lockfile("pnpm-lock.yaml", pnpm).unwrap();
    assert_eq!(lock.resolved(".", "width", "npm:string-width@^4.2.0"), None);
    assert_eq!(lock.resolved(".", "gitdep", "github:shop/gitdep"), None);
    assert_eq!(
        lock.resolved(".", "tar", "https://shop.example/tar-1.0.0.tgz"),
        None
    );
    assert_eq!(
        lock.resolved(".", "plain", "^1.0.0").as_deref(),
        Some("1.0.4")
    );
    let npm = r#"{ "lockfileVersion": 3, "packages": {
        "node_modules/gitdep": { "version": "git+ssh://git@github.com/shop/gitdep.git#0123abc" },
        "node_modules/plain": { "version": "1.0.4" } } }"#;
    let lock = parse_lockfile("package-lock.json", npm).unwrap();
    assert_eq!(lock.resolved(".", "gitdep", "github:shop/gitdep"), None);
    assert_eq!(
        lock.resolved(".", "plain", "^1.0.0").as_deref(),
        Some("1.0.4")
    );
}

#[test]
fn older_pnpm_shapes_resolve() {
    // v6 without workspaces: the sections sit at the top, same shape as an importer.
    let v6 = "lockfileVersion: '6.0'\n\ndependencies:\n  next:\n    specifier: ^15.0.0\n    \
              version: 15.2.1\n\ndevDependencies:\n  typescript:\n    specifier: ^5\n    \
              version: 5.6.3(foo@1.0.0)\n\npackages:\n\n  /next@15.2.1:\n    dev: false\n";
    let lock = parse_lockfile("pnpm-lock.yaml", v6).unwrap();
    assert_eq!(
        lock.resolved(".", "next", "^15.0.0").as_deref(),
        Some("15.2.1")
    );
    assert_eq!(
        lock.resolved(".", "typescript", "^5").as_deref(),
        Some("5.6.3")
    );
    // v5: `name: version` on one line, with a `_` peer suffix.
    let v5 = "lockfileVersion: 5.4\n\nspecifiers:\n  next: ^12\n\ndependencies:\n  \
              next: 12.3.4_react@18.0.0\n";
    let lock = parse_lockfile("pnpm-lock.yaml", v5).unwrap();
    assert_eq!(lock.resolved(".", "next", "^12").as_deref(), Some("12.3.4"));
    // An importer written inline, and a quoted importer key.
    let inline = "lockfileVersion: '9.0'\nimporters:\n  'apps/api': {}\n  \"apps/web\":\n    \
                  dependencies:\n      next:\n        specifier: ^15\n        version: 15.0.0\n";
    let lock = parse_lockfile("pnpm-lock.yaml", inline).unwrap();
    assert_eq!(
        lock.resolved("apps/web", "next", "^15").as_deref(),
        Some("15.0.0")
    );
    assert_eq!(lock.resolved("apps/api", "next", "^15"), None);
}

#[test]
fn an_unknown_lockfile_is_an_error_value_and_a_note() {
    assert_eq!(
        parse_lockfile("pnpm-lock.yaml", "hello: world\n")
            .err()
            .as_deref(),
        Some("lockfile format not recognised")
    );
    assert!(parse_lockfile("yarn.lock", "just text\n").is_err());
    assert!(parse_lockfile("package-lock.json", "{\"a\": 1}").is_err());
    assert!(parse_lockfile("package-lock.json", "{ nope").is_err());
    assert!(parse_lockfile("bun.lock", "{}").is_err());

    let dir = copy_fixture("npm-locks/pnpm");
    fs::write(dir.path().join("pnpm-lock.yaml"), "not a lockfile\n").unwrap();
    let known = known_of(dir.path());
    let m = Manifests::load(dir.path(), &known, &mut ManifestCache::default());
    assert_eq!(
        m.notes,
        vec!["pnpm-lock.yaml: lockfile format not recognised".to_string()]
    );
    // The manifests stay, tied to the file, with nothing resolved.
    assert_eq!(m.npm.len(), 2);
    assert!(m.npm.iter().all(|n| {
        n.lockfile.as_deref() == Some("pnpm-lock.yaml")
            && n.deps.iter().all(|d| d.resolved.is_none())
    }));
}

#[test]
fn the_nearest_lockfile_wins_in_the_documented_order() {
    assert_eq!(
        LOCKFILES,
        &[
            "pnpm-lock.yaml",
            "package-lock.json",
            "npm-shrinkwrap.json",
            "yarn.lock"
        ]
    );
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join("apps/web")).unwrap();
    fs::create_dir_all(p.join("apps/api")).unwrap();
    fs::write(p.join("package.json"), "{}").unwrap();
    fs::write(p.join("apps/web/package.json"), "{}").unwrap();
    fs::write(p.join("apps/api/package.json"), "{}").unwrap();
    // The folder of `apps/api` has its own lockfiles, two of them.
    fs::write(p.join("apps/api/yarn.lock"), "# yarn lockfile v1\n").unwrap();
    fs::write(
        p.join("apps/api/package-lock.json"),
        "{\"lockfileVersion\": 3}",
    )
    .unwrap();
    fs::write(p.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let known = known_of(p);
    let m = Manifests::load(p, &known, &mut ManifestCache::default());
    let lock_of = |path: &str| {
        m.npm
            .iter()
            .find(|n| n.path == path)
            .and_then(|n| n.lockfile.clone())
    };
    assert_eq!(lock_of("package.json").as_deref(), Some("pnpm-lock.yaml"));
    assert_eq!(
        lock_of("apps/web/package.json").as_deref(),
        Some("pnpm-lock.yaml")
    );
    assert_eq!(
        lock_of("apps/api/package.json").as_deref(),
        Some("apps/api/package-lock.json")
    );
    // Sorted by path.
    let paths: Vec<_> = m.npm.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "apps/api/package.json",
            "apps/web/package.json",
            "package.json"
        ]
    );
}

#[test]
fn root_of_and_declares_use_the_nearest_manifest() {
    let m = load_project("pnpm");
    assert_eq!(m.root_of("apps/web/src/page.tsx").unwrap().root, "apps/web");
    assert_eq!(m.root_of("apps/web/package.json").unwrap().root, "apps/web");
    assert_eq!(m.root_of("apps/webshop/x.ts").unwrap().root, "");
    assert_eq!(m.root_of("src/x.ts").unwrap().root, "");
    assert!(m.declares("apps/web/app/page.tsx", "next"));
    // The root declares `typescript` but not `next`; the nearest manifest decides.
    assert!(!m.declares("src/x.ts", "next"));
    assert!(m.declares("src/x.ts", "typescript"));
    assert!(!m.declares("apps/web/app/page.tsx", "typescript"));
    assert!(Manifests::default().root_of("a.ts").is_none());
    assert!(!Manifests::default().declares("a.ts", "next"));
}

#[test]
fn a_package_json_under_node_modules_is_never_read() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("node_modules/x")).unwrap();
    fs::write(dir.path().join("node_modules/x/package.json"), "{}").unwrap();
    let known: BTreeSet<String> = ["node_modules/x/package.json".to_string()].into();
    let m = Manifests::load(dir.path(), &known, &mut ManifestCache::default());
    assert!(m.npm.is_empty());
}

#[test]
fn an_unreadable_package_json_is_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let known: BTreeSet<String> = ["package.json".to_string()].into();
    let m = Manifests::load(dir.path(), &known, &mut ManifestCache::default());
    assert!(m.npm.is_empty());
    assert_eq!(m.notes.len(), 1);
    assert!(m.notes[0].starts_with("package.json: could not be read"));
}

#[test]
fn manifests_reload_only_changed_files() {
    let dir = copy_fixture("npm-locks/pnpm");
    let known = known_of(dir.path());
    let mut cache = ManifestCache::default();
    let first = Manifests::load(dir.path(), &known, &mut cache);
    // Two manifests and one lockfile.
    assert_eq!(cache.parses(), 3);

    let again = Manifests::load(dir.path(), &known, &mut cache);
    assert_eq!(cache.parses(), 3, "nothing changed, nothing re-parsed");
    assert_eq!(again.npm, first.npm);

    // One byte in one package.json.
    let path = dir.path().join("apps/web/package.json");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("^15.0.0", "^15.0.1");
    fs::write(&path, text).unwrap();
    let after = Manifests::load(dir.path(), &known, &mut cache);
    assert_eq!(cache.parses(), 4, "only the changed file is parsed again");
    let web = after.npm.iter().find(|n| n.root == "apps/web").unwrap();
    assert_eq!(dep(web, "next").declared, "^15.0.1");
    assert_eq!(dep(web, "next").resolved.as_deref(), Some("15.2.1"));

    // The lockfile changes: only it is parsed again, and the new version shows.
    let lock = dir.path().join("pnpm-lock.yaml");
    let text = fs::read_to_string(&lock)
        .unwrap()
        .replace("15.2.1(", "15.2.2(");
    fs::write(&lock, text).unwrap();
    let after = Manifests::load(dir.path(), &known, &mut cache);
    assert_eq!(cache.parses(), 5);
    let web = after.npm.iter().find(|n| n.root == "apps/web").unwrap();
    assert_eq!(dep(web, "next").resolved.as_deref(), Some("15.2.2"));
}
