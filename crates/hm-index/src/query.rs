//! Reading the facts back: readable names for files and symbols. The cards of
//! Learn spec §7 are added here later.

use hm_core::ids::file_id;
use hm_store::Store;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::facts::SymbolFact;
use crate::index::Result;

/// The name shown for a file or a symbol: `path`, `path#name` or `path#Owner.name`.
pub fn display_name(rel: &str, sym: Option<&SymbolFact>) -> String {
    match sym {
        None => rel.to_string(),
        Some(s) if s.owner.is_empty() => format!("{rel}#{}", s.name),
        Some(s) => format!("{rel}#{}.{}", s.owner, s.name),
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
        }));
    }
    Ok(None)
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
