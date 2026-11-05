mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::{Duration, SystemTime};

use common::{
    copy_fixture, derived_lines, dump, edit, fresh_index, fresh_index_of, indexed, indexer_for,
    stored_derived,
};
use hm_index::index::Indexer;
use hm_index::routes::derive_tree;
use hm_store::Store;

fn scalar(store: &Store, sql: &str) -> String {
    store
        .conn()
        .query_row(sql, [], |row| row.get::<_, rusqlite::types::Value>(0))
        .map(|v| match v {
            rusqlite::types::Value::Text(t) => t,
            rusqlite::types::Value::Integer(i) => i.to_string(),
            other => format!("{other:?}"),
        })
        .unwrap()
}

#[test]
fn full_index_of_the_fixture_reports_and_stores() {
    let (mut ix, _dir) = indexer_for("ts-basic");
    let r = ix.full().unwrap();
    assert_eq!(r.parsed, 10); // 7 .ts + 2 .tsx + 1 .js; ignored and skipped folders excluded
    assert_eq!(r.parsed_with_errors, 0);
    assert_eq!(r.not_supported_yet, 1); // tool.py
    assert_eq!(r.skipped.get("dir:node_modules"), Some(&1));
    assert_eq!(r.skipped.get("dir:dist"), Some(&1));
    assert_eq!(r.classes.get("test"), Some(&1));
    assert_eq!(r.edges.get("calls"), Some(&8));
    assert_eq!(r.edges.get("imports"), Some(&9));
    assert_eq!(r.refs.get("unresolved"), Some(&2));
    assert_eq!(r.refs.get("external"), Some(&3));
    assert_eq!(r.files.get("typescript"), Some(&7));
    assert_eq!(r.files.get("tsx"), Some(&2));
    assert_eq!(r.files.get("javascript"), Some(&1));
    assert_eq!(r.files.get("python"), Some(&1));
    assert!(r.files.get("other").is_some_and(|n| *n >= 3)); // Dockerfile, guide.md, tsconfigs
    assert_eq!(r.hashed, r.files.values().sum::<u32>());
    assert!(r.symbols > 0);

    let again = ix.full().unwrap();
    assert_eq!((again.hashed, again.parsed), (0, 0));
    assert_eq!(again.edges, r.edges);
    assert_eq!(dump(ix.store()), dump(fresh_index("ts-basic").store()));
}

#[test]
fn the_store_holds_status_hashes_meta_and_evidence() {
    let (ix, dir) = indexed("ts-basic");
    let s = ix.store();
    let status = |path: &str| {
        scalar(
            s,
            &format!("SELECT parse_status FROM file WHERE path = '{path}'"),
        )
    };
    assert_eq!(status("src/db.ts"), "parsed");
    assert_eq!(status("src/scripts/tool.py"), "not_supported_yet");
    assert_eq!(status("docs/guide.md"), "not_parsed");
    assert_eq!(status("Dockerfile"), "not_parsed");
    assert_eq!(status("tsconfig.json"), "not_parsed");
    assert_eq!(
        scalar(
            s,
            "SELECT count(*) FROM file WHERE parse_status = 'parsed' AND (interface_hash IS NULL OR facts_hash IS NULL OR facts_json IS NULL)"
        ),
        "0"
    );
    assert_eq!(
        scalar(
            s,
            "SELECT count(*) FROM file WHERE parse_status != 'parsed' AND (interface_hash IS NOT NULL OR facts_json IS NOT NULL)"
        ),
        "0"
    );
    let loc = scalar(s, "SELECT loc FROM file WHERE path = 'src/db.ts'");
    let lines = fs::read_to_string(dir.path().join("src/db.ts"))
        .unwrap()
        .lines()
        .count();
    assert_eq!(loc, lines.to_string());
    assert_eq!(
        scalar(
            s,
            "SELECT count(*) FROM edge WHERE prov != 'parsed' OR conf != 'sure' OR rule = ''"
        ),
        "0"
    );
    assert_eq!(
        scalar(s, "SELECT value FROM meta WHERE key = 'root'"),
        dir.path().to_string_lossy()
    );
    let at: u64 = scalar(s, "SELECT value FROM meta WHERE key = 'indexed_at'")
        .parse()
        .unwrap();
    assert!(at > 1_700_000_000);
    // `imports` edges and `reverse_import` rows agree.
    assert_eq!(
        scalar(
            s,
            "SELECT count(DISTINCT src_id || dst_id) FROM edge WHERE kind = 'imports'"
        ),
        scalar(s, "SELECT count(*) FROM reverse_import"),
    );
}

#[test]
fn an_unchanged_file_with_a_new_mtime_is_hashed_not_parsed() {
    let (mut ix, dir) = indexed("ts-basic");
    let before = dump(ix.store());
    let file = fs::OpenOptions::new()
        .write(true)
        .open(dir.path().join("src/db.ts"))
        .unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(10))
        .unwrap();
    drop(file);

    let r = ix.full().unwrap();
    assert_eq!((r.hashed, r.parsed), (1, 0));
    assert_eq!(dump(ix.store()), before);
    let again = ix.full().unwrap();
    assert_eq!((again.hashed, again.parsed), (0, 0));
}

#[test]
fn an_edit_is_reparsed_and_matches_a_cold_index() {
    let (mut ix, dir) = indexed("ts-basic");
    edit(dir.path(), "src/db.ts", "export", "// changed\nexport");
    let r = ix.full().unwrap();
    assert_eq!((r.hashed, r.parsed), (1, 1));
    assert_eq!(dump(ix.store()), dump(fresh_index_of(dir.path()).store()));
}

#[test]
fn new_deleted_and_binary_files_are_followed() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::write(dir.path().join("src/extra.ts"), "export const x = 1;\n").unwrap();
    fs::write(dir.path().join("blob.bin"), b"\0\x01\x02").unwrap();
    fs::remove_file(dir.path().join("src/legacy.js")).unwrap();
    let r = ix.full().unwrap();
    assert_eq!(r.parsed, 1);
    assert_eq!(r.skipped.get("binary"), Some(&1));
    assert_eq!(r.files.get("javascript"), None);
    let s = ix.store();
    assert_eq!(
        scalar(s, "SELECT parse_status FROM file WHERE path = 'blob.bin'"),
        "binary"
    );
    assert_eq!(
        scalar(s, "SELECT count(*) FROM file WHERE path = 'src/legacy.js'"),
        "0"
    );
    assert_eq!(dump(s), dump(fresh_index_of(dir.path()).store()));

    // A file that turns binary loses its facts.
    fs::write(dir.path().join("src/extra.ts"), b"\0\0\0").unwrap();
    let r = ix.full().unwrap();
    assert_eq!((r.parsed, r.skipped.get("binary")), (0, Some(&2)));
    assert_eq!(
        scalar(ix.store(), "SELECT count(*) FROM symbol WHERE name = 'x'"),
        "0"
    );
    assert_eq!(dump(ix.store()), dump(fresh_index_of(dir.path()).store()));
}

#[test]
fn a_large_file_is_counted_and_never_parsed() {
    let (mut ix, dir) = indexer_for("ts-basic");
    let big = format!("export const a = 1;\n{}", "// pad\n".repeat(200_000));
    fs::write(dir.path().join("src/big.ts"), big).unwrap();
    let r = ix.full().unwrap();
    assert_eq!(r.skipped.get("too_large"), Some(&1));
    assert_eq!(r.parsed, 10);
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT parse_status || '/' || loc FROM file WHERE path = 'src/big.ts'"
        ),
        "too_large/0"
    );
}

#[test]
fn a_syntax_error_is_reported_not_fatal() {
    let (mut ix, dir) = indexer_for("ts-basic");
    fs::write(dir.path().join("src/bad.ts"), "export function (((\n").unwrap();
    let r = ix.full().unwrap();
    assert_eq!((r.parsed, r.parsed_with_errors), (11, 1));
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT parse_status FROM file WHERE path = 'src/bad.ts'"
        ),
        "parsed_with_errors"
    );
}

#[test]
fn a_reopened_store_parses_nothing_again() {
    let dir = copy_fixture("ts-basic");
    let state = tempfile::tempdir().unwrap();
    let db = state.path().join("learn").join("facts.sqlite");
    let first = {
        let mut ix = Indexer::open(dir.path(), Store::open(&db).unwrap()).unwrap();
        ix.full().unwrap();
        dump(ix.store())
    };
    let mut ix = Indexer::open(dir.path(), Store::open(&db).unwrap()).unwrap();
    let r = ix.full().unwrap();
    assert_eq!((r.hashed, r.parsed), (0, 0));
    assert_eq!(dump(ix.store()), first);

    // The facts came back from the store: an edit that changes one file's imports is
    // still resolved against the other files without re-reading them.
    edit(dir.path(), "src/db.ts", "export", "// c\nexport");
    let r = ix.full().unwrap();
    assert_eq!((r.hashed, r.parsed), (1, 1));
    assert_eq!(dump(ix.store()), dump(fresh_index_of(dir.path()).store()));
}

#[test]
fn every_read_file_has_its_line_count_and_unread_ones_have_zero() {
    let (ix, dir) = indexed("ts-basic");
    for rel in [
        "docs/guide.md",
        "src/scripts/tool.py",
        "tsconfig.json",
        "Dockerfile",
    ] {
        let lines = fs::read_to_string(dir.path().join(rel))
            .unwrap()
            .lines()
            .count();
        assert!(lines > 0, "{rel}");
        assert_eq!(
            scalar(
                ix.store(),
                &format!("SELECT loc FROM file WHERE path = '{rel}'")
            ),
            lines.to_string(),
            "{rel}"
        );
    }
}

#[test]
fn a_stale_class_or_status_is_rewritten_even_with_the_same_bytes() {
    let (mut ix, _dir) = indexed("ts-basic");
    ix.store()
        .conn()
        .execute_batch(
            "UPDATE file SET class = 'doc' WHERE path = 'src/db.ts';
             UPDATE file SET parse_status = 'not_parsed' WHERE path = 'src/scripts/tool.py';",
        )
        .unwrap();
    // Same size and mtime, but the rows no longer say what a cold pass would write.
    let r = ix.full().unwrap();
    assert_eq!(r.hashed, 2);
    assert_eq!(dump(ix.store()), dump(fresh_index("ts-basic").store()));
}

#[test]
fn a_pass_with_nothing_to_do_rewrites_no_derived_rows() {
    let (mut ix, dir) = indexed("ts-basic");
    let conn = ix.store().conn();
    // Temp triggers record every write to the rows a resolve produces.
    let mut sql = String::from("CREATE TEMP TABLE writes(what TEXT);");
    for table in ["edge", "ref", "reverse_import", "symbol"] {
        for op in ["INSERT", "DELETE"] {
            sql.push_str(&format!(
                "CREATE TEMP TRIGGER w_{table}_{op} AFTER {op} ON main.{table}
                 BEGIN INSERT INTO temp.writes VALUES ('{op} {table}'); END;"
            ));
        }
    }
    conn.execute_batch(&sql).unwrap();
    let writes = |ix: &Indexer| scalar(ix.store(), "SELECT count(*) FROM temp.writes");

    let before = dump(ix.store());
    ix.full().unwrap();
    assert_eq!(writes(&ix), "0");
    assert_eq!(dump(ix.store()), before);

    // A touched file (same bytes) is not a change to resolve either.
    let file = fs::OpenOptions::new()
        .write(true)
        .open(dir.path().join("src/db.ts"))
        .unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(20))
        .unwrap();
    drop(file);
    ix.full().unwrap();
    assert_eq!(writes(&ix), "0");

    // A real edit does resolve again.
    edit(dir.path(), "src/db.ts", "export", "// c\nexport");
    ix.full().unwrap();
    assert_ne!(writes(&ix), "0");
}

#[cfg(unix)]
#[test]
fn unreadable_folders_and_files_are_counted_and_the_pass_goes_on() {
    let (mut ix, dir) = indexer_for("ts-basic");
    let Some(_orders) = common::lock(&dir.path().join("src/orders")) else {
        return;
    };
    let Some(_db) = common::lock(&dir.path().join("src/db.ts")) else {
        return;
    };
    let r = ix.full().unwrap();
    assert_eq!(r.skipped.get("unreadable"), Some(&2));
    // Four files in src/orders and src/db.ts are not parsed.
    assert_eq!(r.parsed, 5);
    let s = ix.store();
    assert_eq!(
        scalar(s, "SELECT parse_status FROM file WHERE path = 'src/db.ts'"),
        "unreadable"
    );
    assert_eq!(
        scalar(
            s,
            "SELECT count(*) FROM file WHERE path LIKE 'src/orders/%'"
        ),
        "0"
    );
    assert_eq!(dump(s), dump(fresh_index_of(dir.path()).store()));
}

#[test]
fn the_report_names_the_ignore_files_and_the_config_notes() {
    let (mut ix, dir) = indexer_for("ts-basic");
    let r = ix.full().unwrap();
    assert_eq!(r.ignore_files, vec![".gitignore"]);
    assert!(r.notes.is_empty(), "{:?}", r.notes);

    fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    fs::write(dir.path().join(".git/info/exclude"), "*.log\n").unwrap();
    fs::create_dir_all(dir.path().join("pkg")).unwrap();
    fs::write(
        dir.path().join("pkg/tsconfig.json"),
        r#"{ "extends": "@tsconfig/node20/tsconfig.json" }"#,
    )
    .unwrap();
    fs::write(dir.path().join("pkg/.harbormapignore"), "tmp/\n").unwrap();
    let r = ix.full().unwrap();
    assert_eq!(
        r.ignore_files,
        vec![".git/info/exclude", ".gitignore", "pkg/.harbormapignore"]
    );
    assert_eq!(
        r.notes,
        vec![
            "pkg/tsconfig.json extends @tsconfig/node20/tsconfig.json, which Harbormap \
             does not read"
        ]
    );
}

#[test]
fn report_counts_routes_and_manifests() {
    let (mut ix, dir) = indexer_for("client-hops");
    let r = ix.full().unwrap();
    // GET and POST /api/orders, GET and DELETE /api/orders/{id}.
    assert_eq!(r.routes.get("http"), Some(&4));
    assert_eq!(r.routes.values().sum::<u32>(), 4);
    assert_eq!(r.manifests, 1);
    // next, axios, @tanstack/react-query and swr.
    assert_eq!(r.dependencies, 4);

    // The store holds exactly what `derive` finds, and the report counts it.
    let derived = derive_tree(dir.path());
    assert_eq!(stored_derived(ix.store()), derived_lines(&derived));
    assert_eq!(r.client_calls, derived.client_calls.len() as u32);
    let matched: BTreeSet<(&str, u32)> = derived
        .edges
        .iter()
        .filter(|e| e.kind == "http_calls")
        .map(|e| (e.file.as_str(), e.line))
        .collect();
    assert_eq!(r.client_calls_matched, matched.len() as u32);
    assert_eq!(r.gaps, derived.gaps.len() as u32);
    let mut patterns: BTreeMap<String, u32> = BTreeMap::new();
    for p in &derived.patterns {
        *patterns.entry(p.kind.clone()).or_default() += 1;
    }
    assert_eq!(r.patterns, patterns);
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT name || ' ' || declared || ' ' || dev FROM dependency WHERE name = 'next'"
        ),
        "next 15.2.0 0"
    );
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT kind || ' ' || name || ' ' || detail FROM manifest"
        ),
        r#"npm client-hops {"bin":[],"main":null,"scripts":[]}"#
    );

    // A second pass finds the same and writes nothing new.
    let again = ix.full().unwrap();
    assert_eq!(
        (&again.routes, again.manifests, again.dependencies),
        (&r.routes, r.manifests, r.dependencies)
    );
    assert_eq!(dump(ix.store()), dump(fresh_index_of(dir.path()).store()));
}

#[test]
fn manifest_and_route_notes_join_the_report_notes() {
    let (mut ix, dir) = indexer_for("client-hops");
    // A route group gives the same URL as an existing route file, and the manifest
    // has a field of the wrong type.
    fs::create_dir_all(dir.path().join("app/(shop)/api/orders")).unwrap();
    fs::write(
        dir.path().join("app/(shop)/api/orders/route.ts"),
        "export async function GET() {\n  return Response.json([])\n}\n",
    )
    .unwrap();
    edit(
        dir.path(),
        "package.json",
        "\"private\": true,",
        "\"private\": true,\n  \"main\": 5,",
    );
    let r = ix.full().unwrap();
    assert_eq!(
        r.notes,
        vec![
            // The first in path order is kept.
            "app/api/orders/route.ts:1: GET /api/orders is also declared at \
             app/(shop)/api/orders/route.ts:1",
            "package.json: \"main\" is not a string (line 4)",
        ]
    );
}

#[test]
fn the_store_holds_exactly_what_derive_finds_on_every_fixture() {
    for name in [
        "client-hops",
        "express-api",
        "nest-api",
        "next-app",
        "next-mono",
        "next-pages",
        "npm-locks",
        "patterns-ts",
        "ts-basic",
    ] {
        let (ix, dir) = indexed(name);
        assert_eq!(
            stored_derived(ix.store()),
            derived_lines(&derive_tree(dir.path())),
            "{name}"
        );
    }
}

#[test]
fn rows_with_the_same_store_key_are_derived_once() {
    // Two calls on one line from two symbols share a `client_call` key; a `fetch` and an
    // axios call on one line share an `http_calls` edge key. The store keeps one row of
    // each key; so does `derive`, the first in sorted order.
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in [
        (
            "package.json",
            r#"{ "name": "shop", "dependencies": { "next": "15.2.0" } }"#,
        ),
        (
            "app/api/orders/route.ts",
            "export async function GET() { return Response.json([]) }\n",
        ),
        (
            "lib/c.ts",
            "import axios from 'axios'\n\
             export const a = () => fetch('/api/orders'), b = () => fetch('/api/orders')\n\
             export function both() { fetch('/api/orders'); return axios.get('/api/orders') }\n",
        ),
    ] {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let derived = derive_tree(dir.path());
    let calls_on = |line: u32| {
        derived
            .client_calls
            .iter()
            .filter(|c| c.line == line && c.rule == "ts.client.fetch")
            .count()
    };
    assert_eq!(calls_on(2), 1, "{:#?}", derived.client_calls);
    let edges_on_3 = derived
        .edges
        .iter()
        .filter(|e| e.kind == "http_calls" && e.line == 3)
        .count();
    assert_eq!(edges_on_3, 1, "{:#?}", derived.edges);
    assert_eq!(
        stored_derived(fresh_index_of(dir.path()).store()),
        derived_lines(&derived)
    );
}
