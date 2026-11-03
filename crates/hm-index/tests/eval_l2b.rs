//! Smoke tests on pinned public Go and Python repositories (spec §11.1 and §15). They
//! are `#[ignore]`d because they need the trees that `scripts/eval/fetch.sh` fetches;
//! point `HARBORMAP_EVAL_DIR` at the folder it filled and run
//! `cargo test -p hm-index --test eval_l2b -- --ignored --nocapture`. The fetched trees
//! are only read, never built or run.

use std::collections::{BTreeMap, BTreeSet, HashMap};
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

    /// Whether `refs` holds exactly this (file, kind, text, status).
    fn has_ref(&self, file: &str, kind: &str, text: &str, status: &str) -> bool {
        self.refs.contains(&(
            file.to_string(),
            kind.to_string(),
            text.to_string(),
            status.to_string(),
        ))
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

/// Prints the counts the plan records: symbols, edges by kind, refs by status, time.
fn print_report(name: &str, got: &Indexed) {
    let mut edges: BTreeMap<&str, usize> = BTreeMap::new();
    for (kind, _, _) in &got.edges {
        *edges.entry(kind).or_default() += 1;
    }
    let mut refs: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, _, status) in &got.refs {
        *refs.entry(status).or_default() += 1;
    }
    eprintln!(
        "{name}: files={} symbols={} edges_by_kind={edges:?} refs_by_status={refs:?} took_ms={}",
        got.files.len(),
        got.report.symbols,
        got.report.took_ms
    );
}

#[test]
#[ignore = "needs HARBORMAP_EVAL_DIR (scripts/eval/fetch.sh)"]
fn wtf_indexes_and_resolves() {
    let Some(root) = eval_root("wtf", "") else {
        return;
    };
    let got = index(&root);
    print_report("wtf", &got);
    assert!(got.report.parsed > 0, "nothing was parsed");
    // tree-sitter-go parses every file of this tree without an
    // ERROR or MISSING node, so the set is asserted empty: a grammar change that
    // breaks one file must show up here, with its file name.
    let known: BTreeSet<&str> = BTreeSet::new();
    let unclean: BTreeSet<&str> = got.unclean().iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        unclean,
        known,
        "files parsed with errors: {:?}",
        got.unclean()
    );
    assert_eq!(got.report.parsed_with_errors as usize, unclean.len());

    let main = "cmd/wtfd/main.go";
    assert!(
        got.has_edge("imports", main, "sqlite/dial.go"),
        "{:#?}",
        got.edges
    );
    let run = format!("{main}#Main.Run");
    assert!(
        got.has_edge("calls", &run, "sqlite/dial.go#NewDialService"),
        "{:#?}",
        got.edges
            .iter()
            .filter(|(_, s, _)| *s == run)
            .collect::<Vec<_>>()
    );
    // The import's name is the package clause `http`, not the folder's path.
    assert!(got.has_edge("calls", &run, "http/server.go#ListenAndServeDebug"));
    assert!(got.has_edge("calls", &run, &format!("{main}#expandDSN")));

    // `DialService` has three implementations (sqlite, http, mock): the call stays a ref.
    assert!(
        got.has_ref(
            "http/dial.go",
            "call",
            "s.DialService.FindDialByID",
            "unresolved"
        ),
        "{:#?}",
        got.refs
            .iter()
            .filter(|r| r.0 == "http/dial.go")
            .collect::<Vec<_>>()
    );
    assert!(
        got.has_ref(main, "import", "github.com/pelletier/go-toml", "external"),
        "{:#?}",
        got.refs.iter().filter(|r| r.0 == main).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "needs HARBORMAP_EVAL_DIR (scripts/eval/fetch.sh)"]
fn fastapi_backend_indexes_and_resolves() {
    let Some(root) = eval_root("fastapi-template", "backend") else {
        return;
    };
    let got = index(&root);
    print_report("fastapi-template/backend", &got);
    assert!(got.report.parsed > 0, "nothing was parsed");
    // tree-sitter-python parses every file of this tree without an
    // ERROR or MISSING node, so the set is asserted empty: a grammar change that
    // breaks one file must show up here, with its file name.
    let known: BTreeSet<&str> = BTreeSet::new();
    let unclean: BTreeSet<&str> = got.unclean().iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        unclean,
        known,
        "files parsed with errors: {:?}",
        got.unclean()
    );
    assert_eq!(got.report.parsed_with_errors as usize, unclean.len());

    let login = "app/api/routes/login.py";
    for dst in ["app/crud.py", "app/models.py", "app/api/deps.py"] {
        assert!(
            got.has_edge("imports", login, dst),
            "no import {login} -> {dst}: {:#?}",
            got.edges
                .iter()
                .filter(|(k, s, _)| k == "imports" && s == login)
                .collect::<Vec<_>>()
        );
    }
    // `from app.api.routes import items, login, ...` names a submodule.
    assert!(got.has_edge("imports", "app/api/main.py", login));

    let func = format!("{login}#login_access_token");
    for dst in [
        "app/crud.py#authenticate",
        "app/core/security.py#create_access_token",
        "app/models.py#Token",
    ] {
        assert!(
            got.has_edge("calls", &func, dst),
            "no call {func} -> {dst}: {:#?}",
            got.edges
                .iter()
                .filter(|(_, s, _)| *s == func)
                .collect::<Vec<_>>()
        );
    }
    assert!(
        got.has_ref(login, "import", "fastapi", "external"),
        "{:#?}",
        got.refs.iter().filter(|r| r.0 == login).collect::<Vec<_>>()
    );

    // The test fixtures are parsed like any other file; `tests/conftest.py` is classed `test`.
    let (class, _) = got
        .files
        .get("tests/conftest.py")
        .expect("tests/conftest.py is indexed");
    assert_eq!(class, "test");
}
