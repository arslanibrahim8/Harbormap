//! Derives routes, client calls, inferred edges, patterns and gaps from the facts of every
//! file and the `package.json` manifests (Learn spec §6.3). It is a pure function of its
//! inputs: no file is read here, and every list it returns is sorted.
//!
//! Each framework lives in its own module: `next` (Next.js routes, server actions and
//! middleware), `entries` (`package.json` `bin`, `main`, `scripts.start`), `express`,
//! `nest`, `hops` (client calls and mediated edges) and `patterns`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::hash::Hash;
use std::path::Path;

use hm_core::ids::route_id;
use serde::Serialize;

use crate::facts::FileFacts;
use crate::index::parses;
use crate::manifest::{ManifestCache, Manifests};
use crate::ts::resolve::{ModuleResolver, ModuleTarget, Project, holder, symbol_ids};
use crate::ts::tsconfig::load_tsconfigs;
use crate::walk::walk;

mod entries;
mod express;
mod hops;
mod nest;
mod next;
mod patterns;

/// An entry point: a page, an HTTP handler, a server action, a middleware, a CLI command or
/// a `main`/`start` entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Route {
    /// `r_` + BLAKE3 over `(root, entry_kind, method, pattern)`.
    pub id: String,
    /// The folder of the governing `package.json`; `""` for the repository root.
    pub root: String,
    /// `page`, `http`, `server_action`, `middleware`, `cli` or `main`.
    pub entry_kind: String,
    /// An upper-case HTTP verb, `ANY`, `ACTION`, `BIN`, `MAIN` or `START`.
    pub method: String,
    pub pattern: String,
    /// A symbol id, a file id when the file itself is the handler, or `None`.
    pub handler_id: Option<String>,
    /// The file that declares it.
    pub file: String,
    pub line: u32,
    pub rule: String,
    pub prov: String,
    pub conf: String,
    /// A JSON object with what the rule adds, such as the middleware matcher.
    pub detail: Option<String>,
}

/// A call from client code to a URL, matched or not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ClientCall {
    pub file: String,
    pub src_id: String,
    pub line: u32,
    /// Upper-case; `ANY` when unknown.
    pub method: String,
    pub url: String,
    pub rule: String,
}

/// An edge the rules infer; it is stored with `prov = inferred`, `conf = probably`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct InferredEdge {
    pub kind: String,
    pub src_id: String,
    pub dst_id: String,
    pub file: String,
    pub line: u32,
    pub rule: String,
}

/// A recognised pattern (spec §7.3) on a symbol or a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Pattern {
    pub file: String,
    pub subject_id: String,
    pub kind: String,
    pub line: u32,
    pub rule: String,
    pub conf: String,
    pub term: Option<String>,
    /// A JSON object or plain text with what the rule adds; for `env_read`, the names of
    /// the variables read, sorted and joined by `,` (`DATABASE_URL,NODE_ENV`).
    pub detail: Option<String>,
}

/// A place where a rule could not read what it needs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Gap {
    pub file: String,
    pub subject_id: String,
    pub kind: String,
    pub line: u32,
}

/// Everything `derive` finds. Every list is sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Derived {
    pub routes: Vec<Route>,
    pub client_calls: Vec<ClientCall>,
    pub edges: Vec<InferredEdge>,
    pub patterns: Vec<Pattern>,
    pub gaps: Vec<Gap>,
    /// One line each, such as a route dropped because its id is taken.
    pub notes: Vec<String>,
}

/// What `derive` reads: the facts of every parsed file, the resolver for their imports,
/// and the manifests.
pub struct Inputs<'a> {
    pub project: &'a Project<'a>,
    pub manifests: &'a Manifests,
}

/// Routes, client calls, inferred edges, patterns and gaps of the whole tree.
pub fn derive(inputs: &Inputs) -> Derived {
    let mut out = Derived::default();
    next::routes(inputs, &mut out);
    entries::routes(inputs, &mut out);
    express::routes(inputs, &mut out);
    nest::routes(inputs, &mut out);
    drop_duplicate_routes(&mut out);

    next::middleware_edges(inputs, &mut out);
    express::before_edges(inputs, &mut out);
    nest::before_edges(inputs, &mut out);
    hops::derive(inputs, &mut out);
    patterns::derive(inputs, &mut out);

    out.routes.sort();
    out.client_calls.sort();
    out.edges.sort();
    out.patterns.sort();
    out.gaps.sort();
    out.notes.sort();
    out.notes.dedup();
    // One row per store primary key, the first in sorted order, as the store keeps it.
    keep_first(&mut out.client_calls, |c| {
        (
            c.file.clone(),
            c.line,
            c.method.clone(),
            c.url.clone(),
            c.rule.clone(),
        )
    });
    keep_first(&mut out.edges, |e| {
        (e.kind.clone(), e.src_id.clone(), e.dst_id.clone(), e.line)
    });
    keep_first(&mut out.patterns, |p| {
        (
            p.file.clone(),
            p.subject_id.clone(),
            p.kind.clone(),
            p.line,
            p.rule.clone(),
        )
    });
    keep_first(&mut out.gaps, |g| (g.file.clone(), g.kind.clone(), g.line));
    out
}

/// Keeps the first item of each `key`, in order.
fn keep_first<T, K: Eq + Hash>(items: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(key(item)));
}

/// Keeps the first route of each id in `(file, line)` order; each later one is a note.
fn drop_duplicate_routes(out: &mut Derived) {
    out.routes
        .sort_by(|a, b| (&a.file, a.line, &a.id).cmp(&(&b.file, b.line, &b.id)));
    let mut kept: HashMap<String, (String, u32)> = HashMap::new();
    let mut notes = Vec::new();
    out.routes.retain(|r| {
        if let Some((file, line)) = kept.get(&r.id) {
            notes.push(format!(
                "{}:{}: {} {} is also declared at {file}:{line}",
                r.file, r.line, r.method, r.pattern
            ));
            return false;
        }
        kept.insert(r.id.clone(), (r.file.clone(), r.line));
        true
    });
    out.notes.extend(notes);
}

/// Walks `root`, extracts every file the indexer parses, and derives, without a store.
/// For tests and tools.
#[doc(hidden)]
pub fn derive_tree(root: &Path) -> Derived {
    let Ok(walked) = walk(root) else {
        return Derived::default();
    };
    let known: BTreeSet<String> = walked.files.iter().map(|f| f.rel.clone()).collect();
    let mut facts: BTreeMap<String, FileFacts> = BTreeMap::new();
    for f in &walked.files {
        // The same files the indexer parses.
        let Some(lang) = f.lang.filter(|_| parses(f)) else {
            continue;
        };
        if let Ok(bytes) = fs::read(&f.abs) {
            facts.insert(
                f.rel.clone(),
                crate::ts::extract(&String::from_utf8_lossy(&bytes), lang),
            );
        }
    }
    let tsconfigs = load_tsconfigs(root, &known);
    let manifests = Manifests::load(root, &known, &mut ManifestCache::default());
    let modules = ModuleResolver::new(known, tsconfigs);
    let project = Project {
        facts: &facts,
        modules: &modules,
    };
    derive(&Inputs {
        project: &project,
        manifests: &manifests,
    })
}

/// A route with `handler_id` and `detail` unset, `prov = matched` and `conf = sure`; the
/// caller sets what differs. The id comes from `(root, kind, method, pattern)`.
pub(crate) fn route(
    root: &str,
    kind: &str,
    method: &str,
    pattern: &str,
    file: &str,
    line: u32,
    rule: &str,
) -> Route {
    Route {
        id: route_id(root, kind, method, pattern),
        root: root.to_string(),
        entry_kind: kind.to_string(),
        method: method.to_string(),
        pattern: pattern.to_string(),
        handler_id: None,
        file: file.to_string(),
        line,
        rule: rule.to_string(),
        prov: "matched".to_string(),
        conf: "sure".to_string(),
        detail: None,
    }
}

/// True when `rel` is a file of the tree: one with facts, or one the resolver knows by
/// that exact path (every walked file, parsed or not).
pub(crate) fn is_known(project: &Project, rel: &str) -> bool {
    project.facts.contains_key(rel)
        || matches!(
            project.modules.resolve("", &format!("./{rel}")),
            ModuleTarget::File(found) if found == rel
        )
}

/// True when some file of the tree lies under folder `dir`, by the rule of [`is_known`].
pub(crate) fn has_known_under(project: &Project, dir: &str) -> bool {
    let prefix = format!("{dir}/");
    project
        .facts
        .range(prefix.clone()..)
        .next()
        .is_some_and(|(rel, _)| rel.starts_with(&prefix))
        || project.modules.has_file_under(dir)
}

/// The symbol ids of each file, computed once per file. `symbol_ids` and `source_id`
/// recompute the ids of the whole file on every call; the rules ask many times.
pub(crate) struct Ids<'a> {
    project: &'a Project<'a>,
    by_file: HashMap<String, Vec<String>>,
}

impl<'a> Ids<'a> {
    pub(crate) fn new(project: &'a Project<'a>) -> Ids<'a> {
        Ids {
            project,
            by_file: HashMap::new(),
        }
    }

    /// The id of symbol `index` of file `rel`.
    pub(crate) fn symbol(&mut self, rel: &str, index: usize) -> Option<String> {
        let facts = self.project.facts.get(rel)?;
        self.by_file
            .entry(rel.to_string())
            .or_insert_with(|| symbol_ids(rel, facts))
            .get(index)
            .cloned()
    }

    /// The symbol a fact at `line` with enclosing `owner` belongs to, by the rule of
    /// `resolve::source_id`; `None` where that gives the file id.
    pub(crate) fn holder(
        &mut self,
        rel: &str,
        owner: Option<&(String, String)>,
        line: u32,
    ) -> Option<String> {
        let facts = self.project.facts.get(rel)?;
        let index = holder(facts, owner, line)?;
        self.symbol(rel, index)
    }
}
