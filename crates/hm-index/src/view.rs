//! What the panel reads from the facts: the files and links of the map, whether a file
//! is indexed, and search hits. Everything here is read-only and sorted, so the same
//! facts always give the same answer.

use std::collections::{BTreeMap, HashMap};

use hm_core::ids::file_id;
use hm_store::Store;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::index::Result;

/// The files on the map, the links between them, and what is left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapFacts {
    /// Files of class `source` or `generated`, sorted by path.
    pub files: Vec<MapFile>,
    /// Sorted by (`src`, `dst`).
    pub links: Vec<MapLink>,
    /// How many files of each other class (`test`, `doc`, `infra`) are not on the map.
    pub excluded: BTreeMap<String, u32>,
}

/// One file of the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapFile {
    pub path: String,
    pub lang: Option<String>,
    pub loc: u32,
    pub generated: bool,
}

/// Files `src` uses `dst`. Both are indexes into [`MapFacts::files`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapLink {
    pub src: u32,
    pub dst: u32,
    /// How many `calls` and `extends` edges join the two files; 1 when they only have
    /// `imports` edges, so a link is never invisible.
    pub refs: u32,
}

/// The facts behind the map. Edges end at files or at symbols; a symbol counts as its
/// file. Links to a file that is not on the map (a test, a doc) and links from a file
/// to itself are left out.
pub fn map_facts(store: &Store) -> Result<MapFacts> {
    let conn = store.conn();

    let mut files = Vec::new();
    let mut index_of: HashMap<String, u32> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT id, path, lang, loc, class FROM file
          WHERE class IN ('source', 'generated') ORDER BY path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            MapFile {
                path: row.get(1)?,
                lang: row.get(2)?,
                loc: row.get(3)?,
                generated: row.get::<_, String>(4)? == "generated",
            },
        ))
    })?;
    for row in rows {
        let (id, file) = row?;
        index_of.insert(id, files.len() as u32);
        files.push(file);
    }

    let mut excluded = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT class, count(*) FROM file
          WHERE class NOT IN ('source', 'generated') GROUP BY class",
    )?;
    for row in stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })? {
        let (class, n) = row?;
        excluded.insert(class, n);
    }

    // Lift both ends of every edge to its file, then count per file pair and kind.
    let mut stmt = conn.prepare(
        "SELECT COALESCE(s1.file_id, e.src_id), COALESCE(s2.file_id, e.dst_id), e.kind, count(*)
           FROM edge e
           LEFT JOIN symbol s1 ON s1.id = e.src_id
           LEFT JOIN symbol s2 ON s2.id = e.dst_id
          GROUP BY 1, 2, 3",
    )?;
    // (src, dst) -> (calls + extends rows, whether an `imports` row exists).
    let mut pairs: BTreeMap<(u32, u32), (u32, bool)> = BTreeMap::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u32>(3)?,
        ))
    })?;
    for row in rows {
        let (src, dst, kind, n) = row?;
        let (Some(&src), Some(&dst)) = (index_of.get(&src), index_of.get(&dst)) else {
            continue;
        };
        if src == dst {
            continue;
        }
        let entry = pairs.entry((src, dst)).or_default();
        match kind.as_str() {
            "calls" | "extends" => entry.0 += n,
            "imports" => entry.1 = true,
            _ => {}
        }
    }
    let links = pairs
        .into_iter()
        .filter_map(|((src, dst), (refs, imported))| {
            let refs = if refs > 0 { refs } else { u32::from(imported) };
            (refs > 0).then_some(MapLink { src, dst, refs })
        })
        .collect();

    Ok(MapFacts {
        files,
        links,
        excluded,
    })
}

/// A file the index has a row for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexedFile {
    pub path: String,
    pub lang: Option<String>,
    pub class: String,
}

/// The index row of the file at `rel`, or `None` when the index has none. A file that
/// is on disk but ignored, skipped or new is not a row, so this is also the check that
/// a path may be shown.
pub fn indexed_file(store: &Store, rel: &str) -> Result<Option<IndexedFile>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT path, lang, class FROM file WHERE id = ?1 AND path = ?2",
            params![file_id(rel), rel],
            |row| {
                Ok(IndexedFile {
                    path: row.get(0)?,
                    lang: row.get(1)?,
                    class: row.get(2)?,
                })
            },
        )
        .optional()?)
}

/// One search result: a file, or a symbol inside a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    /// `"file"` or `"symbol"`.
    pub kind: &'static str,
    pub path: String,
    /// `name` or `Owner.name`; `None` for a file.
    pub symbol: Option<String>,
}

/// At most `limit` files and symbols that match `query`, files first. A file matches
/// when `query` is a substring of its path, a symbol when it is a substring of its
/// name (`Owner.name` for a method); case is ignored for ASCII letters, and `%` and
/// `_` mean themselves. An empty or blank query matches nothing.
///
/// Files rank by file name: the whole name (or the name without its last extension)
/// equal to the query, then names that start with it, then shorter paths, then path
/// order. Symbols rank the same way by symbol name, then by their file's path.
pub fn search(store: &Store, query: &str, limit: usize) -> Result<Vec<Hit>> {
    let query = query.trim();
    if query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let conn = store.conn();
    let plain = query.to_ascii_lowercase();
    let escaped = escape_like(&plain);
    let contains = format!("%{escaped}%");
    let prefix = format!("{escaped}%");
    let stem = format!("{escaped}.%");
    let cap = i64::try_from(limit).unwrap_or(i64::MAX);

    // The file name is what follows the last `/`: remove from the path everything up
    // to and including that slash (SQLite has no basename function).
    let mut hits = Vec::new();
    let mut stmt = conn.prepare(
        "WITH f AS (
           SELECT path, lower(path) AS lpath,
                  lower(replace(path, rtrim(path, replace(path, '/', '')), '')) AS base
             FROM file)
         SELECT path FROM f
          WHERE lpath LIKE ?1 ESCAPE '\\'
          ORDER BY CASE
                     WHEN base = ?2 THEN 0
                     WHEN base LIKE ?3 ESCAPE '\\'
                          AND instr(substr(base, length(?2) + 2), '.') = 0 THEN 0
                     WHEN base LIKE ?4 ESCAPE '\\' THEN 1
                     ELSE 2 END,
                   length(path), path
          LIMIT ?5",
    )?;
    for path in stmt.query_map(params![contains, plain, stem, prefix, cap], |row| {
        row.get::<_, String>(0)
    })? {
        hits.push(Hit {
            kind: "file",
            path: path?,
            symbol: None,
        });
    }

    let left = cap - hits.len() as i64;
    if left > 0 {
        let mut stmt = conn.prepare(
            "WITH s AS (
               SELECT f.path AS path, s.name AS name, s.l0 AS l0, s.id AS id,
                      CASE WHEN s.owner = '' THEN s.name ELSE s.owner || '.' || s.name END AS qn
                 FROM symbol s JOIN file f ON f.id = s.file_id)
             SELECT path, qn FROM s
              WHERE lower(qn) LIKE ?1 ESCAPE '\\'
              ORDER BY CASE
                         WHEN lower(qn) = ?2 OR lower(name) = ?2 THEN 0
                         WHEN lower(qn) LIKE ?3 ESCAPE '\\'
                              OR lower(name) LIKE ?3 ESCAPE '\\' THEN 1
                         ELSE 2 END,
                       length(path), path, qn, l0, id
              LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![contains, plain, prefix, left], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (path, name) = row?;
            hits.push(Hit {
                kind: "symbol",
                path,
                symbol: Some(name),
            });
        }
    }
    Ok(hits)
}

/// `text` with `\`, `%` and `_` escaped for `LIKE ... ESCAPE '\'`.
fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
