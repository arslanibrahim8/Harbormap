//! Routes, entries, client calls, manifests and patterns on pinned public repositories
//! (Learn spec §5, §6.3, §7.3 and §11.1). They are `#[ignore]`d because they need the
//! trees that `scripts/eval/fetch.sh` fetches; point `HARBORMAP_EVAL_DIR` at the folder
//! it filled and run `cargo test -p hm-index --test eval_l2a -- --ignored`. The fetched
//! trees are only read, never built or run.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use hm_core::ids::file_id;
use hm_index::facts::FileFacts;
use hm_index::index::{IndexReport, Indexer};
use hm_index::query::{self, DependencyRow, ManifestRow, RouteRow, display_name};
use hm_index::ts::resolve::symbol_ids;

/// What one indexed repository holds, with ids mapped back to readable names: a file
/// as `path`, a symbol as `path#Owner.name`, a route as `METHOD pattern`.
struct Indexed {
    report: IndexReport,
    routes: Vec<RouteRow>,
    manifests: Vec<ManifestRow>,
    dependencies: Vec<DependencyRow>,
    /// (kind, source, destination, rule).
    edges: BTreeSet<(String, String, String, String)>,
    /// (source, method, url, rule).
    client_calls: BTreeSet<(String, String, String, String)>,
    /// (file, kind, rule).
    patterns: BTreeSet<(String, String, String)>,
    /// Ref statuses seen.
    ref_statuses: BTreeSet<String>,
}

impl Indexed {
    fn has_edge(&self, kind: &str, src: &str, dst: &str, rule: &str) -> bool {
        self.edges
            .contains(&(kind.into(), src.into(), dst.into(), rule.into()))
    }

    /// `(entry kind, method, pattern)` of every route, sorted.
    fn route_keys(&self) -> BTreeSet<(String, String, String)> {
        self.routes
            .iter()
            .map(|r| (r.entry_kind.clone(), r.method.clone(), r.pattern.clone()))
            .collect()
    }

    /// The patterns of `kind` with `rule`.
    fn patterns_of(&self, kind: &str, rule: &str) -> BTreeSet<&str> {
        self.patterns
            .iter()
            .filter(|(_, k, r)| k == kind && r == rule)
            .map(|(file, _, _)| file.as_str())
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

/// A cold pass over `root` into an in-memory store, read back through the queries and
/// SQL.
fn index(root: &Path) -> Indexed {
    let store = hm_store::Store::open_in_memory().expect("open a store");
    let mut ix = Indexer::open(root, store).expect("open the indexer");
    let report = ix.full().expect("index the repository");
    eprintln!("{}: {report:?}", root.display());

    let store = ix.store();
    let routes = query::routes(store).expect("routes");
    let manifests = query::manifests(store).expect("manifests");
    let dependencies = query::dependencies(store, None).expect("dependencies");
    let conn = store.conn();
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
    for row in rows("SELECT path, facts_json FROM file") {
        let rel = &row[0];
        names.insert(file_id(rel), display_name(rel, None));
        paths.insert(file_id(rel), rel.clone());
        if !row[1].is_empty() {
            let facts: FileFacts = serde_json::from_str(&row[1]).expect("facts_json reads");
            for (id, sym) in symbol_ids(rel, &facts).into_iter().zip(&facts.symbols) {
                names.insert(id, display_name(rel, Some(sym)));
            }
        }
    }
    for r in &routes {
        names.insert(r.id.clone(), format!("{} {}", r.method, r.pattern));
    }
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));
    let path_of = |id: &str| paths.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let edges = rows("SELECT kind, src_id, dst_id, rule FROM edge")
        .iter()
        .map(|r| (r[0].clone(), name_of(&r[1]), name_of(&r[2]), r[3].clone()))
        .collect();
    let client_calls = rows("SELECT src_id, method, url, rule FROM client_call")
        .iter()
        .map(|r| (name_of(&r[0]), r[1].clone(), r[2].clone(), r[3].clone()))
        .collect();
    let patterns = rows("SELECT file_id, kind, rule FROM pattern")
        .iter()
        .map(|r| (path_of(&r[0]), r[1].clone(), r[2].clone()))
        .collect();
    let ref_statuses = rows("SELECT DISTINCT status FROM ref")
        .into_iter()
        .map(|mut r| r.swap_remove(0))
        .collect();
    Indexed {
        report,
        routes,
        manifests,
        dependencies,
        edges,
        client_calls,
        patterns,
        ref_statuses,
    }
}

fn key(kind: &str, method: &str, pattern: &str) -> (String, String, String) {
    (kind.into(), method.into(), pattern.into())
}

#[test]
#[ignore = "needs HARBORMAP_EVAL_DIR (scripts/eval/fetch.sh)"]
fn next_learn_routes_and_entries_are_found() {
    let Some(root) = eval_root("next-learn", "dashboard/final-example") else {
        return;
    };
    let got = index(&root);

    // Every page, HTTP route, server action, middleware and entry, and nothing else.
    // `app/dashboard/(overview)/page.tsx` is `/dashboard`: a route group adds no segment.
    let actions = "app/lib/actions.ts";
    let expected: BTreeSet<_> = [
        key("page", "GET", "/"),
        key("page", "GET", "/dashboard"),
        key("page", "GET", "/dashboard/customers"),
        key("page", "GET", "/dashboard/invoices"),
        key("page", "GET", "/dashboard/invoices/create"),
        key("page", "GET", "/dashboard/invoices/{id}/edit"),
        key("page", "GET", "/login"),
        key("http", "GET", "/query"),
        key("http", "GET", "/seed"),
        key(
            "server_action",
            "ACTION",
            &format!("{actions}#authenticate"),
        ),
        key(
            "server_action",
            "ACTION",
            &format!("{actions}#createInvoice"),
        ),
        key(
            "server_action",
            "ACTION",
            &format!("{actions}#deleteInvoice"),
        ),
        key(
            "server_action",
            "ACTION",
            &format!("{actions}#updateInvoice"),
        ),
        // The sign-out form's inline `async () => { 'use server'; … }`, owned by `SideNav`.
        key(
            "server_action",
            "ACTION",
            "app/ui/dashboard/sidenav.tsx#SideNav›anonymous",
        ),
        // The matcher's string value: the source spells `\\.png`.
        key(
            "middleware",
            "ANY",
            r"/((?!api|_next/static|_next/image|.*\.png$).*)",
        ),
        key("main", "START", "start"),
    ]
    .into();
    assert_eq!(got.route_keys(), expected, "{:#?}", got.routes);
    // No route is declared twice and every config file reads.
    assert!(got.report.notes.is_empty(), "{:#?}", got.report.notes);

    // Every route sits under the repository root, so ids never collide with another app.
    assert!(got.routes.iter().all(|r| r.root.is_empty()));

    // `proxy.ts` (Next.js 16's name for `middleware.ts`) default-exports an expression
    // (`NextAuth(authConfig).auth`): the handler is the file itself. Its matcher is a
    // lookahead regex, which is not evaluated, so it applies to every route of the app.
    let proxy = got
        .routes
        .iter()
        .find(|r| r.entry_kind == "middleware")
        .expect("one middleware");
    assert_eq!(proxy.file, "proxy.ts");
    assert_eq!(
        proxy.handler_id.as_deref(),
        Some(file_id("proxy.ts").as_str())
    );
    assert_eq!(proxy.handler.as_deref(), Some("proxy.ts"));
    assert_eq!(proxy.rule, "ts.next.middleware");
    assert_eq!(
        (proxy.prov.as_str(), proxy.conf.as_str()),
        ("matched", "probably")
    );
    let detail: serde_json::Value =
        serde_json::from_str(proxy.detail.as_deref().expect("a detail")).expect("JSON detail");
    assert_eq!(
        detail["evaluated"],
        serde_json::Value::Bool(false),
        "{detail}"
    );
    assert_eq!(proxy.label, "Runs before many routes");
    // On the way in: an edge from every page and HTTP route to the middleware file.
    for r in got
        .routes
        .iter()
        .filter(|r| r.entry_kind == "page" || r.entry_kind == "http")
    {
        let src = format!("{} {}", r.method, r.pattern);
        assert!(
            got.has_edge("calls", &src, "proxy.ts", "ts.next.middleware"),
            "no middleware edge from {src}: {:#?}",
            got.edges
        );
    }

    // The handlers: page and route components, the action functions themselves.
    let handler = |kind: &str, pattern: &str| {
        got.routes
            .iter()
            .find(|r| r.entry_kind == kind && r.pattern == pattern)
            .and_then(|r| r.handler.clone())
    };
    assert_eq!(
        handler("page", "/dashboard/invoices/{id}/edit").as_deref(),
        Some("app/dashboard/invoices/[id]/edit/page.tsx#Page")
    );
    assert_eq!(
        handler("http", "/seed").as_deref(),
        Some("app/seed/route.ts#GET")
    );
    let action = format!("{actions}#createInvoice");
    assert_eq!(
        handler("server_action", &action).as_deref(),
        Some(action.as_str())
    );

    // Forms reach their server actions through `useActionState` and `<form action>`;
    // `updateInvoice.bind(null, id)` and `deleteInvoice.bind(null, id)` keep the target.
    for (kind, src, action, rule) in [
        (
            "calls",
            "app/ui/login-form.tsx#LoginForm",
            "authenticate",
            "ts.next.action.state",
        ),
        (
            "calls",
            "app/ui/invoices/create-form.tsx#Form",
            "createInvoice",
            "ts.next.action.state",
        ),
        (
            "calls",
            "app/ui/invoices/edit-form.tsx#EditInvoiceForm",
            "updateInvoice",
            "ts.next.action.state",
        ),
        (
            "calls",
            "app/ui/invoices/buttons.tsx#DeleteInvoice",
            "deleteInvoice",
            "ts.next.action.form",
        ),
    ] {
        let dst = format!("{actions}#{action}");
        assert!(
            got.has_edge(kind, src, &dst, rule),
            "no {rule} edge {src} -> {dst}: {:#?}",
            got.edges
                .iter()
                .filter(|(_, s, _, _)| s == src)
                .collect::<Vec<_>>()
        );
    }

    // `next` is declared `latest` and pinned by the pnpm lockfile.
    assert_eq!(got.manifests.len(), 1, "{:#?}", got.manifests);
    assert_eq!(got.manifests[0].path, "package.json");
    assert_eq!(got.manifests[0].lockfile.as_deref(), Some("pnpm-lock.yaml"));
    let next = got
        .dependencies
        .iter()
        .find(|d| d.name == "next")
        .expect("next is a dependency");
    assert_eq!(next.declared, "latest");
    assert_eq!(next.resolved.as_deref(), Some("16.0.10"));
    assert!(!next.dev);

    // bcrypt in `auth.ts`, zod in both files that validate input.
    let hashes = got.patterns_of("password_hash", "ts.pattern.bcrypt.hash");
    assert!(hashes.contains("auth.ts"), "{:#?}", got.patterns);
    let validations = got.patterns_of("validation", "ts.pattern.zod.parse");
    assert!(
        validations.contains(actions) && validations.contains("auth.ts"),
        "{:#?}",
        got.patterns
    );
}

#[test]
#[ignore = "needs HARBORMAP_EVAL_DIR (scripts/eval/fetch.sh)"]
fn fastapi_frontend_client_calls_are_found() {
    let Some(root) = eval_root("fastapi-template", "frontend") else {
        return;
    };
    let got = index(&root);

    // TanStack Router file routes are not a route rule; the backend is another folder.
    assert!(got.routes.is_empty(), "{:#?}", got.routes);

    // The generated SDK calls `(options.client ?? client).post({ url: … })`: a request
    // helper whatever the receiver, stored though nothing in this folder matches it.
    let login = (
        "src/client/sdk.gen.ts#LoginService.loginAccessToken".to_string(),
        "POST".to_string(),
        "/api/v1/login/access-token".to_string(),
        "ts.client.request_helper".to_string(),
    );
    assert!(got.client_calls.contains(&login), "{:#?}", got.client_calls);
    assert!(
        got.client_calls.len() >= 20,
        "{} client calls: {:#?}",
        got.client_calls.len(),
        got.client_calls
    );
    // Every one is a method of the generated SDK, with its verb read from the callee.
    assert!(
        got.client_calls.iter().all(|(src, method, _, rule)| {
            src.starts_with("src/client/sdk.gen.ts#")
                && !method.is_empty()
                && rule == "ts.client.request_helper"
        }),
        "{:#?}",
        got.client_calls
    );
    assert_eq!(got.report.client_calls_matched, 0);
    assert!(!got.edges.iter().any(|(kind, ..)| kind == "http_calls"));

    // No lockfile in the subfolder: declared versions only.
    assert_eq!(got.manifests.len(), 1, "{:#?}", got.manifests);
    assert_eq!(got.manifests[0].lockfile, None);
    let axios = got
        .dependencies
        .iter()
        .find(|d| d.name == "axios")
        .expect("axios is a dependency");
    assert_eq!(axios.declared, "1.20.0");
    assert_eq!(axios.resolved, None);

    // Globals and local names are told apart from unresolved module-level names.
    assert!(
        got.ref_statuses.contains("builtin") && got.ref_statuses.contains("local"),
        "{:?}",
        got.ref_statuses
    );
}
