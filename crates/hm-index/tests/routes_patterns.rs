//! Patterns and gaps of TS libraries (Learn spec §7.3), from the embedded pack.

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{copy_fixture, fixture, read_fixture};
use hm_core::ids::file_id;
use hm_index::lang::Lang;
use hm_index::routes::{Derived, derive_tree};
use hm_index::ts::resolve::symbol_ids;

const PACK: &str = include_str!("../../../packs/learn/patterns-ts.toml");

const KINDS: [&str; 10] = [
    "orm_read",
    "orm_write",
    "transaction",
    "validation",
    "caching",
    "retry",
    "di",
    "queue",
    "password_hash",
    "token_session",
];

/// `(line, kind, rule, conf)` of every pattern of `file`.
fn found(d: &Derived, file: &str) -> BTreeSet<(u32, String, String, String)> {
    d.patterns
        .iter()
        .filter(|p| p.file == file)
        .map(|p| (p.line, p.kind.clone(), p.rule.clone(), p.conf.clone()))
        .collect()
}

fn row(line: u32, kind: &str, rule: &str, conf: &str) -> (u32, String, String, String) {
    (line, kind.into(), rule.into(), conf.into())
}

/// The id of the symbol `owner`.`name` of fixture file `patterns-ts/rel`.
fn symbol(rel: &str, owner: &str, name: &str) -> String {
    let lang = Lang::from_path(std::path::Path::new(rel)).expect("a source file");
    let facts = hm_index::ts::extract(&read_fixture(&format!("patterns-ts/{rel}")), lang);
    let at = facts
        .symbols
        .iter()
        .position(|s| s.owner == owner && s.name == name)
        .unwrap_or_else(|| panic!("no symbol {name} in {rel}"));
    symbol_ids(rel, &facts).swap_remove(at)
}

#[test]
fn pack_parses_and_every_kind_is_known() {
    let table: toml::Table = toml::from_str(PACK).expect("the pack is valid TOML");
    let rules = table["rule"].as_array().expect("[[rule]] entries");
    assert!(rules.len() >= 25, "{} rules", rules.len());
    let mut ids = BTreeSet::new();
    for rule in rules {
        let rule = rule.as_table().expect("a table");
        let id = rule["id"].as_str().expect("id");
        assert!(ids.insert(id.to_string()), "duplicate id {id}");
        let kind = rule["kind"].as_str().expect("kind");
        assert!(KINDS.contains(&kind), "{id}: unknown kind {kind}");
        let scope = rule["scope"].as_str().expect("scope");
        assert!(
            ["import", "manifest", "global"].contains(&scope),
            "{id}: scope {scope}"
        );
        let receiver = rule
            .get("receiver")
            .and_then(|v| v.as_str())
            .unwrap_or("any");
        assert!(
            ["any", "package", "none"].contains(&receiver),
            "{id}: receiver {receiver}"
        );
        let list = |key: &str| rule.get(key).and_then(|v| v.as_array()).map_or(0, Vec::len);
        assert!(
            list("callees") + list("decorators") > 0,
            "{id}: nothing to match"
        );
        assert!(
            rule["term"].as_str().is_some_and(|t| !t.is_empty()),
            "{id}: term"
        );
        let package = rule.get("package").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(scope == "global", package.is_empty(), "{id}: package");
        assert_eq!(
            scope == "global",
            list("receiver_names") > 0,
            "{id}: receiver_names"
        );
        for key in rule.keys() {
            assert!(
                [
                    "id",
                    "kind",
                    "package",
                    "scope",
                    "callees",
                    "receiver",
                    "decorators",
                    "term",
                    "receiver_names",
                    "receiver_depth"
                ]
                .contains(&key.as_str()),
                "{id}: unknown key {key}"
            );
        }
    }
    for id in [
        "prisma.read",
        "prisma.write",
        "prisma.transaction",
        "drizzle.read",
        "typeorm.transaction",
        "mongoose.read",
        "postgres.sql",
        "vercel-postgres.sql",
        "zod.parse",
        "yup.validate",
        "joi.validate",
        "class-validator.validate",
        "next-cache.revalidate",
        "react.cache",
        "p-retry.retry",
        "abort-signal.timeout",
        "di",
        "bullmq.queue",
        "bcrypt.hash",
        "bcryptjs.hash",
        "argon2.hash",
        "jsonwebtoken.token",
        "jose.token",
        "next-auth.session",
        "next-headers.cookies",
    ] {
        assert!(ids.contains(id), "no rule {id}");
    }
}

#[test]
fn prisma_zod_hash_and_token_patterns_are_found() {
    let d = derive_tree(&fixture("patterns-ts"));
    let want: BTreeSet<_> = [
        // `prisma` is a local `new PrismaClient()`: not bound to the package by an import.
        row(10, "orm_read", "ts.pattern.prisma.read", "probably"),
        row(14, "validation", "ts.pattern.zod.parse", "probably"),
        // `z` is imported from zod.
        row(15, "validation", "ts.pattern.zod.parse", "sure"),
        row(17, "orm_write", "ts.pattern.prisma.write", "probably"),
        row(
            17,
            "transaction",
            "ts.pattern.prisma.transaction",
            "probably",
        ),
        row(22, "password_hash", "ts.pattern.bcryptjs.hash", "sure"),
        row(23, "token_session", "ts.pattern.jsonwebtoken.token", "sure"),
        row(23, "env_read", "ts.pattern.env", "sure"),
    ]
    .into_iter()
    .collect();
    assert_eq!(found(&d, "src/orders.ts"), want);

    // The subject is the symbol that holds the call.
    let list = symbol("src/orders.ts", "", "listOrders");
    let read = d
        .patterns
        .iter()
        .find(|p| p.kind == "orm_read" && p.file == "src/orders.ts")
        .expect("the read");
    assert_eq!(read.subject_id, list);
    assert_eq!(read.term.as_deref(), Some("orm"));
    assert_eq!(read.detail, None);
}

#[test]
fn builtins_and_other_libraries_do_not_match() {
    let d = derive_tree(&fixture("patterns-ts"));
    let orders = found(&d, "src/orders.ts");
    // JSON.parse (line 16) is no zod parse; Map and URLSearchParams `delete` (31, 32)
    // are no ORM writes.
    for line in [16, 31, 32] {
        assert!(
            orders.iter().all(|r| r.0 != line || r.1 == "env_read"),
            "line {line}: {orders:?}"
        );
    }
    // A `sql` tag from drizzle-orm and a `sign` from node:crypto are other libraries'.
    assert_eq!(found(&d, "src/report.ts"), BTreeSet::new());
}

#[test]
fn tagged_template_sql_is_a_call_with_no_receiver() {
    let d = derive_tree(&fixture("patterns-ts"));
    assert_eq!(
        found(&d, "src/pg.ts"),
        [row(4, "orm_read", "ts.pattern.vercel-postgres.sql", "sure")]
            .into_iter()
            .collect()
    );
}

#[test]
fn decorator_patterns_belong_to_the_decorated_class() {
    let d = derive_tree(&fixture("patterns-ts"));
    let di: Vec<_> = d
        .patterns
        .iter()
        .filter(|p| p.file == "src/orders.service.ts")
        .collect();
    assert_eq!(di.len(), 1, "{di:?}");
    assert_eq!(di[0].kind, "di");
    assert_eq!(di[0].rule, "ts.pattern.di");
    assert_eq!(di[0].conf, "sure");
    assert_eq!(
        di[0].subject_id,
        symbol("src/orders.service.ts", "", "OrdersService")
    );
}

#[test]
fn queue_cache_cookie_and_timeout_patterns() {
    let d = derive_tree(&fixture("patterns-ts"));
    let want: BTreeSet<_> = [
        row(5, "queue", "ts.pattern.bullmq.queue", "sure"),
        row(6, "queue", "ts.pattern.bullmq.queue", "sure"),
        row(9, "caching", "ts.pattern.next-cache.revalidate", "sure"),
        row(
            10,
            "token_session",
            "ts.pattern.next-headers.cookies",
            "sure",
        ),
        row(11, "retry", "ts.pattern.abort-signal.timeout", "sure"),
    ]
    .into_iter()
    .collect();
    assert_eq!(found(&d, "src/jobs.ts"), want);
}

#[test]
fn env_reads_and_gaps_are_recorded() {
    let d = derive_tree(&fixture("patterns-ts"));
    let env: Vec<_> = d.patterns.iter().filter(|p| p.kind == "env_read").collect();
    assert_eq!(env.len(), 1);
    assert_eq!(env[0].detail.as_deref(), Some("SHOP_SECRET"));
    assert_eq!(env[0].term.as_deref(), Some("env_var"));
    assert_eq!(env[0].line, 23);
    assert_eq!(env[0].subject_id, symbol("src/orders.ts", "", "signUp"));

    assert_eq!(d.gaps.len(), 1, "{:?}", d.gaps);
    let gap = &d.gaps[0];
    assert_eq!((gap.kind.as_str(), gap.line), ("eval", 27));
    assert_eq!(gap.file, "src/orders.ts");
    assert_eq!(gap.subject_id, symbol("src/orders.ts", "", "evaluate"));
}

#[test]
fn top_level_facts_belong_to_the_file() {
    let dir = copy_fixture("patterns-ts");
    fs::write(
        dir.path().join("src/top.ts"),
        "export const key = process.env.TOP_KEY;\neval(key);\n",
    )
    .unwrap();
    let d = derive_tree(dir.path());
    let env = d.patterns.iter().find(|p| p.file == "src/top.ts").unwrap();
    assert_eq!(env.subject_id, file_id("src/top.ts"));
    let gap = d.gaps.iter().find(|g| g.file == "src/top.ts").unwrap();
    assert_eq!(gap.subject_id, file_id("src/top.ts"));
}

#[test]
fn no_manifest_no_manifest_scoped_pattern() {
    let dir = copy_fixture("patterns-ts");
    common::edit(
        dir.path(),
        "package.json",
        "    \"@prisma/client\": \"6.0.0\",\n",
        "",
    );
    let d = derive_tree(dir.path());
    let orders = found(&d, "src/orders.ts");
    assert!(orders.iter().all(|r| r.1 != "orm_read"), "{orders:?}");
    assert!(
        d.patterns
            .iter()
            .all(|p| !p.rule.starts_with("ts.pattern.prisma"))
    );
    // Rules of other libraries still hold.
    assert!(orders.iter().any(|r| r.2 == "ts.pattern.zod.parse"));

    // With no package.json at all, no manifest-scoped rule applies; import-scoped ones do.
    fs::remove_file(dir.path().join("package.json")).unwrap();
    let d = derive_tree(dir.path());
    let rules: BTreeSet<_> = d.patterns.iter().map(|p| p.rule.as_str()).collect();
    assert!(!rules.contains("ts.pattern.zod.parse"), "{rules:?}");
    assert!(!rules.contains("ts.pattern.di"), "{rules:?}");
    assert!(!rules.contains("ts.pattern.bullmq.queue"), "{rules:?}");
    assert!(rules.contains("ts.pattern.bcryptjs.hash"), "{rules:?}");
}

#[test]
fn import_scope_needs_the_import() {
    let dir = copy_fixture("patterns-ts");
    common::edit(
        dir.path(),
        "src/orders.ts",
        "import bcrypt from \"bcryptjs\";\n",
        "const bcrypt = { hash: async (a: string, b: number) => a + b };\n",
    );
    let d = derive_tree(dir.path());
    assert!(
        d.patterns.iter().all(|p| p.kind != "password_hash"),
        "{:?}",
        d.patterns
    );
}

#[test]
fn every_list_is_sorted() {
    let d = derive_tree(&fixture("patterns-ts"));
    assert!(d.patterns.windows(2).all(|w| w[0] <= w[1]));
    assert!(d.gaps.windows(2).all(|w| w[0] <= w[1]));
    let mut again = d.patterns.clone();
    again.dedup();
    assert_eq!(again, d.patterns);
}

#[test]
fn aliases_and_default_imports_match_by_the_imported_name() {
    let dir = copy_fixture("patterns-ts");
    common::edit(
        dir.path(),
        "package.json",
        "\"zod\": \"3.24.0\"",
        "\"p-retry\": \"7.0.0\",\n    \"postgres\": \"3.4.0\",\n    \"zod\": \"3.24.0\"",
    );
    fs::write(
        dir.path().join("src/alias.ts"),
        "import { hash as digest } from \"argon2\";\n\
         import again from \"p-retry\";\n\
         import postgres from \"postgres\";\n\
         const sql = postgres(\"x\");\n\
         export async function run(pw: string) {\n\
         \x20 await digest(pw);\n\
         \x20 await again(async () => 1);\n\
         \x20 return sql`select 1`;\n\
         }\n",
    )
    .unwrap();
    let d = derive_tree(dir.path());
    let want: BTreeSet<_> = [
        row(6, "password_hash", "ts.pattern.argon2.hash", "sure"),
        row(7, "retry", "ts.pattern.p-retry.retry", "sure"),
        // `sql` is a local value, not an import of the package.
        // The manifest also declares @vercel/postgres, so both `sql` rules say "probably".
        row(8, "orm_read", "ts.pattern.postgres.sql", "probably"),
        row(8, "orm_read", "ts.pattern.vercel-postgres.sql", "probably"),
    ]
    .into_iter()
    .collect();
    assert_eq!(found(&d, "src/alias.ts"), want);
}
