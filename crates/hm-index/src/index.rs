//! The indexer: a full pass that walks a worktree, parses what L1 understands,
//! resolves names across files and writes everything into the facts store in one
//! transaction (Learn spec §5).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use hm_core::ids::file_id;
use hm_store::{Store, StoreError};
use rusqlite::{Transaction, params};
use serde::Serialize;

use crate::facts::{FileFacts, ParseStatus};
use crate::hash::{content_hash, facts_hash, interface_hash};
use crate::lang::Lang;
use crate::ts::resolve::{ModuleResolver, Project, resolve_file, symbol_ids};
use crate::ts::tsconfig::load_tsconfigs;
use crate::walk::{FileClass, Skip, WalkedFile, walk};

/// Where every L1 edge comes from.
const PROVENANCE: &str = "parsed";
/// `Confidence::Sure` as the store spells it (checked by a test).
const CONF_SURE: &str = "sure";

#[derive(Debug)]
pub enum IndexError {
    Store(StoreError),
    Io(io::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Store(err) => write!(f, "{err}"),
            IndexError::Io(err) => write!(f, "cannot read the project: {err}"),
            IndexError::Sqlite(err) => write!(f, "store database error: {err}"),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IndexError::Store(err) => Some(err),
            IndexError::Io(err) => Some(err),
            IndexError::Sqlite(err) => Some(err),
        }
    }
}

impl From<StoreError> for IndexError {
    fn from(err: StoreError) -> Self {
        IndexError::Store(err)
    }
}

impl From<io::Error> for IndexError {
    fn from(err: io::Error) -> Self {
        IndexError::Io(err)
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(err: rusqlite::Error) -> Self {
        IndexError::Sqlite(err)
    }
}

pub type Result<T> = std::result::Result<T, IndexError>;

/// What one pass saw and did. `files`, `classes`, `skipped`, `not_supported_yet`
/// describe the tree as walked; `hashed`, `parsed` and `parsed_with_errors` describe
/// the work of this pass; `symbols`, `edges` and `refs` describe the store after it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct IndexReport {
    /// File count by language name, `other` for files without one.
    pub files: BTreeMap<String, u32>,
    /// File count by class (`source`, `test`, `generated`, `doc`, `infra`).
    pub classes: BTreeMap<String, u32>,
    /// `too_large` and `binary` files, and `dir:<name>` for each kind of folder that
    /// was not entered.
    pub skipped: BTreeMap<String, u32>,
    /// Files whose bytes were read and hashed.
    pub hashed: u32,
    /// Files parsed, those with syntax errors included.
    pub parsed: u32,
    /// Of `parsed`, the files in which the parser found errors.
    pub parsed_with_errors: u32,
    /// Files in a language that L1 recognises but does not parse.
    pub not_supported_yet: u32,
    pub symbols: u32,
    /// Edge count by kind.
    pub edges: BTreeMap<String, u32>,
    /// Ref count by status (`unresolved`, `external`).
    pub refs: BTreeMap<String, u32>,
    pub took_ms: u64,
}

/// A file row as the last pass stored it.
struct Stored {
    class: String,
    lang: Option<String>,
    size: i64,
    mtime_ns: i64,
    content_hash: String,
    parse_status: String,
}

/// What a parse produced, with everything that is stored from it.
struct Parsed {
    facts: FileFacts,
    json: String,
    interface_hash: String,
    facts_hash: String,
}

/// What a pass found out about one walked file.
enum Change {
    /// Same size and mtime as stored: nothing to do.
    Unchanged,
    /// New size or mtime, same bytes: only those columns change.
    Touched,
    /// The walker did not let the file be read.
    Skipped(&'static str),
    /// New or changed bytes.
    Fresh {
        status: &'static str,
        hash: String,
        loc: u32,
        parsed: Option<Box<Parsed>>,
    },
    /// Deleted between the walk and the read.
    Gone,
}

/// Indexes one worktree into a facts store.
pub struct Indexer {
    root: PathBuf,
    store: Store,
    /// Facts of every parsed file, by relative path; the same as `file.facts_json`.
    facts: BTreeMap<String, FileFacts>,
    resolver: ModuleResolver,
}

impl Indexer {
    /// Wraps `store` for the worktree at `root`, loading what an earlier pass stored.
    pub fn open(root: &Path, store: Store) -> Result<Indexer> {
        let facts = load_facts(&store)?;
        let known = stored_paths(&store)?;
        let tsconfigs = load_tsconfigs(root, &known);
        Ok(Indexer {
            root: root.to_path_buf(),
            store,
            facts,
            resolver: ModuleResolver::new(known, tsconfigs),
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Walks the worktree and brings the store in line with it. A file whose size and
    /// mtime match its row is left alone; one that differs is hashed, and parsed only
    /// when its content hash changed too. Every file is resolved again, because an
    /// edge depends on files other than its own. All writes share one transaction.
    pub fn full(&mut self) -> Result<IndexReport> {
        let started = Instant::now();
        let walked = walk(&self.root)?;
        let stored = load_stored(&self.store)?;
        let mut report = IndexReport::default();
        for f in &walked.files {
            *report
                .files
                .entry(f.lang.map_or("other", |l| l.as_str()).to_string())
                .or_default() += 1;
            *report
                .classes
                .entry(f.class.as_str().to_string())
                .or_default() += 1;
            match f.skip {
                Some(skip) => {
                    *report
                        .skipped
                        .entry(skip_name(skip).to_string())
                        .or_default() += 1
                }
                None if unparsed_status(f) == NOT_SUPPORTED => {
                    report.not_supported_yet += 1;
                }
                None => {}
            }
        }
        for (name, count) in &walked.skipped_dirs {
            report.skipped.insert(format!("dir:{name}"), *count);
        }

        // Metadata decides what has to be read; the reading itself runs in parallel.
        let mut changes: Vec<Option<Change>> = Vec::new();
        let mut jobs: Vec<(usize, Job)> = Vec::new();
        for (i, f) in walked.files.iter().enumerate() {
            let old = stored.get(&f.rel);
            let have_facts = self.facts.contains_key(&f.rel);
            if let Some(skip) = f.skip {
                let same = old.is_some_and(|s| {
                    same_stat(s, f) && s.parse_status == skip_name(skip) && same_kind(s, f)
                });
                changes.push(Some(if same {
                    Change::Unchanged
                } else {
                    Change::Skipped(skip_name(skip))
                }));
            } else if old.is_some_and(|s| same_stat(s, f) && fits(s, f, have_facts)) {
                changes.push(Some(Change::Unchanged));
            } else {
                changes.push(None);
                jobs.push((
                    i,
                    Job {
                        file: f,
                        stored: old.filter(|s| !s.content_hash.is_empty()),
                        have_facts,
                    },
                ));
            }
        }
        report.hashed = jobs.len() as u32;
        for (i, result) in run_jobs(&jobs) {
            changes[i] = Some(result?);
        }
        let changes: Vec<Change> = changes
            .into_iter()
            .map(|c| c.expect("every file has a change"))
            .collect();

        // The files that still exist, with the facts they will have.
        let live: Vec<(&WalkedFile, &Change)> = walked
            .files
            .iter()
            .zip(&changes)
            .filter(|(_, c)| !matches!(c, Change::Gone))
            .collect();
        let known: BTreeSet<String> = live.iter().map(|(f, _)| f.rel.clone()).collect();
        let mut facts = std::mem::take(&mut self.facts);
        facts.retain(|rel, _| known.contains(rel));
        for (f, change) in &live {
            match change {
                Change::Fresh {
                    parsed: Some(p), ..
                } => {
                    report.parsed += 1;
                    report.parsed_with_errors +=
                        u32::from(p.facts.status == ParseStatus::ParsedWithErrors);
                    facts.insert(f.rel.clone(), p.facts.clone());
                }
                Change::Fresh { .. } | Change::Skipped(_) => {
                    facts.remove(&f.rel);
                }
                _ => {}
            }
        }
        // Edges depend on other files, so any change to the set of files or to a
        // file's facts means resolving again. A touched file has the same bytes.
        let vanished: Vec<&String> = stored.keys().filter(|p| !known.contains(*p)).collect();
        let dirty = !vanished.is_empty()
            || changes
                .iter()
                .any(|c| !matches!(c, Change::Unchanged | Change::Touched));
        let tsconfigs = load_tsconfigs(&self.root, &known);
        let resolver = ModuleResolver::new(known.clone(), tsconfigs);

        let outcome = self.store.write(|tx| {
            for path in &vanished {
                tx.execute("DELETE FROM file WHERE path = ?1", [path])?;
            }
            for (f, change) in &live {
                write_change(tx, f, change)?;
            }
            let project = Project {
                facts: &facts,
                modules: &resolver,
            };
            if dirty {
                for rel in facts.keys() {
                    replace_resolved(tx, &project, rel)?;
                }
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let root = self.root.to_string_lossy();
            for (key, value) in [("indexed_at", now.to_string()), ("root", root.to_string())] {
                tx.execute(
                    "INSERT INTO meta(key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
                )?;
            }
            Ok(())
        });
        match outcome {
            Ok(()) => {
                self.facts = facts;
                self.resolver = resolver;
            }
            Err(err) => {
                // The map was partly rebuilt; the store is the truth. If it cannot be
                // read either, the map stays empty and the next pass parses again.
                if let Ok(facts) = load_facts(&self.store) {
                    self.facts = facts;
                }
                return Err(err.into());
            }
        }

        let conn = self.store.conn();
        report.symbols = conn.query_row("SELECT count(*) FROM symbol", [], |row| row.get(0))?;
        report.edges = counts(conn, "SELECT kind, count(*) FROM edge GROUP BY kind")?;
        report.refs = counts(conn, "SELECT status, count(*) FROM ref GROUP BY status")?;
        report.took_ms = started.elapsed().as_millis() as u64;
        Ok(report)
    }
}

/// `parse_status` of a file in a language that L1 does not parse.
const NOT_SUPPORTED: &str = "not_supported_yet";

fn skip_name(skip: Skip) -> &'static str {
    match skip {
        Skip::TooLarge => "too_large",
        Skip::Binary => "binary",
    }
}

/// Whether L1 parses the file: a code file in a supported language, not a doc or an
/// infrastructure file.
fn parses(f: &WalkedFile) -> bool {
    f.skip.is_none()
        && !matches!(f.class, FileClass::Doc | FileClass::Infra)
        && f.lang.is_some_and(|l| l.parsed_in_l1())
}

/// The `parse_status` of a file that is read but not parsed.
fn unparsed_status(f: &WalkedFile) -> &'static str {
    match f.lang {
        Some(lang)
            if !matches!(f.class, FileClass::Doc | FileClass::Infra) && !lang.parsed_in_l1() =>
        {
            NOT_SUPPORTED
        }
        _ => "not_parsed",
    }
}

/// Whether the stored class and language are what the walk reports now.
fn same_kind(s: &Stored, f: &WalkedFile) -> bool {
    s.class == f.class.as_str() && s.lang.as_deref() == f.lang.map(|l| l.as_str())
}

fn same_stat(s: &Stored, f: &WalkedFile) -> bool {
    // An mtime of 0 means the platform gave none, so it proves nothing.
    f.mtime_ns != 0 && s.mtime_ns == f.mtime_ns && s.size == f.size as i64
}

/// Whether the stored row is what a pass would write for `f` again.
fn fits(s: &Stored, f: &WalkedFile, have_facts: bool) -> bool {
    if !same_kind(s, f) {
        false
    } else if parses(f) {
        have_facts && matches!(s.parse_status.as_str(), "parsed" | "parsed_with_errors")
    } else {
        s.parse_status == unparsed_status(f)
    }
}

/// One file to read.
struct Job<'a> {
    file: &'a WalkedFile,
    /// The stored row, when it has a content hash.
    stored: Option<&'a Stored>,
    /// Whether the in-memory facts hold this file.
    have_facts: bool,
}

/// Reads and hashes every job, parsing the ones whose bytes changed, spread over the
/// available cores. Extraction is pure, so the result does not depend on scheduling.
fn run_jobs<'a>(jobs: &[(usize, Job<'a>)]) -> Vec<(usize, Result<Change>)> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = jobs.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|(i, job)| (*i, read_one(job).map_err(IndexError::from)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| {
                h.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

fn read_one(job: &Job) -> io::Result<Change> {
    let f = job.file;
    let bytes = match std::fs::read(&f.abs) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Change::Gone),
        Err(err) => return Err(err),
    };
    let hash = content_hash(&bytes);
    let lang = f.lang.filter(|_| parses(f));
    // Same bytes and the row is what a cold pass would write: only stat changed.
    if job
        .stored
        .is_some_and(|s| s.content_hash == hash && fits(s, f, job.have_facts))
    {
        return Ok(Change::Touched);
    }
    let newlines = bytes.iter().filter(|b| **b == b'\n').count();
    let unterminated = bytes.last().is_some_and(|b| *b != b'\n');
    let loc = (newlines + usize::from(unterminated)) as u32;
    let Some(lang) = lang else {
        return Ok(Change::Fresh {
            status: unparsed_status(f),
            hash,
            loc,
            parsed: None,
        });
    };
    let facts = crate::ts::extract(&String::from_utf8_lossy(&bytes), lang);
    let status = match facts.status {
        ParseStatus::Parsed => "parsed",
        ParseStatus::ParsedWithErrors => "parsed_with_errors",
    };
    Ok(Change::Fresh {
        status,
        hash,
        loc,
        parsed: Some(Box::new(Parsed {
            json: serde_json::to_string(&facts).map_err(io::Error::other)?,
            interface_hash: interface_hash(&facts),
            facts_hash: facts_hash(&facts),
            facts,
        })),
    })
}

/// Writes the `file` row (and, for fresh files, its symbols) for one change.
fn write_change(tx: &Transaction, f: &WalkedFile, change: &Change) -> rusqlite::Result<()> {
    let id = file_id(&f.rel);
    let lang = f.lang.map(|l: Lang| l.as_str());
    let (size, mtime) = (f.size as i64, f.mtime_ns);
    match change {
        Change::Unchanged | Change::Gone => {}
        Change::Touched => {
            tx.execute(
                "UPDATE file SET size = ?2, mtime_ns = ?3 WHERE id = ?1",
                params![id, size, mtime],
            )?;
        }
        Change::Skipped(status) => {
            upsert_file(tx, &id, f, lang, "", 0, status, None)?;
            clear_symbols(tx, &id)?;
            clear_resolved(tx, &id)?;
        }
        Change::Fresh {
            status,
            hash,
            loc,
            parsed,
        } => {
            upsert_file(tx, &id, f, lang, hash, *loc, status, parsed.as_deref())?;
            clear_symbols(tx, &id)?;
            match parsed {
                Some(p) => insert_symbols(tx, &id, &f.rel, &p.facts)?,
                // A parsed file's rows are replaced when the pass resolves it.
                None => clear_resolved(tx, &id)?,
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upsert_file(
    tx: &Transaction,
    id: &str,
    f: &WalkedFile,
    lang: Option<&str>,
    hash: &str,
    loc: u32,
    status: &str,
    parsed: Option<&Parsed>,
) -> rusqlite::Result<()> {
    // `ON CONFLICT DO UPDATE` keeps the row, so nothing cascades from it.
    tx.prepare_cached(
        "INSERT INTO file(id, path, lang, class, size, mtime_ns, loc, content_hash,
                          interface_hash, facts_hash, parse_status, facts_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
           path = excluded.path, lang = excluded.lang, class = excluded.class,
           size = excluded.size, mtime_ns = excluded.mtime_ns, loc = excluded.loc,
           content_hash = excluded.content_hash, interface_hash = excluded.interface_hash,
           facts_hash = excluded.facts_hash, parse_status = excluded.parse_status,
           facts_json = excluded.facts_json",
    )?
    .execute(params![
        id,
        f.rel,
        lang,
        f.class.as_str(),
        f.size as i64,
        f.mtime_ns,
        loc,
        hash,
        parsed.map(|p| p.interface_hash.as_str()),
        parsed.map(|p| p.facts_hash.as_str()),
        status,
        parsed.map(|p| p.json.as_str()),
    ])?;
    Ok(())
}

fn clear_symbols(tx: &Transaction, id: &str) -> rusqlite::Result<()> {
    tx.prepare_cached("DELETE FROM symbol WHERE file_id = ?1")?
        .execute([id])?;
    Ok(())
}

/// Removes the rows that resolving a file wrote.
fn clear_resolved(tx: &Transaction, id: &str) -> rusqlite::Result<()> {
    for sql in [
        "DELETE FROM edge WHERE file_id = ?1",
        "DELETE FROM ref WHERE file_id = ?1",
        "DELETE FROM reverse_import WHERE src_file_id = ?1",
    ] {
        tx.prepare_cached(sql)?.execute([id])?;
    }
    Ok(())
}

fn insert_symbols(
    tx: &Transaction,
    file: &str,
    rel: &str,
    facts: &FileFacts,
) -> rusqlite::Result<()> {
    let mut insert = tx.prepare_cached(
        "INSERT INTO symbol(id, file_id, kind, name, owner, sig, sig_hash, body_hash,
                            l0, l1, exported, lines, branches)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    for (id, s) in symbol_ids(rel, facts).iter().zip(&facts.symbols) {
        insert.execute(params![
            id,
            file,
            s.kind.as_str(),
            s.name,
            s.owner,
            s.sig,
            s.sig_hash,
            s.body_hash,
            s.l0,
            s.l1,
            s.exported,
            s.lines,
            s.branches,
        ])?;
    }
    Ok(())
}

/// Resolves the file at `rel` and replaces its `edge`, `ref` and `reverse_import`
/// rows with the result. The file's `file` row must exist.
fn replace_resolved(tx: &Transaction, project: &Project, rel: &str) -> rusqlite::Result<()> {
    let id = file_id(rel);
    clear_resolved(tx, &id)?;
    let resolved = resolve_file(project, rel);
    let mut edge = tx.prepare_cached(
        "INSERT INTO edge(kind, src_id, dst_id, file_id, line, rule, prov, conf)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for e in &resolved.edges {
        edge.execute(params![
            e.kind, e.src_id, e.dst_id, id, e.line, e.rule, PROVENANCE, CONF_SURE
        ])?;
    }
    let mut reference = tx.prepare_cached(
        "INSERT INTO ref(file_id, kind, text, line, status) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for r in &resolved.refs {
        reference.execute(params![id, r.kind, r.text, r.line, r.status])?;
    }
    let mut reverse =
        tx.prepare_cached("INSERT INTO reverse_import(dst_file_id, src_file_id) VALUES (?1, ?2)")?;
    for dst in &resolved.imports_files {
        reverse.execute(params![file_id(dst), id])?;
    }
    Ok(())
}

fn load_stored(store: &Store) -> rusqlite::Result<BTreeMap<String, Stored>> {
    let mut stmt = store.conn().prepare(
        "SELECT path, class, lang, size, mtime_ns, content_hash, parse_status FROM file",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            Stored {
                class: row.get(1)?,
                lang: row.get(2)?,
                size: row.get(3)?,
                mtime_ns: row.get(4)?,
                content_hash: row.get(5)?,
                parse_status: row.get(6)?,
            },
        ))
    })?;
    rows.collect()
}

fn stored_paths(store: &Store) -> rusqlite::Result<BTreeSet<String>> {
    let mut stmt = store.conn().prepare("SELECT path FROM file")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect()
}

/// The facts of every stored file. A row whose JSON no longer reads is left out, so
/// the next pass parses that file again.
fn load_facts(store: &Store) -> rusqlite::Result<BTreeMap<String, FileFacts>> {
    let mut stmt = store
        .conn()
        .prepare("SELECT path, facts_json FROM file WHERE facts_json IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut facts = BTreeMap::new();
    for row in rows {
        let (path, json) = row?;
        if let Ok(parsed) = serde_json::from_str(&json) {
            facts.insert(path, parsed);
        }
    }
    Ok(facts)
}

fn counts(conn: &rusqlite::Connection, sql: &str) -> rusqlite::Result<BTreeMap<String, u32>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_confidence_constant_is_how_the_model_spells_sure() {
        let sure = serde_json::to_value(hm_core::model::Confidence::Sure).unwrap();
        assert_eq!(sure, super::CONF_SURE);
    }
}
