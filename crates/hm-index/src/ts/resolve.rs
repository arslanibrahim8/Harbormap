//! Resolves TypeScript and JavaScript import specifiers to files of the worktree,
//! bundler style: relative paths, `paths`, `baseUrl`, extension and index probing.
//! It also turns the facts of a file into edges and refs: imports, calls and
//! `extends`, resolved through imports and re-exports. A reference that does not
//! name exactly one target becomes a ref, never an edge.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hm_core::ids::{file_id, symbol_id};

use super::tsconfig::{TsConfig, TsConfigs, join_norm};
use crate::facts::{CallFact, FileFacts, ImportKind, SymbolFact, SymbolKind};

/// The extensions tried when a path has no exact match, in this order.
const EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs"];

/// Where a specifier points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleTarget {
    /// A file of the worktree, as a relative path.
    File(String),
    /// A package outside the worktree, by package name.
    External(String),
    /// Looks like it belongs to the worktree, but no file matches.
    Unresolved,
}

/// Resolves specifiers against the files of a worktree and its tsconfigs.
#[derive(Debug, Clone)]
pub struct ModuleResolver {
    known: BTreeSet<String>,
    tsconfigs: TsConfigs,
}

impl ModuleResolver {
    /// `known` is the set of relative file paths that exist in the worktree.
    pub fn new(known: BTreeSet<String>, tsconfigs: TsConfigs) -> Self {
        Self { known, tsconfigs }
    }

    /// Every tsconfig file read or attempted, extended ones and failed ones included.
    /// The resolver must be rebuilt when one of them changes.
    pub fn config_sources(&self) -> BTreeSet<String> {
        self.tsconfigs
            .by_dir
            .values()
            .flat_map(|c| c.sources.iter().cloned())
            .chain(self.tsconfigs.failed.iter().cloned())
            .collect()
    }

    /// Resolves `specifier` as imported from the file `from_rel`.
    pub fn resolve(&self, from_rel: &str, specifier: &str) -> ModuleTarget {
        let from_dir = from_rel.rsplit_once('/').map_or("", |(dir, _)| dir);

        // 1. Relative to the importing file. A relative specifier that matches
        // nothing is unresolved, never external.
        if is_relative(specifier) {
            return self.probe_or_unresolved(join_norm(from_dir, specifier));
        }
        // An absolute path points outside the worktree's own naming.
        if specifier.starts_with('/') {
            return ModuleTarget::Unresolved;
        }

        let config = self.nearest_config(from_dir);
        if let Some(config) = config {
            // 2. `paths`, in file order. A prefix the project claims is not a package.
            let mut claimed = false;
            for (pattern, targets) in &config.paths {
                let Some(captured) = match_pattern(pattern, specifier) else {
                    continue;
                };
                claimed = true;
                for target in targets {
                    let path = target.replacen('*', captured, 1);
                    if let Some(file) = self.probe(&path) {
                        return ModuleTarget::File(file);
                    }
                }
            }
            if claimed {
                return ModuleTarget::Unresolved;
            }
            // 3. `baseUrl`.
            if let Some(base) = &config.base_url
                && let Some(file) = join_norm(base, specifier).and_then(|p| self.probe(&p))
            {
                return ModuleTarget::File(file);
            }
        }

        // 4. A package.
        ModuleTarget::External(package_name(specifier).to_string())
    }

    /// The config of the nearest folder at or above `dir` that has one.
    fn nearest_config(&self, mut dir: &str) -> Option<&TsConfig> {
        loop {
            if let Some(config) = self.tsconfigs.by_dir.get(dir) {
                return Some(config);
            }
            if dir.is_empty() {
                return None;
            }
            dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    fn probe_or_unresolved(&self, path: Option<String>) -> ModuleTarget {
        match path.and_then(|p| self.probe(&p)) {
            Some(file) => ModuleTarget::File(file),
            None => ModuleTarget::Unresolved,
        }
    }

    /// Finds the file `path` names: exact, with an extension, or as a folder index.
    /// A `.js` path also tries the TypeScript file it is compiled from.
    fn probe(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return self.probe_index("");
        }
        if self.known.contains(path) {
            return Some(path.to_string());
        }
        if let Some(stem) = path.strip_suffix(".js") {
            for ext in [".ts", ".tsx"] {
                let candidate = format!("{stem}{ext}");
                if self.known.contains(&candidate) {
                    return Some(candidate);
                }
            }
        }
        for ext in EXTENSIONS {
            let candidate = format!("{path}{ext}");
            if self.known.contains(&candidate) {
                return Some(candidate);
            }
        }
        self.probe_index(path)
    }

    fn probe_index(&self, dir: &str) -> Option<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        EXTENSIONS
            .iter()
            .map(|ext| format!("{prefix}index{ext}"))
            .find(|candidate| self.known.contains(candidate))
    }
}

fn is_relative(specifier: &str) -> bool {
    specifier == "."
        || specifier == ".."
        || specifier.starts_with("./")
        || specifier.starts_with("../")
}

/// What the `*` of `pattern` captured in `specifier`; the empty string when the pattern
/// has no `*` and equals the specifier. `None` when they do not match.
fn match_pattern<'a>(pattern: &str, specifier: &'a str) -> Option<&'a str> {
    match pattern.split_once('*') {
        None => (pattern == specifier).then_some(""),
        Some((prefix, suffix)) => specifier.strip_prefix(prefix)?.strip_suffix(suffix),
    }
}

/// The package a bare specifier names: its first segment, or the first two when it
/// starts with `@`.
fn package_name(specifier: &str) -> &str {
    let mut end = specifier.len();
    let mut segments = 0;
    let wanted = if specifier.starts_with('@') { 2 } else { 1 };
    for (i, c) in specifier.char_indices() {
        if c == '/' {
            segments += 1;
            if segments == wanted {
                end = i;
                break;
            }
        }
    }
    &specifier[..end]
}

/// The files of a worktree with their facts, and the resolver for their specifiers.
pub struct Project<'a> {
    /// Relative path to the facts of that file.
    pub facts: &'a BTreeMap<String, FileFacts>,
    pub modules: &'a ModuleResolver,
}

/// An edge of the graph; every L1 edge is `Sure` and `parsed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRow {
    /// `imports`, `calls` or `extends`.
    pub kind: &'static str,
    pub src_id: String,
    pub dst_id: String,
    pub line: u32,
    pub rule: &'static str,
}

/// A reference that is not an edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefRow {
    /// `import`, `call` or `extends`.
    pub kind: &'static str,
    pub text: String,
    pub line: u32,
    /// `unresolved` or `external`.
    pub status: &'static str,
}

/// What resolving one file gives. Every list is sorted and holds no duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedFile {
    pub edges: Vec<EdgeRow>,
    pub refs: Vec<RefRow>,
    /// The files this one imports (of any import kind), as relative paths.
    pub imports_files: BTreeSet<String>,
}

/// How many re-exports are followed before a name counts as unresolved (spec §6.2).
const MAX_EXPORT_HOPS: u32 = 5;

/// One id per symbol of `facts`, in order. This is the only place symbol ids are made.
///
/// The disambiguator is empty when (owner, name, kind) is unique in the file, the
/// symbol's `sig_hash` otherwise, and `sig_hash#2`, `sig_hash#3` for the second and
/// later symbols that share the `sig_hash` too.
pub fn symbol_ids(rel: &str, facts: &FileFacts) -> Vec<String> {
    let file = file_id(rel);
    let key = |s: &SymbolFact| (s.owner.clone(), s.name.clone(), s.kind.as_str());
    let mut per_key: HashMap<_, u32> = HashMap::new();
    for s in &facts.symbols {
        *per_key.entry(key(s)).or_default() += 1;
    }
    let mut per_hash: HashMap<_, u32> = HashMap::new();
    facts
        .symbols
        .iter()
        .map(|s| {
            let disambiguator = if per_key[&key(s)] == 1 {
                String::new()
            } else {
                let seen = per_hash.entry((key(s), &s.sig_hash)).or_default();
                *seen += 1;
                match *seen {
                    1 => s.sig_hash.clone(),
                    n => format!("{}#{n}", s.sig_hash),
                }
            };
            symbol_id(&file, &s.owner, &s.name, s.kind.as_str(), &disambiguator)
        })
        .collect()
}

/// The top-level symbol of `rel` that `name` names when it is exported (or, with
/// `"default"`, the default export), following `export { a as b } from`,
/// `export * from` and `export default <ident>` for at most five hops. Returns the
/// file that declares it and the index into its `symbols`.
pub fn resolve_export(p: &Project, rel: &str, name: &str) -> Option<(String, usize)> {
    export_at(p, rel, name, 0)
}

fn export_at(p: &Project, rel: &str, name: &str, hops: u32) -> Option<(String, usize)> {
    if hops > MAX_EXPORT_HOPS || name == "*" {
        return None;
    }
    let facts = p.facts.get(rel)?;
    let found = |i: usize| Some((rel.to_string(), i));

    // An explicit export fact: `export { a as b }`, `export { a as b } from`,
    // `export default a`, and the fact every `export function a` gives. It comes first:
    // with `export { a as b }` only `b` is exported, although `a` is flagged.
    if let Some(e) = facts.exports.iter().find(|e| !e.star && e.name == name) {
        let local = e.local.as_deref().unwrap_or(name);
        if let Some(spec) = &e.from {
            return match p.modules.resolve(rel, spec) {
                ModuleTarget::File(target) => export_at(p, &target, local, hops + 1),
                _ => None,
            };
        }
        if let Some(i) = unique_top_level(facts, local, |_| true) {
            return found(i);
        }
        // `import { a } from "./x"; export { a }`.
        return match binding_of(p, rel, facts, local, true)? {
            Binding {
                target: ModuleTarget::File(target),
                imported,
                ..
            } => export_at(p, &target, &imported, hops + 1),
            _ => None,
        };
    }

    // A symbol exported by its declaration without a fact (CommonJS), unless an
    // `export { .. }` list only re-labels it.
    let relabelled = |s: &SymbolFact| {
        facts
            .exports
            .iter()
            .any(|e| !e.star && e.from.is_none() && e.local.as_deref() == Some(&s.name))
    };
    if name == "default" {
        if let Some(i) = facts
            .symbols
            .iter()
            .position(|s| s.default_export && !relabelled(s))
        {
            return found(i);
        }
    } else if let Some(i) = unique_top_level(facts, name, |s| s.exported && !relabelled(s)) {
        return found(i);
    }

    // `export * from` carries every name but the default; two stars that offer
    // different symbols make the name ambiguous.
    if name == "default" {
        return None;
    }
    let mut hit: Option<(String, usize)> = None;
    for e in facts.exports.iter().filter(|e| e.star && e.name == "*") {
        let Some(ModuleTarget::File(target)) = e.from.as_ref().map(|s| p.modules.resolve(rel, s))
        else {
            continue;
        };
        if let Some(found) = export_at(p, &target, name, hops + 1) {
            match &hit {
                Some(prev) if *prev != found => return None,
                _ => hit = Some(found),
            }
        }
    }
    hit
}

/// The index of the one top-level symbol called `name` that passes `keep`. A type
/// merged with a class or function of the same name (`interface Foo` + `class Foo`)
/// counts as the class or function.
fn unique_top_level(
    facts: &FileFacts,
    name: &str,
    keep: impl Fn(&SymbolFact) -> bool,
) -> Option<usize> {
    let all: Vec<usize> = facts
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.owner.is_empty() && s.name == name && keep(s))
        .map(|(i, _)| i)
        .collect();
    if let [only] = all[..] {
        return Some(only);
    }
    let values: Vec<usize> = all
        .into_iter()
        .filter(|&i| facts.symbols[i].kind != SymbolKind::Interface)
        .collect();
    match values[..] {
        [only] => Some(only),
        _ => None,
    }
}

/// A name that an import binds in a file.
struct Binding {
    target: ModuleTarget,
    /// `"default"`, `"*"` or the exported name.
    imported: String,
    via_require: bool,
}

/// The binding of the local name `local` in `facts`. Re-exports bind nothing, and a
/// type-only import binds only when `types` is set (to follow `export { T }`).
fn binding_of(
    p: &Project,
    rel: &str,
    facts: &FileFacts,
    local: &str,
    types: bool,
) -> Option<Binding> {
    facts
        .imports
        .iter()
        .filter(|i| i.kind != ImportKind::ReExport && (types || !i.type_only))
        .find_map(|i| {
            let n = i.names.iter().find(|n| n.local == local)?;
            Some(Binding {
                target: p.modules.resolve(rel, &i.specifier),
                imported: n.imported.clone(),
                via_require: i.kind == ImportKind::Require,
            })
        })
}

/// The index of the symbol that holds a fact at `line` with enclosing `owner`. The fact
/// gives `(owner, name)` only, so of several symbols with that pair (a getter and a
/// setter, a merged `interface` and `class`) the one that can hold calls and spans the
/// line is the one. Of two such, the smaller; when the smallest span is shared (a
/// one-line getter and setter) nothing says which one holds the fact, so it goes to
/// the file rather than to a guess.
fn holder(facts: &FileFacts, owner: Option<&(String, String)>, line: u32) -> Option<usize> {
    let (owner, name) = owner?;
    let holders: Vec<(usize, u32)> = facts
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.owner == *owner
                && s.name == *name
                && matches!(
                    s.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Class
                )
                && s.l0 <= line
                && line <= s.l1
        })
        .map(|(i, s)| (i, s.l1 - s.l0))
        .collect();
    let smallest = holders.iter().map(|&(_, span)| span).min();
    let mut tightest = holders.iter().filter(|&&(_, span)| Some(span) == smallest);
    match (tightest.next(), tightest.next()) {
        (Some(&(i, _)), None) => Some(i),
        _ => None,
    }
}

/// The id of the symbol that holds a fact of `facts` (a call, a route call, a client
/// call) at `line` with enclosing `owner`, or the file id at top level and when no one
/// symbol can be named. This is the one place that decides it.
pub fn source_id(
    rel: &str,
    facts: &FileFacts,
    owner: Option<&(String, String)>,
    line: u32,
) -> String {
    match holder(facts, owner, line) {
        Some(i) => symbol_ids(rel, facts).swap_remove(i),
        None => file_id(rel),
    }
}

/// The symbol that `receiver.callee` (or `callee`) names as seen from `rel`, outside any
/// symbol: `Some((file, index into its symbols))` only when the name resolves to exactly
/// one symbol. A member receiver such as `UsersService` finds a static method.
pub fn resolve_callee(
    p: &Project,
    rel: &str,
    receiver: Option<&str>,
    callee: &str,
) -> Option<(String, usize)> {
    let facts = p.facts.get(rel)?;
    let mut r = Resolver {
        p,
        rel,
        facts,
        file: file_id(rel),
        ids: HashMap::new(),
        edges: BTreeMap::new(),
        refs: BTreeSet::new(),
    };
    match r.resolve_name(receiver, callee, false, None, false) {
        Outcome::Edge { file, symbol, .. } => Some((file, symbol)),
        Outcome::Ref { .. } => None,
    }
}

/// `(package, imported name)` when `local` is bound in `rel` by an import (or `require`)
/// of an external package; `imported` is `"default"`, `"*"` or the exported name.
pub fn binding_package(p: &Project, rel: &str, local: &str) -> Option<(String, String)> {
    let facts = p.facts.get(rel)?;
    let binding = binding_of(p, rel, facts, local, false)?;
    match binding.target {
        ModuleTarget::External(package) => Some((package, binding.imported)),
        _ => None,
    }
}

/// What a call or `extends` resolved to.
enum Outcome {
    Edge {
        file: String,
        symbol: usize,
        rule: &'static str,
    },
    Ref {
        status: &'static str,
    },
}

/// Resolves one file's facts to edges and refs. Anything the facts do not pin to a
/// single symbol is a ref.
pub fn resolve_file(p: &Project, rel: &str) -> ResolvedFile {
    let Some(facts) = p.facts.get(rel) else {
        return ResolvedFile::default();
    };
    let mut r = Resolver {
        p,
        rel,
        facts,
        file: file_id(rel),
        ids: HashMap::new(),
        edges: BTreeMap::new(),
        refs: BTreeSet::new(),
    };
    let mut imports_files = BTreeSet::new();
    r.ids.insert(rel.to_string(), symbol_ids(rel, facts));

    for i in &facts.imports {
        match p.modules.resolve(rel, &i.specifier) {
            ModuleTarget::File(target) => {
                r.edge(
                    "imports",
                    r.file.clone(),
                    file_id(&target),
                    i.line,
                    "ts.import",
                );
                imports_files.insert(target);
            }
            ModuleTarget::External(pkg) => r.reference("import", pkg, i.line, "external"),
            ModuleTarget::Unresolved => {
                r.reference("import", i.specifier.clone(), i.line, "unresolved");
            }
        }
    }

    for c in &facts.calls {
        let src = r.call_source(c);
        let outcome = r.resolve_name(
            c.receiver.as_deref(),
            &c.callee,
            c.jsx,
            c.owner.as_ref(),
            false,
        );
        match outcome {
            Outcome::Edge { file, symbol, rule } => {
                let dst = r.symbol_id_of(&file, symbol);
                r.edge("calls", src, dst, c.line, rule);
            }
            Outcome::Ref { status } => r.reference("call", call_text(c), c.line, status),
        }
    }

    for h in &facts.heritage {
        // No top-level class spans the line (`const K = class extends B {}`): there
        // is no source symbol, so the heritage stays a ref rather than a file edge.
        let Some(src) = r.class_id(&h.class, h.line) else {
            r.reference("extends", h.extends.clone(), h.line, "unresolved");
            continue;
        };
        let (receiver, callee) = split_path(&h.extends);
        match r.resolve_name(receiver, callee, false, None, true) {
            Outcome::Edge { file, symbol, .. } => {
                let dst = r.symbol_id_of(&file, symbol);
                r.edge("extends", src, dst, h.line, "ts.extends");
            }
            Outcome::Ref { status } => r.reference("extends", h.extends.clone(), h.line, status),
        }
    }

    ResolvedFile {
        edges: r
            .edges
            .into_iter()
            .map(|((kind, src_id, dst_id, line), rule)| EdgeRow {
                kind,
                src_id,
                dst_id,
                line,
                rule,
            })
            .collect(),
        refs: r
            .refs
            .into_iter()
            .map(|(kind, text, line, status)| RefRow {
                kind,
                text,
                line,
                status,
            })
            .collect(),
        imports_files,
    }
}

/// `receiver.callee`, or `callee` without a receiver.
fn call_text(c: &CallFact) -> String {
    match &c.receiver {
        Some(r) => format!("{r}.{}", c.callee),
        None => c.callee.clone(),
    }
}

/// Splits `ns.Base` into (`ns`, `Base`); a name without a dot has no receiver.
fn split_path(path: &str) -> (Option<&str>, &str) {
    match path.rsplit_once('.') {
        Some((receiver, name)) => (Some(receiver), name),
        None => (None, path),
    }
}

/// The leading identifier of an expression: `z` for `z.object({})`.
fn root_ident(expr: &str) -> &str {
    let end = expr
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(expr.len());
    &expr[..end]
}

fn is_ident(text: &str) -> bool {
    !text.is_empty() && root_ident(text) == text
}

/// A symbol that a call or `extends` can name.
fn callable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Class | SymbolKind::Variable
    )
}

struct Resolver<'a, 'p> {
    p: &'a Project<'p>,
    rel: &'a str,
    facts: &'a FileFacts,
    file: String,
    /// Symbol ids by file, filled when a file is first needed.
    ids: HashMap<String, Vec<String>>,
    edges: BTreeMap<(&'static str, String, String, u32), &'static str>,
    refs: BTreeSet<(&'static str, String, u32, &'static str)>,
}

impl Resolver<'_, '_> {
    /// Keeps the first rule of two edges that agree on kind, source, target and line.
    fn edge(
        &mut self,
        kind: &'static str,
        src: String,
        dst: String,
        line: u32,
        rule: &'static str,
    ) {
        self.edges.entry((kind, src, dst, line)).or_insert(rule);
    }

    fn reference(&mut self, kind: &'static str, text: String, line: u32, status: &'static str) {
        self.refs.insert((kind, text, line, status));
    }

    fn symbol_id_of(&mut self, rel: &str, index: usize) -> String {
        if !self.ids.contains_key(rel) {
            let ids = self
                .p
                .facts
                .get(rel)
                .map(|f| symbol_ids(rel, f))
                .unwrap_or_default();
            self.ids.insert(rel.to_string(), ids);
        }
        self.ids[rel][index].clone()
    }

    /// The id of the symbol that holds a call, or the file id at top level; see
    /// `source_id`.
    fn call_source(&mut self, c: &CallFact) -> String {
        match holder(self.facts, c.owner.as_ref(), c.line) {
            Some(i) => self.symbol_id_of(self.rel, i),
            None => self.file.clone(),
        }
    }

    /// The id of the class `name` of this file that spans `line`, if there is one.
    fn class_id(&mut self, name: &str, line: u32) -> Option<String> {
        let class = self.facts.symbols.iter().position(|s| {
            s.kind == SymbolKind::Class
                && s.owner.is_empty()
                && s.name == name
                && s.l0 <= line
                && line <= s.l1
        });
        class.map(|i| self.symbol_id_of(self.rel, i))
    }

    /// Resolves `receiver.callee` (or `callee`) as seen from this file. `owner` is the
    /// enclosing symbol of a call, for `this`. With `heritage` set the name is the
    /// operand of `extends`, so a class's static members do not apply.
    fn resolve_name(
        &mut self,
        receiver: Option<&str>,
        callee: &str,
        jsx: bool,
        owner: Option<&(String, String)>,
        heritage: bool,
    ) -> Outcome {
        const UNRESOLVED: Outcome = Outcome::Ref {
            status: "unresolved",
        };
        let (p, rel, facts) = (self.p, self.rel, self.facts);
        let edge = |file: &str, symbol: usize, rule| Outcome::Edge {
            file: file.to_string(),
            symbol,
            rule,
        };
        // The rule of a call to a symbol: JSX, `new` of a class or the given one.
        let rule_for = |file: &str, symbol: usize, rule: &'static str| {
            let kind = p.facts[file].symbols[symbol].kind;
            if jsx {
                "ts.jsx"
            } else if kind == SymbolKind::Class {
                "ts.call.new"
            } else {
                rule
            }
        };
        // The symbol an exported name of another file names, if a call can name it.
        let exported = |target: &str, name: &str| {
            let (file, symbol) = resolve_export(p, target, name)?;
            callable(p.facts[&file].symbols[symbol].kind).then_some((file, symbol))
        };
        let method_of = |file: &str, class: &str, name: &str| {
            let methods: Vec<usize> = p.facts[file]
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.kind == SymbolKind::Method && s.owner == class && s.name == name)
                .map(|(i, _)| i)
                .collect();
            match methods[..] {
                [only] => Some(only),
                _ => None,
            }
        };

        let Some(receiver) = receiver else {
            // `f()`, `new C()`, `<C />`, `class A extends B`.
            if facts
                .symbols
                .iter()
                .any(|s| s.owner.is_empty() && s.name == callee)
            {
                return match unique_top_level(facts, callee, |_| true) {
                    Some(i) if callable(facts.symbols[i].kind) => {
                        edge(rel, i, rule_for(rel, i, "ts.call.local"))
                    }
                    _ => UNRESOLVED,
                };
            }
            let Some(binding) = binding_of(p, rel, facts, callee, false) else {
                return UNRESOLVED;
            };
            return match binding.target {
                ModuleTarget::File(target) if binding.imported != "*" => {
                    match exported(&target, &binding.imported) {
                        Some((file, symbol)) => {
                            let rule = if binding.via_require {
                                "ts.call.require"
                            } else {
                                "ts.call.import"
                            };
                            let rule = rule_for(&file, symbol, rule);
                            edge(&file, symbol, rule)
                        }
                        None => UNRESOLVED,
                    }
                }
                ModuleTarget::External(_) => Outcome::Ref { status: "external" },
                _ => UNRESOLVED,
            };
        };

        if receiver == "this" {
            let class = owner
                .map(|(class, _)| class.as_str())
                .filter(|c| !c.is_empty());
            return match class.and_then(|c| method_of(rel, c, callee)) {
                Some(i) if !heritage => edge(rel, i, "ts.call.this"),
                _ => UNRESOLVED,
            };
        }

        let root = root_ident(receiver);
        if !is_ident(receiver) {
            // `z.object({}).parse()`: only the root says where it came from.
            return match binding_of(p, rel, facts, root, false) {
                Some(Binding {
                    target: ModuleTarget::External(_),
                    ..
                }) if !root.is_empty() => Outcome::Ref { status: "external" },
                _ => UNRESOLVED,
            };
        }

        // `C.m()` on a class of this file.
        if facts
            .symbols
            .iter()
            .any(|s| s.owner.is_empty() && s.name == receiver)
        {
            if heritage {
                return UNRESOLVED;
            }
            let class = unique_top_level(facts, receiver, |_| true)
                .filter(|&i| facts.symbols[i].kind == SymbolKind::Class);
            return match class.and_then(|_| method_of(rel, receiver, callee)) {
                Some(i) => edge(rel, i, "ts.call.static"),
                None => UNRESOLVED,
            };
        }

        let Some(binding) = binding_of(p, rel, facts, receiver, false) else {
            return UNRESOLVED;
        };
        match binding.target {
            ModuleTarget::File(target) if binding.imported == "*" => {
                match exported(&target, callee) {
                    Some((file, symbol)) => {
                        let rule = if jsx { "ts.jsx" } else { "ts.call.namespace" };
                        edge(&file, symbol, rule)
                    }
                    None => UNRESOLVED,
                }
            }
            // `C.m()` on an imported class.
            ModuleTarget::File(target) if !heritage => {
                let class = resolve_export(p, &target, &binding.imported)
                    .filter(|(file, i)| p.facts[file].symbols[*i].kind == SymbolKind::Class);
                match class {
                    Some((file, i)) => {
                        let class_name = &p.facts[&file].symbols[i].name;
                        match method_of(&file, class_name, callee) {
                            Some(m) => edge(&file, m, "ts.call.static"),
                            None => UNRESOLVED,
                        }
                    }
                    None => UNRESOLVED,
                }
            }
            ModuleTarget::External(_) => Outcome::Ref { status: "external" },
            _ => UNRESOLVED,
        }
    }
}
