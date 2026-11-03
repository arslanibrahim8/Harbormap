//! Reading the facts back: readable names for files and symbols, the cards of Learn
//! spec §7, and the routes, manifests, dependencies and external imports of L2a.

use std::collections::{BTreeMap, BTreeSet};

use hm_core::ids::file_id;
use hm_store::Store;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::facts::SymbolFact;
use crate::index::Result;

/// The name shown for a file or a symbol: `path`, `path#name` or `path#Owner.name`.
pub fn display_name(rel: &str, sym: Option<&SymbolFact>) -> String {
    match sym {
        None => rel.to_string(),
        Some(s) => format!("{rel}#{}", qualified(&s.owner, &s.name)),
    }
}

// The cards of Learn spec §7.

/// What the index knows about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileCard {
    pub path: String,
    pub lang: Option<String>,
    pub class: String,
    pub parse_status: String,
    pub lines: u32,
    /// In source order; an enclosing symbol comes before the ones it holds.
    pub symbols: Vec<SymbolRow>,
    /// The files that import this one: source files before test files, then by path.
    pub used_by: Vec<String>,
    /// The files this one imports, then `pkg (external)` for each package, by name.
    pub uses: Vec<String>,
    /// References in this file that resolve to nothing.
    pub unresolved: u32,
    /// The routes and entries the file declares, or that a symbol of it handles.
    pub routes: Vec<RouteRow>,
}

/// One line of a file card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolRow {
    /// `Owner.name`, or `name` for a top-level symbol.
    pub name: String,
    pub kind: String,
    pub l0: u32,
    pub l1: u32,
    pub exported: bool,
    /// How many distinct symbols (or files, for top-level code) call it.
    pub called_from: u32,
}

/// What the index knows about one symbol. Callers and callees are display names, as
/// [`display_name`] gives them, sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolCard {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub l0: u32,
    pub l1: u32,
    pub sig: String,
    pub callers: Vec<String>,
    pub callees: Vec<String>,
    /// Calls inside the symbol that resolve to nothing, as written (`db.insert`).
    pub unresolved_calls: Vec<String>,
    /// The labels of the routes the symbol handles (`API · POST /api/orders`).
    pub roles: Vec<String>,
}

/// The card of the file at `rel`, or `None` when the index has no such file.
pub fn file_card(store: &Store, rel: &str) -> Result<Option<FileCard>> {
    let conn = store.conn();
    let id = file_id(rel);
    let row = conn
        .query_row(
            "SELECT lang, class, parse_status, loc FROM file WHERE id = ?1 AND path = ?2",
            params![id, rel],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((lang, class, parse_status, lines)) = row else {
        return Ok(None);
    };

    let symbols = rows(
        conn,
        "SELECT s.owner, s.name, s.kind, s.l0, s.l1, s.exported,
                (SELECT count(DISTINCT e.src_id) FROM edge e
                  WHERE e.kind = 'calls' AND e.dst_id = s.id)
           FROM symbol s WHERE s.file_id = ?1
          ORDER BY s.l0, s.l1 DESC, s.name, s.id",
        &id,
        |row| {
            let owner: String = row.get(0)?;
            let name: String = row.get(1)?;
            Ok(SymbolRow {
                name: qualified(&owner, &name),
                kind: row.get(2)?,
                l0: row.get(3)?,
                l1: row.get(4)?,
                exported: row.get(5)?,
                called_from: row.get(6)?,
            })
        },
    )?;

    let mut importers = rows(
        conn,
        "SELECT DISTINCT f.path, f.class FROM edge e JOIN file f ON f.id = e.src_id
          WHERE e.kind = 'imports' AND e.dst_id = ?1 AND f.id != ?1",
        &id,
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;
    importers.sort_by(|(a_path, a_class), (b_path, b_class)| {
        (a_class == "test", a_path).cmp(&(b_class == "test", b_path))
    });
    let used_by = importers.into_iter().map(|(path, _)| path).collect();

    let mut uses = rows(
        conn,
        "SELECT DISTINCT f.path FROM edge e JOIN file f ON f.id = e.dst_id
          WHERE e.kind = 'imports' AND e.src_id = ?1 AND f.id != ?1 ORDER BY f.path",
        &id,
        |row| row.get::<_, String>(0),
    )?;
    uses.extend(rows(
        conn,
        "SELECT DISTINCT text FROM ref
          WHERE file_id = ?1 AND kind = 'import' AND status = 'external' ORDER BY text",
        &id,
        |row| {
            row.get::<_, String>(0)
                .map(|pkg| format!("{pkg} (external)"))
        },
    )?);

    let unresolved = conn.query_row(
        "SELECT count(*) FROM ref WHERE file_id = ?1 AND status = 'unresolved'",
        [&id],
        |row| row.get(0),
    )?;

    Ok(Some(FileCard {
        path: rel.to_string(),
        lang,
        class,
        parse_status,
        lines,
        symbols,
        used_by,
        uses,
        unresolved,
        routes: routes_for_file(store, &id)?,
    }))
}

/// The card of `symbol` (`name` or `Owner.name`) in the file at `rel`, or `None` when
/// the file or the symbol is not in the index. If two symbols of the file share the
/// name, the first in source order is taken.
pub fn symbol_card(store: &Store, rel: &str, symbol: &str) -> Result<Option<SymbolCard>> {
    let conn = store.conn();
    let file = file_id(rel);
    // `a.b` is the method `b` of `a`, or else a top-level symbol that is called `a.b`.
    let mut candidates = vec![("", symbol)];
    if let Some((owner, name)) = symbol.rsplit_once('.') {
        candidates.insert(0, (owner, name));
    }
    for (owner, name) in candidates {
        let found = conn
            .query_row(
                "SELECT s.id, s.kind, s.l0, s.l1, s.sig FROM symbol s JOIN file f ON f.id = s.file_id
                  WHERE f.id = ?1 AND f.path = ?2 AND s.owner = ?3 AND s.name = ?4
                  ORDER BY s.l0, s.id LIMIT 1",
                params![file, rel, owner, name],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, kind, l0, l1, sig)) = found else {
            continue;
        };
        let unresolved_calls = conn
            .prepare(
                "SELECT DISTINCT text FROM ref
                  WHERE file_id = ?1 AND kind = 'call' AND status = 'unresolved'
                    AND line BETWEEN ?2 AND ?3 ORDER BY text",
            )?
            .query_map(params![file, l0, l1], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        return Ok(Some(SymbolCard {
            name: qualified(owner, name),
            kind,
            path: rel.to_string(),
            l0,
            l1,
            sig,
            callers: linked(conn, "dst_id", "src_id", &id)?,
            callees: linked(conn, "src_id", "dst_id", &id)?,
            unresolved_calls,
            roles: roles(&routes_for_symbol(store, &id)?),
        }));
    }
    Ok(None)
}

// Routes, manifests and imports (L2a).

/// One entry point: a page, an HTTP handler, a server action, a middleware, a command
/// or a `main`/`start` entry of a `package.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteRow {
    pub id: String,
    /// The folder of the governing `package.json`; `""` for the repository root.
    pub root: String,
    pub entry_kind: String,
    pub method: String,
    pub pattern: String,
    pub handler_id: Option<String>,
    /// The display name of a symbol handler, the file path for a file handler.
    pub handler: Option<String>,
    /// The file that declares the route.
    pub file: String,
    pub line: u32,
    pub rule: String,
    pub prov: String,
    pub conf: String,
    pub detail: Option<String>,
    /// [`role_label`], with the middleware matcher read from `detail`.
    pub label: String,
}

/// The columns every route query selects, and the joins that name the handler.
const ROUTE_SELECT: &str = "SELECT r.id, r.root, r.entry_kind, r.method, r.pattern, r.handler_id,
        f.path, r.line, r.rule, r.prov, r.conf, r.detail, hf.path, hs.owner, hs.name, ff.path
   FROM route r
   JOIN file f ON f.id = r.file_id
   LEFT JOIN symbol hs ON hs.id = r.handler_id
   LEFT JOIN file hf ON hf.id = hs.file_id
   LEFT JOIN file ff ON ff.id = r.handler_id";

const ROUTE_ORDER: &str = "ORDER BY r.root, r.entry_kind, r.pattern, r.method, r.id";

/// Every route and entry, by (root, entry kind, pattern, method).
pub fn routes(store: &Store) -> Result<Vec<RouteRow>> {
    route_query(store, "", &[])
}

/// The routes a file declares and those handled by a symbol that lives in it, sorted
/// as [`routes`] is, each once.
pub fn routes_for_file(store: &Store, file_id: &str) -> Result<Vec<RouteRow>> {
    route_query(
        store,
        "WHERE r.file_id = ?1 OR hs.file_id = ?1",
        &[&file_id],
    )
}

/// The routes whose handler is `symbol_id`.
pub fn routes_for_symbol(store: &Store, symbol_id: &str) -> Result<Vec<RouteRow>> {
    route_query(store, "WHERE r.handler_id = ?1", &[&symbol_id])
}

fn route_query(
    store: &Store,
    filter: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<RouteRow>> {
    let sql = format!("{ROUTE_SELECT} {filter} {ROUTE_ORDER}");
    Ok(store
        .conn()
        .prepare(&sql)?
        .query_map(args, |row| {
            let entry_kind: String = row.get(2)?;
            let method: String = row.get(3)?;
            let pattern: String = row.get(4)?;
            let detail: Option<String> = row.get(11)?;
            let symbol_file: Option<String> = row.get(12)?;
            let owner: Option<String> = row.get(13)?;
            let symbol_name: Option<String> = row.get(14)?;
            let file_handler: Option<String> = row.get(15)?;
            let handler = match (symbol_file, symbol_name) {
                (Some(path), Some(name)) => Some(format!(
                    "{path}#{}",
                    qualified(&owner.unwrap_or_default(), &name)
                )),
                _ => file_handler,
            };
            let label = if middleware_is_unevaluated(&entry_kind, detail.as_deref()) {
                "Runs before many routes".to_string()
            } else {
                role_label(&entry_kind, &method, &pattern)
            };
            Ok(RouteRow {
                id: row.get(0)?,
                root: row.get(1)?,
                entry_kind,
                method,
                pattern,
                handler_id: row.get(5)?,
                handler,
                file: row.get(6)?,
                line: row.get(7)?,
                rule: row.get(8)?,
                prov: row.get(9)?,
                conf: row.get(10)?,
                detail,
                label,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// True for a middleware whose matcher the index could not evaluate (its `detail` says
/// `"evaluated": false`): it applies to many routes, not to a path.
fn middleware_is_unevaluated(entry_kind: &str, detail: Option<&str>) -> bool {
    entry_kind == "middleware"
        && detail
            .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
            .is_some_and(|d| d["evaluated"] == serde_json::Value::Bool(false))
}

/// What a route is called in plain words: `API · POST /api/auth/login`,
/// `Page · /orders/{id}`, `Server action · placeOrder`, `Runs before /account/:path*`,
/// `Command · shop`, `Start script · npm start`, `Entry · index.js`. A middleware
/// without a path of its own (`/*`) runs before many routes.
pub fn role_label(entry_kind: &str, method: &str, pattern: &str) -> String {
    match (entry_kind, method) {
        ("http", _) => format!("API · {method} {pattern}"),
        ("page", _) => format!("Page · {pattern}"),
        ("server_action", _) => format!("Server action · {pattern}"),
        ("middleware", _) if matches!(pattern, "" | "*" | "/*") => {
            "Runs before many routes".to_string()
        }
        ("middleware", _) => format!("Runs before {pattern}"),
        ("cli", _) => format!("Command · {pattern}"),
        ("main", "START") => format!("Start script · npm {pattern}"),
        ("main", _) => format!("Entry · {pattern}"),
        _ => format!("{entry_kind} · {method} {pattern}"),
    }
}

/// The labels of `routes`, in order, each once.
fn roles(routes: &[RouteRow]) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for route in routes {
        if !labels.contains(&route.label) {
            labels.push(route.label.clone());
        }
    }
    labels
}

/// A `package.json` the index read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestRow {
    pub path: String,
    /// The folder of the manifest; `""` at the repository root.
    pub root: String,
    pub kind: String,
    pub name: Option<String>,
    pub lockfile: Option<String>,
    /// (command, script path as written, line)
    pub bin: Vec<(String, String, u32)>,
    /// (path as written, line)
    pub main: Option<(String, u32)>,
    /// (name, command, line)
    pub scripts: Vec<(String, String, u32)>,
}

/// The `detail` column of `manifest`; a field that is missing or of the wrong type is
/// left empty.
#[derive(Default, Deserialize)]
#[serde(default)]
struct ManifestDetail {
    bin: Vec<(String, String, u32)>,
    main: Option<(String, u32)>,
    scripts: Vec<(String, String, u32)>,
}

/// Every manifest, by path.
pub fn manifests(store: &Store) -> Result<Vec<ManifestRow>> {
    Ok(store
        .conn()
        .prepare(
            "SELECT f.path, m.root, m.kind, m.name, m.lockfile, m.detail
               FROM manifest m JOIN file f ON f.id = m.file_id ORDER BY f.path",
        )?
        .query_map([], |row| {
            let detail: String = row.get(5)?;
            let detail: ManifestDetail = serde_json::from_str(&detail).unwrap_or_default();
            Ok(ManifestRow {
                path: row.get(0)?,
                root: row.get(1)?,
                kind: row.get(2)?,
                name: row.get(3)?,
                lockfile: row.get(4)?,
                bin: detail.bin,
                main: detail.main,
                scripts: detail.scripts,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A dependency a manifest declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyRow {
    /// The path of the declaring `package.json`.
    pub manifest: String,
    pub root: String,
    pub name: String,
    pub section: String,
    pub dev: bool,
    pub declared: String,
    /// The version the lockfile pins, when one was read.
    pub resolved: Option<String>,
    pub line: u32,
}

/// The dependencies of every manifest, or only those of the manifest in folder `root`
/// (`""` for the repository root), by (manifest, section, name).
pub fn dependencies(store: &Store, root: Option<&str>) -> Result<Vec<DependencyRow>> {
    Ok(store
        .conn()
        .prepare(
            "SELECT f.path, m.root, d.name, d.section, d.dev, d.declared, d.resolved, d.line
               FROM dependency d
               JOIN manifest m ON m.file_id = d.file_id
               JOIN file f ON f.id = d.file_id
              WHERE ?1 IS NULL OR m.root = ?1
              ORDER BY f.path, d.section, d.name",
        )?
        .query_map([root], |row| {
            Ok(DependencyRow {
                manifest: row.get(0)?,
                root: row.get(1)?,
                name: row.get(2)?,
                section: row.get(3)?,
                dev: row.get(4)?,
                declared: row.get(5)?,
                resolved: row.get(6)?,
                line: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One import of a package that is not part of the project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ExternalImport {
    pub file: String,
    /// The folder of the nearest manifest above the file; `""` for the repository root,
    /// and for a file that no manifest covers.
    pub root: String,
    pub package: String,
    pub line: u32,
}

/// Every external import, by (file, package, line).
pub fn external_imports(store: &Store) -> Result<Vec<ExternalImport>> {
    let roots: BTreeSet<String> = manifests(store)?.into_iter().map(|m| m.root).collect();
    let mut found = store
        .conn()
        .prepare(
            "SELECT DISTINCT f.path, r.text, r.line FROM ref r JOIN file f ON f.id = r.file_id
              WHERE r.kind = 'import' AND r.status = 'external'",
        )?
        .query_map([], |row| {
            let file: String = row.get(0)?;
            Ok(ExternalImport {
                root: nearest_root(&roots, &file),
                file,
                package: row.get(1)?,
                line: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    found.sort();
    Ok(found)
}

/// The longest manifest folder that holds `rel`, or `""`.
fn nearest_root(roots: &BTreeSet<String>, rel: &str) -> String {
    roots
        .iter()
        .filter(|root| {
            !root.is_empty()
                && rel
                    .strip_prefix(root.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|root| root.len())
        .cloned()
        .unwrap_or_default()
}

/// For each package imported from the manifest folder `root`, in how many distinct
/// files.
pub fn import_counts(store: &Store, root: &str) -> Result<BTreeMap<String, u32>> {
    let mut files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for import in external_imports(store)? {
        if import.root == root {
            files.entry(import.package).or_default().insert(import.file);
        }
    }
    Ok(files
        .into_iter()
        .map(|(package, files)| (package, files.len() as u32))
        .collect())
}

/// `Owner.name`, or `name` without an owner.
fn qualified(owner: &str, name: &str) -> String {
    if owner.is_empty() {
        name.to_string()
    } else {
        format!("{owner}.{name}")
    }
}

/// The display names of the nodes at the far end of the `calls` edges that have `id`
/// in column `own`; `other` is the column of the far end. A node is a symbol or, for
/// top-level code, a file. Sorted, without duplicates.
fn linked(conn: &Connection, own: &str, other: &str, id: &str) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT DISTINCT f.path, s.owner, s.name
           FROM edge e
           LEFT JOIN symbol s ON s.id = e.{other}
           JOIN file f ON f.id = COALESCE(s.file_id, e.{other})
          WHERE e.kind = 'calls' AND e.{own} = ?1"
    );
    let mut names = conn
        .prepare(&sql)?
        .query_map([id], |row| {
            let path: String = row.get(0)?;
            let owner: Option<String> = row.get(1)?;
            let name: Option<String> = row.get(2)?;
            Ok(match name {
                Some(name) => format!("{path}#{}", qualified(&owner.unwrap_or_default(), &name)),
                None => path,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    names.sort();
    names.dedup();
    Ok(names)
}

/// Runs `sql` with `id` bound to `?1` and maps every row.
fn rows<T>(
    conn: &Connection,
    sql: &str,
    id: &str,
    map: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    Ok(conn
        .prepare(sql)?
        .query_map([id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    #[test]
    fn names_a_file_a_symbol_and_a_method() {
        let facts = crate::ts::extract("function f() {}\nclass C { m() {} }", Lang::TypeScript);
        let name = |i: usize| display_name("src/a.ts", Some(&facts.symbols[i]));
        assert_eq!(display_name("src/a.ts", None), "src/a.ts");
        assert_eq!(name(0), "src/a.ts#f");
        assert_eq!(name(2), "src/a.ts#C.m");
    }
}
