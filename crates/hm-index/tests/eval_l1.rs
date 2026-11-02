//! Smoke tests on pinned public TypeScript repositories (spec §11.1 and §15). They
//! are `#[ignore]`d because they need the trees that `scripts/eval/fetch.sh` fetches;
//! point `HARBORMAP_EVAL_DIR` at the folder it filled and run
//! `cargo test -p hm-index --test eval_l1 -- --ignored`. The fetched trees are only
//! read, never built or run.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use hm_core::ids::file_id;
use hm_index::facts::FileFacts;
use hm_index::index::{IndexReport, Indexer};
use hm_index::query::display_name;
use hm_index::ts::resolve::symbol_ids;

/// What one indexed repository holds, with ids mapped back to readable names.
struct Indexed {
    report: IndexReport,
    /// (kind, source, destination), as `path` or `path#Owner.name`.
    edges: BTreeSet<(String, String, String)>,
    /// (file, kind, text, status).
    refs: BTreeSet<(String, String, String, String)>,
    /// path -> (class, parse_status).
    files: HashMap<String, (String, String)>,
}

impl Indexed {
    fn has_edge(&self, kind: &str, src: &str, dst: &str) -> bool {
        self.edges
            .contains(&(kind.to_string(), src.to_string(), dst.to_string()))
    }

    /// The files whose parse status is not a clean `parsed`.
    fn unclean(&self) -> Vec<(&String, &(String, String))> {
        self.files
            .iter()
            .filter(|(_, (_, status))| status == "parsed_with_errors")
            .collect()
    }
}

/// `<HARBORMAP_EVAL_DIR>/<repo>/<subdir>`, or `None` (with a message) when the
/// variable is unset, so the test skips.
fn eval_root(repo: &str, subdir: &str) -> Option<PathBuf> {
    let Some(dir) = std::env::var_os("HARBORMAP_EVAL_DIR") else {
        eprintln!("skipped: HARBORMAP_EVAL_DIR is not set (run scripts/eval/fetch.sh first)");
        return None;
    };
    let root = PathBuf::from(dir).join(repo).join(subdir);
    assert!(root.is_dir(), "{} is not a folder", root.display());
    Some(root)
}

/// A cold pass over `root` into an in-memory store, read back through SQL.
fn index(root: &Path) -> Indexed {
    let store = hm_store::Store::open_in_memory().expect("open a store");
    let mut ix = Indexer::open(root, store).expect("open the indexer");
    let report = ix.full().expect("index the repository");
    eprintln!("{}: {report:?}", root.display());

    let conn = ix.store().conn();
    let rows = |sql: &str| -> Vec<Vec<String>> {
        let mut stmt = conn.prepare(sql).expect("prepare");
        let n = stmt.column_count();
        stmt.query_map([], |row| {
            (0..n)
                .map(|i| Ok(row.get::<_, Option<String>>(i)?.unwrap_or_default()))
                .collect::<Result<Vec<_>, _>>()
        })
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
    };

    let mut names: HashMap<String, String> = HashMap::new();
    let mut paths: HashMap<String, String> = HashMap::new();
    let mut files = HashMap::new();
    for row in rows("SELECT path, class, parse_status, facts_json FROM file") {
        let rel = &row[0];
        names.insert(file_id(rel), display_name(rel, None));
        paths.insert(file_id(rel), rel.clone());
        if !row[3].is_empty() {
            let facts: FileFacts = serde_json::from_str(&row[3]).expect("facts_json reads");
            for (id, sym) in symbol_ids(rel, &facts).into_iter().zip(&facts.symbols) {
                names.insert(id, display_name(rel, Some(sym)));
            }
        }
        files.insert(rel.clone(), (row[1].clone(), row[2].clone()));
    }
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));
    let path_of = |id: &str| paths.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let edges = rows("SELECT kind, src_id, dst_id FROM edge")
        .iter()
        .map(|r| (r[0].clone(), name_of(&r[1]), name_of(&r[2])))
        .collect();
    let refs = rows("SELECT file_id, kind, text, status FROM ref")
        .iter()
        .map(|r| (path_of(&r[0]), r[1].clone(), r[2].clone(), r[3].clone()))
        .collect();
    Indexed {
        report,
        edges,
        refs,
        files,
    }
}

#[test]
#[ignore = "needs HARBORMAP_EVAL_DIR (scripts/eval/fetch.sh)"]
fn next_learn_parses_cleanly_and_resolves() {
    let Some(root) = eval_root("next-learn", "dashboard/final-example") else {
        return;
    };
    let got = index(&root);
    assert!(got.report.parsed > 0, "nothing was parsed");
    // tree-sitter-typescript 0.23.2 (the latest release) cannot parse a tagged template
    // with type arguments: the grammar reports a MISSING `!` after `sql<User[]>` in
    // auth.ts line 13 (`await sql<User[]>`SELECT ...``) and in app/lib/data.ts lines
    // 22, 35, 96, 147, 172 and 189. The facts of both files are still extracted.
    // Exactly these two files are unclean: any other file must parse without errors,
    // and a grammar fix that cleans one of them must remove it from this list.
    let known: BTreeSet<&str> = ["auth.ts", "app/lib/data.ts"].into();
    let unclean: BTreeSet<&str> = got.unclean().iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        unclean,
        known,
        "files parsed with errors: {:?}",
        got.unclean()
    );
    assert_eq!(got.report.parsed_with_errors as usize, unclean.len());

    let page = "app/dashboard/invoices/create/page.tsx";
    let form = "app/ui/invoices/create-form.tsx";
    assert!(
        got.has_edge("imports", page, "app/lib/data.ts"),
        "{:#?}",
        got.edges
    );
    assert!(got.has_edge("imports", page, form));
    assert!(got.has_edge(
        "calls",
        &format!("{page}#Page"),
        "app/lib/data.ts#fetchCustomers"
    ));
    assert!(got.has_edge("imports", form, "app/lib/actions.ts"));
    assert!(
        got.refs.contains(&(
            form.to_string(),
            "import".to_string(),
            "next".to_string(),
            "external".to_string()
        )),
        "{:#?}",
        got.refs
    );
}

#[test]
#[ignore = "needs HARBORMAP_EVAL_DIR (scripts/eval/fetch.sh)"]
fn fastapi_frontend_parses_and_follows_the_barrel() {
    let Some(root) = eval_root("fastapi-template", "frontend") else {
        return;
    };
    let got = index(&root);
    assert!(got.report.parsed > 0, "nothing was parsed");
    assert_eq!(
        got.report.parsed_with_errors,
        0,
        "files parsed with errors: {:?}",
        got.unclean()
    );

    let hook = "src/hooks/useAuth.ts";
    assert!(
        got.has_edge("imports", hook, "src/client/index.ts"),
        "{:#?}",
        got.edges
    );
    assert!(got.has_edge("imports", hook, "src/utils.ts"));
    assert!(got.has_edge("imports", hook, "src/hooks/useCustomToast.ts"));

    // `LoginService` comes in through the named re-export in `src/client/index.ts`.
    let target = "src/client/sdk.gen.ts#LoginService.loginAccessToken";
    let prefix = format!("{hook}#");
    assert!(
        got.edges
            .iter()
            .any(|(kind, src, dst)| kind == "calls" && src.starts_with(&prefix) && dst == target),
        "no call from {hook} to {target}: {:#?}",
        got.edges
            .iter()
            .filter(|(_, src, _)| src.starts_with(&prefix))
            .collect::<Vec<_>>()
    );

    // The generated client is parsed like any other file and carries its class.
    let (class, status) = got
        .files
        .get("src/client/sdk.gen.ts")
        .expect("sdk.gen.ts is indexed");
    assert_eq!(class, "generated");
    assert_eq!(status, "parsed");
}
