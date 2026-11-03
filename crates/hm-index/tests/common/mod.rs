//! Helpers shared by the integration tests; later tasks add to it.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The folder of fixture `name` under `tests/fixtures`.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The text of a fixture file; `rel` is relative to `tests/fixtures`, as in
/// `ts-basic/src/db.ts`.
pub fn read_fixture(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A copy of fixture `name` in a temporary folder that is not a git repository. Every
/// file on disk comes along, including the ignored and skipped ones.
pub fn copy_fixture(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    copy_tree(&fixture(name), dir.path()).expect("copy the fixture");
    dir
}

/// A resolver for fixture `name`: the walked files are the known set and every
/// tsconfig in it is loaded.
pub fn resolver_for(name: &str) -> hm_index::ts::resolve::ModuleResolver {
    let root = fixture(name);
    let walked = hm_index::walk::walk(&root).expect("walk the fixture");
    let known: std::collections::BTreeSet<String> =
        walked.files.iter().map(|f| f.rel.clone()).collect();
    let tsconfigs = hm_index::ts::tsconfig::load_tsconfigs(&root, &known);
    hm_index::ts::resolve::ModuleResolver::new(known, tsconfigs)
}

/// What `named_edges_for` found, with ids mapped back to readable names.
#[derive(Debug, Default)]
pub struct Named {
    /// (kind, source, destination), as `path` or `path#Owner.name`.
    pub edges: std::collections::BTreeSet<(String, String, String)>,
    /// (file, kind, text, status).
    pub refs: std::collections::BTreeSet<(String, String, String, String)>,
}

/// Extracts every parsed file of fixture `name`, resolves each one and names the ids.
pub fn named_edges_for(name: &str) -> Named {
    use hm_core::ids::file_id;
    use hm_index::lang::Lang;
    use hm_index::query::display_name;
    use hm_index::ts::resolve::{Project, resolve_file, symbol_ids};
    use std::collections::{BTreeMap, HashMap};

    let root = fixture(name);
    let modules = resolver_for(name);
    let walked = hm_index::walk::walk(&root).expect("walk the fixture");
    let mut facts = BTreeMap::new();
    for f in &walked.files {
        let Some(lang) = f.lang.filter(Lang::parsed_in_l1) else {
            continue;
        };
        let src = fs::read_to_string(&f.abs).expect("read a fixture file");
        facts.insert(f.rel.clone(), hm_index::ts::extract(&src, lang));
    }

    let mut names: HashMap<String, String> = HashMap::new();
    for (rel, file_facts) in &facts {
        names.insert(file_id(rel), display_name(rel, None));
        for (id, sym) in symbol_ids(rel, file_facts)
            .into_iter()
            .zip(&file_facts.symbols)
        {
            names.insert(id, display_name(rel, Some(sym)));
        }
    }
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    let mut out = Named::default();
    for rel in facts.keys() {
        let resolved = resolve_file(&project, rel);
        for e in resolved.edges {
            out.edges
                .insert((e.kind.into(), name_of(&e.src_id), name_of(&e.dst_id)));
        }
        for r in resolved.refs {
            out.refs
                .insert((rel.clone(), r.kind.into(), r.text, r.status.into()));
        }
    }
    out
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// An indexer for a copy of fixture `name` with an in-memory store, not yet run. The
/// `TempDir` comes last: drop the indexer first, because Windows cannot delete a
/// folder that SQLite or the watcher holds open.
pub fn indexer_for(name: &str) -> (hm_index::index::Indexer, tempfile::TempDir) {
    let dir = copy_fixture(name);
    let store = hm_store::Store::open_in_memory().expect("open a store");
    let ix = hm_index::index::Indexer::open(dir.path(), store).expect("open the indexer");
    (ix, dir)
}

/// A cold full pass over fixture `name` in place (it is only read) into a new
/// in-memory store.
pub fn fresh_index(name: &str) -> hm_index::index::Indexer {
    fresh_index_of(&fixture(name))
}

/// A cold full pass over `dir` into a new in-memory store.
pub fn fresh_index_of(dir: &Path) -> hm_index::index::Indexer {
    let store = hm_store::Store::open_in_memory().expect("open a store");
    let mut ix = hm_index::index::Indexer::open(dir, store).expect("open the indexer");
    ix.full().expect("index the folder");
    ix
}

/// A copy of fixture `name`, already indexed. The `TempDir` comes last, as in
/// `indexer_for`.
pub fn indexed(name: &str) -> (hm_index::index::Indexer, tempfile::TempDir) {
    let (mut ix, dir) = indexer_for(name);
    ix.full().expect("index the fixture");
    (ix, dir)
}

/// Replaces the text `from` with `to` in `rel` under `dir`; `from` must be there.
pub fn edit(dir: &Path, rel: &str, from: &str, to: &str) {
    let path = dir.join(rel);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(text.contains(from), "{rel} does not contain {from:?}");
    fs::write(&path, text.replacen(from, to, 1)).expect("write the edited file");
}

/// A set of owned strings, for comparing path lists.
pub fn set<const N: usize>(items: [&str; N]) -> std::collections::BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Every row of the store as sorted text, ids replaced by readable names. It leaves
/// out what changes between runs (mtimes, `indexed_at`), so two stores that hold the
/// same index dump the same text.
pub fn dump(store: &hm_store::Store) -> String {
    use hm_core::ids::file_id;
    use hm_index::facts::FileFacts;
    use hm_index::query::display_name;
    use hm_index::ts::resolve::symbol_ids;
    use std::collections::HashMap;

    let conn = store.conn();
    let col = |sql: &str| -> Vec<Vec<String>> {
        let mut stmt = conn.prepare(sql).expect("prepare");
        let n = stmt.column_count();
        stmt.query_map([], |row| {
            (0..n)
                .map(|i| {
                    Ok(match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => "-".to_string(),
                        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                        rusqlite::types::ValueRef::Real(v) => v.to_string(),
                        rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into(),
                        rusqlite::types::ValueRef::Blob(_) => "<blob>".to_string(),
                    })
                })
                .collect()
        })
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
    };

    let mut names: HashMap<String, String> = HashMap::new();
    let mut paths: HashMap<String, String> = HashMap::new();
    for row in col("SELECT path, facts_json FROM file") {
        let (rel, json) = (&row[0], &row[1]);
        names.insert(file_id(rel), display_name(rel, None));
        paths.insert(file_id(rel), rel.clone());
        if json != "-" {
            let facts: FileFacts = serde_json::from_str(json).expect("facts_json reads");
            for (id, sym) in symbol_ids(rel, &facts).into_iter().zip(&facts.symbols) {
                names.insert(id, display_name(rel, Some(sym)));
            }
        }
    }
    let name = |id: &str| names.get(id).cloned().unwrap_or_else(|| format!("?{id}"));
    let path = |id: &str| paths.get(id).cloned().unwrap_or_else(|| format!("?{id}"));

    let mut sections: Vec<(&str, Vec<String>)> = Vec::new();
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    sections.push((
        "file",
        sorted(
            col("SELECT path, lang, class, loc, parse_status, content_hash, interface_hash, facts_hash FROM file")
                .iter()
                .map(|r| r.join(" | "))
                .collect(),
        ),
    ));
    sections.push((
        "symbol",
        sorted(
            col("SELECT file_id, kind, name, owner, sig, sig_hash, body_hash, l0, l1, exported, lines, branches, id FROM symbol")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} | {}",
                        name(&r[12]),
                        path(&r[0]),
                        r[1..12].join(" | ")
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "edge",
        sorted(
            col("SELECT kind, src_id, dst_id, file_id, line, rule, prov, conf FROM edge")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} -> {} | {} | {} | {} | {} | {}",
                        r[0],
                        name(&r[1]),
                        name(&r[2]),
                        path(&r[3]),
                        r[4],
                        r[5],
                        r[6],
                        r[7]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "ref",
        sorted(
            col("SELECT file_id, kind, text, line, status FROM ref")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} | {} | {} | {}",
                        path(&r[0]),
                        r[1],
                        r[2],
                        r[3],
                        r[4]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "reverse_import",
        sorted(
            col("SELECT dst_file_id, src_file_id FROM reverse_import")
                .iter()
                .map(|r| format!("{} <- {}", path(&r[0]), path(&r[1])))
                .collect(),
        ),
    ));

    sections.push((
        "route",
        sorted(
            col("SELECT id, root, entry_kind, method, pattern, handler_id, file_id, line, rule, prov, conf, detail FROM route")
                .iter()
                .map(|r| {
                    let handler = if r[5] == "-" { "-".to_string() } else { name(&r[5]) };
                    format!(
                        "{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}",
                        r[0],
                        r[1],
                        r[2],
                        r[3],
                        r[4],
                        handler,
                        path(&r[6]),
                        r[7],
                        r[8],
                        r[9],
                        r[10],
                        r[11]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "client_call",
        sorted(
            col("SELECT file_id, src_id, line, method, url, rule FROM client_call")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} | {} | {} | {} | {}",
                        path(&r[0]),
                        name(&r[1]),
                        r[2],
                        r[3],
                        r[4],
                        r[5]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "manifest",
        sorted(
            col("SELECT file_id, root, kind, name, lockfile, detail FROM manifest")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} | {} | {} | {} | {}",
                        path(&r[0]),
                        r[1],
                        r[2],
                        r[3],
                        r[4],
                        r[5]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "dependency",
        sorted(
            col("SELECT file_id, section, name, declared, resolved, dev, line FROM dependency")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} | {} | {} | {} | {} | {}",
                        path(&r[0]),
                        r[1],
                        r[2],
                        r[3],
                        r[4],
                        r[5],
                        r[6]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "pattern",
        sorted(
            col("SELECT file_id, subject_id, kind, line, rule, conf, term, detail FROM pattern")
                .iter()
                .map(|r| {
                    format!(
                        "{} | {} | {} | {} | {} | {} | {} | {}",
                        path(&r[0]),
                        name(&r[1]),
                        r[2],
                        r[3],
                        r[4],
                        r[5],
                        r[6],
                        r[7]
                    )
                })
                .collect(),
        ),
    ));
    sections.push((
        "gap",
        sorted(
            col("SELECT file_id, subject_id, kind, line FROM gap")
                .iter()
                .map(|r| format!("{} | {} | {} | {}", path(&r[0]), name(&r[1]), r[2], r[3]))
                .collect(),
        ),
    ));

    let mut out = String::new();
    for (title, rows) in sections {
        out.push_str(&format!("[{title}] {}\n", rows.len()));
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
    }
    out
}

/// Gives a path its permissions back when dropped, so the `TempDir` can be removed.
#[cfg(unix)]
pub struct Locked {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

/// Takes every permission away from the file or folder at `path` until the guard
/// drops. `None` when that does not stop reading it, as when the tests run as root.
#[cfg(unix)]
pub fn lock(path: &Path) -> Option<Locked> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path).expect("stat").permissions().mode();
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("chmod");
    let guard = Locked {
        path: path.to_path_buf(),
        mode,
    };
    let readable = if path.is_dir() {
        fs::read_dir(path).is_ok()
    } else {
        fs::File::open(path).is_ok()
    };
    if readable {
        eprintln!("skipped: permissions do not stop this user (root?)");
        return None;
    }
    Some(guard)
}
