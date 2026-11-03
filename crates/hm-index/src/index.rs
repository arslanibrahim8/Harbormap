//! The indexer: a full pass that walks a worktree, parses what L1 understands,
//! resolves names across files and writes everything into the facts store in one
//! transaction (Learn spec §5). After a save, `apply` brings the store in line with
//! the changed files and with what depends on them, so that it always equals a cold
//! index of the same tree (spec §5.4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use hm_core::ids::file_id;
use hm_store::{Store, StoreError};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::facts::{FileFacts, ImportKind, ImportedName, ParseStatus, SymbolKind};
use crate::hash::{content_hash, facts_hash, interface_hash};
use crate::lang::Lang;
use crate::langs::{self, Resolvers, Workspace};
use crate::ts::resolve::{EdgeRow, ModuleTarget, symbol_ids};
use crate::walk::{
    FileClass, IGNORE_FILES, Skip, WalkRules, WalkedFile, is_unreadable, walk, walked_file,
};

/// Where an exact edge comes from.
const PROVENANCE: &str = "parsed";
/// Where a name-match edge comes from.
const PROV_INFERRED: &str = "inferred";
/// `Confidence::Sure` as the store spells it (checked by a test).
const CONF_SURE: &str = "sure";
/// `Confidence::Probably` as the store spells it (checked by a test).
const CONF_PROBABLY: &str = "probably";

/// The `(prov, conf)` an edge is stored with.
fn evidence(e: &EdgeRow) -> (&'static str, &'static str) {
    if e.inferred {
        (PROV_INFERRED, CONF_PROBABLY)
    } else {
        (PROVENANCE, CONF_SURE)
    }
}

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
    /// `too_large`, `binary` and `unreadable` files (`unreadable` counts folders that
    /// could not be read too), and `dir:<name>` for each kind of folder that was not
    /// entered.
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
    /// The ignore files the walk applied, `.git/info/exclude` included; relative and
    /// sorted.
    pub ignore_files: Vec<String>,
    /// What the pass could not follow in the project's configuration (a tsconfig
    /// `extends` that names a package, a config that does not parse), one line each,
    /// sorted.
    pub notes: Vec<String>,
    pub took_ms: u64,
}

/// What [`Indexer::apply`] or [`Indexer::rescan`] changed. Paths are relative,
/// symbols are ids, and every list is sorted. L3 marks journeys and cards from the
/// `symbols_*` lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Delta {
    /// Files whose content hash changed, new files included.
    pub files_changed: Vec<String>,
    /// Files that left the index: deleted, or no longer admitted by the walk.
    pub files_removed: Vec<String>,
    /// Files whose interface hash changed (or that gained or lost their facts).
    pub interface_changed: Vec<String>,
    /// Files resolved again because of a change elsewhere; the changed and removed
    /// files themselves are not listed.
    pub reresolved: Vec<String>,
    pub symbols_added: Vec<String>,
    pub symbols_removed: Vec<String>,
    /// Symbols that kept their id and got a new body hash.
    pub symbols_body_changed: Vec<String>,
    /// Symbols that kept their id and got a new signature hash.
    pub symbols_sig_changed: Vec<String>,
    /// Edge rows inserted and deleted. An edge whose rule alone changed counts as one
    /// removed and one added.
    pub edges_added: u32,
    pub edges_removed: u32,
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
    resolvers: Resolvers,
    /// The include rules of the last walk; `None` until this indexer has walked.
    rules: Option<WalkRules>,
    /// The folders the last walk admitted.
    dirs: Vec<String>,
}

impl Indexer {
    /// Wraps `store` for the worktree at `root`, loading what an earlier pass stored.
    pub fn open(root: &Path, store: Store) -> Result<Indexer> {
        let facts = load_facts(&store)?;
        let known = stored_paths(store.conn())?;
        Ok(Indexer {
            root: root.to_path_buf(),
            store,
            facts,
            resolvers: Resolvers::load(root, known),
            rules: None,
            dirs: Vec::new(),
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The include rules of the last walk, which `apply` filters with; `None` until
    /// this indexer has walked. A watcher must filter with the same rules.
    pub fn walk_rules(&self) -> Option<&WalkRules> {
        self.rules.as_ref()
    }

    /// The folders the last walk admitted (`""` is the root), for a watcher to watch.
    pub fn admitted_dirs(&self) -> &[String] {
        &self.dirs
    }

    /// Walks the worktree and brings the store in line with it. A file whose size and
    /// mtime match its row is left alone; one that differs is hashed, and parsed only
    /// when its content hash changed too. Every file is resolved again, because an
    /// edge depends on files other than its own. All writes share one transaction.
    pub fn full(&mut self) -> Result<IndexReport> {
        let started = Instant::now();
        let mut walked = walk(&self.root)?;
        let stored = load_stored(&self.store)?;
        let mut report = IndexReport {
            ignore_files: std::mem::take(&mut walked.ignore_files),
            ..IndexReport::default()
        };
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
        if !walked.unreadable.is_empty() {
            *report
                .skipped
                .entry(skip_name(Skip::Unreadable).to_string())
                .or_default() += walked.unreadable.len() as u32;
        }

        // Metadata decides what has to be read; the reading itself runs in parallel.
        let mut changes: Vec<Option<Change>> = Vec::new();
        let mut jobs: Vec<(usize, Job)> = Vec::new();
        for (i, f) in walked.files.iter().enumerate() {
            let old = stored.get(&f.rel);
            let have_facts = self.facts.contains_key(&f.rel);
            if old.is_some_and(|s| same_stat(s, f) && current(s, f, have_facts)) {
                changes.push(Some(Change::Unchanged));
            } else if let Some(skip) = f.skip {
                changes.push(Some(Change::Skipped(skip_name(skip))));
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
                    // A file the walk could open and the read could not.
                    if let (Change::Skipped(name), None) = (change, f.skip) {
                        *report.skipped.entry(name.to_string()).or_default() += 1;
                    }
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
        let resolvers = Resolvers::load(&self.root, known.clone());
        report.notes = resolvers.notes();

        let outcome = self.store.write(|tx| {
            for path in &vanished {
                tx.execute("DELETE FROM file WHERE path = ?1", [path])?;
            }
            for (f, change) in &live {
                write_change(tx, f, change)?;
            }
            let project = Workspace::new(&facts, &resolvers);
            if dirty {
                for rel in facts.keys() {
                    replace_resolved(tx, &project, rel)?;
                }
            }
            write_meta(tx, &self.root)
        });
        match outcome {
            Ok(()) => {
                self.facts = facts;
                self.resolvers = resolvers;
                self.rules = Some(walked.rules);
                self.dirs = walked.dirs;
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

    /// Brings the store in line with the files at `changed` (relative paths) and with
    /// what depends on them, in one transaction. A path that exists is always read
    /// and hashed: coarse mtimes miss fast saves of the same size. Paths the walk
    /// rules reject are ignored. A path that no longer exists removes its file, or
    /// every indexed file under it when it was a folder. A folder that exists (moved
    /// or copied in) is walked with the same rules. A changed ignore file, or an
    /// indexer that has not walked yet, makes this a [`rescan`](Indexer::rescan).
    pub fn apply(&mut self, changed: &BTreeSet<String>) -> Result<Delta> {
        let Some(rules) = &self.rules else {
            return self.rescan();
        };
        if changed
            .iter()
            .any(|rel| rel == GIT_EXCLUDE || (is_ignore_file(rel) && rules.admits(rel)))
        {
            return self.rescan();
        }

        let mut names = Names::new(&self.root);
        let mut paths = BTreeSet::new();
        for rel in changed.iter().filter(|rel| !rel.is_empty()) {
            if !names.is_file(rel)? {
                // A folder that vanished, moved or replaced a file: its stored files
                // are read again or removed. An ignore file among them took its rules
                // along, so that needs a walk.
                let under = stored_under(&self.store, rel)?;
                if under.iter().any(|rel| is_ignore_file(rel)) {
                    return self.rescan();
                }
                paths.extend(under);
            }
            if names.is_dir(rel)? && rules.admits_dir(rel) {
                // A folder moved or copied in: its own ignore file would change the
                // rules, so that needs a walk.
                if files_under(&self.root, rules, rel, &mut paths)? {
                    return self.rescan();
                }
            }
            paths.insert(rel.clone());
        }
        let stored = load_stored_at(&self.store, &paths)?;
        let mut files = Vec::new();
        let mut gone = Vec::new();
        for rel in paths {
            if names.is_file(&rel)? && rules.admits(&rel) {
                match walked_file(rel.clone(), &abs_path(&self.root, &rel)) {
                    Ok(f) => files.push(f),
                    // A file that cannot even be stat-ed is not in a walk either.
                    Err(err) if is_gone(&err) || is_unreadable(&err) => gone.push(rel),
                    Err(err) => return Err(err.into()),
                }
            } else if stored.contains_key(&rel) {
                gone.push(rel);
            }
        }
        self.bring_in_line(files, gone, &stored, None)
    }

    /// Walks the worktree and brings the store in line with it, like
    /// [`full`](Indexer::full), but says what changed. A file whose size and mtime
    /// match its row is not read. For a branch switch or lost watcher events.
    pub fn rescan(&mut self) -> Result<Delta> {
        let walked = walk(&self.root)?;
        let stored = load_stored(&self.store)?;
        let known: BTreeSet<String> = walked.files.iter().map(|f| f.rel.clone()).collect();
        let gone: Vec<String> = stored
            .keys()
            .filter(|rel| !known.contains(*rel))
            .cloned()
            .collect();
        let files: Vec<WalkedFile> = walked
            .files
            .into_iter()
            .filter(|f| {
                let have_facts = self.facts.contains_key(&f.rel);
                !stored
                    .get(&f.rel)
                    .is_some_and(|s| same_stat(s, f) && current(s, f, have_facts))
            })
            .collect();
        let walk = Rescan {
            known,
            rules: walked.rules,
            dirs: walked.dirs,
        };
        self.bring_in_line(files, gone, &stored, Some(walk))
    }

    /// Reads `files`, removes `gone`, and resolves again what the changes reach, in
    /// one transaction. `stored` holds the rows of the paths that have one. With
    /// `rescan`, any change resolves every file again, as [`full`](Indexer::full)
    /// does; without it, only the files a change can reach (spec §5.4).
    fn bring_in_line(
        &mut self,
        files: Vec<WalkedFile>,
        gone: Vec<String>,
        stored: &BTreeMap<String, Stored>,
        rescan: Option<Rescan>,
    ) -> Result<Delta> {
        // 1. Read and hash every file; parse those whose bytes changed.
        let mut changes: Vec<Option<Change>> = Vec::new();
        let mut jobs: Vec<(usize, Job)> = Vec::new();
        for (i, f) in files.iter().enumerate() {
            let old = stored.get(&f.rel);
            let have_facts = self.facts.contains_key(&f.rel);
            match f.skip {
                // A skipped file is never read; its row says all there is.
                Some(skip) => changes.push(Some(match old {
                    Some(s) if current(s, f, have_facts) => {
                        if same_stat(s, f) {
                            Change::Unchanged
                        } else {
                            Change::Touched
                        }
                    }
                    _ => Change::Skipped(skip_name(skip)),
                })),
                None => {
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
        }
        for (i, result) in run_jobs(&jobs) {
            changes[i] = Some(result?);
        }
        let mut gone = gone;
        let mut writes: Vec<(&WalkedFile, Change)> = Vec::new();
        for (f, change) in files.iter().zip(changes) {
            match change.expect("every file has a change") {
                Change::Unchanged => {}
                Change::Gone => gone.push(f.rel.clone()),
                change => writes.push((f, change)),
            }
        }
        let gone: BTreeSet<String> = gone.into_iter().collect();
        let removed: Vec<String> = gone
            .iter()
            .filter(|rel| stored.contains_key(*rel))
            .cloned()
            .collect();

        // 2. The new facts, and what changed in them.
        let mut delta = Delta::default();
        let mut facts = std::mem::take(&mut self.facts);
        let mut rewritten: BTreeSet<String> = BTreeSet::new();
        let mut added: Vec<String> = Vec::new();
        // Files whose change other files can see, so their importers resolve again.
        let mut seeds: Vec<String> = Vec::new();
        // Names that files may reach by name alone and whose symbols changed.
        let mut renamed: BTreeSet<String> = BTreeSet::new();
        // A stored file that gains its facts: Go importers hold no row for it.
        let mut gained = false;
        for (f, change) in &writes {
            let hash = match change {
                Change::Fresh { hash, .. } => hash.as_str(),
                Change::Skipped(_) => "",
                _ => continue,
            };
            let rel = &f.rel;
            let old_hash = stored.get(rel).map(|s| s.content_hash.as_str());
            if old_hash.is_none() {
                added.push(rel.clone());
            }
            if old_hash != Some(hash) {
                delta.files_changed.push(rel.clone());
            }
            let old = facts.remove(rel);
            let new = match change {
                Change::Fresh {
                    parsed: Some(p), ..
                } => Some(p.facts.clone()),
                _ => None,
            };
            diff_symbols(rel, old.as_ref(), new.as_ref(), &mut delta);
            let interface = old.as_ref().map(interface_hash) != new.as_ref().map(interface_hash);
            if interface {
                delta.interface_changed.push(rel.clone());
            }
            if interface
                || old.as_ref().map(|f| Surface::of(rel, f))
                    != new.as_ref().map(|f| Surface::of(rel, f))
            {
                seeds.push(rel.clone());
            }
            renamed.extend(langs::inferred_changes(rel, old.as_ref(), new.as_ref()));
            gained |= old_hash.is_some() && old.is_none() && new.is_some();
            if let Some(new) = new {
                facts.insert(rel.clone(), new);
            }
            rewritten.insert(rel.clone());
        }
        for rel in &removed {
            let old = facts.remove(rel);
            diff_symbols(rel, old.as_ref(), None, &mut delta);
            renamed.extend(langs::inferred_changes(rel, old.as_ref(), None));
        }

        // 3. Whether the resolver is rebuilt, which resolves every file again: a new
        // or removed file can change where any specifier points, and so can a config
        // file. A rescan does so on any change, as `full` does.
        let sources = self.resolvers.config_sources();
        let config = delta
            .files_changed
            .iter()
            .chain(&removed)
            .any(|rel| sources.contains(rel));
        let rebuild = match &rescan {
            Some(_) => !rewritten.is_empty() || !removed.is_empty(),
            None => !added.is_empty() || !removed.is_empty() || config || gained,
        };

        // 4. One transaction: the files to resolve again, removals, file and symbol
        // rows, then the resolved rows of those files, written as differences.
        let (root, resolvers) = (&self.root, &self.resolvers);
        let outcome = self.store.write(|tx| {
            let rebuilt = if rebuild {
                let known = match &rescan {
                    Some(walk) => walk
                        .known
                        .iter()
                        .filter(|rel| !gone.contains(*rel))
                        .cloned()
                        .collect(),
                    None => {
                        let mut known = stored_paths(tx)?;
                        known.retain(|rel| !gone.contains(rel));
                        known.extend(added.iter().cloned());
                        known
                    }
                };
                Some(Resolvers::load(root, known))
            } else {
                None
            };
            let project = Workspace::new(&facts, rebuilt.as_ref().unwrap_or(resolvers));
            // Read before anything is written: the importers as the last pass stored them.
            let resolve: BTreeSet<String> = if rebuilt.is_some() {
                facts.keys().cloned().collect()
            } else {
                // Files that read a seed with no row to it count as seeds too.
                let readers = langs::implicit_readers(&project, &seeds);
                let mut sources = seeds.clone();
                sources.extend(readers.iter().cloned());
                let mut resolve = dependents(tx, &project, &sources)?;
                resolve.extend(readers);
                for rel in &seeds {
                    resolve.extend(langs::implicit_dependents(&project, rel));
                }
                if !renamed.is_empty() {
                    resolve.extend(
                        facts
                            .iter()
                            .filter(|(rel, f)| langs::reads_names(rel, f, &renamed))
                            .map(|(rel, _)| rel.clone()),
                    );
                }
                resolve.extend(
                    rewritten
                        .iter()
                        .filter(|rel| facts.contains_key(*rel))
                        .cloned(),
                );
                resolve
            };

            let mut edges = (0, 0);
            for rel in &removed {
                edges.1 += count_edges(tx, &file_id(rel))?;
                tx.execute("DELETE FROM file WHERE path = ?1", [rel])?;
            }
            for (f, change) in &writes {
                // `write_change` clears the rows of a file that is no longer parsed.
                if matches!(
                    change,
                    Change::Skipped(_) | Change::Fresh { parsed: None, .. }
                ) {
                    edges.1 += count_edges(tx, &file_id(&f.rel))?;
                }
                write_change(tx, f, change)?;
            }
            for rel in &resolve {
                let (added, removed) = sync_resolved(tx, &project, rel)?;
                edges.0 += added;
                edges.1 += removed;
            }
            if rescan.is_some() {
                write_meta(tx, root)?;
            }
            Ok((edges, resolve, rebuilt))
        });
        let ((edges_added, edges_removed), resolve, rebuilt) = match outcome {
            Ok(done) => done,
            Err(err) => {
                // The map was partly rebuilt; the store is the truth (see `full`).
                if let Ok(facts) = load_facts(&self.store) {
                    self.facts = facts;
                }
                return Err(err.into());
            }
        };
        self.facts = facts;
        if let Some(resolvers) = rebuilt {
            self.resolvers = resolvers;
        }
        if let Some(walk) = rescan {
            self.rules = Some(walk.rules);
            self.dirs = walk.dirs;
        }

        delta.files_removed = removed;
        delta.reresolved = resolve
            .into_iter()
            .filter(|rel| !rewritten.contains(rel))
            .collect();
        delta.edges_added = edges_added;
        delta.edges_removed = edges_removed;
        for list in [
            &mut delta.files_changed,
            &mut delta.files_removed,
            &mut delta.interface_changed,
            &mut delta.symbols_added,
            &mut delta.symbols_removed,
            &mut delta.symbols_body_changed,
            &mut delta.symbols_sig_changed,
        ] {
            list.sort();
        }
        Ok(delta)
    }
}

/// `parse_status` of a file in a language that L1 does not parse.
const NOT_SUPPORTED: &str = "not_supported_yet";

fn skip_name(skip: Skip) -> &'static str {
    match skip {
        Skip::TooLarge => "too_large",
        Skip::Binary => "binary",
        Skip::Unreadable => "unreadable",
    }
}

/// Whether L1 parses the file: a code file in a supported language, not a doc or an
/// infrastructure file.
fn parses(f: &WalkedFile) -> bool {
    f.skip.is_none()
        && !matches!(f.class, FileClass::Doc | FileClass::Infra)
        && f.lang.is_some_and(|l| l.parsed())
}

/// The `parse_status` of a file that is read but not parsed.
fn unparsed_status(f: &WalkedFile) -> &'static str {
    match f.lang {
        Some(lang) if !matches!(f.class, FileClass::Doc | FileClass::Infra) && !lang.parsed() => {
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
        // Readable when the walk sniffed it, but not any more.
        Err(err) if is_unreadable(&err) => return Ok(Change::Skipped("unreadable")),
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
    let facts = crate::langs::extract(&String::from_utf8_lossy(&bytes), lang);
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
fn replace_resolved(tx: &Transaction, project: &Workspace, rel: &str) -> rusqlite::Result<()> {
    let id = file_id(rel);
    clear_resolved(tx, &id)?;
    let resolved = langs::resolve_file(project, rel);
    let mut edge = tx.prepare_cached(
        "INSERT INTO edge(kind, src_id, dst_id, file_id, line, rule, prov, conf)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for e in &resolved.edges {
        let (prov, conf) = evidence(e);
        edge.execute(params![
            e.kind, e.src_id, e.dst_id, id, e.line, e.rule, prov, conf
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

/// The repository's own exclude file, which the walk honours like an ignore file.
const GIT_EXCLUDE: &str = ".git/info/exclude";

/// Whether `rel` names a `.gitignore` or `.harbormapignore`.
fn is_ignore_file(rel: &str) -> bool {
    IGNORE_FILES.contains(&rel.rsplit('/').next().unwrap_or(rel))
}

/// Adds the files under the folder `dir` that `rules` admit to `out`, entering only
/// the folders it admits and following no symlinks, as the walk does. Returns
/// whether an admitted ignore file was found among them.
fn files_under(
    root: &Path,
    rules: &WalkRules,
    dir: &str,
    out: &mut BTreeSet<String>,
) -> io::Result<bool> {
    let entries = match fs::read_dir(abs_path(root, dir)) {
        Ok(entries) => entries,
        // The walk counts a folder it may not read and sees nothing in it.
        Err(err) if is_gone(&err) || is_unreadable(&err) => return Ok(false),
        Err(err) => return Err(err),
    };
    let mut ignore_file = false;
    for entry in entries {
        let entry = entry?;
        // The walk cannot name a file whose name is not UTF-8 either.
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let rel = format!("{dir}/{name}");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if rules.admits_dir(&rel) {
                ignore_file |= files_under(root, rules, &rel, out)?;
            }
        } else if file_type.is_file() && rules.admits(&rel) {
            ignore_file |= is_ignore_file(&rel);
            out.insert(rel);
        }
    }
    Ok(ignore_file)
}

/// What a rescan walked: the files that exist, the rules that admitted them and the
/// folders it entered.
struct Rescan {
    known: BTreeSet<String>,
    rules: WalkRules,
    dirs: Vec<String>,
}

/// Whether the stored row is what a pass would write for `f`, size and mtime aside.
fn current(s: &Stored, f: &WalkedFile, have_facts: bool) -> bool {
    match f.skip {
        Some(skip) => s.parse_status == skip_name(skip) && same_kind(s, f),
        None => fits(s, f, have_facts),
    }
}

fn write_meta(tx: &Transaction, root: &Path) -> rusqlite::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let root = root.to_string_lossy();
    for (key, value) in [("indexed_at", now.to_string()), ("root", root.to_string())] {
        tx.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
}

/// The path of `rel` (`/`-separated) under `root`.
fn abs_path(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// A path that names nothing any more, or names a file inside what is now a file.
fn is_gone(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// Says whether paths under a root exist, spelled exactly as given. On a file system
/// that ignores case, `src/button.ts` still stats after a rename to `src/Button.ts`;
/// only the listing of `src` tells the two names apart. Listings are read once.
struct Names<'a> {
    root: &'a Path,
    /// The names in each folder read so far (relative, `""` is the root); `None`
    /// when the folder cannot be listed.
    listings: BTreeMap<String, Option<BTreeSet<String>>>,
}

impl<'a> Names<'a> {
    fn new(root: &'a Path) -> Self {
        Names {
            root,
            listings: BTreeMap::new(),
        }
    }

    /// Whether `rel` is a regular file under this spelling (see [`is_file`]).
    fn is_file(&mut self, rel: &str) -> io::Result<bool> {
        Ok(is_file(&abs_path(self.root, rel))? && self.spelled(rel))
    }

    /// Whether `rel` is a folder under this spelling (see [`is_dir`]).
    fn is_dir(&mut self, rel: &str) -> io::Result<bool> {
        Ok(is_dir(&abs_path(self.root, rel))? && self.spelled(rel))
    }

    /// Whether each part of `rel` is listed, with exactly its spelling, in the
    /// folder above it.
    fn spelled(&mut self, rel: &str) -> bool {
        let mut dir = String::new();
        for part in rel.split('/') {
            let root = self.root;
            let listing = self.listings.entry(dir.clone()).or_insert_with(|| {
                let entries = fs::read_dir(abs_path(root, &dir)).ok()?;
                Some(
                    entries
                        .flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect(),
                )
            });
            if !listing.as_ref().is_some_and(|names| names.contains(part)) {
                return false;
            }
            if !dir.is_empty() {
                dir.push('/');
            }
            dir.push_str(part);
        }
        true
    }
}

/// Whether `abs` is a regular file. Like the walk, symlinks are not followed, and a
/// path inside a folder that may not be read is not seen.
fn is_file(abs: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(abs) {
        Ok(meta) => Ok(meta.is_file()),
        Err(err) if is_gone(&err) || is_unreadable(&err) => Ok(false),
        Err(err) => Err(err),
    }
}

/// Whether `abs` is a folder (not a symlink to one).
fn is_dir(abs: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(abs) {
        Ok(meta) => Ok(meta.is_dir()),
        Err(err) if is_gone(&err) || is_unreadable(&err) => Ok(false),
        Err(err) => Err(err),
    }
}

/// What other files read of one file's facts when they are resolved: its symbol
/// ids, each symbol's owner, name, kind and export flags, its exports, and the
/// imports that an `export { a }` hands on. Lines are left out, so a body edit that
/// moves code does not reach importers.
#[derive(PartialEq)]
struct Surface<'a> {
    ids: Vec<String>,
    symbols: Vec<(&'a str, &'a str, SymbolKind, bool, bool)>,
    exports: Vec<(&'a str, Option<&'a str>, Option<&'a str>, bool)>,
    forwarded: Vec<(&'a str, ImportKind, &'a [ImportedName], bool)>,
    /// `(class, base)` of a Python file: subclasses elsewhere inherit through them.
    heritage: Vec<(&'a str, &'a str)>,
}

impl<'a> Surface<'a> {
    fn of(rel: &str, facts: &'a FileFacts) -> Surface<'a> {
        // Any name a Python file imports can be imported from it in turn.
        let python = Lang::from_path(Path::new(rel)) == Some(Lang::Python);
        let locals: Vec<&str> = facts
            .exports
            .iter()
            .filter(|e| !e.star && e.from.is_none())
            .map(|e| e.local.as_deref().unwrap_or(&e.name))
            .collect();
        Surface {
            ids: symbol_ids(rel, facts),
            symbols: facts
                .symbols
                .iter()
                .map(|s| {
                    (
                        s.owner.as_str(),
                        s.name.as_str(),
                        s.kind,
                        s.exported,
                        s.default_export,
                    )
                })
                .collect(),
            exports: facts
                .exports
                .iter()
                .map(|e| {
                    (
                        e.name.as_str(),
                        e.local.as_deref(),
                        e.from.as_deref(),
                        e.star,
                    )
                })
                .collect(),
            forwarded: facts
                .imports
                .iter()
                .filter(|i| {
                    python
                        || (i.kind != ImportKind::ReExport
                            && i.names.iter().any(|n| locals.contains(&n.local.as_str())))
                })
                .map(|i| {
                    (
                        i.specifier.as_str(),
                        i.kind,
                        i.names.as_slice(),
                        i.type_only,
                    )
                })
                .collect(),
            heritage: facts
                .heritage
                .iter()
                .filter(|_| python)
                .map(|h| (h.class.as_str(), h.extends.as_str()))
                .collect(),
        }
    }
}

/// Adds the symbol changes of the file at `rel` from `old` to `new` facts to `delta`.
fn diff_symbols(rel: &str, old: Option<&FileFacts>, new: Option<&FileFacts>, delta: &mut Delta) {
    let by_id = |facts: Option<&FileFacts>| -> BTreeMap<String, (String, String)> {
        facts.map_or_else(BTreeMap::new, |f| {
            symbol_ids(rel, f)
                .into_iter()
                .zip(&f.symbols)
                .map(|(id, s)| (id, (s.sig_hash.clone(), s.body_hash.clone())))
                .collect()
        })
    };
    let (old, new) = (by_id(old), by_id(new));
    for (id, (sig, body)) in &new {
        match old.get(id) {
            None => delta.symbols_added.push(id.clone()),
            Some((old_sig, old_body)) => {
                if old_sig != sig {
                    delta.symbols_sig_changed.push(id.clone());
                }
                if old_body != body {
                    delta.symbols_body_changed.push(id.clone());
                }
            }
        }
    }
    delta
        .symbols_removed
        .extend(old.into_keys().filter(|id| !new.contains_key(id)));
}

/// How many files a change travels through files that hand names on: as many as the
/// re-export hops `resolve_export` follows (spec §6.2).
const FORWARD_HOPS: usize = 5;

/// The files whose resolution can read the facts of a file in `seeds`: their
/// importers, and the importers of every importer that hands a name on from one
/// (a re-export, or an `export { a }` of an import), for up to [`FORWARD_HOPS`] hops.
fn dependents(
    conn: &rusqlite::Connection,
    project: &Workspace,
    seeds: &[String],
) -> rusqlite::Result<BTreeSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.path FROM reverse_import r JOIN file f ON f.id = r.src_file_id
         WHERE r.dst_file_id = ?1",
    )?;
    let mut out = BTreeSet::new();
    let mut seen: BTreeSet<String> = seeds.iter().cloned().collect();
    let mut level = seeds.to_vec();
    for hop in 0..=FORWARD_HOPS {
        let mut next = Vec::new();
        for target in &level {
            let importers = stmt
                .query_map([file_id(target)], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for importer in importers {
                if hop < FORWARD_HOPS
                    && forwards(project, &importer, target)
                    && seen.insert(importer.clone())
                {
                    next.push(importer.clone());
                }
                out.insert(importer);
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    Ok(out)
}

/// Whether resolving a name exported by `rel` can lead on into `target`: `rel`
/// re-exports from it, or exports a name that it imports from it. A Python file does
/// when one of its imports leads to `target`; a Go package hands no names on.
fn forwards(w: &Workspace, rel: &str, target: &str) -> bool {
    match Lang::from_path(Path::new(rel)) {
        Some(Lang::Python) => return crate::py::resolve::forwards(&w.py(), rel, target),
        Some(Lang::Go) => return false,
        _ => {}
    }
    let p = w.ts();
    let Some(facts) = p.facts.get(rel) else {
        return false;
    };
    let into =
        |spec: &str| matches!(p.modules.resolve(rel, spec), ModuleTarget::File(f) if f == target);
    facts.exports.iter().any(|e| match &e.from {
        Some(spec) => into(spec),
        None => {
            let local = e.local.as_deref().unwrap_or(&e.name);
            facts.imports.iter().any(|i| {
                i.kind != ImportKind::ReExport
                    && i.names.iter().any(|n| n.local == local)
                    && into(&i.specifier)
            })
        }
    })
}

fn count_edges(tx: &Transaction, id: &str) -> rusqlite::Result<u32> {
    tx.prepare_cached("SELECT count(*) FROM edge WHERE file_id = ?1")?
        .query_row([id], |row| row.get(0))
}

/// Resolves the file at `rel` and writes only the difference from its stored
/// `edge`, `ref` and `reverse_import` rows, so resolving a file again whose result
/// did not change writes nothing. Returns the number of edges added and removed.
fn sync_resolved(tx: &Transaction, project: &Workspace, rel: &str) -> rusqlite::Result<(u32, u32)> {
    let id = file_id(rel);
    let resolved = langs::resolve_file(project, rel);

    type EdgeKey = (String, String, String, u32, String, String, String);
    let old_edges: BTreeSet<EdgeKey> = tx
        .prepare_cached(
            "SELECT kind, src_id, dst_id, line, rule, prov, conf FROM edge WHERE file_id = ?1",
        )?
        .query_map([&id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let new_edges: BTreeSet<EdgeKey> = resolved
        .edges
        .iter()
        .map(|e| {
            let (prov, conf) = evidence(e);
            (
                e.kind.to_string(),
                e.src_id.clone(),
                e.dst_id.clone(),
                e.line,
                e.rule.to_string(),
                prov.to_string(),
                conf.to_string(),
            )
        })
        .collect();
    let mut delete = tx.prepare_cached(
        "DELETE FROM edge WHERE kind = ?1 AND src_id = ?2 AND dst_id = ?3 AND line = ?4",
    )?;
    let gone: Vec<_> = old_edges.difference(&new_edges).collect();
    for (kind, src, dst, line, ..) in &gone {
        delete.execute(params![kind, src, dst, line])?;
    }
    let mut insert = tx.prepare_cached(
        "INSERT INTO edge(kind, src_id, dst_id, file_id, line, rule, prov, conf)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let fresh: Vec<_> = new_edges.difference(&old_edges).collect();
    for (kind, src, dst, line, rule, prov, conf) in &fresh {
        insert.execute(params![kind, src, dst, id, line, rule, prov, conf])?;
    }

    let old_refs: BTreeSet<(String, String, u32, String)> = tx
        .prepare_cached("SELECT kind, text, line, status FROM ref WHERE file_id = ?1")?
        .query_map([&id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let new_refs: BTreeSet<(String, String, u32, String)> = resolved
        .refs
        .iter()
        .map(|r| {
            (
                r.kind.to_string(),
                r.text.clone(),
                r.line,
                r.status.to_string(),
            )
        })
        .collect();
    let mut delete = tx.prepare_cached(
        "DELETE FROM ref WHERE file_id = ?1 AND kind = ?2 AND text = ?3 AND line = ?4",
    )?;
    for (kind, text, line, _) in old_refs.difference(&new_refs) {
        delete.execute(params![id, kind, text, line])?;
    }
    let mut insert = tx.prepare_cached(
        "INSERT INTO ref(file_id, kind, text, line, status) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (kind, text, line, status) in new_refs.difference(&old_refs) {
        insert.execute(params![id, kind, text, line, status])?;
    }

    let old_imports: BTreeSet<String> = tx
        .prepare_cached("SELECT dst_file_id FROM reverse_import WHERE src_file_id = ?1")?
        .query_map([&id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let new_imports: BTreeSet<String> = resolved.imports_files.iter().map(|f| file_id(f)).collect();
    let mut delete = tx
        .prepare_cached("DELETE FROM reverse_import WHERE dst_file_id = ?1 AND src_file_id = ?2")?;
    for dst in old_imports.difference(&new_imports) {
        delete.execute(params![dst, id])?;
    }
    let mut insert =
        tx.prepare_cached("INSERT INTO reverse_import(dst_file_id, src_file_id) VALUES (?1, ?2)")?;
    for dst in new_imports.difference(&old_imports) {
        insert.execute(params![dst, id])?;
    }
    Ok((fresh.len() as u32, gone.len() as u32))
}

/// The columns `stored_row` reads.
const STORED_COLUMNS: &str = "path, class, lang, size, mtime_ns, content_hash, parse_status";

fn stored_row(row: &rusqlite::Row) -> rusqlite::Result<(String, Stored)> {
    Ok((
        row.get(0)?,
        Stored {
            class: row.get(1)?,
            lang: row.get(2)?,
            size: row.get(3)?,
            mtime_ns: row.get(4)?,
            content_hash: row.get(5)?,
            parse_status: row.get(6)?,
        },
    ))
}

fn load_stored(store: &Store) -> rusqlite::Result<BTreeMap<String, Stored>> {
    let mut stmt = store
        .conn()
        .prepare(&format!("SELECT {STORED_COLUMNS} FROM file"))?;
    let rows = stmt.query_map([], stored_row)?;
    rows.collect()
}

/// The stored rows of `paths`, for those that have one.
fn load_stored_at(
    store: &Store,
    paths: &BTreeSet<String>,
) -> rusqlite::Result<BTreeMap<String, Stored>> {
    let mut stmt = store.conn().prepare(&format!(
        "SELECT {STORED_COLUMNS} FROM file WHERE path = ?1"
    ))?;
    let mut out = BTreeMap::new();
    for path in paths {
        if let Some((path, row)) = stmt.query_row([path], stored_row).optional()? {
            out.insert(path, row);
        }
    }
    Ok(out)
}

/// The stored paths under the folder `dir`.
fn stored_under(store: &Store, dir: &str) -> rusqlite::Result<Vec<String>> {
    // Every path that starts with `dir/` sorts between `dir/` and `dir0`.
    let mut stmt = store
        .conn()
        .prepare("SELECT path FROM file WHERE path >= ?1 AND path < ?2")?;
    let rows = stmt.query_map([format!("{dir}/"), format!("{dir}0")], |row| row.get(0))?;
    rows.collect()
}

fn stored_paths(conn: &rusqlite::Connection) -> rusqlite::Result<BTreeSet<String>> {
    let mut stmt = conn.prepare("SELECT path FROM file")?;
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

    #[test]
    fn the_confidence_constants_are_how_the_model_spells_them() {
        let sure = serde_json::to_value(hm_core::model::Confidence::Sure).unwrap();
        assert_eq!(sure, super::CONF_SURE);
        let probably = serde_json::to_value(hm_core::model::Confidence::Probably).unwrap();
        assert_eq!(probably, super::CONF_PROBABLY);
    }
}
