mod common;

use std::collections::BTreeSet;
use std::fs;

use common::resolver_for;
use hm_index::ts::resolve::{ModuleResolver, ModuleTarget};
use hm_index::ts::tsconfig::{TsConfigs, load_tsconfigs};

fn file(p: &str) -> ModuleTarget {
    ModuleTarget::File(p.into())
}

/// Writes `files` (relative path, text) into a temporary folder and loads its tsconfigs.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, TsConfigs, ModuleResolver) {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let known: BTreeSet<String> = files.iter().map(|(rel, _)| rel.to_string()).collect();
    let configs = load_tsconfigs(dir.path(), &known);
    let resolver = ModuleResolver::new(known, configs.clone());
    (dir, configs, resolver)
}

#[test]
fn fixture_specifiers_resolve() {
    let r = resolver_for("ts-basic");
    assert_eq!(
        r.resolve("src/index.ts", "@/orders/service"),
        file("src/orders/service.ts")
    );
    assert_eq!(
        r.resolve("src/ui/Page.tsx", "@/orders"),
        file("src/orders/index.ts")
    );
    assert_eq!(
        r.resolve("src/orders/service.ts", "../db"),
        file("src/db.ts")
    );
    assert_eq!(
        r.resolve("src/ui/Page.tsx", "./Button"),
        file("src/ui/Button.tsx")
    );
    assert_eq!(
        r.resolve("src/orders/service.ts", "zod"),
        ModuleTarget::External("zod".into())
    );
    assert_eq!(
        r.resolve("src/x.ts", "@scope/pkg/sub"),
        ModuleTarget::External("@scope/pkg".into())
    );
    assert_eq!(
        r.resolve("src/index.ts", "./missing"),
        ModuleTarget::Unresolved
    );
}

#[test]
fn the_fixture_config_chain_is_read() {
    let r = resolver_for("ts-basic");
    let sources: Vec<String> = r.config_sources().into_iter().collect();
    assert_eq!(sources, ["tsconfig.base.json", "tsconfig.json"]);
}

#[test]
fn tsconfig_with_comments_and_an_extends_loop_loads() {
    let (_dir, configs, _) = project(&[
        (
            "a/tsconfig.json",
            r#"{
  // the loop closes here
  "extends": "../b/tsconfig.json",
  "compilerOptions": { "paths": { "~/*": ["lib/*"], }, },
}"#,
        ),
        ("b/tsconfig.json", r#"{ "extends": "../a/tsconfig.json" }"#),
    ]);
    // The targets are resolved against the folder of the file that set them.
    assert!(
        configs.by_dir["a"]
            .paths
            .contains(&("~/*".to_string(), vec!["a/lib/*".to_string()]))
    );
}

#[test]
fn paths_resolve_against_the_file_that_set_them() {
    let (_dir, _, r) = project(&[
        (
            "a/tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "~/*": ["lib/*"] } } }"#,
        ),
        ("a/lib/util.ts", ""),
        ("a/src/main.ts", ""),
    ]);
    assert_eq!(r.resolve("a/src/main.ts", "~/util"), file("a/lib/util.ts"));
}

#[test]
fn a_child_replaces_inherited_paths_and_keeps_the_base_url() {
    let (_dir, _, r) = project(&[
        (
            "tsconfig.base.json",
            r#"{ "compilerOptions": { "baseUrl": "src", "paths": { "@old/*": ["old/*"] } } }"#,
        ),
        (
            "tsconfig.json",
            r#"{ "extends": "./tsconfig.base.json",
                 "compilerOptions": { "paths": { "@new/*": ["new/*"] } } }"#,
        ),
        ("src/new/a.ts", ""),
        ("src/old/a.ts", ""),
        ("src/util.ts", ""),
        ("src/main.ts", ""),
    ]);
    assert_eq!(r.resolve("src/main.ts", "@new/a"), file("src/new/a.ts"));
    // "@old/*" was replaced, so "@old/a" is a package again.
    assert_eq!(
        r.resolve("src/main.ts", "@old/a"),
        ModuleTarget::External("@old/a".into())
    );
    // baseUrl still comes from the base file.
    assert_eq!(r.resolve("src/main.ts", "util"), file("src/util.ts"));
}

#[test]
fn a_matching_pattern_without_a_target_is_unresolved_not_external() {
    let (_dir, _, r) = project(&[
        (
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["src/*"] } } }"#,
        ),
        ("src/main.ts", ""),
    ]);
    assert_eq!(r.resolve("src/main.ts", "@/nope"), ModuleTarget::Unresolved);
}

#[test]
fn the_nearest_ancestor_config_wins() {
    let (_dir, _, r) = project(&[
        (
            "tsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["root/*"] } } }"#,
        ),
        (
            "pkg/jsconfig.json",
            r#"{ "compilerOptions": { "paths": { "@/*": ["src/*"] } } }"#,
        ),
        ("root/a.ts", ""),
        ("pkg/src/a.ts", ""),
        ("pkg/src/main.ts", ""),
        ("main.ts", ""),
    ]);
    assert_eq!(r.resolve("pkg/src/main.ts", "@/a"), file("pkg/src/a.ts"));
    assert_eq!(r.resolve("main.ts", "@/a"), file("root/a.ts"));
}

#[test]
fn a_package_extends_is_noted_and_not_read() {
    let (_dir, configs, r) = project(&[(
        "tsconfig.json",
        r#"{ "extends": "@tsconfig/node20/tsconfig.json" }"#,
    )]);
    assert_eq!(
        configs.notes,
        ["tsconfig.json extends @tsconfig/node20/tsconfig.json, which Harbormap does not read"]
    );
    let sources: Vec<String> = r.config_sources().into_iter().collect();
    assert_eq!(sources, ["tsconfig.json"]);
}

#[test]
fn extension_probing_follows_the_documented_order() {
    let (_dir, _, r) = project(&[
        ("a.ts", ""),
        ("a.js", ""),
        ("b.js", ""),
        ("c/index.tsx", ""),
        ("d.d.ts", ""),
        ("e.ts", ""),
        ("main.ts", ""),
    ]);
    assert_eq!(r.resolve("main.ts", "./a"), file("a.ts"));
    assert_eq!(r.resolve("main.ts", "./a.js"), file("a.js")); // exact wins
    assert_eq!(r.resolve("main.ts", "./b.js"), file("b.js"));
    assert_eq!(r.resolve("main.ts", "./c"), file("c/index.tsx"));
    assert_eq!(r.resolve("main.ts", "./d"), file("d.d.ts"));
    assert_eq!(r.resolve("main.ts", "./e.js"), file("e.ts")); // ESM-style TS import
    assert_eq!(r.resolve("main.ts", "./e.json"), ModuleTarget::Unresolved);
}

#[test]
fn a_relative_path_above_the_root_is_unresolved() {
    let r = resolver_for("ts-basic");
    assert_eq!(
        r.resolve("src/index.ts", "../../../x"),
        ModuleTarget::Unresolved
    );
}
