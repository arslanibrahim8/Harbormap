use std::collections::BTreeMap;
use std::path::Path;

use hm_index::facts::FileFacts;
use hm_index::lang::Lang;
use hm_index::ts::resolve::{
    ModuleResolver, Project, binding_package, resolve_callee, source_id, symbol_ids,
};
use hm_index::ts::tsconfig::TsConfigs;
use hm_index::ts::web::normalize_client_url;

/// Extracts `files` (relative path, text) without touching the disk.
fn facts_of(files: &[(&str, &str)]) -> (BTreeMap<String, FileFacts>, ModuleResolver) {
    let facts = files
        .iter()
        .map(|(rel, text)| {
            let lang = Lang::from_path(Path::new(rel)).unwrap();
            (rel.to_string(), hm_index::ts::extract(text, lang))
        })
        .collect();
    let known = files.iter().map(|(rel, _)| rel.to_string()).collect();
    (facts, ModuleResolver::new(known, TsConfigs::default()))
}

#[test]
fn files_without_web_facts_serialise_as_in_l1() {
    let f = hm_index::ts::extract("export function a() { return b(); }", Lang::TypeScript);
    assert!(f.web.is_empty());
    let json = serde_json::to_value(&f).unwrap();
    assert!(json.get("web").is_none(), "{json}");
    let back: FileFacts = serde_json::from_value(json).unwrap();
    assert_eq!(back, f);
}

#[test]
fn old_facts_json_without_web_reads() {
    let old =
        r#"{"status":"parsed","symbols":[],"imports":[],"exports":[],"calls":[],"heritage":[]}"#;
    let f: FileFacts = serde_json::from_str(old).unwrap();
    assert!(f.web.is_empty());
}

#[test]
fn client_urls_normalise() {
    for (raw, want) in [
        ("/api/orders/{}", "/api/orders/{}"),
        ("/api/orders?page=2", "/api/orders"),
        ("https://shop.example/api/x#top", "/api/x"),
        ("{}/api/orders", "{}/api/orders"),
        ("/api//x/", "/api/x"),
    ] {
        assert_eq!(normalize_client_url(raw), want, "{raw}");
    }
}

#[test]
fn resolution_helpers_name_symbols_and_packages() {
    let (facts, modules) = facts_of(&[
        (
            "a.ts",
            "import express from 'express'; import { place } from './b'; export function run() { place(); }",
        ),
        ("b.ts", "export function place() {}"),
    ]);
    let p = Project {
        facts: &facts,
        modules: &modules,
    };
    assert_eq!(
        resolve_callee(&p, "a.ts", None, "place"),
        Some(("b.ts".to_string(), 0))
    );
    assert_eq!(resolve_callee(&p, "a.ts", None, "express"), None);
    assert_eq!(resolve_callee(&p, "a.ts", None, "missing"), None);
    assert_eq!(
        binding_package(&p, "a.ts", "express"),
        Some(("express".to_string(), "default".to_string()))
    );
    assert_eq!(binding_package(&p, "a.ts", "place"), None);
    assert_eq!(binding_package(&p, "a.ts", "nothing"), None);
    let a = &facts["a.ts"];
    let run = symbol_ids("a.ts", a)[0].clone();
    assert_eq!(
        source_id("a.ts", a, Some(&("".to_string(), "run".to_string())), 1),
        run
    );
    assert_eq!(source_id("a.ts", a, None, 1), hm_core::ids::file_id("a.ts"));
}

#[test]
fn a_member_receiver_resolves_to_a_static_method() {
    let (facts, modules) = facts_of(&[
        (
            "a.ts",
            "import { UsersService } from './s'; export function f() {}",
        ),
        (
            "s.ts",
            "export class UsersService { static readUserMe() {} }",
        ),
    ]);
    let p = Project {
        facts: &facts,
        modules: &modules,
    };
    let found = resolve_callee(&p, "a.ts", Some("UsersService"), "readUserMe").unwrap();
    assert_eq!(found.0, "s.ts");
    assert_eq!(facts["s.ts"].symbols[found.1].name, "readUserMe");
}
