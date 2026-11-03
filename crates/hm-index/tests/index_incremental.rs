mod common;

use std::fs;

use common::{derived_lines, dump, edit, fresh_index_of, indexed, set, stored_derived};
use hm_core::ids::file_id;
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

/// The `[edge]` section of `dump`, one row per line.
fn edges(ix: &Indexer) -> Vec<String> {
    section(ix, "edge")
}

fn refs(ix: &Indexer) -> Vec<String> {
    section(ix, "ref")
}

fn section(ix: &Indexer, name: &str) -> Vec<String> {
    let text = dump(ix.store());
    let header = format!("[{name}] ");
    text.lines()
        .skip_while(|l| !l.starts_with(&header))
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .map(str::to_string)
        .collect()
}

fn has_edge(ix: &Indexer, kind: &str, from: &str, to: &str) -> bool {
    let prefix = format!("{kind} | {from} -> {to} |");
    edges(ix).iter().any(|e| e.starts_with(&prefix))
}

fn has_ref(ix: &Indexer, file: &str, text: &str, status: &str) -> bool {
    refs(ix).iter().any(|r| {
        let cols: Vec<&str> = r.split(" | ").collect();
        cols[0] == file && cols[2] == text && cols[4] == status
    })
}

/// Edge rows whose destination is neither a stored symbol nor a stored file.
fn dangling_edges(ix: &Indexer) -> String {
    scalar(
        ix.store(),
        "SELECT count(*) FROM edge WHERE dst_id NOT IN (SELECT id FROM symbol)
                                     AND dst_id NOT IN (SELECT id FROM file)",
    )
}

fn assert_cold(ix: &Indexer, dir: &std::path::Path) {
    assert_eq!(dump(ix.store()), dump(fresh_index_of(dir).store()));
}

#[test]
fn a_body_edit_reparses_one_file_and_touches_no_importer() {
    let (mut ix, dir) = indexed("ts-basic");
    edit(
        dir.path(),
        "src/orders/mail.ts",
        "return order;",
        "return { order };",
    );
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!(d.files_changed, vec!["src/orders/mail.ts"]);
    assert!(d.interface_changed.is_empty() && d.reresolved.is_empty());
    assert_eq!(d.symbols_body_changed.len(), 1);
    assert!(d.symbols_added.is_empty() && d.symbols_removed.is_empty());
    assert!(d.symbols_sig_changed.is_empty());
    assert_eq!((d.edges_added, d.edges_removed), (0, 0));
    assert_cold(&ix, dir.path());
}

#[test]
fn renaming_an_export_reresolves_importers() {
    let (mut ix, dir) = indexed("ts-basic");
    assert!(has_edge(
        &ix,
        "calls",
        "src/orders/service.ts#placeOrder",
        "src/orders/mail.ts#sendReceipt"
    ));
    edit(
        dir.path(),
        "src/orders/mail.ts",
        "function sendReceipt(",
        "function sendMail(",
    );
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!(d.files_changed, vec!["src/orders/mail.ts"]);
    assert_eq!(d.interface_changed, vec!["src/orders/mail.ts"]);
    // service.ts imports mail.ts; orders/index.ts re-exports from it, so Page.tsx,
    // which imports orders/index.ts, is resolved again too.
    assert_eq!(
        d.reresolved,
        vec![
            "src/orders/index.ts",
            "src/orders/service.ts",
            "src/ui/Page.tsx"
        ]
    );
    assert_eq!((d.symbols_added.len(), d.symbols_removed.len()), (1, 1));
    assert_eq!((d.edges_added, d.edges_removed), (0, 2));
    assert!(!edges(&ix).iter().any(|e| e.contains("mail.ts#send")));
    assert!(has_ref(
        &ix,
        "src/orders/service.ts",
        "mail.sendReceipt",
        "unresolved"
    ));
    assert!(has_ref(&ix, "src/ui/Page.tsx", "receipt", "unresolved"));
    assert_eq!(dangling_edges(&ix), "0");
    assert_cold(&ix, dir.path());

    // And back: the edges return.
    edit(
        dir.path(),
        "src/orders/mail.ts",
        "function sendMail(",
        "function sendReceipt(",
    );
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!((d.edges_added, d.edges_removed), (2, 0));
    assert!(has_edge(
        &ix,
        "calls",
        "src/ui/Page.tsx#Page",
        "src/orders/mail.ts#sendReceipt"
    ));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_rename_reaches_importers_through_a_chain_of_barrels() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::write(
        dir.path().join("src/top.ts"),
        "export * from \"./orders\";\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/use.ts"),
        "import { receipt } from \"./top\";\nexport function use() {\n  receipt(1);\n}\n",
    )
    .unwrap();
    ix.apply(&set(["src/top.ts", "src/use.ts"])).unwrap();
    assert!(has_edge(
        &ix,
        "calls",
        "src/use.ts#use",
        "src/orders/mail.ts#sendReceipt"
    ));

    edit(
        dir.path(),
        "src/orders/mail.ts",
        "function sendReceipt(",
        "function sendMail(",
    );
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert!(d.reresolved.contains(&"src/top.ts".to_string()));
    assert!(d.reresolved.contains(&"src/use.ts".to_string()));
    assert!(has_ref(&ix, "src/use.ts", "receipt", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_rename_travels_the_longest_re_export_chain() {
    let (mut ix, dir) = indexed("ts-basic");
    // use.ts → b5 → b4 → b3 → b2 → b1 → mail.ts: five re-export hops, the most that
    // resolving follows.
    let mut paths = vec!["src/use.ts".to_string()];
    for k in 1..=5 {
        let from = if k == 1 {
            "./orders/mail".to_string()
        } else {
            format!("./b{}", k - 1)
        };
        fs::write(
            dir.path().join(format!("src/b{k}.ts")),
            format!("export * from \"{from}\";\n"),
        )
        .unwrap();
        paths.push(format!("src/b{k}.ts"));
    }
    fs::write(
        dir.path().join("src/use.ts"),
        "import { sendReceipt } from \"./b5\";\nexport function use() {\n  sendReceipt(1);\n}\n",
    )
    .unwrap();
    ix.apply(&paths.iter().cloned().collect()).unwrap();
    assert!(has_edge(
        &ix,
        "calls",
        "src/use.ts#use",
        "src/orders/mail.ts#sendReceipt"
    ));

    edit(
        dir.path(),
        "src/orders/mail.ts",
        "function sendReceipt(",
        "function sendMail(",
    );
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert!(d.reresolved.contains(&"src/use.ts".to_string()));
    assert!(has_ref(&ix, "src/use.ts", "sendReceipt", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_symbol_whose_id_changes_sends_its_importers_to_resolve_again() {
    let (mut ix, dir) = indexed("ts-basic");
    // A second `sendReceipt` gives both a sig_hash disambiguator, so the id that
    // service.ts and Page.tsx point at goes away.
    edit(
        dir.path(),
        "src/orders/mail.ts",
        "export function sendReceipt",
        "function sendReceipt(n: number) {\n  return n;\n}\nexport function sendReceipt",
    );
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert!(d.reresolved.contains(&"src/orders/service.ts".to_string()));
    assert!(d.reresolved.contains(&"src/ui/Page.tsx".to_string()));
    assert_eq!(d.symbols_removed.len(), 1);
    assert_eq!(dangling_edges(&ix), "0");
    assert_cold(&ix, dir.path());
}

#[test]
fn moving_the_source_of_a_handed_on_import_reaches_importers() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::write(
        dir.path().join("src/fwd.ts"),
        "import { sendReceipt } from \"./orders/mail\";\nexport { sendReceipt };\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/use.ts"),
        "import { sendReceipt } from \"./fwd\";\nexport function use() {\n  sendReceipt(1);\n}\n",
    )
    .unwrap();
    ix.apply(&set(["src/fwd.ts", "src/use.ts"])).unwrap();
    assert!(has_edge(
        &ix,
        "calls",
        "src/use.ts#use",
        "src/orders/mail.ts#sendReceipt"
    ));

    // The interface of fwd.ts (its exports) stays the same; only where the name
    // comes from changes.
    edit(
        dir.path(),
        "src/fwd.ts",
        "import { sendReceipt } from \"./orders/mail\"",
        "import { placeOrder as sendReceipt } from \"./orders/service\"",
    );
    let d = ix.apply(&set(["src/fwd.ts"])).unwrap();
    assert!(d.interface_changed.is_empty());
    assert_eq!(d.reresolved, vec!["src/use.ts"]);
    assert!(has_edge(
        &ix,
        "calls",
        "src/use.ts#use",
        "src/orders/service.ts#placeOrder"
    ));
    assert_cold(&ix, dir.path());
}

#[test]
fn adding_the_missing_file_resolves_a_waiting_import() {
    let (mut ix, dir) = indexed("ts-basic");
    edit(
        dir.path(),
        "src/index.ts",
        "import { placeOrder }",
        "import { x } from \"./later\";\nimport { placeOrder }",
    );
    ix.apply(&set(["src/index.ts"])).unwrap();
    assert!(has_ref(&ix, "src/index.ts", "./later", "unresolved"));
    assert_cold(&ix, dir.path());

    fs::write(dir.path().join("src/later.ts"), "export const x = 1;\n").unwrap();
    let d = ix.apply(&set(["src/later.ts"])).unwrap();
    assert_eq!(d.files_changed, vec!["src/later.ts"]);
    assert!(d.reresolved.contains(&"src/index.ts".to_string()));
    assert!(has_edge(&ix, "imports", "src/index.ts", "src/later.ts"));
    assert!(!has_ref(&ix, "src/index.ts", "./later", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_file_that_vanishes_before_reading_is_a_delete() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::remove_file(dir.path().join("src/db.ts")).unwrap();
    let d = ix.apply(&set(["src/db.ts"])).unwrap();
    assert_eq!(d.files_removed, vec!["src/db.ts"]);
    assert!(d.files_changed.is_empty());
    assert!(d.reresolved.contains(&"src/orders/service.ts".to_string()));
    assert!(!d.reresolved.contains(&"src/db.ts".to_string()));
    assert_eq!(d.symbols_removed.len(), 1);
    assert!(has_ref(&ix, "src/orders/service.ts", "../db", "unresolved"));
    assert_cold(&ix, dir.path());
}

#[test]
fn removing_a_folder_removes_its_files() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::remove_dir_all(dir.path().join("src/orders")).unwrap();
    let d = ix.apply(&set(["src/orders"])).unwrap();
    assert_eq!(
        d.files_removed,
        vec![
            "src/orders/index.ts",
            "src/orders/mail.ts",
            "src/orders/service.test.ts",
            "src/orders/service.ts",
        ]
    );
    assert!(d.reresolved.contains(&"src/index.ts".to_string()));
    assert_eq!(dangling_edges(&ix), "0");
    assert_cold(&ix, dir.path());
}

#[test]
fn typing_an_error_keeps_edges_into_intact_symbols() {
    let (mut ix, dir) = indexed("ts-basic");
    let path = dir.path().join("src/orders/mail.ts");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}\nexport function half( {{")).unwrap();
    ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT parse_status FROM file WHERE path = 'src/orders/mail.ts'"
        ),
        "parsed_with_errors"
    );
    assert!(has_edge(
        &ix,
        "calls",
        "src/orders/service.ts#placeOrder",
        "src/orders/mail.ts#sendReceipt"
    ));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_tsconfig_change_reresolves_without_reparsing() {
    let (mut ix, dir) = indexed("ts-basic");
    edit(dir.path(), "tsconfig.json", "\"src/*\"", "\"src/orders/*\"");
    let d = ix.apply(&set(["tsconfig.json"])).unwrap();
    assert_eq!(d.files_changed, vec!["tsconfig.json"]);
    assert!(d.symbols_added.is_empty() && d.symbols_body_changed.is_empty());
    assert!(d.reresolved.contains(&"src/index.ts".to_string()));
    assert!(has_ref(
        &ix,
        "src/index.ts",
        "@/orders/service",
        "unresolved"
    ));
    assert_cold(&ix, dir.path());
}

#[test]
fn an_unchanged_save_changes_nothing() {
    let (mut ix, dir) = indexed("ts-basic");
    let before = dump(ix.store());
    let d = ix.apply(&set(["src/db.ts", "src/missing.ts"])).unwrap();
    assert_eq!(d, Default::default());
    assert_eq!(dump(ix.store()), before);
    assert_cold(&ix, dir.path());
}

#[test]
fn ignored_paths_are_left_alone() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::write(dir.path().join("ignored/new.ts"), "export const y = 2;\n").unwrap();
    fs::write(dir.path().join("node_modules/left-pad/x.js"), "x();\n").unwrap();
    let d = ix
        .apply(&set(["ignored/new.ts", "node_modules/left-pad/x.js"]))
        .unwrap();
    assert_eq!(d, Default::default());
    assert_cold(&ix, dir.path());
}

#[test]
fn a_changed_ignore_file_rescans() {
    let (mut ix, dir) = indexed("ts-basic");
    let gitignore = dir.path().join(".gitignore");
    let text = fs::read_to_string(&gitignore).unwrap();
    fs::write(&gitignore, format!("{text}\nsrc/legacy.js\n")).unwrap();
    let d = ix.apply(&set([".gitignore"])).unwrap();
    assert_eq!(d.files_changed, vec![".gitignore"]);
    assert_eq!(d.files_removed, vec!["src/legacy.js"]);
    assert_cold(&ix, dir.path());

    // The new rules hold for later saves too.
    fs::write(dir.path().join("src/legacy.js"), "export const z = 3;\n").unwrap();
    let d = ix.apply(&set(["src/legacy.js"])).unwrap();
    assert_eq!(d, Default::default());
    assert_cold(&ix, dir.path());
}

#[test]
fn a_file_that_turns_binary_loses_its_facts_and_its_callers_edges() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::write(dir.path().join("src/orders/mail.ts"), b"\0\x01\x02").unwrap();
    let d = ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!(d.files_changed, vec!["src/orders/mail.ts"]);
    assert_eq!(d.symbols_removed.len(), 1);
    assert!(d.reresolved.contains(&"src/orders/service.ts".to_string()));
    assert_cold(&ix, dir.path());
}

#[test]
fn rescan_equals_a_cold_index_and_says_what_changed() {
    let (mut ix, dir) = indexed("ts-basic");
    edit(
        dir.path(),
        "src/orders/mail.ts",
        "return order;",
        "return { order };",
    );
    fs::remove_file(dir.path().join("src/legacy.js")).unwrap();
    fs::write(dir.path().join("src/extra.ts"), "export const x = 1;\n").unwrap();
    let d = ix.rescan().unwrap();
    assert_eq!(d.files_changed, vec!["src/extra.ts", "src/orders/mail.ts"]);
    assert_eq!(d.files_removed, vec!["src/legacy.js"]);
    assert_eq!(d.symbols_body_changed.len(), 1);
    assert_eq!(d.symbols_added.len(), 1);
    assert!(d.reresolved.contains(&"src/orders/service.ts".to_string()));
    assert_cold(&ix, dir.path());

    let d = ix.rescan().unwrap();
    assert_eq!(d, Default::default());
}

#[test]
fn a_fresh_indexer_rescans_on_its_first_apply() {
    let dir = common::copy_fixture("ts-basic");
    // An indexer that never walked does not know the walk rules, so it walks.
    let store = Store::open_in_memory().unwrap();
    let mut ix = Indexer::open(dir.path(), store).unwrap();
    let d = ix.apply(&set(["src/db.ts"])).unwrap();
    assert!(d.files_changed.len() > 10);
    assert_cold(&ix, dir.path());
}

#[test]
fn resolving_every_file_again_writes_only_what_changed() {
    let (mut ix, dir) = indexed("ts-basic");
    // Temp triggers record every write to the rows a resolve produces.
    let mut sql = String::from("CREATE TEMP TABLE writes(what TEXT);");
    for table in ["edge", "ref", "reverse_import"] {
        for op in ["INSERT", "DELETE", "UPDATE"] {
            sql.push_str(&format!(
                "CREATE TEMP TRIGGER w_{table}_{op} AFTER {op} ON main.{table}
                 BEGIN INSERT INTO temp.writes VALUES ('{op} {table}'); END;"
            ));
        }
    }
    ix.store().conn().execute_batch(&sql).unwrap();

    // A new file resolves every file again, but none of their rows change.
    fs::write(dir.path().join("src/extra.ts"), "export const x = 1;\n").unwrap();
    let d = ix.apply(&set(["src/extra.ts"])).unwrap();
    assert_eq!(d.reresolved.len(), 10);
    assert_eq!(scalar(ix.store(), "SELECT count(*) FROM temp.writes"), "0");
    assert_cold(&ix, dir.path());

    // A body edit writes nothing either.
    edit(
        dir.path(),
        "src/orders/mail.ts",
        "return order;",
        "return { order };",
    );
    ix.apply(&set(["src/orders/mail.ts"])).unwrap();
    assert_eq!(scalar(ix.store(), "SELECT count(*) FROM temp.writes"), "0");
}

#[test]
fn renaming_a_folder_moves_its_files() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::rename(dir.path().join("src/orders"), dir.path().join("src/shop")).unwrap();
    let d = ix.apply(&set(["src/orders", "src/shop"])).unwrap();
    assert_eq!(d.files_removed.len(), 4);
    assert_eq!(
        d.files_changed,
        vec![
            "src/shop/index.ts",
            "src/shop/mail.ts",
            "src/shop/service.test.ts",
            "src/shop/service.ts",
        ]
    );
    assert_eq!(dangling_edges(&ix), "0");
    assert_cold(&ix, dir.path());
}

#[test]
fn a_folder_copied_in_is_indexed() {
    let (mut ix, dir) = indexed("ts-basic");
    let new = dir.path().join("src/extra/deep");
    fs::create_dir_all(&new).unwrap();
    fs::write(
        dir.path().join("src/extra/a.ts"),
        "import { b } from \"./deep/b\";\nexport function a() {\n  b();\n}\n",
    )
    .unwrap();
    fs::write(new.join("b.ts"), "export function b() {}\n").unwrap();
    // A skip folder inside is not entered.
    fs::create_dir_all(dir.path().join("src/extra/node_modules")).unwrap();
    fs::write(dir.path().join("src/extra/node_modules/c.js"), "c();\n").unwrap();
    let d = ix.apply(&set(["src/extra"])).unwrap();
    assert_eq!(
        d.files_changed,
        vec!["src/extra/a.ts", "src/extra/deep/b.ts"]
    );
    assert!(has_edge(
        &ix,
        "calls",
        "src/extra/a.ts#a",
        "src/extra/deep/b.ts#b"
    ));
    assert_cold(&ix, dir.path());
}

#[test]
fn a_folder_with_its_own_ignore_file_rescans() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::create_dir_all(dir.path().join("src/extra")).unwrap();
    fs::write(dir.path().join("src/extra/.gitignore"), "hidden.ts\n").unwrap();
    fs::write(
        dir.path().join("src/extra/hidden.ts"),
        "export const h = 1;\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/extra/seen.ts"),
        "export const s = 1;\n",
    )
    .unwrap();
    let d = ix.apply(&set(["src/extra"])).unwrap();
    assert_eq!(
        d.files_changed,
        vec!["src/extra/.gitignore", "src/extra/seen.ts"]
    );
    assert_cold(&ix, dir.path());
}

#[test]
fn a_changed_git_exclude_rescans() {
    let (mut ix, dir) = indexed("ts-basic");
    fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    fs::write(dir.path().join(".git/info/exclude"), "src/legacy.js\n").unwrap();
    let d = ix.apply(&set([".git/info/exclude"])).unwrap();
    assert_eq!(d.files_removed, vec!["src/legacy.js"]);
    assert_cold(&ix, dir.path());
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_or_file_leaves_the_index_and_comes_back() {
    let (mut ix, dir) = indexed("ts-basic");
    let Some(orders) = common::lock(&dir.path().join("src/orders")) else {
        return;
    };
    let Some(db) = common::lock(&dir.path().join("src/db.ts")) else {
        return;
    };
    let changed = set(["src/orders", "src/db.ts"]);
    let d = ix.apply(&changed).unwrap();
    assert_eq!(d.files_removed.len(), 4);
    assert_eq!(d.files_changed, vec!["src/db.ts"]);
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT parse_status FROM file WHERE path = 'src/db.ts'"
        ),
        "unreadable"
    );
    assert_eq!(dangling_edges(&ix), "0");
    assert_cold(&ix, dir.path());
    assert_eq!(ix.rescan().unwrap(), Default::default());

    drop((orders, db));
    let d = ix.apply(&changed).unwrap();
    assert_eq!(d.files_changed.len(), 5);
    assert_cold(&ix, dir.path());
}

#[test]
fn a_vanished_folder_with_an_ignore_file_takes_its_rules_along() {
    let (mut ix, dir) = indexed("ts-basic");
    let extra = dir.path().join("src/extra");
    fs::create_dir_all(&extra).unwrap();
    fs::write(extra.join(".gitignore"), "hidden.ts\n").unwrap();
    fs::write(extra.join("seen.ts"), "export const s = 1;\n").unwrap();
    ix.apply(&set(["src/extra"])).unwrap();
    assert_cold(&ix, dir.path());

    fs::remove_dir_all(&extra).unwrap();
    let d = ix.apply(&set(["src/extra"])).unwrap();
    assert_eq!(
        d.files_removed,
        vec!["src/extra/.gitignore", "src/extra/seen.ts"]
    );
    assert_cold(&ix, dir.path());

    // The folder comes back without its ignore file: nothing hides hidden.ts now.
    fs::create_dir_all(&extra).unwrap();
    fs::write(extra.join("hidden.ts"), "export const h = 1;\n").unwrap();
    let d = ix.apply(&set(["src/extra/hidden.ts"])).unwrap();
    assert_eq!(d.files_changed, vec!["src/extra/hidden.ts"]);
    assert_cold(&ix, dir.path());
}

/// Whether names in the temporary folder match without regard to case, as on
/// macOS and Windows by default.
fn case_insensitive() -> bool {
    let probe = tempfile::tempdir().unwrap();
    fs::write(probe.path().join("probe"), "").unwrap();
    probe.path().join("PROBE").exists()
}

#[test]
fn a_case_only_rename_keeps_only_the_new_name() {
    if !case_insensitive() {
        eprintln!("skipped: the file system tells names apart by case");
        return;
    }
    let (mut ix, dir) = indexed("ts-basic");
    fs::rename(dir.path().join("src/db.ts"), dir.path().join("src/DB.ts")).unwrap();
    let d = ix.apply(&set(["src/DB.ts", "src/db.ts"])).unwrap();
    assert_eq!(d.files_removed, vec!["src/db.ts"]);
    assert_eq!(d.files_changed, vec!["src/DB.ts"]);
    // `../db` from service.ts no longer names a file.
    assert!(has_ref(&ix, "src/orders/service.ts", "../db", "unresolved"));
    assert_cold(&ix, dir.path());

    // A folder: the files under the old name go.
    fs::rename(dir.path().join("src/orders"), dir.path().join("src/Orders")).unwrap();
    let d = ix.apply(&set(["src/Orders", "src/orders"])).unwrap();
    assert_eq!(d.files_removed.len(), 4);
    assert_eq!(d.files_changed.len(), 4);
    assert_cold(&ix, dir.path());
}

/// Records every insert, update and delete on the tables `derive` writes, and on
/// `edge`, into `temp.writes`.
fn record_derived_writes(ix: &Indexer) {
    let mut sql = String::from("CREATE TEMP TABLE writes(what TEXT);");
    for table in [
        "route",
        "client_call",
        "manifest",
        "dependency",
        "pattern",
        "gap",
        "edge",
    ] {
        for op in ["INSERT", "DELETE", "UPDATE"] {
            sql.push_str(&format!(
                "CREATE TEMP TRIGGER w_{table}_{op} AFTER {op} ON main.{table}
                 BEGIN INSERT INTO temp.writes VALUES ('{op} {table}'); END;"
            ));
        }
    }
    ix.store().conn().execute_batch(&sql).unwrap();
}

/// The route ids declared by the file at `rel`, sorted.
fn route_ids(ix: &Indexer, rel: &str) -> Vec<String> {
    let mut stmt = ix
        .store()
        .conn()
        .prepare("SELECT id FROM route WHERE file_id = ?1 ORDER BY id")
        .unwrap();
    stmt.query_map([file_id(rel)], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// `METHOD pattern` of every stored route, sorted.
fn routes(ix: &Indexer) -> Vec<String> {
    let mut stmt = ix
        .store()
        .conn()
        .prepare("SELECT method || ' ' || pattern FROM route ORDER BY 1")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn deleting_a_route_file_drops_its_routes_and_hops() {
    let (mut ix, dir) = indexed("client-hops");
    let rel = "app/api/orders/[id]/route.ts";
    let ids = route_ids(&ix, rel);
    assert_eq!(ids.len(), 2, "GET and DELETE /api/orders/{{id}}");
    // `removeOrder` calls `DELETE /api/orders/{id}`: there is an edge to lose.
    let into = |ix: &Indexer| {
        scalar(
            ix.store(),
            &format!(
                "SELECT count(*) FROM edge WHERE kind = 'http_calls' AND dst_id IN ('{}')",
                ids.join("', '")
            ),
        )
    };
    assert_ne!(into(&ix), "0");

    fs::remove_file(dir.path().join(rel)).unwrap();
    let d = ix.apply(&set([rel])).unwrap();
    assert_eq!(d.files_removed, vec![rel]);
    assert_eq!(d.routes_removed, ids);
    assert!(d.routes_added.is_empty(), "{:?}", d.routes_added);
    assert_eq!(routes(&ix), vec!["GET /api/orders", "POST /api/orders"]);
    for id in &ids {
        assert_eq!(
            scalar(
                ix.store(),
                &format!("SELECT count(*) FROM edge WHERE dst_id = '{id}' OR src_id = '{id}'")
            ),
            "0"
        );
    }
    assert_eq!(
        stored_derived(ix.store()),
        derived_lines(&derive_tree(dir.path()))
    );
    assert_cold(&ix, dir.path());

    // Putting it back brings the same ids back.
    fs::create_dir_all(dir.path().join("app/api/orders/[id]")).unwrap();
    fs::write(
        dir.path().join(rel),
        common::read_fixture(&format!("client-hops/{rel}")),
    )
    .unwrap();
    let d = ix.apply(&set([rel])).unwrap();
    assert_eq!(d.routes_added, ids);
    assert!(d.routes_removed.is_empty());
    assert_ne!(into(&ix), "0");
    assert_cold(&ix, dir.path());
}

#[test]
fn a_route_another_file_also_declares_is_neither_added_nor_removed() {
    let (mut ix, dir) = indexed("client-hops");
    // A route group gives `GET /api/orders` again; the first in path order,
    // `app/(shop)/…`, is kept and the original's is a note.
    let group = "app/(shop)/api/orders/route.ts";
    let rel = "app/api/orders/route.ts";
    let before = route_ids(&ix, rel);
    assert_eq!(before.len(), 2, "GET and POST /api/orders");
    fs::create_dir_all(dir.path().join("app/(shop)/api/orders")).unwrap();
    fs::write(
        dir.path().join(group),
        "export async function GET() {\n  return Response.json([])\n}\n",
    )
    .unwrap();
    let d = ix.apply(&set([group])).unwrap();
    assert!(
        d.routes_added.is_empty() && d.routes_removed.is_empty(),
        "{d:?}"
    );
    let moved = route_ids(&ix, group);
    assert_eq!(moved.len(), 1);
    assert!(before.contains(&moved[0]));

    // Deleting the group's file takes its row along; derive brings `GET /api/orders`
    // back from the original file with the same id: stored before and after, it is
    // neither added nor removed.
    fs::remove_file(dir.path().join(group)).unwrap();
    let d = ix.apply(&set([group])).unwrap();
    assert_eq!(route_ids(&ix, rel), before);
    assert!(d.routes_added.is_empty(), "{:?}", d.routes_added);
    assert!(d.routes_removed.is_empty(), "{:?}", d.routes_removed);
    assert_cold(&ix, dir.path());
}

#[test]
fn a_body_edit_keeps_routes_and_writes_nothing_derived() {
    let (mut ix, dir) = indexed("client-hops");
    record_derived_writes(&ix);
    // A statement on the declaration line: the body changes, no line moves.
    edit(
        dir.path(),
        "lib/api.ts",
        "export async function loadOrders() {",
        "export async function loadOrders() { void 0;",
    );
    let d = ix.apply(&set(["lib/api.ts"])).unwrap();
    assert_eq!(d.symbols_body_changed.len(), 1);
    assert!(d.routes_added.is_empty() && d.routes_removed.is_empty());
    assert_eq!((d.edges_added, d.edges_removed), (0, 0));
    assert_eq!(scalar(ix.store(), "SELECT count(*) FROM temp.writes"), "0");
    assert_cold(&ix, dir.path());
}

#[test]
fn breaking_a_route_file_keeps_the_verbs_that_parse() {
    let (mut ix, dir) = indexed("client-hops");
    let rel = "app/api/orders/route.ts";
    let ids = route_ids(&ix, rel);
    let path = dir.path().join(rel);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}\nexport async function PUT( {{\n")).unwrap();
    let d = ix.apply(&set([rel])).unwrap();
    assert_eq!(
        scalar(
            ix.store(),
            &format!("SELECT parse_status FROM file WHERE path = '{rel}'")
        ),
        "parsed_with_errors"
    );
    assert!(d.routes_added.is_empty() && d.routes_removed.is_empty());
    assert_eq!(route_ids(&ix, rel), ids);
    assert!(routes(&ix).contains(&"GET /api/orders".to_string()));
    assert!(routes(&ix).contains(&"POST /api/orders".to_string()));
    assert!(!routes(&ix).iter().any(|r| r.starts_with("PUT ")));
    assert_cold(&ix, dir.path());
}

#[test]
fn deleting_the_lockfile_unpins_the_versions() {
    let (mut ix, dir) = indexed("client-hops");
    fs::write(
        dir.path().join("package-lock.json"),
        r#"{ "lockfileVersion": 3, "packages": { "node_modules/axios": { "version": "1.7.9" } } }"#,
    )
    .unwrap();
    ix.apply(&set(["package-lock.json"])).unwrap();
    let axios = |ix: &Indexer| {
        scalar(
            ix.store(),
            "SELECT coalesce(resolved, '-') FROM dependency WHERE name = 'axios'",
        )
    };
    assert_eq!(axios(&ix), "1.7.9");
    fs::remove_file(dir.path().join("package-lock.json")).unwrap();
    ix.apply(&set(["package-lock.json"])).unwrap();
    assert_eq!(axios(&ix), "-");
    assert_eq!(
        scalar(ix.store(), "SELECT coalesce(lockfile, '-') FROM manifest"),
        "-"
    );
    assert_cold(&ix, dir.path());
}

#[test]
fn a_lockfile_too_large_to_parse_is_read_again_when_it_changes() {
    // Over the 1 MiB parse limit the walk skips it; only its size and mtime are stored.
    let lock = |version: &str| {
        let mut text = format!(
            "{{ \"lockfileVersion\": 3, \"packages\": {{ \"node_modules/axios\": {{ \"version\": \"{version}\" }} }} }}\n"
        );
        text.push_str(&" \n".repeat(600_000));
        text
    };
    let (mut ix, dir) = indexed("client-hops");
    let path = dir.path().join("package-lock.json");
    fs::write(&path, lock("1.7.8")).unwrap();
    ix.apply(&set(["package-lock.json"])).unwrap();
    let axios = |ix: &Indexer| {
        scalar(
            ix.store(),
            "SELECT coalesce(resolved, '-') FROM dependency WHERE name = 'axios'",
        )
    };
    assert_eq!(axios(&ix), "1.7.8");
    assert_eq!(
        scalar(
            ix.store(),
            "SELECT parse_status FROM file WHERE path = 'package-lock.json'"
        ),
        "too_large"
    );
    fs::write(&path, lock("1.7.9")).unwrap();
    ix.apply(&set(["package-lock.json"])).unwrap();
    assert_eq!(axios(&ix), "1.7.9");
    assert_cold(&ix, dir.path());
}

#[test]
fn a_package_json_in_a_new_folder_adds_its_manifest_and_entries() {
    let (mut ix, dir) = indexed("client-hops");
    fs::create_dir_all(dir.path().join("tools/cli")).unwrap();
    fs::write(
        dir.path().join("tools/cli/package.json"),
        r#"{ "name": "shop-cli", "bin": { "shop": "main.js" } }"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("tools/cli/main.js"),
        "console.log('shop')\n",
    )
    .unwrap();
    let d = ix.apply(&set(["tools"])).unwrap();
    assert_eq!(d.routes_added.len(), 1, "{d:?}");
    assert_eq!(scalar(ix.store(), "SELECT count(*) FROM manifest"), "2");
    assert!(
        routes(&ix).contains(&"BIN shop".to_string()),
        "{:?}",
        routes(&ix)
    );
    assert_cold(&ix, dir.path());
}

#[test]
fn editing_package_json_updates_dependencies() {
    let (mut ix, dir) = indexed("client-hops");
    let declared = |ix: &Indexer| {
        scalar(
            ix.store(),
            "SELECT declared || ' ' || coalesce(resolved, '-') FROM dependency WHERE name = 'axios'",
        )
    };
    assert_eq!(declared(&ix), "1.7.0 -");
    edit(
        dir.path(),
        "package.json",
        "\"axios\": \"1.7.0\"",
        "\"axios\": \"^1.8.0\"",
    );
    ix.apply(&set(["package.json"])).unwrap();
    assert_eq!(declared(&ix), "^1.8.0 -");
    assert_cold(&ix, dir.path());

    // A new lockfile pins the version.
    fs::write(
        dir.path().join("package-lock.json"),
        r#"{
  "name": "client-hops",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "client-hops" },
    "node_modules/axios": { "version": "1.8.4" }
  }
}
"#,
    )
    .unwrap();
    ix.apply(&set(["package-lock.json"])).unwrap();
    assert_eq!(declared(&ix), "^1.8.0 1.8.4");
    assert_eq!(
        scalar(ix.store(), "SELECT lockfile FROM manifest"),
        "package-lock.json"
    );
    assert_cold(&ix, dir.path());

    // Dropping `next` takes the Next.js routes away; adding it back returns them.
    edit(dir.path(), "package.json", "\"next\": \"15.2.0\",", "");
    let d = ix.apply(&set(["package.json"])).unwrap();
    assert_eq!(d.routes_removed.len(), 4);
    assert!(routes(&ix).is_empty());
    assert_cold(&ix, dir.path());
    edit(
        dir.path(),
        "package.json",
        "\"axios\"",
        "\"next\": \"15.2.0\",\n    \"axios\"",
    );
    let d = ix.apply(&set(["package.json"])).unwrap();
    assert_eq!(d.routes_added.len(), 4);
    assert_cold(&ix, dir.path());
}
